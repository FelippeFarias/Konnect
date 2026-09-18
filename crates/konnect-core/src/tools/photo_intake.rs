//! `photo_intake` toolset — wraps the external `retrace` Python package as a
//! Konnect-native subprocess tool.
//!
//! `retrace` is optional and is never shipped: [`check_retrace`](tools) reports
//! its absence as a fact, not as a tool failure, exactly as
//! `handle_check_freerouting` (`integration.rs:1710`) does for Freerouting.
//!
//! Two properties of this toolset are load-bearing and are implemented here
//! rather than asked of the caller:
//!
//! - **Every** subprocess runs with `HOME` *and* `USERPROFILE` redirected to a
//!   directory scoped to that single call. `retrace` writes three global JSON
//!   stores under `Path.home()` on every run with no environment override, so
//!   process-level redirection is the only thing that keeps a scan out of the
//!   real user's `~/.local/share/retrace`.
//! - `retrace` writes `""`, never `null`, for an absent `marking`,
//!   `part_number`, `datasheet_url`, `value` or `package`. Those five fields
//!   deserialize through [`empty_string_as_none`] so that "retrace read no
//!   marking" and "retrace read a marking" stay distinguishable downstream.
//!
//! No tool in this toolset touches a live KiCad board, so every one of them
//! keeps the default [`BoardAccess::None`](crate::tools::BoardAccess) and none
//! calls `with_board_access`.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{require_str, ToolContext, ToolDef};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, info, warn};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Capability probes are a single import; they never do work. Matches
/// `run_java_command`'s 10 s probe ceiling (`integration.rs:1702`).
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Default scan ceiling. Roughly 4x headroom over the "<30 s on a known sample
/// image" target, and deliberately far below `LONG_TIMEOUT`'s 600 s
/// (`cli.rs:23`), which exists for whole-board KiCad renders.
const DEFAULT_SCAN_TIMEOUT_SECS: u64 = 120;
const MIN_SCAN_TIMEOUT_SECS: u64 = 5;
const MAX_SCAN_TIMEOUT_SECS: u64 = 1800;

/// Printed by the probe: the version proves `retrace` imports, and
/// `sys.executable` names the interpreter that imported it — so the path
/// reported back, and the path `-m retrace` later runs on, is the one the
/// probe actually validated.
const VERSION_PROBE: &str =
    "import retrace, sys; print(retrace.__version__); print(sys.executable)";

/// `detection` is the YOLO path (`ultralytics`), `ocr` is chip-marking OCR
/// (`easyocr`) — the two packages named by retrace's own fallback warnings.
const EXTRAS_PROBE: &str = "import importlib.util as u\n\
def has(m):\n\
    try:\n\
        return u.find_spec(m) is not None\n\
    except Exception:\n\
        return False\n\
print('detection=%d' % has('ultralytics'))\n\
print('ocr=%d' % has('easyocr'))";

const INSTALL_NOTE: &str =
    "retrace is not importable by any candidate interpreter. Install it with \
     `pip install git+https://github.com/ericrihm/retrace.git`, then pass `python_path` or set \
     `photo_intake.retrace_python_path` in your Konnect config.";

const FULL_INSTALL_NOTE: &str =
    "retrace and both optional extras (detection, ocr) are importable. Scans use the ML path.";

const CONTOUR_FALLBACK_NOTE: &str =
    "retrace is installed without its ML extras, so scan_pcb_photo will use the OpenCV \
     contour-only fallback: components come back with confidence 0.5 and coarse labels, and \
     marking/value/part_number are not attempted.";

// ─── Typed `analysis.json` ────────────────────────────────────────────────────

/// `analysis.json` as `retrace scan --format json` writes it (verified against
/// retrace 0.3.0 on Windows; the capture is checked in at
/// `tests/fixtures/photo_intake/analysis.json`).
///
/// Every non-identifier field is optional so that an added or removed key in a
/// later retrace release degrades the result rather than failing the parse.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetraceAnalysis {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub components: Vec<RetraceComponent>,
    #[serde(default)]
    pub traces: Vec<RetraceTrace>,
    #[serde(default)]
    pub pattern_matches: Vec<RetracePatternMatch>,
    /// Carried through untyped: it is a summary of the fields above, and
    /// retrace is free to add counters to it.
    #[serde(default)]
    pub summary: serde_json::Value,
}

/// One detected component. `bbox` is `[x, y, w, h]` in source-image pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetraceComponent {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    pub bbox: [i64; 4],
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub marking: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub part_number: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub datasheet_url: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub value: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub package: Option<String>,
}

/// One traced conductor. Empty on the contour-only path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetraceTrace {
    pub id: String,
    #[serde(default)]
    pub points: Vec<[i64; 2]>,
    #[serde(default)]
    pub width_px: Option<f64>,
    #[serde(default)]
    pub from_component: Option<String>,
    #[serde(default)]
    pub to_component: Option<String>,
}

/// One canonical-subcircuit match. Surfaced read-only: no tool in this crate
/// creates, types, values or approves a component from one, and `score` /
/// `is_partial` are carried verbatim rather than collapsed into a verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetracePatternMatch {
    pub pattern_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub component_roles: BTreeMap<String, String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub is_partial: Option<bool>,
}

/// retrace writes `""` for a field it did not read. A plain `Option<String>`
/// turns that into `Some("")`, which reads downstream as a value the scan
/// produced — the silent-guess failure this toolset exists to prevent. Only
/// whitespace counts as absent; a real value is never rewritten.
fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.filter(|value| !value.trim().is_empty()))
}

// ─── Subprocess runner ────────────────────────────────────────────────────────

/// Why a `retrace` subprocess produced no output to read. Kept separate from
/// `anyhow` so a caller can report a timeout differently from a spawn failure
/// without matching on message text.
#[derive(Debug)]
pub(crate) enum RetraceRunError {
    /// The process could not be started, or died before producing output.
    Spawn { program: String, message: String },
    /// The process was still running at the deadline and was killed.
    Timeout { seconds: u64 },
}

impl std::fmt::Display for RetraceRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetraceRunError::Spawn { program, message } => {
                write!(formatter, "failed to run '{program}': {message}")
            }
            RetraceRunError::Timeout { seconds } => {
                write!(formatter, "timed out after {seconds} seconds")
            }
        }
    }
}

/// Run one subprocess with its home directory scoped to this call.
///
/// `program` is the resolved interpreter on every production path; it is a
/// parameter rather than a fixed `python` so the redirection itself is
/// testable without an interpreter installed.
///
/// `kill_on_drop(true)` is not decoration: a timed-out retrace keeps writing
/// into `scoped_home` after the caller has given up on it, and on Windows
/// holds a handle that blocks that directory's cleanup.
pub(crate) async fn run_retrace(
    program: &Path,
    args: &[String],
    scoped_home: &Path,
    run_timeout: Duration,
) -> Result<std::process::Output, RetraceRunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .env("HOME", scoped_home)
        .env("USERPROFILE", scoped_home)
        .kill_on_drop(true);

    debug!(
        program = %program.display(),
        args = ?args,
        home = %scoped_home.display(),
        "[BETA] retrace subprocess"
    );

    match tokio::time::timeout(run_timeout, command.output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(RetraceRunError::Spawn {
            program: program.display().to_string(),
            message: error.to_string(),
        }),
        Err(_) => Err(RetraceRunError::Timeout {
            seconds: run_timeout.as_secs(),
        }),
    }
}

/// A private twin of `cli_failure_diagnostics` (`cli.rs:25`), which is private
/// to `cli.rs`. Widening that module's surface to save three lines would trade
/// a real API boundary for a cosmetic one.
fn retrace_diagnostics(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = stdout.trim();
    let stderr = stderr.trim();

    match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("stdout:\n{stdout}\nstderr:\n{stderr}"),
        (false, true) => format!("stdout:\n{stdout}"),
        (true, false) => format!("stderr:\n{stderr}"),
        (true, true) => "no diagnostic output".to_string(),
    }
}

/// A fresh home directory for one call's subprocesses. Dropped — and deleted —
/// when the call returns.
fn scoped_home_dir() -> std::io::Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(".konnect-retrace-home-")
        .tempdir()
}

/// `canonicalize` returns `\\?\C:\…` on Windows. That prefix is an OS escape
/// for the 260-character limit, not part of the path, and not every Python
/// library accepts one — the same reason `portable_uri` (`library.rs:1818`)
/// strips it. Separators are left native: the subprocess is a local one.
fn subprocess_arg(path: &Path) -> String {
    let raw = path.to_string_lossy().into_owned();
    raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()
}

// ─── Interpreter discovery and capability probe ───────────────────────────────

/// A candidate interpreter as an argv prefix, because the Windows launcher is
/// `py -3` rather than a single program name.
type Candidate = Vec<String>;

/// Discovery order: explicit argument, configured path, `RETRACE_PYTHON`, then
/// PATH. `py -3` leads on Windows because the PEP 397 launcher resolves a
/// registered install without PATH mutation, while a bare `python` there is
/// frequently the Microsoft Store alias stub; `python3` leads `python`
/// elsewhere because `python` still means Python 2 on some systems.
fn candidate_interpreters(argument: Option<&str>, configured: Option<&str>) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = Vec::new();

    for explicit in [argument, configured] {
        if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
            push_candidate(&mut candidates, vec![value.to_string()]);
        }
    }
    if let Some(value) = std::env::var_os("RETRACE_PYTHON") {
        let value = value.to_string_lossy().trim().to_string();
        if !value.is_empty() {
            push_candidate(&mut candidates, vec![value]);
        }
    }
    if cfg!(windows) {
        push_candidate(&mut candidates, vec!["py".to_string(), "-3".to_string()]);
    }
    push_candidate(&mut candidates, vec!["python3".to_string()]);
    push_candidate(&mut candidates, vec!["python".to_string()]);

    candidates
}

fn push_candidate(candidates: &mut Vec<Candidate>, candidate: Candidate) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

/// What a resolved interpreter can do. `python_path` is `sys.executable` as
/// reported by the interpreter that imported `retrace`, so "which interpreter
/// did you use" is an answer rather than an inference.
#[derive(Debug, Clone)]
pub(crate) struct RetraceCapability {
    pub python_path: PathBuf,
    pub retrace_version: Option<String>,
    pub extras: RetraceExtras,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RetraceExtras {
    pub detection: bool,
    pub ocr: bool,
}

/// Probe one candidate. A failure is a string, never an error result: absence
/// is what this function is for.
async fn probe_candidate(
    candidate: &Candidate,
    scoped_home: &Path,
) -> Result<(PathBuf, Option<String>), String> {
    let (program, prefix) = candidate.split_first().ok_or("empty candidate")?;
    let mut args: Vec<String> = prefix.to_vec();
    args.push("-c".to_string());
    args.push(VERSION_PROBE.to_string());

    let output = run_retrace(Path::new(program), &args, scoped_home, PROBE_TIMEOUT)
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(retrace_diagnostics(&output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_version_probe(&stdout, Path::new(program)))
}

/// First line is `retrace.__version__`, second is `sys.executable`. A missing
/// second line falls back to the candidate's own program name rather than
/// inventing a path.
fn parse_version_probe(stdout: &str, fallback: &Path) -> (PathBuf, Option<String>) {
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let version = lines.next().map(str::to_string);
    let executable = lines
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.to_path_buf());
    (executable, version)
}

async fn probe_extras(python: &Path, scoped_home: &Path) -> RetraceExtras {
    let args = vec!["-c".to_string(), EXTRAS_PROBE.to_string()];
    match run_retrace(python, &args, scoped_home, PROBE_TIMEOUT).await {
        Ok(output) if output.status.success() => {
            parse_extras(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(output) => {
            warn!(
                "[BETA] retrace extras probe failed: {}",
                retrace_diagnostics(&output)
            );
            RetraceExtras::default()
        }
        Err(error) => {
            warn!("[BETA] retrace extras probe failed: {error}");
            RetraceExtras::default()
        }
    }
}

fn parse_extras(stdout: &str) -> RetraceExtras {
    RetraceExtras {
        detection: stdout.contains("detection=1"),
        ocr: stdout.contains("ocr=1"),
    }
}

/// Resolve the first candidate that can import `retrace`, and report what its
/// extras are. `Err` carries the diagnostics of the last candidate that got
/// far enough to say something.
pub(crate) async fn resolve_retrace(
    argument: Option<&str>,
    configured: Option<&str>,
    scoped_home: &Path,
) -> Result<RetraceCapability, Vec<String>> {
    let mut attempts = Vec::new();
    for candidate in candidate_interpreters(argument, configured) {
        match probe_candidate(&candidate, scoped_home).await {
            Ok((python_path, retrace_version)) => {
                info!(
                    python = %python_path.display(),
                    version = retrace_version.as_deref().unwrap_or("unknown"),
                    "[BETA] retrace resolved"
                );
                let extras = probe_extras(&python_path, scoped_home).await;
                return Ok(RetraceCapability {
                    python_path,
                    retrace_version,
                    extras,
                });
            }
            Err(detail) => attempts.push(format!("{}: {detail}", candidate.join(" "))),
        }
    }
    Err(attempts)
}

/// The `check_retrace` response. Absence is a field, never an error result —
/// the same convention `handle_check_freerouting` follows.
fn build_check_response(
    capability: Option<&RetraceCapability>,
    attempts: &[String],
) -> serde_json::Value {
    match capability {
        None => json!({
            "available": false,
            "python_path": null,
            "retrace_version": null,
            "extras": { "detection": false, "ocr": false },
            "candidates_tried": attempts,
            "note": INSTALL_NOTE,
        }),
        Some(capability) => {
            let extras = capability.extras;
            let note = if extras.detection && extras.ocr {
                FULL_INSTALL_NOTE.to_string()
            } else {
                let missing: Vec<&str> = [("detection", extras.detection), ("ocr", extras.ocr)]
                    .into_iter()
                    .filter(|(_, present)| !present)
                    .map(|(name, _)| name)
                    .collect();
                format!(
                    "{CONTOUR_FALLBACK_NOTE} Missing extras: {}.",
                    missing.join(", ")
                )
            };
            json!({
                "available": true,
                "python_path": subprocess_arg(&capability.python_path),
                "retrace_version": capability.retrace_version,
                "extras": { "detection": extras.detection, "ocr": extras.ocr },
                "note": note,
            })
        }
    }
}

// ─── Scan support ─────────────────────────────────────────────────────────────

/// `timeout_seconds` argument > `photo_intake.retrace_timeout_seconds` >
/// built-in default, clamped so neither a typo nor a stale config can produce
/// a zero-second or effectively infinite scan.
fn resolve_scan_timeout(argument: Option<u64>, configured: Option<u64>) -> Duration {
    let seconds = argument
        .or(configured)
        .unwrap_or(DEFAULT_SCAN_TIMEOUT_SECS)
        .clamp(MIN_SCAN_TIMEOUT_SECS, MAX_SCAN_TIMEOUT_SECS);
    Duration::from_secs(seconds)
}

/// Which detector actually ran, from the subprocess's own stderr, with the
/// extras probe as a backstop.
///
/// retrace can import `ultralytics` and still fall back — a missing model
/// file, a CUDA init failure — so the probe alone would report
/// `used_fallback: false` over a result produced with no detection at all. The
/// `||` makes stderr primary; if a later retrace release rewords the warning,
/// the expression degrades to the probe's answer and errs toward `true`.
fn derive_fallback(stderr: &str, extras: RetraceExtras) -> (bool, serde_json::Value) {
    let yolo_warning_seen = stderr.contains("YOLO not available");
    let ocr_warning_seen = stderr.contains("easyocr is not installed");
    let used_fallback = yolo_warning_seen || !extras.detection;
    (
        used_fallback,
        json!({
            "yolo_warning_seen": yolo_warning_seen,
            "ocr_warning_seen": ocr_warning_seen,
            "extras_detection": extras.detection,
            "extras_ocr": extras.ocr,
        }),
    )
}

/// The scan's output directory: computed, never supplied. An argument that
/// does not exist cannot be got wrong, which is why `output_dir` is absent
/// from the schema rather than validated in it.
///
/// The final `starts_with` re-check is what catches the one case the computed
/// path cannot: a pre-existing `.konnect` or `photo_intake` that is a symlink
/// to somewhere else.
fn prepare_map_dir(project_dir: &Path, map_id: &str) -> Result<PathBuf, String> {
    let map_dir = project_dir
        .join(".konnect")
        .join("photo_intake")
        .join(map_id);
    std::fs::create_dir_all(&map_dir)
        .map_err(|error| format!("Could not create {}: {error}", map_dir.display()))?;
    let canonical = map_dir
        .canonicalize()
        .map_err(|error| format!("Could not resolve {}: {error}", map_dir.display()))?;
    if !canonical.starts_with(project_dir) {
        return Err(format!(
            "Refusing to write outside the project: {} resolves to {}, which is not under {}",
            map_dir.display(),
            canonical.display(),
            project_dir.display()
        ));
    }
    Ok(canonical)
}

fn canonical_existing_dir(raw: &str, field: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("'{field}' does not resolve: {} ({error})", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("'{field}' is not a directory: {}", path.display()));
    }
    Ok(canonical)
}

fn canonical_existing_file(raw: &str, field: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("'{field}' does not resolve: {} ({error})", path.display()))?;
    if !canonical.is_file() {
        return Err(format!("'{field}' is not a file: {}", path.display()));
    }
    Ok(canonical)
}

/// The error a caller sees when no interpreter can import `retrace`. Named as
/// its own function so the message that points at `check_retrace` has one
/// source, and so it is assertable without an interpreter present.
fn retrace_unavailable_error(attempts: &[String]) -> CallToolResult {
    CallToolResult::error(format!(
        "retrace is not available, so no scan was run. Call check_retrace for diagnosis. \
         Candidates tried: {}",
        if attempts.is_empty() {
            "(none)".to_string()
        } else {
            attempts.join(" | ")
        }
    ))
}

// ─── Review map: schema, content hash, and paths ──────────────────────────────

/// The review map's file name inside `<project>/.konnect/photo_intake/<map_id>/`.
const REVIEW_MAP_FILE: &str = "review_map.json";

/// `map_id` is a path component, so it is a token rather than a string: 1..=64
/// characters of `[A-Za-z0-9_-]`. Stated once here, enforced twice — in the
/// JSON Schema of the tools that take it directly, and in Rust for every tool,
/// because `save_photo_review_map` carries it nested inside `map`, where a
/// top-level schema keyword never reaches (design D13).
const MAP_ID_PATTERN: &str = "^[A-Za-z0-9_-]{1,64}$";

/// Net provenance. A closed vocabulary because `traced` is a claim about
/// evidence: a reviewer reading "traced" must be able to trust that the
/// connection came from the scan and not from a guess spelled differently.
const NET_SOURCES: [&str; 3] = ["traced", "inferred", "manual"];

/// A persisted review map, in design D8's canonical shape.
///
/// This struct is the *validator* for an incoming map, not the transport for a
/// persisted one: `load_photo_review_map` returns the file's parsed
/// `serde_json::Value` verbatim so a reviewer's hand edits — including keys
/// this struct has never heard of — survive a round trip. `save` and `approve`
/// parse into it to reject a malformed map before anything is written, then
/// hash and store the JSON object itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoReviewMap {
    /// Server-assigned: `scan_pcb_photo` mints it, nothing else does.
    pub map_id: String,
    /// Rewritten on every save; outside the content hash (D16).
    #[serde(default)]
    pub saved_at: Option<String>,
    pub source_images: Vec<String>,
    /// Always user-supplied — no scan can produce a physical scale.
    pub scale_reference: ScaleReference,
    #[serde(default)]
    pub components: Vec<ReviewComponent>,
    #[serde(default)]
    pub nets: Vec<ReviewNet>,
    /// Advisory and never evidence (D14): optional, may be absent entirely,
    /// and outside the content hash so a hint can neither confer nor revoke a
    /// human's approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcircuit_hints: Option<Vec<RetracePatternMatch>>,
    /// Server-owned. A client-supplied value is ignored and overwritten (D5).
    #[serde(default)]
    pub approved: bool,
    #[serde(default)]
    pub approved_at: Option<String>,
    #[serde(default)]
    pub content_hash_at_approval: Option<String>,
}

/// The physical calibration the reviewer supplies, e.g. `board_edge_mm` / `50`
/// or `package` / `0805`.
///
/// `kind` is deliberately not validated against a closed list: this change
/// persists the scale reference without consuming it (Slice 2 does the
/// consuming), so a new kind is additive and inert, and rejecting one here
/// would be a gate on data no tool reads yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleReference {
    pub kind: String,
    pub value: String,
}

/// One component under review. `confidence` is required and carried verbatim:
/// the spec's flagging rule ("below 0.6 is flagged, never auto-corrected")
/// only works if the number survives the round trip unrounded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComponent {
    /// retrace's own id (`C0000`), so every row is traceable to `analysis.json`.
    pub component_id: String,
    /// Assigned during review; absent before it.
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub value: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub footprint_suggestion: Option<String>,
    pub confidence: f64,
    /// `[x, y, w, h]` in source-image pixels, copied verbatim from retrace.
    pub bbox_px: [i64; 4],
    /// Per-component approval, set by a human during review.
    #[serde(default)]
    pub approved: bool,
}

/// One net, as `ref`-to-`ref`-or-pin connections plus where they came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewNet {
    pub connections: Vec<String>,
    pub source: String,
}

/// The fields the approval gate covers, named explicitly rather than derived
/// by excluding others: an omission is then visible at this list instead of
/// hiding in a denylist (design D16).
const CONTENT_KEYS: [&str; 4] = ["source_images", "scale_reference", "components", "nets"];

/// Everything outside the hash: identity, bookkeeping the tools write
/// themselves (hashing it would make every save revoke its own approval), and
/// the advisory hints. `CONTENT_KEYS` plus this is every key of design D8.
/// Only the test that checks this split against the schema reads it, and it
/// lives here rather than in that test because the split is the decision, not
/// the assertion.
#[allow(dead_code)]
const UNHASHED_KEYS: [&str; 6] = [
    "map_id",
    "saved_at",
    "approved",
    "approved_at",
    "content_hash_at_approval",
    "subcircuit_hints",
];

/// SHA-256, lowercase hex, over a fresh object holding only [`CONTENT_KEYS`],
/// cloned from `map` — the exact digest form `design_hash.rs:47` already
/// produces. The one implementation of the gate's "is this still the content a
/// human said yes to" question; `save_photo_review_map`,
/// `approve_photo_review_map` and the schematic-build re-check (D6 step 3) all
/// call it rather than each hashing their own selection.
///
/// Never hash the file's bytes: the user is *supposed* to hand-edit this JSON,
/// and a whitespace- or key-order-sensitive digest would revoke approval on a
/// reformat, which teaches people to re-approve without looking.
///
/// **Depends on `serde_json`'s `preserve_order` feature being off**, which
/// makes `serde_json::Map` a `BTreeMap` and so emits object keys sorted at
/// every depth. Turning it on later would silently change every stored hash
/// and revoke every approval in the field; `Cargo.toml:30` declares
/// `serde_json = "1"` with no features, and the feature appears nowhere in the
/// workspace or lockfile.
pub(crate) fn review_map_content_hash(map: &serde_json::Value) -> String {
    let mut covered = serde_json::Map::new();
    for key in CONTENT_KEYS {
        covered.insert(
            key.to_string(),
            map.get(key).cloned().unwrap_or(serde_json::Value::Null),
        );
    }
    // Serializing a `Value` that was built by cloning cannot fail.
    let bytes =
        serde_json::to_vec(&serde_json::Value::Object(covered)).expect("a cloned Value serializes");
    format!("{:x}", Sha256::digest(&bytes))
}

/// Whether the persisted map's own `approved` flag still describes its current
/// content. Computed server-side so a consumer — the schematic-build re-check
/// of D6 step 3 above all — re-checks the gate without reimplementing the hash.
pub(crate) fn approval_is_valid(map: &serde_json::Value) -> bool {
    map.get("approved") == Some(&serde_json::Value::Bool(true))
        && map
            .get("content_hash_at_approval")
            .and_then(serde_json::Value::as_str)
            == Some(review_map_content_hash(map).as_str())
}

// ─── Time ─────────────────────────────────────────────────────────────────────

/// RFC 3339 UTC at second resolution (`2026-09-18T03:22:44Z`).
///
/// Hand-rolled because the workspace has no date crate at all — `chrono` and
/// `time` appear in neither `Cargo.toml` nor `Cargo.lock` — and adding one to
/// format two timestamps would be a larger change than the arithmetic it
/// replaces. `civil_from_days` is Hinnant's algorithm, exact for every date
/// after 0000-03-01.
fn rfc3339_utc(unix_seconds: i64) -> String {
    let (year, month, day) = civil_from_days(unix_seconds.div_euclid(86_400));
    let second_of_day = unix_seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3600,
        (second_of_day / 60) % 60,
        second_of_day % 60
    )
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn now_rfc3339_utc() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    rfc3339_utc(seconds)
}

// ─── Review-map paths and validation ──────────────────────────────────────────

/// Reject anything that is not [`MAP_ID_PATTERN`] *before* it reaches a path
/// join. A token rule rejects `/`, `\`, `.`, `..`, `:` and NUL by construction
/// rather than by blacklist, so there is no traversal spelling left to miss.
fn validate_map_id(raw: &str) -> Result<(), String> {
    let length = raw.chars().count();
    let valid = (1..=64).contains(&length)
        && raw.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "'map_id' must match {MAP_ID_PATTERN} (1-64 characters of A-Z a-z 0-9 _ -); got \
             {raw:?}. No file was read or written."
        ))
    }
}

/// The directory of a map that already exists. Unlike [`prepare_map_dir`] it
/// creates nothing: a `map_id` is server-assigned by `scan_pcb_photo` (D13
/// rule 3), so a caller naming a directory that is not there is naming a map
/// that does not exist, not asking for one to be minted.
fn existing_map_dir(project_dir: &Path, map_id: &str) -> Result<PathBuf, String> {
    validate_map_id(map_id)?;
    let map_dir = project_dir
        .join(".konnect")
        .join("photo_intake")
        .join(map_id);
    let canonical = map_dir.canonicalize().map_err(|error| {
        format!(
            "No review map directory for map_id '{map_id}' under {} ({error}). Run \
             scan_pcb_photo first — map ids are server-assigned.",
            project_dir.display()
        )
    })?;
    if !canonical.is_dir() {
        return Err(format!(
            "'{map_id}' does not name a directory: {}",
            canonical.display()
        ));
    }
    if !canonical.starts_with(project_dir) {
        return Err(format!(
            "Refusing to read or write outside the project: {} resolves to {}, which is not under \
             {}",
            map_dir.display(),
            canonical.display(),
            project_dir.display()
        ));
    }
    Ok(canonical)
}

/// Read the persisted map as a `Value`. Deliberately untyped: what is on disk
/// is what a reviewer last edited, and `load` must hand it back as it found it.
async fn read_review_map(path: &Path) -> Result<serde_json::Value, String> {
    let raw = tokio::fs::read_to_string(path).await.map_err(|error| {
        format!(
            "No review map at {} ({error}). Call save_photo_review_map first.",
            path.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "{} is not valid JSON ({error}). Fix the file by hand — it was left untouched.",
            path.display()
        )
    })?;
    if !value.is_object() {
        return Err(format!("{} does not hold a JSON object.", path.display()));
    }
    Ok(value)
}

/// Pretty-printed for the same reason `write_config` (`config.rs:110`) is:
/// this file exists to be opened and edited by a human.
async fn write_review_map(path: &Path, map: &serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(map).map_err(|error| error.to_string())?;
    tokio::fs::write(path, content)
        .await
        .map_err(|error| format!("Could not write {}: {error}", path.display()))
}

/// Parse an incoming map into [`PhotoReviewMap`] to reject a malformed one
/// before anything is written, and check the one vocabulary the gate's meaning
/// rests on.
fn validate_incoming_map(map: &serde_json::Value) -> Result<PhotoReviewMap, String> {
    let parsed: PhotoReviewMap = serde_json::from_value(map.clone()).map_err(|error| {
        format!(
            "the review map does not match the review-map schema: {error}. Nothing was written."
        )
    })?;
    validate_map_id(&parsed.map_id)?;
    for (index, net) in parsed.nets.iter().enumerate() {
        if !NET_SOURCES.contains(&net.source.as_str()) {
            return Err(format!(
                "nets[{index}].source is {:?}; it must be one of {}. Nothing was written.",
                net.source,
                NET_SOURCES.join(", ")
            ));
        }
    }
    Ok(parsed)
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

/// The JSON Schema for `save_photo_review_map`'s `map` argument, spelled out
/// rather than left as a bare `{"type": "object"}`.
///
/// `ToolDef::new` runs `close_input_schema` (`tools/mod.rs:134`), which inserts
/// `additionalProperties: false` into every object subschema that does not
/// declare one. A `map` with no `properties` therefore published a schema that
/// accepted `{}` and nothing else, and the MCP dispatcher validates before it
/// dispatches (`mcp/handler.rs:366`) — so no caller could ever save a review
/// map. Keep this in sync with [`PhotoReviewMap`] (design D8);
/// `the_map_schema_names_every_review_map_field` fails when they drift.
///
/// Every object a reviewer hand-edits is left open
/// (`"additionalProperties": true`, which `entry().or_insert()` preserves):
/// annotations that no tool reads must survive a save, so the schema may not
/// refuse the keys the handler is required to persist.
fn review_map_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "The full review map. Its map_id must name a scan directory that already exists (scan_pcb_photo assigns it). Approval fields are ignored on input and rewritten by the server. Keys beyond these are preserved verbatim, so reviewer annotations survive a save.",
        "additionalProperties": true,
        "properties": {
            "map_id": {
                "type": "string",
                "pattern": MAP_ID_PATTERN,
                "description": "The map identifier assigned by scan_pcb_photo. Its scan directory must already exist."
            },
            "saved_at": {
                "type": ["string", "null"],
                "description": "Server-owned: rewritten on every save. Outside the approval hash."
            },
            "source_images": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Absolute paths of the photographs this map was read from. Required — populate it before the first save."
            },
            "scale_reference": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "kind": { "type": "string", "description": "e.g. board_edge_mm or package." },
                    "value": { "type": "string", "description": "e.g. '50' or '0805'." }
                },
                "required": ["kind", "value"],
                "description": "Always user-supplied; never estimated from pixels."
            },
            "components": {
                "type": "array",
                "description": "One entry per detected component, carried over from the scan and never invented.",
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "properties": {
                        "component_id": { "type": "string", "description": "The scan's own id, copied verbatim." },
                        "ref": { "type": ["string", "null"], "description": "Reference designator, null until a human assigns it." },
                        "type": { "type": ["string", "null"], "description": "Coarse class: resistor, capacitor, ic, connector, ..." },
                        "value": { "type": ["string", "null"], "description": "Null when the scan read nothing. Never guess." },
                        "footprint_suggestion": { "type": ["string", "null"], "description": "A suggestion only; schematic build resolves the real footprint." },
                        "confidence": { "type": "number", "description": "The scan's own number, unrounded. Below 0.6 is flagged for manual identification." },
                        "bbox_px": {
                            "type": "array",
                            "items": { "type": "integer" },
                            "minItems": 4,
                            "maxItems": 4,
                            "description": "[x, y, w, h] in source-image pixels, copied verbatim."
                        },
                        "approved": { "type": "boolean", "description": "Per-component human decision." }
                    },
                    "required": ["component_id", "confidence", "bbox_px"]
                }
            },
            "nets": {
                "type": "array",
                "description": "Reviewed connectivity. retrace's synthetic netlist is never a source for this list.",
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "properties": {
                        "connections": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "ref-and-pin endpoints, e.g. R1.1."
                        },
                        "source": {
                            "type": "string",
                            "enum": NET_SOURCES,
                            "description": "traced, inferred or manual - tagged honestly."
                        }
                    },
                    "required": ["connections", "source"]
                }
            },
            "subcircuit_hints": {
                "type": "array",
                "description": "The scan's pattern_matches, verbatim. Advisory and never evidence; outside the approval hash."
            },
            "approved": { "type": "boolean", "description": "Server-owned: ignored on input and rewritten." },
            "approved_at": { "type": ["string", "null"], "description": "Server-owned: ignored on input and rewritten." },
            "content_hash_at_approval": { "type": ["string", "null"], "description": "Server-owned: ignored on input and rewritten." }
        },
        "required": ["map_id", "source_images", "scale_reference"]
    })
}

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "check_retrace",
            "Report whether the optional `retrace` Python package is usable for PCB photo \
             analysis: which interpreter resolved, the retrace version, and which optional \
             extras (detection, ocr) are importable. Absence is reported as a field, never as \
             an error — call this first to diagnose any scan_pcb_photo failure.",
            json!({
                "type": "object",
                "properties": {
                    "python_path": {
                        "type": "string",
                        "description": "Python interpreter to probe. If omitted, uses photo_intake.retrace_python_path, then RETRACE_PYTHON, then PATH discovery."
                    }
                },
                "required": []
            }),
            |args, ctx| async move { handle_check_retrace(args, ctx).await }
        ),
        tool!(
            "scan_pcb_photo",
            "Run a retrace component scan over a PCB photo and return the parsed analysis: \
             components with pixel bboxes, traces, and canonical-subcircuit pattern matches. \
             The raw analysis.json is kept under <project_dir>/.konnect/photo_intake/<map_id>/ \
             as evidence. Results are a starting point for human review, never a netlist.",
            json!({
                "type": "object",
                "properties": {
                    "image_path": {
                        "type": "string",
                        "description": "Path to the board photo (PNG/JPEG). May live anywhere; it is read, never written."
                    },
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory. Scan output is written under <project_dir>/.konnect/photo_intake/."
                    },
                    "python_path": {
                        "type": "string",
                        "description": "Python interpreter to use. If omitted, resolved exactly as check_retrace resolves it."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 5,
                        "maximum": 1800,
                        "description": "Scan timeout. Defaults to photo_intake.retrace_timeout_seconds (120)."
                    }
                },
                "required": ["image_path", "project_dir"]
            }),
            |args, ctx| async move { handle_scan_pcb_photo(args, ctx).await }
        ),
        tool!(
            "save_photo_review_map",
            "Persist a photo review map as editable JSON under \
             <project_dir>/.konnect/photo_intake/<map_id>/review_map.json. The map's approval \
             state is server-owned: this tool never grants approval, and any edit to the \
             reviewed content revokes an approval the map already had.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory the map belongs to."
                    },
                    "map": review_map_schema()
                },
                "required": ["project_dir", "map"]
            }),
            |args, ctx| async move { handle_save_photo_review_map(args, ctx).await }
        ),
        tool!(
            "load_photo_review_map",
            "Return a persisted photo review map exactly as it is on disk, including edits made \
             to the JSON file by hand, plus a server-computed approval_valid flag stating \
             whether its recorded approval still covers its current content. Any consumer of a \
             review map must check approval_valid, not the map's own flag.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory the map belongs to."
                    },
                    "map_id": {
                        "type": "string",
                        "pattern": "^[A-Za-z0-9_-]{1,64}$",
                        "description": "The map identifier assigned by scan_pcb_photo."
                    }
                },
                "required": ["project_dir", "map_id"]
            }),
            |args, ctx| async move { handle_load_photo_review_map(args, ctx).await }
        ),
        tool!(
            "approve_photo_review_map",
            "Record explicit human approval of a persisted review map: sets its approved flag, \
             stamps approved_at, and records the hash of the exact content approved. This is \
             the only way a map becomes approved, and it must be called again after any edit \
             to the map's components, nets, source images or scale reference.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory the map belongs to."
                    },
                    "map_id": {
                        "type": "string",
                        "pattern": "^[A-Za-z0-9_-]{1,64}$",
                        "description": "The map identifier assigned by scan_pcb_photo."
                    }
                },
                "required": ["project_dir", "map_id"]
            }),
            |args, ctx| async move { handle_approve_photo_review_map(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_check_retrace(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let config = crate::tools::config::effective_config(ctx.config.project_dir.as_deref()).await;
    let configured = config["photo_intake"]["retrace_python_path"].as_str();

    let home = scoped_home_dir()?;
    let resolved = resolve_retrace(args["python_path"].as_str(), configured, home.path()).await;

    Ok(match resolved {
        Ok(capability) => CallToolResult::json(&build_check_response(Some(&capability), &[])),
        Err(attempts) => CallToolResult::json(&build_check_response(None, &attempts)),
    })
}

async fn handle_scan_pcb_photo(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let image_arg = match require_str(args, "image_path") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let image_path = match canonical_existing_file(&image_arg, "image_path") {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    let project_dir = match canonical_existing_dir(&project_arg, "project_dir") {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };

    let config = crate::tools::config::effective_config(Some(&project_dir)).await;
    let configured_python = config["photo_intake"]["retrace_python_path"].as_str();
    let run_timeout = resolve_scan_timeout(
        args["timeout_seconds"].as_u64(),
        config["photo_intake"]["retrace_timeout_seconds"].as_u64(),
    );

    let home = scoped_home_dir()?;
    let capability =
        match resolve_retrace(args["python_path"].as_str(), configured_python, home.path()).await {
            Ok(capability) => capability,
            Err(attempts) => return Ok(retrace_unavailable_error(&attempts)),
        };

    let map_id = uuid::Uuid::new_v4().to_string();
    let map_dir = match prepare_map_dir(&project_dir, &map_id) {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };

    let scan_args: Vec<String> = vec![
        "-m".to_string(),
        "retrace".to_string(),
        "scan".to_string(),
        subprocess_arg(&image_path),
        "--format".to_string(),
        "json".to_string(),
        "-o".to_string(),
        subprocess_arg(&map_dir),
    ];

    info!(
        image = %image_path.display(),
        map_id = %map_id,
        "[BETA] retrace scan"
    );
    let started = std::time::Instant::now();
    let output = match run_retrace(
        &capability.python_path,
        &scan_args,
        home.path(),
        run_timeout,
    )
    .await
    {
        Ok(output) => output,
        // A killed scan may have left a truncated analysis.json behind. It is
        // never parsed: a half-written component list must not become a
        // half-populated result.
        Err(error @ RetraceRunError::Timeout { .. }) => {
            return Ok(CallToolResult::error(format!(
                "retrace scan {error}. Partial output (if any) is under {}; it was not parsed. \
                 Raise timeout_seconds or photo_intake.retrace_timeout_seconds if the scan \
                 legitimately needs longer.",
                map_dir.display()
            )));
        }
        Err(error) => return Ok(CallToolResult::error(format!("retrace scan {error}"))),
    };
    let duration_seconds = (started.elapsed().as_secs_f64() * 1000.0).round() / 1000.0;

    if !output.status.success() {
        return Ok(CallToolResult::error(format!(
            "retrace scan exited with {}:\n{}",
            output.status.code().unwrap_or(-1),
            retrace_diagnostics(&output)
        )));
    }

    // A zero exit is necessary but not sufficient — the same lesson
    // `verify_nonempty_file` (`cli.rs:357`) encodes for kicad-cli.
    let analysis_path = map_dir.join("analysis.json");
    let raw = match tokio::fs::read_to_string(&analysis_path).await {
        Ok(raw) => raw,
        Err(error) => {
            return Ok(CallToolResult::error(format!(
                "retrace scan reported success but {} is unreadable ({error}).\n{}",
                analysis_path.display(),
                retrace_diagnostics(&output)
            )));
        }
    };
    let analysis: RetraceAnalysis = match serde_json::from_str(&raw) {
        Ok(analysis) => analysis,
        Err(error) => {
            return Ok(CallToolResult::error(format!(
                "retrace scan wrote {} but it could not be parsed: {error}",
                analysis_path.display()
            )));
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr);
    let (used_fallback, fallback_evidence) = derive_fallback(&stderr, capability.extras);

    Ok(CallToolResult::json(&json!({
        "map_id": map_id,
        "components": analysis.components,
        "traces": analysis.traces,
        "pattern_matches": analysis.pattern_matches,
        "analysis_json_path": subprocess_arg(&analysis_path),
        "duration_seconds": duration_seconds,
        "used_fallback": used_fallback,
        "fallback_evidence": fallback_evidence,
    })))
}

/// `project_dir` + `map_id` → the review map's file, with every design D13
/// rule applied. One implementation, shared by all three map tools, so the
/// confinement check cannot drift between them.
fn resolve_map_file(project_arg: &str, map_id: &str) -> Result<PathBuf, String> {
    let project_dir = canonical_existing_dir(project_arg, "project_dir")?;
    Ok(existing_map_dir(&project_dir, map_id)?.join(REVIEW_MAP_FILE))
}

async fn handle_save_photo_review_map(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let Some(map_arg) = args.get("map").filter(|value| value.is_object()) else {
        return Ok(CallToolResult::error(
            "'map' is missing or is not a JSON object.",
        ));
    };
    let parsed = match validate_incoming_map(map_arg) {
        Ok(parsed) => parsed,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    let map_file = match resolve_map_file(&project_arg, &parsed.map_id) {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };

    // Normalizing through the struct means the file is always in design D8's
    // shape whatever the caller sent, and — the point of D16's float
    // discipline — the object hashed below is the object written, never one
    // re-derived from it.
    let mut value = serde_json::to_value(&parsed)?;
    let content_hash = review_map_content_hash(&value);

    // Approval survives a save only when the content is exactly what was
    // approved. The previous state is read from disk; the payload's own
    // approval fields were never consulted and are about to be overwritten.
    let previous = read_review_map(&map_file).await.ok();
    let keeps_approval = previous.as_ref().is_some_and(|previous| {
        previous.get("approved") == Some(&serde_json::Value::Bool(true))
            && previous
                .get("content_hash_at_approval")
                .and_then(serde_json::Value::as_str)
                == Some(content_hash.as_str())
    });

    value["saved_at"] = json!(now_rfc3339_utc());
    value["approved"] = json!(keeps_approval);
    // Cleared rather than kept on a revoking save: the only way back to
    // `approved: true` is a human calling approve_photo_review_map again, so
    // an edit-then-undo cannot silently restore a gate nobody re-opened.
    value["approved_at"] = if keeps_approval {
        previous
            .as_ref()
            .and_then(|previous| previous.get("approved_at").cloned())
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    value["content_hash_at_approval"] = if keeps_approval {
        json!(content_hash)
    } else {
        serde_json::Value::Null
    };

    if let Err(message) = write_review_map(&map_file, &value).await {
        return Ok(CallToolResult::error(message));
    }
    info!(
        map_id = %parsed.map_id,
        components = parsed.components.len(),
        nets = parsed.nets.len(),
        approved = keeps_approval,
        "[BETA] review map saved"
    );

    Ok(CallToolResult::json(&json!({
        "map_id": parsed.map_id,
        "saved_path": subprocess_arg(&map_file),
        "approved": keeps_approval,
    })))
}

async fn handle_load_photo_review_map(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let map_id = match require_str(args, "map_id") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let map_file = match resolve_map_file(&project_arg, &map_id) {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    let map = match read_review_map(&map_file).await {
        Ok(map) => map,
        Err(message) => return Ok(CallToolResult::error(message)),
    };

    // The map is returned as found, hand edits and all — but whether its own
    // `approved` flag still means anything is answered here rather than left
    // to the caller, because a caller that recomputes the hash itself is a
    // caller that can get it wrong.
    Ok(CallToolResult::json(&json!({
        "map": map,
        "approval_valid": approval_is_valid(&map),
        "saved_path": subprocess_arg(&map_file),
    })))
}

async fn handle_approve_photo_review_map(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let project_arg = match require_str(args, "project_dir") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let map_id = match require_str(args, "map_id") {
        Ok(value) => value.to_string(),
        Err(rejection) => return Ok(rejection),
    };
    let map_file = match resolve_map_file(&project_arg, &map_id) {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    let mut map = match read_review_map(&map_file).await {
        Ok(map) => map,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    // A map broken by a hand edit is not approvable: approval is a statement
    // about content a human read, and an unparseable component list is not
    // content anyone read.
    if let Err(message) = validate_incoming_map(&map) {
        return Ok(CallToolResult::error(format!(
            "{} cannot be approved: {message}",
            map_file.display()
        )));
    }

    // Hashed from the value just read, with no round trip in between (D16).
    let content_hash = review_map_content_hash(&map);
    let approved_at = now_rfc3339_utc();
    map["approved"] = json!(true);
    map["approved_at"] = json!(approved_at);
    map["content_hash_at_approval"] = json!(content_hash);

    if let Err(message) = write_review_map(&map_file, &map).await {
        return Ok(CallToolResult::error(message));
    }
    info!(map_id = %map_id, "[BETA] review map approved");

    Ok(CallToolResult::json(&json!({
        "map_id": map_id,
        "approved": true,
        "approved_at": approved_at,
        "content_hash_at_approval": content_hash,
    })))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod analysis_parsing_tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/photo_intake/analysis.json");

    #[test]
    fn captured_analysis_round_trips_through_the_typed_structs() {
        let analysis: RetraceAnalysis = serde_json::from_str(FIXTURE).expect("fixture parses");

        assert_eq!(analysis.version.as_deref(), Some("0.3.0"));
        assert_eq!(analysis.components.len(), 4);
        assert!(analysis.traces.is_empty());
        assert_eq!(analysis.pattern_matches.len(), 2);

        let first = &analysis.components[0];
        assert_eq!(first.id, "C0000");
        assert_eq!(first.label.as_deref(), Some("resistor"));
        assert_eq!(first.confidence, Some(0.5));
        assert_eq!(first.bbox, [295, 195, 51, 31]);

        let hint = &analysis.pattern_matches[0];
        assert_eq!(hint.pattern_name, "pull_up_resistor");
        assert_eq!(hint.score, Some(0.7));
        assert_eq!(hint.is_partial, Some(false));
        assert_eq!(
            hint.component_roles.get("resistor").map(String::as_str),
            Some("C0000")
        );
        assert_eq!(analysis.summary["components"], 4);

        // Round trip: serialize what was parsed and parse it again.
        let reserialized = serde_json::to_string(&analysis).expect("serializes");
        let again: RetraceAnalysis = serde_json::from_str(&reserialized).expect("reparses");
        assert_eq!(again.components.len(), analysis.components.len());
        assert_eq!(again.components[0].bbox, first.bbox);
        assert_eq!(again.pattern_matches[0].pattern_name, hint.pattern_name);
    }

    /// retrace writes `""`, not `null`, for a field it did not read. Without
    /// the normalizer these deserialize to `Some("")` and a reviewer sees a
    /// filled field that was never read.
    #[test]
    fn absent_strings_deserialize_to_none_not_some_empty() {
        let analysis: RetraceAnalysis = serde_json::from_str(FIXTURE).expect("fixture parses");
        for component in &analysis.components {
            assert_eq!(component.marking, None, "{} marking", component.id);
            assert_eq!(component.part_number, None, "{} part_number", component.id);
            assert_eq!(component.value, None, "{} value", component.id);
            assert_eq!(component.package, None, "{} package", component.id);
            assert_eq!(
                component.datasheet_url, None,
                "{} datasheet_url",
                component.id
            );
        }

        // A real value is never rewritten.
        let component: RetraceComponent = serde_json::from_value(json!({
            "id": "C0009",
            "bbox": [0, 0, 1, 1],
            "marking": "  ",
            "value": "10k"
        }))
        .expect("parses");
        assert_eq!(component.marking, None, "whitespace-only is absent");
        assert_eq!(component.value.as_deref(), Some("10k"));
        assert_eq!(component.label, None);
    }

    /// Schema drift must degrade the result, not fail the parse.
    #[test]
    fn unknown_and_missing_optional_fields_do_not_break_the_parse() {
        let analysis: RetraceAnalysis = serde_json::from_str(
            r#"{"components": [{"id": "C0", "bbox": [1,2,3,4]}], "future_key": 7}"#,
        )
        .expect("parses");
        assert_eq!(analysis.components.len(), 1);
        assert_eq!(analysis.components[0].confidence, None);
        assert!(analysis.pattern_matches.is_empty());
    }
}

#[cfg(test)]
mod runner_tests {
    use super::*;

    /// A shell command that writes `marker.txt` into whatever the process
    /// believes its home directory is. Portable stand-in for retrace, which
    /// writes its global stores under `Path.home()` on every run.
    fn home_writing_command(marker: &str) -> (PathBuf, Vec<String>) {
        if cfg!(windows) {
            (
                PathBuf::from("cmd.exe"),
                vec![
                    "/C".to_string(),
                    format!("echo probe > %USERPROFILE%\\{marker}"),
                ],
            )
        } else {
            (
                PathBuf::from("/bin/sh"),
                vec!["-c".to_string(), format!("echo probe > \"$HOME/{marker}\"")],
            )
        }
    }

    /// Spawned directly, never through a shell: `kill_on_drop` kills the child
    /// it spawned, not that child's children, and a surviving grandchild keeps
    /// the inherited stdout pipe open — which makes the test binary wait out
    /// the full sleep at shutdown even though the timeout fired on time.
    fn sleeping_command() -> (PathBuf, Vec<String>) {
        if cfg!(windows) {
            (
                PathBuf::from("ping"),
                vec!["-n".to_string(), "20".to_string(), "127.0.0.1".to_string()],
            )
        } else {
            (PathBuf::from("sleep"), vec!["20".to_string()])
        }
    }

    /// Spec scenario "scan does not pollute the real user's home directory":
    /// the subprocess's own idea of home is the scoped directory, so the write
    /// it would have made under `~/.local/share/retrace` lands there instead.
    #[tokio::test]
    async fn run_retrace_redirects_home_and_userprofile_away_from_the_real_home() {
        let home = scoped_home_dir().expect("scoped home");
        let real_homes: Vec<PathBuf> = ["HOME", "USERPROFILE"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        assert!(
            !real_homes.is_empty(),
            "neither HOME nor USERPROFILE is set, so this test would prove nothing"
        );

        // A unique marker: a leftover from a failed run must never make the
        // next run fail for the wrong reason.
        let marker = format!("konnect-home-probe-{}.txt", uuid::Uuid::new_v4());
        let (program, args) = home_writing_command(&marker);
        let output = run_retrace(&program, &args, home.path(), Duration::from_secs(30))
            .await
            .expect("shell runs");
        assert!(output.status.success(), "{}", retrace_diagnostics(&output));

        // Clean up before asserting, so a regression does not also litter the
        // developer's home directory.
        let polluted: Vec<PathBuf> = real_homes
            .iter()
            .map(|real| real.join(&marker))
            .filter(|path| path.exists())
            .collect();
        for path in &polluted {
            let _ = std::fs::remove_file(path);
        }

        assert!(
            polluted.is_empty(),
            "the subprocess wrote into the real home: {polluted:?}"
        );
        assert!(
            home.path().join(&marker).exists(),
            "the subprocess wrote somewhere other than the scoped home"
        );
    }

    #[tokio::test]
    async fn run_retrace_reports_a_timeout_rather_than_waiting() {
        let home = scoped_home_dir().expect("scoped home");
        let (program, args) = sleeping_command();
        let error = run_retrace(&program, &args, home.path(), Duration::from_secs(1))
            .await
            .expect_err("must time out");
        match error {
            RetraceRunError::Timeout { seconds } => assert_eq!(seconds, 1),
            other => panic!("expected a timeout, got {other}"),
        }
    }

    #[tokio::test]
    async fn run_retrace_reports_a_missing_program_instead_of_panicking() {
        let home = scoped_home_dir().expect("scoped home");
        let error = run_retrace(
            Path::new("konnect-no-such-interpreter"),
            &["-c".to_string(), "pass".to_string()],
            home.path(),
            PROBE_TIMEOUT,
        )
        .await
        .expect_err("must fail to spawn");
        assert!(
            matches!(error, RetraceRunError::Spawn { .. }),
            "expected a spawn failure, got {error}"
        );
    }

    #[test]
    fn subprocess_arg_drops_the_windows_verbatim_prefix() {
        assert_eq!(
            subprocess_arg(Path::new(r"\\?\C:\boards\top.png")),
            r"C:\boards\top.png"
        );
        assert_eq!(subprocess_arg(Path::new("/tmp/top.png")), "/tmp/top.png");
    }
}

#[cfg(test)]
mod capability_probe_tests {
    use super::*;

    fn capability(detection: bool, ocr: bool) -> RetraceCapability {
        RetraceCapability {
            python_path: PathBuf::from("C:/py/python.exe"),
            retrace_version: Some("0.3.0".to_string()),
            extras: RetraceExtras { detection, ocr },
        }
    }

    /// Spec scenario "retrace and extras are fully installed".
    #[test]
    fn check_retrace_reports_a_full_install_with_both_extras() {
        let response = build_check_response(Some(&capability(true, true)), &[]);
        assert_eq!(response["available"], true);
        assert_eq!(response["retrace_version"], "0.3.0");
        assert_eq!(response["extras"]["detection"], true);
        assert_eq!(response["extras"]["ocr"], true);
        assert_eq!(response["python_path"], "C:/py/python.exe");
        assert!(!CallToolResult::json(&response).is_error);
    }

    /// Spec scenario "retrace is missing entirely": absence is a field with an
    /// install note, and the result is NOT an error result.
    #[test]
    fn check_retrace_reports_absence_as_a_fact_not_an_error() {
        let attempts = vec!["python: no module named retrace".to_string()];
        let response = build_check_response(None, &attempts);
        assert_eq!(response["available"], false);
        assert_eq!(response["extras"]["detection"], false);
        assert_eq!(response["extras"]["ocr"], false);
        let note = response["note"].as_str().expect("note");
        assert!(note.contains("pip install"), "{note}");
        assert_eq!(response["candidates_tried"][0], attempts[0]);
        assert!(
            !CallToolResult::json(&response).is_error,
            "absence must never be an error result"
        );
    }

    /// Spec scenario "base retrace present, ML extras absent".
    #[test]
    fn check_retrace_reports_a_base_install_without_ml_extras() {
        let response = build_check_response(Some(&capability(false, false)), &[]);
        assert_eq!(response["available"], true);
        assert_eq!(response["extras"]["detection"], false);
        assert_eq!(response["extras"]["ocr"], false);
        let note = response["note"].as_str().expect("note");
        assert!(note.contains("contour"), "{note}");
        assert!(note.contains("detection, ocr"), "{note}");
    }

    #[test]
    fn candidate_order_is_argument_then_config_then_discovery() {
        let candidates = candidate_interpreters(Some("  C:/arg/python.exe "), Some("/cfg/python3"));
        assert_eq!(candidates[0], vec!["C:/arg/python.exe".to_string()]);
        assert_eq!(candidates[1], vec!["/cfg/python3".to_string()]);
        assert_eq!(
            candidates.last().expect("non-empty"),
            &vec!["python".to_string()],
            "bare `python` is the last resort"
        );
        assert_eq!(
            cfg!(windows),
            candidates.contains(&vec!["py".to_string(), "-3".to_string()]),
            "the PEP 397 launcher is a Windows-only candidate"
        );
        // An empty or blank argument is not a candidate.
        let discovery_only = candidate_interpreters(Some("   "), None);
        assert!(!discovery_only.iter().any(|c| c[0].trim().is_empty()));
    }

    #[test]
    fn version_probe_reports_the_interpreter_that_imported_retrace() {
        let (python, version) =
            parse_version_probe("0.3.0\r\nC:/venv/Scripts/python.exe\r\n", Path::new("py"));
        assert_eq!(version.as_deref(), Some("0.3.0"));
        assert_eq!(python, PathBuf::from("C:/venv/Scripts/python.exe"));

        // A probe that printed only a version still names an interpreter.
        let (fallback, _) = parse_version_probe("0.3.0\n", Path::new("python3"));
        assert_eq!(fallback, PathBuf::from("python3"));
    }

    #[test]
    fn extras_probe_output_parses_both_flags() {
        assert_eq!(
            parse_extras("detection=1\nocr=0\n"),
            RetraceExtras {
                detection: true,
                ocr: false
            }
        );
        assert_eq!(parse_extras(""), RetraceExtras::default());
    }

    #[tokio::test]
    async fn resolving_a_nonexistent_interpreter_yields_attempts_not_a_panic() {
        let home = scoped_home_dir().expect("scoped home");
        let attempts = resolve_retrace(Some("konnect-no-such-interpreter"), None, home.path())
            .await
            .err();
        // PATH discovery may legitimately find a Python with retrace on a
        // developer machine, so the assertion is about the explicit candidate.
        if let Some(attempts) = attempts {
            assert!(
                attempts
                    .iter()
                    .any(|attempt| attempt.starts_with("konnect-no-such-interpreter")),
                "{attempts:?}"
            );
        }
    }
}

#[cfg(test)]
mod test_support {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;

    pub(super) fn test_ctx() -> ToolContext {
        ToolContext::new(ServerConfig::default(), Arc::new(ToolRouter::new()))
    }

    pub(super) fn response_json(result: &CallToolResult) -> serde_json::Value {
        match &result.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => {
                serde_json::from_str(text).expect("payload is json")
            }
            other => panic!("expected text content, got {other:?}"),
        }
    }

    pub(super) fn response_text(result: &CallToolResult) -> String {
        match &result.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => text.clone(),
            other => panic!("expected text content, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod scan_contract_tests {
    use super::test_support::{response_text, test_ctx};
    use super::*;

    fn scan_schema() -> serde_json::Value {
        tools()
            .into_iter()
            .find(|tool| tool.name == "scan_pcb_photo")
            .expect("scan_pcb_photo is defined")
            .input_schema
    }

    #[test]
    fn photo_intake_exposes_its_tools_by_name() {
        let names: Vec<&str> = tools().iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                "check_retrace",
                "scan_pcb_photo",
                "save_photo_review_map",
                "load_photo_review_map",
                "approve_photo_review_map"
            ]
        );
        assert!(
            tools()
                .iter()
                .all(|tool| tool.board_access == crate::tools::BoardAccess::None),
            "no photo_intake tool touches a live board"
        );
    }

    /// Design D2's table, asserted: the free-form `output_dir` the planner
    /// proposed is absent rather than validated.
    #[test]
    fn scan_schema_requires_image_and_project_and_offers_no_output_dir() {
        let schema = scan_schema();
        let properties = schema["properties"].as_object().expect("properties");
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "image_path",
                "project_dir",
                "python_path",
                "timeout_seconds"
            ]
        );
        assert_eq!(schema["required"], json!(["image_path", "project_dir"]));
        assert!(
            !properties.contains_key("output_dir"),
            "the output directory is computed, never supplied"
        );
    }

    #[test]
    fn check_schema_offers_only_python_path() {
        let schema = tools()
            .into_iter()
            .find(|tool| tool.name == "check_retrace")
            .expect("check_retrace is defined")
            .input_schema;
        let properties = schema["properties"].as_object().expect("properties");
        assert_eq!(
            properties.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["python_path"]
        );
        assert_eq!(schema["required"], json!([]));
    }

    #[test]
    fn scan_timeout_prefers_the_argument_then_config_and_clamps_both() {
        assert_eq!(
            resolve_scan_timeout(None, None),
            Duration::from_secs(DEFAULT_SCAN_TIMEOUT_SECS)
        );
        assert_eq!(
            resolve_scan_timeout(None, Some(300)),
            Duration::from_secs(300)
        );
        assert_eq!(
            resolve_scan_timeout(Some(45), Some(300)),
            Duration::from_secs(45)
        );
        assert_eq!(resolve_scan_timeout(Some(0), None), Duration::from_secs(5));
        assert_eq!(
            resolve_scan_timeout(Some(u64::MAX), None),
            Duration::from_secs(MAX_SCAN_TIMEOUT_SECS)
        );
    }

    /// Design D11: stderr is the primary signal, the extras probe is the
    /// backstop, and both raw signals stay visible in the response.
    #[test]
    fn used_fallback_prefers_stderr_and_falls_back_to_the_extras_probe() {
        let ml = RetraceExtras {
            detection: true,
            ocr: true,
        };
        let none = RetraceExtras::default();

        let (used, evidence) = derive_fallback("", ml);
        assert!(!used, "extras present and no warning means the ML path ran");
        assert_eq!(evidence["yolo_warning_seen"], false);
        assert_eq!(evidence["extras_detection"], true);

        // ultralytics imports but YOLO still did not run — the case a
        // probe-only derivation would report as `false`.
        let (used, evidence) = derive_fallback(
            "WARNING YOLO not available \u{2014} using contour-based fallback\n",
            ml,
        );
        assert!(used, "stderr overrides the probe");
        assert_eq!(evidence["yolo_warning_seen"], true);
        assert_eq!(evidence["extras_detection"], true);

        // The warning was reworded or swallowed; the probe backstops it.
        let (used, evidence) = derive_fallback("", none);
        assert!(used);
        assert_eq!(evidence["yolo_warning_seen"], false);
        assert_eq!(evidence["extras_detection"], false);

        let (_, evidence) = derive_fallback("WARNING easyocr is not installed\n", none);
        assert_eq!(evidence["ocr_warning_seen"], true);
    }

    #[test]
    fn map_dir_is_computed_under_the_project_and_re_checked() {
        let project = tempfile::tempdir().expect("tempdir");
        let project_dir = project.path().canonicalize().expect("canonical project");
        let map_dir = prepare_map_dir(&project_dir, "abc-123").expect("map dir");
        assert!(map_dir.starts_with(&project_dir));
        assert!(map_dir.ends_with("abc-123"));
        assert!(map_dir.is_dir());
        assert!(project_dir.join(".konnect").join("photo_intake").is_dir());
    }

    #[test]
    fn unavailable_scan_points_at_check_retrace() {
        let result = retrace_unavailable_error(&["python: boom".to_string()]);
        assert!(result.is_error);
        let text = response_text(&result);
        assert!(text.contains("check_retrace"), "{text}");
    }

    #[tokio::test]
    async fn scan_rejects_a_missing_image_before_spawning_anything() {
        let project = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        let result = handle_scan_pcb_photo(
            &json!({
                "image_path": project.path().join("nope.png").to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
            }),
            &ctx,
        )
        .await
        .expect("handler returns");
        assert!(result.is_error);
        let text = response_text(&result);
        assert!(text.contains("image_path"), "{text}");
        assert!(
            !project.path().join(".konnect").exists(),
            "a rejected scan must not create the project scratch directory"
        );
    }
}

/// The synthetic board the live test scans. Drawn rather than checked in: no
/// redistributable real-photo fixture exists, and a drawn board keeps the test
/// honest about what it proves (plumbing, not recognition accuracy).
#[cfg(test)]
mod review_map_tests {
    use super::test_support::{response_json, response_text, test_ctx};
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/photo_intake/analysis.json");

    /// A project with one scan directory already minted, exactly as
    /// `scan_pcb_photo` leaves it. Tests go through `prepare_map_dir` rather
    /// than creating the directory themselves, because a `map_id` whose
    /// directory the server never minted is a case production cannot reach.
    fn scanned_project(map_id: &str) -> (tempfile::TempDir, PathBuf) {
        let project = tempfile::tempdir().expect("temp project");
        let canonical = project.path().canonicalize().expect("canonical project");
        prepare_map_dir(&canonical, map_id).expect("scan directory");
        (project, canonical)
    }

    fn map_file(project_canonical: &Path, map_id: &str) -> PathBuf {
        project_canonical
            .join(".konnect")
            .join("photo_intake")
            .join(map_id)
            .join(REVIEW_MAP_FILE)
    }

    /// A minimal but complete map in design D8's shape.
    fn sample_map(map_id: &str) -> serde_json::Value {
        json!({
            "map_id": map_id,
            "saved_at": null,
            "source_images": ["C:/boards/top.png"],
            "scale_reference": { "kind": "board_edge_mm", "value": "50" },
            "components": [
                {
                    "component_id": "C0000",
                    "ref": "R1",
                    "type": "resistor",
                    "value": "10k",
                    "footprint_suggestion": "Resistor_SMD:R_0805_2012Metric",
                    "confidence": 0.82,
                    "bbox_px": [295, 195, 51, 31],
                    "approved": true
                }
            ],
            "nets": [
                { "connections": ["R1.1", "C2.2"], "source": "traced" }
            ],
            "approved": false,
            "approved_at": null,
            "content_hash_at_approval": null
        })
    }

    /// The fixture's real scan, turned into a review map through the structs a
    /// reviewer's client would use. Deterministic, so its digest can be pinned.
    fn fixture_review_map() -> serde_json::Value {
        let analysis: RetraceAnalysis = serde_json::from_str(FIXTURE).expect("fixture parses");
        let components: Vec<ReviewComponent> = analysis
            .components
            .iter()
            .enumerate()
            .map(|(index, component)| ReviewComponent {
                component_id: component.id.clone(),
                reference: Some(format!("R{}", index + 1)),
                kind: component.label.clone(),
                value: component.value.clone(),
                footprint_suggestion: None,
                confidence: component.confidence.unwrap_or_default(),
                bbox_px: component.bbox,
                approved: false,
            })
            .collect();
        let map = PhotoReviewMap {
            map_id: "fixture-map".to_string(),
            saved_at: Some("2026-09-18T03:22:44Z".to_string()),
            source_images: vec!["C:/boards/top.png".to_string()],
            scale_reference: ScaleReference {
                kind: "board_edge_mm".to_string(),
                value: "50".to_string(),
            },
            components,
            nets: vec![ReviewNet {
                connections: vec!["R1.1".to_string(), "R2.2".to_string()],
                source: "traced".to_string(),
            }],
            subcircuit_hints: Some(analysis.pattern_matches.clone()),
            approved: false,
            approved_at: None,
            content_hash_at_approval: None,
        };
        serde_json::to_value(&map).expect("review map serializes")
    }

    async fn save(project: &Path, map: &serde_json::Value) -> CallToolResult {
        handle_save_photo_review_map(
            &json!({ "project_dir": project.to_string_lossy(), "map": map }),
            &test_ctx(),
        )
        .await
        .expect("save returns a result")
    }

    async fn load(project: &Path, map_id: &str) -> CallToolResult {
        handle_load_photo_review_map(
            &json!({ "project_dir": project.to_string_lossy(), "map_id": map_id }),
            &test_ctx(),
        )
        .await
        .expect("load returns a result")
    }

    async fn approve(project: &Path, map_id: &str) -> CallToolResult {
        handle_approve_photo_review_map(
            &json!({ "project_dir": project.to_string_lossy(), "map_id": map_id }),
            &test_ctx(),
        )
        .await
        .expect("approve returns a result")
    }

    // ─── Content hash (task 3.6, design D16) ─────────────────────────────────

    /// Pinned, because the digest is persisted in every approved map in the
    /// field: if a refactor changes what or how this hashes, every stored
    /// approval silently becomes invalid, and only a pinned literal turns that
    /// into a failing test rather than a support ticket.
    ///
    /// The literal was derived independently of this implementation — SHA-256
    /// of Python's `json.dumps(covered, sort_keys=True, separators=(',', ':'))`
    /// over the same four fields — so it pins the canonical form design D16
    /// specifies (sorted keys at every depth, no insignificant whitespace),
    /// not merely whatever `review_map_content_hash` happens to emit.
    #[test]
    fn the_content_hash_of_the_fixture_map_is_pinned() {
        assert_eq!(
            review_map_content_hash(&fixture_review_map()),
            "18afc7b999d88ad5ec0320760e920b7099c257d809b5961b0e5a83d1afb0908a"
        );
    }

    /// Bookkeeping the tools write themselves, and hints no tool reads, sit
    /// outside the hash — otherwise every save would revoke its own approval,
    /// and a re-scan's new hint could revoke a human's.
    #[test]
    fn saved_at_and_subcircuit_hints_are_outside_the_content_hash() {
        let map = fixture_review_map();
        let baseline = review_map_content_hash(&map);

        for (key, value) in [
            ("saved_at", json!("1999-01-01T00:00:00Z")),
            ("subcircuit_hints", json!([])),
            ("map_id", json!("a-different-id")),
            ("approved", json!(true)),
            ("approved_at", json!("1999-01-01T00:00:00Z")),
            ("content_hash_at_approval", json!("deadbeef")),
        ] {
            let mut edited = map.clone();
            edited[key] = value;
            assert_eq!(
                review_map_content_hash(&edited),
                baseline,
                "{key} must not affect the content hash"
            );
        }
    }

    /// One named edit to a review map, for the coverage table below.
    type MapEdit = (&'static str, Box<dyn Fn(&mut serde_json::Value)>);

    /// The other half: everything the reviewer actually approved is covered.
    #[test]
    fn editing_any_reviewed_field_changes_the_content_hash() {
        let map = fixture_review_map();
        let baseline = review_map_content_hash(&map);

        let edits: Vec<MapEdit> = vec![
            (
                "components[0].ref",
                Box::new(|map: &mut serde_json::Value| map["components"][0]["ref"] = json!("R9")),
            ),
            (
                "components[0].value",
                Box::new(|map: &mut serde_json::Value| {
                    map["components"][0]["value"] = json!("22k")
                }),
            ),
            (
                "components[0].confidence",
                Box::new(|map: &mut serde_json::Value| {
                    map["components"][0]["confidence"] = json!(0.51)
                }),
            ),
            (
                "components[0].approved",
                Box::new(|map: &mut serde_json::Value| {
                    map["components"][0]["approved"] = json!(true)
                }),
            ),
            (
                "components[0].bbox_px",
                Box::new(|map: &mut serde_json::Value| {
                    map["components"][0]["bbox_px"] = json!([1, 2, 3, 4])
                }),
            ),
            (
                "component order",
                Box::new(|map: &mut serde_json::Value| {
                    let components = map["components"].as_array_mut().expect("components");
                    components.swap(0, 1);
                }),
            ),
            (
                "nets[0].connections",
                Box::new(|map: &mut serde_json::Value| {
                    map["nets"][0]["connections"] = json!(["R1.1", "R3.2"])
                }),
            ),
            (
                "nets[0].source",
                Box::new(|map: &mut serde_json::Value| map["nets"][0]["source"] = json!("manual")),
            ),
            (
                "source_images",
                Box::new(|map: &mut serde_json::Value| {
                    map["source_images"] = json!(["C:/boards/bottom.png"])
                }),
            ),
            (
                "scale_reference.value",
                Box::new(|map: &mut serde_json::Value| {
                    map["scale_reference"]["value"] = json!("60")
                }),
            ),
        ];

        for (what, edit) in edits {
            let mut edited = map.clone();
            edit(&mut edited);
            assert_ne!(
                review_map_content_hash(&edited),
                baseline,
                "editing {what} must change the content hash"
            );
        }
    }

    /// D16's trade-off, made into a test: a field added to design D8 without a
    /// decision about the hash is an unguarded field. Splitting every schema
    /// key between the covered list and the excluded list forces that decision.
    #[test]
    fn every_schema_key_is_either_hashed_or_deliberately_not() {
        let map = fixture_review_map();
        let schema_keys: std::collections::BTreeSet<&str> = map
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        let accounted: std::collections::BTreeSet<&str> = CONTENT_KEYS
            .iter()
            .chain(UNHASHED_KEYS.iter())
            .copied()
            .collect();
        assert_eq!(
            schema_keys, accounted,
            "every review-map key must be listed in CONTENT_KEYS or UNHASHED_KEYS"
        );
    }

    // ─── Structs (task 3.1) ──────────────────────────────────────────────────

    #[test]
    fn a_review_map_round_trips_and_tolerates_absent_subcircuit_hints() {
        let raw = sample_map("round-trip");
        let parsed: PhotoReviewMap = serde_json::from_value(raw.clone()).expect("parses");
        assert!(parsed.subcircuit_hints.is_none());
        assert_eq!(parsed.components[0].reference.as_deref(), Some("R1"));
        assert_eq!(parsed.components[0].kind.as_deref(), Some("resistor"));
        assert_eq!(parsed.components[0].bbox_px, [295, 195, 51, 31]);

        // Absent hints stay absent rather than becoming `null`, so a map that
        // never had them hashes and reads identically before and after.
        let reserialized = serde_json::to_value(&parsed).expect("serializes");
        assert!(reserialized.get("subcircuit_hints").is_none());
        assert_eq!(
            review_map_content_hash(&reserialized),
            review_map_content_hash(&raw)
        );
    }

    // ─── Time ────────────────────────────────────────────────────────────────

    #[test]
    fn rfc3339_utc_matches_known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        // A leap day, which is where hand-rolled date arithmetic goes wrong.
        assert_eq!(rfc3339_utc(1_709_208_000), "2024-02-29T12:00:00Z");
        assert_eq!(rfc3339_utc(253_402_300_799), "9999-12-31T23:59:59Z");
        assert!(now_rfc3339_utc().ends_with('Z'));
    }

    // ─── save_photo_review_map (tasks 3.2, 3.5) ──────────────────────────────

    /// Spec scenario "a freshly scanned map persists as editable JSON".
    #[tokio::test]
    async fn a_freshly_scanned_map_persists_as_editable_json() {
        let (project, canonical) = scanned_project("scan-1");
        let saved = save(project.path(), &sample_map("scan-1")).await;
        let body = response_json(&saved);
        assert_eq!(body["map_id"], "scan-1");
        assert_eq!(body["approved"], json!(false));

        let path = map_file(&canonical, "scan-1");
        assert_eq!(body["saved_path"], subprocess_arg(&path));
        let on_disk = std::fs::read_to_string(&path).expect("file written");
        assert!(
            on_disk.contains("\n  \"components\""),
            "the file a human is asked to edit is pretty-printed:\n{on_disk}"
        );

        let loaded = response_json(&load(project.path(), "scan-1").await);
        let stored: serde_json::Value = serde_json::from_str(&on_disk).expect("parses");
        assert_eq!(loaded["map"], stored);
        assert_eq!(
            loaded["map"]["components"],
            sample_map("scan-1")["components"]
        );
        assert_eq!(loaded["map"]["nets"], sample_map("scan-1")["nets"]);
    }

    /// Spec scenario "low-confidence components are flagged, never
    /// auto-corrected": the number survives unrounded and nothing is invented
    /// to fill the fields the scan left empty.
    #[tokio::test]
    async fn low_confidence_components_are_flagged_never_auto_corrected() {
        let (project, _canonical) = scanned_project("low-conf");
        let mut map = sample_map("low-conf");
        map["components"][0]["confidence"] = json!(0.43);
        map["components"][0]["value"] = serde_json::Value::Null;
        map["components"][0]["footprint_suggestion"] = serde_json::Value::Null;
        save(project.path(), &map).await;

        let loaded = response_json(&load(project.path(), "low-conf").await);
        let component = &loaded["map"]["components"][0];
        assert_eq!(component["confidence"], json!(0.43));
        assert_eq!(component["value"], serde_json::Value::Null);
        assert_eq!(component["footprint_suggestion"], serde_json::Value::Null);
        assert!(
            component.get("part_number").is_none(),
            "no part number is synthesized: {component}"
        );
    }

    /// Task 3.5 / spec scenario "unapproved map cannot reach schematic build":
    /// saving is not approving, however the payload is spelled.
    #[tokio::test]
    async fn save_alone_never_approves() {
        let (project, canonical) = scanned_project("never-approves");
        let mut map = sample_map("never-approves");
        map["approved"] = json!(true);
        map["approved_at"] = json!("2026-01-01T00:00:00Z");
        map["content_hash_at_approval"] = json!(review_map_content_hash(&map));

        let body = response_json(&save(project.path(), &map).await);
        assert_eq!(body["approved"], json!(false));

        let stored: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(map_file(&canonical, "never-approves")).unwrap(),
        )
        .expect("parses");
        assert_eq!(stored["approved"], json!(false));
        assert_eq!(stored["approved_at"], serde_json::Value::Null);
        assert_eq!(stored["content_hash_at_approval"], serde_json::Value::Null);
        assert!(!approval_is_valid(&stored));
        assert_eq!(
            response_json(&load(project.path(), "never-approves").await)["approval_valid"],
            json!(false)
        );
    }

    /// Design D5: a save that changes nothing keeps the approval, so the tools
    /// do not revoke a gate the human never re-opened over their own
    /// `saved_at` rewrite.
    #[tokio::test]
    async fn resaving_identical_content_keeps_the_approval() {
        let (project, _canonical) = scanned_project("idempotent");
        save(project.path(), &sample_map("idempotent")).await;
        approve(project.path(), "idempotent").await;

        let body = response_json(&save(project.path(), &sample_map("idempotent")).await);
        assert_eq!(body["approved"], json!(true));
        let loaded = response_json(&load(project.path(), "idempotent").await);
        assert_eq!(loaded["map"]["approved"], json!(true));
        assert_eq!(loaded["approval_valid"], json!(true));
        // The timestamp of the human's decision is not refreshed by a save.
        assert!(loaded["map"]["approved_at"].is_string());
    }

    /// Spec scenario "editing an approved map revokes approval" — and the
    /// undo does not quietly restore it: only a human calling
    /// `approve_photo_review_map` can produce that state again.
    #[tokio::test]
    async fn editing_an_approved_map_revokes_approval_and_undo_does_not_restore_it() {
        let (project, _canonical) = scanned_project("revoke");
        save(project.path(), &sample_map("revoke")).await;
        approve(project.path(), "revoke").await;

        let mut edited = sample_map("revoke");
        edited["components"][0]["value"] = json!("47k");
        let body = response_json(&save(project.path(), &edited).await);
        assert_eq!(body["approved"], json!(false));

        let loaded = response_json(&load(project.path(), "revoke").await);
        assert_eq!(loaded["map"]["approved"], json!(false));
        assert_eq!(loaded["map"]["approved_at"], serde_json::Value::Null);
        assert_eq!(
            loaded["map"]["content_hash_at_approval"],
            serde_json::Value::Null
        );
        assert_eq!(loaded["approval_valid"], json!(false));

        // Reverting the edit restores the content, not the approval.
        let reverted = response_json(&save(project.path(), &sample_map("revoke")).await);
        assert_eq!(reverted["approved"], json!(false));
    }

    #[tokio::test]
    async fn save_rejects_a_malformed_map_and_an_unknown_net_source() {
        let (project, canonical) = scanned_project("malformed");

        let mut missing_scale = sample_map("malformed");
        missing_scale
            .as_object_mut()
            .unwrap()
            .remove("scale_reference");
        let message = response_text(&save(project.path(), &missing_scale).await);
        assert!(
            message.contains("scale_reference") && message.contains("Nothing was written"),
            "{message}"
        );

        let mut invented_source = sample_map("malformed");
        invented_source["nets"][0]["source"] = json!("guessed");
        let message = response_text(&save(project.path(), &invented_source).await);
        assert!(message.contains("traced, inferred, manual"), "{message}");

        assert!(
            !map_file(&canonical, "malformed").exists(),
            "a rejected map must not leave a file behind"
        );
    }

    /// Design D13 rule 3: map ids are server-assigned, so a map whose scan
    /// directory does not exist is a map that does not exist — never a
    /// directory to mint on the caller's say-so.
    #[tokio::test]
    async fn save_refuses_a_map_id_no_scan_ever_assigned() {
        let (project, canonical) = scanned_project("real-map");
        let message = response_text(&save(project.path(), &sample_map("invented-map")).await);
        assert!(message.contains("scan_pcb_photo"), "{message}");
        assert!(
            !canonical
                .join(".konnect")
                .join("photo_intake")
                .join("invented-map")
                .exists(),
            "no directory is minted for an unknown map id"
        );
    }

    // ─── load_photo_review_map (task 3.3) ────────────────────────────────────

    /// Spec scenario "the review map survives across sessions": what comes
    /// back is what is on disk, including edits made with a text editor
    /// between sessions.
    #[tokio::test]
    async fn the_review_map_survives_across_sessions_with_hand_edits_intact() {
        let (project, canonical) = scanned_project("sessions");
        save(project.path(), &sample_map("sessions")).await;

        let path = map_file(&canonical, "sessions");
        let mut stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("parses");
        stored["components"][0]["ref"] = json!("R42");
        stored["components"][0]["reviewer_note"] = json!("checked against the photo");
        std::fs::write(&path, serde_json::to_string_pretty(&stored).unwrap()).unwrap();

        let loaded = response_json(&load(project.path(), "sessions").await);
        assert_eq!(loaded["map"]["components"][0]["ref"], json!("R42"));
        assert_eq!(
            loaded["map"]["components"][0]["reviewer_note"],
            json!("checked against the photo"),
            "a key the struct has never heard of survives the round trip"
        );
        assert_eq!(loaded["map"], stored);
    }

    /// The orchestrator's decision, and design D6 step 3's reason for it: the
    /// map's own `approved` flag is reported as found, while `approval_valid`
    /// answers whether it still covers the content — server-side, so a
    /// consumer never has to recompute the hash to re-check the gate.
    #[tokio::test]
    async fn load_reports_approval_valid_false_after_an_out_of_band_edit() {
        let (project, canonical) = scanned_project("out-of-band");
        save(project.path(), &sample_map("out-of-band")).await;
        approve(project.path(), "out-of-band").await;

        let before = response_json(&load(project.path(), "out-of-band").await);
        assert_eq!(before["map"]["approved"], json!(true));
        assert_eq!(before["approval_valid"], json!(true));

        // Hand-edit the components straight in the file, bypassing save.
        let path = map_file(&canonical, "out-of-band");
        let mut stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("parses");
        stored["components"][0]["value"] = json!("1M");
        std::fs::write(&path, serde_json::to_string_pretty(&stored).unwrap()).unwrap();

        let after = response_json(&load(project.path(), "out-of-band").await);
        assert_eq!(
            after["map"]["approved"],
            json!(true),
            "the file still says what it says"
        );
        assert_eq!(
            after["approval_valid"],
            json!(false),
            "but the approval no longer covers this content"
        );

        // A reformat, by contrast, is not an edit: the hash is over content,
        // not bytes.
        std::fs::write(&path, serde_json::to_string(&stored).unwrap()).unwrap();
        let compact = response_json(&load(project.path(), "out-of-band").await);
        assert_eq!(compact["approval_valid"], json!(false));
    }

    /// Design D13 rule 2. The sentinel is a perfectly valid review map placed
    /// outside the project: if a rejected `map_id` ever reached a path join,
    /// this is the file that would come back.
    #[tokio::test]
    async fn map_id_traversal_is_rejected_and_reads_nothing() {
        let (project, _canonical) = scanned_project("good-map");
        let outside = tempfile::tempdir().expect("outside project");
        let outside_canonical = outside.path().canonicalize().expect("canonical");
        let stolen_dir = outside_canonical
            .join(".konnect")
            .join("photo_intake")
            .join("secret");
        std::fs::create_dir_all(&stolen_dir).unwrap();
        std::fs::write(
            stolen_dir.join(REVIEW_MAP_FILE),
            r#"{"map_id": "secret", "stolen": true}"#,
        )
        .unwrap();
        let outside_name = outside_canonical
            .file_name()
            .expect("temp dir name")
            .to_string_lossy()
            .into_owned();

        let traversal = format!("../../../{outside_name}/.konnect/photo_intake/secret");
        for bad in [
            traversal.as_str(),
            "..",
            "../good-map",
            "a/b",
            r"a\b",
            "",
            "has space",
            "dot.dot",
            "C:",
            &"x".repeat(65),
        ] {
            for result in [
                load(project.path(), bad).await,
                approve(project.path(), bad).await,
            ] {
                let message = response_text(&result);
                assert!(
                    message.contains("'map_id' must match"),
                    "{bad:?} must be rejected as a token, got: {message}"
                );
                assert!(
                    !message.contains("stolen"),
                    "{bad:?} reached the filesystem: {message}"
                );
            }
        }

        // A map id that is a legal token but names another project's map is
        // still not reachable: the directory is computed under this project.
        let message = response_text(&load(project.path(), "secret").await);
        assert!(message.contains("scan_pcb_photo"), "{message}");
        assert!(!message.contains("stolen"), "{message}");
    }

    // ─── approve_photo_review_map (task 3.4) ─────────────────────────────────

    /// Spec scenario "approval requires an explicit call": this is the action
    /// that records the decision, the time it was taken, and exactly what was
    /// decided about.
    #[tokio::test]
    async fn approve_records_the_hash_and_a_timestamp() {
        let (project, canonical) = scanned_project("approve-1");
        save(project.path(), &sample_map("approve-1")).await;

        let body = response_json(&approve(project.path(), "approve-1").await);
        assert_eq!(body["approved"], json!(true));
        let approved_at = body["approved_at"].as_str().expect("approved_at");
        assert!(
            approved_at.ends_with('Z') && approved_at.len() == 20,
            "{approved_at}"
        );

        let stored: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(map_file(&canonical, "approve-1")).unwrap(),
        )
        .expect("parses");
        assert_eq!(stored["approved"], json!(true));
        assert_eq!(stored["approved_at"], json!(approved_at));
        assert_eq!(
            stored["content_hash_at_approval"],
            json!(review_map_content_hash(&stored)),
            "the recorded hash is the hash of the content that was approved"
        );
        assert_eq!(
            body["content_hash_at_approval"],
            stored["content_hash_at_approval"]
        );
        assert!(approval_is_valid(&stored));
    }

    /// Approval is a statement about content a human read. A map broken by a
    /// hand edit is not content anyone read, and approving it would record a
    /// hash over a shape no consumer can use.
    #[tokio::test]
    async fn approve_refuses_a_map_broken_by_a_hand_edit() {
        let (project, canonical) = scanned_project("broken");
        save(project.path(), &sample_map("broken")).await;
        let path = map_file(&canonical, "broken");
        let mut stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("parses");
        stored["components"][0]["confidence"] = json!("very high");
        std::fs::write(&path, serde_json::to_string_pretty(&stored).unwrap()).unwrap();

        let message = response_text(&approve(project.path(), "broken").await);
        assert!(message.contains("cannot be approved"), "{message}");
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("parses");
        assert_eq!(after["approved"], json!(false), "the file was left alone");
    }

    #[tokio::test]
    async fn approving_a_map_that_was_never_saved_is_an_error_not_a_new_file() {
        let (project, canonical) = scanned_project("scanned-not-saved");
        let message = response_text(&approve(project.path(), "scanned-not-saved").await);
        assert!(message.contains("save_photo_review_map"), "{message}");
        assert!(!map_file(&canonical, "scanned-not-saved").exists());
    }

    // ─── Tool schemas (task 4.2, design D2) ──────────────────────────────────

    fn schema_of(name: &str) -> serde_json::Value {
        tools()
            .into_iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} is defined"))
            .input_schema
    }

    fn property_names(schema: &serde_json::Value) -> Vec<String> {
        let mut names: Vec<String> = schema["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_map_tool_schemas_match_design_d2() {
        let save_schema = schema_of("save_photo_review_map");
        assert_eq!(property_names(&save_schema), vec!["map", "project_dir"]);
        assert_eq!(save_schema["required"], json!(["project_dir", "map"]));
        assert_eq!(save_schema["properties"]["map"]["type"], json!("object"));
    }

    /// The published `map` subschema must name every review-map field and must
    /// stay open to the keys a reviewer adds by hand.
    ///
    /// Both halves are load-bearing. `ToolDef::new` closes any object
    /// subschema that declares no `additionalProperties`, so a `map` with no
    /// `properties` published a schema that accepted `{}` and nothing else and
    /// no caller could save at all; and a *closed* `map` would refuse exactly
    /// the annotations `save_photo_review_map` is required to persist.
    #[test]
    fn the_map_schema_names_every_review_map_field_and_stays_open() {
        let map_schema = schema_of("save_photo_review_map")["properties"]["map"].clone();

        let declared: std::collections::BTreeSet<String> = map_schema["properties"]
            .as_object()
            .expect("the map schema declares properties")
            .keys()
            .cloned()
            .collect();
        let actual: std::collections::BTreeSet<String> = fixture_review_map()
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            declared, actual,
            "the map schema and PhotoReviewMap (design D8) have drifted"
        );

        for (what, subschema) in [
            ("map", &map_schema),
            (
                "components[]",
                &map_schema["properties"]["components"]["items"],
            ),
            ("nets[]", &map_schema["properties"]["nets"]["items"]),
        ] {
            assert_eq!(
                subschema["additionalProperties"],
                json!(true),
                "{what} must stay open so hand annotations survive a save"
            );
        }

        assert_eq!(
            map_schema["properties"]["map_id"]["pattern"],
            json!(MAP_ID_PATTERN),
            "save enforces the map_id token in its schema too"
        );
        assert_eq!(
            map_schema["properties"]["nets"]["items"]["properties"]["source"]["enum"],
            json!(NET_SOURCES)
        );
    }

    #[test]
    fn the_load_and_approve_schemas_match_design_d2() {
        for name in ["load_photo_review_map", "approve_photo_review_map"] {
            let schema = schema_of(name);
            assert_eq!(
                property_names(&schema),
                vec!["map_id", "project_dir"],
                "{name}"
            );
            assert_eq!(
                schema["required"],
                json!(["project_dir", "map_id"]),
                "{name}"
            );
            assert_eq!(
                schema["properties"]["map_id"]["pattern"],
                json!(MAP_ID_PATTERN),
                "{name} enforces the map_id token in its schema too"
            );
        }
    }
}

#[cfg(test)]
mod synthetic_board {
    use std::path::Path;

    /// Four light rectangles on a board-green field, at the bbox coordinates
    /// the checked-in capture was produced from.
    pub(super) fn write_synthetic_board_png(path: &Path) -> anyhow::Result<()> {
        use image::{Rgb, RgbImage};

        const BOARD: Rgb<u8> = Rgb([12, 74, 39]);
        const PART: Rgb<u8> = Rgb([210, 210, 205]);
        let mut board = RgbImage::from_pixel(800, 600, BOARD);
        for [x, y, width, height] in [
            [100u32, 100, 121, 81],
            [225, 115, 271, 41],
            [295, 195, 51, 31],
            [500, 100, 61, 201],
        ] {
            for row in y..(y + height).min(600) {
                for column in x..(x + width).min(800) {
                    board.put_pixel(column, row, PART);
                }
            }
        }
        board.save(path)?;
        Ok(())
    }

    #[test]
    fn helper_writes_a_readable_png() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.png");
        write_synthetic_board_png(&path).expect("writes");
        let decoded = image::open(&path).expect("decodes");
        assert_eq!((decoded.width(), decoded.height()), (800, 600));
    }
}

#[cfg(test)]
mod live_retrace_tests {
    use super::synthetic_board::write_synthetic_board_png;
    use super::test_support::{response_json, test_ctx};
    use super::*;

    /// The live path, off by default. `RETRACE_PYTHON` is read with `expect`
    /// rather than skipped silently, mirroring `FREEROUTING_JAR`
    /// (`freerouting_mcp.rs:903`): an `--ignored` run that cannot find an
    /// interpreter fails loudly instead of passing vacuously.
    #[tokio::test]
    #[ignore = "requires Python with retrace installed (set RETRACE_PYTHON)"]
    async fn scan_pcb_photo_runs_a_real_retrace_scan() {
        let python = std::env::var("RETRACE_PYTHON").expect("set RETRACE_PYTHON");
        let project = tempfile::tempdir().expect("tempdir");
        let image = project.path().join("board.png");
        write_synthetic_board_png(&image).expect("synthetic board");

        let ctx = test_ctx();
        let result = handle_scan_pcb_photo(
            &json!({
                "image_path": image.to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
                "python_path": python,
                "timeout_seconds": 120,
            }),
            &ctx,
        )
        .await
        .expect("handler returns");
        assert!(!result.is_error, "{:?}", result.content);

        let payload = response_json(&result);
        assert!(
            !payload["components"]
                .as_array()
                .expect("components")
                .is_empty(),
            "the synthetic board must yield at least one component: {payload}"
        );
        assert!(payload["analysis_json_path"].as_str().is_some());
        let duration = payload["duration_seconds"].as_f64().expect("duration");
        assert!(duration > 0.0 && duration < 120.0, "{duration}");

        // Design D11's derivation, asserted against both raw signals.
        let evidence = &payload["fallback_evidence"];
        let yolo = evidence["yolo_warning_seen"].as_bool().expect("yolo flag");
        let detection = evidence["extras_detection"]
            .as_bool()
            .expect("detection flag");
        assert!(evidence["ocr_warning_seen"].is_boolean());
        assert!(evidence["extras_ocr"].is_boolean());
        assert_eq!(
            payload["used_fallback"].as_bool().expect("used_fallback"),
            yolo || !detection,
            "used_fallback must be stderr OR'd with the extras probe: {payload}"
        );

        // The raw analysis stays under the project as evidence. Both sides are
        // compared without the Windows verbatim prefix, which is what the
        // response carries.
        let reported = payload["analysis_json_path"]
            .as_str()
            .expect("analysis path");
        let analysis_path = PathBuf::from(reported);
        assert!(analysis_path.is_file(), "{reported}");
        let project_root = subprocess_arg(&project.path().canonicalize().expect("canonical"));
        assert!(
            reported.starts_with(&project_root),
            "{reported} is not under {project_root}"
        );
    }
}
