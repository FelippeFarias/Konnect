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
use crate::tools::{require_str, ServerConfig, ToolContext, ToolDef};
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
///
/// Which prefix it is decides what to strip, and getting that wrong is not
/// cosmetic. `std::path::Prefix::VerbatimUNC` spells a network share as
/// `\\?\UNC\srv\share\…`, so dropping the whole prefix yields
/// `UNC\srv\share\…` — a **relative** path, which would send retrace's `-o`
/// output into the server process's working directory rather than into the
/// project. Only `VerbatimDisk` (`\\?\C:\…`) may lose its prefix outright;
/// `VerbatimUNC` keeps a UNC root, and any other verbatim form (a volume
/// GUID, a device path) means nothing without its prefix and is left as is.
fn subprocess_arg(path: &Path) -> String {
    let raw = path.to_string_lossy().into_owned();
    let Some(rest) = raw.strip_prefix(r"\\?\") else {
        return raw;
    };
    if let Some(share) = rest.strip_prefix(r"UNC\") {
        return format!(r"\\{share}");
    }
    let mut start = rest.chars();
    let verbatim_disk = matches!(
        (start.next(), start.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    );
    if verbatim_disk {
        return rest.to_string();
    }
    raw
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
    /// The candidates that were tried and rejected before `python_path` won,
    /// each with the diagnostic that rejected it. Carried on the capability
    /// rather than beside it so every consumer of a resolution reports the
    /// same thing: a fall-through past the interpreter the caller named is
    /// only visible if the tool that fell through says so.
    pub candidates_tried: Vec<String>,
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
                    candidates_tried: attempts,
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
                "candidates_tried": capability.candidates_tried,
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
///
/// **Both extras count.** `ultralytics` (detection) and `easyocr` (OCR) are
/// separate installs, and the flag's published meaning is a biconditional:
/// `used_fallback: true` means marking, `value` and `part_number` were *never
/// attempted* (`SKILL.md`, `references/review-map-schema.md`,
/// `pcb-photo-intake-agent.md`). A detection-only install never attempts them
/// either, so reporting `false` there would teach a reviewer to read an empty
/// `value` as "nothing was printed on the part" — the silent guess this whole
/// change exists to prevent. `fallback_evidence` keeps all four raw signals,
/// so which half was missing stays visible in the tool's own output.
fn derive_fallback(stderr: &str, extras: RetraceExtras) -> (bool, serde_json::Value) {
    let yolo_warning_seen = stderr.contains("YOLO not available");
    let ocr_warning_seen = stderr.contains("easyocr is not installed");
    let used_fallback = yolo_warning_seen || ocr_warning_seen || !extras.detection || !extras.ocr;
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

/// The `scan_pcb_photo` response. A function rather than a `json!` inside the
/// handler so the shape is assertable without an interpreter: the handler
/// around it is 200 lines of subprocess plumbing that no default-run test can
/// reach.
fn build_scan_response(
    map_id: &str,
    capability: &RetraceCapability,
    analysis: &RetraceAnalysis,
    analysis_path: &Path,
    duration_seconds: f64,
    stderr: &str,
) -> serde_json::Value {
    let (used_fallback, fallback_evidence) = derive_fallback(stderr, capability.extras);
    json!({
        "map_id": map_id,
        "components": analysis.components,
        "traces": analysis.traces,
        "pattern_matches": analysis.pattern_matches,
        "analysis_json_path": subprocess_arg(analysis_path),
        "duration_seconds": duration_seconds,
        "used_fallback": used_fallback,
        "fallback_evidence": fallback_evidence,
        // Spelled exactly as `check_retrace` spells them (design D15): a scan
        // run by an interpreter the caller did not name must be visible in the
        // scan's own result, not inferred from a separate probe call.
        "python_path": subprocess_arg(&capability.python_path),
        "candidates_tried": capability.candidates_tried,
    })
}

/// Design D1. Nothing on disk changes until every argument and every bound has
/// been checked: the view is rendered in memory first, and only then does the
/// `views/` directory get created and the PNG written. A rejected call leaves
/// the map directory exactly as it found it.
///
/// Synchronous CPU work inside an async handler on purpose: there is no
/// network and no subprocess to await, and the size caps in [`render_view`]
/// are what bound how long it can run.
async fn handle_prepare_board_photo(
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
    let map_id = match require_str(args, "map_id") {
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
    // Design D8: the creating variant is `scan_pcb_photo`'s alone. A `map_id`
    // whose directory is not there names a map that does not exist.
    let map_dir = match existing_map_dir(&project_dir, &map_id) {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };

    let label = match args.get("label").filter(|value| !value.is_null()) {
        None => None,
        Some(value) => {
            let Some(raw) = value.as_str() else {
                return Ok(CallToolResult::error("'label' must be a string."));
            };
            // Validated before any path join, by the same token rule `map_id`
            // uses — which rejects `..`, `/`, `\`, `:` and NUL by construction.
            if let Err(message) = validate_path_token(raw, "label") {
                return Ok(CallToolResult::error(message));
            }
            Some(raw.to_string())
        }
    };
    let crop = match parse_crop(args) {
        Ok(crop) => crop,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    let rotate = match parse_rotate(args) {
        Ok(rotate) => rotate,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    let scale = match parse_scale(args) {
        Ok(scale) => scale,
        Err(message) => return Ok(CallToolResult::error(message)),
    };

    let rendered = match render_view(&image_path, crop, rotate, scale) {
        Ok(rendered) => rendered,
        Err(message) => return Ok(CallToolResult::error(message)),
    };

    let views_dir = match prepare_views_dir(&map_dir) {
        Ok(path) => path,
        Err(message) => return Ok(CallToolResult::error(message)),
    };
    let file_name = match &label {
        Some(label) => format!("{label}.png"),
        None => format!("{}.png", next_view_number(&views_dir)),
    };
    let view_path = views_dir.join(file_name);
    // Design fix (round-1 review MINOR-4): a reused `label`, or an
    // auto-assigned number that collides after an earlier view was deleted,
    // must never silently replace a file. Evidence pointers in an approved
    // `dossier` name a `{view, rect_px}` pair, and a reviewer relies on that
    // pair staying stable once approved — an overwritten view can turn an
    // approved pointer into a picture of something else with no signal at
    // all. Refuse instead, naming the path so the caller can pick a
    // different label or remove the old file deliberately.
    if view_path.exists() {
        return Ok(CallToolResult::error(format!(
            "{} already exists. prepare_board_photo never overwrites a view: choose a \
             different label, or remove the existing file yourself if you mean to replace it.",
            view_path.display()
        )));
    }
    if let Err(error) = rendered.image.save(&view_path) {
        return Ok(CallToolResult::error(format!(
            "Could not write {} ({error}).",
            view_path.display()
        )));
    }

    // Design D3: reported only when the map the view belongs to already
    // carries a resolved scale. Never estimated here — this tool has no way to
    // know what a pixel is worth.
    let mm_per_px = read_review_map(&map_dir.join(REVIEW_MAP_FILE))
        .await
        .ok()
        .and_then(|map| {
            map.get("scale_reference")
                .and_then(|scale| scale.get("mm_per_px"))
                .and_then(serde_json::Value::as_f64)
        });

    info!(
        map_id = %map_id,
        view = %view_path.display(),
        "[BETA] board photo view prepared"
    );

    let mut response = json!({
        "view_path": subprocess_arg(&view_path),
        "source_size_px": rendered.source_size_px,
        "source_rect_px": rendered.source_rect_px,
        "output_size_px": rendered.output_size_px,
        "exif_orientation": format!("{:?}", rendered.orientation),
    });
    if let Some(mm_per_px) = mm_per_px {
        response["mm_per_px"] = json!(mm_per_px);
    }
    Ok(CallToolResult::json(&response))
}

/// The project whose configuration applies to this call: the `project_dir` the
/// caller named, else the server's configured project.
///
/// `check_retrace` and `scan_pcb_photo` both resolve it here. They used to
/// differ — the probe read `ctx.config.project_dir` while the scan read its
/// `project_dir` argument — so a Phase-0 capability report could name the
/// interpreter one project configured while the scan used another's.
fn config_project_dir(argument: Option<&Path>, config: &ServerConfig) -> Option<PathBuf> {
    argument
        .map(Path::to_path_buf)
        .or_else(|| config.project_dir.clone())
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
    /// The board dossier (design D4): what the photos say this board is, with
    /// an evidence pointer and a confidence behind every claim. Untyped on
    /// purpose — it is reviewed content a human edits by hand, so the struct
    /// validates its presence and shape, never its field list.
    ///
    /// `skip_serializing_if` is load-bearing, not cosmetic (design D6): the
    /// save path overlays `to_value(&parsed)` onto the incoming map, so a
    /// `None` that serialized as `null` would write `"dossier": null` into
    /// every record, join the content hash, and revoke every approval in the
    /// field on its next save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dossier: Option<serde_json::Value>,
    /// The design brief derived from the dossier (design D5). Optional,
    /// untyped and skipped when absent for exactly the reasons above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_brief: Option<serde_json::Value>,
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
    /// Millimeters per pixel, present only when the agent tied the scale to a
    /// named physical feature (design D3). Always paired with `evidence`: a
    /// scale nobody can name evidence for stays unresolved rather than
    /// estimated.
    ///
    /// Skipped when absent for design D6's reason — see [`PhotoReviewMap`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mm_per_px: Option<f64>,
    /// The feature and reasoning the `mm_per_px` above rests on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
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

/// Sections this change adds to the gate (design D6). Covered only when the
/// saved map actually carries them, so a map written before this change —
/// which never had them — hashes to exactly the bytes it always did.
///
/// A named `const` rather than two inline literals because
/// `every_schema_key_is_either_hashed_or_deliberately_not` chains it: a third
/// section added to the schema without a hash decision then fails the suite
/// instead of shipping outside the gate.
const OPTIONAL_CONTENT_KEYS: [&str; 2] = ["dossier", "design_brief"];

/// Design D3's additions to the scale reference. Same conditional rule, same
/// reason: a map whose scale was never resolved hashes as it always did.
const SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS: [&str; 2] = ["mm_per_px", "evidence"];

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

/// The design D8 fields of one component. The hash projects every element onto
/// exactly these, so a reviewer annotation nested inside a component is as
/// invisible to the gate as one at the top level — which is what D8 and D16
/// promise about the three objects the schema declares open.
const COMPONENT_CONTENT_KEYS: [&str; 8] = [
    "component_id",
    "ref",
    "type",
    "value",
    "footprint_suggestion",
    "confidence",
    "bbox_px",
    "approved",
];

/// The design D8 fields of one net.
const NET_CONTENT_KEYS: [&str; 2] = ["connections", "source"];

/// The design D8 fields of the scale reference.
const SCALE_REFERENCE_CONTENT_KEYS: [&str; 2] = ["kind", "value"];

/// Component fields where retrace's `""` means "never read" (design D1), which
/// `save` normalizes to `null`. The hash applies the same normalization, or the
/// save that normalizes would revoke the approval it had just matched.
const EMPTY_STRING_IS_NULL: [&str; 2] = ["value", "footprint_suggestion"];

/// One object of the review map, projected onto the design D8 fields it is
/// allowed to contribute to the hash, with D8's own normalization applied.
///
/// A value that is not an object at all is hashed verbatim: both writing paths
/// run `validate_incoming_map` first, so the only caller that can reach a
/// broken shape is [`approval_is_valid`] on a file someone hand-edited into
/// nonsense, where no digest can match anyway.
fn hashed_fields(
    value: &serde_json::Value,
    keys: &[&str],
    empty_string_is_null: &[&str],
) -> serde_json::Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut covered = serde_json::Map::new();
    for key in keys {
        let mut field = object.get(*key).cloned().unwrap_or(serde_json::Value::Null);
        if empty_string_is_null.contains(key) && field.as_str() == Some("") {
            field = serde_json::Value::Null;
        }
        covered.insert((*key).to_string(), field);
    }
    serde_json::Value::Object(covered)
}

/// [`hashed_fields`] plus keys that are covered only when the object actually
/// carries them (design D6).
///
/// Separate from [`hashed_fields`] on purpose, not an `optional_keys`
/// parameter added to it: that function's *unconditional* insert is what makes
/// a component with no `ref` contribute a `"ref": null` member, and every
/// digest stored in the field depends on it. Teaching it to skip absent keys
/// would change the canonical bytes of every existing map and invalidate every
/// approval already granted.
fn hashed_fields_with_optional(
    value: &serde_json::Value,
    keys: &[&str],
    optional_keys: &[&str],
) -> serde_json::Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let serde_json::Value::Object(mut covered) = hashed_fields(value, keys, &[]) else {
        return value.clone();
    };
    for key in optional_keys {
        if let Some(field) = object.get(*key) {
            covered.insert((*key).to_string(), field.clone());
        }
    }
    serde_json::Value::Object(covered)
}

/// [`hashed_fields`] over an array, keeping stored order (design D16).
fn hashed_elements(
    value: Option<&serde_json::Value>,
    keys: &[&str],
    empty_string_is_null: &[&str],
) -> serde_json::Value {
    match value {
        Some(serde_json::Value::Array(items)) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| hashed_fields(item, keys, empty_string_is_null))
                .collect(),
        ),
        Some(other) => other.clone(),
        None => serde_json::Value::Null,
    }
}

/// SHA-256, lowercase hex, over a fresh object holding only [`CONTENT_KEYS`],
/// each of them projected onto the design D8 fields of its own shape
/// ([`COMPONENT_CONTENT_KEYS`], [`NET_CONTENT_KEYS`],
/// [`SCALE_REFERENCE_CONTENT_KEYS`]) rather than cloned whole — the exact
/// digest form `design_hash.rs:47` already produces. Cloning the subtrees put
/// every key nested inside `components`, `nets` and `scale_reference` in the
/// hash, so a reviewer note added to a component after the approval revoked
/// it; those are the three objects D8 declares open precisely so notes can go
/// there. The one implementation of the gate's "is this still the content a
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
        let field = map.get(key);
        covered.insert(
            key.to_string(),
            match key {
                "scale_reference" => match field {
                    Some(value) => hashed_fields_with_optional(
                        value,
                        &SCALE_REFERENCE_CONTENT_KEYS,
                        &SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS,
                    ),
                    None => serde_json::Value::Null,
                },
                "components" => {
                    hashed_elements(field, &COMPONENT_CONTENT_KEYS, &EMPTY_STRING_IS_NULL)
                }
                "nets" => hashed_elements(field, &NET_CONTENT_KEYS, &[]),
                // `source_images` is an array of strings: it has no object to
                // hide an unknown key in.
                _ => field.cloned().unwrap_or(serde_json::Value::Null),
            },
        );
    }
    // Design D6: the two additive sections join the hashed bytes only once the
    // map carries them, so a map written before they existed hashes to exactly
    // the bytes it always did — and adding one to an approved map moves the
    // digest, which is what makes the second review checkpoint the same
    // mechanism as the first.
    //
    // Cloned whole, with no field projection. The projection the three older
    // sections get exists to keep tool-written bookkeeping and reviewer
    // annotations nested in a machine-written record out of the gate; neither
    // exists here — no tool writes into these two, and they *are* the reviewed
    // content — so a key list would buy nothing and could only fail open.
    for key in OPTIONAL_CONTENT_KEYS {
        if let Some(section) = map.get(key) {
            covered.insert(key.to_string(), section.clone());
        }
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

pub(crate) fn now_rfc3339_utc() -> String {
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
///
/// Takes the field name because `prepare_board_photo`'s `label` is a path
/// component for exactly the same reason `map_id` is, and an error that names
/// the wrong argument sends the caller to fix the wrong thing.
fn validate_path_token(raw: &str, field: &str) -> Result<(), String> {
    let length = raw.chars().count();
    let valid = (1..=64).contains(&length)
        && raw.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "'{field}' must match {MAP_ID_PATTERN} (1-64 characters of A-Z a-z 0-9 _ -); got \
             {raw:?}. No file was read or written."
        ))
    }
}

fn validate_map_id(raw: &str) -> Result<(), String> {
    validate_path_token(raw, "map_id")
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

/// The JSON type name to quote back at a caller who sent the wrong shape.
fn section_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
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
    // Design D6: on disk one of these sections is either a JSON object or not
    // there at all. `Option<Value>` would happily carry a string, an array or
    // a `null`, and a `null` is the dangerous one — it would reach the hash as
    // a present-but-empty member and revoke an approval nobody edited. Refused
    // here, with the alternative named, rather than silently normalized.
    for key in OPTIONAL_CONTENT_KEYS {
        if let Some(section) = map.get(key) {
            if !section.is_object() {
                return Err(format!(
                    "'{key}' is present but is not a JSON object (got {}). Omit the key entirely \
                     to remove the section. Nothing was written.",
                    section_type_name(section)
                ));
            }
        }
    }
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

// ─── Board photo views (design D1) ────────────────────────────────────────────

/// The `views/` subdirectory of a map, where `prepare_board_photo` writes.
const VIEWS_DIR: &str = "views";

/// Design D1's four bounds, named once so the schema, the error messages and
/// the tests cannot drift apart.
///
/// The handler is synchronous CPU-and-memory work with no network and no
/// subprocess, so a wall-clock timeout would guard nothing these already
/// guard. 50 MP at RGBA8 is 200 MB decoded and a 4096 x 4096 output is 67 MB,
/// which puts peak resident under ~300 MB per call; `MAX_DECODE_ALLOC_BYTES`
/// backstops a header that lies about its own dimensions.
const MAX_SOURCE_MEGAPIXELS: u64 = 50;
const MAX_SOURCE_PIXELS: u64 = MAX_SOURCE_MEGAPIXELS * 1_000_000;
const MAX_VIEW_SIDE_PX: u32 = 4096;
const MIN_VIEW_SCALE: f64 = 0.25;
const MAX_VIEW_SCALE: f64 = 4.0;
const MAX_DECODE_ALLOC_BYTES: u64 = 512 * 1024 * 1024;

/// `[x, y, w, h]` of a crop rectangle, in oriented source-image pixel space.
type CropRect = [u32; 4];

/// One rendered view, still in memory: nothing is written until every bound
/// has been checked, so a rejected call leaves no file and no `views/`
/// directory behind.
struct RenderedView {
    image: image::DynamicImage,
    /// `[w, h]` of the decoded image **after** EXIF orientation.
    source_size_px: [u32; 2],
    /// `[x, y, w, h]` actually used, in that same oriented space.
    source_rect_px: [u32; 4],
    output_size_px: [u32; 2],
    orientation: image::metadata::Orientation,
}

/// Decode → EXIF-orient → crop → rotate → scale, design D1's steps 1-8.
///
/// The orientation dance is explicit because `ImageReader::decode()` does
/// **not** apply the tag: a phone original carries one, the reader the agent
/// used to pick its rectangle may or may not have honoured it, and the agent
/// cannot detect the mismatch from the cropped result. Orienting first and
/// reporting both the applied variant and the oriented size makes the
/// coordinate space this tool used inspectable.
fn render_view(
    image_path: &Path,
    crop: Option<CropRect>,
    rotate: u32,
    scale: f64,
) -> Result<RenderedView, String> {
    use image::ImageDecoder as _;

    let mut reader = image::ImageReader::open(image_path)
        .map_err(|error| format!("Could not open {} ({error}).", image_path.display()))?
        .with_guessed_format()
        .map_err(|error| {
            format!(
                "Could not determine the image format of {} ({error}). PNG and JPEG are \
                 supported.",
                image_path.display()
            )
        })?;
    // Set explicitly rather than inherited from the crate's default, so the
    // cap this code states is the cap it enforces. `Limits` is
    // `#[non_exhaustive]`, hence the field assignment rather than a literal.
    let mut limits = image::Limits::no_limits();
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(limits);

    let mut decoder = reader
        .into_decoder()
        .map_err(|error| format!("Could not read {} ({error}).", image_path.display()))?;
    // Read from the header, before a pixel is decoded: a crafted file whose
    // header claims 65535 x 65535 must cost nothing but this comparison.
    let (header_width, header_height) = decoder.dimensions();
    let header_pixels = u64::from(header_width) * u64::from(header_height);
    if header_pixels > MAX_SOURCE_PIXELS {
        return Err(format!(
            "{} is {header_width}x{header_height} px, which is over prepare_board_photo's \
             {MAX_SOURCE_MEGAPIXELS} MP ({MAX_SOURCE_PIXELS} px) cap. Nothing was decoded and no \
             file was written. Downscale the photo first.",
            image_path.display()
        ));
    }
    // Defaults to `NoTransforms` for a format or a file with no EXIF tag, so
    // this is safe on a WhatsApp JPEG whose metadata has been stripped.
    let orientation = decoder.orientation().map_err(|error| {
        format!(
            "Could not read the EXIF orientation of {} ({error}).",
            image_path.display()
        )
    })?;
    let mut image = image::DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Could not decode {} ({error}).", image_path.display()))?;
    image.apply_orientation(orientation);

    let (oriented_width, oriented_height) = (image.width(), image.height());
    let source_rect_px = match crop {
        // Bounds-checked against the **oriented** dimensions, and rejected
        // rather than clamped: a silently shrunk rectangle leaves the agent
        // reasoning about pixel coordinates in a space that does not exist.
        Some([x, y, width, height]) => {
            let (right, bottom) = (
                u64::from(x) + u64::from(width),
                u64::from(y) + u64::from(height),
            );
            if right > u64::from(oriented_width) || bottom > u64::from(oriented_height) {
                return Err(format!(
                    "crop [{x}, {y}, {width}, {height}] falls outside {}, which is \
                     {oriented_width}x{oriented_height} px after its EXIF orientation \
                     ({orientation:?}) was applied. No file was written.",
                    image_path.display()
                ));
            }
            image = image.crop_imm(x, y, width, height);
            [x, y, width, height]
        }
        None => [0, 0, oriented_width, oriented_height],
    };

    let image = match rotate {
        90 => image.rotate90(),
        180 => image.rotate180(),
        270 => image.rotate270(),
        _ => image,
    };

    let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
    let (output_width, output_height) = (scaled(image.width()), scaled(image.height()));
    // Checked before the destination buffer is allocated, not after.
    if output_width > MAX_VIEW_SIDE_PX || output_height > MAX_VIEW_SIDE_PX {
        return Err(format!(
            "scale {scale} would produce a {output_width}x{output_height} px view, whose long \
             side is over prepare_board_photo's {MAX_VIEW_SIDE_PX} px cap. Nothing was allocated \
             and no file was written. Use a smaller scale or a tighter crop."
        ));
    }
    let image = if (output_width, output_height) == (image.width(), image.height()) {
        // A resample at the source size is not a no-op — Lanczos3 would ring
        // the edges of a view nobody asked to resize.
        image
    } else {
        image.resize_exact(
            output_width,
            output_height,
            image::imageops::FilterType::Lanczos3,
        )
    };

    Ok(RenderedView {
        image,
        source_size_px: [oriented_width, oriented_height],
        source_rect_px,
        output_size_px: [output_width, output_height],
        orientation,
    })
}

/// `<map_dir>/views/`, created on demand and re-checked for confinement the
/// same way [`prepare_map_dir`] re-checks the map directory: a pre-existing
/// `views` that is a symlink to somewhere else is the one case a computed path
/// cannot rule out.
fn prepare_views_dir(map_dir: &Path) -> Result<PathBuf, String> {
    let views = map_dir.join(VIEWS_DIR);
    std::fs::create_dir_all(&views)
        .map_err(|error| format!("Could not create {}: {error}", views.display()))?;
    let canonical = views
        .canonicalize()
        .map_err(|error| format!("Could not resolve {}: {error}", views.display()))?;
    if !canonical.starts_with(map_dir) {
        return Err(format!(
            "Refusing to write outside the map directory: {} resolves to {}, which is not under \
             {}",
            views.display(),
            canonical.display(),
            map_dir.display()
        ));
    }
    Ok(canonical)
}

/// Design D1: `1 +` however many files the `views/` directory already holds.
/// A directory that cannot be listed counts as empty — the save below is what
/// reports a real filesystem problem.
fn next_view_number(views_dir: &Path) -> usize {
    std::fs::read_dir(views_dir)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
        + 1
}

/// `crop`, `rotate` and `scale`, validated in Rust as well as in the schema:
/// the schema is the MCP dispatcher's guard, and these handlers are also
/// called directly.
fn parse_crop(args: &serde_json::Value) -> Result<Option<CropRect>, String> {
    let Some(crop) = args.get("crop").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let Some(object) = crop.as_object() else {
        return Err("'crop' must be an object with integer x, y, w and h.".to_string());
    };
    let mut rectangle = [0u32; 4];
    for (index, (field, minimum)) in [("x", 0), ("y", 0), ("w", 1), ("h", 1)].iter().enumerate() {
        let value = object
            .get(*field)
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("'crop.{field}' is missing or is not an integer."))?;
        if value < *minimum {
            return Err(format!(
                "'crop.{field}' is {value}; it must be at least {minimum}."
            ));
        }
        rectangle[index] = u32::try_from(value)
            .map_err(|_| format!("'crop.{field}' is {value}, which is larger than any image."))?;
    }
    Ok(Some(rectangle))
}

fn parse_rotate(args: &serde_json::Value) -> Result<u32, String> {
    let Some(rotate) = args.get("rotate").filter(|value| !value.is_null()) else {
        return Ok(0);
    };
    match rotate.as_i64() {
        Some(degrees @ (0 | 90 | 180 | 270)) => Ok(degrees as u32),
        _ => Err(format!(
            "'rotate' is {rotate}; it must be one of 0, 90, 180 or 270."
        )),
    }
}

fn parse_scale(args: &serde_json::Value) -> Result<f64, String> {
    let Some(scale) = args.get("scale").filter(|value| !value.is_null()) else {
        return Ok(1.0);
    };
    let Some(factor) = scale.as_f64() else {
        return Err(format!("'scale' is {scale}; it must be a number."));
    };
    // Rejected, never clamped: a silently clamped scale makes output_size_px
    // disagree with what the agent asked for.
    if !(MIN_VIEW_SCALE..=MAX_VIEW_SCALE).contains(&factor) {
        return Err(format!(
            "'scale' is {factor}; it must be between {MIN_VIEW_SCALE} and {MAX_VIEW_SCALE}. It is \
             rejected rather than clamped, so that the view's reported size is always the size \
             you asked for. No file was written."
        ));
    }
    Ok(factor)
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

/// The `dossier` subschema (design D4), lifted out of [`review_map_schema`]
/// because one `json!` for the whole review map exceeds the macro recursion
/// limit — and because the two additive sections are read on their own.
///
/// Every array of objects is declared with a prose `description` and **no**
/// `items` subschema, following `subcircuit_hints`: the router's
/// `fixed_records_are_closed_and_only_reviewed_maps_are_extensible` walker
/// never descends into an array without `items`, so the open-record
/// allowlist stays at design D7's six paths however many fields an element
/// gains later. The per-field reference lives in the skill docs, where the
/// writing agent actually reads it.
fn dossier_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "description": "Optional board dossier: what the photos say this board is. Omit the key entirely when there is none — a null is rejected. Every claim carries basis (observed|inferred), a numeric confidence, and evidence entries shaped {view, rect_px, note?} naming a file in the map's views/ directory or one of source_images. Joins the approval hash the moment it is present, so adding it revokes an existing approval. Field-by-field reference: the kicad-board-dossier skill's dossier-schema.md.",
        "properties": {
            "identity": {
                "type": "object",
                "additionalProperties": true,
                "description": "What this board is, as one claim.",
                "properties": {
                    "summary": { "type": "string", "description": "One sentence, quoting the silkscreen identity where there is one." },
                    "basis": { "type": "string", "description": "observed or inferred." },
                    "confidence": { "type": "number", "description": "0..1." },
                    "evidence": { "type": "array", "description": "{view, rect_px:[x,y,w,h], note?} pointers." }
                }
            },
            "physical": {
                "type": "object",
                "additionalProperties": true,
                "description": "Sizes, holes and connectors. Pixel values always; millimeters only once a scale is resolved.",
                "properties": {
                    "board_size_px": { "type": "array", "items": { "type": "integer" }, "description": "[w, h] in the oriented source space." },
                    "board_size_mm": { "type": ["array", "null"], "description": "[w, h] in mm, null until a scale reference resolves one. Never estimated." },
                    "scale_status": { "type": "string", "description": "Why board_size_mm is null, when it is." },
                    "mounting_holes": { "type": "array", "description": "{position_px, role, count, evidence?} entries." },
                    "connectors": { "type": "array", "description": "{type, location_px, edge, basis, confidence, evidence?} entries." },
                    "layers_visible": { "type": "string", "description": "Which layers the photos actually show." }
                }
            },
            "component_survey": {
                "type": "array",
                "description": "One entry per visual class: {visual_class, count, count_method, count_confidence, count_alternatives[], locations[], retrace_component_ids[], notes?}. Two methods disagreeing is evidence — record both rather than the trusted number alone."
            },
            "silkscreen_markings": {
                "type": "array",
                "description": "{text, location_px, basis, confidence, evidence[]} entries, read off the board and never inferred from context."
            },
            "topology_claims": {
                "type": "array",
                "description": "{claim_id, question, basis, evidence[], hypotheses[], resolution_path} entries. A question the evidence does not settle keeps every candidate as its own hypothesis {label, description, confidence, calculation, assumptions[]}, never one guess."
            },
            "retrace_correlation": {
                "type": "array",
                "description": "{component_id, bbox_px, visual_class, overlap_confidence} entries tying survey observations to scan_pcb_photo component ids."
            },
            "photo_views_used": {
                "type": "array",
                "items": { "type": "string" },
                "description": "The view files the survey was read from."
            },
            "design_brief_seed": {
                "type": "object",
                "additionalProperties": true,
                "description": "A preliminary sketch of the implied design direction, distinct from the full design_brief.",
                "properties": {
                    "summary": { "type": "string", "description": "Topology idea, rough BOM shape, physical spec." },
                    "depends_on_open_questions": { "type": "array", "items": { "type": "string" }, "description": "claim_id values this sketch rests on." }
                }
            },
            "open_questions": {
                "type": "array",
                "items": { "type": "string" },
                "description": "What the photos could not settle. A gap belongs here, never filled with a plausible number."
            }
        }
    })
}

/// The `design_brief` subschema (design D5). Same shape rules as
/// [`dossier_schema`], same reason.
fn design_brief_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "description": "Optional design brief derived from an approved dossier. Omit the key entirely when there is none — a null is rejected. Joins the approval hash the moment it is present, which is what makes the second review checkpoint the same mechanism as the first. Field-by-field reference: the kicad-design-reconstruction skill's design-brief-schema.md.",
        "properties": {
            "derived_from_dossier": { "type": "boolean", "description": "True when this brief was written from that map's approved dossier." },
            "block_diagram": {
                "type": "array",
                "description": "{block, function, inputs[], outputs[]} entries."
            },
            "circuits": {
                "type": "array",
                "description": "{block, description, calculated_values[], derating_notes} entries; each calculated value states its formula and assumptions."
            },
            "bom": {
                "type": "array",
                "description": "{role, kicad_symbol, kicad_footprint, resolution_status, search_terms_used[], candidates[], value, quantity, source} entries. kicad_symbol/kicad_footprint come from search_symbols/search_footprints or are null with resolution_status 'unresolved' — writing one no search returned is this schema's single forbidden act."
            },
            "physical_constraints": {
                "type": "object",
                "additionalProperties": true,
                "description": "The layout constraint record: one key per row of the kicad-pcb skill's layout-methodology reference. Every key is always present; a row with no answer is null or [] AND named in unresolved, which is what distinguishes 'asked, unknown' from 'never considered'.",
                "properties": {
                    "board_size_mm": { "type": ["array", "null"], "description": "[w, h] in mm, null until resolved." },
                    "board_size_status": { "type": ["string", "null"], "description": "Why board_size_mm is null, when it is." },
                    "mounting_holes": { "type": "array", "description": "{position_mm, position_px, diameter_mm} entries." },
                    "enclosure": { "type": ["string", "null"], "description": "Enclosure constraint, or null." },
                    "max_component_height_mm": { "type": ["number", "null"], "description": "Available height, or null." },
                    "connector_edges": { "type": "array", "description": "{edge, type, pitch_mm, position_px} entries." },
                    "user_facing_parts": { "type": "array", "description": "{role, why_user_facing} entries." },
                    "net_currents": { "type": "array", "description": "{net, continuous_a, peak_a, basis, note} entries." },
                    "net_voltages": { "type": "array", "description": "{net, nominal_v, surge_v, basis, note} entries." },
                    "signal_speeds": { "type": "array", "description": "{net, frequency_hz, rise_time_ns} entries." },
                    "sensitive_nets": { "type": "array", "description": "{net, why} entries." },
                    "layer_count": { "type": ["integer", "null"], "description": "Layer count, or null." },
                    "stackup": { "type": ["string", "null"], "description": "Stackup, or null." },
                    "fabricator": { "type": ["string", "null"], "description": "Fabricator capability, or null." },
                    "assembly_notes": { "type": ["string", "null"], "description": "Assembly and test process, or null." },
                    "keep_outs": { "type": "array", "description": "{region_mm | region_px, why} entries." },
                    "unresolved": { "type": "array", "items": { "type": "string" }, "description": "Every field above still unanswered. Layout refuses to proceed when it names board_size_mm, net currents, net voltages, connector edges or the enclosure." }
                }
            },
            "assumptions": { "type": "array", "items": { "type": "string" }, "description": "What every calculation above rests on." },
            "open_questions": { "type": "array", "items": { "type": "string" }, "description": "What the brief could not settle." }
        }
    })
}

/// The JSON Schema for `save_photo_review_map`'s `map` argument, spelled out
/// rather than left as a bare `{"type": "object"}`.
///
/// `ToolDef::new` runs `close_input_schema` (`tools/mod.rs:134`), which inserts
/// `additionalProperties: false` into every object subschema that does not
/// declare one. A `map` with no `properties` therefore published a schema that
/// accepted `{}` and nothing else, and the MCP dispatcher validates before it
/// dispatches (`mcp/handler.rs:366`) — so no caller could ever save a review
/// map. Keep this in sync with [`PhotoReviewMap`] (design D8);
/// `the_map_schema_names_every_review_map_field_and_stays_open` fails when
/// they drift.
///
/// Every object a reviewer hand-edits is left open
/// (`"additionalProperties": true`, which `entry().or_insert()` preserves):
/// annotations that no tool reads must survive a save, so the schema may not
/// refuse the keys the handler is required to persist.
fn review_map_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "The full review map. Its map_id must name a scan directory that already exists (scan_pcb_photo assigns it). The tool-owned keys (approval_valid, approved, approved_at, content_hash_at_approval, saved_at) are dropped on input and re-derived from the stored record. Keys beyond these are preserved verbatim, so reviewer annotations survive a save.",
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
                    "kind": { "type": "string", "description": "e.g. board_edge_mm, package or mounting_hole_pitch." },
                    "value": { "type": "string", "description": "e.g. '50' or '0805'." },
                    "mm_per_px": { "type": "number", "description": "Millimeters per pixel, only once it is resolved from a named physical feature. Omit the key entirely when it is not — never estimate it, and never write it without evidence." },
                    "evidence": { "type": "string", "description": "The feature and reasoning mm_per_px rests on, e.g. '21px between M3 hole centers, 5mm actual pitch stated by the user'. Required whenever mm_per_px is present." }
                },
                "required": ["kind", "value"],
                "description": "User-supplied, or agent-resolved from a named physical feature; never estimated from pixels alone."
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
            "dossier": dossier_schema(),
            "design_brief": design_brief_schema(),
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
             analysis: which interpreter resolved, the retrace version, which optional \
             extras (detection, ocr) are importable, and which candidates were rejected on \
             the way there. Absence is reported as a field, never as an error — call this \
             first to diagnose any scan_pcb_photo failure.",
            json!({
                "type": "object",
                "properties": {
                    "python_path": {
                        "type": "string",
                        "description": "Python interpreter to probe. If omitted, uses photo_intake.retrace_python_path, then RETRACE_PYTHON, then PATH discovery."
                    },
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project whose config supplies photo_intake.retrace_python_path. Optional; defaults to the server's configured project. Pass the same project_dir you will pass to scan_pcb_photo, or this probe may resolve a different interpreter than the scan does."
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
            "prepare_board_photo",
            "Save a cropped, rotated and/or scaled PNG view of a board photo under an existing \
             review map's views/ directory, so you can read silkscreen text, terminals and \
             component markings the full photo is too coarse for. One image in, one view out: \
             call it once per view. EXIF orientation is applied first and reported, and crop is \
             interpreted in that oriented space. This tool decodes and re-encodes only — \
             counting, classification and OCR are yours to do from the views it produces. \
             Refuses to overwrite an existing view file — a reused label, or an auto-assigned \
             number that collides after an earlier view was deleted, is an error naming the \
             path, never a silent replace.",
            json!({
                "type": "object",
                "properties": {
                    "image_path": {
                        "type": "string",
                        "description": "Path to the source photo (PNG/JPEG). May live anywhere; it is read, never written."
                    },
                    "project_dir": {
                        "type": "string",
                        "description": "KiCad project directory the map belongs to."
                    },
                    "map_id": {
                        "type": "string",
                        "pattern": MAP_ID_PATTERN,
                        "description": "The map identifier assigned by scan_pcb_photo. Its directory must already exist — this tool never mints one."
                    },
                    "crop": {
                        "type": "object",
                        "properties": {
                            "x": { "type": "integer", "minimum": 0, "description": "Left edge, in pixels of the EXIF-oriented source." },
                            "y": { "type": "integer", "minimum": 0, "description": "Top edge, in pixels of the EXIF-oriented source." },
                            "w": { "type": "integer", "minimum": 1, "description": "Width in pixels." },
                            "h": { "type": "integer", "minimum": 1, "description": "Height in pixels." }
                        },
                        "required": ["x", "y", "w", "h"],
                        "description": "Region to cut out, in the coordinate space reported as source_size_px. Omit for the whole image. A rectangle that does not fit is an error naming the actual size, never a silently clamped one."
                    },
                    "rotate": {
                        "type": "integer",
                        "enum": [0, 90, 180, 270],
                        "description": "Clockwise rotation applied after crop, on top of any EXIF orientation already applied. For a photo taken sideways with no EXIF tag at all."
                    },
                    "scale": {
                        "type": "number",
                        "minimum": MIN_VIEW_SCALE,
                        "maximum": MAX_VIEW_SCALE,
                        "description": "Resampling factor applied after rotate. Out of range is rejected, not clamped, so output_size_px is always the size you asked for."
                    },
                    "label": {
                        "type": "string",
                        "pattern": MAP_ID_PATTERN,
                        "description": "File name stem for the view, e.g. boardA_top_left. Omitted, the view is numbered. The output directory itself is computed and never supplied."
                    }
                },
                "required": ["image_path", "project_dir", "map_id"]
            }),
            |args, ctx| async move { handle_prepare_board_photo(args, ctx).await }
        ),
        tool!(
            "save_photo_review_map",
            "Persist a photo review map as editable JSON under \
             <project_dir>/.konnect/photo_intake/<map_id>/review_map.json. The map's approval \
             state is server-owned: this tool never grants approval, and any edit to the \
             reviewed content revokes an approval the map already had. Keys the tools own \
             (approval_valid, approved, approved_at, content_hash_at_approval, saved_at) are \
             ignored on input and re-derived from the stored record, so re-saving a map you \
             loaded is safe; every other key is preserved verbatim.",
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
    // Canonicalized exactly as `scan_pcb_photo` canonicalizes it: a probe that
    // reads a different project's config than the scan it is meant to diagnose
    // is worse than no probe.
    let requested = match args["project_dir"].as_str() {
        Some(raw) => match canonical_existing_dir(raw, "project_dir") {
            Ok(path) => Some(path),
            Err(message) => return Ok(CallToolResult::error(message)),
        },
        None => None,
    };
    let project_dir = config_project_dir(requested.as_deref(), &ctx.config);
    let config = crate::tools::config::effective_config(project_dir.as_deref()).await;
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
    ctx: &ToolContext,
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

    let config_dir = config_project_dir(Some(&project_dir), &ctx.config);
    let config = crate::tools::config::effective_config(config_dir.as_deref()).await;
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

    Ok(CallToolResult::json(&build_scan_response(
        &map_id,
        &capability,
        &analysis,
        &analysis_path,
        duration_seconds,
        &stderr,
    )))
}

/// Lay `normalized` over `incoming`, keeping every key `normalized` does not
/// mention.
///
/// Objects merge key by key; arrays merge element by element with
/// `normalized`'s length authoritative (a component the struct dropped for
/// being malformed never gets here — `validate_incoming_map` refused the whole
/// map first); everything else is replaced outright, `null` included, so
/// D1's `""` → `null` normalization still lands.
///
/// Deliberately *not* `config::deep_merge`: that one treats a `null` overlay
/// as "no opinion" and keeps the base value, which is exactly backwards here —
/// "retrace read no marking" is the value being written.
fn overlay_known_fields(
    incoming: &serde_json::Value,
    normalized: serde_json::Value,
) -> serde_json::Value {
    match (incoming, normalized) {
        (serde_json::Value::Object(incoming), serde_json::Value::Object(normalized)) => {
            let mut merged = incoming.clone();
            for (key, value) in normalized {
                let base = merged.get(&key).cloned().unwrap_or(serde_json::Value::Null);
                merged.insert(key, overlay_known_fields(&base, value));
            }
            serde_json::Value::Object(merged)
        }
        (serde_json::Value::Array(incoming), serde_json::Value::Array(normalized)) => {
            serde_json::Value::Array(
                normalized
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| match incoming.get(index) {
                        Some(base) => overlay_known_fields(base, value),
                        None => value,
                    })
                    .collect(),
            )
        }
        (_, normalized) => normalized,
    }
}

/// The names the tools own inside the review map: the bookkeeping `save` and
/// `approve` write themselves, plus `approval_valid`, which `load` computes
/// server-side and never persists.
///
/// A payload carrying them is not rejected — re-saving a map you loaded is the
/// documented workflow, and a loaded map legitimately carries the four
/// persisted ones — but the incoming values are dropped before anything is
/// written, so the record on disk states only what the server decided. Without
/// this, opening the schema (`additionalProperties: true`) let a caller
/// persist `approval_valid: true` and `load` answer with two keys of that
/// name, one forged and one computed.
const TOOL_OWNED_KEYS: [&str; 5] = [
    "approval_valid",
    "approved",
    "approved_at",
    "content_hash_at_approval",
    "saved_at",
];

/// The incoming map with [`TOOL_OWNED_KEYS`] removed, so the overlay carries
/// through every *other* unknown key and none of these.
fn without_tool_owned_keys(map: &serde_json::Value) -> serde_json::Value {
    let mut stripped = map.clone();
    if let Some(object) = stripped.as_object_mut() {
        for key in TOOL_OWNED_KEYS {
            object.remove(key);
        }
    }
    stripped
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

    // The struct is the validator, not the transport. Normalizing through it
    // puts the file in design D8's shape whatever the caller sent — but it has
    // no catch-all, so serializing it alone deleted every key it does not
    // name. `SKILL.md` promises hand edits survive and then tells the agent to
    // re-save after every edit, which made that save the call that destroyed
    // the reviewer's annotations. Layering the normalized value over the
    // incoming one keeps both properties: D8's shape (and D1's empty-string
    // normalization) win on the keys the struct owns, everything else is
    // carried through untouched. The object hashed below is still the object
    // written, never one re-derived from it.
    //
    // `TOOL_OWNED_KEYS` is the one exception: those names are stripped from
    // the incoming object first, so a payload cannot smuggle `approval_valid`
    // (or a stale `approved`) past the overlay and into the record. The
    // bookkeeping written below is derived only from what is on disk.
    let mut value = overlay_known_fields(
        &without_tool_owned_keys(map_arg),
        serde_json::to_value(&parsed)?,
    );
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

    /// The two verbatim prefixes `canonicalize` produces, and a path with none.
    ///
    /// The UNC one is what bites: stripping the whole prefix from
    /// `\\?\UNC\srv\share\proj` leaves `UNC\srv\share\proj`, a *relative*
    /// path, so a project on a network share would hand retrace an output
    /// directory under the server process's CWD instead of under the project.
    #[test]
    fn subprocess_arg_drops_the_windows_verbatim_prefix() {
        // VerbatimDisk.
        assert_eq!(
            subprocess_arg(Path::new(r"\\?\C:\boards\top.png")),
            r"C:\boards\top.png"
        );
        // VerbatimUNC: the prefix becomes the UNC root, never nothing.
        assert_eq!(
            subprocess_arg(Path::new(r"\\?\UNC\srv\share\proj\top.png")),
            r"\\srv\share\proj\top.png"
        );
        assert!(
            !subprocess_arg(Path::new(r"\\?\UNC\srv\share\proj")).starts_with("UNC"),
            "a UNC path must never come out relative"
        );
        // Anything else is meaningless without its prefix, so it is left alone.
        assert_eq!(
            subprocess_arg(Path::new(r"\\?\Volume{1a2b}\boards")),
            r"\\?\Volume{1a2b}\boards"
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
            candidates_tried: Vec::new(),
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

    /// "Which interpreter did you use" must be answerable from the scan's own
    /// result, not inferred.
    ///
    /// `resolve_retrace` falls through to PATH when an explicit `python_path`
    /// or the configured `retrace_python_path` fails the import probe (D15).
    /// Without these two fields a scan run by an interpreter the user never
    /// named is indistinguishable from one that honoured them — the exact
    /// hardening `check_retrace` already carries, applied to the tool whose
    /// output ends up on a board.
    #[test]
    fn the_scan_response_names_the_interpreter_that_ran_just_as_check_retrace_does() {
        let capability = capability(false, false);
        let analysis = RetraceAnalysis::default();
        let scan = build_scan_response(
            "map-1",
            &capability,
            &analysis,
            Path::new(r"C:\proj\.konnect\photo_intake\map-1\analysis.json"),
            0.6,
            "",
        );
        let probe = build_check_response(Some(&capability), &[]);

        assert_eq!(
            scan["python_path"], probe["python_path"],
            "the scan must report the interpreter that ran, spelled as check_retrace spells it"
        );
        assert_eq!(scan["python_path"], "C:/py/python.exe");
        assert_eq!(
            scan["candidates_tried"], probe["candidates_tried"],
            "and the candidates it rejected on the way there"
        );

        // A resolution that had to fall past the caller's interpreter says so
        // in both tools.
        let fell_through = RetraceCapability {
            candidates_tried: vec!["C:/asked/python.exe: no module named retrace".to_string()],
            ..capability
        };
        let scan = build_scan_response(
            "map-1",
            &fell_through,
            &analysis,
            Path::new("/tmp/analysis.json"),
            0.6,
            "",
        );
        assert_eq!(
            scan["candidates_tried"][0],
            "C:/asked/python.exe: no module named retrace"
        );
        assert_eq!(
            build_check_response(Some(&fell_through), &[])["candidates_tried"],
            scan["candidates_tried"]
        );
    }

    /// `check_retrace` read `ctx.config.project_dir` while `scan_pcb_photo`
    /// read the `project_dir` argument, so a Phase-0 capability report could
    /// name a configured interpreter the scan then never used. One resolver,
    /// both handlers.
    #[test]
    fn both_handlers_resolve_the_config_project_the_same_way() {
        let server = ServerConfig {
            project_dir: Some(PathBuf::from("/server/project")),
            ..Default::default()
        };
        assert_eq!(
            config_project_dir(Some(Path::new("/argument/project")), &server),
            Some(PathBuf::from("/argument/project")),
            "an explicit project_dir wins"
        );
        assert_eq!(
            config_project_dir(None, &server),
            Some(PathBuf::from("/server/project")),
            "otherwise the server's configured project"
        );
        assert_eq!(
            config_project_dir(None, &ServerConfig::default()),
            None,
            "and neither means built-in defaults only"
        );
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
                "prepare_board_photo",
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

    /// `check_retrace` takes the interpreter to probe and the project whose
    /// config names one — both optional, nothing else.
    ///
    /// `project_dir` was added by the review finding that `check_retrace` read
    /// `ctx.config.project_dir` while `scan_pcb_photo` read its `project_dir`
    /// argument: an agent could not ask the probe about the project it was
    /// about to scan, so the Phase-0 report could name an interpreter the scan
    /// never used. This test previously pinned `python_path` alone.
    #[test]
    fn check_schema_offers_the_interpreter_and_the_project_whose_config_names_one() {
        let schema = tools()
            .into_iter()
            .find(|tool| tool.name == "check_retrace")
            .expect("check_retrace is defined")
            .input_schema;
        let properties = schema["properties"].as_object().expect("properties");
        assert_eq!(
            properties.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["project_dir", "python_path"]
        );
        assert_eq!(properties["project_dir"]["type"], json!("string"));
        assert_eq!(
            schema["required"],
            json!([]),
            "both stay optional — the probe is callable before a project is open"
        );
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

    /// `ultralytics` and `easyocr` are separate extras, so detection without
    /// OCR is a plausible install — and it is exactly the case three asset
    /// files describe as `used_fallback: true` ("marking, `value` and
    /// `part_number` were **never attempted**"). Reading `false` there would
    /// teach the agent that an empty `value` means "nothing printed on the
    /// part" when in fact nothing ever looked.
    #[test]
    fn used_fallback_is_true_whenever_any_extra_did_not_run() {
        let detection_only = RetraceExtras {
            detection: true,
            ocr: false,
        };
        let (used, evidence) = derive_fallback("", detection_only);
        assert!(
            used,
            "OCR never ran, so marking/value/part_number were never attempted: {evidence}"
        );
        assert_eq!(evidence["extras_detection"], true);
        assert_eq!(evidence["extras_ocr"], false);
        assert_eq!(
            evidence["ocr_warning_seen"], false,
            "the raw signals stay distinguishable even when the verdict collapses them"
        );

        // The mirror case: OCR present, detection gone.
        let ocr_only = RetraceExtras {
            detection: false,
            ocr: true,
        };
        let (used, _) = derive_fallback("", ocr_only);
        assert!(used);

        // And the stderr half, with both extras importable.
        let ml = RetraceExtras {
            detection: true,
            ocr: true,
        };
        let (used, evidence) = derive_fallback("WARNING easyocr is not installed\n", ml);
        assert!(
            used,
            "easyocr imported but OCR still did not run — stderr is primary: {evidence}"
        );
        assert!(!derive_fallback("", ml).0, "both extras ran, no warning");
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

    /// `both_handlers_resolve_the_config_project_the_same_way` tests the
    /// resolver; this tests the two handlers that call it. `check_retrace`
    /// passed its raw `project_dir` argument through while `scan_pcb_photo`
    /// canonicalized first, so the probe could resolve a different project's
    /// config than the scan it is meant to diagnose — and a `project_dir`
    /// that is not there was a silent fall-through in one and an error in
    /// the other.
    #[tokio::test]
    async fn check_retrace_rejects_a_project_dir_the_scan_would_reject() {
        let project = tempfile::tempdir().expect("tempdir");
        let image = project.path().join("board.png");
        std::fs::write(&image, b"not really a png").expect("write image");
        let missing = project.path().join("no-such-project");
        let ctx = test_ctx();

        let checked =
            handle_check_retrace(&json!({ "project_dir": missing.to_string_lossy() }), &ctx)
                .await
                .expect("handler returns");
        let scanned = handle_scan_pcb_photo(
            &json!({
                "image_path": image.to_string_lossy(),
                "project_dir": missing.to_string_lossy(),
            }),
            &ctx,
        )
        .await
        .expect("handler returns");

        assert!(checked.is_error, "{}", response_text(&checked));
        assert!(scanned.is_error, "{}", response_text(&scanned));
        assert_eq!(
            response_text(&checked),
            response_text(&scanned),
            "both handlers must reject an unresolvable project_dir the same way"
        );
        assert!(
            response_text(&checked).contains("'project_dir' does not resolve"),
            "{}",
            response_text(&checked)
        );
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
                // Left unresolved on purpose: the pinned digest below is the
                // digest of a map written before this change, so the fixture
                // must keep carrying none of the fields this change adds.
                mm_per_px: None,
                evidence: None,
            },
            components,
            nets: vec![ReviewNet {
                connections: vec!["R1.1".to_string(), "R2.2".to_string()],
                source: "traced".to_string(),
            }],
            subcircuit_hints: Some(analysis.pattern_matches.clone()),
            dossier: None,
            design_brief: None,
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

    /// The open record's other half: a key the schema does not name is
    /// invisible to the gate *wherever* it sits, not only at the top level.
    ///
    /// The hash used to clone `components`, `nets` and `scale_reference`
    /// whole, so a reviewer note added to a component after the approval moved
    /// the digest and silently revoked it — the opposite of what design D8 and
    /// D16 promise about the three objects they declare open, and the one
    /// ordering `out_of_schema_keys_survive_a_save_and_load_round_trip` (which
    /// annotates before approving) cannot reach.
    #[test]
    fn unknown_keys_are_outside_the_content_hash_at_every_depth() {
        let map = fixture_review_map();
        let baseline = review_map_content_hash(&map);

        let annotations: Vec<MapEdit> = vec![
            (
                "a top-level note",
                Box::new(|map: &mut serde_json::Value| {
                    map["reviewer_note"] = json!("checked against the photo on 2026-05-01")
                }),
            ),
            (
                "a per-component note",
                Box::new(|map: &mut serde_json::Value| {
                    map["components"][0]["datasheet_url"] = json!("https://example.invalid/r.pdf")
                }),
            ),
            (
                "a per-net note",
                Box::new(|map: &mut serde_json::Value| {
                    map["nets"][0]["measured_with"] = json!("continuity tester")
                }),
            ),
            (
                "a scale_reference note",
                Box::new(|map: &mut serde_json::Value| {
                    map["scale_reference"]["measured_from"] = json!("the silkscreen outline")
                }),
            ),
        ];

        for (what, annotate) in annotations {
            let mut annotated = map.clone();
            annotate(&mut annotated);
            assert_eq!(
                review_map_content_hash(&annotated),
                baseline,
                "{what} must not move the content hash"
            );
        }
    }

    /// D1's `""` → `null` normalization happens before the hash, so the value
    /// retrace wrote and the value `save` persists are one value to the gate.
    /// Otherwise the save that normalizes would revoke the approval it had
    /// just matched.
    #[test]
    fn an_empty_string_hashes_as_the_null_save_writes_for_it() {
        let mut with_null = fixture_review_map();
        with_null["components"][0]["value"] = serde_json::Value::Null;
        with_null["components"][0]["footprint_suggestion"] = serde_json::Value::Null;
        let mut with_empty = fixture_review_map();
        with_empty["components"][0]["value"] = json!("");
        with_empty["components"][0]["footprint_suggestion"] = json!("");

        assert_eq!(
            review_map_content_hash(&with_null),
            review_map_content_hash(&with_empty),
            "\"\" and null are the same reading, so they must be the same digest"
        );
    }

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

    /// D16's trade-off, made into a test: a field added to the review map
    /// without a decision about the hash is an unguarded field. Splitting
    /// every schema key between the covered lists and the excluded list forces
    /// that decision.
    ///
    /// Read off `review_map_schema()` rather than off the fixture map, because
    /// design D6's two sections are exactly the keys a fixture written before
    /// them does *not* carry — and they are the keys whose coverage most needs
    /// guarding. The schema is also what a third section would have to be
    /// added to, so this is where that addition fails the suite.
    /// `the_map_schema_names_every_review_map_field_and_stays_open` keeps the
    /// schema and [`PhotoReviewMap`] from drifting apart underneath it.
    #[test]
    fn every_schema_key_is_either_hashed_or_deliberately_not() {
        let schema = review_map_schema();
        let schema_keys: std::collections::BTreeSet<&str> = schema["properties"]
            .as_object()
            .expect("the map schema declares properties")
            .keys()
            .map(String::as_str)
            .collect();
        let accounted: std::collections::BTreeSet<&str> = CONTENT_KEYS
            .iter()
            .chain(UNHASHED_KEYS.iter())
            .chain(OPTIONAL_CONTENT_KEYS.iter())
            .copied()
            .collect();
        assert_eq!(
            schema_keys, accounted,
            "every review-map key must be listed in CONTENT_KEYS, OPTIONAL_CONTENT_KEYS or \
             UNHASHED_KEYS"
        );

        // The same decision one level down: design D3's two additions to the
        // scale reference are conditionally covered, and nothing else may
        // appear there without joining one of the two lists.
        let scale_keys: std::collections::BTreeSet<&str> = schema["properties"]["scale_reference"]
            ["properties"]
            .as_object()
            .expect("the scale reference declares properties")
            .keys()
            .map(String::as_str)
            .collect();
        let scale_accounted: std::collections::BTreeSet<&str> = SCALE_REFERENCE_CONTENT_KEYS
            .iter()
            .chain(SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS.iter())
            .copied()
            .collect();
        assert_eq!(
            scale_keys, scale_accounted,
            "every scale_reference key must be listed in SCALE_REFERENCE_CONTENT_KEYS or \
             SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS"
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

    // ─── The two additive sections (tasks 2.1 and 2.4, design D4/D5/D6) ──────

    /// A dossier small enough to read in a diff but carrying one instance of
    /// every shape design D4 declares: a claim object with an evidence
    /// pointer, an array of objects, and a nested singleton object.
    fn sample_dossier() -> serde_json::Value {
        json!({
            "identity": {
                "summary": "24 V LED lamp module",
                "basis": "observed",
                "confidence": 0.95,
                "evidence": [
                    { "view": "boardA_zoom3.png", "rect_px": [30, 430, 180, 24], "note": "silkscreen identity line" }
                ]
            },
            "physical": {
                "board_size_px": [410, 445],
                "board_size_mm": null,
                "scale_status": "unresolved — no scale reference supplied",
                "mounting_holes": [{ "position_px": [18, 20], "role": "plated_corner", "count": 4 }],
                "connectors": [{ "type": "2-pin screw terminal", "edge": "bottom", "basis": "observed", "confidence": 0.9 }],
                "layers_visible": "top silkscreen + bottom copper only"
            },
            "component_survey": [
                {
                    "visual_class": "led_5mm_clear",
                    "count": 107,
                    "count_method": "blob_count_hsv",
                    "count_confidence": 0.8,
                    "count_alternatives": [{ "method": "hough_circles", "count": 63 }],
                    "locations": [{ "region": "full board", "view": "boardA_top.png", "count": 107 }],
                    "retrace_component_ids": []
                }
            ],
            "silkscreen_markings": [
                { "text": "24V", "location_px": [15, 410], "basis": "observed", "confidence": 0.9 }
            ],
            "topology_claims": [
                {
                    "claim_id": "string-length",
                    "question": "How many LEDs per series string?",
                    "basis": "inferred",
                    "hypotheses": [
                        { "label": "A", "description": "6 per string", "confidence": 0.6, "calculation": "6 x 2.1V = 12.6V", "assumptions": ["Vf 2.1V"] },
                        { "label": "B", "description": "8-10 per string", "confidence": 0.3, "calculation": null, "assumptions": [] }
                    ],
                    "resolution_path": "count LEDs along one serpentine trace"
                }
            ],
            "retrace_correlation": [
                { "component_id": "C0000", "bbox_px": [100, 120, 20, 20], "overlap_confidence": 0.7 }
            ],
            "photo_views_used": ["boardA_top.png"],
            "design_brief_seed": {
                "summary": "N strings of 6x 5mm LEDs + one series resistor each",
                "depends_on_open_questions": ["string-length"]
            },
            "open_questions": ["exact board size in mm — no scale reference supplied"]
        })
    }

    /// The design-brief counterpart, carrying the one field design D5 calls
    /// the schema's single forbidden act if faked: an unresolved BOM entry.
    fn sample_design_brief() -> serde_json::Value {
        json!({
            "derived_from_dossier": true,
            "block_diagram": [
                { "block": "led_string_1", "function": "6x LED series string", "inputs": ["24V_RAIL", "GND"], "outputs": [] }
            ],
            "circuits": [
                {
                    "block": "led_string_1",
                    "description": "6 LEDs in series + 1 series resistor",
                    "calculated_values": [
                        { "parameter": "R1", "value_ohms": 620, "formula": "(24V - 6*2.1V) / 0.02A", "assumptions": ["Vf=2.1V"] }
                    ],
                    "derating_notes": "0.25 W dissipated; use 1/2 W axial"
                }
            ],
            "bom": [
                {
                    "role": "LED (5mm, through-hole)",
                    "kicad_symbol": "Device:LED",
                    "kicad_footprint": "LED_THT:LED_D5.0mm",
                    "resolution_status": "resolved",
                    "search_terms_used": ["LED"],
                    "candidates": [],
                    "quantity": 107,
                    "source": "matched"
                }
            ],
            "physical_constraints": {
                "board_size_mm": null,
                "board_size_status": "pending scale resolution",
                "mounting_holes": [{ "position_mm": null, "position_px": [18, 20], "diameter_mm": 3.2 }],
                "enclosure": null,
                "max_component_height_mm": null,
                "connector_edges": [{ "edge": "bottom", "type": "2-pin screw terminal", "pitch_mm": 5.08 }],
                "user_facing_parts": [],
                "net_currents": [{ "net": "24V_RAIL", "continuous_a": 0.38, "basis": "inferred" }],
                "net_voltages": [{ "net": "24V_RAIL", "nominal_v": 24, "basis": "observed" }],
                "signal_speeds": [],
                "sensitive_nets": [],
                "layer_count": null,
                "stackup": null,
                "fabricator": null,
                "assembly_notes": null,
                "keep_outs": [],
                "unresolved": ["board_size_mm", "enclosure", "layer_count"]
            },
            "assumptions": ["Vf averaged at 2.1V"],
            "open_questions": ["string length not yet resolved"]
        })
    }

    fn map_with_both_sections(map_id: &str) -> serde_json::Value {
        let mut map = sample_map(map_id);
        map["dossier"] = sample_dossier();
        map["design_brief"] = sample_design_brief();
        map
    }

    /// Spec scenarios "a dossier persists like any other reviewed section" and
    /// "a design brief persists additively": both sections survive the
    /// validator-then-overlay write path byte for byte, nested arrays and
    /// evidence pointers included.
    #[tokio::test]
    async fn a_map_with_both_new_sections_round_trips_through_save_and_load() {
        let (project, _canonical) = scanned_project("both-sections");
        let map = map_with_both_sections("both-sections");
        save(project.path(), &map).await;

        let loaded = response_json(&load(project.path(), "both-sections").await);
        assert_eq!(loaded["map"]["dossier"], sample_dossier());
        assert_eq!(loaded["map"]["design_brief"], sample_design_brief());
    }

    /// Design D6: on disk a section is either an object or not there. A
    /// `null` is refused with a message that names the alternative, so the
    /// "absent vs null" ambiguity never reaches the hash.
    #[tokio::test]
    async fn save_rejects_a_new_section_that_is_present_but_not_an_object() {
        let (project, canonical) = scanned_project("not-an-object");

        for (key, value) in [
            ("dossier", serde_json::Value::Null),
            ("dossier", json!("a sentence")),
            ("design_brief", serde_json::Value::Null),
            ("design_brief", json!([])),
        ] {
            let mut map = sample_map("not-an-object");
            map[key] = value.clone();
            let message = response_text(&save(project.path(), &map).await);
            assert!(
                message.contains(key)
                    && message.contains("Omit")
                    && message.contains("Nothing was written"),
                "{key} = {value}: {message}"
            );
        }

        assert!(
            !map_file(&canonical, "not-an-object").exists(),
            "a rejected map must not leave a file behind"
        );
    }

    /// Design D7: the two sections are optional additions to an existing
    /// record, and every level a human hand-edits stays open.
    #[test]
    fn the_map_schema_declares_both_sections_optional_and_open() {
        let map_schema = schema_of("save_photo_review_map")["properties"]["map"].clone();
        let required = map_schema["required"].as_array().expect("required");

        for section in OPTIONAL_CONTENT_KEYS {
            assert!(
                !required.iter().any(|name| name == section),
                "{section} must stay optional — a map written before this change has none"
            );
            assert_eq!(
                map_schema["properties"][section]["additionalProperties"],
                json!(true),
                "{section} must stay open so hand annotations survive a save"
            );
        }

        // Design D7: every array of objects is declared without an `items`
        // subschema, so the open-record allowlist stays at its six paths
        // instead of growing an entry per element field.
        for (section, arrays) in [
            (
                "dossier",
                vec![
                    "component_survey",
                    "silkscreen_markings",
                    "topology_claims",
                    "retrace_correlation",
                ],
            ),
            ("design_brief", vec!["block_diagram", "circuits", "bom"]),
        ] {
            for array in arrays {
                let node = &map_schema["properties"][section]["properties"][array];
                assert_eq!(node["type"], json!("array"), "{section}.{array}");
                assert!(
                    node.get("items").is_none(),
                    "{section}.{array} must declare no items subschema (design D7)"
                );
            }
        }
    }

    /// Design D6, case (1): the first checkpoint. A map approved while it had
    /// no dossier is describing content the reviewer never saw once one
    /// arrives, so the approval must not survive it.
    #[tokio::test]
    async fn adding_a_dossier_to_an_approved_map_revokes_its_approval() {
        let (project, _canonical) = scanned_project("dossier-added");
        let map = sample_map("dossier-added");
        save(project.path(), &map).await;
        approve(project.path(), "dossier-added").await;
        assert_eq!(
            response_json(&load(project.path(), "dossier-added").await)["approval_valid"],
            json!(true),
            "the map is approved before the dossier arrives"
        );

        let mut with_dossier = map.clone();
        with_dossier["dossier"] = sample_dossier();
        assert_ne!(
            review_map_content_hash(&map),
            review_map_content_hash(&with_dossier),
            "a dossier must join the hashed bytes"
        );

        let saved = response_json(&save(project.path(), &with_dossier).await);
        assert_eq!(saved["approved"], json!(false));
        let loaded = response_json(&load(project.path(), "dossier-added").await);
        assert_eq!(loaded["approval_valid"], json!(false));
        assert_eq!(loaded["map"]["approved_at"], serde_json::Value::Null);
        assert_eq!(
            loaded["map"]["content_hash_at_approval"],
            serde_json::Value::Null
        );
    }

    /// Design D6, case (2): the second checkpoint, through the same
    /// mechanism. Re-approving the dossier does not pre-approve the design
    /// brief that follows it.
    #[tokio::test]
    async fn adding_a_design_brief_revokes_a_re_approved_dossier_only_map() {
        let (project, _canonical) = scanned_project("brief-added");
        let mut with_dossier = sample_map("brief-added");
        with_dossier["dossier"] = sample_dossier();
        save(project.path(), &with_dossier).await;
        approve(project.path(), "brief-added").await;
        assert_eq!(
            response_json(&load(project.path(), "brief-added").await)["approval_valid"],
            json!(true),
            "checkpoint one: the dossier is approved"
        );

        let mut with_brief = with_dossier.clone();
        with_brief["design_brief"] = sample_design_brief();
        assert_ne!(
            review_map_content_hash(&with_dossier),
            review_map_content_hash(&with_brief)
        );

        assert_eq!(
            response_json(&save(project.path(), &with_brief).await)["approved"],
            json!(false),
            "checkpoint two: the brief needs its own approval"
        );
        assert_eq!(
            response_json(&load(project.path(), "brief-added").await)["approval_valid"],
            json!(false)
        );
    }

    /// Design D6, case (3): the same conditional rule one level down. A scale
    /// the agent resolved is covered; one that was never resolved contributes
    /// exactly the two members it always did.
    #[test]
    fn resolving_a_scale_reference_changes_the_hash_and_leaving_it_unresolved_does_not() {
        let base = sample_map("scale");
        let baseline = review_map_content_hash(&base);

        for (what, field, value) in [
            ("mm_per_px", "mm_per_px", json!(0.24)),
            (
                "evidence",
                "evidence",
                json!("21px between M3 hole centers, 5mm pitch stated by the user"),
            ),
        ] {
            let mut resolved = base.clone();
            resolved["scale_reference"][field] = value;
            assert_ne!(
                review_map_content_hash(&resolved),
                baseline,
                "a scale reference gaining {what} must move the digest"
            );
        }

        // The other direction: an unresolved scale reference serializes to
        // exactly `{kind, value}` — no null members — so its contribution to
        // the digest is byte-identical to what this function produced before
        // design D3 added the pair.
        let unresolved = serde_json::to_value(ScaleReference {
            kind: "board_edge_mm".to_string(),
            value: "50".to_string(),
            mm_per_px: None,
            evidence: None,
        })
        .expect("serializes");
        assert_eq!(
            unresolved,
            json!({ "kind": "board_edge_mm", "value": "50" })
        );
        let mut through_the_struct = base.clone();
        through_the_struct["scale_reference"] = unresolved;
        assert_eq!(review_map_content_hash(&through_the_struct), baseline);
    }

    /// Design D6, case (4): a removal is a real removal. The record written is
    /// the overlay over the *incoming* map, not a merge onto the stored one,
    /// so a key the caller omits is genuinely gone afterwards — and the gate
    /// closes in that direction too.
    #[tokio::test]
    async fn removing_an_approved_dossier_drops_the_section_and_revokes_approval() {
        let (project, _canonical) = scanned_project("dossier-removed");
        let mut with_dossier = sample_map("dossier-removed");
        with_dossier["dossier"] = sample_dossier();
        save(project.path(), &with_dossier).await;
        approve(project.path(), "dossier-removed").await;

        assert_eq!(
            response_json(&save(project.path(), &sample_map("dossier-removed")).await)["approved"],
            json!(false)
        );
        let loaded = response_json(&load(project.path(), "dossier-removed").await);
        assert!(
            loaded["map"].get("dossier").is_none(),
            "the section is gone from the record, not left behind as null: {}",
            loaded["map"]
        );
        assert_eq!(loaded["approval_valid"], json!(false));
    }

    /// Design D6, case (5) — the `skip_serializing_if` guard, and the reason
    /// this change is safe to ship against maps already approved in the field.
    ///
    /// Without `skip_serializing_if = "Option::is_none"` on the four new
    /// fields, `serde_json::to_value(&parsed)` emits `"dossier": null`, the
    /// save path's overlay writes that null into every record, `map.get`
    /// answers `Some(Null)`, the conditional insert fires, and every
    /// pre-existing approval is revoked by a save that changed nothing.
    /// Mirrors `a_review_map_round_trips_and_tolerates_absent_subcircuit_hints`.
    #[tokio::test]
    async fn a_map_with_neither_new_section_survives_a_save_round_trip_unchanged() {
        let raw = sample_map("no-sections");
        let parsed: PhotoReviewMap = serde_json::from_value(raw.clone()).expect("parses");
        assert!(parsed.dossier.is_none() && parsed.design_brief.is_none());

        let reserialized = serde_json::to_value(&parsed).expect("serializes");
        for absent in OPTIONAL_CONTENT_KEYS {
            assert!(
                reserialized.get(absent).is_none(),
                "an absent {absent} must stay absent, never serialize as null"
            );
        }
        for absent in SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS {
            assert!(
                reserialized["scale_reference"].get(absent).is_none(),
                "an unresolved scale_reference.{absent} must stay absent"
            );
        }
        assert_eq!(
            review_map_content_hash(&reserialized),
            review_map_content_hash(&raw)
        );

        // And the property that actually matters in the field: approving a map
        // that has neither section, then re-saving exactly what `load` handed
        // back, keeps the approval.
        let (project, _canonical) = scanned_project("no-sections");
        save(project.path(), &raw).await;
        approve(project.path(), "no-sections").await;
        let loaded = response_json(&load(project.path(), "no-sections").await);
        assert_eq!(loaded["approval_valid"], json!(true));

        let resaved = response_json(&save(project.path(), &loaded["map"]).await);
        assert_eq!(
            resaved["approved"],
            json!(true),
            "re-saving an untouched map must not revoke its approval"
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
        // The fixture carries none of design D6's optional sections — that is
        // what keeps its pinned digest the digest of a pre-change map — so the
        // schema legitimately declares those two keys and the fixture does
        // not. Everything else must match key for key.
        let actual: std::collections::BTreeSet<String> = fixture_review_map()
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .chain(OPTIONAL_CONTENT_KEYS.iter().map(|key| (*key).to_string()))
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

/// `prepare_board_photo` (design D1 and D8).
#[cfg(test)]
mod board_view_tests {
    use super::test_support::{response_json, response_text, test_ctx};
    use super::*;

    /// A project with one scan directory already minted, exactly as
    /// `scan_pcb_photo` leaves it.
    fn scanned_project(map_id: &str) -> (tempfile::TempDir, PathBuf) {
        let project = tempfile::tempdir().expect("temp project");
        let canonical = project.path().canonicalize().expect("canonical project");
        prepare_map_dir(&canonical, map_id).expect("scan directory");
        (project, canonical)
    }

    fn views_dir(project_canonical: &Path, map_id: &str) -> PathBuf {
        project_canonical
            .join(".konnect")
            .join("photo_intake")
            .join(map_id)
            .join(VIEWS_DIR)
    }

    /// A `width` x `height` image whose top-left `block` x `block` corner is
    /// red and whose remainder is black. A crop that lands on the wrong corner
    /// then shows up as a colour rather than only as a size, which is what
    /// makes the orientation test below able to fail.
    fn corner_marked(width: u32, height: u32, block: u32) -> image::RgbImage {
        let mut image = image::RgbImage::from_pixel(width, height, image::Rgb([0, 0, 0]));
        for y in 0..block.min(height) {
            for x in 0..block.min(width) {
                image.put_pixel(x, y, image::Rgb([255, 0, 0]));
            }
        }
        image
    }

    fn encode(image: &image::RgbImage, format: image::ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image.clone())
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .expect("encodes");
        bytes
    }

    fn write_source(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("writes the source photo");
        path
    }

    /// PNG's CRC-32 (ISO-HDLC, reflected polynomial), needed to splice a chunk
    /// into an encoded PNG. Hand-rolled rather than pulled in as a dependency:
    /// eight lines beat a crate for two test fixtures.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// Splice an `eXIf` chunk carrying one EXIF orientation entry in right
    /// after IHDR. The `png` crate parses `eXIf` into `info().exif_metadata`
    /// verbatim, and that is exactly the raw little-endian TIFF block
    /// `Orientation::from_exif_chunk` reads — so this produces a file the
    /// production decode path sees an orientation tag on, with no checked-in
    /// photo and no camera.
    fn with_exif_orientation(png: &[u8], exif_orientation: u8) -> Vec<u8> {
        let mut tiff = vec![
            0x49, 0x49, 0x2A, 0x00, // little-endian TIFF magic
            0x08, 0x00, 0x00, 0x00, // IFD0 begins at offset 8
            0x01, 0x00, // one entry
            0x12, 0x01, // tag 0x0112, Orientation
            0x03, 0x00, // type SHORT
            0x01, 0x00, 0x00, 0x00, // count 1
        ];
        tiff.extend_from_slice(&[exif_orientation, 0x00, 0x00, 0x00]); // value, padded
        tiff.extend_from_slice(&[0x00; 4]); // no next IFD

        let mut chunk = (tiff.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"eXIf");
        chunk.extend_from_slice(&tiff);
        let crc = crc32(&chunk[4..]);
        chunk.extend_from_slice(&crc.to_be_bytes());

        // 8-byte signature followed by a 25-byte IHDR chunk.
        const AFTER_IHDR: usize = 8 + 25;
        let mut spliced = png[..AFTER_IHDR].to_vec();
        spliced.extend_from_slice(&chunk);
        spliced.extend_from_slice(&png[AFTER_IHDR..]);
        spliced
    }

    /// A real PNG whose IHDR is rewritten to claim a size its pixel data does
    /// not have. Nothing ever decodes it: `decoder.dimensions()` reads the
    /// header, which is exactly where design D1 puts the megapixel check.
    fn png_claiming(png: &[u8], width: u32, height: u32) -> Vec<u8> {
        let mut patched = png.to_vec();
        patched[16..20].copy_from_slice(&width.to_be_bytes());
        patched[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(&patched[12..29]); // chunk type plus its 13 data bytes
        patched[29..33].copy_from_slice(&crc.to_be_bytes());
        patched
    }

    async fn prepare(args: serde_json::Value) -> CallToolResult {
        handle_prepare_board_photo(&args, &test_ctx())
            .await
            .expect("handler returns a result")
    }

    fn is_red(path: &Path) -> bool {
        let view = image::open(path).expect("the view decodes").to_rgb8();
        view.pixels().all(|pixel| pixel.0 == [255, 0, 0])
    }

    // ─── Schema (task 1.2, design D1) ────────────────────────────────────────

    #[test]
    fn the_view_schema_requires_the_three_paths_and_computes_the_rest() {
        let schema = tools()
            .into_iter()
            .find(|tool| tool.name == "prepare_board_photo")
            .expect("prepare_board_photo is defined")
            .input_schema;

        let mut names: Vec<&str> = schema["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "crop",
                "image_path",
                "label",
                "map_id",
                "project_dir",
                "rotate",
                "scale"
            ],
            "the output location is computed, never an argument"
        );
        assert_eq!(
            schema["required"],
            json!(["image_path", "project_dir", "map_id"])
        );
        for token in ["map_id", "label"] {
            assert_eq!(
                schema["properties"][token]["pattern"],
                json!(MAP_ID_PATTERN),
                "{token} is a path component, so the token rule is in the schema too"
            );
        }
        assert_eq!(
            schema["properties"]["rotate"]["enum"],
            json!([0, 90, 180, 270])
        );
        assert_eq!(
            schema["properties"]["scale"]["minimum"],
            json!(MIN_VIEW_SCALE)
        );
        assert_eq!(
            schema["properties"]["scale"]["maximum"],
            json!(MAX_VIEW_SCALE)
        );
    }

    // ─── Pipeline (task 1.3, design D1 steps 1-9) ────────────────────────────

    /// Spec scenario "a cropped, scaled view is produced": what the response
    /// says the view is must be what is on disk, or an agent reasoning about
    /// pixel coordinates is reasoning about a space that does not exist.
    #[tokio::test]
    async fn a_cropped_and_scaled_view_is_saved_at_exactly_the_size_it_reports() {
        let (project, canonical) = scanned_project("cropped");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(40, 20, 10), image::ImageFormat::Png),
        );

        let result = response_json(
            &prepare(json!({
                "image_path": source.to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
                "map_id": "cropped",
                "crop": { "x": 0, "y": 0, "w": 10, "h": 10 },
                "scale": 2.0,
                "label": "boardA_zoom"
            }))
            .await,
        );

        assert_eq!(result["source_size_px"], json!([40, 20]));
        assert_eq!(result["source_rect_px"], json!([0, 0, 10, 10]));
        assert_eq!(result["output_size_px"], json!([20, 20]));
        assert_eq!(result["exif_orientation"], json!("NoTransforms"));

        let view = views_dir(&canonical, "cropped").join("boardA_zoom.png");
        assert!(view.exists(), "the view is written where the response says");
        let decoded = image::open(&view).expect("the view decodes");
        assert_eq!(
            (decoded.width(), decoded.height()),
            (20, 20),
            "the saved PNG's size is output_size_px, not an aspect-ratio rounding of it"
        );
        assert!(is_red(&view), "the crop landed on the marked corner");
    }

    /// Round-1 review MINOR-4: a reused `label` (or a colliding auto-assigned
    /// number) must never silently replace a view. An approved dossier's
    /// `{view, rect_px}` evidence pointer relies on the file it names staying
    /// what it was when the pointer was written.
    #[tokio::test]
    async fn a_reused_label_is_refused_and_the_original_view_is_unchanged() {
        let (project, canonical) = scanned_project("reused");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(40, 20, 10), image::ImageFormat::Png),
        );
        let base = json!({
            "image_path": source.to_string_lossy(),
            "project_dir": project.path().to_string_lossy(),
            "map_id": "reused",
            "crop": { "x": 0, "y": 0, "w": 10, "h": 10 },
            "label": "boardA_zoom"
        });

        prepare(base.clone()).await;
        let view = views_dir(&canonical, "reused").join("boardA_zoom.png");
        let original_bytes = std::fs::read(&view).expect("the first view was written");

        // Same label, a different crop this time — a naive re-save would
        // silently swap the file's meaning under an unchanged evidence
        // pointer.
        let mut second = base.clone();
        second["crop"] = json!({ "x": 10, "y": 0, "w": 10, "h": 10 });
        let result = prepare(second).await;

        assert!(result.is_error, "{}", response_text(&result));
        assert!(
            response_text(&result).contains(&view.display().to_string()),
            "the error names the path that already exists: {}",
            response_text(&result)
        );

        let bytes_after = std::fs::read(&view).expect("the original view still exists");
        assert_eq!(
            original_bytes, bytes_after,
            "a refused overwrite must not touch the file already on disk"
        );
    }

    /// The whole reason design D1 orients before it crops: a phone original
    /// carries the tag, the viewer the agent picked its rectangle in may or
    /// may not have honoured it, and the agent cannot tell from the result.
    ///
    /// The source is 40x20 with a red top-left corner and an EXIF orientation
    /// of 6 (`Rotate90`). Oriented, it is 20x40 with the red corner at the
    /// top *right* — so a crop at `[10, 0, 10, 10]` is all red and one at
    /// `[0, 0, 10, 10]` is not. Skip `apply_orientation` and both assertions
    /// invert.
    #[tokio::test]
    async fn exif_orientation_is_applied_before_the_crop_is_measured() {
        let (project, canonical) = scanned_project("oriented");
        let source = write_source(
            project.path(),
            "rotated.png",
            &with_exif_orientation(
                &encode(&corner_marked(40, 20, 10), image::ImageFormat::Png),
                6,
            ),
        );
        let base = json!({
            "image_path": source.to_string_lossy(),
            "project_dir": project.path().to_string_lossy(),
            "map_id": "oriented",
        });

        let mut whole = base.clone();
        whole["label"] = json!("whole");
        let result = response_json(&prepare(whole).await);
        assert_eq!(
            result["exif_orientation"],
            json!("Rotate90"),
            "the applied orientation is reported, so the coordinate space is inspectable"
        );
        assert_eq!(
            result["source_size_px"],
            json!([20, 40]),
            "source_size_px is the oriented size, not the stored one"
        );
        assert_eq!(result["source_rect_px"], json!([0, 0, 20, 40]));

        for (label, rect, expect_red) in [
            ("marked", json!({ "x": 10, "y": 0, "w": 10, "h": 10 }), true),
            ("blank", json!({ "x": 0, "y": 0, "w": 10, "h": 10 }), false),
        ] {
            let mut args = base.clone();
            args["label"] = json!(label);
            args["crop"] = rect;
            prepare(args).await;
            assert_eq!(
                is_red(&views_dir(&canonical, "oriented").join(format!("{label}.png"))),
                expect_red,
                "the {label} crop is measured in the oriented space"
            );
        }
    }

    /// The `jpeg` feature design D2 adds, exercised: the reference board
    /// photos are WhatsApp JPEGs, and a decoder that is not compiled in is a
    /// tool that cannot look at them.
    #[tokio::test]
    async fn a_jpeg_photo_goes_through_the_same_pipeline() {
        let (project, canonical) = scanned_project("jpeg");
        let source = write_source(
            project.path(),
            "boardA.jpeg",
            &encode(&corner_marked(64, 32, 16), image::ImageFormat::Jpeg),
        );

        let result = response_json(
            &prepare(json!({
                "image_path": source.to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
                "map_id": "jpeg",
                "label": "from_jpeg"
            }))
            .await,
        );
        assert_eq!(result["source_size_px"], json!([64, 32]));
        assert_eq!(result["output_size_px"], json!([64, 32]));

        let view = views_dir(&canonical, "jpeg").join("from_jpeg.png");
        let decoded = image::open(&view).expect("the view decodes");
        assert_eq!((decoded.width(), decoded.height()), (64, 32));
    }

    /// Design D1's naming rule: `label` when given, else `1 +` whatever the
    /// directory already holds.
    #[tokio::test]
    async fn unlabelled_views_are_numbered_from_what_the_directory_already_holds() {
        let (project, canonical) = scanned_project("numbered");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(16, 16, 4), image::ImageFormat::Png),
        );
        let args = json!({
            "image_path": source.to_string_lossy(),
            "project_dir": project.path().to_string_lossy(),
            "map_id": "numbered",
        });

        for expected in ["1.png", "2.png", "3.png"] {
            let result = response_json(&prepare(args.clone()).await);
            assert!(
                result["view_path"]
                    .as_str()
                    .expect("view_path")
                    .ends_with(expected),
                "expected {expected}, got {}",
                result["view_path"]
            );
            assert!(views_dir(&canonical, "numbered").join(expected).exists());
        }
    }

    // ─── Path rule (task 1.4, design D8) ─────────────────────────────────────

    /// Spec scenario "a map directory that does not exist is rejected": a map
    /// id is server-assigned, so naming one that is not there names a map that
    /// does not exist — never a directory to mint on the caller's say-so.
    #[tokio::test]
    async fn a_map_id_no_scan_ever_assigned_is_rejected_and_mints_nothing() {
        let (project, canonical) = scanned_project("real-map");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(16, 16, 4), image::ImageFormat::Png),
        );

        let message = response_text(
            &prepare(json!({
                "image_path": source.to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
                "map_id": "invented-map",
            }))
            .await,
        );
        assert!(message.contains("scan_pcb_photo"), "{message}");
        assert!(
            !canonical
                .join(".konnect")
                .join("photo_intake")
                .join("invented-map")
                .exists(),
            "no directory is minted for an unknown map id"
        );
        assert!(
            !views_dir(&canonical, "real-map").exists(),
            "and nothing is created under the map that does exist either"
        );
    }

    #[tokio::test]
    async fn a_label_that_is_not_a_token_is_rejected_before_any_path_join() {
        let (project, canonical) = scanned_project("labelled");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(16, 16, 4), image::ImageFormat::Png),
        );

        for label in ["..", "../escape", "views/../..", "a\\b", "a:b", ""] {
            let message = response_text(
                &prepare(json!({
                    "image_path": source.to_string_lossy(),
                    "project_dir": project.path().to_string_lossy(),
                    "map_id": "labelled",
                    "label": label,
                }))
                .await,
            );
            assert!(
                message.contains("'label'") && message.contains("No file was read or written"),
                "{label:?}: {message}"
            );
        }
        assert!(
            !views_dir(&canonical, "labelled").exists(),
            "a rejected label must not even create the views directory"
        );
    }

    // ─── mm_per_px and the four bounds (task 1.5, design D1 and D3) ──────────

    /// Spec scenario "a view carries a physical scale when one is resolved",
    /// both halves. The tool never estimates one: absent means absent.
    #[tokio::test]
    async fn mm_per_px_is_reported_only_when_the_saved_map_resolved_one() {
        let (project, _canonical) = scanned_project("scaled");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(16, 16, 4), image::ImageFormat::Png),
        );
        let args = json!({
            "image_path": source.to_string_lossy(),
            "project_dir": project.path().to_string_lossy(),
            "map_id": "scaled",
        });

        // (a) No map on disk at all.
        let result = response_json(&prepare(args.clone()).await);
        assert!(result.get("mm_per_px").is_none(), "{result}");

        let mut map = json!({
            "map_id": "scaled",
            "source_images": [source.to_string_lossy()],
            "scale_reference": { "kind": "board_edge_mm", "value": "50" },
            "components": [],
            "nets": []
        });
        let project_dir = project.path().to_path_buf();
        let save = |map: serde_json::Value| {
            let project_dir = project_dir.clone();
            async move {
                handle_save_photo_review_map(
                    &json!({ "project_dir": project_dir.to_string_lossy(), "map": map }),
                    &test_ctx(),
                )
                .await
                .expect("save returns a result")
            }
        };

        // (b) A map whose scale was never resolved.
        save(map.clone()).await;
        let result = response_json(&prepare(args.clone()).await);
        assert!(result.get("mm_per_px").is_none(), "{result}");

        // (c) One the agent resolved, with its evidence.
        map["scale_reference"]["mm_per_px"] = json!(0.24);
        map["scale_reference"]["evidence"] =
            json!("21px between M3 hole centers, 5mm pitch stated by the user");
        save(map).await;
        let result = response_json(&prepare(args).await);
        assert_eq!(result["mm_per_px"], json!(0.24));
    }

    /// Spec scenario "an out-of-bounds crop is rejected": the error names the
    /// size the tool actually saw, because the size the caller assumed is the
    /// thing that was wrong.
    #[tokio::test]
    async fn an_out_of_bounds_crop_names_the_oriented_dimensions_and_writes_nothing() {
        let (project, canonical) = scanned_project("out-of-bounds");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(40, 20, 10), image::ImageFormat::Png),
        );

        let message = response_text(
            &prepare(json!({
                "image_path": source.to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
                "map_id": "out-of-bounds",
                "crop": { "x": 30, "y": 0, "w": 20, "h": 10 },
            }))
            .await,
        );
        assert!(message.contains("40x20"), "{message}");
        assert!(message.contains("No file was written"), "{message}");
        assert!(!views_dir(&canonical, "out-of-bounds").exists());
    }

    #[tokio::test]
    async fn a_photo_over_the_megapixel_cap_is_rejected_before_it_is_decoded() {
        let (project, canonical) = scanned_project("too-many-pixels");
        let source = write_source(
            project.path(),
            "huge.png",
            // A 16x16 file whose header claims 65535 x 65535 — the crafted
            // case cause 7 of the pre-mortem names. Its pixel data is never
            // touched, which is the point.
            &png_claiming(
                &encode(&corner_marked(16, 16, 4), image::ImageFormat::Png),
                65_535,
                65_535,
            ),
        );

        let message = response_text(
            &prepare(json!({
                "image_path": source.to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
                "map_id": "too-many-pixels",
            }))
            .await,
        );
        assert!(message.contains("65535x65535"), "{message}");
        assert!(
            message.contains(&MAX_SOURCE_MEGAPIXELS.to_string()),
            "the cap is named too: {message}"
        );
        assert!(!views_dir(&canonical, "too-many-pixels").exists());
    }

    #[tokio::test]
    async fn a_scale_outside_the_range_is_rejected_rather_than_clamped() {
        let (project, canonical) = scanned_project("bad-scale");
        let source = write_source(
            project.path(),
            "boardA.png",
            &encode(&corner_marked(16, 16, 4), image::ImageFormat::Png),
        );

        for (scale, actual) in [(5.0, "5"), (0.1, "0.1")] {
            let message = response_text(
                &prepare(json!({
                    "image_path": source.to_string_lossy(),
                    "project_dir": project.path().to_string_lossy(),
                    "map_id": "bad-scale",
                    "scale": scale,
                }))
                .await,
            );
            assert!(message.contains(actual), "{message}");
            assert!(
                message.contains(&MIN_VIEW_SCALE.to_string())
                    && message.contains(&MAX_VIEW_SCALE.to_string()),
                "both ends of the range are named: {message}"
            );
            assert!(
                message.contains("rejected rather than clamped"),
                "{message}"
            );
        }
        assert!(!views_dir(&canonical, "bad-scale").exists());
    }

    #[tokio::test]
    async fn a_view_longer_than_the_side_cap_is_rejected_before_it_is_allocated() {
        let (project, canonical) = scanned_project("too-wide");
        let source = write_source(
            project.path(),
            "wide.png",
            &encode(&corner_marked(1100, 8, 4), image::ImageFormat::Png),
        );

        let message = response_text(
            &prepare(json!({
                "image_path": source.to_string_lossy(),
                "project_dir": project.path().to_string_lossy(),
                "map_id": "too-wide",
                "scale": 4.0,
            }))
            .await,
        );
        assert!(message.contains("4400x32"), "{message}");
        assert!(
            message.contains(&MAX_VIEW_SIDE_PX.to_string()),
            "the cap is named too: {message}"
        );
        assert!(!views_dir(&canonical, "too-wide").exists());
    }
}

/// The synthetic board the live tests scan. Drawn rather than checked in: no
/// redistributable real-photo fixture exists, and a drawn board keeps the test
/// honest about what it proves (plumbing, not recognition accuracy).
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

        // The interpreter that actually ran, in the scan's own result: a
        // resolution that fell past the one the caller named has to be
        // visible here, not inferred from a separate check_retrace call.
        let reported = payload["python_path"].as_str().expect("python_path");
        assert!(
            Path::new(reported).is_file(),
            "the scan must name the real interpreter it ran: {payload}"
        );
        assert_eq!(
            Path::new(reported).file_name(),
            Path::new(&python).file_name(),
            "the scan ran a different interpreter than RETRACE_PYTHON named: {payload}"
        );
        assert!(
            payload["candidates_tried"].is_array(),
            "candidates_tried is reported whether or not anything was rejected: {payload}"
        );

        // Design D11's derivation, asserted against all four raw signals.
        let evidence = &payload["fallback_evidence"];
        let yolo = evidence["yolo_warning_seen"].as_bool().expect("yolo flag");
        let ocr_warning = evidence["ocr_warning_seen"].as_bool().expect("ocr warning");
        let detection = evidence["extras_detection"]
            .as_bool()
            .expect("detection flag");
        let ocr = evidence["extras_ocr"].as_bool().expect("ocr extra");
        assert_eq!(
            payload["used_fallback"].as_bool().expect("used_fallback"),
            yolo || ocr_warning || !detection || !ocr,
            "used_fallback must be stderr OR'd with both halves of the extras probe: {payload}"
        );

        // D11's stderr literal is an unversioned contract, so pin it against
        // a real run rather than re-deriving it from the response's own
        // fields: on a machine without the detection extra retrace must still
        // print the marker `derive_fallback` greps for. When retrace rewords
        // it, this assertion is what says so.
        if !detection {
            assert!(
                yolo,
                "retrace ran without the detection extra and did not print \
                 'YOLO not available' — the stderr contract moved, and \
                 used_fallback now rests on the probe alone: {payload}"
            );
        }

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
