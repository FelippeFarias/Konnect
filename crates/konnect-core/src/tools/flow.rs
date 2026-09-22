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

use serde::{Deserialize, Serialize};
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
