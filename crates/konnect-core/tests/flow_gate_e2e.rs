//! End-to-end exercise of the `flow` gate chain through the same surface an
//! MCP client reaches: the toolset is loaded through `ToolRouter`, every call
//! is validated against the tool's own compiled input schema, and the handler
//! is invoked through the `ToolDef` the router handed back.
//!
//! The unit tests inside `tools/flow.rs` call each handler directly. What they
//! cannot show is that the published schemas admit the calls the chain needs
//! (an uncallable schema fails here, at the validator, before any handler
//! runs) and that the six tools compose: that the `job_id` `flow_start` mints
//! is the one the others accept, that an approval binds to the design and the
//! records a human was shown, and that a rewind always re-asks.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ─── Harness ──────────────────────────────────────────────────────────────────

/// Load `flow` the way an LLM does and keep the router around, so the tools
/// under test are the ones the registry actually publishes.
async fn loaded_toolset() -> (Arc<ToolContext>, Vec<ToolDef>) {
    let router = Arc::new(ToolRouter::new());
    let defs = router
        .load("flow")
        .await
        .expect("flow is a registered toolset");
    let ctx = Arc::new(ToolContext::new(ServerConfig::default(), router));
    (ctx, defs)
}

/// Validate against the tool's own compiled schema, then dispatch. Validating
/// first matters: a test that only calls the handler can pass on arguments the
/// server would have rejected before the handler ever ran.
async fn call(ctx: &Arc<ToolContext>, defs: &[ToolDef], name: &str, args: Value) -> CallToolResult {
    let def = defs
        .iter()
        .find(|def| def.name == name)
        .unwrap_or_else(|| panic!("{name} is in the flow toolset"));
    if let Err(error) = def.input_validator.validate(&args) {
        panic!("{name} rejected its own arguments at the schema: {error}");
    }
    (def.handler)(&args, ctx.clone())
        .await
        .unwrap_or_else(|error| panic!("{name} handler returned Err: {error}"))
}

fn text(result: &CallToolResult) -> String {
    match &result.content[0] {
        ToolContent::Text { text } => text.clone(),
        other => panic!("expected text content, got {other:?}"),
    }
}

fn payload(result: &CallToolResult) -> Value {
    serde_json::from_str(&text(result))
        .unwrap_or_else(|_| panic!("expected a JSON payload, got {}", text(result)))
}

/// Dispatch and insist the call succeeded, returning its JSON payload.
async fn ok(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    name: &str,
    args: Value,
    what: &str,
) -> Value {
    let result = call(ctx, defs, name, args).await;
    assert!(!result.is_error, "{what} failed: {}", text(&result));
    payload(&result)
}

/// Dispatch and insist the call was refused as `stale_target`, returning the
/// refusal's message.
async fn refused_stale(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    name: &str,
    args: Value,
    what: &str,
) -> String {
    let result = call(ctx, defs, name, args).await;
    assert!(result.is_error, "{what} must be refused: {}", text(&result));
    let body = payload(&result);
    assert_eq!(body["error"]["kind"], "stale_target", "{what}: {body}");
    body["message"]
        .as_str()
        .expect("refusal message")
        .to_string()
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const PROJECT_FILE: &str = "demo.kicad_pro";
const ARCHITECTURE: &str = "# Architecture\n\nUSB-C in, 3V3 LDO, one MCU.\n\nReadiness: PASS\n";

/// A KiCad project directory: a project file and one schematic.
fn project() -> (tempfile::TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical project");
    std::fs::write(path.join(PROJECT_FILE), "{\"version\": 1}\n").expect("write project");
    std::fs::write(path.join("demo.kicad_sch"), "(kicad_sch)\n").expect("write schematic");
    let arg = path.to_string_lossy().to_string();
    (dir, path, arg)
}

fn record(filename: &str, content: &str) -> Value {
    json!({ "filename": filename, "content": content })
}

/// The three records `architecture` must supply when it is left forward.
fn architecture_records(architecture: &str) -> Value {
    json!([
        record("architecture.md", architecture),
        record(
            "worst-case.md",
            "# Worst case\n\nLDO dissipation 0.4 W at 5.5 V in.\n"
        ),
        record("pin-plan.md", "# Pin plan\n\nPA9 USART1_TX.\n"),
    ])
}

fn record_path(project: &Path, filename: &str) -> PathBuf {
    project
        .join(".konnect")
        .join("flow")
        .join("records")
        .join(filename)
}

/// `flow_start` → requirements → architecture → `gate:architecture`, every
/// step through the published schemas. Returns the job id and the payload of
/// the move into the gate.
async fn open_and_reach_the_architecture_gate(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
) -> (String, Value) {
    let started = ok(
        ctx,
        defs,
        "flow_start",
        json!({
            "project_dir": project_arg,
            "objective": "USB sensor board",
            "lane": "new_board",
        }),
        "flow_start",
    )
    .await;
    assert_eq!(started["phase"], "requirements", "{started}");
    let job_id = started["job_id"].as_str().expect("job_id").to_string();

    let to_architecture = ok(
        ctx,
        defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": "architecture",
            "records": [record("constraints.md", "# Constraints\n\nUSB-C, 50x30 mm.\n")],
        }),
        "leave requirements",
    )
    .await;
    assert_eq!(to_architecture["phase"], "architecture");

    let entered = ok(
        ctx,
        defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": "gate:architecture",
            "records": architecture_records(ARCHITECTURE),
        }),
        "leave architecture with Readiness: PASS",
    )
    .await;
    assert_eq!(entered["phase"], "gate:architecture", "{entered}");
    assert_eq!(entered["transition"], "advance");
    (job_id, entered)
}

async fn approve_architecture(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
    job_id: &str,
    what: &str,
) -> Value {
    let approved = ok(
        ctx,
        defs,
        "flow_gate",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "gate_name": "architecture",
            "decision": "approve",
            "summary": "Showed the block diagram, the power budget and the pin plan.",
            "user_words": "Looks right, go ahead with the schematic.",
        }),
        what,
    )
    .await;
    assert_eq!(approved["decision"], "approve", "{approved}");
    assert_eq!(approved["approved_by"], "user", "{approved}");
    assert_eq!(
        approved["phase"], "gate:architecture",
        "a gate decision never moves the phase"
    );
    approved
}

fn leave_gate_args(project_arg: &str, job_id: &str) -> Value {
    json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": "schematic" })
}

fn rewind_args(project_arg: &str, job_id: &str, reason: &str) -> Value {
    json!({
        "project_dir": project_arg,
        "job_id": job_id,
        "to_phase": "architecture",
        "reason": reason,
    })
}

async fn status(ctx: &Arc<ToolContext>, defs: &[ToolDef], project_arg: &str) -> Value {
    ok(
        ctx,
        defs,
        "flow_status",
        json!({ "project_dir": project_arg }),
        "flow_status",
    )
    .await
}

// ─── The published contract ───────────────────────────────────────────────────

/// The harness's own guarantee: the loaded schemas are closed and enforce
/// their required fields, so `call` really does stand between a malformed
/// argument set and the handler.
#[tokio::test]
async fn the_loaded_schemas_refuse_what_the_server_would_refuse() {
    let (_ctx, defs) = loaded_toolset().await;
    let names: Vec<&str> = defs.iter().map(|def| def.name).collect();
    assert_eq!(
        names,
        [
            "flow_status",
            "flow_start",
            "flow_advance",
            "flow_gate",
            "flow_log",
            "flow_defer",
        ]
    );
    let schema = |name: &str| {
        defs.iter()
            .find(|def| def.name == name)
            .expect("tool is loaded")
            .input_validator
            .clone()
    };

    let gate = schema("flow_gate");
    let approval = json!({
        "project_dir": ".",
        "job_id": "demo-20260921-140000",
        "gate_name": "architecture",
        "decision": "approve",
        "summary": "Shown and decided.",
        "user_words": "",
    });
    assert!(
        gate.validate(&approval).is_ok(),
        "empty user_words is a legal shape"
    );
    let mut without_words = approval.clone();
    without_words.as_object_mut().unwrap().remove("user_words");
    assert!(
        gate.validate(&without_words).is_err(),
        "user_words is required"
    );
    let mut forged = approval.clone();
    forged["approved_by"] = json!("user");
    assert!(
        gate.validate(&forged).is_err(),
        "flow_gate's record is closed"
    );

    let advance = schema("flow_advance");
    let with_extra_record_key = json!({
        "project_dir": ".",
        "job_id": "demo-20260921-140000",
        "to_phase": "gate:architecture",
        "records": [{ "filename": "architecture.md", "content": "x", "sha256": "0" }],
    });
    assert!(
        advance.validate(&with_extra_record_key).is_err(),
        "a record item is closed too"
    );
}

/// Reviewer 11's Scenario A through the published `flow_start` schema: a job
/// that holds `gate:purchase` without `manufacturing` would bind the purchase
/// approval to whatever `manufacturing.md` an earlier job left on disk, so it
/// is refused before anything is created.
#[tokio::test]
async fn a_gate_without_its_phase_is_refused_through_flow_start() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();

    let result = call(
        &ctx,
        &defs,
        "flow_start",
        json!({
            "project_dir": project_arg,
            "objective": "Re-route after the board changed",
            "lane": "board_revision",
            "phases": ["routing", "prefab_review", "gate:purchase"],
        }),
    )
    .await;
    assert!(
        result.is_error,
        "a purchase gate without manufacturing must be refused: {}",
        text(&result)
    );
    let body = payload(&result);
    assert_eq!(body["error"]["kind"], "invalid_argument", "{body}");
    assert_eq!(body["error"]["field"], "phases", "{body}");
    let message = body["message"].as_str().expect("refusal message");
    assert!(
        message.contains("\"gate:purchase\"") && message.contains("manufacturing"),
        "the refusal names the gate and its missing phase: {message}"
    );
    assert!(
        !project_dir.join(".konnect").join("flow").exists(),
        "a refused start creates no flow directory"
    );
}

/// start → requirements → architecture (`PASS`) → approve → a `.kicad_pro`
/// byte changes → leaving is refused → rewind to `architecture`, re-advance,
/// re-approve → leaving succeeds.
#[tokio::test]
async fn a_design_change_after_approval_forces_a_rewind_and_a_new_approval() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();

    let (job_id, entered) = open_and_reach_the_architecture_gate(&ctx, &defs, &project_arg).await;
    let first = approve_architecture(&ctx, &defs, &project_arg, &job_id, "first approval").await;
    assert_eq!(first["visit"], entered["history_index"], "{first}");
    assert_eq!(first["design_hash_at_approval"], entered["design_hash"]);

    // One byte of the project file changes after the human said yes.
    let project_file = project_dir.join(PROJECT_FILE);
    std::fs::write(&project_file, "{\"version\": 2}\n").expect("edit project");

    let message = refused_stale(
        &ctx,
        &defs,
        "flow_advance",
        leave_gate_args(&project_arg, &job_id),
        "leaving the gate after a design change",
    )
    .await;
    assert!(
        message.contains("the design changed after gate \"architecture\" was approved"),
        "{message}"
    );
    let unchanged = status(&ctx, &defs, &project_arg).await;
    assert_eq!(
        unchanged["phase"], "gate:architecture",
        "a refusal writes nothing"
    );
    assert_eq!(
        unchanged["gate_approvals"]["architecture"]["valid"],
        json!(false),
        "{unchanged}"
    );

    // Rewind: the approval goes with it.
    let rewound = ok(
        &ctx,
        &defs,
        "flow_advance",
        rewind_args(
            &project_arg,
            &job_id,
            "the project file changed after approval",
        ),
        "rewind to architecture",
    )
    .await;
    assert_eq!(rewound["transition"], "rewind", "{rewound}");
    assert_eq!(rewound["phase"], "architecture");
    assert_eq!(rewound["cleared_approvals"], json!(["architecture"]));

    // Re-advance with the same records: the gate entry now carries the new
    // design hash.
    let re_entered = ok(
        &ctx,
        &defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": "gate:architecture",
            "records": architecture_records(ARCHITECTURE),
        }),
        "re-advance into the gate",
    )
    .await;
    assert_ne!(re_entered["design_hash"], entered["design_hash"]);
    assert_eq!(
        re_entered["package_hash"], entered["package_hash"],
        "same records, same package"
    );

    let second = approve_architecture(&ctx, &defs, &project_arg, &job_id, "re-approval").await;
    assert_eq!(second["visit"], re_entered["history_index"], "{second}");
    assert_ne!(second["visit"], first["visit"]);
    assert_eq!(second["design_hash_at_approval"], re_entered["design_hash"]);

    let left = ok(
        &ctx,
        &defs,
        "flow_advance",
        leave_gate_args(&project_arg, &job_id),
        "leaving the gate after the re-approval",
    )
    .await;
    assert_eq!(left["phase"], "schematic", "{left}");
    assert_eq!(left["from"], "gate:architecture");
    assert_eq!(
        status(&ctx, &defs, &project_arg).await["phase"],
        "schematic"
    );
}

/// The package case: the design never changes, only `architecture.md` does.
/// After a rewind, re-advancing a changed record leaves the earlier approval
/// unusable — leaving the gate needs an approval of what is on disk now, and
/// that approval is in turn revoked by any later edit to the records.
#[tokio::test]
async fn a_changed_architecture_after_a_rewind_leaves_the_earlier_approval_unusable() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();

    let (job_id, entered) = open_and_reach_the_architecture_gate(&ctx, &defs, &project_arg).await;
    let first = approve_architecture(&ctx, &defs, &project_arg, &job_id, "first approval").await;
    assert_eq!(first["package_hash_at_approval"], entered["package_hash"]);

    ok(
        &ctx,
        &defs,
        "flow_advance",
        rewind_args(
            &project_arg,
            &job_id,
            "the LDO does not meet the dropout budget",
        ),
        "rewind to architecture",
    )
    .await;

    let changed = ARCHITECTURE.replace("3V3 LDO", "3V3 buck");
    let re_entered = ok(
        &ctx,
        &defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": "gate:architecture",
            "records": architecture_records(&changed),
        }),
        "re-advance a changed architecture.md",
    )
    .await;
    assert_eq!(
        re_entered["design_hash"], entered["design_hash"],
        "the KiCad design did not change"
    );
    assert_ne!(
        re_entered["package_hash"], first["package_hash_at_approval"],
        "the package the first approval saw is not the one on disk"
    );
    assert_eq!(
        std::fs::read_to_string(record_path(&project_dir, "architecture.md")).unwrap(),
        changed
    );

    // The earlier approval is gone, not merely stale, and cannot carry the
    // job out of the gate.
    let at_gate = status(&ctx, &defs, &project_arg).await;
    assert!(
        at_gate["gate_approvals"].get("architecture").is_none(),
        "{at_gate}"
    );
    let message = refused_stale(
        &ctx,
        &defs,
        "flow_advance",
        leave_gate_args(&project_arg, &job_id),
        "leaving the gate on the earlier approval",
    )
    .await;
    assert!(
        message.contains("no approval recorded during this visit"),
        "{message}"
    );
    assert_eq!(
        status(&ctx, &defs, &project_arg).await["phase"],
        "gate:architecture"
    );

    // A new approval binds to the changed package.
    let second = approve_architecture(&ctx, &defs, &project_arg, &job_id, "re-approval").await;
    assert_eq!(
        second["package_hash_at_approval"],
        re_entered["package_hash"]
    );

    // And it binds to the records' content, which the design hash cannot see
    // (`design_state_hash` skips `.konnect/`): a hand edit after the approval
    // is refused by the package check alone — same visit, same design.
    let on_disk = record_path(&project_dir, "architecture.md");
    std::fs::write(&on_disk, format!("{changed}Also a second regulator.\n")).unwrap();
    let message = refused_stale(
        &ctx,
        &defs,
        "flow_advance",
        leave_gate_args(&project_arg, &job_id),
        "leaving the gate after architecture.md was edited by hand",
    )
    .await;
    assert!(
        message.contains("the package of gate \"architecture\" changed")
            && message.contains("architecture.md"),
        "{message}"
    );
    std::fs::write(&on_disk, &changed).unwrap();

    // With the approved bytes back, the job may leave.
    let left = ok(
        &ctx,
        &defs,
        "flow_advance",
        leave_gate_args(&project_arg, &job_id),
        "leaving the gate after approving the changed package",
    )
    .await;
    assert_eq!(left["phase"], "schematic", "{left}");
}
