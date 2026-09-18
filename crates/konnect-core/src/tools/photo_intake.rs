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

// ─── Tool definitions ─────────────────────────────────────────────────────────

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
        assert_eq!(names, vec!["check_retrace", "scan_pcb_photo"]);
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
