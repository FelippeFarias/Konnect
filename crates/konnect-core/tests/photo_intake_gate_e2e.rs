//! End-to-end exercise of the `photo_intake` approval gate through the same
//! surface an MCP client reaches: the toolset is loaded through `ToolRouter`,
//! every call is validated against the tool's own compiled input schema, and
//! the handler is invoked through the `ToolDef` the router handed back.
//!
//! The unit tests inside `tools/photo_intake.rs` call each handler directly and
//! in isolation. What they cannot show is that the five tools compose: that the
//! `map_id` one tool mints is the one the next accepts, that a save after an
//! approval revokes it, and that a reviewer's hand edit to the file on disk is
//! visible to the next `load` and turns `approval_valid` back to false. That
//! whole chain is what a schematic build depends on, so it is tested as a
//! chain.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ─── Harness ──────────────────────────────────────────────────────────────────

/// Load `photo_intake` the way an LLM does and keep the router around, so the
/// tools under test are the ones the registry actually publishes.
async fn loaded_toolset() -> (Arc<ToolContext>, Vec<ToolDef>) {
    let router = Arc::new(ToolRouter::new());
    let defs = router
        .load("photo_intake")
        .await
        .expect("photo_intake is a registered toolset");
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
        .unwrap_or_else(|| panic!("{name} is in the photo_intake toolset"));
    if let Err(error) = def.input_validator.validate(&args) {
        panic!("{name} rejected its own arguments at the schema: {error}");
    }
    (def.handler)(&args, ctx.clone())
        .await
        .unwrap_or_else(|error| panic!("{name} handler returned Err: {error}"))
}

fn payload(result: &CallToolResult) -> Value {
    match &result.content[0] {
        ToolContent::Text { text } => {
            serde_json::from_str(text).unwrap_or_else(|_| json!({ "_text": text }))
        }
        other => panic!("expected text content, got {other:?}"),
    }
}

fn text(result: &CallToolResult) -> String {
    match &result.content[0] {
        ToolContent::Text { text } => text.clone(),
        other => panic!("expected text content, got {other:?}"),
    }
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

/// The published `save_photo_review_map` schema must accept a review map.
///
/// It once did not: `tools()` declared `map` as a bare `{"type": "object"}`,
/// and `close_input_schema` (`tools/mod.rs:134`) recursed into it and inserted
/// `additionalProperties: false`. With no `properties` alongside it, the
/// subschema admitted `{}` and nothing else — while `{}` is in turn refused by
/// the handler's own `validate_incoming_map`. The MCP dispatcher validates
/// before it dispatches (`mcp/handler.rs:366`), so no caller could ever save a
/// review map and the approval gate was unreachable in production.
///
/// Every unit test of this tool calls the handler directly and so never meets
/// the validator; this one validates and nothing else.
#[test]
fn save_photo_review_map_accepts_a_real_review_map() {
    let defs = konnect_core::router::registry::tools_for("photo_intake")
        .expect("photo_intake is registered");
    let save = defs
        .iter()
        .find(|def| def.name == "save_photo_review_map")
        .expect("save tool");
    let args = json!({
        "project_dir": ".",
        "map": review_map("some-map-id", "board.png"),
    });
    if let Err(error) = save.input_validator.validate(&args) {
        panic!(
            "the published schema rejects a well-formed review map: {error}\nschema: {}",
            serde_json::to_string_pretty(&save.input_schema).unwrap()
        );
    }
}

/// The directory `scan_pcb_photo` mints for a map. Created directly here so
/// the gate chain can be tested on a machine with no Python at all; the live
/// test below gets its `map_id` from a real scan instead.
fn mint_map_dir(project: &Path, map_id: &str) -> PathBuf {
    let dir = project.join(".konnect").join("photo_intake").join(map_id);
    std::fs::create_dir_all(&dir).expect("create map dir");
    dir
}

fn map_file(project: &Path, map_id: &str) -> PathBuf {
    project
        .join(".konnect")
        .join("photo_intake")
        .join(map_id)
        .join("review_map.json")
}

/// A minimal but schema-complete review map: one confident component, one
/// component under the 0.6 flagging threshold, one traced net.
fn review_map(map_id: &str, image: &str) -> Value {
    json!({
        "map_id": map_id,
        "source_images": [image],
        "scale_reference": { "kind": "board_edge_mm", "value": "50" },
        "components": [
            {
                "component_id": "C0000",
                "ref": "U1",
                "type": "ic",
                "value": "AMS1117-3.3",
                "footprint_suggestion": "Package_TO_SOT_SMD:SOT-223",
                "confidence": 0.91,
                "bbox_px": [100, 100, 121, 81],
                "approved": true
            },
            {
                "component_id": "C0001",
                "ref": null,
                "type": "unknown",
                "value": null,
                "footprint_suggestion": null,
                "confidence": 0.5,
                "bbox_px": [225, 115, 271, 41],
                "approved": false
            }
        ],
        "nets": [
            { "connections": ["U1.1", "C0001"], "source": "traced" }
        ]
    })
}

// ─── The gate chain, no Python required ───────────────────────────────────────

/// A reviewer's annotations must survive the call the skill tells them to make.
///
/// `SKILL.md` promises "hand edits survive and are the expected workflow" and
/// then instructs the agent to re-save after any edit. `save` normalized
/// through `PhotoReviewMap`, which has no catch-all, so that re-save silently
/// deleted every key the struct does not name — top-level *and* per-component.
/// The load path was already correct, and its test proved only the load path.
#[tokio::test]
async fn out_of_schema_keys_survive_a_save_and_load_round_trip() {
    let (ctx, defs) = loaded_toolset().await;
    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let map_id = "e2e-annotations";
    mint_map_dir(&project_dir, map_id);
    let project_arg = project_dir.to_string_lossy().to_string();

    let mut incoming = review_map(map_id, "board.png");
    incoming["reviewer_note"] = json!("checked against the photo on 2026-05-01");
    incoming["components"][0]["datasheet_url"] = json!("https://example.invalid/ams1117.pdf");
    incoming["components"][1]["why_unknown"] = json!("obscured by the connector");
    incoming["nets"][0]["measured_with"] = json!("continuity tester");
    // retrace writes "" where it read nothing; the save must still normalize
    // that to null, or "read and empty" and "never read" become the same row.
    incoming["components"][1]["value"] = json!("");

    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": incoming }),
        "save with annotations",
    )
    .await;

    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load",
    )
    .await;
    let map = &loaded["map"];
    assert_eq!(
        map["reviewer_note"],
        json!("checked against the photo on 2026-05-01"),
        "a top-level annotation was dropped by save: {map}"
    );
    assert_eq!(
        map["components"][0]["datasheet_url"],
        json!("https://example.invalid/ams1117.pdf"),
        "a per-component annotation was dropped by save: {map}"
    );
    assert_eq!(
        map["components"][1]["why_unknown"],
        json!("obscured by the connector")
    );
    assert_eq!(map["nets"][0]["measured_with"], json!("continuity tester"));

    // The schema fields still round-trip, and the server still owns approval.
    assert_eq!(map["components"][0]["ref"], json!("U1"));
    assert_eq!(map["components"][1]["confidence"], json!(0.5));
    assert_eq!(
        map["components"][1]["value"],
        Value::Null,
        "preserving unknown keys must not stop the schema keys being normalized"
    );
    assert_eq!(map["approved"], json!(false));
    assert!(map["saved_at"].as_str().expect("saved_at").ends_with('Z'));

    // And the gate still closes over them: approve, then re-save the loaded
    // map unchanged, and the approval survives because the content did.
    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "approve",
    )
    .await;
    let approved = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load after approval",
    )
    .await;
    let resaved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": approved["map"].clone() }),
        "re-save unchanged",
    )
    .await;
    assert_eq!(
        resaved["approved"],
        json!(true),
        "re-saving the loaded map unchanged must not revoke approval — \
         which it would if save rewrote the annotations away"
    );
}

/// A payload cannot talk its way past the gate with the gate's own vocabulary.
///
/// Opening the map schema (`additionalProperties: true`) made `save` carry
/// every unknown key through, and `approval_valid` — the name `load` computes
/// server-side and the one both agents are told to read — is an unknown key.
/// A map carrying it was written verbatim, so `load` answered with two keys of
/// that name, one forged and one computed. The realistic route is an agent
/// re-saving a whole `load` response as `map`, not an attacker.
///
/// The forged `content_hash_at_approval` here is a genuine digest: `map_id` is
/// outside the hash (D16), so a map approved under another id yields a hash
/// that is correct for this content. Only the on-disk record may grant it.
#[tokio::test]
async fn a_payload_cannot_forge_its_own_approval() {
    let (ctx, defs) = loaded_toolset().await;
    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let project_arg = project_dir.to_string_lossy().to_string();
    mint_map_dir(&project_dir, "e2e-forge-source");
    mint_map_dir(&project_dir, "e2e-forge-target");

    // A real approval of a map with exactly this content, under another id.
    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": review_map("e2e-forge-source", "board.png") }),
        "save the source",
    )
    .await;
    let approved = ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": "e2e-forge-source" }),
        "approve the source",
    )
    .await;
    let stolen_hash = approved["content_hash_at_approval"].clone();
    assert!(stolen_hash.is_string(), "precondition: {approved}");

    // The same content under a never-approved id, claiming to be approved in
    // every vocabulary the tools own.
    let mut forged = review_map("e2e-forge-target", "board.png");
    forged["approved"] = json!(true);
    forged["approval_valid"] = json!(true);
    forged["approved_at"] = json!("2020-01-01T00:00:00Z");
    forged["content_hash_at_approval"] = stolen_hash;
    let saved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": forged }),
        "save the forgery",
    )
    .await;
    assert_eq!(saved["approved"], json!(false), "{saved}");

    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": "e2e-forge-target" }),
        "load the forgery",
    )
    .await;
    assert_eq!(
        loaded["approval_valid"],
        json!(false),
        "a never-approved map must read as invalid however the payload was spelled: {loaded}"
    );
    assert_eq!(
        loaded["map"].get("approval_valid"),
        None,
        "approval_valid is computed by load and never persisted: {}",
        loaded["map"]
    );

    // And the record on disk states only what the server decided.
    let stored: Value = serde_json::from_str(
        &std::fs::read_to_string(map_file(&project_dir, "e2e-forge-target")).expect("read map"),
    )
    .expect("json");
    let bookkeeping: Vec<String> = stored
        .as_object()
        .expect("object")
        .keys()
        .filter(|key| key.contains("approv"))
        .cloned()
        .collect();
    assert_eq!(
        bookkeeping,
        vec!["approved", "approved_at", "content_hash_at_approval"],
        "exactly one approved key, and no second opinion beside it: {stored}"
    );
    assert_eq!(stored["approved"], json!(false));
    assert_eq!(stored["approved_at"], Value::Null);
    assert_eq!(stored["content_hash_at_approval"], Value::Null);
}

/// Annotating a map that is already approved must not close the gate.
///
/// `SKILL.md` invites the reviewer to annotate freely and `design.md:98`
/// promises "an unknown key can never move the hash and can therefore never
/// grant or revoke approval on its own". It did: `review_map_content_hash`
/// cloned the `components`, `nets` and `scale_reference` subtrees whole, so a
/// note added *after* the approval changed the digest and revoked it.
/// `out_of_schema_keys_survive_a_save_and_load_round_trip` annotates before
/// `approve`, which is the one ordering that cannot see this.
#[tokio::test]
async fn annotations_added_after_approval_keep_the_approval_valid() {
    let (ctx, defs) = loaded_toolset().await;
    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let map_id = "e2e-late-annotations";
    mint_map_dir(&project_dir, map_id);
    let project_arg = project_dir.to_string_lossy().to_string();

    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": review_map(map_id, "board.png") }),
        "save",
    )
    .await;
    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "approve",
    )
    .await;

    // Now annotate, the way the skill tells the reviewer to: keys at the top
    // level, inside a component, inside a net and inside the scale reference.
    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load after approval",
    )
    .await;
    assert_eq!(loaded["approval_valid"], json!(true), "precondition");
    let mut annotated = loaded["map"].clone();
    annotated["reviewer_note"] = json!("checked against the photo on 2026-05-01");
    annotated["components"][0]["datasheet_url"] = json!("https://example.invalid/ams1117.pdf");
    annotated["nets"][0]["measured_with"] = json!("continuity tester");
    annotated["scale_reference"]["measured_from"] = json!("the silkscreen outline");

    let resaved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": annotated.clone() }),
        "save with late annotations",
    )
    .await;
    assert_eq!(
        resaved["approved"],
        json!(true),
        "an annotation is not review content, so it must not revoke approval: {resaved}"
    );

    let reloaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load after annotating",
    )
    .await;
    assert_eq!(reloaded["approval_valid"], json!(true));
    assert_eq!(
        reloaded["map"]["nets"][0]["measured_with"],
        json!("continuity tester")
    );
    assert_eq!(
        reloaded["map"]["scale_reference"]["measured_from"],
        json!("the silkscreen outline")
    );

    // And the gate still closes on the thing it is for: one schema field.
    let mut edited = reloaded["map"].clone();
    edited["components"][0]["ref"] = json!("U9");
    let revoked = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": edited }),
        "save with a schema-field edit",
    )
    .await;
    assert_eq!(
        revoked["approved"],
        json!(false),
        "editing a reviewed field must still revoke: {revoked}"
    );
}

/// save → load → approve → load → hand-edit → load → save, through the router.
///
/// Every assertion here is about a transition between two tools, which is
/// exactly what the per-handler unit tests cannot reach.
#[tokio::test]
async fn the_approval_gate_survives_a_full_save_approve_edit_cycle() {
    let (ctx, defs) = loaded_toolset().await;
    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let map_id = "e2e-gate-map";
    mint_map_dir(&project_dir, map_id);
    let project_arg = project_dir.to_string_lossy().to_string();

    // 1. A save never approves, whatever the payload claims. The map below
    //    arrives with `approved: true` already set.
    let mut incoming = review_map(map_id, "board.png");
    incoming["approved"] = json!(true);
    incoming["approved_at"] = json!("2020-01-01T00:00:00Z");
    let saved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": incoming }),
        "save",
    )
    .await;
    assert_eq!(
        saved["approved"],
        json!(false),
        "a client-supplied approval must never survive a save: {saved}"
    );
    assert_eq!(saved["map_id"], json!(map_id));

    // 2. Loading an unapproved map reports the gate shut.
    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load before approval",
    )
    .await;
    assert_eq!(loaded["approval_valid"], json!(false));
    assert_eq!(loaded["map"]["approved"], json!(false));
    // The low-confidence row survived unrounded and unguessed.
    assert_eq!(loaded["map"]["components"][1]["confidence"], json!(0.5));
    assert_eq!(loaded["map"]["components"][1]["value"], Value::Null);

    // 3. Only approve_photo_review_map opens the gate.
    let approved = ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "approve",
    )
    .await;
    assert_eq!(approved["approved"], json!(true));
    let stamped_hash = approved["content_hash_at_approval"]
        .as_str()
        .expect("content_hash_at_approval")
        .to_string();
    assert_eq!(stamped_hash.len(), 64, "sha-256 hex: {stamped_hash}");
    assert!(approved["approved_at"]
        .as_str()
        .expect("approved_at")
        .ends_with('Z'));

    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load after approval",
    )
    .await;
    assert_eq!(loaded["approval_valid"], json!(true));

    // 4. A reviewer edits the JSON file by hand — the documented workflow.
    let file = map_file(&project_dir, map_id);
    let mut on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).expect("read map")).expect("json");
    on_disk["components"][1]["ref"] = json!("C1");
    on_disk["components"][1]["value"] = json!("100nF");
    on_disk["reviewer_note"] = json!("hand-added key that must survive a round trip");
    std::fs::write(&file, serde_json::to_string_pretty(&on_disk).unwrap()).expect("write map");

    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load after hand edit",
    )
    .await;
    assert_eq!(
        loaded["approval_valid"],
        json!(false),
        "an edit after approval must invalidate it: {loaded}"
    );
    assert_eq!(
        loaded["map"]["approved"],
        json!(true),
        "the map's own flag is stale by design — that is why consumers read approval_valid"
    );
    assert_eq!(loaded["map"]["components"][1]["value"], json!("100nF"));
    assert_eq!(
        loaded["map"]["reviewer_note"],
        json!("hand-added key that must survive a round trip"),
        "load must return the file as found, unknown keys included"
    );

    // 5. Saving the edited map writes the revocation through.
    let resaved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": loaded["map"].clone() }),
        "save after hand edit",
    )
    .await;
    assert_eq!(resaved["approved"], json!(false));
    let final_load = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "final load",
    )
    .await;
    assert_eq!(final_load["approval_valid"], json!(false));
    assert_eq!(final_load["map"]["approved"], json!(false));
    assert_eq!(final_load["map"]["approved_at"], Value::Null);
    assert_eq!(final_load["map"]["content_hash_at_approval"], Value::Null);

    // 6. And a map_id that could walk out of the project never reaches a
    //    handler: the published schema refuses it at the server's front door.
    for escape in ["..", "../../elsewhere", "..\\..\\elsewhere", "a/b"] {
        let def = defs
            .iter()
            .find(|def| def.name == "load_photo_review_map")
            .expect("load tool");
        assert!(
            def.input_validator
                .validate(&json!({ "project_dir": project_arg, "map_id": escape }))
                .is_err(),
            "map_id {escape:?} must be rejected by the schema before any path is joined"
        );
    }
}

// ─── The live chain, retrace required ─────────────────────────────────────────

/// Does the tool chain hold when the `map_id` comes from a real scan?
///
/// Off by default. `RETRACE_PYTHON` is read with `expect` rather than skipped,
/// so an `--ignored` run on a machine without an interpreter fails loudly
/// instead of passing vacuously — the same contract as the live scan test in
/// `tools/photo_intake.rs`.
///
/// This test mutates the process's `HOME`/`USERPROFILE` and is therefore the
/// only `#[ignore]`d test in this file: `--ignored` runs it alone.
#[tokio::test]
#[ignore = "requires Python with retrace installed (set RETRACE_PYTHON)"]
async fn a_real_scan_feeds_the_gate_and_leaves_the_home_directory_alone() {
    let python = std::env::var("RETRACE_PYTHON").expect("set RETRACE_PYTHON");
    let (ctx, defs) = loaded_toolset().await;

    // The real user's home, captured before anything is redirected.
    let real_home = dirs::home_dir().expect("a home directory");
    let real_store = real_home.join(".local").join("share").join("retrace");
    let real_store_before = store_fingerprint(&real_store);

    // Control: retrace's global stores follow HOME. Run the scanner directly
    // with HOME pointed at a directory of our choosing and confirm it writes
    // there. Without this, "the real home gained nothing" could be true simply
    // because retrace never writes to a home at all.
    let control_home = tempfile::tempdir().expect("tempdir");
    let control_image = control_home.path().join("board.png");
    write_synthetic_board_png(&control_image).expect("synthetic board");
    let control_out = control_home.path().join("out");
    let control = std::process::Command::new(&python)
        .args([
            "-m",
            "retrace",
            "scan",
            &control_image.to_string_lossy(),
            "--format",
            "json",
            "-o",
            &control_out.to_string_lossy(),
        ])
        .env("HOME", control_home.path())
        .env("USERPROFILE", control_home.path())
        .output()
        .expect("spawn retrace");
    assert!(
        control.status.success(),
        "control scan failed: {}",
        String::from_utf8_lossy(&control.stderr)
    );
    let control_store = control_home
        .path()
        .join(".local")
        .join("share")
        .join("retrace");
    assert!(
        control_store.is_dir(),
        "retrace must write its global stores under the HOME it is given — \
         the scoped-home defence is meaningless otherwise (looked in {})",
        control_store.display()
    );

    // Now point the *process's* home at a directory nothing should touch, and
    // run the real tool. scan_pcb_photo scopes each subprocess to its own
    // throwaway home, so this stand-in must come back empty.
    let pretend_home = tempfile::tempdir().expect("tempdir");
    let previous_home = std::env::var_os("HOME");
    let previous_profile = std::env::var_os("USERPROFILE");
    std::env::set_var("HOME", pretend_home.path());
    std::env::set_var("USERPROFILE", pretend_home.path());
    let restore = || {
        match &previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match &previous_profile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
    };

    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let project_arg = project_dir.to_string_lossy().to_string();
    let image = project_dir.join("board.png");
    write_synthetic_board_png(&image).expect("synthetic board");

    // 1. Capability probe with the venv interpreter.
    let probe = ok(
        &ctx,
        &defs,
        "check_retrace",
        json!({ "python_path": python }),
        "check_retrace",
    )
    .await;
    assert_eq!(
        probe["available"],
        json!(true),
        "RETRACE_PYTHON must point at an interpreter with retrace: {probe}"
    );
    assert!(probe["retrace_version"].as_str().is_some(), "{probe}");
    assert!(probe["extras"]["detection"].is_boolean(), "{probe}");
    assert!(probe["extras"]["ocr"].is_boolean(), "{probe}");

    // 2. Scan the synthetic board.
    let scan = ok(
        &ctx,
        &defs,
        "scan_pcb_photo",
        json!({
            "image_path": image.to_string_lossy(),
            "project_dir": project_arg,
            "python_path": python,
            "timeout_seconds": 120,
        }),
        "scan_pcb_photo",
    )
    .await;
    let map_id = scan["map_id"].as_str().expect("map_id").to_string();
    let components = scan["components"].as_array().expect("components").clone();
    assert!(!components.is_empty(), "{scan}");
    assert!(
        PathBuf::from(scan["analysis_json_path"].as_str().expect("path")).is_file(),
        "{scan}"
    );

    // 3-7. The scan's own map_id drives the gate chain.
    let map = json!({
        "map_id": map_id,
        "source_images": [image.to_string_lossy()],
        "scale_reference": { "kind": "board_edge_mm", "value": "50" },
        "components": components
            .iter()
            .map(|component| json!({
                "component_id": component["id"],
                "ref": Value::Null,
                "type": component["label"],
                "value": component["value"],
                "footprint_suggestion": Value::Null,
                "confidence": component["confidence"].as_f64().unwrap_or(0.0),
                "bbox_px": component["bbox"],
                "approved": false,
            }))
            .collect::<Vec<_>>(),
        "nets": [],
        "subcircuit_hints": scan["pattern_matches"],
    });

    let saved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": map }),
        "save",
    )
    .await;
    assert_eq!(saved["approved"], json!(false));

    let before = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load before approval",
    )
    .await;
    assert_eq!(before["approval_valid"], json!(false));

    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "approve",
    )
    .await;
    let after = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load after approval",
    )
    .await;
    assert_eq!(after["approval_valid"], json!(true));

    let file = map_file(&project_dir, &map_id);
    let mut on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
    on_disk["components"][0]["ref"] = json!("U1");
    std::fs::write(&file, serde_json::to_string_pretty(&on_disk).unwrap()).expect("write");

    let edited = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": project_arg, "map_id": map_id }),
        "load after edit",
    )
    .await;
    assert_eq!(edited["approval_valid"], json!(false));

    let resaved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": project_arg, "map": edited["map"].clone() }),
        "save after edit",
    )
    .await;
    assert_eq!(resaved["approved"], json!(false));

    // 8. Nothing reached either home. Checked before the restore so a failure
    //    reports against the directory the calls actually saw.
    let pretend_store = pretend_home
        .path()
        .join(".local")
        .join("share")
        .join("retrace");
    let pretend_leak = pretend_store.exists();
    let real_store_after = store_fingerprint(&real_store);
    restore();

    assert!(
        !pretend_leak,
        "a scan wrote retrace state into the process's home at {} — the per-call scoped \
         home did not hold",
        pretend_store.display()
    );
    assert_eq!(
        real_store_before,
        real_store_after,
        "the real user's retrace store at {} changed during the run",
        real_store.display()
    );
}

/// Sorted `(relative path, length)` for every file under `root`, or an empty
/// list when it does not exist. Enough to catch a file appearing, disappearing
/// or growing, without depending on mtime resolution.
fn store_fingerprint(root: &Path) -> Vec<(String, u64)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, u64)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if let Ok(meta) = entry.metadata() {
                let name = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((name, meta.len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Four light rectangles on a board-green field — the same synthetic board the
/// in-crate live test draws, reproduced here because that helper is private.
fn write_synthetic_board_png(path: &Path) -> anyhow::Result<()> {
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
