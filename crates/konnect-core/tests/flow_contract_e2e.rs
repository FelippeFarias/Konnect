//! The `flow` scenarios `flow_gate_e2e.rs` does not reach, driven through the
//! same surface an MCP client uses: the toolset is loaded through
//! `ToolRouter`, every call is validated against the tool's own compiled input
//! schema, and the handler is invoked through the `ToolDef` the router handed
//! back.
//!
//! `flow_gate_e2e.rs` exercises `flow_status`, `flow_start`, `flow_advance`
//! and `flow_gate`; `flow_log` and `flow_defer` were only ever called on their
//! handlers. This file closes that gap and walks the paths where a human gate
//! could be lost: autonomous mode across both gates of one job, a lane whose
//! last phase is the purchase gate, and a lane subset that drops a gate.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::observability::{new_call_id, unix_ms, CallRecord, CallStatus};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ─── Harness (the same one `flow_gate_e2e.rs` uses) ───────────────────────────

async fn loaded_toolset() -> (Arc<ToolContext>, Vec<ToolDef>) {
    let router = Arc::new(ToolRouter::new());
    let defs = router
        .load("flow")
        .await
        .expect("flow is a registered toolset");
    let ctx = Arc::new(ToolContext::new(ServerConfig::default(), router));
    (ctx, defs)
}

fn def<'a>(defs: &'a [ToolDef], name: &str) -> &'a ToolDef {
    defs.iter()
        .find(|def| def.name == name)
        .unwrap_or_else(|| panic!("{name} is in the flow toolset"))
}

/// Validate against the tool's own compiled schema, then dispatch.
async fn call(ctx: &Arc<ToolContext>, defs: &[ToolDef], name: &str, args: Value) -> CallToolResult {
    let def = def(defs, name);
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

async fn ok(ctx: &Arc<ToolContext>, defs: &[ToolDef], name: &str, args: Value) -> Value {
    let what = format!("{name} {args}");
    let result = call(ctx, defs, name, args).await;
    assert!(!result.is_error, "{what} failed: {}", text(&result));
    payload(&result)
}

/// Dispatch and insist the handler refused, returning the refusal body.
async fn refused(ctx: &Arc<ToolContext>, defs: &[ToolDef], name: &str, args: Value) -> Value {
    let what = format!("{name} {args}");
    let result = call(ctx, defs, name, args).await;
    assert!(result.is_error, "{what} must be refused: {}", text(&result));
    payload(&result)
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const PHOTO_PHASES: [&str; 9] = [
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

/// A KiCad project directory: a project file, one schematic and one board.
fn project() -> (tempfile::TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical project");
    std::fs::write(path.join("demo.kicad_pro"), "{\"version\": 1}\n").expect("write project");
    std::fs::write(path.join("demo.kicad_sch"), "(kicad_sch)\n").expect("write schematic");
    std::fs::write(path.join("demo.kicad_pcb"), "(kicad_pcb)\n").expect("write board");
    let arg = path.to_string_lossy().to_string();
    (dir, path, arg)
}

fn flow_dir(project: &Path) -> PathBuf {
    project.join(".konnect").join("flow")
}

fn records(pairs: &[(&str, &str)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(filename, content)| json!({ "filename": filename, "content": content }))
            .collect(),
    )
}

async fn start(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
    lane: &str,
    phases: &[&str],
    mode: &str,
) -> String {
    let started = ok(
        ctx,
        defs,
        "flow_start",
        json!({
            "project_dir": project_arg,
            "objective": "Demo board",
            "lane": lane,
            "phases": phases,
            "mode": mode,
        }),
    )
    .await;
    assert_eq!(started["phase"], phases[0], "{started}");
    started["job_id"].as_str().expect("job_id").to_string()
}

async fn advance(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
    job_id: &str,
    to_phase: &str,
    supplied: &[(&str, &str)],
) -> Value {
    let moved = ok(
        ctx,
        defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": to_phase,
            "records": records(supplied),
        }),
    )
    .await;
    assert_eq!(moved["phase"], to_phase, "{moved}");
    moved
}

fn gate_args(project_arg: &str, job_id: &str, gate_name: &str, user_words: &str) -> Value {
    json!({
        "project_dir": project_arg,
        "job_id": job_id,
        "gate_name": gate_name,
        "decision": "approve",
        "summary": "Showed the package, the renders and the purchase-gate checks.",
        "user_words": user_words,
    })
}

async fn status(ctx: &Arc<ToolContext>, defs: &[ToolDef], project_arg: &str) -> Value {
    ok(
        ctx,
        defs,
        "flow_status",
        json!({ "project_dir": project_arg }),
    )
    .await
}

async fn read(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
    names: &[&str],
) -> Value {
    ok(
        ctx,
        defs,
        "flow_status",
        json!({ "project_dir": project_arg, "read": names }),
    )
    .await
}

// ─── Single writer: a folder that is not a KiCad project ──────────────────────

/// Spec "a folder that is not a KiCad project is refused": ANY flow tool, not
/// only `flow_status` and `flow_start`, which are the two the unit tests call.
#[tokio::test]
async fn every_flow_tool_refuses_a_folder_that_is_not_a_kicad_project() {
    let (ctx, defs) = loaded_toolset().await;
    let dir = tempfile::tempdir().expect("tempdir");
    // A project one level down is not a top-level project.
    std::fs::create_dir_all(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested").join("demo.kicad_pro"), "{}").unwrap();
    let not_a_project = dir.path().to_string_lossy().to_string();
    let job_id = "demo-20260921-140000";

    let calls = [
        ("flow_status", json!({ "project_dir": not_a_project })),
        (
            "flow_start",
            json!({ "project_dir": not_a_project, "objective": "o", "lane": "new_board" }),
        ),
        (
            "flow_advance",
            json!({ "project_dir": not_a_project, "job_id": job_id, "to_phase": "architecture" }),
        ),
        (
            "flow_gate",
            gate_args(&not_a_project, job_id, "architecture", "yes"),
        ),
        (
            "flow_log",
            json!({ "project_dir": not_a_project, "job_id": job_id, "kind": "evidence", "message": "m" }),
        ),
        (
            "flow_defer",
            json!({ "project_dir": not_a_project, "job_id": job_id, "kind": "finding", "description": "d" }),
        ),
    ];
    for (name, args) in calls {
        let body = refused(&ctx, &defs, name, args).await;
        assert_eq!(body["error"]["field"], "project_dir", "{name}: {body}");
        assert!(
            !dir.path().join(".konnect").exists() && !dir.path().join("nested/.konnect").exists(),
            "{name} created a flow directory in a folder that is not a KiCad project"
        );
    }
}

// ─── flow_start: schema, lane subsets, round trip ─────────────────────────────

/// Spec "an unrecognized lane is rejected": the published schema refuses it,
/// so the handler never runs and nothing is written.
#[tokio::test]
async fn an_unknown_lane_is_refused_by_the_published_schema() {
    let (_ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();
    let schema = &def(&defs, "flow_start").input_validator;
    for lane in ["sideways", "NEW_BOARD", ""] {
        let args = json!({ "project_dir": project_arg, "objective": "o", "lane": lane });
        assert!(
            schema.validate(&args).is_err(),
            "lane {lane:?} passed the schema"
        );
    }
    let valid = json!({ "project_dir": project_arg, "objective": "o", "lane": "new_board" });
    assert!(schema.validate(&valid).is_ok());
    assert!(!project_dir.join(".konnect").exists());
}

/// A lane subset can never drop a human gate: each gated phase without its
/// gate is refused by name, through the router, and creates nothing.
#[tokio::test]
async fn a_lane_subset_that_drops_a_gate_is_refused() {
    let (ctx, defs) = loaded_toolset().await;
    let cases: [(&str, &[&str], &str); 5] = [
        (
            "board_revision",
            &["architecture", "schematic"],
            "architecture",
        ),
        ("board_revision", &["placement", "routing"], "placement"),
        ("fab_only", &["manufacturing"], "manufacturing"),
        ("fab_only", &["manufacturing", "learn"], "manufacturing"),
        ("review_only", &["gate:purchase", "learn"], "gate:purchase"),
    ];
    for (lane, phases, entry) in cases {
        let (_dir, project_dir, project_arg) = project();
        for mode in ["guided", "autonomous"] {
            let body = refused(
                &ctx,
                &defs,
                "flow_start",
                json!({
                    "project_dir": project_arg,
                    "objective": "o",
                    "lane": lane,
                    "phases": phases,
                    "mode": mode,
                }),
            )
            .await;
            assert_eq!(body["error"]["field"], "phases", "{phases:?}: {body}");
            assert!(
                body["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains(&format!("\"{entry}\"")),
                "{phases:?}: the refusal must name {entry}: {body}"
            );
            assert!(
                !project_dir.join(".konnect").exists(),
                "{phases:?}: a refused start creates nothing"
            );
        }
    }
}

/// Task 1.1 / spec "hostile text survives a round trip", through the tools: an
/// objective with `---`, a double quote, a newline, `:`, `ç`, other accented
/// letters and backticks is written by `flow_start` and read back identical
/// by `flow_status`, and no line of `STATE.md` but the two fences is `---`.
#[tokio::test]
async fn a_hostile_objective_round_trips_through_the_published_tools() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();
    let objective = "Placa ---\n---\n\"aspas\": ação `crase` ç e é: ok\r\nfim";
    ok(
        &ctx,
        &defs,
        "flow_start",
        json!({ "project_dir": project_arg, "objective": objective, "lane": "new_board" }),
    )
    .await;
    let state = status(&ctx, &defs, &project_arg).await;
    assert_eq!(state["job"]["objective"], objective, "{state}");
    let raw = std::fs::read_to_string(flow_dir(&project_dir).join("STATE.md")).unwrap();
    let fences = raw.lines().filter(|line| line.trim_end() == "---").count();
    assert_eq!(
        fences, 2,
        "only the front-matter fences may be `---`:\n{raw}"
    );
}

// ─── flow_status: reality and readable names ──────────────────────────────────

/// Spec "status reports the current design hash and any lock files", "status
/// on a project with no active job", "an agent reads its input records
/// through the tool" and "no caller path reaches the flow directory".
#[tokio::test]
async fn status_reports_the_design_hash_the_locks_and_the_records_it_is_asked_for() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();
    std::fs::write(project_dir.join("~demo.kicad_pro.lck"), "felip host").unwrap();
    std::fs::write(project_dir.join("~demo.kicad_pcb.lck"), "felip host").unwrap();

    let idle = status(&ctx, &defs, &project_arg).await;
    assert!(idle["job"].is_null() && idle["phase"].is_null(), "{idle}");
    let hash = idle["design_hash"].as_str().expect("design_hash");
    assert!(
        hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()),
        "{idle}"
    );
    let locks = idle["lock_files"].as_array().expect("lock_files").clone();
    for lock in ["~demo.kicad_pro.lck", "~demo.kicad_pcb.lck"] {
        assert!(locks.contains(&json!(lock)), "{lock} missing: {idle}");
    }
    assert!(
        !project_dir.join(".konnect").exists(),
        "status creates nothing"
    );

    for name in [
        "../x",
        "STATE.md",
        "records/constraints.md",
        "memory/../STATE.md",
    ] {
        let body = refused(
            &ctx,
            &defs,
            "flow_status",
            json!({ "project_dir": project_arg, "read": [name] }),
        )
        .await;
        assert_eq!(body["error"]["kind"], "invalid_argument", "{name}: {body}");
        assert_eq!(body["error"]["field"], "read", "{name}: {body}");
    }
    assert!(!project_dir.join(".konnect").exists());

    let job_id = start(
        &ctx,
        &defs,
        &project_arg,
        "new_board",
        &["requirements", "architecture", "gate:architecture"],
        "guided",
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "architecture",
        &[("constraints.md", "# Constraints\n\nUSB-C, 50x30 mm.\n")],
    )
    .await;
    let reading = read(
        &ctx,
        &defs,
        &project_arg,
        &["constraints.md", "architecture.md"],
    )
    .await;
    assert_eq!(
        reading["contents"]["constraints.md"],
        "# Constraints\n\nUSB-C, 50x30 mm.\n"
    );
    assert_eq!(reading["missing"], json!(["architecture.md"]));
    assert_eq!(reading["phase"], "architecture");
    assert_eq!(
        reading["last_transition"]["records"],
        json!(["constraints.md"])
    );
    assert_eq!(
        reading["last_transition"]["design_hash"],
        reading["design_hash"]
    );
}

// ─── Gates: autonomous mode and the purchase gate ─────────────────────────────

/// Spec "autonomous mode still stops at purchase", in ONE job: the session
/// approves `placement` itself, and `purchase` is refused without the user's
/// words (empty or blank) — so leaving the gate is refused — until the user's
/// words arrive, after which the job leaves the gate and runs to `closed`.
#[tokio::test]
async fn an_autonomous_job_approves_placement_itself_and_still_stops_at_purchase() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, _project_dir, project_arg) = project();
    let job_id = start(
        &ctx,
        &defs,
        &project_arg,
        "photo_to_kicad",
        &PHOTO_PHASES,
        "autonomous",
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "schematic_review",
        &[("schematic-evidence.md", "# Evidence\n")],
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "placement",
        &[("ledger-schematic.md", "# Ledger\n")],
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "gate:placement",
        &[("placement.md", "# Placement\n")],
    )
    .await;

    let placement = ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(&project_arg, &job_id, "placement", ""),
    )
    .await;
    assert_eq!(placement["approved_by"], "session", "{placement}");
    let at_gate = status(&ctx, &defs, &project_arg).await;
    assert_eq!(
        at_gate["gate_approvals"]["placement"]["approved_by"],
        "session"
    );
    assert_eq!(
        at_gate["gate_approvals"]["placement"]["valid"], true,
        "{at_gate}"
    );

    advance(&ctx, &defs, &project_arg, &job_id, "routing", &[]).await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "prefab_review",
        &[("routing.md", "# Routing\n")],
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "manufacturing",
        &[("ledger-prefab.md", "# Ledger\n")],
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "gate:purchase",
        &[("manufacturing.md", "# Package\n")],
    )
    .await;

    for words in ["", "   ", "\n\t"] {
        let body = refused(
            &ctx,
            &defs,
            "flow_gate",
            gate_args(&project_arg, &job_id, "purchase", words),
        )
        .await;
        assert_eq!(
            body["error"]["kind"], "invalid_argument",
            "{words:?}: {body}"
        );
        assert_eq!(body["error"]["field"], "user_words", "{words:?}: {body}");
        let now = status(&ctx, &defs, &project_arg).await;
        assert!(
            now["gate_approvals"].get("purchase").is_none(),
            "{words:?}: {now}"
        );
        assert_eq!(now["phase"], "gate:purchase");
    }
    let content = read(&ctx, &defs, &project_arg, &["gates/purchase.md"]).await;
    assert_eq!(
        content["missing"],
        json!(["gates/purchase.md"]),
        "no gate record was written"
    );

    // The next phase is out of reach without the approval.
    let leave = refused(
        &ctx,
        &defs,
        "flow_advance",
        json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": "learn" }),
    )
    .await;
    assert_eq!(leave["error"]["kind"], "stale_target", "{leave}");
    assert_eq!(leave["error"]["target"], "gate:purchase", "{leave}");
    assert_eq!(
        status(&ctx, &defs, &project_arg).await["phase"],
        "gate:purchase"
    );

    let purchase = ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(
            &project_arg,
            &job_id,
            "purchase",
            "Pode comprar: 5 placas, pedido aprovado.",
        ),
    )
    .await;
    assert_eq!(purchase["approved_by"], "user", "{purchase}");
    let approved = status(&ctx, &defs, &project_arg).await;
    assert_eq!(
        approved["gate_approvals"]["purchase"]["valid"], true,
        "{approved}"
    );
    assert_eq!(
        approved["gate_approvals"]["purchase"]["user_words"],
        "Pode comprar: 5 placas, pedido aprovado."
    );

    advance(&ctx, &defs, &project_arg, &job_id, "learn", &[]).await;
    advance(&ctx, &defs, &project_arg, &job_id, "closed", &[]).await;
    assert_eq!(status(&ctx, &defs, &project_arg).await["phase"], "closed");
}

/// A lane whose LAST phase is `gate:purchase` closes by a forward move out of
/// the gate — which, reason or not, needs the gate's approval. Closing is not
/// an abandon there, so it is no way around the purchase gate in either mode.
#[tokio::test]
async fn a_job_ending_at_the_purchase_gate_cannot_close_around_it() {
    let (ctx, defs) = loaded_toolset().await;
    for mode in ["guided", "autonomous"] {
        let (_dir, _project_dir, project_arg) = project();
        let job_id = start(
            &ctx,
            &defs,
            &project_arg,
            "fab_only",
            &["manufacturing", "gate:purchase"],
            mode,
        )
        .await;
        advance(
            &ctx,
            &defs,
            &project_arg,
            &job_id,
            "gate:purchase",
            &[("manufacturing.md", "# Package\n")],
        )
        .await;

        for extra in [json!({}), json!({ "reason": "the user left" })] {
            let mut args =
                json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": "closed" });
            for (key, value) in extra.as_object().unwrap() {
                args[key] = value.clone();
            }
            let body = refused(&ctx, &defs, "flow_advance", args).await;
            assert_eq!(
                body["error"]["kind"], "stale_target",
                "{mode} {extra}: {body}"
            );
            assert_eq!(
                body["error"]["target"], "gate:purchase",
                "{mode} {extra}: {body}"
            );
            assert_eq!(
                status(&ctx, &defs, &project_arg).await["phase"],
                "gate:purchase"
            );
        }

        ok(
            &ctx,
            &defs,
            "flow_gate",
            gate_args(&project_arg, &job_id, "purchase", "Aprovado, pode pedir."),
        )
        .await;
        advance(&ctx, &defs, &project_arg, &job_id, "closed", &[]).await;
    }
}

// ─── flow_advance: FIX round and evidence check through the router ────────────

/// Spec "a FIX round is a rewind with a reason", "a rewind without a reason is
/// refused" and "an uncalled tool is reported absent", through the router.
#[tokio::test]
async fn a_fix_round_and_the_evidence_check_answer_through_the_router() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, _project_dir, project_arg) = project();
    let job_id = start(
        &ctx,
        &defs,
        &project_arg,
        "photo_to_kicad",
        &PHOTO_PHASES,
        "guided",
    )
    .await;

    for status in [CallStatus::Error, CallStatus::Ok] {
        ctx.observer
            .record(CallRecord {
                call_id: new_call_id(),
                ts: unix_ms(),
                tool: "run_erc".into(),
                toolset: Some("sch_export".into()),
                dur_ms: 1,
                status,
                error_kind: None,
                args_bytes: 2,
                result_bytes: 2,
            })
            .await;
    }
    let moved = ok(
        &ctx,
        &defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": "schematic_review",
            "records": records(&[("schematic-evidence.md", "ERC clean\n")]),
            "evidence_calls": ["run_erc", "render_schematic_png"],
        }),
    )
    .await;
    assert_eq!(
        moved["phase"], "schematic_review",
        "the check never refuses"
    );
    assert_eq!(
        moved["evidence_check"]["confirmed"],
        json!(["run_erc"]),
        "{moved}"
    );
    assert_eq!(
        moved["evidence_check"]["absent"],
        json!(["render_schematic_png"]),
        "{moved}"
    );
    let after = status(&ctx, &defs, &project_arg).await;
    assert_eq!(
        after["last_transition"]["evidence_check"]["absent"],
        json!(["render_schematic_png"])
    );

    for reason in [None, Some(""), Some("   ")] {
        let mut args =
            json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": "schematic" });
        if let Some(reason) = reason {
            args["reason"] = json!(reason);
        }
        let body = refused(&ctx, &defs, "flow_advance", args).await;
        assert_eq!(body["error"]["field"], "reason", "{reason:?}: {body}");
        assert_eq!(
            status(&ctx, &defs, &project_arg).await["phase"],
            "schematic_review"
        );
    }
    let rewound = ok(
        &ctx,
        &defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": "schematic",
            "reason": "R3 pull-up missing",
        }),
    )
    .await;
    assert_eq!(rewound["transition"], "rewind", "{rewound}");
    let counted = status(&ctx, &defs, &project_arg).await;
    assert_eq!(counted["phase"], "schematic");
    assert_eq!(
        counted["fix_rounds"],
        json!({ "schematic_review": 1, "prefab_review": 0 })
    );
}

// ─── flow_log and flow_defer through the published contract ───────────────────

/// Every `flow_log` kind and every `flow_defer` kind, through the router, each
/// read back with `flow_status` (spec "evidence needs no reason or rollback",
/// "a decision without a reason/rollback is refused", "each handoff gets its
/// own file", "a deferred finding is recorded").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_journal_tools_answer_through_the_published_contract() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, _project_dir, project_arg) = project();
    let job_id = start(
        &ctx,
        &defs,
        &project_arg,
        "new_board",
        &["requirements", "architecture", "gate:architecture"],
        "guided",
    )
    .await;
    let log_args = |extra: Value| {
        let mut args = json!({ "project_dir": project_arg, "job_id": job_id });
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        args
    };
    let log_text = |status: &Value| {
        status["contents"]["log"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };

    let before = log_text(&read(&ctx, &defs, &project_arg, &["log"]).await);
    for (extra, field) in [
        (
            json!({ "kind": "decision", "message": "USB-C only.", "why": "", "rollback": "swap J1" }),
            "why",
        ),
        (
            json!({ "kind": "decision", "message": "USB-C only.", "why": "cutout", "rollback": " " }),
            "rollback",
        ),
        (
            json!({ "kind": "evidence", "message": "ERC 0", "scope": "project" }),
            "scope",
        ),
    ] {
        let body = refused(&ctx, &defs, "flow_log", log_args(extra.clone())).await;
        assert_eq!(body["error"]["field"], field, "{extra}: {body}");
        assert_eq!(
            log_text(&read(&ctx, &defs, &project_arg, &["log"]).await),
            before,
            "{extra}: a refusal appends nothing"
        );
    }

    let decided = ok(
        &ctx,
        &defs,
        "flow_log",
        log_args(json!({
            "kind": "decision",
            "message": "USB-C only; no micro-USB footprint.",
            "why": "the enclosure has a USB-C cutout",
            "rollback": "swap J1 for a micro-USB receptacle",
            "role": "requirements",
        })),
    )
    .await;
    assert_eq!(decided["read_name"], "log", "{decided}");
    let evidence = ok(
        &ctx,
        &defs,
        "flow_log",
        log_args(json!({ "kind": "evidence", "message": "C1 GRM21BR61E106KA73L: 12000 in stock on the part page, 2026-09-22." })),
    )
    .await;
    assert_eq!(evidence["read_name"], "log", "{evidence}");
    let project_lesson = ok(
        &ctx,
        &defs,
        "flow_log",
        log_args(json!({ "kind": "lesson", "message": "The enclosure fixes J1's side.", "role": "layout", "scope": "project" })),
    )
    .await;
    assert_eq!(
        project_lesson["read_name"], "memory/layout.md",
        "{project_lesson}"
    );
    let role_lesson = ok(
        &ctx,
        &defs,
        "flow_log",
        log_args(json!({ "kind": "lesson", "message": "Read pads before rotating.", "role": "layout", "scope": "role" })),
    )
    .await;
    assert_eq!(
        role_lesson["read_name"], "lessons-candidates.md",
        "{role_lesson}"
    );
    for expected in ["01-review.md", "02-review.md"] {
        let handoff = ok(
            &ctx,
            &defs,
            "flow_log",
            log_args(json!({ "kind": "handoff", "message": format!("---\nverdict: DONE\n---\n# {expected}\n"), "role": "review" })),
        )
        .await;
        assert_eq!(
            handoff["read_name"],
            format!("handoffs/{expected}"),
            "{handoff}"
        );
    }

    let journal = read(
        &ctx,
        &defs,
        &project_arg,
        &[
            "log",
            "memory/layout.md",
            "lessons-candidates.md",
            "handoffs/02-review.md",
        ],
    )
    .await;
    assert_eq!(journal["missing"], json!([]), "{journal}");
    let log = log_text(&journal);
    assert!(
        log.contains("the enclosure has a USB-C cutout") && log.contains("12000 in stock"),
        "{log}"
    );
    assert!(journal["contents"]["memory/layout.md"]
        .as_str()
        .unwrap()
        .contains("J1's side"));
    assert!(journal["contents"]["lessons-candidates.md"]
        .as_str()
        .unwrap()
        .contains("Read pads"));
    assert_eq!(
        journal["contents"]["handoffs/02-review.md"],
        "---\nverdict: DONE\n---\n# 02-review.md\n"
    );
    assert_eq!(journal["handoffs"], json!(["01-review.md", "02-review.md"]));

    for (kind, list) in [
        ("finding", "deferred_findings"),
        ("queue_item", "queue"),
        ("pending_approval", "pending_approvals"),
    ] {
        let deferred = ok(
            &ctx,
            &defs,
            "flow_defer",
            log_args(
                json!({ "kind": kind, "description": format!("a {kind}"), "owner": "sourcing" }),
            ),
        )
        .await;
        assert_eq!(deferred["list"], list, "{deferred}");
        let now = status(&ctx, &defs, &project_arg).await;
        assert_eq!(now[list][0]["description"], format!("a {kind}"), "{now}");
    }
    let empty = refused(
        &ctx,
        &defs,
        "flow_defer",
        log_args(json!({ "kind": "finding", "description": "  " })),
    )
    .await;
    assert_eq!(empty["error"]["field"], "description", "{empty}");

    // Concurrent defers through the router both land (spec "concurrent calls
    // do not lose entries").
    let (first, second) = tokio::join!(
        call(
            &ctx,
            &defs,
            "flow_defer",
            log_args(json!({ "kind": "finding", "description": "racer one" }))
        ),
        call(
            &ctx,
            &defs,
            "flow_defer",
            log_args(json!({ "kind": "finding", "description": "racer two" }))
        )
    );
    assert!(
        !first.is_error && !second.is_error,
        "{} / {}",
        text(&first),
        text(&second)
    );
    let mut findings: Vec<String> = status(&ctx, &defs, &project_arg).await["deferred_findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["description"].as_str().unwrap().to_string())
        .collect();
    findings.sort();
    assert_eq!(findings, ["a finding", "racer one", "racer two"]);
}

// ─── Registration: through the meta-tools a client calls ──────────────────────

/// Spec "the flow toolset is discoverable", through the two meta-tools a
/// client actually calls: `list_toolboxes` lists `flow` (six tools, category
/// `orchestration`), and `load_toolset("flow")` exposes exactly the six tools
/// in their published order.
#[tokio::test]
async fn list_toolboxes_and_load_toolset_expose_the_six_flow_tools() {
    use konnect_core::router::meta_tools::handle_meta_tool;
    let ctx = Arc::new(ToolContext::new(
        ServerConfig::default(),
        Arc::new(ToolRouter::new()),
    ));

    let listed = handle_meta_tool("list_toolboxes", &json!({}), &ctx)
        .await
        .expect("list_toolboxes is a meta-tool");
    let listing = payload(&listed);
    let flow = listing["toolsets"]
        .as_array()
        .expect("toolsets")
        .iter()
        .find(|toolset| toolset["name"] == "flow")
        .unwrap_or_else(|| panic!("list_toolboxes has no flow entry: {listing}"))
        .clone();
    assert_eq!(flow["tool_count"], 6, "{flow}");
    assert_eq!(flow["category"], "orchestration", "{flow}");
    assert_eq!(flow["loaded"], false, "{flow}");

    let loaded = handle_meta_tool("load_toolset", &json!({ "name": "flow" }), &ctx)
        .await
        .expect("load_toolset is a meta-tool");
    assert!(!loaded.is_error, "{}", text(&loaded));
    let names: Vec<String> = payload(&loaded)["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_string())
        .collect();
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

    let relisted = payload(
        &handle_meta_tool("list_toolboxes", &json!({}), &ctx)
            .await
            .expect("list_toolboxes"),
    );
    let flow = relisted["toolsets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|toolset| toolset["name"] == "flow")
        .unwrap()
        .clone();
    assert_eq!(flow["loaded"], true, "{flow}");
}
