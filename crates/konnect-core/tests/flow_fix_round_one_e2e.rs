//! Fix round 1 of the konnect-orchestrator change (reviewer 11, DECISIONs
//! A–D and H), driven through the published `flow` contract: the toolset is
//! loaded through `ToolRouter`, every call is validated against the tool's
//! own compiled input schema, and the handler runs through the `ToolDef` the
//! router handed back.
//!
//! The unit tests inside `tools/flow.rs` prove each rule against a handler;
//! these walk reviewer 11's reproductions end to end — a lane shape that
//! bound an approval to an earlier job's record (SERIOUS), a `STATE.md`
//! write that failed after the side files already recorded the decision
//! (minor 1, both directions), a passed gate's approval read back (minor 2),
//! and a rewind into a gate (minor 3) together with the route that replaces
//! it.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ─── Harness ──────────────────────────────────────────────────────────────────

async fn loaded_toolset() -> (Arc<ToolContext>, Vec<ToolDef>) {
    let router = Arc::new(ToolRouter::new());
    let defs = router
        .load("flow")
        .await
        .expect("flow is a registered toolset");
    let ctx = Arc::new(ToolContext::new(ServerConfig::default(), router));
    (ctx, defs)
}

/// Validate against the tool's own compiled schema, then dispatch.
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

/// Dispatch and insist the call was refused with `kind`, returning the body.
async fn refused(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    name: &str,
    args: Value,
    kind: &str,
    what: &str,
) -> Value {
    let result = call(ctx, defs, name, args).await;
    assert!(result.is_error, "{what} must be refused: {}", text(&result));
    let body = payload(&result);
    assert_eq!(body["error"]["kind"], kind, "{what}: {body}");
    body
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const ARCHITECTURE: &str = "# Architecture\n\nUSB-C in, 3V3 LDO, one MCU.\n\nReadiness: PASS\n";

fn project() -> (tempfile::TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical project");
    std::fs::write(path.join("demo.kicad_pro"), "{\"version\": 1}\n").expect("write project");
    std::fs::write(path.join("demo.kicad_sch"), "(kicad_sch)\n").expect("write schematic");
    let arg = path.to_string_lossy().to_string();
    (dir, path, arg)
}

fn record(filename: &str, content: &str) -> Value {
    json!({ "filename": filename, "content": content })
}

fn architecture_records() -> Value {
    json!([
        record("architecture.md", ARCHITECTURE),
        record("worst-case.md", "# Worst case\n\nLDO 0.4 W.\n"),
        record("pin-plan.md", "# Pin plan\n\nPA9 USART1_TX.\n"),
    ])
}

fn flow_dir(project: &Path) -> PathBuf {
    project.join(".konnect").join("flow")
}

/// Every file under `.konnect/flow/`, relative path → bytes: a refusal that
/// "wrote nothing" must leave this identical, and an accepted call must not.
fn fingerprint(project: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                out.insert(rel, std::fs::read(&path).expect("read flow file"));
            }
        }
    }
    let mut out = BTreeMap::new();
    let root = flow_dir(project);
    walk(&root, &root, &mut out);
    out
}

/// Every log file of the project, concatenated.
fn all_logs(project: &Path) -> String {
    let dir = flow_dir(project).join("log");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return String::new();
    };
    entries
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect()
}

async fn start(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
    lane: &str,
    phases: Option<&[&str]>,
) -> String {
    let mut args = json!({
        "project_dir": project_arg,
        "objective": "USB sensor board",
        "lane": lane,
    });
    if let Some(phases) = phases {
        args["phases"] = json!(phases);
    }
    let started = ok(ctx, defs, "flow_start", args, "flow_start").await;
    started["job_id"].as_str().expect("job_id").to_string()
}

async fn advance(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
    job_id: &str,
    to_phase: &str,
    records: Value,
) -> Value {
    let mut args = json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": to_phase });
    if records.as_array().is_some_and(|list| !list.is_empty()) {
        args["records"] = records;
    }
    ok(
        ctx,
        defs,
        "flow_advance",
        args,
        &format!("advance to {to_phase}"),
    )
    .await
}

async fn rewind(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
    job_id: &str,
    to_phase: &str,
) -> CallToolResult {
    call(
        ctx,
        defs,
        "flow_advance",
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": to_phase,
            "reason": "the review found the pin plan wrong",
        }),
    )
    .await
}

fn gate_args(project_arg: &str, job_id: &str, gate: &str, decision: &str, words: &str) -> Value {
    json!({
        "project_dir": project_arg,
        "job_id": job_id,
        "gate_name": gate,
        "decision": decision,
        "summary": "Showed the package to the user.",
        "user_words": words,
    })
}

fn defer_args(project_arg: &str, job_id: &str, description: &str) -> Value {
    json!({
        "project_dir": project_arg,
        "job_id": job_id,
        "kind": "finding",
        "description": description,
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

/// `flow_start` (new_board) → requirements → architecture → the gate.
async fn at_architecture_gate(
    ctx: &Arc<ToolContext>,
    defs: &[ToolDef],
    project_arg: &str,
) -> String {
    let job_id = start(ctx, defs, project_arg, "new_board", None).await;
    advance(
        ctx,
        defs,
        project_arg,
        &job_id,
        "architecture",
        json!([record("constraints.md", "# Constraints\n\nUSB-C.\n")]),
    )
    .await;
    let entered = advance(
        ctx,
        defs,
        project_arg,
        &job_id,
        "gate:architecture",
        architecture_records(),
    )
    .await;
    assert_eq!(entered["phase"], "gate:architecture", "{entered}");
    job_id
}

// ─── SERIOUS: a gate without its producing phase ──────────────────────────────

/// Reviewer 11's P1, both shapes plus the placement one, on a project where a
/// closed job left its `manufacturing.md` and a stale `Readiness: PASS`
/// `architecture.md` on disk: each lane is refused as `invalid_argument`
/// naming the gate and its missing phase, and `.konnect/flow/` is byte-for-byte
/// unchanged (the closed job stays closed). The corrected lane opens, must
/// re-supply `manufacturing.md` in THIS job to enter the purchase gate, and the
/// approval then binds that record: rewriting it afterwards blocks the exit.
#[tokio::test]
async fn reviewer_11_lane_shapes_are_refused_after_a_closed_job_left_its_records() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();

    // Job A: a fab_only job records rev A's package, is approved and closes.
    let job_a = start(
        &ctx,
        &defs,
        &project_arg,
        "fab_only",
        Some(&["manufacturing", "gate:purchase"]),
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_a,
        "gate:purchase",
        json!([record(
            "manufacturing.md",
            "# REV A package\n\nBOM rev A, cost USD 100.\n"
        )]),
    )
    .await;
    ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(&project_arg, &job_a, "purchase", "approve", "ok, compra"),
        "approve rev A",
    )
    .await;
    let closed = advance(&ctx, &defs, &project_arg, &job_a, "closed", json!([])).await;
    assert_eq!(closed["phase"], "closed", "{closed}");
    // A stale architecture record an earlier job left behind (reproduction B).
    std::fs::write(
        flow_dir(&project_dir)
            .join("records")
            .join("architecture.md"),
        ARCHITECTURE,
    )
    .unwrap();

    let before = fingerprint(&project_dir);
    for (phases, gate, phase) in [
        (
            &["routing", "prefab_review", "gate:purchase"][..],
            "gate:purchase",
            "manufacturing",
        ),
        (
            &["requirements", "gate:architecture", "schematic"][..],
            "gate:architecture",
            "architecture",
        ),
        (
            &["schematic", "schematic_review", "gate:placement", "routing"][..],
            "gate:placement",
            "placement",
        ),
    ] {
        let body = refused(
            &ctx,
            &defs,
            "flow_start",
            json!({
                "project_dir": project_arg,
                "objective": "Re-route after the board changed",
                "lane": "board_revision",
                "phases": phases,
            }),
            "invalid_argument",
            &format!("{phases:?}"),
        )
        .await;
        assert_eq!(body["error"]["field"], "phases", "{phases:?}: {body}");
        let message = body["message"].as_str().expect("refusal message");
        assert!(
            message.contains(&format!("{gate:?}")) && message.contains(&format!("{phase:?}")),
            "{phases:?}: the refusal names {gate} and its missing phase {phase}: {message}"
        );
        assert_eq!(
            fingerprint(&project_dir),
            before,
            "{phases:?}: a refused start writes nothing"
        );
    }
    assert_eq!(status(&ctx, &defs, &project_arg).await["phase"], "closed");

    // The corrected lane opens, and changes the fingerprint.
    let job_b = start(
        &ctx,
        &defs,
        &project_arg,
        "board_revision",
        Some(&["routing", "prefab_review", "manufacturing", "gate:purchase"]),
    )
    .await;
    assert_ne!(
        fingerprint(&project_dir),
        before,
        "an accepted start writes"
    );
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_b,
        "prefab_review",
        json!([record("routing.md", "# Routing\n\nDRC clean.\n")]),
    )
    .await;
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_b,
        "manufacturing",
        json!([record(
            "ledger-prefab.md",
            "# Ledger\n\nNo FIX_BEFORE_FAB.\n"
        )]),
    )
    .await;
    // Rev A's record on disk does not count: this job must supply its own.
    let body = refused(
        &ctx,
        &defs,
        "flow_advance",
        json!({ "project_dir": project_arg, "job_id": job_b, "to_phase": "gate:purchase" }),
        "invalid_argument",
        "entering the purchase gate without this job's manufacturing.md",
    )
    .await;
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("manufacturing.md")),
        "{body}"
    );
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_b,
        "gate:purchase",
        json!([record(
            "manufacturing.md",
            "# REV B package\n\nBOM rev B, cost USD 120.\n"
        )]),
    )
    .await;
    let shown = ok(
        &ctx,
        &defs,
        "flow_status",
        json!({ "project_dir": project_arg, "read": ["manufacturing.md"] }),
        "read the package the user is shown",
    )
    .await;
    assert!(
        shown["contents"]["manufacturing.md"]
            .as_str()
            .is_some_and(|c| c.contains("REV B")),
        "{shown}"
    );
    ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(
            &project_arg,
            &job_b,
            "purchase",
            "approve",
            "ok, compra a rev B",
        ),
        "approve rev B",
    )
    .await;
    std::fs::write(
        flow_dir(&project_dir)
            .join("records")
            .join("manufacturing.md"),
        "# REV A package\n\nBOM rev A, cost USD 100.\n",
    )
    .unwrap();
    let stale = call(
        &ctx,
        &defs,
        "flow_advance",
        json!({ "project_dir": project_arg, "job_id": job_b, "to_phase": "closed" }),
    )
    .await;
    assert!(
        stale.is_error,
        "the purchase approval binds this job's manufacturing.md: {}",
        text(&stale)
    );
}

// ─── Minor 1: STATE.md is the one commit point ────────────────────────────────

/// DECISIONs C and H, the direction the unit tests reach: the state change
/// commits and a side file that cannot be written afterwards comes back as a
/// `warning` on a success — through the published schemas, for `flow_gate`,
/// `flow_advance` and `flow_defer` — and `warning` is `null` once the side
/// files land. Also minor 2: the approval reads `current` with `valid` at the
/// gate and `passed` without `valid` after leaving it.
#[tokio::test]
async fn side_files_follow_the_state_commit_through_the_router() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();
    let job_id = at_architecture_gate(&ctx, &defs, &project_arg).await;

    let flow = flow_dir(&project_dir);
    let gates_blocker = flow.join("records").join("gates");
    std::fs::write(&gates_blocker, "not a directory\n").unwrap();
    let log_blocker = flow.join("log");
    std::fs::remove_dir_all(&log_blocker).unwrap();
    std::fs::write(&log_blocker, "not a directory\n").unwrap();

    let approved = ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(
            &project_arg,
            &job_id,
            "architecture",
            "approve",
            "pode seguir",
        ),
        "approve with both side files blocked",
    )
    .await;
    let warning = approved["warning"]
        .as_str()
        .unwrap_or_else(|| panic!("a warning string: {approved}"));
    let sep = std::path::MAIN_SEPARATOR;
    assert!(
        warning.contains(&format!("records{sep}gates"))
            && warning.contains(&format!("flow{sep}log"))
            && warning.contains("Do not repeat the call"),
        "the warning names both failed writes and forbids a retry: {warning}"
    );
    let at_gate = status(&ctx, &defs, &project_arg).await;
    let approval = &at_gate["gate_approvals"]["architecture"];
    assert_eq!(approval["status"], "current", "{at_gate}");
    assert_eq!(approval["valid"], true, "{at_gate}");
    assert_eq!(approval["user_words"], "pode seguir", "{at_gate}");

    let left = advance(&ctx, &defs, &project_arg, &job_id, "schematic", json!([])).await;
    assert_eq!(left["phase"], "schematic", "{left}");
    assert!(
        left["warning"]
            .as_str()
            .is_some_and(|w| w.contains(&format!("flow{sep}log"))),
        "{left}"
    );
    let deferred = ok(
        &ctx,
        &defs,
        "flow_defer",
        defer_args(&project_arg, &job_id, "silk R3 overlaps pad"),
        "defer with the log blocked",
    )
    .await;
    assert!(
        deferred["warning"]
            .as_str()
            .is_some_and(|w| w.contains(&format!("flow{sep}log"))),
        "{deferred}"
    );

    // The first schematic save after the gate: the approval is passed, not
    // recomputed (minor 2).
    std::fs::write(project_dir.join("demo.kicad_sch"), "(kicad_sch (saved))\n").unwrap();
    let moved_on = status(&ctx, &defs, &project_arg).await;
    assert_eq!(moved_on["phase"], "schematic", "{moved_on}");
    let passed = &moved_on["gate_approvals"]["architecture"];
    assert_eq!(passed["status"], "passed", "{moved_on}");
    assert!(passed.get("valid").is_none(), "{moved_on}");
    assert_eq!(
        passed["design_hash_at_approval"], approved["design_hash_at_approval"],
        "{moved_on}"
    );
    assert_eq!(
        moved_on["deferred_findings"].as_array().map(Vec::len),
        Some(1),
        "{moved_on}"
    );
    assert!(gates_blocker.is_file() && log_blocker.is_file());

    // Once the log can be written again, the warning is null.
    std::fs::remove_file(&log_blocker).unwrap();
    let clean = ok(
        &ctx,
        &defs,
        "flow_defer",
        defer_args(&project_arg, &job_id, "R7 value unconfirmed"),
        "defer with the log writable",
    )
    .await;
    assert!(clean["warning"].is_null(), "{clean}");
    assert!(all_logs(&project_dir).contains("R7 value unconfirmed"));
}

/// Reviewer 11's P2, the direction the fix guarantees by construction: a
/// reader holds `STATE.md` without `FILE_SHARE_DELETE`, so the rename that
/// commits it fails. A reject, a defer and a transition are each refused, and
/// none leaves a gate file or a log line claiming a change `STATE.md` never
/// held. Once the reader lets go, the committed approval still opens the gate.
#[cfg(windows)]
#[tokio::test]
async fn a_failed_state_commit_leaves_no_side_file_claiming_the_change() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;

    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();
    let job_id = at_architecture_gate(&ctx, &defs, &project_arg).await;
    ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(&project_arg, &job_id, "architecture", "approve", "sim"),
        "approve",
    )
    .await;
    let gate_file = flow_dir(&project_dir)
        .join("records")
        .join("gates")
        .join("architecture.md");
    let gate_before = std::fs::read_to_string(&gate_file).expect("gate file");
    assert!(gate_before.contains("approve"), "{gate_before}");
    let before = fingerprint(&project_dir);

    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(flow_dir(&project_dir).join("STATE.md"))
        .expect("hold STATE.md");
    for (name, args) in [
        (
            "flow_gate",
            gate_args(
                &project_arg,
                &job_id,
                "architecture",
                "reject",
                "nao, cancela",
            ),
        ),
        (
            "flow_defer",
            defer_args(&project_arg, &job_id, "a finding that never landed"),
        ),
        (
            "flow_advance",
            json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": "schematic" }),
        ),
    ] {
        let result = call(&ctx, &defs, name, args).await;
        assert!(
            result.is_error,
            "{name}: the STATE.md write must fail: {}",
            text(&result)
        );
        assert_eq!(
            fingerprint(&project_dir),
            before,
            "{name}: no side file may record a change STATE.md does not hold"
        );
    }
    drop(reader);

    let logs = all_logs(&project_dir);
    assert!(
        !logs.contains("reject") && !logs.contains("never landed"),
        "{logs}"
    );
    let after = status(&ctx, &defs, &project_arg).await;
    assert_eq!(after["phase"], "gate:architecture", "{after}");
    assert_eq!(
        after["gate_approvals"]["architecture"]["valid"], true,
        "{after}"
    );
    let left = advance(&ctx, &defs, &project_arg, &job_id, "schematic", json!([])).await;
    assert!(left["warning"].is_null(), "{left}");
}

// ─── Minor 3: no rewind into a gate ───────────────────────────────────────────

/// From `schematic`, a rewind to `gate:architecture` is refused before the
/// reason check, naming the gate and its producer, and writes nothing. The
/// route that replaces it — rewind to `architecture`, re-advance into the gate
/// with fresh records — clears the approval, re-asks, and a new approval opens
/// the gate again.
#[tokio::test]
async fn a_rewind_into_a_gate_is_refused_and_the_producer_route_re_asks() {
    let (ctx, defs) = loaded_toolset().await;
    let (_dir, project_dir, project_arg) = project();
    let job_id = at_architecture_gate(&ctx, &defs, &project_arg).await;
    ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(&project_arg, &job_id, "architecture", "approve", "sim"),
        "approve",
    )
    .await;
    advance(&ctx, &defs, &project_arg, &job_id, "schematic", json!([])).await;

    let before = fingerprint(&project_dir);
    for args in [
        json!({
            "project_dir": project_arg,
            "job_id": job_id,
            "to_phase": "gate:architecture",
            "reason": "re-show the block diagram",
        }),
        json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": "gate:architecture" }),
    ] {
        let body = refused(
            &ctx,
            &defs,
            "flow_advance",
            args.clone(),
            "invalid_argument",
            "a rewind into a gate",
        )
        .await;
        assert_eq!(body["error"]["field"], "to_phase", "{args}: {body}");
        let message = body["message"].as_str().expect("message");
        assert!(
            message.contains("\"gate:architecture\"") && message.contains("\"architecture\""),
            "{args}: {message}"
        );
        assert_eq!(fingerprint(&project_dir), before, "{args}: nothing written");
    }

    let back = rewind(&ctx, &defs, &project_arg, &job_id, "architecture").await;
    assert!(!back.is_error, "{}", text(&back));
    assert_eq!(
        payload(&back)["cleared_approvals"],
        json!(["architecture"]),
        "{}",
        text(&back)
    );
    advance(
        &ctx,
        &defs,
        &project_arg,
        &job_id,
        "gate:architecture",
        architecture_records(),
    )
    .await;
    let re_entered = status(&ctx, &defs, &project_arg).await;
    assert!(
        re_entered["gate_approvals"].get("architecture").is_none(),
        "the re-entered gate asks again: {re_entered}"
    );
    let unapproved = call(
        &ctx,
        &defs,
        "flow_advance",
        json!({ "project_dir": project_arg, "job_id": job_id, "to_phase": "schematic" }),
    )
    .await;
    assert!(unapproved.is_error, "{}", text(&unapproved));
    ok(
        &ctx,
        &defs,
        "flow_gate",
        gate_args(
            &project_arg,
            &job_id,
            "architecture",
            "approve",
            "agora sim",
        ),
        "re-approve",
    )
    .await;
    let current = status(&ctx, &defs, &project_arg).await;
    assert_eq!(
        current["gate_approvals"]["architecture"]["status"], "current",
        "{current}"
    );
    assert_eq!(
        current["gate_approvals"]["architecture"]["valid"], true,
        "{current}"
    );
    let left = advance(&ctx, &defs, &project_arg, &job_id, "schematic", json!([])).await;
    assert_eq!(left["phase"], "schematic", "{left}");
}
