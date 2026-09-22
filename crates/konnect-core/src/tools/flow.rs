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

// Foundations land before the handlers that consume them; this allow is
// removed in the same series, once `flow_advance` uses the last of them.
#![allow(dead_code)]

use crate::design_hash::design_state_hash;
use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
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
/// phase without its gate would silently drop a human approval.
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

/// Who granted an approval: the user's own words, or the session under
/// autonomous mode (D3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApprovedBy {
    User,
    Session,
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
/// phase brings its gate. Every error quotes the offending entry.
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
        "\nValidity is recomputed by `flow_status`; this body shows what was recorded.\n\n",
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

/// Each recorded approval with `valid` recomputed: both keys still equal the
/// design and the package as they are now.
fn gate_validity(flow_dir: &Path, state: &JobState, design_hash: &str) -> Value {
    let mut gates = serde_json::Map::new();
    for (gate, approval) in &state.gate_approvals {
        let mut entry = json!(approval);
        match current_package(flow_dir, &state.phases, &format!("gate:{gate}")) {
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

// ─── Tool definitions (design D1) ─────────────────────────────────────────────

/// The `flow` tools in D1 order. The router registers them (task 1.8).
pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "flow_status",
        "Read a KiCad project's orchestration state and the reality it binds to: the \
         job and its phase (null when none), the current design_state_hash and the files \
         it covered, KiCad lock files beside them, gate approvals with a recomputed \
         `valid`, deferred items, FIX rounds per review phase, the newest transition, the \
         job's handoffs, the next step, and the content of each requested `read` name. \
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
    )]
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
        let cases: [(&[&str], &str, &str); 5] = [
            (&["schematic", "requirements"], "\"requirements\"", "order"),
            (&["schematic", "schematic"], "\"schematic\"", "repeat"),
            (&["schematic", "layout"], "\"layout\"", "not a phase"),
            (&["gate:placement", "routing"], "\"gate:placement\"", "gate"),
            (&["placement", "routing"], "\"placement\"", "gate:placement"),
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
        let mut state = job_at("schematic");
        let package = package_hash(
            &flow_path(&dir).join("records"),
            &package_records(&state.phases, "gate:architecture"),
        )
        .unwrap();
        state.gate_approvals.insert(
            "architecture".into(),
            GateApproval {
                decision: GateDecision::Approve,
                approved_by: ApprovedBy::User,
                approved_at: "2026-09-21T15:00:00Z".into(),
                design_hash_at_approval: design_hash,
                package_hash_at_approval: package.hash,
                visit: 1,
                summary: "ok".into(),
                user_words: "aprovado".into(),
            },
        );
        let mut rewind = HistoryEntry::new(
            HistoryKind::Rewind,
            Some("schematic_review"),
            "schematic",
            "2026-09-21T16:00:00Z",
            "0000",
        );
        rewind.reason = Some("ERC finding".into());
        state.history.push(rewind);
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
    }
}
