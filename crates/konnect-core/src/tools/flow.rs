//! `flow` toolset — the orchestration state of one KiCad project, kept in
//! `<project>/.konnect/flow/` and written only by the `flow_*` tools
//! (design D1/D2 of the `konnect-orchestrator` change).
//!
//! Three properties are load-bearing and live here rather than in the
//! callers' discipline:
//!
//! - **`STATE.md` round-trips.** Its front matter is pretty-printed JSON
//!   between `---` fences (no YAML crate exists in the workspace, and a
//!   hand-rolled YAML writer is exactly the lossy round trip this file must
//!   not have). JSON escapes newlines inside strings, so no front-matter line
//!   can ever equal `---`. The body below the front matter is regenerated
//!   from the struct on every write and never parsed.
//! - **Nothing below `project_dir` is a caller path.** Every file name comes
//!   from an enum, a server-computed number or the validated `job_id`, and
//!   every directory the toolset touches is re-canonicalized and confined to
//!   the canonical project.
//! - **A gate binds to content, not to time.** `design_hash` is
//!   [`design_state_hash`](crate::design_hash::design_state_hash); the package
//!   hash ([`package_hash`]) covers the records a human was shown, because the
//!   design hash excludes `.konnect/` by construction.
//!
//! No tool here touches a live KiCad board, so every one keeps the default
//! [`BoardAccess::None`](crate::tools::BoardAccess).

use crate::design_hash::design_state_hash;
use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::observability::{CallRecord, CallStatus};
use crate::tool;
use crate::tools::{invalid_arg, opt_str_list, require_str, ToolContext, ToolDef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ─── Tokens (design D3, D4, D7) ───────────────────────────────────────────────

/// The canonical phase order (D3). Position in this array is the only order
/// the validator knows: a lane is a strictly increasing subsequence of it.
pub(crate) const CANONICAL_PHASES: [&str; 12] = [
    "requirements",
    "architecture",
    "gate:architecture",
    "schematic",
    "schematic_review",
    "placement",
    "gate:placement",
    "routing",
    "prefab_review",
    "manufacturing",
    "gate:purchase",
    "learn",
];

/// The terminal pseudo-phase: `flow_advance(to_phase: "closed")`. Not part of
/// any job's `phases`.
pub(crate) const CLOSED: &str = "closed";

/// A gated phase and the gate it brings (D3): a lane subset that includes the
/// phase without its gate would silently drop a human approval, and one that
/// includes the gate without its phase would bind the approval to a record
/// no phase of this job produced.
const GATED_PHASES: [(&str, &str); 3] = [
    ("architecture", "gate:architecture"),
    ("placement", "gate:placement"),
    ("manufacturing", "gate:purchase"),
];

/// D4: the records a work phase must supply, in the same `flow_advance` call,
/// when it is left forward. Gates and `learn` supply none.
const PHASE_RECORDS: [(&str, &[&str]); 8] = [
    ("requirements", &["constraints.md"]),
    (
        "architecture",
        &["architecture.md", "worst-case.md", "pin-plan.md"],
    ),
    ("schematic", &["schematic-evidence.md"]),
    ("schematic_review", &["ledger-schematic.md"]),
    ("placement", &["placement.md"]),
    ("routing", &["routing.md"]),
    ("prefab_review", &["ledger-prefab.md"]),
    ("manufacturing", &["manufacturing.md"]),
];

/// The ten record names, in phase order — the `records[].filename` enum.
/// `the_vocabularies_agree_with_each_other` pins it to [`PHASE_RECORDS`].
pub(crate) const RECORD_NAMES: [&str; 10] = [
    "constraints.md",
    "architecture.md",
    "worst-case.md",
    "pin-plan.md",
    "schematic-evidence.md",
    "ledger-schematic.md",
    "placement.md",
    "routing.md",
    "ledger-prefab.md",
    "manufacturing.md",
];

/// The record whose last line D5 parses.
pub(crate) const ARCHITECTURE_RECORD: &str = "architecture.md";

/// D7's role slugs: the `role` enum of `flow_log` and the `<role>` of
/// `memory/<role>.md` and `handoffs/<NN>-<role>.md`.
pub(crate) const ROLES: [&str; 12] = [
    "requirements",
    "architecture",
    "sourcing",
    "schematic",
    "library",
    "layout",
    "review",
    "manufacture",
    "photo-intake",
    "design-reconstruction",
    "curator",
    "orchestrator",
];

/// `flow_gate`'s `gate_name` values; `gate:<name>` is the phase token.
pub(crate) const GATE_NAMES: [&str; 3] = ["architecture", "placement", "purchase"];

/// The one front-matter schema this build reads and writes.
pub(crate) const STATE_SCHEMA: u32 = 1;

/// The state file's name inside the flow directory.
pub(crate) const STATE_FILE: &str = "STATE.md";

const FENCE: &str = "---";

fn canonical_index(token: &str) -> Option<usize> {
    CANONICAL_PHASES.iter().position(|phase| *phase == token)
}

/// A canonical gate token (`gate:architecture`, `gate:placement`,
/// `gate:purchase`).
pub(crate) fn is_gate(token: &str) -> bool {
    token.starts_with("gate:") && canonical_index(token).is_some()
}

/// The records `phase` must supply when left forward (D4); empty for gates,
/// `learn` and anything unknown.
pub(crate) fn required_records(phase: &str) -> &'static [&'static str] {
    PHASE_RECORDS
        .iter()
        .find(|(owner, _)| *owner == phase)
        .map_or(&[], |(_, records)| records)
}

/// The phase that owns a record name (D4's one writer per record).
pub(crate) fn record_phase(filename: &str) -> Option<&'static str> {
    PHASE_RECORDS
        .iter()
        .find(|(_, records)| records.contains(&filename))
        .map(|(phase, _)| *phase)
}

// ─── Lane, mode and the other closed vocabularies ─────────────────────────────

/// `flow_start`'s `lane` (D3). Serialized snake_case; [`Lane::as_str`] is the
/// same spelling for messages and parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Lane {
    NewBoard,
    BoardRevision,
    ReviewOnly,
    FabOnly,
    PhotoToKicad,
}

impl Lane {
    pub(crate) const ALL: [Lane; 5] = [
        Lane::NewBoard,
        Lane::BoardRevision,
        Lane::ReviewOnly,
        Lane::FabOnly,
        Lane::PhotoToKicad,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Lane::NewBoard => "new_board",
            Lane::BoardRevision => "board_revision",
            Lane::ReviewOnly => "review_only",
            Lane::FabOnly => "fab_only",
            Lane::PhotoToKicad => "photo_to_kicad",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Lane> {
        Lane::ALL.into_iter().find(|lane| lane.as_str() == raw)
    }
}

/// `flow_start`'s `mode` (D3 "autonomous mode"); `guided` is the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    Guided,
    Autonomous,
}

impl Mode {
    pub(crate) const ALL: [Mode; 2] = [Mode::Guided, Mode::Autonomous];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Mode::Guided => "guided",
            Mode::Autonomous => "autonomous",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|mode| mode.as_str() == raw)
    }
}

/// What a history entry records. `advance` is a forward move into a phase,
/// `close` the forward move out of the last one; `rewind` and `abandon` are
/// D3's backward and early-terminal transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HistoryKind {
    Start,
    Advance,
    Rewind,
    Abandon,
    Close,
}

impl HistoryKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            HistoryKind::Start => "start",
            HistoryKind::Advance => "advance",
            HistoryKind::Rewind => "rewind",
            HistoryKind::Abandon => "abandon",
            HistoryKind::Close => "close",
        }
    }
}

/// `flow_gate`'s `decision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GateDecision {
    Approve,
    Reject,
}

impl GateDecision {
    pub(crate) const ALL: [GateDecision; 2] = [GateDecision::Approve, GateDecision::Reject];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            GateDecision::Approve => "approve",
            GateDecision::Reject => "reject",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<GateDecision> {
        GateDecision::ALL
            .into_iter()
            .find(|decision| decision.as_str() == raw)
    }
}

/// Who granted an approval: the user's own words, or the session under
/// autonomous mode (D3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApprovedBy {
    User,
    Session,
}

impl ApprovedBy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ApprovedBy::User => "user",
            ApprovedBy::Session => "session",
        }
    }
}

// ─── STATE.md structs (design D2) ─────────────────────────────────────────────
//
// `deny_unknown_fields` everywhere: a field written by a newer binary is
// refused, never silently dropped by an older binary's rewrite. Optional
// fields skip serialization when empty, so absent stays absent.

/// The front matter of `STATE.md`: one job, its sequence, where it stands,
/// and everything recorded about it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobState {
    pub schema: u32,
    pub job_id: String,
    pub objective: String,
    pub lane: Lane,
    pub mode: Mode,
    /// The job's validated subsequence of [`CANONICAL_PHASES`].
    pub phases: Vec<String>,
    /// An entry of `phases`, or [`CLOSED`].
    pub phase: String,
    pub started_at: String,
    /// Keyed by gate name (`architecture`, `placement`, `purchase`).
    #[serde(default)]
    pub gate_approvals: BTreeMap<String, GateApproval>,
    #[serde(default)]
    pub pending_approvals: Vec<DeferredItem>,
    #[serde(default)]
    pub deferred_findings: Vec<DeferredItem>,
    #[serde(default)]
    pub queue: Vec<DeferredItem>,
    /// Append-only. An entry's index is its identity: a gate approval's
    /// `visit` is the index of the entry that entered the gate phase.
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

/// One transition. The entry that **enters** a gate phase carries the keys
/// the gate's approval is later compared against (D11): `design_hash` and
/// `package_hash`, plus `package_files` (per-record digest, `null` = absent)
/// so a refusal can name what changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HistoryEntry {
    pub kind: HistoryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub to: String,
    pub at: String,
    pub design_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_hash: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub package_files: BTreeMap<String, Option<String>>,
    /// Record names supplied in this call (forward transitions only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_calls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_check: Option<EvidenceCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HistoryEntry {
    pub(crate) fn new(
        kind: HistoryKind,
        from: Option<&str>,
        to: &str,
        at: &str,
        design_hash: &str,
    ) -> Self {
        HistoryEntry {
            kind,
            from: from.map(str::to_string),
            to: to.to_string(),
            at: at.to_string(),
            design_hash: design_hash.to_string(),
            package_hash: None,
            package_files: BTreeMap::new(),
            records: Vec::new(),
            evidence_calls: Vec::new(),
            evidence_check: None,
            reason: None,
        }
    }
}

/// D6: the cited evidence calls against the observer ring at transition time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceCheck {
    pub confirmed: Vec<String>,
    pub not_ok: Vec<String>,
    pub absent: Vec<String>,
    pub ring_calls: usize,
}

/// A recorded gate approval (D1 `flow_gate`, D11). `visit` is the history
/// index of the entry into the gate phase it was granted during.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateApproval {
    pub decision: GateDecision,
    pub approved_by: ApprovedBy,
    pub approved_at: String,
    pub design_hash_at_approval: String,
    pub package_hash_at_approval: String,
    pub visit: usize,
    pub summary: String,
    pub user_words: String,
}

/// An entry of `pending_approvals`, `deferred_findings` or `queue`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeferredItem {
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub added_at: String,
    pub phase: String,
}

// ─── The D3 sequence validator ────────────────────────────────────────────────

/// D3, generic over lanes (no per-lane arm): non-empty; every token known;
/// strictly increasing in canonical order; a gate is never first; a gated
/// phase brings its gate, and a gate brings its phase (Fix round 1, DECISION
/// A). Every error quotes the offending entry.
pub(crate) fn validate_phases(phases: &[String]) -> Result<(), String> {
    if phases.is_empty() {
        return Err("phases must name at least one phase".to_string());
    }
    let mut previous: Option<(usize, &str)> = None;
    for (position, token) in phases.iter().enumerate() {
        let Some(index) = canonical_index(token) else {
            return Err(format!(
                "phases[{position}] {token:?} is not a phase token; expected entries of: {}",
                CANONICAL_PHASES.join(", ")
            ));
        };
        if position == 0 && is_gate(token) {
            return Err(format!(
                "phases[0] {token:?} is a gate; a job never starts at a gate"
            ));
        }
        if let Some((previous_index, previous_token)) = previous {
            if index == previous_index {
                return Err(format!(
                    "phases[{position}] {token:?} repeats the entry before it; each phase \
                     appears at most once"
                ));
            }
            if index < previous_index {
                return Err(format!(
                    "phases[{position}] {token:?} comes before {previous_token:?} in the \
                     canonical order; phases must follow it: {}",
                    CANONICAL_PHASES.join(" → ")
                ));
            }
        }
        previous = Some((index, token));
    }
    for (phase, gate) in GATED_PHASES {
        if phases.iter().any(|entry| entry == phase) && !phases.iter().any(|entry| entry == gate) {
            return Err(format!(
                "phases includes {phase:?} without its gate {gate:?}; a lane subset cannot drop \
                 a human gate"
            ));
        }
    }
    for (phase, gate) in GATED_PHASES {
        if phases.iter().any(|entry| entry == gate) && !phases.iter().any(|entry| entry == phase) {
            return Err(format!(
                "phases includes {gate:?} without its phase {phase:?}; a gate's package must \
                 come from this job, not from a record an earlier job left on disk"
            ));
        }
    }
    Ok(())
}

// ─── STATE.md render / parse ──────────────────────────────────────────────────

/// Render the whole file: JSON front matter, then a body regenerated from the
/// same struct. `parse_state(&render_state(s)) == s` for every `s`.
pub(crate) fn render_state(state: &JobState) -> String {
    // Infallible for this type: every map key is a String and no field has a
    // custom serializer that can fail.
    let front = serde_json::to_string_pretty(state).unwrap_or_default();
    format!("{FENCE}\n{front}\n{FENCE}\n{}", render_body(state))
}

/// Parse `STATE.md`. CRLF-tolerant (git autocrlf) and BOM-tolerant (editors);
/// the body is never read. A front matter that does not parse or does not
/// validate is an error — hand edits are refused, never guessed at.
pub(crate) fn parse_state(text: &str) -> Result<JobState, String> {
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| format!("{STATE_FILE} does not open with a `{FENCE}` fence"))?;
    let end = closing_fence(rest)
        .ok_or_else(|| format!("{STATE_FILE} has no closing `{FENCE}` fence"))?;
    let state: JobState = serde_json::from_str(&rest[..end])
        .map_err(|error| format!("{STATE_FILE} front matter does not parse: {error}"))?;
    validate_state(&state)?;
    Ok(state)
}

/// Offset of the first line that is exactly the fence.
fn closing_fence(rest: &str) -> Option<usize> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches('\n') == FENCE {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Semantic checks serde cannot express. `job_id` and `started_at` become
/// path components (the log file, the handoff directory), so a hand edit that
/// makes them unsafe is refused here, before any path is built from them.
fn validate_state(state: &JobState) -> Result<(), String> {
    if state.schema != STATE_SCHEMA {
        return Err(format!(
            "{STATE_FILE} schema {} is not supported by this build (it reads schema \
             {STATE_SCHEMA})",
            state.schema
        ));
    }
    validate_job_id(&state.job_id)
        .map_err(|reason| format!("{STATE_FILE} job_id {:?} {reason}", state.job_id))?;
    if !is_utc_timestamp(&state.started_at) {
        return Err(format!(
            "{STATE_FILE} started_at {:?} is not a UTC timestamp like 2026-09-21T14:00:00Z",
            state.started_at
        ));
    }
    validate_phases(&state.phases).map_err(|error| format!("{STATE_FILE}: {error}"))?;
    if state.phase != CLOSED && !state.phases.contains(&state.phase) {
        return Err(format!(
            "{STATE_FILE} phase {:?} is neither an entry of the job's phases nor {CLOSED:?}",
            state.phase
        ));
    }
    if let Some(gate) = state
        .gate_approvals
        .keys()
        .find(|gate| !GATE_NAMES.contains(&gate.as_str()))
    {
        return Err(format!(
            "{STATE_FILE} gate_approvals key {gate:?} is not a gate name ({})",
            GATE_NAMES.join(", ")
        ));
    }
    Ok(())
}

/// `^[a-z0-9][a-z0-9-]*$` — the shape `flow_start` mints (D2).
pub(crate) fn validate_job_id(job_id: &str) -> Result<(), &'static str> {
    let mut characters = job_id.chars();
    let starts_well = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit());
    let rest_ok = job_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if starts_well && rest_ok {
        Ok(())
    } else {
        Err("must match ^[a-z0-9][a-z0-9-]*$")
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ`, digits where digits belong — the shape
/// `now_rfc3339_utc` produces.
fn is_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
}

/// The next step from the current phase: what leaving it needs and where it
/// goes. `None` once the job is closed.
pub(crate) struct NextStep {
    pub phase: String,
    pub required_records: &'static [&'static str],
    pub next_phase: String,
    pub is_gate: bool,
}

pub(crate) fn next_step(state: &JobState) -> Option<NextStep> {
    let position = state
        .phases
        .iter()
        .position(|phase| *phase == state.phase)?;
    let next_phase = state
        .phases
        .get(position + 1)
        .cloned()
        .unwrap_or_else(|| CLOSED.to_string());
    Some(NextStep {
        phase: state.phase.clone(),
        required_records: required_records(&state.phase),
        next_phase,
        is_gate: is_gate(&state.phase),
    })
}

/// The Markdown body. Never parsed; single-line renderings of free text (via
/// JSON string escaping) keep it readable whatever the objective contains.
fn render_body(state: &JobState) -> String {
    let mut body = String::new();
    body.push_str(&format!("# Flow state — {}\n\n", state.job_id));
    body.push_str(
        "> Written by the Konnect `flow` toolset. This body is regenerated from the front \
         matter on every write: edits below the front matter are discarded, and a front \
         matter that no longer parses is refused until fixed, never guessed at.\n\n",
    );
    body.push_str("## Job\n\n");
    body.push_str(&format!("- Objective: {}\n", one_line(&state.objective)));
    body.push_str(&format!(
        "- Lane: `{}` · mode: `{}` · started: {}\n",
        state.lane.as_str(),
        state.mode.as_str(),
        state.started_at
    ));
    body.push_str(&format!("- Phases: {}\n\n", state.phases.join(" → ")));

    body.push_str("## Phase and next step\n\n");
    body.push_str(&format!("- Phase: `{}`\n", state.phase));
    match next_step(state) {
        Some(step) if step.is_gate => body.push_str(&format!(
            "- Next: an approval of `{}` recorded during this visit, then `{}`\n",
            step.phase, step.next_phase
        )),
        Some(step) if step.required_records.is_empty() => {
            body.push_str(&format!("- Next: `{}`\n", step.next_phase))
        }
        Some(step) => body.push_str(&format!(
            "- Next: `{}`, supplying {}\n",
            step.next_phase,
            step.required_records.join(", ")
        )),
        None => body.push_str("- The job is closed.\n"),
    }
    body.push('\n');

    body.push_str("## Gates\n\n");
    if state.gate_approvals.is_empty() {
        body.push_str("(no approvals recorded)\n");
    }
    for (gate, approval) in &state.gate_approvals {
        body.push_str(&format!(
            "- `{gate}`: approved by {} at {} (visit {}), design {}, package {}\n",
            match approval.approved_by {
                ApprovedBy::User => "the user",
                ApprovedBy::Session => "the session",
            },
            approval.approved_at,
            approval.visit,
            short_hash(&approval.design_hash_at_approval),
            short_hash(&approval.package_hash_at_approval)
        ));
    }
    body.push_str(
        "\nValidity is recomputed by `flow_status` only for the gate the job stands at now; \
         this body shows what was recorded.\n\n",
    );

    for (title, items) in [
        ("Pending approvals", &state.pending_approvals),
        ("Deferred findings", &state.deferred_findings),
        ("Queue", &state.queue),
    ] {
        body.push_str(&format!("## {title}\n\n"));
        if items.is_empty() {
            body.push_str("(none)\n");
        }
        for item in items {
            body.push_str(&format!(
                "- {} ({}, {}{})\n",
                one_line(&item.description),
                item.phase,
                item.added_at,
                item.owner
                    .as_deref()
                    .map(|owner| format!(", owner {}", one_line(owner)))
                    .unwrap_or_default()
            ));
        }
        body.push('\n');
    }

    body.push_str(
        "## History\n\n| # | kind | from | to | at | records |\n|---|---|---|---|---|---|\n",
    );
    for (index, entry) in state.history.iter().enumerate() {
        body.push_str(&format!(
            "| {index} | {} | {} | {} | {} | {} |\n",
            entry.kind.as_str(),
            entry.from.as_deref().unwrap_or(""),
            entry.to,
            entry.at,
            entry.records.join(", ")
        ));
    }
    body
}

/// Free text as one JSON-escaped line: newlines, quotes and backticks can
/// neither break the layout nor forge a heading.
fn one_line(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

// ─── project_dir and the flow directory (design D2) ───────────────────────────

/// Canonicalize `project_dir` and require a `*.kicad_pro` directly inside it.
/// The top-level rule also stops a caller from passing a parent folder and
/// hashing a whole document tree on every call.
pub(crate) fn resolve_project_dir(raw: &str) -> Result<PathBuf, String> {
    if raw.trim().is_empty() {
        return Err("'project_dir' is empty".to_string());
    }
    let path = Path::new(raw);
    let canonical = path.canonicalize().map_err(|error| {
        format!(
            "'project_dir' does not resolve: {} ({error})",
            path.display()
        )
    })?;
    if !canonical.is_dir() {
        return Err(format!(
            "'project_dir' is not a directory: {}",
            path.display()
        ));
    }
    let entries = std::fs::read_dir(&canonical).map_err(|error| {
        format!(
            "'project_dir' cannot be listed: {} ({error})",
            path.display()
        )
    })?;
    let has_project = entries.flatten().any(|entry| {
        let entry_path = entry.path();
        entry_path.is_file()
            && entry_path
                .extension()
                .is_some_and(|extension| extension == "kicad_pro")
    });
    if !has_project {
        return Err(format!(
            "'project_dir' {} holds no top-level *.kicad_pro file; pass the KiCad project \
             directory itself. Nothing was written.",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn flow_dir_path(project: &Path) -> PathBuf {
    project.join(".konnect").join("flow")
}

/// Refuse a resolved directory that escaped the project (a `.konnect` or a
/// subdirectory symlinked elsewhere) — `prepare_map_dir`'s re-check,
/// re-implemented privately as the repo's convention is.
fn confine(canonical: &Path, root: &Path, shown: &Path) -> Result<(), String> {
    if canonical.starts_with(root) {
        Ok(())
    } else {
        Err(format!(
            "Refusing to use {}: it resolves to {}, which is not under {}",
            shown.display(),
            canonical.display(),
            root.display()
        ))
    }
}

/// The existing flow directory, canonical and confined; `None` when there is
/// none. Creates nothing.
pub(crate) fn existing_flow_dir(project: &Path) -> Result<Option<PathBuf>, String> {
    let dir = flow_dir_path(project);
    let canonical = match dir.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not resolve {} ({error})", dir.display())),
    };
    confine(&canonical, project, &dir)?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a directory", canonical.display()));
    }
    Ok(Some(canonical))
}

/// Create the flow directory. Only `flow_start` calls this, and only after
/// every validation passed.
pub(crate) fn create_flow_dir(project: &Path) -> Result<PathBuf, String> {
    let dir = flow_dir_path(project);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Could not create {} ({error})", dir.display()))?;
    let canonical = dir
        .canonicalize()
        .map_err(|error| format!("Could not resolve {} ({error})", dir.display()))?;
    confine(&canonical, project, &dir)?;
    Ok(canonical)
}

/// A subdirectory of the flow directory, created on demand and confined.
/// Every component is a constant or the validated `job_id`.
pub(crate) fn ensure_flow_subdir(flow_dir: &Path, components: &[&str]) -> Result<PathBuf, String> {
    let dir = components
        .iter()
        .fold(flow_dir.to_path_buf(), |path, component| {
            path.join(component)
        });
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Could not create {} ({error})", dir.display()))?;
    let canonical = dir
        .canonicalize()
        .map_err(|error| format!("Could not resolve {} ({error})", dir.display()))?;
    confine(&canonical, flow_dir, &dir)?;
    Ok(canonical)
}

// ─── The D5 readiness line ────────────────────────────────────────────────────

/// `architecture.md`'s readiness, from its last non-empty line, trimmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    Pass,
    /// `Readiness: BLOCKED` followed by a reason; the reason, trimmed of the
    /// separator (`—`, `–`, `-`, `:`).
    Blocked(String),
    /// No line, or a line that is neither form.
    Malformed,
}

/// D5's one parser, two callers (`flow_advance` leaving `architecture`,
/// `flow_gate` approving it). Only an exact `Readiness: PASS` passes.
pub(crate) fn parse_readiness(content: &str) -> Readiness {
    let Some(last) = content
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
    else {
        return Readiness::Malformed;
    };
    if last == "Readiness: PASS" {
        return Readiness::Pass;
    }
    let Some(rest) = last.strip_prefix("Readiness: BLOCKED") else {
        return Readiness::Malformed;
    };
    if rest.chars().next().is_some_and(char::is_alphanumeric) {
        return Readiness::Malformed;
    }
    let reason = rest
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '—' | '–' | '-' | ':'))
        .trim();
    if reason.is_empty() {
        Readiness::Malformed
    } else {
        Readiness::Blocked(reason.to_string())
    }
}

// ─── Packages and the package hash (design D11) ───────────────────────────────

/// The records a gate's package covers: those D4 requires of every phase in
/// the job's sequence after the previous gate (or the start) and before this
/// gate. Derived from the sequence, so every lane gets its package with no
/// per-gate table. Empty when `gate` is not in `phases`.
pub(crate) fn package_records(phases: &[String], gate: &str) -> Vec<&'static str> {
    let Some(gate_position) = phases.iter().position(|phase| phase == gate) else {
        return Vec::new();
    };
    let start = phases[..gate_position]
        .iter()
        .rposition(|phase| is_gate(phase))
        .map_or(0, |previous_gate| previous_gate + 1);
    phases[start..gate_position]
        .iter()
        .flat_map(|phase| required_records(phase).iter().copied())
        .collect()
}

/// A package hash and the per-record digests it was built from (`None` =
/// absent), so a stale refusal can name the changed or missing file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageDigest {
    pub hash: String,
    pub files: BTreeMap<String, Option<String>>,
}

/// Hash the named records under `records_dir`: LF-normalized and taken in
/// sorted order like `design_state_hash`, each framed as `name\0` then a
/// presence tag — `+` with an 8-byte length and the content, or `-` when
/// absent — so an absent, an empty and a NUL-carrying record all differ, and
/// deleting a record changes the key.
pub(crate) fn package_hash(records_dir: &Path, names: &[&str]) -> std::io::Result<PackageDigest> {
    let mut contents: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
    for name in names {
        let content = match std::fs::read(records_dir.join(name)) {
            Ok(bytes) => Some(normalize_eol(&bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        contents.insert((*name).to_string(), content);
    }
    let mut hasher = Sha256::new();
    let mut files = BTreeMap::new();
    for (name, content) in &contents {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        match content {
            Some(bytes) => {
                hasher.update(b"+");
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
                files.insert(name.clone(), Some(format!("{:x}", Sha256::digest(bytes))));
            }
            None => {
                hasher.update(b"-");
                files.insert(name.clone(), None);
            }
        }
    }
    Ok(PackageDigest {
        hash: format!("{:x}", hasher.finalize()),
        files,
    })
}

/// CRLF and lone CR become LF — `design_hash.rs`'s rule, re-stated here
/// because that helper is private to its module.
fn normalize_eol(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    let mut bytes = content.iter().peekable();
    while let Some(&byte) = bytes.next() {
        if byte == b'\r' {
            if bytes.peek() == Some(&&b'\n') {
                bytes.next();
            }
            out.push(b'\n');
        } else {
            out.push(byte);
        }
    }
    out
}

// ─── Flow-directory layout (design D2) ────────────────────────────────────────

const RECORDS_DIR: &str = "records";
const GATES_DIR: &str = "gates";
const LOG_DIR: &str = "log";
const MEMORY_DIR: &str = "memory";
const HANDOFFS_DIR: &str = "handoffs";
const LESSONS_FILE: &str = "lessons-candidates.md";

/// The review phases whose rewinds count as FIX rounds (D3).
const REVIEW_PHASES: [&str; 2] = ["schematic_review", "prefab_review"];

/// One job, one log file: `log/<started_at date>-<job_id>.md` (D2). Both
/// parts were validated by [`parse_state`] or minted by `flow_start`.
fn log_file_name(state: &JobState) -> String {
    format!("{}-{}.md", &state.started_at[..10], state.job_id)
}

/// The package a gate phase binds to, hashed from the records on disk now.
pub(crate) fn current_package(
    flow_dir: &Path,
    phases: &[String],
    gate_token: &str,
) -> std::io::Result<PackageDigest> {
    package_hash(
        &flow_dir.join(RECORDS_DIR),
        &package_records(phases, gate_token),
    )
}

/// Read and parse `STATE.md` under the shared document lock. `Ok(None)` when
/// the flow directory holds no state file.
fn read_state(flow_dir: &Path) -> Result<Option<JobState>, String> {
    let path = flow_dir.join(STATE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let text = konnect_sexp::read_consistent(&path)
        .map_err(|error| format!("Could not read {} ({error})", path.display()))?;
    parse_state(&text).map(Some)
}

// ─── Typed errors ─────────────────────────────────────────────────────────────

fn conflict(path: &Path, message: String) -> CallToolResult {
    CallToolResult::error_kind(
        ToolErrorKind::Conflict {
            paths: vec![path.display().to_string()],
        },
        message,
    )
}

fn stale(target: &str, reason: String) -> CallToolResult {
    CallToolResult::error_kind(
        ToolErrorKind::StaleTarget {
            target: target.to_string(),
            reason: reason.clone(),
        },
        reason,
    )
}

// ─── flow_status (design D1, D4, D11) ─────────────────────────────────────────

/// What a `read` name resolves to (D4's readable names). There is no caller
/// path: every name maps onto a fixed layout slot.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReadTarget {
    /// Path components below the flow directory.
    Flow(Vec<String>),
    /// The current job's log file.
    Log,
    /// A file in the current job's handoff directory.
    Handoff(String),
}

fn classify_read_name(name: &str) -> Option<ReadTarget> {
    let flow = |parts: &[&str]| {
        Some(ReadTarget::Flow(
            parts.iter().map(|part| part.to_string()).collect(),
        ))
    };
    if RECORD_NAMES.contains(&name) || name == LESSONS_FILE {
        return flow(&[RECORDS_DIR, name]);
    }
    if name == "log" {
        return Some(ReadTarget::Log);
    }
    if let Some(file) = name.strip_prefix("gates/") {
        let gate = file.strip_suffix(".md")?;
        return if GATE_NAMES.contains(&gate) {
            flow(&[RECORDS_DIR, GATES_DIR, file])
        } else {
            None
        };
    }
    if let Some(file) = name.strip_prefix("memory/") {
        let role = file.strip_suffix(".md")?;
        return if ROLES.contains(&role) {
            flow(&[MEMORY_DIR, file])
        } else {
            None
        };
    }
    if let Some(file) = name.strip_prefix("handoffs/") {
        return is_handoff_file_name(file).then(|| ReadTarget::Handoff(file.to_string()));
    }
    None
}

/// `<NN>-<role>.md`: at least two digits, a D7 role slug.
fn is_handoff_file_name(file: &str) -> bool {
    let Some((number, role)) = file
        .strip_suffix(".md")
        .and_then(|stem| stem.split_once('-'))
    else {
        return false;
    };
    number.len() >= 2 && number.bytes().all(|byte| byte.is_ascii_digit()) && ROLES.contains(&role)
}

fn read_targets(args: &Value) -> Result<Vec<(String, ReadTarget)>, CallToolResult> {
    let names = opt_str_list(args, "read")?.unwrap_or_default();
    names
        .into_iter()
        .map(|name| match classify_read_name(&name) {
            Some(target) => Ok((name, target)),
            None => Err(invalid_arg(
                "read",
                &format!(
                    "{name:?} is not a readable name. Readable: the ten record names, \
                     {LESSONS_FILE}, gates/<architecture|placement|purchase>.md, log, \
                     memory/<role>.md, handoffs/<NN>-<role>.md. Nothing was read."
                ),
            )),
        })
        .collect()
}

async fn handle_flow_status(args: &Value, _ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let read = match read_targets(args) {
        Ok(read) => read,
        Err(rejection) => return Ok(rejection),
    };
    let project = match resolve_project_dir(&project_arg) {
        Ok(project) => project,
        Err(reason) => return Ok(invalid_arg("project_dir", &reason)),
    };
    Ok(tokio::task::spawn_blocking(move || status_report(&project, &read)).await?)
}

/// The whole status. Refuses only for argument reasons; flow-state problems
/// become fields (`state_error`), and nothing is created.
fn status_report(project: &Path, read: &[(String, ReadTarget)]) -> CallToolResult {
    let (design_hash, design_files) = match design_state_hash(project) {
        Ok(hashed) => hashed,
        Err(error) => {
            return CallToolResult::error(format!(
                "Could not hash the design under {}: {error:#}",
                project.display()
            ))
        }
    };
    let lock_files = lock_files(project, &design_files);

    let mut state_error = None;
    let flow_dir = existing_flow_dir(project).unwrap_or_else(|error| {
        state_error = Some(error);
        None
    });
    let state = match flow_dir.as_deref().map(read_state) {
        Some(Ok(state)) => state,
        Some(Err(error)) => {
            state_error = Some(error);
            None
        }
        None => None,
    };

    let mut contents = serde_json::Map::new();
    let mut missing = Vec::new();
    for (name, target) in read {
        match read_target(flow_dir.as_deref(), state.as_ref(), target) {
            Ok(Some(text)) => {
                contents.insert(name.clone(), Value::String(text));
            }
            Ok(None) => missing.push(name.clone()),
            Err(rejection) => return rejection,
        }
    }

    let mut response = json!({
        "project_dir": project.display().to_string(),
        "job": null,
        "phase": null,
        "design_hash": design_hash,
        "design_files": design_files,
        "lock_files": lock_files,
        "gate_approvals": {},
        "pending_approvals": [],
        "deferred_findings": [],
        "queue": [],
        "fix_rounds": fix_rounds(None),
        "last_transition": null,
        "handoffs": [],
        "next_step": null,
        "contents": contents,
        "missing": missing,
        "state_error": state_error,
    });
    if let (Some(flow_dir), Some(state)) = (&flow_dir, &state) {
        response["job"] = json!({
            "job_id": state.job_id,
            "objective": state.objective,
            "lane": state.lane,
            "mode": state.mode,
            "started_at": state.started_at,
            "phases": state.phases,
        });
        response["phase"] = json!(state.phase);
        response["gate_approvals"] = gate_validity(flow_dir, state, &design_hash);
        response["pending_approvals"] = json!(state.pending_approvals);
        response["deferred_findings"] = json!(state.deferred_findings);
        response["queue"] = json!(state.queue);
        response["fix_rounds"] = fix_rounds(Some(state));
        response["last_transition"] = json!(state.history.last());
        response["handoffs"] = json!(list_handoffs(flow_dir, &state.job_id));
        response["next_step"] = next_step(state).map_or(Value::Null, |step| {
            json!({
                "phase": step.phase,
                "required_records": step.required_records,
                "next_phase": step.next_phase,
                "is_gate": step.is_gate,
            })
        });
    }
    CallToolResult::json(&response)
}

/// KiCad's `~<file>.lck` beside **every** covered file, `.kicad_pro`
/// included — `kicad_editor_lock_path` ignores `.kicad_pro` (D11).
fn lock_files(project: &Path, design_files: &[String]) -> Vec<String> {
    let mut locks: Vec<String> = design_files
        .iter()
        .filter_map(|relative| {
            let lock = match relative.rsplit_once('/') {
                Some((dir, name)) => format!("{dir}/~{name}.lck"),
                None => format!("~{relative}.lck"),
            };
            std::fs::symlink_metadata(project.join(&lock))
                .is_ok()
                .then_some(lock)
        })
        .collect();
    locks.sort();
    locks.dedup();
    locks
}

/// Each recorded approval with its `status` (Fix round 1, DECISION D): the
/// gate the job stands at is `current`, with `valid` recomputed — both keys
/// still equal the design and the package as they are now. Every other
/// approval is `passed` and carries no `valid`: work after a gate changes the
/// design by design, and the hashes it was approved at are already there.
fn gate_validity(flow_dir: &Path, state: &JobState, design_hash: &str) -> Value {
    let mut gates = serde_json::Map::new();
    for (gate, approval) in &state.gate_approvals {
        let mut entry = json!(approval);
        let gate_token = format!("gate:{gate}");
        if gate_token != state.phase {
            entry["status"] = json!("passed");
            gates.insert(gate.clone(), entry);
            continue;
        }
        entry["status"] = json!("current");
        match current_package(flow_dir, &state.phases, &gate_token) {
            Ok(package) => {
                entry["valid"] = json!(
                    approval.design_hash_at_approval == design_hash
                        && approval.package_hash_at_approval == package.hash
                );
            }
            Err(error) => {
                entry["valid"] = json!(false);
                entry["validity_error"] = json!(format!("package unreadable: {error}"));
            }
        }
        gates.insert(gate.clone(), entry);
    }
    Value::Object(gates)
}

fn fix_rounds(state: Option<&JobState>) -> Value {
    let mut rounds = serde_json::Map::new();
    for phase in REVIEW_PHASES {
        let count = state.map_or(0, |state| {
            state
                .history
                .iter()
                .filter(|entry| {
                    entry.kind == HistoryKind::Rewind && entry.from.as_deref() == Some(phase)
                })
                .count()
        });
        rounds.insert(phase.to_string(), json!(count));
    }
    Value::Object(rounds)
}

/// The job's handoff file names, sorted; anything not shaped like a handoff
/// is not listed (and so not readable).
fn list_handoffs(flow_dir: &Path, job_id: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(flow_dir.join(HANDOFFS_DIR).join(job_id)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| is_handoff_file_name(name))
        .collect();
    names.sort();
    names
}

/// One `read` name's content: `Ok(None)` when absent (reported in
/// `missing`), a refusal when it resolves outside the flow directory.
fn read_target(
    flow_dir: Option<&Path>,
    state: Option<&JobState>,
    target: &ReadTarget,
) -> Result<Option<String>, CallToolResult> {
    let Some(flow_dir) = flow_dir else {
        return Ok(None);
    };
    let path = match (target, state) {
        (ReadTarget::Flow(parts), _) => parts
            .iter()
            .fold(flow_dir.to_path_buf(), |path, part| path.join(part)),
        (ReadTarget::Log, Some(state)) => flow_dir.join(LOG_DIR).join(log_file_name(state)),
        (ReadTarget::Handoff(file), Some(state)) => {
            flow_dir.join(HANDOFFS_DIR).join(&state.job_id).join(file)
        }
        (_, None) => return Ok(None),
    };
    let canonical = match path.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CallToolResult::error(format!(
                "Could not resolve {} ({error})",
                path.display()
            )))
        }
    };
    if let Err(reason) = confine(&canonical, flow_dir, &path) {
        return Err(invalid_arg("read", &reason));
    }
    if !canonical.is_file() {
        return Ok(None);
    }
    std::fs::read(&canonical)
        .map(|bytes| Some(String::from_utf8_lossy(&bytes).into_owned()))
        .map_err(|error| {
            CallToolResult::error(format!("Could not read {} ({error})", path.display()))
        })
}

// ─── The job log ──────────────────────────────────────────────────────────────

/// One timestamped log entry: a heading and bullet lines.
fn log_entry(at: &str, title: &str, lines: &[String]) -> String {
    let mut entry = format!("## {at} · {title}\n\n");
    for line in lines {
        entry.push_str(&format!("- {line}\n"));
    }
    entry.push('\n');
    entry
}

/// Append one entry to the job's log, creating `log/` on demand.
fn append_log(flow_dir: &Path, state: &JobState, entry: &str) -> Result<(), String> {
    append_log_file(flow_dir, &log_file_name(state), entry)
}

/// [`append_log`] by file name, for a write that runs after the state it was
/// named from has been committed.
fn append_log_file(flow_dir: &Path, file_name: &str, entry: &str) -> Result<(), String> {
    let dir = ensure_flow_subdir(flow_dir, &[LOG_DIR])?;
    append_file(&dir.join(file_name), entry)
}

/// Append one entry with a single `write_all` on an append-mode handle (the
/// observer's JSONL pattern).
fn append_file(path: &Path, entry: &str) -> Result<(), String> {
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(entry.as_bytes()))
        .map_err(|error| format!("Could not append to {} ({error})", path.display()))
}

// ─── flow_start (design D1, D2, D3) ───────────────────────────────────────────

/// D1's slug: lowercase ASCII alphanumerics, every other run → one `-`,
/// trimmed, at most 40 characters, `job` when nothing is left.
pub(crate) fn job_slug(objective: &str) -> String {
    let mut slug = String::new();
    let mut separator_pending = false;
    for character in objective.chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                slug.push('-');
            }
            separator_pending = false;
            slug.push(character.to_ascii_lowercase());
        } else {
            separator_pending = true;
        }
    }
    let mut truncated: String = slug.chars().take(40).collect();
    while truncated.ends_with('-') {
        truncated.pop();
    }
    if truncated.is_empty() {
        "job".to_string()
    } else {
        truncated
    }
}

/// `2026-09-21T14:00:05Z` → `20260921-140005`.
fn compact_stamp(at: &str) -> String {
    let digits = |range: std::ops::Range<usize>| at.get(range).unwrap_or("00");
    format!(
        "{}{}{}-{}{}{}",
        digits(0..4),
        digits(5..7),
        digits(8..10),
        digits(11..13),
        digits(14..16),
        digits(17..19)
    )
}

async fn handle_flow_start(args: &Value, _ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let objective = match require_str(args, "objective") {
        Ok(value) if !value.trim().is_empty() => value.to_string(),
        Ok(_) => {
            return Ok(invalid_arg(
                "objective",
                "must not be empty. Nothing was written.",
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let lane = match require_str(args, "lane").map(Lane::parse) {
        Ok(Some(lane)) => lane,
        Ok(None) => {
            return Ok(invalid_arg(
                "lane",
                &format!(
                    "must be one of: {}. Nothing was written.",
                    Lane::ALL.map(Lane::as_str).join(", ")
                ),
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let mode = match args.get("mode") {
        None | Some(Value::Null) => Mode::Guided,
        Some(value) => match value.as_str().and_then(Mode::parse) {
            Some(mode) => mode,
            None => {
                return Ok(invalid_arg(
                    "mode",
                    "must be guided or autonomous. Nothing was written.",
                ))
            }
        },
    };
    let phases = match opt_str_list(args, "phases") {
        Ok(Some(phases)) => phases,
        Ok(None) if lane == Lane::NewBoard => {
            CANONICAL_PHASES.iter().map(|p| p.to_string()).collect()
        }
        Ok(None) => {
            return Ok(invalid_arg(
                "phases",
                &format!(
                    "is required for lane {}; only new_board defaults to the full sequence. \
                     Nothing was written.",
                    lane.as_str()
                ),
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    if let Err(reason) = validate_phases(&phases) {
        return Ok(invalid_arg(
            "phases",
            &format!("{reason}. Nothing was written."),
        ));
    }
    let project = match resolve_project_dir(&project_arg) {
        Ok(project) => project,
        Err(reason) => return Ok(invalid_arg("project_dir", &reason)),
    };
    Ok(
        tokio::task::spawn_blocking(move || start_job(&project, objective, lane, mode, phases))
            .await?,
    )
}

/// Every argument is valid by now; the only remaining refusal is an existing
/// job that is not closed (or a state file that does not parse).
fn start_job(
    project: &Path,
    objective: String,
    lane: Lane,
    mode: Mode,
    phases: Vec<String>,
) -> CallToolResult {
    let (design_hash, _) = match design_state_hash(project) {
        Ok(hashed) => hashed,
        Err(error) => {
            return CallToolResult::error(format!(
                "Could not hash the design under {}: {error:#}. Nothing was written.",
                project.display()
            ))
        }
    };
    let at = crate::tools::photo_intake::now_rfc3339_utc();
    let first = phases[0].clone();
    let state = JobState {
        schema: STATE_SCHEMA,
        job_id: format!("{}-{}", job_slug(&objective), compact_stamp(&at)),
        objective,
        lane,
        mode,
        phases,
        phase: first.clone(),
        started_at: at.clone(),
        gate_approvals: BTreeMap::new(),
        pending_approvals: Vec::new(),
        deferred_findings: Vec::new(),
        queue: Vec::new(),
        history: vec![HistoryEntry::new(
            HistoryKind::Start,
            None,
            &first,
            &at,
            &design_hash,
        )],
    };

    let flow_dir = match create_flow_dir(project) {
        Ok(flow_dir) => flow_dir,
        Err(reason) => return CallToolResult::error(reason),
    };
    let state_path = flow_dir.join(STATE_FILE);
    if let Err(refusal) = write_first_state(&state_path, &render_state(&state)) {
        return refusal;
    }

    let entry = log_entry(
        &at,
        "start",
        &[
            format!(
                "job `{}` · lane `{}` · mode `{}`",
                state.job_id,
                lane.as_str(),
                mode.as_str()
            ),
            format!("objective: {}", one_line(&state.objective)),
            format!("phases: {}", state.phases.join(" → ")),
            format!("design_hash: `{design_hash}`"),
        ],
    );
    let log_error = append_log(&flow_dir, &state, &entry).err();

    CallToolResult::json(&json!({
        "job_id": state.job_id,
        "project_dir": project.display().to_string(),
        "objective": state.objective,
        "lane": state.lane,
        "mode": state.mode,
        "phases": state.phases,
        "phase": state.phase,
        "started_at": state.started_at,
        "design_hash": design_hash,
        "state_file": state_path.display().to_string(),
        "log_error": log_error,
    }))
}

/// Create `STATE.md` no-clobber, so two racing starts cannot both open a job;
/// the loser — or any call finding a state file — falls through to the locked
/// path, which replaces the file only when its job is closed.
fn write_first_state(state_path: &Path, rendered: &str) -> Result<(), CallToolResult> {
    match konnect_sexp::write_new_atomic(state_path, rendered) {
        Ok(()) => return Ok(()),
        Err(konnect_sexp::SexpError::Io(error))
            if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(CallToolResult::error(format!(
                "Could not create {} ({error})",
                state_path.display()
            )))
        }
    }
    let outcome = konnect_sexp::transact_atomic(state_path, |current| {
        let refusal = match parse_state(current) {
            Ok(existing) if existing.phase == CLOSED => {
                return Ok((rendered.to_string(), Ok(())));
            }
            Ok(existing) => format!(
                "Job {} is still active at phase {:?} in {}. Close or abandon it with \
                 flow_advance before starting another. Nothing was written.",
                existing.job_id,
                existing.phase,
                state_path.display()
            ),
            Err(error) => format!(
                "{error}. Fix or remove {} by hand; it was left untouched and no job was \
                 started.",
                state_path.display()
            ),
        };
        Ok((current.to_string(), Err(conflict(state_path, refusal))))
    });
    match outcome {
        Ok(result) => result,
        Err(error) => Err(CallToolResult::error(format!(
            "Could not update {} ({error})",
            state_path.display()
        ))),
    }
}

// ─── flow_advance (design D1, D3, D4, D5, D11) ────────────────────────────────

/// How `to_phase` relates to the job's sequence (D3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transition {
    /// The entry right after the current one.
    Forward,
    /// `closed` from the last entry.
    Close,
    /// Any earlier entry.
    Rewind,
    /// `closed` from any entry but the last.
    Abandon,
}

/// Classify a move of an open job (its `phase` is an entry of `phases`).
/// Skip-ahead, same phase and tokens outside the job are errors.
pub(crate) fn classify_transition(state: &JobState, to_phase: &str) -> Result<Transition, String> {
    let Some(current) = state.phases.iter().position(|phase| *phase == state.phase) else {
        return Err(format!(
            "the job is not at an entry of its phases ({:?})",
            state.phase
        ));
    };
    let is_last = current + 1 == state.phases.len();
    if to_phase == CLOSED {
        return Ok(if is_last {
            Transition::Close
        } else {
            Transition::Abandon
        });
    }
    let Some(target) = state.phases.iter().position(|phase| phase == to_phase) else {
        return Err(format!(
            "{to_phase:?} is not in this job's phases: {}",
            state.phases.join(", ")
        ));
    };
    let next = state.phases.get(current + 1).map_or(CLOSED, String::as_str);
    match target.cmp(&current) {
        std::cmp::Ordering::Less => Ok(Transition::Rewind),
        std::cmp::Ordering::Equal => Err(format!(
            "the job is already at {to_phase:?}; the next phase is {next:?}"
        )),
        std::cmp::Ordering::Greater if target == current + 1 => Ok(Transition::Forward),
        std::cmp::Ordering::Greater => Err(format!(
            "{to_phase:?} skips ahead of {:?}; the next phase is {next:?}",
            state.phase
        )),
    }
}

/// The history index of the entry that entered the current phase — a gate
/// approval's `visit` must equal it for the approval to count.
pub(crate) fn current_visit(state: &JobState) -> Option<usize> {
    state
        .history
        .iter()
        .rposition(|entry| entry.to == state.phase)
}

/// A validated `flow_advance` call, before it meets the state file.
struct AdvanceRequest {
    job_id: String,
    to_phase: String,
    records: Vec<(String, String)>,
    evidence_calls: Vec<String>,
    /// D6, computed from the observer ring when the call arrived; `None`
    /// when no evidence call was cited.
    evidence_check: Option<EvidenceCheck>,
    reason: Option<String>,
}

/// D6: each cited tool (first occurrence only) against the observer ring —
/// `confirmed` with at least one `ok` call, `not_ok` present only with
/// another status, `absent` not in the ring. A report, never a refusal: a
/// resumed job's evidence may predate this server process, and the ring has
/// no caller identity, so it catches "never ran", not "ran elsewhere".
pub(crate) fn evidence_check(cited: &[String], ring: &[CallRecord]) -> EvidenceCheck {
    let mut check = EvidenceCheck {
        ring_calls: ring.len(),
        ..EvidenceCheck::default()
    };
    for (position, tool) in cited.iter().enumerate() {
        if cited[..position].contains(tool) {
            continue;
        }
        let mut calls = ring.iter().filter(|call| call.tool == *tool).peekable();
        let bucket = if calls.peek().is_none() {
            &mut check.absent
        } else if calls.any(|call| call.status == CallStatus::Ok) {
            &mut check.confirmed
        } else {
            &mut check.not_ok
        };
        bucket.push(tool.clone());
    }
    check
}

/// `records`: an array of `{filename, content}`, each filename one of D4's
/// ten names, none repeated.
fn parse_records(args: &Value) -> Result<Vec<(String, String)>, CallToolResult> {
    let entries = match args.get("records") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(entries)) => entries,
        Some(_) => {
            return Err(invalid_arg(
                "records",
                "expected an array of {filename, content} objects",
            ))
        }
    };
    let mut records: Vec<(String, String)> = Vec::new();
    for entry in entries {
        let filename = entry.get("filename").and_then(Value::as_str);
        let content = entry.get("content").and_then(Value::as_str);
        let (Some(filename), Some(content)) = (filename, content) else {
            return Err(invalid_arg(
                "records",
                "every entry needs a string `filename` and a string `content`",
            ));
        };
        if !RECORD_NAMES.contains(&filename) {
            return Err(invalid_arg(
                "records",
                &format!(
                    "{filename:?} is not a record name; expected one of: {}",
                    RECORD_NAMES.join(", ")
                ),
            ));
        }
        if records.iter().any(|(name, _)| name == filename) {
            return Err(invalid_arg(
                "records",
                &format!("{filename:?} appears more than once"),
            ));
        }
        records.push((filename.to_string(), content.to_string()));
    }
    Ok(records)
}

async fn handle_flow_advance(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let job_id = match require_str(args, "job_id") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let to_phase = match require_str(args, "to_phase") {
        Ok(value) if value == CLOSED || canonical_index(value).is_some() => value.to_string(),
        Ok(value) => {
            return Ok(invalid_arg(
                "to_phase",
                &format!("{value:?} is not a phase token or {CLOSED:?}. Nothing was written."),
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let records = match parse_records(args) {
        Ok(records) => records,
        Err(rejection) => return Ok(rejection),
    };
    let evidence_calls = match opt_str_list(args, "evidence_calls") {
        Ok(calls) => calls.unwrap_or_default(),
        Err(rejection) => return Ok(rejection),
    };
    let reason = match args.get("reason") {
        None | Some(Value::Null) => None,
        Some(Value::String(reason)) => Some(reason.clone()).filter(|r| !r.trim().is_empty()),
        Some(_) => return Ok(invalid_arg("reason", "must be a string")),
    };
    let project = match resolve_project_dir(&project_arg) {
        Ok(project) => project,
        Err(reason) => return Ok(invalid_arg("project_dir", &reason)),
    };
    // D6: read the ring now, in the server process that served the producer's
    // calls — they are its most recent ones at this moment.
    let evidence_check = if evidence_calls.is_empty() {
        None
    } else {
        Some(evidence_check(
            &evidence_calls,
            &ctx.observer.recent(0).await,
        ))
    };
    let request = AdvanceRequest {
        job_id,
        to_phase,
        records,
        evidence_calls,
        evidence_check,
        reason,
    };
    Ok(tokio::task::spawn_blocking(move || advance_job(&project, &request)).await?)
}

/// The existing `STATE.md` of a mutating call: absent → `stale_target`,
/// nothing created (only `flow_start` creates the flow directory).
fn existing_state_path(project: &Path) -> Result<(PathBuf, PathBuf), CallToolResult> {
    let no_job = || {
        stale(
            STATE_FILE,
            format!(
                "{} has no flow job ({STATE_FILE} is absent); call flow_start first. Nothing \
                 was written.",
                project.display()
            ),
        )
    };
    let flow_dir = match existing_flow_dir(project) {
        Ok(Some(flow_dir)) => flow_dir,
        Ok(None) => return Err(no_job()),
        Err(reason) => return Err(CallToolResult::error(reason)),
    };
    let state_path = flow_dir.join(STATE_FILE);
    if !state_path.is_file() {
        return Err(no_job());
    }
    Ok((flow_dir, state_path))
}

/// Run a mutation of an existing job under the `STATE.md` lock (D2): `apply`
/// receives the flow directory, the state path and the current text, and
/// returns the new text and the response. A refusal hands the text back
/// unchanged, so `STATE.md` is not rewritten — and `apply` writes its side
/// files only after every check passed.
fn transact_state(
    project: &Path,
    apply: impl FnOnce(&Path, &Path, &str) -> Result<(String, Value), CallToolResult>,
) -> CallToolResult {
    match transact(project, apply) {
        Ok((_, response)) => CallToolResult::json(&response),
        Err(refusal) => refusal,
    }
}

/// The locked read-modify-write behind every mutation of an existing job:
/// `Ok` (with the flow directory and `apply`'s payload) only once the new
/// `STATE.md` is in place.
fn transact<T>(
    project: &Path,
    apply: impl FnOnce(&Path, &Path, &str) -> Result<(String, T), CallToolResult>,
) -> Result<(PathBuf, T), CallToolResult> {
    let (flow_dir, state_path) = existing_state_path(project)?;
    let outcome = konnect_sexp::transact_atomic(&state_path, |current| {
        Ok(match apply(&flow_dir, &state_path, current) {
            Ok((next, payload)) => (next, Ok(payload)),
            Err(refusal) => (current.to_string(), Err(refusal)),
        })
    });
    match outcome {
        Ok(Ok(payload)) => Ok((flow_dir, payload)),
        Ok(Err(refusal)) => Err(refusal),
        Err(error) => Err(CallToolResult::error(format!(
            "Could not update {} ({error})",
            state_path.display()
        ))),
    }
}

/// An accepted gate decision, transition or deferred item: its response, and
/// the derived side files that must FOLLOW the `STATE.md` commit (D2, Fix
/// round 1, DECISIONs C and H) so neither can record a change `STATE.md`
/// never held.
struct Committed {
    response: Value,
    /// `records/gates/<gate>.md`'s file name and full text (`flow_gate` only).
    gate_file: Option<(String, String)>,
    /// The job's log file name and the entry to append.
    log: (String, String),
}

/// `transact_state` for `flow_gate`, `flow_advance` and `flow_defer`:
/// `STATE.md`'s successful write is the one commit point. The gate file and
/// the log entry are written only after it, and a failure there is a
/// `warning` on the success response (the `flow_start` `log_error` pattern) —
/// never an error, which would invite a retry of a change `STATE.md` already
/// holds.
fn transact_then_record(
    project: &Path,
    apply: impl FnOnce(&Path, &Path, &str) -> Result<(String, Committed), CallToolResult>,
) -> CallToolResult {
    let (flow_dir, committed) = match transact(project, apply) {
        Ok(done) => done,
        Err(refusal) => return refusal,
    };
    let mut response = committed.response;
    response["warning"] = json!(record_after_commit(
        &flow_dir,
        committed.gate_file.as_ref(),
        &committed.log
    ));
    CallToolResult::json(&response)
}

/// Write the derived side files of a committed change; `None` when both
/// landed, otherwise the warning naming every write that failed.
fn record_after_commit(
    flow_dir: &Path,
    gate_file: Option<&(String, String)>,
    (log_name, entry): &(String, String),
) -> Option<String> {
    let mut failures = Vec::new();
    if let Some((name, text)) = gate_file {
        let written = ensure_flow_subdir(flow_dir, &[RECORDS_DIR, GATES_DIR]).and_then(|dir| {
            let path = dir.join(name);
            konnect_sexp::write_atomic(&path, text)
                .map_err(|error| format!("Could not write {} ({error})", path.display()))
        });
        failures.extend(written.err());
    }
    failures.extend(append_log_file(flow_dir, log_name, entry).err());
    (!failures.is_empty()).then(|| {
        format!(
            "{STATE_FILE} holds this change, but a derived side file was not written: {}. Do \
             not repeat the call; flow_status shows the committed state.",
            failures.join("; ")
        )
    })
}

/// The job in `current`, refused as a `conflict` when the front matter does
/// not parse and as a `stale_target` when `job_id` is not the project's job
/// (active or closed).
fn load_job(state_path: &Path, current: &str, job_id: &str) -> Result<JobState, CallToolResult> {
    let state = parse_state(current).map_err(|error| {
        conflict(
            state_path,
            format!("{error}. Fix it by hand; nothing was written."),
        )
    })?;
    if job_id != state.job_id {
        return Err(stale(
            &format!("job:{job_id}"),
            format!(
                "job_id {job_id:?} is not this project's job ({:?}); read flow_status. Nothing \
                 was written.",
                state.job_id
            ),
        ));
    }
    Ok(state)
}

/// Run the transition under the `STATE.md` lock; its log entry follows the
/// commit.
fn advance_job(project: &Path, request: &AdvanceRequest) -> CallToolResult {
    transact_then_record(project, |flow_dir, state_path, current| {
        apply_advance(project, flow_dir, state_path, current, request)
    })
}

/// Parse, check the job, classify, then dispatch on the transition. Returns
/// the new `STATE.md` text and the committed change.
fn apply_advance(
    project: &Path,
    flow_dir: &Path,
    state_path: &Path,
    current: &str,
    request: &AdvanceRequest,
) -> Result<(String, Committed), CallToolResult> {
    let state = load_job(state_path, current, &request.job_id)?;
    if state.phase == CLOSED {
        return Err(stale(
            &format!("job:{}", request.job_id),
            format!(
                "job {:?} is closed; start a new one with flow_start. Nothing was written.",
                state.job_id
            ),
        ));
    }
    let transition = classify_transition(&state, &request.to_phase)
        .map_err(|reason| invalid_arg("to_phase", &format!("{reason}. Nothing was written.")))?;
    match transition {
        Transition::Forward | Transition::Close => {
            advance_forward(project, flow_dir, state, request, transition)
        }
        Transition::Rewind | Transition::Abandon => move_back(project, state, request, transition),
    }
}

/// D4's same-call rule: every record the phase being left produces must be
/// in THIS call's `records`, and every supplied record must belong to that
/// phase. A file already on disk never counts — it was written by an earlier
/// visit or an earlier job.
fn check_forward_records(
    leaving: &str,
    records: &[(String, String)],
) -> Result<(), CallToolResult> {
    for (name, _) in records {
        let owner = record_phase(name).unwrap_or("no phase");
        if owner != leaving {
            return Err(invalid_arg(
                "records",
                &format!(
                    "{name} belongs to phase {owner:?}, not to {leaving:?}, the phase being \
                     left. Nothing was written."
                ),
            ));
        }
    }
    let missing: Vec<&str> = required_records(leaving)
        .iter()
        .copied()
        .filter(|required| !records.iter().any(|(name, _)| name == required))
        .collect();
    if !missing.is_empty() {
        return Err(invalid_arg(
            "records",
            &format!(
                "leaving {leaving:?} requires {} in this call's records; a record already on \
                 disk does not count. Nothing was written.",
                missing.join(", ")
            ),
        ));
    }
    if leaving == "architecture" {
        let content = records
            .iter()
            .find(|(name, _)| name == ARCHITECTURE_RECORD)
            .map_or("", |(_, content)| content.as_str());
        let problem = match parse_readiness(content) {
            Readiness::Pass => return Ok(()),
            Readiness::Blocked(reason) => format!("its readiness line is BLOCKED — {reason}"),
            Readiness::Malformed => "its last non-empty line is not a readiness line".to_string(),
        };
        return Err(invalid_arg(
            "records",
            &format!(
                "{ARCHITECTURE_RECORD} must end with `Readiness: PASS` to leave architecture; \
                 {problem}. Collect the missing value (typically a rewind to requirements). \
                 Nothing was written."
            ),
        ));
    }
    Ok(())
}

/// Leaving a gate phase: the approval must be of the current visit, and the
/// design and package must still equal what it approved (D11).
fn check_gate_exit(
    flow_dir: &Path,
    state: &JobState,
    design_hash: &str,
) -> Result<(), CallToolResult> {
    let gate_token = state.phase.as_str();
    let gate = gate_token.trim_start_matches("gate:");
    let visit = current_visit(state);
    let Some(approval) = state
        .gate_approvals
        .get(gate)
        .filter(|approval| Some(approval.visit) == visit)
    else {
        return Err(stale(
            gate_token,
            format!(
                "gate {gate:?} has no approval recorded during this visit; approve it with \
                 flow_gate first. Nothing was written."
            ),
        ));
    };
    if approval.design_hash_at_approval != design_hash {
        return Err(stale(
            gate_token,
            format!(
                "the design changed after gate {gate:?} was approved (design_hash {} then, {} \
                 now); rewind or re-approve. Nothing was written.",
                short_hash(&approval.design_hash_at_approval),
                short_hash(design_hash)
            ),
        ));
    }
    let package = current_package(flow_dir, &state.phases, gate_token).map_err(|error| {
        CallToolResult::error(format!(
            "Could not hash the package of {gate_token}: {error}"
        ))
    })?;
    if approval.package_hash_at_approval != package.hash {
        let shown = visit
            .and_then(|visit| state.history.get(visit))
            .map(|entry| &entry.package_files);
        return Err(stale(
            gate_token,
            format!(
                "the package of gate {gate:?} changed after it was approved (changed or \
                 missing: {}); rewind or re-approve. Nothing was written.",
                changed_package_files(shown, &package.files)
            ),
        ));
    }
    Ok(())
}

/// The package records whose digest now differs from what the gate entry
/// recorded (`shown`) — changed, deleted or newly present — for a refusal to
/// name; `unknown` when the entry kept no per-record digests.
fn changed_package_files(
    shown: Option<&BTreeMap<String, Option<String>>>,
    now: &BTreeMap<String, Option<String>>,
) -> String {
    let changed: Vec<&str> = now
        .iter()
        .filter(|(name, digest)| shown.and_then(|files| files.get(*name)) != Some(*digest))
        .map(|(name, _)| name.as_str())
        .collect();
    if changed.is_empty() {
        "unknown".to_string()
    } else {
        changed.join(", ")
    }
}

/// A forward move (or the close from the last entry): validate, hash, write
/// the records, then build the history entry and the new state.
fn advance_forward(
    project: &Path,
    flow_dir: &Path,
    state: JobState,
    request: &AdvanceRequest,
    transition: Transition,
) -> Result<(String, Committed), CallToolResult> {
    let leaving = state.phase.clone();
    check_forward_records(&leaving, &request.records)?;
    let design_hash = hash_design(project)?;
    if is_gate(&leaving) {
        check_gate_exit(flow_dir, &state, &design_hash)?;
    }

    // Everything is validated: the phase records before STATE.md (D2, D4 — a
    // stray copy is harmless, a retry must re-supply it); the log entry only
    // after the commit (Fix round 1, DECISION C).
    if !request.records.is_empty() {
        let records_dir =
            ensure_flow_subdir(flow_dir, &[RECORDS_DIR]).map_err(CallToolResult::error)?;
        for (name, content) in &request.records {
            let path = records_dir.join(name);
            konnect_sexp::write_atomic(&path, content).map_err(|error| {
                CallToolResult::error(format!("Could not write {} ({error})", path.display()))
            })?;
        }
    }

    let at = crate::tools::photo_intake::now_rfc3339_utc();
    let kind = if transition == Transition::Close {
        HistoryKind::Close
    } else {
        HistoryKind::Advance
    };
    let mut entry = HistoryEntry::new(kind, Some(&leaving), &request.to_phase, &at, &design_hash);
    entry.records = request
        .records
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    entry.evidence_calls = request.evidence_calls.clone();
    entry.evidence_check = request.evidence_check.clone();
    entry.reason = request.reason.clone();
    record_gate_keys(flow_dir, &state.phases, &mut entry)?;
    Ok(commit_transition(state, entry, Vec::new()))
}

/// A rewind (any earlier entry) or an abandon (`closed` before the last
/// entry), D3: never into a gate, a non-empty `reason`, no `records`. A
/// rewind removes the approval of every gate at or after its target — they
/// are granted again on the way back, when the forward move into the gate
/// records the keys the next approval is compared against (D11).
fn move_back(
    project: &Path,
    mut state: JobState,
    request: &AdvanceRequest,
    transition: Transition,
) -> Result<(String, Committed), CallToolResult> {
    // Fix round 1, DECISION B: a rewind into a gate would bind the next
    // approval to the design as it is now, behind a package produced before.
    if is_gate(&request.to_phase) {
        let producer = GATED_PHASES
            .iter()
            .find(|(_, gate)| *gate == request.to_phase)
            .map_or("the phase before it", |(phase, _)| *phase);
        return Err(invalid_arg(
            "to_phase",
            &format!(
                "a rewind cannot target the gate {:?}; rewind to {producer:?}, the phase that \
                 produces its package, and advance into the gate again so its approval sees \
                 a fresh package. Nothing was written.",
                request.to_phase
            ),
        ));
    }
    let leaving = state.phase.clone();
    let kind = if transition == Transition::Rewind {
        HistoryKind::Rewind
    } else {
        HistoryKind::Abandon
    };
    let Some(reason) = request.reason.clone() else {
        return Err(invalid_arg(
            "reason",
            &format!(
                "{:?} from {leaving:?} is {} and needs a non-empty reason. Nothing was \
                 written.",
                request.to_phase,
                if kind == HistoryKind::Rewind {
                    "a rewind"
                } else {
                    "an abandon (closed before the last phase)"
                }
            ),
        ));
    };
    if !request.records.is_empty() {
        return Err(invalid_arg(
            "records",
            &format!(
                "a {} supplies no records; records are written only by a forward move. \
                 Nothing was written.",
                kind.as_str()
            ),
        ));
    }
    let design_hash = hash_design(project)?;

    let mut cleared = Vec::new();
    if let Some(target) = state
        .phases
        .iter()
        .position(|phase| *phase == request.to_phase)
    {
        for token in state.phases[target..].iter().filter(|token| is_gate(token)) {
            let gate = token.trim_start_matches("gate:");
            if state.gate_approvals.remove(gate).is_some() {
                cleared.push(gate.to_string());
            }
        }
    }

    let at = crate::tools::photo_intake::now_rfc3339_utc();
    let mut entry = HistoryEntry::new(kind, Some(&leaving), &request.to_phase, &at, &design_hash);
    entry.evidence_calls = request.evidence_calls.clone();
    entry.evidence_check = request.evidence_check.clone();
    entry.reason = Some(reason);
    Ok(commit_transition(state, entry, cleared))
}

/// `design_state_hash` of the project, or a refusal that wrote nothing.
fn hash_design(project: &Path) -> Result<String, CallToolResult> {
    design_state_hash(project)
        .map(|(hash, _)| hash)
        .map_err(|error| {
            CallToolResult::error(format!(
                "Could not hash the design under {}: {error:#}. Nothing was written.",
                project.display()
            ))
        })
}

/// An entry INTO a gate phase carries the keys that gate's approval is
/// compared against (D11): the package hash and its per-record digests.
fn record_gate_keys(
    flow_dir: &Path,
    phases: &[String],
    entry: &mut HistoryEntry,
) -> Result<(), CallToolResult> {
    if !is_gate(&entry.to) {
        return Ok(());
    }
    let package = current_package(flow_dir, phases, &entry.to).map_err(|error| {
        CallToolResult::error(format!(
            "Could not hash the package of {}: {error}. Nothing was written.",
            entry.to
        ))
    })?;
    entry.package_hash = Some(package.hash);
    entry.package_files = package.files;
    Ok(())
}

/// The tail every accepted transition shares: compose the log entry, move the
/// phase, append the history, render the new `STATE.md`. The log entry is
/// returned, not written: it is appended only once the state file has been
/// replaced (Fix round 1, DECISION C), so it never records a transition
/// `STATE.md` does not hold.
fn commit_transition(
    mut state: JobState,
    entry: HistoryEntry,
    cleared_approvals: Vec<String>,
) -> (String, Committed) {
    let leaving = entry.from.clone().unwrap_or_default();
    let mut lines = vec![format!("design_hash: `{}`", entry.design_hash)];
    if !entry.records.is_empty() {
        lines.push(format!("records: {}", entry.records.join(", ")));
    }
    if let Some(package_hash) = &entry.package_hash {
        lines.push(format!("package_hash: `{package_hash}`"));
    }
    if !entry.evidence_calls.is_empty() {
        lines.push(format!(
            "evidence_calls: {}",
            entry.evidence_calls.join(", ")
        ));
    }
    if let Some(check) = &entry.evidence_check {
        lines.push(format!(
            "evidence_check: confirmed [{}] · not_ok [{}] · absent [{}] · ring {} calls",
            check.confirmed.join(", "),
            check.not_ok.join(", "),
            check.absent.join(", "),
            check.ring_calls
        ));
    }
    if !cleared_approvals.is_empty() {
        lines.push(format!(
            "cleared approvals: {}",
            cleared_approvals.join(", ")
        ));
    }
    if let Some(reason) = &entry.reason {
        lines.push(format!("reason: {}", one_line(reason)));
    }
    let log = log_entry(
        &entry.at,
        &format!("{} {leaving} → {}", entry.kind.as_str(), entry.to),
        &lines,
    );

    let response = json!({
        "job_id": state.job_id,
        "transition": entry.kind.as_str(),
        "from": leaving,
        "phase": entry.to,
        "records_written": entry.records,
        "design_hash": entry.design_hash,
        "package_hash": entry.package_hash,
        "evidence_check": entry.evidence_check,
        "cleared_approvals": cleared_approvals,
        "history_index": state.history.len(),
    });
    state.phase = entry.to.clone();
    state.history.push(entry);
    let committed = Committed {
        response,
        gate_file: None,
        log: (log_file_name(&state), log),
    };
    (render_state(&state), committed)
}

// ─── flow_gate (design D1, D3, D5, D11) ───────────────────────────────────────

/// A validated `flow_gate` call, before it meets the state file.
struct GateRequest {
    job_id: String,
    /// A [`GATE_NAMES`] entry; the phase token is `gate:<gate>`.
    gate: String,
    decision: GateDecision,
    summary: String,
    user_words: String,
}

async fn handle_flow_gate(args: &Value, _ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let job_id = match require_str(args, "job_id") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let gate = match require_str(args, "gate_name") {
        Ok(value) if GATE_NAMES.contains(&value) => value.to_string(),
        Ok(value) => {
            return Ok(invalid_arg(
                "gate_name",
                &format!(
                    "{value:?} is not a gate; expected one of: {}. Nothing was written.",
                    GATE_NAMES.join(", ")
                ),
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let decision = match require_str(args, "decision").map(GateDecision::parse) {
        Ok(Some(decision)) => decision,
        Ok(None) => {
            return Ok(invalid_arg(
                "decision",
                "must be approve or reject. Nothing was written.",
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let summary = match require_str(args, "summary") {
        Ok(value) if !value.trim().is_empty() => value.to_string(),
        Ok(_) => {
            return Ok(invalid_arg(
                "summary",
                "must say what was shown and decided. Nothing was written.",
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let user_words = match require_str(args, "user_words") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let project = match resolve_project_dir(&project_arg) {
        Ok(project) => project,
        Err(reason) => return Ok(invalid_arg("project_dir", &reason)),
    };
    let request = GateRequest {
        job_id,
        gate,
        decision,
        summary,
        user_words,
    };
    Ok(tokio::task::spawn_blocking(move || {
        transact_then_record(&project, |flow_dir, state_path, current| {
            apply_gate(&project, flow_dir, state_path, current, &request)
        })
    })
    .await?)
}

/// Record the decision: every check first, then the new `STATE.md`; the gate
/// file and the log entry are returned for the caller to write once it is
/// committed (Fix round 1, DECISION C). The phase never moves — leaving the
/// gate is `flow_advance`'s.
fn apply_gate(
    project: &Path,
    flow_dir: &Path,
    state_path: &Path,
    current: &str,
    request: &GateRequest,
) -> Result<(String, Committed), CallToolResult> {
    let mut state = load_job(state_path, current, &request.job_id)?;
    let gate_token = format!("gate:{}", request.gate);
    if state.phase != gate_token {
        return Err(invalid_arg(
            "gate_name",
            &format!(
                "the job is at {:?}, not at {gate_token:?}; a gate decision is recorded only \
                 while the job stands at that gate. Nothing was written.",
                state.phase
            ),
        ));
    }
    let at = crate::tools::photo_intake::now_rfc3339_utc();
    let (approval, removed) = match request.decision {
        GateDecision::Approve => {
            let approval = check_approval(project, flow_dir, &state, request, &gate_token, &at)?;
            (Some(approval), false)
        }
        GateDecision::Reject => (None, state.gate_approvals.contains_key(&request.gate)),
    };

    // Every check passed. The gate file and the log entry are composed here
    // and written by the caller only after STATE.md commits (DECISION C).
    let gate_file_name = format!("{}.md", request.gate);
    let gate_path = flow_dir
        .join(RECORDS_DIR)
        .join(GATES_DIR)
        .join(&gate_file_name);
    let history_package = current_visit(&state)
        .and_then(|visit| state.history.get(visit))
        .map(|entry| &entry.package_files);
    let file = gate_file_text(&state, request, &at, approval.as_ref(), history_package);

    let mut lines = Vec::new();
    if let Some(approval) = &approval {
        lines.push(format!(
            "approved by {} · visit {}",
            approval.approved_by.as_str(),
            approval.visit
        ));
        lines.push(format!(
            "design_hash: `{}`",
            approval.design_hash_at_approval
        ));
        lines.push(format!(
            "package_hash: `{}`",
            approval.package_hash_at_approval
        ));
    } else {
        lines.push(format!("removed approval: {removed}"));
    }
    lines.push(format!("summary: {}", one_line(&request.summary)));
    lines.push(format!("user_words: {}", one_line(&request.user_words)));
    let log = log_entry(
        &at,
        &format!("gate {} {}", request.gate, request.decision.as_str()),
        &lines,
    );

    let response = json!({
        "job_id": state.job_id,
        "gate": request.gate,
        "decision": request.decision,
        "phase": state.phase,
        "approved_by": approval.as_ref().map(|approval| approval.approved_by),
        "approved_at": approval.as_ref().map(|approval| approval.approved_at.clone()),
        "visit": approval.as_ref().map(|approval| approval.visit),
        "design_hash_at_approval": approval
            .as_ref()
            .map(|approval| approval.design_hash_at_approval.clone()),
        "package_hash_at_approval": approval
            .as_ref()
            .map(|approval| approval.package_hash_at_approval.clone()),
        "removed_approval": removed,
        "gate_file": gate_path.display().to_string(),
    });
    match approval {
        Some(approval) => {
            state.gate_approvals.insert(request.gate.clone(), approval);
        }
        None => {
            state.gate_approvals.remove(&request.gate);
        }
    }
    let committed = Committed {
        response,
        gate_file: Some((gate_file_name, file)),
        log: (log_file_name(&state), log),
    };
    Ok((render_state(&state), committed))
}

/// The approval rules, in order (D1, D3, D5, D11): the user's own words
/// unless an autonomous session approves `architecture` or `placement`;
/// `architecture.md`'s readiness line for `architecture`; and both keys equal
/// to those recorded by the history entry that ENTERED this gate phase — the
/// human approves what was produced and shown, never whatever the files
/// became meanwhile.
fn check_approval(
    project: &Path,
    flow_dir: &Path,
    state: &JobState,
    request: &GateRequest,
    gate_token: &str,
    at: &str,
) -> Result<GateApproval, CallToolResult> {
    let session_approval = request.user_words.trim().is_empty();
    if session_approval && (state.mode == Mode::Guided || request.gate == "purchase") {
        return Err(invalid_arg(
            "user_words",
            &format!(
                "approving {:?} needs the user's own words, quoted verbatim — {}. Nothing was \
                 written.",
                request.gate,
                if request.gate == "purchase" {
                    "purchase always does, in every mode"
                } else {
                    "this job is guided"
                }
            ),
        ));
    }
    if request.gate == "architecture" {
        let path = flow_dir.join(RECORDS_DIR).join(ARCHITECTURE_RECORD);
        let content = match std::fs::read(&path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(CallToolResult::error(format!(
                    "Could not read {} ({error}). Nothing was written.",
                    path.display()
                )))
            }
        };
        let problem = match parse_readiness(&content) {
            Readiness::Pass => None,
            Readiness::Blocked(reason) => Some(format!("its readiness line is BLOCKED — {reason}")),
            Readiness::Malformed => {
                Some("its last non-empty line is not a readiness line".to_string())
            }
        };
        if let Some(problem) = problem {
            return Err(invalid_arg(
                "decision",
                &format!(
                    "records/{ARCHITECTURE_RECORD} must end with `Readiness: PASS` to approve \
                     architecture; {problem}. Nothing was written."
                ),
            ));
        }
    }

    let Some(visit) = current_visit(state) else {
        return Err(stale(
            gate_token,
            format!(
                "no history entry entered {gate_token}, so there are no keys to approve \
                 against; rewind and re-enter it. Nothing was written."
            ),
        ));
    };
    let entered = &state.history[visit];
    let design_hash = hash_design(project)?;
    if design_hash != entered.design_hash {
        return Err(stale(
            gate_token,
            format!(
                "the design changed after the job entered {gate_token} (design_hash {} then, \
                 {} now), so the user was not shown this state; rewind to the phase that \
                 produced it and re-enter the gate. Nothing was written.",
                short_hash(&entered.design_hash),
                short_hash(&design_hash)
            ),
        ));
    }
    let package = current_package(flow_dir, &state.phases, gate_token).map_err(|error| {
        CallToolResult::error(format!(
            "Could not hash the package of {gate_token}: {error}. Nothing was written."
        ))
    })?;
    if entered.package_hash.as_deref() != Some(package.hash.as_str()) {
        return Err(stale(
            gate_token,
            format!(
                "the package of {gate_token} changed after the job entered it (changed or \
                 missing: {}), so the user was not shown this state; rewind to the phase that \
                 produced it and re-enter the gate. Nothing was written.",
                changed_package_files(Some(&entered.package_files), &package.files)
            ),
        ));
    }
    Ok(GateApproval {
        decision: GateDecision::Approve,
        approved_by: if session_approval {
            ApprovedBy::Session
        } else {
            ApprovedBy::User
        },
        approved_at: at.to_string(),
        design_hash_at_approval: design_hash,
        package_hash_at_approval: package.hash,
        visit,
        summary: request.summary.clone(),
        user_words: request.user_words.clone(),
    })
}

/// `records/gates/<gate>.md`: the latest decision on the gate, with the
/// user's words verbatim (blockquoted, so no line can forge a heading). Each
/// earlier decision stays in the job log.
fn gate_file_text(
    state: &JobState,
    request: &GateRequest,
    at: &str,
    approval: Option<&GateApproval>,
    package_files: Option<&BTreeMap<String, Option<String>>>,
) -> String {
    let mut text = format!(
        "# Gate `{}` — {}\n\n- Job: `{}`\n- At: {at}\n",
        request.gate,
        request.decision.as_str(),
        state.job_id
    );
    if let Some(approval) = approval {
        text.push_str(&format!(
            "- Approved by: {}\n- Visit: {} (the history entry that entered gate:{})\n\
             - design_hash: `{}`\n- package_hash: `{}`\n",
            approval.approved_by.as_str(),
            approval.visit,
            request.gate,
            approval.design_hash_at_approval,
            approval.package_hash_at_approval
        ));
        for (name, digest) in package_files.into_iter().flatten() {
            text.push_str(&format!(
                "  - {name}: {}\n",
                digest
                    .as_deref()
                    .map_or("absent".to_string(), |digest| format!("`{digest}`"))
            ));
        }
    }
    text.push_str(&format!(
        "\n## Summary\n\n{}\n\n## User's words\n\n{}\n",
        blockquote(&request.summary),
        match (request.user_words.trim().is_empty(), approval) {
            (true, Some(_)) => "(none — approved by the session under autonomous mode)".to_string(),
            (true, None) => "(none)".to_string(),
            (false, _) => blockquote(&request.user_words),
        }
    ));
    text
}

/// Every line prefixed with `> `: free text kept readable and verbatim, yet
/// unable to open a heading or a list of its own.
fn blockquote(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── flow_log and flow_defer (design D1, D7) ──────────────────────────────────

/// `flow_log`'s kinds: `decision`/`evidence` → the job log; `lesson` →
/// `memory/<role>.md` or the candidates queue by `scope`; `handoff` → a new
/// numbered file. Each destination has exactly this one writer.
const LOG_KINDS: [&str; 4] = ["decision", "evidence", "lesson", "handoff"];

/// A lesson's `scope` (D7 triage): `project` stays in `memory/<role>.md`,
/// `role` and `technology` queue in `lessons-candidates.md` for the curator.
const LESSON_SCOPES: [&str; 3] = ["role", "technology", "project"];

/// `flow_defer`'s kinds: `finding` → `deferred_findings`, `queue_item` →
/// `queue`, `pending_approval` → `pending_approvals` (see `apply_defer`).
const DEFER_KINDS: [&str; 3] = ["finding", "queue_item", "pending_approval"];

/// `(parameter, kinds it applies to, kinds that require it)` — D1: a
/// parameter outside its kinds is refused, never silently dropped. Checked in
/// this order, so a refusal names the first offending parameter.
const LOG_PARAMETERS: [(&str, &[&str], &[&str]); 4] = [
    ("why", &["decision"], &["decision"]),
    ("rollback", &["decision"], &["decision"]),
    (
        "role",
        &["decision", "evidence", "lesson", "handoff"],
        &["lesson", "handoff"],
    ),
    ("scope", &["lesson"], &["lesson"]),
];

/// A validated `flow_log` call, before it meets the state file.
struct LogRequest {
    job_id: String,
    kind: String,
    message: String,
    why: Option<String>,
    rollback: Option<String>,
    role: Option<String>,
    scope: Option<String>,
}

/// An optional string argument: absent, null or blank is `None`; any other
/// type is an argument error.
fn opt_text(args: &Value, key: &str) -> Result<Option<String>, CallToolResult> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone()).filter(|text| !text.trim().is_empty())),
        Some(_) => Err(invalid_arg(key, "must be a string")),
    }
}

/// An optional closed-vocabulary argument (the schema enforces the enum; the
/// handler re-checks, as every flow handler does).
fn opt_token(args: &Value, key: &str, allowed: &[&str]) -> Result<Option<String>, CallToolResult> {
    match opt_text(args, key)? {
        Some(token) if !allowed.contains(&token.as_str()) => Err(invalid_arg(
            key,
            &format!(
                "{token:?} is not one of: {}. Nothing was written.",
                allowed.join(", ")
            ),
        )),
        token => Ok(token),
    }
}

async fn handle_flow_log(args: &Value, _ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let job_id = match require_str(args, "job_id") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let kind = match require_str(args, "kind") {
        Ok(value) if LOG_KINDS.contains(&value) => value.to_string(),
        Ok(value) => {
            return Ok(invalid_arg(
                "kind",
                &format!(
                    "{value:?} is not one of: {}. Nothing was written.",
                    LOG_KINDS.join(", ")
                ),
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let message = match require_str(args, "message") {
        Ok(value) if !value.trim().is_empty() => value.to_string(),
        Ok(_) => {
            return Ok(invalid_arg(
                "message",
                "must not be empty. Nothing was written.",
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let request = match log_request(args, job_id, kind, message) {
        Ok(request) => request,
        Err(rejection) => return Ok(rejection),
    };
    let project = match resolve_project_dir(&project_arg) {
        Ok(project) => project,
        Err(reason) => return Ok(invalid_arg("project_dir", &reason)),
    };
    Ok(tokio::task::spawn_blocking(move || {
        transact_state(&project, |flow_dir, state_path, current| {
            apply_log(flow_dir, state_path, current, &request)
        })
    })
    .await?)
}

/// The optional parameters, typed and checked against [`LOG_PARAMETERS`].
fn log_request(
    args: &Value,
    job_id: String,
    kind: String,
    message: String,
) -> Result<LogRequest, CallToolResult> {
    let request = LogRequest {
        job_id,
        kind,
        message,
        why: opt_text(args, "why")?,
        rollback: opt_text(args, "rollback")?,
        role: opt_token(args, "role", &ROLES)?,
        scope: opt_token(args, "scope", &LESSON_SCOPES)?,
    };
    check_log_parameters(&request)?;
    Ok(request)
}

/// [`LOG_PARAMETERS`] against the call: present outside its kinds, or
/// absent (or blank) where its kind requires it, is refused.
fn check_log_parameters(request: &LogRequest) -> Result<(), CallToolResult> {
    let kind = request.kind.as_str();
    for (parameter, applies_to, required_by) in LOG_PARAMETERS {
        let present = match parameter {
            "why" => request.why.is_some(),
            "rollback" => request.rollback.is_some(),
            "role" => request.role.is_some(),
            _ => request.scope.is_some(),
        };
        if present && !applies_to.contains(&kind) {
            return Err(invalid_arg(
                parameter,
                &format!(
                    "does not apply to kind {kind} (only to: {}). Nothing was written.",
                    applies_to.join(", ")
                ),
            ));
        }
        if !present && required_by.contains(&kind) {
            return Err(invalid_arg(
                parameter,
                &format!("kind {kind} requires a non-empty {parameter}. Nothing was written."),
            ));
        }
    }
    Ok(())
}

/// Append the entry to the one file its kind owns, under the `STATE.md` lock
/// (so handoff numbering cannot race), and hand `STATE.md` back unchanged.
/// Allowed on a closed job: the journal outlives the transitions.
fn apply_log(
    flow_dir: &Path,
    state_path: &Path,
    current: &str,
    request: &LogRequest,
) -> Result<(String, Value), CallToolResult> {
    let state = load_job(state_path, current, &request.job_id)?;
    let at = crate::tools::photo_intake::now_rfc3339_utc();
    let role = request.role.as_deref().unwrap_or_default();
    let (path, read_name) = match (request.kind.as_str(), request.scope.as_deref()) {
        ("decision" | "evidence", _) => {
            let mut lines = Vec::new();
            if let Some(why) = &request.why {
                lines.push(format!("why: {}", one_line(why)));
            }
            if let Some(rollback) = &request.rollback {
                lines.push(format!("rollback: {}", one_line(rollback)));
            }
            let title = match &request.role {
                Some(role) => format!("{} · role `{role}`", request.kind),
                None => request.kind.clone(),
            };
            let dir = ensure_flow_subdir(flow_dir, &[LOG_DIR]).map_err(CallToolResult::error)?;
            let path = dir.join(log_file_name(&state));
            append_file(&path, &journal_entry(&at, &title, &request.message, &lines))
                .map_err(CallToolResult::error)?;
            (path, "log".to_string())
        }
        ("lesson", Some("project")) => {
            let dir = ensure_flow_subdir(flow_dir, &[MEMORY_DIR]).map_err(CallToolResult::error)?;
            let file = format!("{role}.md");
            let path = dir.join(&file);
            let title = format!("lesson · job `{}`", state.job_id);
            append_file(&path, &journal_entry(&at, &title, &request.message, &[]))
                .map_err(CallToolResult::error)?;
            (path, format!("{MEMORY_DIR}/{file}"))
        }
        ("lesson", Some(scope)) => {
            let dir =
                ensure_flow_subdir(flow_dir, &[RECORDS_DIR]).map_err(CallToolResult::error)?;
            let path = dir.join(LESSONS_FILE);
            let title = format!(
                "lesson · role `{role}` · scope `{scope}` · job `{}`",
                state.job_id
            );
            append_file(&path, &journal_entry(&at, &title, &request.message, &[]))
                .map_err(CallToolResult::error)?;
            (path, LESSONS_FILE.to_string())
        }
        ("handoff", _) => {
            let dir = ensure_flow_subdir(flow_dir, &[HANDOFFS_DIR, state.job_id.as_str()])
                .map_err(CallToolResult::error)?;
            let file = format!(
                "{:02}-{role}.md",
                next_handoff_number(flow_dir, &state.job_id)
            );
            let path = dir.join(&file);
            konnect_sexp::write_new_atomic(&path, &request.message).map_err(|error| {
                CallToolResult::error(format!("Could not create {} ({error})", path.display()))
            })?;
            (path, format!("{HANDOFFS_DIR}/{file}"))
        }
        _ => {
            return Err(invalid_arg(
                "kind",
                "has no destination for these parameters. Nothing was written.",
            ))
        }
    };
    let response = json!({
        "job_id": state.job_id,
        "kind": request.kind,
        "at": at,
        "read_name": read_name,
        "file": path.display().to_string(),
    });
    Ok((current.to_string(), response))
}

/// One journal entry: a timestamped heading, the message blockquoted (kept
/// verbatim, unable to forge a heading), then metadata bullets.
fn journal_entry(at: &str, title: &str, message: &str, lines: &[String]) -> String {
    let mut entry = format!("## {at} · {title}\n\n{}\n\n", blockquote(message));
    for line in lines {
        entry.push_str(&format!("- {line}\n"));
    }
    if !lines.is_empty() {
        entry.push('\n');
    }
    entry
}

/// One past the highest handoff number in the job's directory: the existing
/// count + 1 while none was removed, and never a number already taken when
/// one was (so the no-clobber create cannot collide).
fn next_handoff_number(flow_dir: &Path, job_id: &str) -> usize {
    list_handoffs(flow_dir, job_id)
        .iter()
        .filter_map(|name| {
            name.split_once('-')
                .and_then(|(number, _)| number.parse::<usize>().ok())
        })
        .max()
        .unwrap_or(0)
        + 1
}

/// A validated `flow_defer` call.
struct DeferRequest {
    job_id: String,
    kind: &'static str,
    description: String,
    owner: Option<String>,
}

async fn handle_flow_defer(args: &Value, _ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let job_id = match require_str(args, "job_id") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let kind = match require_str(args, "kind") {
        Ok(value) => match DEFER_KINDS.iter().find(|kind| **kind == value) {
            Some(kind) => *kind,
            None => {
                return Ok(invalid_arg(
                    "kind",
                    "must be finding, queue_item or pending_approval. Nothing was written.",
                ))
            }
        },
        Err(rejection) => return Ok(rejection),
    };
    let description = match require_str(args, "description") {
        Ok(value) if !value.trim().is_empty() => value.to_string(),
        Ok(_) => {
            return Ok(invalid_arg(
                "description",
                "must not be empty. Nothing was written.",
            ))
        }
        Err(rejection) => return Ok(rejection),
    };
    let owner = match opt_text(args, "owner") {
        Ok(owner) => owner,
        Err(rejection) => return Ok(rejection),
    };
    let project = match resolve_project_dir(&project_arg) {
        Ok(project) => project,
        Err(reason) => return Ok(invalid_arg("project_dir", &reason)),
    };
    let request = DeferRequest {
        job_id,
        kind,
        description,
        owner,
    };
    Ok(tokio::task::spawn_blocking(move || {
        transact_then_record(&project, |_, state_path, current| {
            apply_defer(state_path, current, &request)
        })
    })
    .await?)
}

/// Append the item to its list under the `STATE.md` lock, in any phase —
/// `closed` included; lists are append-only (resolution is a `flow_log`).
/// The log entry is returned, not written: it follows the commit (D2, Fix
/// round 1, DECISION H).
fn apply_defer(
    state_path: &Path,
    current: &str,
    request: &DeferRequest,
) -> Result<(String, Committed), CallToolResult> {
    let mut state = load_job(state_path, current, &request.job_id)?;
    let at = crate::tools::photo_intake::now_rfc3339_utc();
    let item = DeferredItem {
        description: request.description.clone(),
        owner: request.owner.clone(),
        added_at: at.clone(),
        phase: state.phase.clone(),
    };
    let (list_name, list) = match request.kind {
        "finding" => ("deferred_findings", &mut state.deferred_findings),
        "queue_item" => ("queue", &mut state.queue),
        _ => ("pending_approvals", &mut state.pending_approvals),
    };
    list.push(item.clone());
    let count = list.len();

    let mut lines = vec![
        format!("description: {}", one_line(&item.description)),
        format!("phase: `{}`", item.phase),
    ];
    if let Some(owner) = &item.owner {
        lines.push(format!("owner: {}", one_line(owner)));
    }
    let log = log_entry(&at, &format!("defer {}", request.kind), &lines);
    let committed = Committed {
        response: json!({
            "job_id": state.job_id,
            "kind": request.kind,
            "list": list_name,
            "count": count,
            "item": item,
        }),
        gate_file: None,
        log: (log_file_name(&state), log),
    };
    Ok((render_state(&state), committed))
}

// ─── Tool definitions (design D1) ─────────────────────────────────────────────

/// The `flow` tools in D1 order. The router registers them (task 1.8).
pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "flow_status",
            "Read a KiCad project's orchestration state and the reality it binds to: the \
         job and its phase (null when none), the current design_state_hash and the files \
         it covered, KiCad lock files beside them, gate approvals (`valid` is recomputed \
         only for the gate the job stands at now; a passed gate reports status: passed \
         and no `valid`), deferred items, FIX rounds per review phase, the newest \
         transition, the job's handoffs, the next step, and the content of each requested \
         `read` name. \
         Creates nothing and never refuses because of flow state — an unparseable \
         STATE.md is reported in `state_error`.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory: must hold a *.kicad_pro directly."
                    },
                    "read": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Records to return in `contents` (absent ones are listed in `missing`): a record name such as constraints.md or architecture.md, lessons-candidates.md, gates/<architecture|placement|purchase>.md, log (this job's log), memory/<role>.md, or handoffs/<NN>-<role>.md as `handoffs` lists them."
                    }
                },
                "required": ["project_dir"]
            }),
            |args, ctx| async move { handle_flow_status(args, ctx).await }
        ),
        tool!(
            "flow_start",
            "Open an orchestration job on a KiCad project: validates the lane's phase \
             sequence, writes .konnect/flow/STATE.md with the first phase and a start \
             history entry carrying the current design_state_hash, and logs it. Only one \
             job at a time: a job that is not closed is a conflict (close or abandon it \
             with flow_advance first). Returns the server-minted job_id every later flow \
             call names.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory: must hold a *.kicad_pro directly."
                    },
                    "objective": {
                        "type": "string",
                        "description": "What the job delivers, in the user's terms. Also seeds the job_id slug."
                    },
                    "lane": {
                        "type": "string",
                        "enum": Lane::ALL.map(Lane::as_str),
                        "description": "new_board runs the full sequence by default; every other lane must pass `phases`."
                    },
                    "phases": {
                        "type": "array",
                        "items": { "type": "string", "enum": CANONICAL_PHASES },
                        "description": "A strictly increasing subsequence of the canonical order that never starts at a gate and keeps each gate its phase brings (architecture, placement, manufacturing → purchase). Required unless lane is new_board."
                    },
                    "mode": {
                        "type": "string",
                        "enum": Mode::ALL.map(Mode::as_str),
                        "description": "guided (default): every gate needs the user's own words. autonomous: architecture and placement may be approved by the session; purchase still needs the user."
                    }
                },
                "required": ["project_dir", "objective", "lane"]
            }),
            |args, ctx| async move { handle_flow_start(args, ctx).await }
        ),
        tool!(
            "flow_advance",
            "Move the project's job to `to_phase`. Forward (the next entry of the job's \
             phases, or closed from the last one) requires every record the phase being left \
             produces in THIS call's `records` — a record already on disk never counts — and \
             each supplied record must belong to that phase; leaving architecture requires \
             architecture.md to end with `Readiness: PASS`; leaving a gate requires an \
             approval from this visit whose design and package hashes still match. Records \
             are written only when the whole transition is accepted; \
             a validation refusal writes nothing. A success always carries `warning`: null, \
             or a string naming the log entry not written after STATE.md committed — the \
             move stands, so never repeat the call. The producer of a phase calls this as \
             its last action.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory: must hold a *.kicad_pro directly."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "The job_id flow_start returned (flow_status reports it)."
                    },
                    "to_phase": {
                        "type": "string",
                        "enum": CANONICAL_PHASES.iter().copied().chain([CLOSED]).collect::<Vec<_>>(),
                        "description": "The next phase of the job's sequence, or closed from its last phase."
                    },
                    "records": {
                        "type": "array",
                        "description": "The records of the phase being left, as Markdown text. Written to .konnect/flow/records/<filename>.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "filename": { "type": "string", "enum": RECORD_NAMES },
                                "content": { "type": "string" }
                            },
                            "required": ["filename", "content"]
                        }
                    },
                    "evidence_calls": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tool names whose results this phase's records cite (for example run_erc). Stored in the history entry."
                    },
                    "reason": {
                        "type": "string",
                        "description": "Why the move is made; stored in the history entry."
                    }
                },
                "required": ["project_dir", "job_id", "to_phase"]
            }),
            |args, ctx| async move { handle_flow_advance(args, ctx).await }
        ),
        tool!(
            "flow_gate",
            "Record the user's decision on the gate the job stands at (gate:<gate_name>). \
             approve binds to what was shown: the design_state_hash and the package hash \
             recorded when the job entered the gate must still match (a design or record \
             changed since is stale_target, naming the changed records); architecture also \
             needs architecture.md to end with `Readiness: PASS`; user_words must quote the \
             user verbatim, except that an autonomous job may approve architecture or \
             placement with empty user_words (recorded as approved_by: session) — purchase \
             always needs them. reject removes any approval. Writes \
             records/gates/<gate_name>.md and logs it; never moves the phase. A success \
             always carries `warning`: null, or a string naming a side file not written \
             after STATE.md committed — the decision stands, so never repeat the call.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory: must hold a *.kicad_pro directly."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "The job_id flow_start returned (flow_status reports it)."
                    },
                    "gate_name": {
                        "type": "string",
                        "enum": GATE_NAMES,
                        "description": "The gate the job stands at: its phase is gate:<gate_name>."
                    },
                    "decision": {
                        "type": "string",
                        "enum": GateDecision::ALL.map(GateDecision::as_str),
                        "description": "approve records a hash-bound approval; reject removes any approval of this gate."
                    },
                    "summary": {
                        "type": "string",
                        "description": "What was shown to the user and what was decided (under autonomous mode, the session's reasoning). Must not be empty."
                    },
                    "user_words": {
                        "type": "string",
                        "description": "The user's own words, verbatim. Empty only when an autonomous job approves architecture or placement."
                    }
                },
                "required": ["project_dir", "job_id", "gate_name", "decision", "summary", "user_words"]
            }),
            |args, ctx| async move { handle_flow_gate(args, ctx).await }
        ),
        tool!(
            "flow_log",
            "Append one journal entry for the project's job (active or closed); `kind` picks \
             the one file that owns it: decision (needs why and rollback) or evidence → the \
             job log; lesson (needs role and scope) → memory/<role>.md for scope project, \
             records/lessons-candidates.md for scope role or technology; handoff (needs \
             role) → a new handoffs/<job_id>/<NN>-<role>.md holding `message` verbatim. A \
             parameter that does not apply to the kind is refused. Returns `read_name`, the \
             name flow_status(read) takes. Never changes STATE.md.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory: must hold a *.kicad_pro directly."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "The job_id flow_start returned (flow_status reports it)."
                    },
                    "kind": {
                        "type": "string",
                        "enum": LOG_KINDS,
                        "description": "decision | evidence → the job log; lesson → project memory or the candidates queue by scope; handoff → a new numbered handoff file."
                    },
                    "message": {
                        "type": "string",
                        "description": "The entry as Markdown; for a handoff, the whole handoff file."
                    },
                    "why": {
                        "type": "string",
                        "description": "decision only, required: why it was decided."
                    },
                    "rollback": {
                        "type": "string",
                        "description": "decision only, required: how to undo it."
                    },
                    "role": {
                        "type": "string",
                        "enum": ROLES,
                        "description": "Required for lesson (the memory file) and handoff (the file name); an optional author tag on decision and evidence."
                    },
                    "scope": {
                        "type": "string",
                        "enum": LESSON_SCOPES,
                        "description": "lesson only, required: project (stays in memory/<role>.md), role or technology (queued in lessons-candidates.md)."
                    }
                },
                "required": ["project_dir", "job_id", "kind", "message"]
            }),
            |args, ctx| async move { handle_flow_log(args, ctx).await }
        ),
        tool!(
            "flow_defer",
            "Append an item to the job's STATE.md list — finding → deferred_findings, \
             queue_item → queue, pending_approval → pending_approvals — with the phase it was \
             found in, and log it. Works in any phase, closed included; lists are \
             append-only (record a resolution with flow_log). Refuses only a foreign job_id \
             or an empty description. A success always carries `warning`: null, or a string \
             naming the log entry not written after STATE.md committed — the item is \
             recorded, so never repeat the call.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory: must hold a *.kicad_pro directly."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "The job_id flow_start returned (flow_status reports it)."
                    },
                    "kind": {
                        "type": "string",
                        "enum": DEFER_KINDS,
                        "description": "finding, queue_item or pending_approval."
                    },
                    "description": {
                        "type": "string",
                        "description": "The item, in one or two sentences. Must not be empty."
                    },
                    "owner": {
                        "type": "string",
                        "description": "Who should pick it up (optional)."
                    }
                },
                "required": ["project_dir", "job_id", "kind", "description"]
            }),
            |args, ctx| async move { handle_flow_defer(args, ctx).await }
        ),
    ]
}

// ─── Tests: foundations (task 1.1) ────────────────────────────────────────────

#[cfg(test)]
mod foundation_tests {
    use super::*;

    const HOSTILE_OBJECTIVE: &str = "Placa ---\n---\n\"aspas\": conversão `crase` e fim: ok";

    fn owned(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| token.to_string()).collect()
    }

    fn full_state(objective: &str) -> JobState {
        let mut entered_gate = HistoryEntry::new(
            HistoryKind::Advance,
            Some("architecture"),
            "gate:architecture",
            "2026-09-21T14:40:00Z",
            "9f2c",
        );
        entered_gate.package_hash = Some("ab12".into());
        entered_gate.package_files = BTreeMap::from([
            ("architecture.md".to_string(), Some("cd34".to_string())),
            ("constraints.md".to_string(), None),
        ]);
        entered_gate.records = owned(&["architecture.md", "worst-case.md", "pin-plan.md"]);
        entered_gate.evidence_calls = owned(&["run_erc"]);
        entered_gate.evidence_check = Some(EvidenceCheck {
            confirmed: owned(&["run_erc"]),
            not_ok: Vec::new(),
            absent: owned(&["render_schematic_png"]),
            ring_calls: 7,
        });
        entered_gate.reason = Some("a reason: with `ticks`\nand a newline".into());
        JobState {
            schema: STATE_SCHEMA,
            job_id: "placa-20260921-140000".into(),
            objective: objective.into(),
            lane: Lane::NewBoard,
            mode: Mode::Autonomous,
            phases: owned(&CANONICAL_PHASES),
            phase: "gate:architecture".into(),
            started_at: "2026-09-21T14:00:00Z".into(),
            gate_approvals: BTreeMap::from([(
                "architecture".to_string(),
                GateApproval {
                    decision: GateDecision::Approve,
                    approved_by: ApprovedBy::Session,
                    approved_at: "2026-09-21T15:00:00Z".into(),
                    design_hash_at_approval: "9f2c".into(),
                    package_hash_at_approval: "ab12".into(),
                    visit: 2,
                    summary: "looks right".into(),
                    user_words: String::new(),
                },
            )]),
            pending_approvals: vec![DeferredItem {
                description: "buy the reel".into(),
                owner: None,
                added_at: "2026-09-21T14:10:00Z".into(),
                phase: "requirements".into(),
            }],
            deferred_findings: vec![DeferredItem {
                description: "silk `R3` overlaps | pad".into(),
                owner: Some("layout".into()),
                added_at: "2026-09-21T14:11:00Z".into(),
                phase: "architecture".into(),
            }],
            queue: Vec::new(),
            history: vec![
                HistoryEntry::new(
                    HistoryKind::Start,
                    None,
                    "requirements",
                    "2026-09-21T14:00:00Z",
                    "9f2c",
                ),
                HistoryEntry::new(
                    HistoryKind::Advance,
                    Some("requirements"),
                    "architecture",
                    "2026-09-21T14:20:00Z",
                    "9f2c",
                ),
                entered_gate,
            ],
        }
    }

    #[test]
    fn a_hostile_objective_round_trips_through_state_md() {
        let state = full_state(HOSTILE_OBJECTIVE);
        let text = render_state(&state);
        assert!(text.starts_with("---\n{"), "front matter opens the file");

        let parsed = parse_state(&text).expect("the rendered state parses");
        assert_eq!(parsed.objective, HOSTILE_OBJECTIVE);
        assert_eq!(parsed, state);

        // git autocrlf may hand the parser CRLF; JSON escapes the objective's
        // own newlines, so only the fences and layout change.
        let crlf = text.replace('\n', "\r\n");
        assert_eq!(parse_state(&crlf).expect("CRLF parses"), state);
    }

    #[test]
    fn the_body_is_regenerated_and_never_parsed() {
        let state = full_state("plain");
        let text = render_state(&state);
        assert!(
            text.contains("regenerated"),
            "the body says it is regenerated"
        );
        let edited = format!("{text}\nA hand edit.\n---\n{{\"schema\": 9}}\n---\n");
        assert_eq!(parse_state(&edited).expect("body edits are ignored"), state);
    }

    #[test]
    fn absent_optional_fields_stay_absent() {
        let text = render_state(&full_state("plain"));
        let start = &text[text.find("\"kind\": \"start\"").expect("start entry")..];
        let start_entry = &start[..start.find('}').expect("entry closes")];
        for absent in ["from", "package_hash", "package_files", "records", "reason"] {
            assert!(
                !start_entry.contains(&format!("\"{absent}\"")),
                "{absent} must not be serialized when empty: {start_entry}"
            );
        }
    }

    #[test]
    fn front_matter_edits_are_refused_not_guessed() {
        let text = render_state(&full_state("plain"));

        let unknown = text.replacen("\"schema\": 1,", "\"schema\": 1,\n  \"surprise\": true,", 1);
        let error = parse_state(&unknown).expect_err("unknown field refused");
        assert!(error.contains("surprise"), "{error}");

        let newer = text.replacen("\"schema\": 1,", "\"schema\": 2,", 1);
        let error = parse_state(&newer).expect_err("newer schema refused");
        assert!(error.contains("schema"), "{error}");

        let broken = text.replacen("\"schema\": 1,", "\"schema\": 1,,", 1);
        assert!(parse_state(&broken).is_err());

        let traversal = text.replacen(
            "\"job_id\": \"placa-20260921-140000\"",
            "\"job_id\": \"../../escape\"",
            1,
        );
        let error = parse_state(&traversal).expect_err("a path-unsafe job_id is refused");
        assert!(error.contains("job_id"), "{error}");

        let foreign_phase = text.replacen(
            "\"phase\": \"gate:architecture\"",
            "\"phase\": \"somewhere\"",
            1,
        );
        let error = parse_state(&foreign_phase).expect_err("a phase outside the job is refused");
        assert!(error.contains("somewhere"), "{error}");

        assert!(parse_state("no front matter").is_err());
        assert!(
            parse_state("---\n{}\n").is_err(),
            "an unclosed fence is refused"
        );
    }

    #[test]
    fn the_validator_accepts_every_documented_lane_sequence() {
        let lanes: [&[&str]; 5] = [
            &CANONICAL_PHASES,
            &[
                "architecture",
                "gate:architecture",
                "schematic",
                "schematic_review",
                "prefab_review",
                "manufacturing",
                "gate:purchase",
            ],
            &["prefab_review"],
            &["manufacturing", "gate:purchase"],
            &[
                "schematic",
                "schematic_review",
                "placement",
                "gate:placement",
                "routing",
                "prefab_review",
                "manufacturing",
                "gate:purchase",
                "learn",
            ],
        ];
        for lane in lanes {
            assert_eq!(validate_phases(&owned(lane)), Ok(()), "{lane:?}");
        }
    }

    #[test]
    fn the_validator_names_the_entry_it_rejects() {
        let cases: [(&[&str], &str, &str); 7] = [
            (&["schematic", "requirements"], "\"requirements\"", "order"),
            (&["schematic", "schematic"], "\"schematic\"", "repeat"),
            (&["schematic", "layout"], "\"layout\"", "not a phase"),
            (&["gate:placement", "routing"], "\"gate:placement\"", "gate"),
            (&["placement", "routing"], "\"placement\"", "gate:placement"),
            // Reviewer 11's two shapes: a gate whose producing phase is not in
            // the job would bind its approval to a leftover record.
            (
                &["routing", "prefab_review", "gate:purchase"],
                "\"gate:purchase\"",
                "manufacturing",
            ),
            (
                &["requirements", "gate:architecture", "schematic"],
                "\"gate:architecture\"",
                "architecture",
            ),
        ];
        for (phases, entry, rule) in cases {
            let error = validate_phases(&owned(phases)).expect_err("rejected");
            assert!(error.contains(entry), "{phases:?}: {error}");
            assert!(error.contains(rule), "{phases:?}: {error}");
        }
        assert!(
            validate_phases(&[]).is_err(),
            "an empty sequence is refused"
        );
    }

    #[test]
    fn readiness_accepts_only_an_exact_trimmed_pass_line() {
        assert_eq!(
            parse_readiness("# Arch\n\nReadiness: PASS\n"),
            Readiness::Pass
        );
        assert_eq!(
            parse_readiness("# Arch\r\n   Readiness: PASS \t\r\n\r\n\n"),
            Readiness::Pass
        );
        for malformed in [
            "",
            "# Arch\nno readiness line\n",
            "Readiness: PASS\nlater text\n",
            "readiness: pass",
            "Readiness: PASS.",
            "Readiness: PASSED",
            "Readiness:PASS",
            "Readiness: BLOCKED",
            "Readiness: BLOCKEDX no gap",
        ] {
            assert_eq!(
                parse_readiness(malformed),
                Readiness::Malformed,
                "{malformed:?}"
            );
        }
    }

    #[test]
    fn a_blocked_readiness_line_carries_its_reason() {
        assert_eq!(
            parse_readiness("x\nReadiness: BLOCKED — input voltage range never given\n"),
            Readiness::Blocked("input voltage range never given".into())
        );
        assert_eq!(
            parse_readiness("Readiness: BLOCKED - no USB current budget"),
            Readiness::Blocked("no USB current budget".into())
        );
    }

    #[test]
    fn packages_are_derived_from_the_sequence_between_gates() {
        let full = owned(&CANONICAL_PHASES);
        assert_eq!(
            package_records(&full, "gate:architecture"),
            [
                "constraints.md",
                "architecture.md",
                "worst-case.md",
                "pin-plan.md"
            ]
        );
        assert_eq!(
            package_records(&full, "gate:placement"),
            [
                "schematic-evidence.md",
                "ledger-schematic.md",
                "placement.md"
            ]
        );
        assert_eq!(
            package_records(&full, "gate:purchase"),
            ["routing.md", "ledger-prefab.md", "manufacturing.md"]
        );
        let fab_only = owned(&["manufacturing", "gate:purchase"]);
        assert_eq!(
            package_records(&fab_only, "gate:purchase"),
            ["manufacturing.md"]
        );
        let revision = owned(&[
            "architecture",
            "gate:architecture",
            "schematic",
            "schematic_review",
            "prefab_review",
            "manufacturing",
            "gate:purchase",
        ]);
        assert_eq!(
            package_records(&revision, "gate:purchase"),
            [
                "schematic-evidence.md",
                "ledger-schematic.md",
                "ledger-prefab.md",
                "manufacturing.md"
            ]
        );
        assert!(package_records(&fab_only, "gate:placement").is_empty());
    }

    #[test]
    fn the_package_hash_sees_content_absence_and_not_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let names = ["constraints.md", "architecture.md"];
        let nothing = package_hash(dir.path(), &names).unwrap();
        assert_eq!(nothing.files.get("constraints.md"), Some(&None));

        std::fs::write(dir.path().join("constraints.md"), "").unwrap();
        let empty = package_hash(dir.path(), &names).unwrap();
        assert_ne!(
            empty.hash, nothing.hash,
            "an empty file is not an absent one"
        );

        std::fs::write(dir.path().join("constraints.md"), "a\nb\n").unwrap();
        let lf = package_hash(dir.path(), &names).unwrap();
        std::fs::write(dir.path().join("constraints.md"), "a\r\nb\r\n").unwrap();
        let crlf = package_hash(dir.path(), &names).unwrap();
        assert_eq!(lf, crlf, "line endings are not content");

        std::fs::write(dir.path().join("constraints.md"), "a\nc\n").unwrap();
        let changed = package_hash(dir.path(), &names).unwrap();
        assert_ne!(changed.hash, lf.hash);
        assert_ne!(
            changed.files["constraints.md"], lf.files["constraints.md"],
            "the per-file digest names what changed"
        );

        std::fs::remove_file(dir.path().join("constraints.md")).unwrap();
        assert_eq!(package_hash(dir.path(), &names).unwrap(), nothing);
    }

    #[test]
    fn project_dir_must_hold_a_top_level_kicad_project() {
        let dir = tempfile::tempdir().unwrap();
        let error = resolve_project_dir(dir.path().to_str().unwrap()).expect_err("no project");
        assert!(error.contains(".kicad_pro"), "{error}");

        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("demo.kicad_pro"), "{}").unwrap();
        std::fs::write(dir.path().join("~demo.kicad_pro.lck"), "lock").unwrap();
        assert!(
            resolve_project_dir(dir.path().to_str().unwrap()).is_err(),
            "a nested project or a lock file is not a top-level project"
        );

        std::fs::write(dir.path().join("demo.kicad_pro"), "{}").unwrap();
        let resolved = resolve_project_dir(dir.path().to_str().unwrap()).expect("project");
        assert_eq!(resolved, dir.path().canonicalize().unwrap());

        assert!(resolve_project_dir("").is_err());
        let missing = dir.path().join("missing");
        assert!(resolve_project_dir(missing.to_str().unwrap()).is_err());
    }

    #[test]
    fn only_the_creating_call_makes_the_flow_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("demo.kicad_pro"), "{}").unwrap();
        let project = resolve_project_dir(dir.path().to_str().unwrap()).unwrap();

        assert_eq!(existing_flow_dir(&project), Ok(None));
        assert!(
            !project.join(".konnect").exists(),
            "a lookup creates nothing"
        );

        let created = create_flow_dir(&project).expect("created");
        assert!(created.is_dir());
        assert!(created.starts_with(&project));
        assert_eq!(existing_flow_dir(&project), Ok(Some(created)));
    }

    /// The token tables are the single source; these guards fail by name when
    /// one of them drifts from another.
    #[test]
    fn the_vocabularies_agree_with_each_other() {
        for lane in Lane::ALL {
            assert_eq!(serde_json::to_value(lane).unwrap(), lane.as_str());
        }
        for mode in Mode::ALL {
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.as_str());
        }
        for decision in GateDecision::ALL {
            assert_eq!(serde_json::to_value(decision).unwrap(), decision.as_str());
            assert_eq!(GateDecision::parse(decision.as_str()), Some(decision));
        }
        for approved_by in [ApprovedBy::User, ApprovedBy::Session] {
            assert_eq!(
                serde_json::to_value(approved_by).unwrap(),
                approved_by.as_str()
            );
        }
        let flattened: Vec<&str> = PHASE_RECORDS
            .iter()
            .flat_map(|(_, records)| records.iter().copied())
            .collect();
        assert_eq!(flattened, RECORD_NAMES);
        for (phase, records) in PHASE_RECORDS {
            assert!(CANONICAL_PHASES.contains(&phase), "{phase}");
            assert_eq!(required_records(phase), records);
        }
        for (phase, gate) in GATED_PHASES {
            assert!(CANONICAL_PHASES.contains(&phase), "{phase}");
            assert!(CANONICAL_PHASES.contains(&gate), "{gate}");
        }
        assert!(required_records("gate:architecture").is_empty());
        assert!(required_records("learn").is_empty());
    }
}

// ─── Tests: shared handler fixtures ───────────────────────────────────────────

#[cfg(test)]
mod test_support {
    use super::*;
    use crate::mcp::protocol::{CallToolResult, ToolContent};
    use crate::router::ToolRouter;
    use crate::tools::{ServerConfig, ToolContext};
    use serde_json::Value;
    use std::sync::Arc;

    pub(super) const JOB_ID: &str = "demo-20260921-140000";
    pub(super) const STARTED_AT: &str = "2026-09-21T14:00:00Z";

    pub(super) fn ctx() -> ToolContext {
        ToolContext::new(ServerConfig::default(), Arc::new(ToolRouter::new()))
    }

    pub(super) fn text(result: &CallToolResult) -> String {
        match &result.content[0] {
            ToolContent::Text { text } => text.clone(),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    pub(super) fn body(result: &CallToolResult) -> Value {
        serde_json::from_str(&text(result)).expect("payload is json")
    }

    /// The structured error's `kind`, asserting the result is an error.
    pub(super) fn error_kind(result: &CallToolResult) -> String {
        assert!(result.is_error, "expected an error, got {}", text(result));
        body(result)["error"]["kind"]
            .as_str()
            .unwrap_or("(no kind)")
            .to_string()
    }

    /// A KiCad project directory: a project file and one schematic.
    pub(super) fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("demo.kicad_pro"), "{}\n").unwrap();
        std::fs::write(dir.path().join("demo.kicad_sch"), "(kicad_sch)\n").unwrap();
        dir
    }

    pub(super) fn arg(dir: &tempfile::TempDir) -> String {
        dir.path().to_str().unwrap().to_string()
    }

    pub(super) fn flow_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join(".konnect").join("flow")
    }

    /// A `new_board` job standing at `phase`, with only its start entry.
    pub(super) fn job_at(phase: &str) -> JobState {
        JobState {
            schema: STATE_SCHEMA,
            job_id: JOB_ID.into(),
            objective: "Demo board".into(),
            lane: Lane::NewBoard,
            mode: Mode::Guided,
            phases: CANONICAL_PHASES.iter().map(|p| p.to_string()).collect(),
            phase: phase.into(),
            started_at: STARTED_AT.into(),
            gate_approvals: BTreeMap::new(),
            pending_approvals: Vec::new(),
            deferred_findings: Vec::new(),
            queue: Vec::new(),
            history: vec![HistoryEntry::new(
                HistoryKind::Start,
                None,
                "requirements",
                STARTED_AT,
                "0000",
            )],
        }
    }

    /// Write `STATE.md` directly, as an earlier call (or a hand edit) left it.
    pub(super) fn plant_state(dir: &tempfile::TempDir, state: &JobState) {
        std::fs::create_dir_all(flow_path(dir)).unwrap();
        std::fs::write(flow_path(dir).join(STATE_FILE), render_state(state)).unwrap();
    }

    pub(super) fn plant_file(dir: &tempfile::TempDir, relative: &str, content: &str) {
        let path = flow_path(dir).join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
}

// ─── Tests: flow_status (task 1.2) ────────────────────────────────────────────

#[cfg(test)]
mod status_tests {
    use super::test_support::*;
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn status_without_a_flow_directory_reports_no_job_and_creates_nothing() {
        let dir = project();
        let result = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx())
            .await
            .unwrap();
        assert!(!result.is_error, "{}", text(&result));
        let status = body(&result);
        assert!(status["job"].is_null(), "{status}");
        assert!(status["phase"].is_null(), "{status}");
        assert!(status["state_error"].is_null(), "{status}");
        assert!(status["next_step"].is_null(), "{status}");
        assert_eq!(status["design_hash"].as_str().unwrap().len(), 64);
        assert_eq!(
            status["design_files"],
            json!(["demo.kicad_pro", "demo.kicad_sch"])
        );
        assert_eq!(status["lock_files"], json!([]));
        assert_eq!(
            status["fix_rounds"],
            json!({ "schematic_review": 0, "prefab_review": 0 })
        );
        assert!(
            !dir.path().join(".konnect").exists(),
            "flow_status must create nothing"
        );
    }

    #[tokio::test]
    async fn status_lists_every_kicad_lock_beside_a_covered_file() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("demo.kicad_pcb"), "(kicad_pcb)\n").unwrap();
        std::fs::write(root.join("~demo.kicad_pro.lck"), "felip host").unwrap();
        std::fs::write(root.join("~demo.kicad_pcb.lck"), "felip host").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("sheet.kicad_sch"), "(kicad_sch)\n").unwrap();
        std::fs::write(root.join("sub").join("~sheet.kicad_sch.lck"), "x").unwrap();
        // A lock beside no covered file is not a lock on this design.
        std::fs::write(root.join("~ghost.kicad_sch.lck"), "x").unwrap();

        let result = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx())
            .await
            .unwrap();
        assert_eq!(
            body(&result)["lock_files"],
            json!([
                "sub/~sheet.kicad_sch.lck",
                "~demo.kicad_pcb.lck",
                "~demo.kicad_pro.lck"
            ])
        );
    }

    #[tokio::test]
    async fn a_broken_state_file_is_reported_not_fatal() {
        let dir = project();
        plant_file(&dir, STATE_FILE, "---\n{ \"schema\": 1,, }\n---\n");
        let result = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx())
            .await
            .unwrap();
        assert!(!result.is_error, "{}", text(&result));
        let status = body(&result);
        let state_error = status["state_error"].as_str().expect("state_error set");
        assert!(state_error.contains(STATE_FILE), "{state_error}");
        assert!(status["job"].is_null());
    }

    #[tokio::test]
    async fn a_read_name_outside_the_readable_set_is_an_invalid_argument() {
        let dir = project();
        for name in [
            "../x",
            "STATE.md",
            "records/constraints.md",
            "gates/other.md",
            "memory/hacker.md",
            "memory/../STATE.md",
            "handoffs/1-review.md",
            "handoffs/01-nobody.md",
            "log/../../x",
        ] {
            let result =
                handle_flow_status(&json!({ "project_dir": arg(&dir), "read": [name] }), &ctx())
                    .await
                    .unwrap();
            assert_eq!(error_kind(&result), "invalid_argument", "{name}");
            assert_eq!(body(&result)["error"]["field"], "read", "{name}");
        }
        assert!(!dir.path().join(".konnect").exists());
    }

    #[tokio::test]
    async fn status_reports_the_job_and_reads_records_through_the_tool() {
        let dir = project();
        let mut state = job_at("architecture");
        let mut advance = HistoryEntry::new(
            HistoryKind::Advance,
            Some("requirements"),
            "architecture",
            "2026-09-21T14:20:00Z",
            "0000",
        );
        advance.records = vec!["constraints.md".into()];
        state.history.push(advance);
        plant_state(&dir, &state);
        plant_file(&dir, "records/constraints.md", "# Constraints\n");
        plant_file(&dir, &format!("log/2026-09-21-{JOB_ID}.md"), "log line\n");
        plant_file(
            &dir,
            &format!("handoffs/{JOB_ID}/01-requirements.md"),
            "handoff\n",
        );
        plant_file(
            &dir,
            &format!("handoffs/{JOB_ID}/notes.txt"),
            "not a handoff\n",
        );

        let result = handle_flow_status(
            &json!({
                "project_dir": arg(&dir),
                "read": [
                    "constraints.md",
                    "architecture.md",
                    "log",
                    "handoffs/01-requirements.md",
                    "memory/layout.md"
                ]
            }),
            &ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{}", text(&result));
        let status = body(&result);
        assert_eq!(status["job"]["job_id"], JOB_ID);
        assert_eq!(status["job"]["lane"], "new_board");
        assert_eq!(status["job"]["mode"], "guided");
        assert_eq!(status["job"]["phases"], json!(CANONICAL_PHASES));
        assert_eq!(status["phase"], "architecture");
        assert_eq!(
            status["next_step"],
            json!({
                "phase": "architecture",
                "required_records": ["architecture.md", "worst-case.md", "pin-plan.md"],
                "next_phase": "gate:architecture",
                "is_gate": false
            })
        );
        assert_eq!(status["last_transition"]["kind"], "advance");
        assert_eq!(status["last_transition"]["to"], "architecture");
        assert_eq!(status["handoffs"], json!(["01-requirements.md"]));
        assert_eq!(status["contents"]["constraints.md"], "# Constraints\n");
        assert_eq!(status["contents"]["log"], "log line\n");
        assert_eq!(
            status["contents"]["handoffs/01-requirements.md"],
            "handoff\n"
        );
        assert_eq!(
            status["missing"],
            json!(["architecture.md", "memory/layout.md"])
        );
    }

    /// The job stands AT `gate:architecture`, reached through a FIX-round
    /// rewind to `architecture`: since Fix round 1 (DECISION D) only the
    /// current gate's approval gets a recomputed `valid` — this test stood at
    /// `schematic` before, where the approval now reports `passed`.
    #[tokio::test]
    async fn gate_validity_is_recomputed_against_the_current_hashes() {
        let dir = project();
        for record in [
            "constraints.md",
            "architecture.md",
            "worst-case.md",
            "pin-plan.md",
        ] {
            plant_file(
                &dir,
                &format!("records/{record}"),
                "content\nReadiness: PASS\n",
            );
        }
        let canonical = dir.path().canonicalize().unwrap();
        let (design_hash, _) = crate::design_hash::design_state_hash(&canonical).unwrap();
        let mut state = job_at("gate:architecture");
        let package = package_hash(
            &flow_path(&dir).join("records"),
            &package_records(&state.phases, "gate:architecture"),
        )
        .unwrap();
        let mut rewind = HistoryEntry::new(
            HistoryKind::Rewind,
            Some("schematic_review"),
            "architecture",
            "2026-09-21T16:00:00Z",
            "0000",
        );
        rewind.reason = Some("ERC finding".into());
        state.history.push(rewind);
        state.history.push(HistoryEntry::new(
            HistoryKind::Advance,
            Some("architecture"),
            "gate:architecture",
            "2026-09-21T16:30:00Z",
            &design_hash,
        ));
        state.gate_approvals.insert(
            "architecture".into(),
            GateApproval {
                decision: GateDecision::Approve,
                approved_by: ApprovedBy::User,
                approved_at: "2026-09-21T17:00:00Z".into(),
                design_hash_at_approval: design_hash,
                package_hash_at_approval: package.hash,
                visit: 2,
                summary: "ok".into(),
                user_words: "aprovado".into(),
            },
        );
        plant_state(&dir, &state);

        let status = |dir: &tempfile::TempDir| {
            let project_dir = arg(dir);
            async move {
                body(
                    &handle_flow_status(&json!({ "project_dir": project_dir }), &ctx())
                        .await
                        .unwrap(),
                )
            }
        };
        let fresh = status(&dir).await;
        assert_eq!(
            fresh["gate_approvals"]["architecture"]["status"], "current",
            "{fresh}"
        );
        assert_eq!(
            fresh["gate_approvals"]["architecture"]["valid"], true,
            "{fresh}"
        );
        assert_eq!(
            fresh["gate_approvals"]["architecture"]["user_words"],
            "aprovado"
        );
        assert_eq!(
            fresh["fix_rounds"],
            json!({ "schematic_review": 1, "prefab_review": 0 })
        );

        std::fs::write(dir.path().join("demo.kicad_sch"), "(kicad_sch (changed))\n").unwrap();
        let design_changed = status(&dir).await;
        assert_eq!(
            design_changed["gate_approvals"]["architecture"]["valid"],
            false
        );

        std::fs::write(dir.path().join("demo.kicad_sch"), "(kicad_sch)\n").unwrap();
        assert_eq!(
            status(&dir).await["gate_approvals"]["architecture"]["valid"],
            true
        );
        plant_file(&dir, "records/architecture.md", "edited\nReadiness: PASS\n");
        let package_changed = status(&dir).await;
        assert_eq!(
            package_changed["gate_approvals"]["architecture"]["valid"],
            false
        );
    }

    /// Fix round 1, DECISION D (reviewer 11 minor 2): only the gate the job
    /// stands at gets a recomputed `valid`. An approval of a gate already left
    /// reports `status: "passed"` with the hashes it was approved at — the
    /// first schematic save after the architecture gate is normal work, not a
    /// reason to re-ask or rewind.
    #[tokio::test]
    async fn a_passed_gates_approval_reports_status_passed_without_a_valid_field() {
        use super::advance_tests::{advance, at_architecture_gate};
        use super::gate_tests::{at_placement_gate, gate};

        let status = |dir: &tempfile::TempDir| {
            let project_dir = arg(dir);
            async move {
                body(
                    &handle_flow_status(&json!({ "project_dir": project_dir }), &ctx())
                        .await
                        .unwrap(),
                )
            }
        };

        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        let approved = gate(&dir, &job_id, "architecture", "approve", "pode seguir").await;
        assert!(!approved.is_error, "{}", text(&approved));
        let approved_at = body(&approved)["design_hash_at_approval"].clone();
        let left = advance(&dir, &job_id, "schematic", &[]).await;
        assert!(!left.is_error, "{}", text(&left));
        // The first schematic save: the design no longer matches the approval.
        std::fs::write(dir.path().join("demo.kicad_sch"), "(kicad_sch (saved))\n").unwrap();
        let moved_on = status(&dir).await;
        let passed = &moved_on["gate_approvals"]["architecture"];
        assert_eq!(passed["status"], "passed", "{moved_on}");
        assert!(
            passed.get("valid").is_none(),
            "a passed gate carries no recomputed valid: {moved_on}"
        );
        assert!(passed.get("validity_error").is_none(), "{moved_on}");
        assert_eq!(passed["design_hash_at_approval"], approved_at, "{moved_on}");
        assert_eq!(passed["user_words"], "pode seguir");

        let at_gate = project();
        let job_id = at_placement_gate(&at_gate, "guided").await;
        let approved = gate(&at_gate, &job_id, "placement", "approve", "roteia").await;
        assert!(!approved.is_error, "{}", text(&approved));
        let standing = status(&at_gate).await;
        let current = &standing["gate_approvals"]["placement"];
        assert_eq!(current["status"], "current", "{standing}");
        assert_eq!(current["valid"], true, "{standing}");
        std::fs::write(
            at_gate.path().join("demo.kicad_sch"),
            "(kicad_sch (moved))\n",
        )
        .unwrap();
        let stale = status(&at_gate).await;
        assert_eq!(
            stale["gate_approvals"]["placement"]["status"], "current",
            "{stale}"
        );
        assert_eq!(
            stale["gate_approvals"]["placement"]["valid"], false,
            "the current gate's valid is recomputed: {stale}"
        );
    }

    #[tokio::test]
    async fn status_refuses_a_directory_that_is_not_a_kicad_project() {
        let dir = tempfile::tempdir().unwrap();
        let result = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx())
            .await
            .unwrap();
        assert_eq!(error_kind(&result), "invalid_argument");
        assert!(!dir.path().join(".konnect").exists());
    }
}

// ─── Tests: published schemas ─────────────────────────────────────────────────

#[cfg(test)]
mod schema_tests {
    use super::*;

    fn tool(name: &str) -> ToolDef {
        tools()
            .into_iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} is not defined"))
    }

    /// Unit tests call handlers directly; the dispatcher validates against the
    /// closed schema FIRST, so each documented call shape is checked here.
    #[test]
    fn the_published_schemas_accept_the_documented_calls() {
        let status = tool("flow_status");
        assert!(status
            .input_validator
            .is_valid(&json!({ "project_dir": "p", "read": ["constraints.md", "log"] })));
        assert!(!status
            .input_validator
            .is_valid(&json!({ "project_dir": "p", "surprise": 1 })));
        assert!(!status.input_validator.is_valid(&json!({ "read": [] })));

        let start = tool("flow_start");
        assert!(start.input_validator.is_valid(&json!({
            "project_dir": "p",
            "objective": "o",
            "lane": "fab_only",
            "phases": ["manufacturing", "gate:purchase"],
            "mode": "autonomous"
        })));
        assert!(!start
            .input_validator
            .is_valid(&json!({ "project_dir": "p", "objective": "o", "lane": "sideways" })));
        assert!(!start.input_validator.is_valid(&json!({
            "project_dir": "p",
            "objective": "o",
            "lane": "new_board",
            "phases": ["layout"]
        })));

        let advance = tool("flow_advance");
        assert!(advance.input_validator.is_valid(&json!({
            "project_dir": "p",
            "job_id": "j",
            "to_phase": "gate:architecture",
            "records": [{ "filename": "architecture.md", "content": "Readiness: PASS" }],
            "evidence_calls": ["run_erc"],
            "reason": "r"
        })));
        assert!(advance
            .input_validator
            .is_valid(&json!({ "project_dir": "p", "job_id": "j", "to_phase": "closed" })));
        assert!(!advance.input_validator.is_valid(&json!({
            "project_dir": "p",
            "job_id": "j",
            "to_phase": "architecture",
            "records": [{ "filename": "../escape.md", "content": "x" }]
        })));
        assert!(!advance.input_validator.is_valid(&json!({
            "project_dir": "p",
            "job_id": "j",
            "to_phase": "architecture",
            "records": [{ "filename": "constraints.md", "content": "x", "extra": 1 }]
        })));

        let gate = tool("flow_gate");
        let approve = json!({
            "project_dir": "p",
            "job_id": "j",
            "gate_name": "placement",
            "decision": "approve",
            "summary": "s",
            "user_words": ""
        });
        assert!(gate.input_validator.is_valid(&approve));
        for (key, bad) in [
            ("gate_name", json!("layout")),
            ("decision", json!("maybe")),
            ("extra", json!(1)),
        ] {
            let mut call = approve.clone();
            call[key] = bad;
            assert!(!gate.input_validator.is_valid(&call), "{key}");
        }
        let mut without_words = approve.clone();
        without_words.as_object_mut().unwrap().remove("user_words");
        assert!(!gate.input_validator.is_valid(&without_words));

        let log = tool("flow_log");
        for call in [
            json!({
                "project_dir": "p", "job_id": "j", "kind": "decision", "message": "m",
                "why": "w", "rollback": "r", "role": "requirements"
            }),
            json!({
                "project_dir": "p", "job_id": "j", "kind": "lesson", "message": "m",
                "role": "photo-intake", "scope": "technology"
            }),
            json!({ "project_dir": "p", "job_id": "j", "kind": "handoff", "message": "m", "role": "review" }),
        ] {
            assert!(log.input_validator.is_valid(&call), "{call}");
        }
        for bad in [
            json!({ "project_dir": "p", "job_id": "j", "kind": "gossip", "message": "m" }),
            json!({ "project_dir": "p", "job_id": "j", "kind": "lesson", "message": "m", "role": "nobody" }),
            json!({ "project_dir": "p", "job_id": "j", "kind": "lesson", "message": "m", "scope": "global" }),
            json!({ "project_dir": "p", "job_id": "j", "kind": "evidence" }),
        ] {
            assert!(!log.input_validator.is_valid(&bad), "{bad}");
        }

        let defer = tool("flow_defer");
        assert!(defer.input_validator.is_valid(&json!({
            "project_dir": "p", "job_id": "j", "kind": "queue_item", "description": "d", "owner": "o"
        })));
        assert!(!defer.input_validator.is_valid(&json!({
            "project_dir": "p", "job_id": "j", "kind": "todo", "description": "d"
        })));

        let names: Vec<&str> = tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            [
                "flow_status",
                "flow_start",
                "flow_advance",
                "flow_gate",
                "flow_log",
                "flow_defer"
            ],
            "D1 order"
        );
    }
}

// ─── Tests: flow_start (task 1.3) ─────────────────────────────────────────────

#[cfg(test)]
mod start_tests {
    use super::test_support::*;
    use super::*;
    use crate::mcp::protocol::CallToolResult;
    use serde_json::{json, Value};

    /// `flow_start` with a new_board default call, overlaid with `extra`.
    async fn start(dir: &tempfile::TempDir, extra: Value) -> CallToolResult {
        let mut args = json!({
            "project_dir": arg(dir),
            "objective": "Demo board",
            "lane": "new_board",
        });
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        handle_flow_start(&args, &ctx()).await.unwrap()
    }

    fn state_on_disk(dir: &tempfile::TempDir) -> JobState {
        let text = std::fs::read_to_string(flow_path(dir).join(STATE_FILE)).unwrap();
        parse_state(&text).unwrap()
    }

    #[test]
    fn job_ids_are_a_slug_and_a_utc_stamp() {
        assert_eq!(
            job_slug("Conversor USB-serial com ESP32"),
            "conversor-usb-serial-com-esp32"
        );
        assert_eq!(job_slug("  --Ação!! "), "a-o");
        assert_eq!(job_slug("!!!"), "job");
        assert_eq!(job_slug(""), "job");
        assert_eq!(job_slug(&"a".repeat(39)).len(), 39);
        assert_eq!(
            job_slug(&format!("{} tail", "b".repeat(39))),
            "b".repeat(39),
            "a cut that lands on a separator is trimmed"
        );
        assert_eq!(job_slug(&"c".repeat(60)).len(), 40);
        assert_eq!(compact_stamp("2026-09-21T14:00:05Z"), "20260921-140005");
    }

    #[tokio::test]
    async fn a_new_board_job_opens_at_its_first_phase() {
        let dir = project();
        let result = start(&dir, json!({})).await;
        assert!(!result.is_error, "{}", text(&result));
        let started = body(&result);
        let job_id = started["job_id"].as_str().unwrap().to_string();
        assert!(job_id.starts_with("demo-board-"), "{job_id}");
        assert_eq!(job_id.len(), "demo-board-".len() + "20260921-140000".len());
        assert_eq!(validate_job_id(&job_id), Ok(()));
        assert_eq!(started["phase"], "requirements");
        assert_eq!(started["mode"], "guided");

        let state = state_on_disk(&dir);
        assert_eq!(state.job_id, job_id);
        assert_eq!(state.phase, "requirements");
        assert_eq!(state.phases, CANONICAL_PHASES);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].kind, HistoryKind::Start);
        assert_eq!(state.history[0].to, "requirements");
        assert_eq!(state.history[0].design_hash, started["design_hash"]);
        assert_eq!(state.history[0].design_hash.len(), 64);

        let log = flow_path(&dir)
            .join("log")
            .join(format!("{}-{job_id}.md", &state.started_at[..10]));
        let log_text = std::fs::read_to_string(&log).expect("the start is logged");
        assert!(log_text.contains("start"), "{log_text}");

        let status = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx())
            .await
            .unwrap();
        assert_eq!(body(&status)["job"]["job_id"], job_id.as_str());
    }

    #[tokio::test]
    async fn a_second_start_while_a_job_is_active_is_a_conflict_and_changes_nothing() {
        let dir = project();
        assert!(!start(&dir, json!({})).await.is_error);
        let state_file = flow_path(&dir).join(STATE_FILE);
        let before = std::fs::read(&state_file).unwrap();

        let second = start(&dir, json!({ "objective": "Another board" })).await;
        assert_eq!(error_kind(&second), "conflict");
        let paths = body(&second)["error"]["paths"].to_string();
        assert!(paths.contains(STATE_FILE), "{paths}");
        assert_eq!(std::fs::read(&state_file).unwrap(), before);
    }

    #[tokio::test]
    async fn missing_or_invalid_arguments_are_refused_before_anything_is_created() {
        let cases = [
            (json!({ "lane": "fab_only" }), "phases", ""),
            (
                json!({ "phases": ["placement", "routing"] }),
                "phases",
                "\\\"placement\\\"",
            ),
            (
                json!({ "lane": "board_revision", "phases": ["schematic", "requirements"] }),
                "phases",
                "\\\"requirements\\\"",
            ),
            (json!({ "lane": "review_only", "phases": [] }), "phases", ""),
            (json!({ "objective": "  " }), "objective", ""),
            (json!({ "lane": "sideways" }), "lane", ""),
            (json!({ "mode": "reckless" }), "mode", ""),
        ];
        for (extra, field, named) in cases {
            let dir = project();
            let result = start(&dir, extra.clone()).await;
            assert_eq!(error_kind(&result), "invalid_argument", "{extra}");
            assert_eq!(body(&result)["error"]["field"], field, "{extra}");
            assert!(text(&result).contains(named), "{extra}: {}", text(&result));
            assert!(
                !dir.path().join(".konnect").exists(),
                "{extra}: a refusal creates nothing"
            );
        }
    }

    #[tokio::test]
    async fn a_directory_without_a_top_level_project_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested").join("demo.kicad_pro"), "{}").unwrap();
        let result = start(&dir, json!({})).await;
        assert_eq!(error_kind(&result), "invalid_argument");
        assert_eq!(body(&result)["error"]["field"], "project_dir");
        assert!(!dir.path().join(".konnect").exists());
    }

    #[tokio::test]
    async fn a_closed_job_lets_the_next_one_start() {
        let dir = project();
        plant_state(&dir, &job_at(CLOSED));
        let result = start(
            &dir,
            json!({
                "objective": "Fab run",
                "lane": "fab_only",
                "phases": ["manufacturing", "gate:purchase"],
                "mode": "autonomous"
            }),
        )
        .await;
        assert!(!result.is_error, "{}", text(&result));
        let state = state_on_disk(&dir);
        assert_eq!(state.objective, "Fab run");
        assert_eq!(state.lane, Lane::FabOnly);
        assert_eq!(state.mode, Mode::Autonomous);
        assert_eq!(state.phase, "manufacturing");
        assert_ne!(state.job_id, JOB_ID);
    }

    #[tokio::test]
    async fn an_unparseable_state_file_is_a_conflict_not_an_overwrite() {
        let dir = project();
        plant_file(&dir, STATE_FILE, "hand-edited garbage\n");
        let state_file = flow_path(&dir).join(STATE_FILE);
        let result = start(&dir, json!({})).await;
        assert_eq!(error_kind(&result), "conflict");
        assert_eq!(
            std::fs::read_to_string(&state_file).unwrap(),
            "hand-edited garbage\n"
        );
    }

    #[tokio::test]
    async fn racing_starts_open_exactly_one_job() {
        let dir = project();
        let (first, second) = tokio::join!(
            start(&dir, json!({ "objective": "Racer one" })),
            start(&dir, json!({ "objective": "Racer two" }))
        );
        let outcomes = [first.is_error, second.is_error];
        assert_eq!(
            outcomes.iter().filter(|failed| !**failed).count(),
            1,
            "{} / {}",
            text(&first),
            text(&second)
        );
        let loser = if first.is_error { &first } else { &second };
        assert_eq!(error_kind(loser), "conflict");
    }
}

// ─── Tests: flow_advance forward (task 1.4) ───────────────────────────────────

#[cfg(test)]
mod advance_tests {
    use super::test_support::*;
    use super::*;
    use crate::mcp::protocol::CallToolResult;
    use serde_json::{json, Value};

    pub(super) const PASSING_ARCHITECTURE: &str = "# Architecture\n\nBlocks.\n\nReadiness: PASS\n";

    /// Open a job through the real tool and return its job_id.
    pub(super) async fn open(
        dir: &tempfile::TempDir,
        lane: &str,
        phases: Option<&[&str]>,
    ) -> String {
        let mut args = json!({ "project_dir": arg(dir), "objective": "Demo board", "lane": lane });
        if let Some(phases) = phases {
            args["phases"] = json!(phases);
        }
        let result = handle_flow_start(&args, &ctx()).await.unwrap();
        assert!(!result.is_error, "{}", text(&result));
        body(&result)["job_id"].as_str().unwrap().to_string()
    }

    pub(super) async fn advance(
        dir: &tempfile::TempDir,
        job_id: &str,
        to_phase: &str,
        records: &[(&str, &str)],
    ) -> CallToolResult {
        let records: Vec<Value> = records
            .iter()
            .map(|(filename, content)| json!({ "filename": filename, "content": content }))
            .collect();
        handle_flow_advance(
            &json!({
                "project_dir": arg(dir),
                "job_id": job_id,
                "to_phase": to_phase,
                "records": records,
            }),
            &ctx(),
        )
        .await
        .unwrap()
    }

    /// `flow_advance` through `ctx` (whose observer the evidence check
    /// reads), with `extra` overlaid on the three required arguments.
    pub(super) async fn advance_with(
        dir: &tempfile::TempDir,
        ctx: &ToolContext,
        job_id: &str,
        to_phase: &str,
        extra: Value,
    ) -> CallToolResult {
        let mut args = json!({ "project_dir": arg(dir), "job_id": job_id, "to_phase": to_phase });
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        handle_flow_advance(&args, ctx).await.unwrap()
    }

    pub(super) fn state_bytes(dir: &tempfile::TempDir) -> Vec<u8> {
        std::fs::read(flow_path(dir).join(STATE_FILE)).unwrap()
    }

    pub(super) fn state_on_disk(dir: &tempfile::TempDir) -> JobState {
        parse_state(&String::from_utf8(state_bytes(dir)).unwrap()).unwrap()
    }

    pub(super) fn record_path(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        flow_path(dir).join("records").join(name)
    }

    /// A new_board job standing at `gate:architecture`, entered by the real
    /// tool with a passing package.
    pub(super) async fn at_architecture_gate(dir: &tempfile::TempDir) -> String {
        let job_id = open(dir, "new_board", None).await;
        let left = advance(dir, &job_id, "architecture", &[("constraints.md", "# C\n")]).await;
        assert!(!left.is_error, "{}", text(&left));
        let entered = advance(
            dir,
            &job_id,
            "gate:architecture",
            &[
                ("architecture.md", PASSING_ARCHITECTURE),
                ("worst-case.md", "# WC\n"),
                ("pin-plan.md", "# Pins\n"),
            ],
        )
        .await;
        assert!(!entered.is_error, "{}", text(&entered));
        job_id
    }

    /// Fix round 1, DECISION C: the log entry follows the `STATE.md` commit.
    /// With `log` a plain file the append fails, yet the transition stands —
    /// the phase has moved, the record written before the commit (D4) is on
    /// disk, and the failure is a `warning` on a success.
    #[tokio::test]
    async fn a_log_append_failure_after_state_commits_is_a_warning() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let log_dir = flow_path(&dir).join("log");
        std::fs::remove_dir_all(&log_dir).unwrap();
        std::fs::write(&log_dir, "not a directory\n").unwrap();

        let moved = advance(
            &dir,
            &job_id,
            "architecture",
            &[("constraints.md", "# C\n")],
        )
        .await;
        assert!(!moved.is_error, "{}", text(&moved));
        assert_eq!(body(&moved)["phase"], "architecture");
        let warning = body(&moved)["warning"]
            .as_str()
            .unwrap_or_else(|| panic!("a warning field: {}", text(&moved)))
            .to_string();
        assert!(
            warning.contains(&format!("flow{}log", std::path::MAIN_SEPARATOR)),
            "the warning names the failed write: {warning}"
        );
        assert_eq!(state_on_disk(&dir).phase, "architecture");
        assert_eq!(
            std::fs::read_to_string(record_path(&dir, "constraints.md")).unwrap(),
            "# C\n"
        );
        assert!(log_dir.is_file(), "the blocker is left as it was");
    }

    #[tokio::test]
    async fn leaving_requirements_with_its_record_writes_it_and_moves_the_phase() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let result = advance(
            &dir,
            &job_id,
            "architecture",
            &[("constraints.md", "# Constraints\nUSB-C\n")],
        )
        .await;
        assert!(!result.is_error, "{}", text(&result));
        assert_eq!(body(&result)["phase"], "architecture");
        assert_eq!(
            body(&result).get("warning"),
            Some(&Value::Null),
            "every side file landed after the commit: {}",
            text(&result)
        );

        assert_eq!(
            std::fs::read_to_string(record_path(&dir, "constraints.md")).unwrap(),
            "# Constraints\nUSB-C\n"
        );
        let state = state_on_disk(&dir);
        assert_eq!(state.phase, "architecture");
        let entry = state.history.last().unwrap();
        assert_eq!(entry.kind, HistoryKind::Advance);
        assert_eq!(entry.from.as_deref(), Some("requirements"));
        assert_eq!(entry.to, "architecture");
        assert_eq!(entry.records, ["constraints.md"]);
        assert_eq!(entry.design_hash, state.history[0].design_hash);
        assert!(entry.package_hash.is_none(), "not entering a gate");

        let log = flow_path(&dir)
            .join("log")
            .join(format!("{}-{job_id}.md", &state.started_at[..10]));
        let log_text = std::fs::read_to_string(log).unwrap();
        assert!(
            log_text.contains("requirements → architecture"),
            "{log_text}"
        );
    }

    /// Design D4 / pre-mortem 3: a record already on disk — here a stale
    /// architecture.md from an earlier write — never satisfies an exit.
    #[tokio::test]
    async fn a_stale_record_on_disk_does_not_satisfy_leaving_architecture() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        assert!(
            !advance(
                &dir,
                &job_id,
                "architecture",
                &[("constraints.md", "# C\n")]
            )
            .await
            .is_error
        );
        let stale = "# Old board\n\nReadiness: PASS\n";
        std::fs::write(record_path(&dir, "architecture.md"), stale).unwrap();
        let before = state_bytes(&dir);

        let result = advance(
            &dir,
            &job_id,
            "gate:architecture",
            &[("worst-case.md", "# WC\n"), ("pin-plan.md", "# Pins\n")],
        )
        .await;
        assert_eq!(error_kind(&result), "invalid_argument");
        assert_eq!(body(&result)["error"]["field"], "records");
        // The same-call rule itself must refuse — not the readiness check,
        // which would also refuse an empty architecture.md and so mask it.
        assert!(
            text(&result).contains("requires architecture.md"),
            "{}",
            text(&result)
        );
        assert_eq!(state_bytes(&dir), before, "STATE.md unchanged");
        assert_eq!(state_on_disk(&dir).phase, "architecture");
        assert_eq!(
            std::fs::read_to_string(record_path(&dir, "architecture.md")).unwrap(),
            stale
        );
        assert!(
            !record_path(&dir, "worst-case.md").exists()
                && !record_path(&dir, "pin-plan.md").exists(),
            "a refusal writes none of the supplied records"
        );
    }

    #[tokio::test]
    async fn a_skip_ahead_or_foreign_target_is_refused() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let before = state_bytes(&dir);
        for to_phase in ["gate:architecture", "requirements", "learn", "closed"] {
            let result = advance(&dir, &job_id, to_phase, &[("constraints.md", "# C\n")]).await;
            if to_phase == "closed" {
                // An abandon, not a forward move: never allowed with records.
                assert!(result.is_error, "{to_phase}");
            } else {
                assert_eq!(error_kind(&result), "invalid_argument", "{to_phase}");
                assert_eq!(body(&result)["error"]["field"], "to_phase", "{to_phase}");
            }
            assert_eq!(state_bytes(&dir), before, "{to_phase}");
            assert!(!record_path(&dir, "constraints.md").exists(), "{to_phase}");
        }

        let fab = project();
        let fab_job = open(&fab, "fab_only", Some(&["manufacturing", "gate:purchase"])).await;
        let result = advance(&fab, &fab_job, "routing", &[]).await;
        assert_eq!(error_kind(&result), "invalid_argument");
        assert!(text(&result).contains("routing"), "{}", text(&result));
    }

    #[tokio::test]
    async fn a_record_belonging_to_another_phase_is_refused() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let before = state_bytes(&dir);
        let result = advance(
            &dir,
            &job_id,
            "architecture",
            &[("constraints.md", "# C\n"), ("placement.md", "# P\n")],
        )
        .await;
        assert_eq!(error_kind(&result), "invalid_argument");
        assert_eq!(body(&result)["error"]["field"], "records");
        assert!(text(&result).contains("placement.md"), "{}", text(&result));
        assert_eq!(state_bytes(&dir), before);
        assert!(!record_path(&dir, "constraints.md").exists());
        assert!(!record_path(&dir, "placement.md").exists());
    }

    #[tokio::test]
    async fn only_a_passing_readiness_line_leaves_architecture() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        assert!(
            !advance(
                &dir,
                &job_id,
                "architecture",
                &[("constraints.md", "# C\n")]
            )
            .await
            .is_error
        );
        for (architecture, expected) in [
            (
                "# A\nReadiness: BLOCKED — no input voltage range\n",
                "no input voltage range",
            ),
            ("# A\nno readiness line\n", "Readiness: PASS"),
        ] {
            let before = state_bytes(&dir);
            let result = advance(
                &dir,
                &job_id,
                "gate:architecture",
                &[
                    ("architecture.md", architecture),
                    ("worst-case.md", "# WC\n"),
                    ("pin-plan.md", "# Pins\n"),
                ],
            )
            .await;
            assert_eq!(error_kind(&result), "invalid_argument", "{architecture}");
            assert!(text(&result).contains(expected), "{}", text(&result));
            assert_eq!(state_bytes(&dir), before);
            assert!(!record_path(&dir, "architecture.md").exists());
        }
    }

    #[tokio::test]
    async fn entering_a_gate_records_the_package_it_binds_to() {
        let dir = project();
        at_architecture_gate(&dir).await;
        let state = state_on_disk(&dir);
        assert_eq!(state.phase, "gate:architecture");
        let entry = state.history.last().unwrap();
        let expected = package_hash(
            &flow_path(&dir).join("records"),
            &[
                "constraints.md",
                "architecture.md",
                "worst-case.md",
                "pin-plan.md",
            ],
        )
        .unwrap();
        assert_eq!(entry.package_hash.as_deref(), Some(expected.hash.as_str()));
        assert_eq!(entry.package_files, expected.files);
        assert_eq!(entry.package_files.len(), 4);
        assert!(entry.package_files.values().all(Option::is_some));
    }

    /// Leaving a gate needs an approval granted during THIS visit whose keys
    /// still match the design and the package.
    #[tokio::test]
    async fn leaving_a_gate_requires_a_current_visit_approval_with_matching_keys() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;

        let unapproved = advance(&dir, &job_id, "schematic", &[]).await;
        assert_eq!(error_kind(&unapproved), "stale_target");
        assert_eq!(body(&unapproved)["error"]["target"], "gate:architecture");

        let entered = state_on_disk(&dir);
        let visit = entered.history.len() - 1;
        let approve = |visit: usize| {
            let mut state = entered.clone();
            let entry = &state.history[visit.min(state.history.len() - 1)];
            let approval = GateApproval {
                decision: GateDecision::Approve,
                approved_by: ApprovedBy::User,
                approved_at: "2026-09-21T15:00:00Z".into(),
                design_hash_at_approval: entry.design_hash.clone(),
                package_hash_at_approval: entry.package_hash.clone().unwrap_or_default(),
                visit,
                summary: "shown".into(),
                user_words: "pode seguir".into(),
            };
            state.gate_approvals.insert("architecture".into(), approval);
            state
        };

        plant_state(&dir, &approve(visit - 1));
        let old_visit = advance(&dir, &job_id, "schematic", &[]).await;
        assert_eq!(
            error_kind(&old_visit),
            "stale_target",
            "an earlier visit's approval"
        );

        plant_state(&dir, &approve(visit));
        std::fs::write(dir.path().join("demo.kicad_sch"), "(kicad_sch (moved))\n").unwrap();
        let design_changed = advance(&dir, &job_id, "schematic", &[]).await;
        assert_eq!(error_kind(&design_changed), "stale_target");
        std::fs::write(dir.path().join("demo.kicad_sch"), "(kicad_sch)\n").unwrap();

        std::fs::write(record_path(&dir, "pin-plan.md"), "# Pins, edited\n").unwrap();
        let package_changed = advance(&dir, &job_id, "schematic", &[]).await;
        assert_eq!(error_kind(&package_changed), "stale_target");
        assert!(
            text(&package_changed).contains("pin-plan.md"),
            "{}",
            text(&package_changed)
        );
        std::fs::write(record_path(&dir, "pin-plan.md"), "# Pins\n").unwrap();

        let left = advance(&dir, &job_id, "schematic", &[]).await;
        assert!(!left.is_error, "{}", text(&left));
        assert_eq!(state_on_disk(&dir).phase, "schematic");
    }

    #[tokio::test]
    async fn the_last_phase_closes_the_job_with_its_record() {
        let dir = project();
        let job_id = open(&dir, "review_only", Some(&["prefab_review"])).await;
        let result = advance(
            &dir,
            &job_id,
            "closed",
            &[("ledger-prefab.md", "# Ledger\n")],
        )
        .await;
        assert!(!result.is_error, "{}", text(&result));
        let state = state_on_disk(&dir);
        assert_eq!(state.phase, CLOSED);
        assert_eq!(state.history.last().unwrap().kind, HistoryKind::Close);

        let after_close = advance(&dir, &job_id, "closed", &[]).await;
        assert_eq!(
            error_kind(&after_close),
            "stale_target",
            "a closed job moves no more"
        );
    }

    #[tokio::test]
    async fn a_foreign_job_or_a_missing_state_is_a_stale_target_that_creates_nothing() {
        let dir = project();
        let orphan = advance(&dir, "demo-20260921-140000", "architecture", &[]).await;
        assert_eq!(error_kind(&orphan), "stale_target");
        assert!(!dir.path().join(".konnect").exists());

        let job_id = open(&dir, "new_board", None).await;
        let before = state_bytes(&dir);
        let foreign = advance(
            &dir,
            "another-20260101-000000",
            "architecture",
            &[("constraints.md", "# C\n")],
        )
        .await;
        assert_eq!(error_kind(&foreign), "stale_target");
        assert_eq!(state_bytes(&dir), before);
        assert!(!record_path(&dir, "constraints.md").exists());
        assert_ne!(job_id, "another-20260101-000000");
    }
}

// ─── Tests: flow_advance rewind, abandon, evidence (task 1.5) ─────────────────

#[cfg(test)]
mod rewind_tests {
    use super::advance_tests::*;
    use super::test_support::*;
    use super::*;
    use crate::observability::{CallRecord, CallStatus};
    use serde_json::json;

    /// The photo lane's sequence (D3): it starts at `schematic`, so a job
    /// reaches the review phases through the real tool in a few calls.
    pub(super) const PHOTO_PHASES: [&str; 9] = [
        "schematic",
        "schematic_review",
        "placement",
        "gate:placement",
        "routing",
        "prefab_review",
        "manufacturing",
        "gate:purchase",
        "learn",
    ];

    fn planted_approval(visit: usize) -> GateApproval {
        GateApproval {
            decision: GateDecision::Approve,
            approved_by: ApprovedBy::User,
            approved_at: "2026-09-21T15:00:00Z".into(),
            design_hash_at_approval: "0000".into(),
            package_hash_at_approval: "0000".into(),
            visit,
            summary: "shown".into(),
            user_words: "pode seguir".into(),
        }
    }

    pub(super) fn call(tool: &str, status: CallStatus) -> CallRecord {
        CallRecord {
            call_id: crate::observability::new_call_id(),
            ts: crate::observability::unix_ms(),
            tool: tool.to_string(),
            toolset: Some("test".to_string()),
            dur_ms: 1,
            status,
            error_kind: None,
            args_bytes: 2,
            result_bytes: 2,
        }
    }

    /// Acceptance 1.5: a FIX round is a rewind that needs a reason, supplies
    /// no records, clears the gates at or after its target and is counted.
    #[tokio::test]
    async fn a_fix_round_is_a_rewind_with_a_reason() {
        let dir = project();
        let mut state = job_at("schematic_review");
        state
            .gate_approvals
            .insert("architecture".into(), planted_approval(0));
        state
            .gate_approvals
            .insert("placement".into(), planted_approval(0));
        plant_state(&dir, &state);
        let before = state_bytes(&dir);

        for extra in [json!({}), json!({ "reason": "  " })] {
            let refused = advance_with(&dir, &ctx(), JOB_ID, "schematic", extra.clone()).await;
            assert_eq!(error_kind(&refused), "invalid_argument", "{extra}");
            assert_eq!(body(&refused)["error"]["field"], "reason", "{extra}");
            assert_eq!(state_bytes(&dir), before, "{extra}");
        }
        let with_records = advance_with(
            &dir,
            &ctx(),
            JOB_ID,
            "schematic",
            json!({
                "reason": "ERC finding",
                "records": [{ "filename": "schematic-evidence.md", "content": "x" }]
            }),
        )
        .await;
        assert_eq!(error_kind(&with_records), "invalid_argument");
        assert_eq!(body(&with_records)["error"]["field"], "records");
        assert_eq!(state_bytes(&dir), before);
        assert!(!record_path(&dir, "schematic-evidence.md").exists());

        let moved = advance_with(
            &dir,
            &ctx(),
            JOB_ID,
            "schematic",
            json!({ "reason": "R3 pull-up missing" }),
        )
        .await;
        assert!(!moved.is_error, "{}", text(&moved));
        assert_eq!(body(&moved)["transition"], "rewind");
        assert_eq!(body(&moved)["cleared_approvals"], json!(["placement"]));
        let state = state_on_disk(&dir);
        assert_eq!(state.phase, "schematic");
        assert!(
            state.gate_approvals.contains_key("architecture"),
            "a gate before the target keeps its approval"
        );
        assert!(
            !state.gate_approvals.contains_key("placement"),
            "a gate after the target must be granted again"
        );
        let entry = state.history.last().unwrap();
        assert_eq!(entry.kind, HistoryKind::Rewind);
        assert_eq!(entry.from.as_deref(), Some("schematic_review"));
        assert_eq!(entry.to, "schematic");
        assert_eq!(entry.reason.as_deref(), Some("R3 pull-up missing"));
        assert!(entry.records.is_empty() && entry.package_hash.is_none());

        let status = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx())
            .await
            .unwrap();
        assert_eq!(
            body(&status)["fix_rounds"],
            json!({ "schematic_review": 1, "prefab_review": 0 })
        );
    }

    /// A rewind to the phase that produces a gate's package clears that
    /// gate's approval, and the forward move back into the gate is the entry
    /// the next approval compares against, so it carries the fresh package
    /// keys — returning asks again. (Before Fix round 1 this test rewound
    /// INTO the gate; DECISION B, reviewer 11 minor 4, now refuses that —
    /// see `a_rewind_cannot_target_a_gate`.)
    #[tokio::test]
    async fn a_rewind_to_a_gates_phase_re_enters_it_with_the_keys_its_next_approval_needs() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        let mut state = state_on_disk(&dir);
        let visit = state.history.len() - 1;
        let entered = state.history[visit].clone();
        let mut approval = planted_approval(visit);
        approval.design_hash_at_approval = entered.design_hash.clone();
        approval.package_hash_at_approval = entered.package_hash.clone().unwrap();
        state.gate_approvals.insert("architecture".into(), approval);
        plant_state(&dir, &state);
        let left = advance(&dir, &job_id, "schematic", &[]).await;
        assert!(!left.is_error, "{}", text(&left));

        let rewound = advance_with(
            &dir,
            &ctx(),
            &job_id,
            "architecture",
            json!({ "reason": "pin conflict on GPIO0" }),
        )
        .await;
        assert!(!rewound.is_error, "{}", text(&rewound));
        assert_eq!(body(&rewound)["cleared_approvals"], json!(["architecture"]));
        let state = state_on_disk(&dir);
        assert_eq!(state.phase, "architecture");
        assert!(
            state.gate_approvals.is_empty(),
            "the gate after the target loses its approval"
        );
        let rewind = state.history.last().unwrap();
        assert_eq!(rewind.kind, HistoryKind::Rewind);
        assert!(
            rewind.package_hash.is_none(),
            "a rewind never enters a gate"
        );

        let re_entered = advance(
            &dir,
            &job_id,
            "gate:architecture",
            &[
                ("architecture.md", PASSING_ARCHITECTURE),
                ("worst-case.md", "# WC\n"),
                ("pin-plan.md", "# Pins, fixed\n"),
            ],
        )
        .await;
        assert!(!re_entered.is_error, "{}", text(&re_entered));
        let state = state_on_disk(&dir);
        assert_eq!(state.phase, "gate:architecture");
        let entry = state.history.last().unwrap();
        assert_eq!(entry.kind, HistoryKind::Advance);
        let expected = package_hash(
            &flow_path(&dir).join("records"),
            &[
                "constraints.md",
                "architecture.md",
                "worst-case.md",
                "pin-plan.md",
            ],
        )
        .unwrap();
        assert_eq!(entry.package_hash.as_deref(), Some(expected.hash.as_str()));
        assert_eq!(entry.package_files, expected.files);
        assert_ne!(
            entry.package_hash, entered.package_hash,
            "the fixed pin plan"
        );
        assert_eq!(current_visit(&state), Some(state.history.len() - 1));

        let again = advance(&dir, &job_id, "schematic", &[]).await;
        assert_eq!(
            error_kind(&again),
            "stale_target",
            "a rewind and return asks again"
        );
    }

    /// Fix round 1, DECISION B (reviewer 11 minor 4): a rewind into a gate
    /// would bind the next approval to the design as it is at rewind time —
    /// a routed board behind a placement package that predates the routing.
    /// The refusal comes before every other check (even the missing reason)
    /// and names the gate and the phase to rewind to instead.
    #[tokio::test]
    async fn a_rewind_cannot_target_a_gate() {
        let dir = project();
        let mut state = job_at("routing");
        state
            .gate_approvals
            .insert("placement".into(), planted_approval(0));
        plant_state(&dir, &state);
        let before = state_bytes(&dir);

        for extra in [
            json!({ "reason": "the routing broke the placement" }),
            json!({}),
        ] {
            let refused = advance_with(&dir, &ctx(), JOB_ID, "gate:placement", extra.clone()).await;
            assert_eq!(error_kind(&refused), "invalid_argument", "{extra}");
            assert_eq!(body(&refused)["error"]["field"], "to_phase", "{extra}");
            let message = body(&refused)["message"].as_str().unwrap().to_string();
            assert!(
                message.contains("\"gate:placement\"") && message.contains("\"placement\""),
                "{extra}: the refusal names the gate and its producing phase: {message}"
            );
            assert_eq!(state_bytes(&dir), before, "{extra}: nothing is written");
        }
        assert!(
            !flow_path(&dir).join("log").exists(),
            "a refused rewind appends no log entry"
        );
    }

    /// Acceptance 1.5: `closed` from a middle phase is an abandon — it needs
    /// a reason, ends the job, and lets the next job start.
    #[tokio::test]
    async fn closing_from_a_middle_phase_needs_a_reason_and_ends_the_job() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let left = advance(
            &dir,
            &job_id,
            "architecture",
            &[("constraints.md", "# C\n")],
        )
        .await;
        assert!(!left.is_error, "{}", text(&left));
        let before = state_bytes(&dir);

        let unexplained = advance_with(&dir, &ctx(), &job_id, "closed", json!({})).await;
        assert_eq!(error_kind(&unexplained), "invalid_argument");
        assert_eq!(body(&unexplained)["error"]["field"], "reason");
        assert_eq!(state_bytes(&dir), before);

        let abandoned = advance_with(
            &dir,
            &ctx(),
            &job_id,
            "closed",
            json!({ "reason": "customer cancelled the board" }),
        )
        .await;
        assert!(!abandoned.is_error, "{}", text(&abandoned));
        assert_eq!(body(&abandoned)["transition"], "abandon");
        let state = state_on_disk(&dir);
        assert_eq!(state.phase, CLOSED);
        let entry = state.history.last().unwrap();
        assert_eq!(entry.kind, HistoryKind::Abandon);
        assert_eq!(entry.from.as_deref(), Some("architecture"));
        assert_eq!(entry.to, CLOSED);
        assert_eq!(
            entry.reason.as_deref(),
            Some("customer cancelled the board")
        );

        let after = advance_with(
            &dir,
            &ctx(),
            &job_id,
            "closed",
            json!({ "reason": "again" }),
        )
        .await;
        assert_eq!(
            error_kind(&after),
            "stale_target",
            "a closed job moves no more"
        );
        let next = handle_flow_start(
            &json!({ "project_dir": arg(&dir), "objective": "Next board", "lane": "new_board" }),
            &ctx(),
        )
        .await
        .unwrap();
        assert!(!next.is_error, "{}", text(&next));
    }

    /// Acceptance 1.5 / D6: the cited calls are checked against the observer
    /// ring during the call, stored, and never used to refuse.
    #[tokio::test]
    async fn the_evidence_check_reports_what_the_ring_confirms_and_never_refuses() {
        let dir = project();
        let job_id = open(&dir, "photo_to_kicad", Some(&PHOTO_PHASES)).await;
        let ctx = ctx();
        ctx.observer
            .record(call("run_erc", CallStatus::Error))
            .await;
        ctx.observer.record(call("run_erc", CallStatus::Ok)).await;
        ctx.observer
            .record(call("get_drc_violations", CallStatus::Error))
            .await;

        let result = advance_with(
            &dir,
            &ctx,
            &job_id,
            "schematic_review",
            json!({
                "records": [{ "filename": "schematic-evidence.md", "content": "ERC clean\n" }],
                "evidence_calls": ["run_erc", "render_schematic_png"]
            }),
        )
        .await;
        assert!(!result.is_error, "{}", text(&result));
        let expected = EvidenceCheck {
            confirmed: vec!["run_erc".into()],
            not_ok: Vec::new(),
            absent: vec!["render_schematic_png".into()],
            ring_calls: 3,
        };
        let entry = state_on_disk(&dir).history.last().unwrap().clone();
        assert_eq!(entry.evidence_check, Some(expected.clone()));
        assert_eq!(entry.evidence_calls, ["run_erc", "render_schematic_png"]);
        assert_eq!(body(&result)["evidence_check"], json!(expected));
        let status = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx)
            .await
            .unwrap();
        assert_eq!(
            body(&status)["last_transition"]["evidence_check"]["confirmed"],
            json!(["run_erc"])
        );

        let second = advance_with(
            &dir,
            &ctx,
            &job_id,
            "placement",
            json!({
                "records": [{ "filename": "ledger-schematic.md", "content": "no findings\n" }],
                "evidence_calls": ["get_drc_violations"]
            }),
        )
        .await;
        assert!(!second.is_error, "{}", text(&second));
        let check = state_on_disk(&dir)
            .history
            .last()
            .unwrap()
            .evidence_check
            .clone()
            .unwrap();
        assert_eq!(
            check.not_ok,
            ["get_drc_violations"],
            "present only as an error"
        );
        assert!(check.confirmed.is_empty() && check.absent.is_empty());
    }
}

// ─── Tests: flow_gate (task 1.6) ──────────────────────────────────────────────

#[cfg(test)]
mod gate_tests {
    use super::advance_tests::*;
    use super::rewind_tests::PHOTO_PHASES;
    use super::test_support::*;
    use super::*;
    use crate::mcp::protocol::CallToolResult;
    use serde_json::json;

    pub(super) async fn gate(
        dir: &tempfile::TempDir,
        job_id: &str,
        gate_name: &str,
        decision: &str,
        user_words: &str,
    ) -> CallToolResult {
        handle_flow_gate(
            &json!({
                "project_dir": arg(dir),
                "job_id": job_id,
                "gate_name": gate_name,
                "decision": decision,
                "summary": "Showed the package and the renders.",
                "user_words": user_words,
            }),
            &ctx(),
        )
        .await
        .unwrap()
    }

    fn gate_file(dir: &tempfile::TempDir, gate_name: &str) -> std::path::PathBuf {
        flow_path(dir)
            .join("records")
            .join("gates")
            .join(format!("{gate_name}.md"))
    }

    async fn open_in(dir: &tempfile::TempDir, lane: &str, phases: &[&str], mode: &str) -> String {
        let result = handle_flow_start(
            &json!({
                "project_dir": arg(dir),
                "objective": "Demo board",
                "lane": lane,
                "phases": phases,
                "mode": mode,
            }),
            &ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{}", text(&result));
        body(&result)["job_id"].as_str().unwrap().to_string()
    }

    /// A photo-lane job standing at `gate:placement`, entered by the real tool.
    pub(super) async fn at_placement_gate(dir: &tempfile::TempDir, mode: &str) -> String {
        let job_id = open_in(dir, "photo_to_kicad", &PHOTO_PHASES, mode).await;
        for (to_phase, record) in [
            ("schematic_review", "schematic-evidence.md"),
            ("placement", "ledger-schematic.md"),
            ("gate:placement", "placement.md"),
        ] {
            let moved = advance(dir, &job_id, to_phase, &[(record, "# Record\n")]).await;
            assert!(!moved.is_error, "{to_phase}: {}", text(&moved));
        }
        job_id
    }

    /// Acceptance 1.6: D5 is re-checked at approval. The gate-entry keys are
    /// re-planted over the BLOCKED file, so the readiness rule is the ONLY one
    /// that can refuse — the stale-package rule would otherwise mask it.
    #[tokio::test]
    async fn approving_architecture_against_a_blocked_readiness_writes_no_gate_file() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        std::fs::write(
            record_path(&dir, "architecture.md"),
            "# A\n\nReadiness: BLOCKED — no input voltage range\n",
        )
        .unwrap();
        let mut state = state_on_disk(&dir);
        let package =
            current_package(&flow_path(&dir), &state.phases, "gate:architecture").unwrap();
        let entered = state.history.last_mut().unwrap();
        entered.package_hash = Some(package.hash);
        entered.package_files = package.files;
        plant_state(&dir, &state);
        let before = state_bytes(&dir);

        let refused = gate(&dir, &job_id, "architecture", "approve", "aprovado").await;
        assert_eq!(error_kind(&refused), "invalid_argument");
        assert!(
            text(&refused).contains("BLOCKED") && text(&refused).contains("no input voltage range"),
            "{}",
            text(&refused)
        );
        assert!(!gate_file(&dir, "architecture").exists());
        assert_eq!(state_bytes(&dir), before);
    }

    /// Acceptance 1.6: empty `user_words` is refused in a guided job, and for
    /// `purchase` even in an autonomous one.
    #[tokio::test]
    async fn an_approval_without_the_users_words_is_refused_in_guided_mode_and_at_purchase() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        let before = state_bytes(&dir);
        for words in ["", "   "] {
            let refused = gate(&dir, &job_id, "architecture", "approve", words).await;
            assert_eq!(error_kind(&refused), "invalid_argument", "{words:?}");
            assert_eq!(body(&refused)["error"]["field"], "user_words", "{words:?}");
            assert!(!gate_file(&dir, "architecture").exists());
            assert_eq!(state_bytes(&dir), before);
        }

        let fab = project();
        let fab_job = open_in(
            &fab,
            "fab_only",
            &["manufacturing", "gate:purchase"],
            "autonomous",
        )
        .await;
        let entered = advance(
            &fab,
            &fab_job,
            "gate:purchase",
            &[("manufacturing.md", "# Package\n")],
        )
        .await;
        assert!(!entered.is_error, "{}", text(&entered));
        let before = state_bytes(&fab);
        let refused = gate(&fab, &fab_job, "purchase", "approve", "").await;
        assert_eq!(error_kind(&refused), "invalid_argument");
        assert_eq!(body(&refused)["error"]["field"], "user_words");
        assert!(text(&refused).contains("purchase"), "{}", text(&refused));
        assert!(!gate_file(&fab, "purchase").exists());
        assert_eq!(state_bytes(&fab), before);
    }

    /// Acceptance 1.6: an autonomous session may approve `placement` itself.
    #[tokio::test]
    async fn autonomous_mode_lets_the_session_approve_placement() {
        let dir = project();
        let job_id = at_placement_gate(&dir, "autonomous").await;
        let approved = gate(&dir, &job_id, "placement", "approve", "").await;
        assert!(!approved.is_error, "{}", text(&approved));
        assert_eq!(body(&approved)["approved_by"], "session");

        let state = state_on_disk(&dir);
        let approval = &state.gate_approvals["placement"];
        assert_eq!(approval.approved_by, ApprovedBy::Session);
        assert_eq!(approval.user_words, "");
        assert_eq!(Some(approval.visit), current_visit(&state));
        let (design_hash, _) =
            crate::design_hash::design_state_hash(&dir.path().canonicalize().unwrap()).unwrap();
        assert_eq!(approval.design_hash_at_approval, design_hash);
        let file = std::fs::read_to_string(gate_file(&dir, "placement")).unwrap();
        assert!(
            file.contains("approve") && file.contains("session"),
            "{file}"
        );

        let left = advance(&dir, &job_id, "routing", &[]).await;
        assert!(!left.is_error, "{}", text(&left));
    }

    /// Acceptance 1.6 / architect's mistake #2: approval compares against the
    /// keys of the entry that ENTERED the gate — a `.kicad_pcb` byte changed
    /// since then is refused, although now-vs-now would match.
    #[tokio::test]
    async fn a_board_changed_since_the_gate_was_entered_is_a_stale_target() {
        let dir = project();
        std::fs::write(dir.path().join("demo.kicad_pcb"), "(kicad_pcb)\n").unwrap();
        let job_id = at_placement_gate(&dir, "guided").await;
        let before = state_bytes(&dir);

        std::fs::write(dir.path().join("demo.kicad_pcb"), "(kicad_pcb )\n").unwrap();
        let refused = gate(&dir, &job_id, "placement", "approve", "pode fabricar").await;
        assert_eq!(error_kind(&refused), "stale_target");
        assert_eq!(body(&refused)["error"]["target"], "gate:placement");
        assert!(!gate_file(&dir, "placement").exists());
        assert_eq!(state_bytes(&dir), before);
        std::fs::write(dir.path().join("demo.kicad_pcb"), "(kicad_pcb)\n").unwrap();

        std::fs::write(record_path(&dir, "placement.md"), "# Record, edited\n").unwrap();
        let package_changed = gate(&dir, &job_id, "placement", "approve", "pode fabricar").await;
        assert_eq!(error_kind(&package_changed), "stale_target");
        assert!(
            text(&package_changed).contains("placement.md"),
            "{}",
            text(&package_changed)
        );
        assert_eq!(state_bytes(&dir), before);
        std::fs::write(record_path(&dir, "placement.md"), "# Record\n").unwrap();

        let approved = gate(&dir, &job_id, "placement", "approve", "pode fabricar").await;
        assert!(!approved.is_error, "{}", text(&approved));
    }

    /// A guided approval records the D11 keys of the gate entry and its
    /// visit, writes the gate file with the user's words verbatim, logs it,
    /// and is what lets the job leave the gate.
    #[tokio::test]
    async fn a_guided_approval_binds_to_the_gate_entry_and_lets_the_job_leave() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        let words = "Pode seguir: \"USB-C\" ok\nsem o LDO extra";
        let approved = gate(&dir, &job_id, "architecture", "approve", words).await;
        assert!(!approved.is_error, "{}", text(&approved));

        let state = state_on_disk(&dir);
        let visit = state.history.len() - 1;
        let entered = &state.history[visit];
        let approval = &state.gate_approvals["architecture"];
        assert_eq!(approval.decision, GateDecision::Approve);
        assert_eq!(approval.approved_by, ApprovedBy::User);
        assert_eq!(approval.visit, visit);
        assert_eq!(approval.design_hash_at_approval, entered.design_hash);
        assert_eq!(
            Some(approval.package_hash_at_approval.as_str()),
            entered.package_hash.as_deref()
        );
        assert_eq!(approval.user_words, words);
        assert_eq!(approval.summary, "Showed the package and the renders.");
        assert_eq!(
            state.phase, "gate:architecture",
            "a decision moves no phase"
        );

        let file = std::fs::read_to_string(gate_file(&dir, "architecture")).unwrap();
        assert!(
            file.contains("> Pode seguir: \"USB-C\" ok\n> sem o LDO extra"),
            "{file}"
        );
        assert!(file.contains(&approval.package_hash_at_approval), "{file}");
        let log = std::fs::read_to_string(
            flow_path(&dir)
                .join("log")
                .join(format!("{}-{job_id}.md", &state.started_at[..10])),
        )
        .unwrap();
        assert!(log.contains("gate architecture approve"), "{log}");

        let left = advance(&dir, &job_id, "schematic", &[]).await;
        assert!(!left.is_error, "{}", text(&left));
    }

    #[tokio::test]
    async fn a_decision_needs_the_job_at_that_gate_and_a_summary() {
        let dir = project();
        let orphan = gate(
            &dir,
            "demo-20260921-140000",
            "architecture",
            "approve",
            "ok",
        )
        .await;
        assert_eq!(error_kind(&orphan), "stale_target");
        assert!(!dir.path().join(".konnect").exists(), "creates nothing");

        let working = open(&dir, "new_board", None).await;
        let not_at_gate = gate(&dir, &working, "architecture", "approve", "ok").await;
        assert_eq!(error_kind(&not_at_gate), "invalid_argument");
        assert_eq!(body(&not_at_gate)["error"]["field"], "gate_name");

        let at_gate = project();
        let job_id = at_architecture_gate(&at_gate).await;
        let before = state_bytes(&at_gate);
        let other_gate = gate(&at_gate, &job_id, "placement", "approve", "ok").await;
        assert_eq!(error_kind(&other_gate), "invalid_argument");
        assert_eq!(body(&other_gate)["error"]["field"], "gate_name");
        let foreign = gate(
            &at_gate,
            "another-20260101-000000",
            "architecture",
            "approve",
            "ok",
        )
        .await;
        assert_eq!(error_kind(&foreign), "stale_target");
        let no_summary = handle_flow_gate(
            &json!({
                "project_dir": arg(&at_gate),
                "job_id": job_id,
                "gate_name": "architecture",
                "decision": "approve",
                "summary": " ",
                "user_words": "ok",
            }),
            &ctx(),
        )
        .await
        .unwrap();
        assert_eq!(error_kind(&no_summary), "invalid_argument");
        assert_eq!(body(&no_summary)["error"]["field"], "summary");
        assert!(!gate_file(&at_gate, "architecture").exists());
        assert_eq!(state_bytes(&at_gate), before);
    }

    /// `reject` removes the approval, records the rejection in the gate file
    /// and the log, and leaves the phase where it is.
    #[tokio::test]
    async fn a_rejection_removes_the_approval_and_keeps_the_phase() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        assert!(
            !gate(&dir, &job_id, "architecture", "approve", "sim")
                .await
                .is_error
        );
        let rejected = gate(&dir, &job_id, "architecture", "reject", "").await;
        assert!(!rejected.is_error, "{}", text(&rejected));
        assert_eq!(body(&rejected)["removed_approval"], true);

        let state = state_on_disk(&dir);
        assert!(state.gate_approvals.is_empty());
        assert_eq!(state.phase, "gate:architecture");
        let file = std::fs::read_to_string(gate_file(&dir, "architecture")).unwrap();
        assert!(file.contains("reject"), "{file}");
        let left = advance(&dir, &job_id, "schematic", &[]).await;
        assert_eq!(error_kind(&left), "stale_target");
    }

    /// Fix round 1, DECISION C (reviewer 11 minor 1): `STATE.md` is the one
    /// commit point. `records/gates` is a plain file, so the gate file cannot
    /// be written — the approval is still committed, the log still records
    /// it, and the failure comes back as a `warning` on a success, never as
    /// an error inviting a retry of a decision `STATE.md` already holds.
    #[tokio::test]
    async fn a_gate_file_write_failure_after_state_commits_is_a_warning() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        let blocker = flow_path(&dir).join("records").join("gates");
        std::fs::write(&blocker, "not a directory\n").unwrap();

        let approved = gate(&dir, &job_id, "architecture", "approve", "pode seguir").await;
        assert!(!approved.is_error, "{}", text(&approved));
        let warning = body(&approved)["warning"]
            .as_str()
            .unwrap_or_else(|| panic!("a warning field: {}", text(&approved)))
            .to_string();
        assert!(
            warning.contains(&format!("records{}gates", std::path::MAIN_SEPARATOR)),
            "the warning names the failed write: {warning}"
        );
        assert_eq!(body(&approved)["decision"], "approve");

        let status = handle_flow_status(&json!({ "project_dir": arg(&dir) }), &ctx())
            .await
            .unwrap();
        let approval = &body(&status)["gate_approvals"]["architecture"];
        assert_eq!(approval["status"], "current", "{}", text(&status));
        assert_eq!(approval["approved_by"], "user", "{}", text(&status));
        assert_eq!(approval["user_words"], "pode seguir");
        assert!(blocker.is_file(), "the blocker is left as it was");
        let log_dir = flow_path(&dir).join("log");
        let log = std::fs::read_dir(&log_dir)
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<String>();
        assert!(
            log.contains("gate architecture approve"),
            "the log entry is still written after the commit: {log}"
        );
        let left = advance(&dir, &job_id, "schematic", &[]).await;
        assert!(
            !left.is_error,
            "the committed approval opens the gate: {}",
            text(&left)
        );
    }

    /// After a rewind to the gate's phase and the forward move back into the
    /// gate, the approval binds to the RE-ENTRY's keys (the package as fixed),
    /// and leaving works again. (Before Fix round 1 this rewound INTO the
    /// gate; DECISION B now refuses that.)
    #[tokio::test]
    async fn an_approval_after_a_rewind_and_re_entry_binds_to_the_re_entry() {
        let dir = project();
        let job_id = at_architecture_gate(&dir).await;
        assert!(
            !gate(&dir, &job_id, "architecture", "approve", "sim")
                .await
                .is_error
        );
        assert!(!advance(&dir, &job_id, "schematic", &[]).await.is_error);
        let rewound = advance_with(
            &dir,
            &ctx(),
            &job_id,
            "architecture",
            json!({ "reason": "pin conflict" }),
        )
        .await;
        assert!(!rewound.is_error, "{}", text(&rewound));
        let re_entered = advance(
            &dir,
            &job_id,
            "gate:architecture",
            &[
                ("architecture.md", PASSING_ARCHITECTURE),
                ("worst-case.md", "# WC\n"),
                ("pin-plan.md", "# Pins, fixed\n"),
            ],
        )
        .await;
        assert!(!re_entered.is_error, "{}", text(&re_entered));

        let approved = gate(&dir, &job_id, "architecture", "approve", "agora sim").await;
        assert!(!approved.is_error, "{}", text(&approved));
        let state = state_on_disk(&dir);
        let approval = &state.gate_approvals["architecture"];
        assert_eq!(approval.visit, state.history.len() - 1);
        assert_eq!(
            Some(approval.package_hash_at_approval.as_str()),
            state.history.last().unwrap().package_hash.as_deref()
        );
        let left = advance(&dir, &job_id, "schematic", &[]).await;
        assert!(!left.is_error, "{}", text(&left));
    }
}

// ─── Tests: flow_log and flow_defer (task 1.7) ────────────────────────────────

#[cfg(test)]
mod journal_tests {
    use super::advance_tests::*;
    use super::test_support::*;
    use super::*;
    use crate::mcp::protocol::CallToolResult;
    use serde_json::{json, Value};

    async fn log(
        dir: &tempfile::TempDir,
        job_id: &str,
        kind: &str,
        extra: Value,
    ) -> CallToolResult {
        let mut args = json!({
            "project_dir": arg(dir),
            "job_id": job_id,
            "kind": kind,
            "message": "USB-C only; no micro-USB footprint.",
        });
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        handle_flow_log(&args, &ctx()).await.unwrap()
    }

    async fn defer(
        dir: &tempfile::TempDir,
        job_id: &str,
        kind: &str,
        extra: Value,
    ) -> CallToolResult {
        let mut args = json!({
            "project_dir": arg(dir),
            "job_id": job_id,
            "kind": kind,
            "description": "silk R3 overlaps pad",
        });
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        handle_flow_defer(&args, &ctx()).await.unwrap()
    }

    fn log_path(dir: &tempfile::TempDir, job_id: &str) -> std::path::PathBuf {
        let state = state_on_disk(dir);
        flow_path(dir)
            .join("log")
            .join(format!("{}-{job_id}.md", &state.started_at[..10]))
    }

    /// Every byte a flow_log refusal could have touched: the log, both lesson
    /// destinations, the handoff directory's listing and STATE.md.
    fn journal_snapshot(dir: &tempfile::TempDir, job_id: &str) -> Vec<Option<Vec<u8>>> {
        let flow = flow_path(dir);
        let mut snapshot: Vec<Option<Vec<u8>>> = [
            log_path(dir, job_id),
            flow.join("records").join("lessons-candidates.md"),
            flow.join("STATE.md"),
        ]
        .iter()
        .map(|path| std::fs::read(path).ok())
        .collect();
        for role in ROLES {
            snapshot.push(std::fs::read(flow.join("memory").join(format!("{role}.md"))).ok());
        }
        snapshot.push(Some(list_handoffs(&flow, job_id).join("\n").into_bytes()));
        snapshot
    }

    /// Acceptance 1.7: a decision needs a non-empty `why` and `rollback`; a
    /// parameter that does not apply to the kind (`scope` on evidence) is
    /// refused; a refusal appends nothing anywhere.
    #[tokio::test]
    async fn a_decision_needs_why_and_rollback_and_evidence_takes_no_scope() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let before = journal_snapshot(&dir, &job_id);
        for (kind, extra, field) in [
            (
                "decision",
                json!({ "why": "", "rollback": "revert" }),
                "why",
            ),
            ("decision", json!({ "rollback": "revert" }), "why"),
            (
                "decision",
                json!({ "why": "cost", "rollback": "  " }),
                "rollback",
            ),
            ("decision", json!({ "why": "cost" }), "rollback"),
            (
                "decision",
                json!({ "why": "c", "rollback": "r", "scope": "project" }),
                "scope",
            ),
            ("evidence", json!({ "scope": "project" }), "scope"),
            ("evidence", json!({ "why": "because" }), "why"),
            ("evidence", json!({ "message": " " }), "message"),
        ] {
            let refused = log(&dir, &job_id, kind, extra.clone()).await;
            assert_eq!(error_kind(&refused), "invalid_argument", "{kind} {extra}");
            assert_eq!(body(&refused)["error"]["field"], field, "{kind} {extra}");
            assert_eq!(
                journal_snapshot(&dir, &job_id),
                before,
                "{kind} {extra}: nothing appended"
            );
        }

        let decided = log(
            &dir,
            &job_id,
            "decision",
            json!({ "why": "the enclosure has a USB-C cutout", "rollback": "swap J1", "role": "requirements" }),
        )
        .await;
        assert!(!decided.is_error, "{}", text(&decided));
        assert_eq!(body(&decided)["read_name"], "log");
        let evidence = log(&dir, &job_id, "evidence", json!({})).await;
        assert!(!evidence.is_error, "{}", text(&evidence));
        let text = std::fs::read_to_string(log_path(&dir, &job_id)).unwrap();
        assert!(text.contains("decision"), "{text}");
        assert!(
            text.contains("> USB-C only; no micro-USB footprint."),
            "{text}"
        );
        assert!(
            text.contains("the enclosure has a USB-C cutout") && text.contains("swap J1"),
            "{text}"
        );
        assert!(text.contains("evidence"), "{text}");
        assert_eq!(
            state_bytes(&dir),
            before[2].clone().unwrap(),
            "flow_log never rewrites STATE.md"
        );
    }

    /// Acceptance 1.7: each handoff is a new file numbered after the ones
    /// already there, holding the message verbatim.
    #[tokio::test]
    async fn each_handoff_gets_its_own_numbered_file() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let handoffs = flow_path(&dir).join("handoffs").join(&job_id);
        for (message, expected) in [
            ("---\nverdict: FIX\n---\n# Review one\n", "01-review.md"),
            ("---\nverdict: DONE\n---\n# Review two\n", "02-review.md"),
        ] {
            let result = log(
                &dir,
                &job_id,
                "handoff",
                json!({ "role": "review", "message": message }),
            )
            .await;
            assert!(!result.is_error, "{}", text(&result));
            assert_eq!(body(&result)["read_name"], format!("handoffs/{expected}"));
            assert_eq!(
                std::fs::read_to_string(handoffs.join(expected)).unwrap(),
                message
            );
        }
        let status = handle_flow_status(
            &json!({ "project_dir": arg(&dir), "read": ["handoffs/02-review.md"] }),
            &ctx(),
        )
        .await
        .unwrap();
        assert_eq!(
            body(&status)["handoffs"],
            json!(["01-review.md", "02-review.md"])
        );
        assert!(body(&status)["contents"]["handoffs/02-review.md"]
            .as_str()
            .unwrap()
            .contains("Review two"));

        let before = journal_snapshot(&dir, &job_id);
        for (extra, field) in [
            (json!({}), "role"),
            (json!({ "role": "review", "scope": "role" }), "scope"),
            (json!({ "role": "review", "rollback": "r" }), "rollback"),
        ] {
            let refused = log(&dir, &job_id, "handoff", extra.clone()).await;
            assert_eq!(error_kind(&refused), "invalid_argument", "{extra}");
            assert_eq!(body(&refused)["error"]["field"], field, "{extra}");
            assert_eq!(journal_snapshot(&dir, &job_id), before, "{extra}");
        }
    }

    /// D7: a project lesson goes to memory/<role>.md, a role or technology
    /// lesson to the candidates queue tagged with both; both need role and
    /// scope, and both are readable through flow_status.
    #[tokio::test]
    async fn lessons_go_to_project_memory_or_the_candidates_queue() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let before = journal_snapshot(&dir, &job_id);
        for (extra, field) in [
            (json!({ "scope": "project" }), "role"),
            (json!({ "role": "layout" }), "scope"),
            (
                json!({ "role": "layout", "scope": "project", "why": "w" }),
                "why",
            ),
        ] {
            let refused = log(&dir, &job_id, "lesson", extra.clone()).await;
            assert_eq!(error_kind(&refused), "invalid_argument", "{extra}");
            assert_eq!(body(&refused)["error"]["field"], field, "{extra}");
            assert_eq!(journal_snapshot(&dir, &job_id), before, "{extra}");
        }

        let project_lesson = log(
            &dir,
            &job_id,
            "lesson",
            json!({ "role": "layout", "scope": "project", "message": "J1 sits on the bottom edge here." }),
        )
        .await;
        assert!(!project_lesson.is_error, "{}", text(&project_lesson));
        assert_eq!(body(&project_lesson)["read_name"], "memory/layout.md");
        let technology_lesson = log(
            &dir,
            &job_id,
            "lesson",
            json!({ "role": "schematic", "scope": "technology", "message": "ESP32 GPIO0 strapping needs a pull-up." }),
        )
        .await;
        assert!(!technology_lesson.is_error, "{}", text(&technology_lesson));
        assert_eq!(
            body(&technology_lesson)["read_name"],
            "lessons-candidates.md"
        );

        let status = handle_flow_status(
            &json!({ "project_dir": arg(&dir), "read": ["memory/layout.md", "lessons-candidates.md"] }),
            &ctx(),
        )
        .await
        .unwrap();
        let contents = &body(&status)["contents"];
        let memory = contents["memory/layout.md"].as_str().unwrap();
        assert!(
            memory.contains("J1 sits on the bottom edge here."),
            "{memory}"
        );
        assert!(!memory.contains("ESP32"), "{memory}");
        let candidates = contents["lessons-candidates.md"].as_str().unwrap();
        assert!(
            candidates.contains("ESP32 GPIO0 strapping needs a pull-up."),
            "{candidates}"
        );
        assert!(
            candidates.contains("role `schematic`") && candidates.contains("scope `technology`"),
            "{candidates}"
        );
        assert!(!candidates.contains("J1 sits"), "{candidates}");
    }

    /// D1: the journal stays open on a closed job, but only for the
    /// project's own job, and never creates a flow directory.
    #[tokio::test]
    async fn the_journal_accepts_a_closed_job_but_not_a_foreign_one() {
        let dir = project();
        let orphan = log(&dir, JOB_ID, "evidence", json!({})).await;
        assert_eq!(error_kind(&orphan), "stale_target");
        let orphan_defer = defer(&dir, JOB_ID, "finding", json!({})).await;
        assert_eq!(error_kind(&orphan_defer), "stale_target");
        assert!(!dir.path().join(".konnect").exists(), "creates nothing");

        plant_state(&dir, &job_at(CLOSED));
        let closed = log(&dir, JOB_ID, "evidence", json!({})).await;
        assert!(!closed.is_error, "{}", text(&closed));
        let deferred = defer(&dir, JOB_ID, "queue_item", json!({})).await;
        assert!(!deferred.is_error, "{}", text(&deferred));
        assert_eq!(state_on_disk(&dir).queue.len(), 1);

        let before = state_bytes(&dir);
        let foreign = log(&dir, "another-20260101-000000", "evidence", json!({})).await;
        assert_eq!(error_kind(&foreign), "stale_target");
        let foreign_defer = defer(&dir, "another-20260101-000000", "finding", json!({})).await;
        assert_eq!(error_kind(&foreign_defer), "stale_target");
        assert_eq!(state_bytes(&dir), before);
    }

    /// Acceptance 1.7 / pre-mortem 6: two concurrent defers serialize on the
    /// STATE.md lock and both land.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_concurrent_defers_both_land() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let (first, second) = tokio::join!(
            defer(
                &dir,
                &job_id,
                "finding",
                json!({ "description": "finding one" })
            ),
            defer(
                &dir,
                &job_id,
                "finding",
                json!({ "description": "finding two" })
            )
        );
        assert!(!first.is_error, "{}", text(&first));
        assert!(!second.is_error, "{}", text(&second));
        let mut descriptions: Vec<String> = state_on_disk(&dir)
            .deferred_findings
            .into_iter()
            .map(|item| item.description)
            .collect();
        descriptions.sort();
        assert_eq!(descriptions, ["finding one", "finding two"]);
    }

    /// Fix round 1, DECISION H: `flow_defer`'s log entry follows the
    /// `STATE.md` commit, as it does for gates and transitions. With `log` a
    /// plain file the append fails, yet the item stands in `STATE.md` and the
    /// failure is a `warning` on a success; once the log can be written, the
    /// next defer logs and reports `warning: null`.
    #[tokio::test]
    async fn a_defer_log_failure_after_state_commits_is_a_warning() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let log_dir = flow_path(&dir).join("log");
        std::fs::remove_dir_all(&log_dir).unwrap();
        std::fs::write(&log_dir, "not a directory\n").unwrap();

        let deferred = defer(&dir, &job_id, "finding", json!({})).await;
        assert!(!deferred.is_error, "{}", text(&deferred));
        assert_eq!(body(&deferred)["list"], "deferred_findings");
        let warning = body(&deferred)["warning"]
            .as_str()
            .unwrap_or_else(|| panic!("a warning field: {}", text(&deferred)))
            .to_string();
        assert!(
            warning.contains(&format!("flow{}log", std::path::MAIN_SEPARATOR)),
            "the warning names the failed write: {warning}"
        );
        let state = state_on_disk(&dir);
        assert_eq!(state.deferred_findings.len(), 1);
        assert_eq!(
            state.deferred_findings[0].description,
            "silk R3 overlaps pad"
        );
        assert!(log_dir.is_file(), "the blocker is left as it was");

        std::fs::remove_file(&log_dir).unwrap();
        let logged = defer(
            &dir,
            &job_id,
            "queue_item",
            json!({ "description": "order the reel" }),
        )
        .await;
        assert!(!logged.is_error, "{}", text(&logged));
        assert!(
            body(&logged).get("warning").is_some_and(Value::is_null),
            "warning is present and null on a clean defer: {}",
            text(&logged)
        );
        let text = std::fs::read_to_string(log_path(&dir, &job_id)).unwrap();
        assert!(
            text.contains("defer queue_item") && text.contains("order the reel"),
            "{text}"
        );
    }

    /// Each kind lands in its own list with the phase it was found in; an
    /// empty description is refused and changes nothing.
    #[tokio::test]
    async fn a_deferred_item_lands_in_its_list_with_its_phase() {
        let dir = project();
        let job_id = open(&dir, "new_board", None).await;
        let queued = defer(
            &dir,
            &job_id,
            "queue_item",
            json!({ "description": "order the reel", "owner": "sourcing" }),
        )
        .await;
        assert!(!queued.is_error, "{}", text(&queued));
        assert_eq!(body(&queued)["list"], "queue");
        let pending = defer(&dir, &job_id, "pending_approval", json!({ "owner": "" })).await;
        assert!(!pending.is_error, "{}", text(&pending));

        let state = state_on_disk(&dir);
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue[0].description, "order the reel");
        assert_eq!(state.queue[0].owner.as_deref(), Some("sourcing"));
        assert_eq!(state.queue[0].phase, "requirements");
        assert!(is_utc_timestamp(&state.queue[0].added_at));
        assert_eq!(state.pending_approvals.len(), 1);
        assert_eq!(
            state.pending_approvals[0].owner, None,
            "an empty owner is no owner"
        );
        assert!(state.deferred_findings.is_empty());
        let text = std::fs::read_to_string(log_path(&dir, &job_id)).unwrap();
        assert!(
            text.contains("defer queue_item") && text.contains("order the reel"),
            "{text}"
        );

        let before = state_bytes(&dir);
        let empty = defer(&dir, &job_id, "finding", json!({ "description": "  " })).await;
        assert_eq!(error_kind(&empty), "invalid_argument");
        assert_eq!(body(&empty)["error"]["field"], "description");
        assert_eq!(state_bytes(&dir), before);
    }
}
