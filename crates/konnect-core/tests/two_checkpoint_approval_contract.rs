//! The two sequential approval checkpoints one review map carries, walked end
//! to end through the published contract: the toolset is loaded through
//! `ToolRouter`, every call is validated against the tool's own compiled input
//! schema, and only then dispatched.
//!
//! The unit tests in `tools/photo_intake.rs` prove each revocation
//! individually and stop at `approval_valid: false`. What none of them shows
//! is the step the spec's "adding a design brief revokes a dossier-only
//! approval" scenario ends on — that calling `approve_photo_review_map` again
//! *restores* `approval_valid: true` for the map now carrying the brief. A
//! gate that revokes and never re-opens passes every revocation test and is
//! useless, so the re-approval is asserted here, and through the same surface
//! `kicad-schematic-build-agent` and `kicad-pcb-layout-agent` read.
//!
//! Added by QA for VERIFY round 1 of `board-dossier-reconstruction`; nothing
//! in the crate is modified by this file.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ─── Harness ──────────────────────────────────────────────────────────────────

async fn loaded_toolset() -> (Arc<ToolContext>, Vec<ToolDef>) {
    let router = Arc::new(ToolRouter::new());
    let defs = router
        .load("photo_intake")
        .await
        .expect("photo_intake is a registered toolset");
    let ctx = Arc::new(ToolContext::new(ServerConfig::default(), router));
    (ctx, defs)
}

/// Validate against the published schema, then dispatch.
async fn call(ctx: &Arc<ToolContext>, defs: &[ToolDef], name: &str, args: Value) -> CallToolResult {
    let def = defs
        .iter()
        .find(|def| def.name == name)
        .unwrap_or_else(|| panic!("{name} is in the photo_intake toolset"));
    if let Err(error) = def.input_validator.validate(&args) {
        panic!("{name} rejected its own arguments at the published schema: {error}");
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
    serde_json::from_str(&text(result)).expect("the response is JSON")
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

fn mint_map_dir(project: &Path, map_id: &str) -> PathBuf {
    let dir = project.join(".konnect").join("photo_intake").join(map_id);
    std::fs::create_dir_all(&dir).expect("create map dir");
    dir
}

fn base_map(map_id: &str) -> Value {
    json!({
        "map_id": map_id,
        "source_images": ["boardA.jpg"],
        "scale_reference": { "kind": "board_edge_mm", "value": "50" },
        "components": [
            {
                "component_id": "C0000",
                "ref": "R1",
                "type": "resistor",
                "value": "1k",
                "footprint_suggestion": "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal",
                "confidence": 0.82,
                "bbox_px": [10, 10, 40, 20],
                "approved": true
            }
        ],
        "nets": [ { "connections": ["R1.1", "C0000"], "source": "traced" } ]
    })
}

fn sample_dossier() -> Value {
    json!({
        "identity": {
            "summary": "24 V LED sign driver board, silkscreen SEMAFARO 1.3 24V 03/2020",
            "basis": "observed",
            "confidence": 0.9,
            "evidence": [
                { "view": "A_terminal_silk.png", "rect_px": [30, 430, 180, 24] }
            ]
        },
        "component_survey": [
            {
                "visual_class": "led_5mm_clear",
                "count": 107,
                "count_method": "manual_count_by_region",
                "count_confidence": 0.8,
                "locations": [
                    {
                        "region": "top-left",
                        "view": "A_center_leds.png",
                        "rect_px": [0, 0, 400, 300],
                        "count": 107,
                        "kind": "observation"
                    }
                ]
            }
        ],
        "open_questions": ["board size in mm — no scale reference resolved"]
    })
}

fn sample_design_brief() -> Value {
    json!({
        "derived_from_dossier": true,
        "block_diagram": [
            { "name": "power_input", "inputs": ["VIN_24V"], "outputs": ["VBUS"] }
        ],
        "bom": [
            {
                "role": "series resistor",
                "kicad_symbol": "Device:R",
                "kicad_footprint": "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal",
                "value": "560",
                "quantity": 19,
                "resolution_status": "resolved",
                "match_kind": "matched"
            }
        ],
        "physical_constraints": {
            "board_size_mm": null,
            "board_size_status": "unresolved — no scale reference",
            "unresolved": ["board_size_mm"]
        },
        "open_questions": ["string length not settled by the dossier"]
    })
}

/// Fresh `approval_valid` straight from `load_photo_review_map`.
async fn approval_valid(ctx: &Arc<ToolContext>, defs: &[ToolDef], project: &str, id: &str) -> bool {
    let loaded = ok(
        ctx,
        defs,
        "load_photo_review_map",
        json!({ "project_dir": project, "map_id": id }),
        "load",
    )
    .await;
    loaded["approval_valid"]
        .as_bool()
        .unwrap_or_else(|| panic!("approval_valid is a boolean: {loaded}"))
}

// ─── The two checkpoints ──────────────────────────────────────────────────────

/// save → approve → **dossier** → revoked → approve → **design_brief** →
/// revoked → approve → valid again → remove the dossier → revoked. One map,
/// one mechanism, two sequential gates that both re-open on an explicit call.
#[tokio::test]
async fn both_checkpoints_revoke_on_content_and_reopen_only_on_an_explicit_approval() {
    let (ctx, defs) = loaded_toolset().await;
    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let map_id = "two-checkpoints";
    mint_map_dir(&project_dir, map_id);
    let dir = project_dir.to_string_lossy().to_string();
    let approve = json!({ "project_dir": dir, "map_id": map_id });

    // Checkpoint zero: the base map, approved.
    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": dir, "map": base_map(map_id) }),
        "save the base map",
    )
    .await;
    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        approve.clone(),
        "approve the base map",
    )
    .await;
    assert!(
        approval_valid(&ctx, &defs, &dir, map_id).await,
        "the base map is approved"
    );

    // Checkpoint one: the dossier arrives and revokes it.
    let mut with_dossier = base_map(map_id);
    with_dossier["dossier"] = sample_dossier();
    let saved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": dir, "map": with_dossier }),
        "save with a dossier",
    )
    .await;
    assert_eq!(
        saved["approved"],
        json!(false),
        "adding a dossier revokes the approval"
    );
    assert!(
        !approval_valid(&ctx, &defs, &dir, map_id).await,
        "checkpoint one is closed until it is approved on its own terms"
    );

    // …and re-opens on an explicit call. This is the half no existing test
    // covers: a gate that only ever revokes would satisfy every revocation
    // assertion and still block the pipeline forever.
    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        approve.clone(),
        "approve the dossier",
    )
    .await;
    assert!(
        approval_valid(&ctx, &defs, &dir, map_id).await,
        "checkpoint one re-opens for the dossier"
    );

    // Checkpoint two: the design brief revokes the dossier's approval.
    let mut with_brief = base_map(map_id);
    with_brief["dossier"] = sample_dossier();
    with_brief["design_brief"] = sample_design_brief();
    let saved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": dir, "map": with_brief.clone() }),
        "save with a design brief",
    )
    .await;
    assert_eq!(
        saved["approved"],
        json!(false),
        "the brief needs its own approval"
    );
    assert!(
        !approval_valid(&ctx, &defs, &dir, map_id).await,
        "checkpoint two is closed — this is what the two consumer agents read"
    );

    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        approve.clone(),
        "approve the design brief",
    )
    .await;
    assert!(
        approval_valid(&ctx, &defs, &dir, map_id).await,
        "checkpoint two re-opens, and only now may a build consume the brief"
    );

    // Both sections survived the round trip intact, which is what the two
    // "persists additively" scenarios promise.
    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": dir, "map_id": map_id }),
        "load both sections",
    )
    .await;
    assert_eq!(loaded["map"]["dossier"], sample_dossier());
    assert_eq!(loaded["map"]["design_brief"], sample_design_brief());

    // Removing an approved section is an edit like any other.
    let mut brief_only = base_map(map_id);
    brief_only["design_brief"] = sample_design_brief();
    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": dir, "map": brief_only }),
        "save with the dossier removed",
    )
    .await;
    assert!(
        !approval_valid(&ctx, &defs, &dir, map_id).await,
        "dropping the dossier revokes the approval too"
    );
    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": dir, "map_id": map_id }),
        "load after removal",
    )
    .await;
    assert!(
        loaded["map"].get("dossier").is_none()
            || loaded["map"]["dossier"] == serde_json::Value::Null,
        "the section is gone from the record: {}",
        loaded["map"]
    );
}

/// A map that never gains either section must keep hashing as it always did —
/// `save` must not start writing a `dossier: null` key into the record and
/// quietly move every legacy map's digest. (The digest itself is pinned by
/// `the_content_hash_of_the_fixture_map_is_pinned` in `tools/photo_intake.rs`;
/// this asserts the observable half, through the contract.)
#[tokio::test]
async fn a_map_with_neither_section_keeps_its_approval_across_a_resave() {
    let (ctx, defs) = loaded_toolset().await;
    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let map_id = "no-new-sections";
    mint_map_dir(&project_dir, map_id);
    let dir = project_dir.to_string_lossy().to_string();

    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": dir, "map": base_map(map_id) }),
        "save",
    )
    .await;
    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        json!({ "project_dir": dir, "map_id": map_id }),
        "approve",
    )
    .await;
    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": dir, "map": base_map(map_id) }),
        "resave identical content",
    )
    .await;

    assert!(
        approval_valid(&ctx, &defs, &dir, map_id).await,
        "the optional sections contribute nothing when absent, so an identical \
         resave cannot revoke an approval"
    );
    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": dir, "map_id": map_id }),
        "load",
    )
    .await;
    let record = loaded["map"].as_object().expect("the map is an object");
    assert!(
        !record.contains_key("dossier") && !record.contains_key("design_brief"),
        "an absent section must not be serialized at all: {:?}",
        record.keys().collect::<Vec<_>>()
    );
}

/// `"dossier": null` is a present-but-wrong-typed section, not an omission,
/// and the contract has to say so rather than store it — a stored `null` would
/// join the hashed bytes and revoke an approval for nothing.
///
/// Through the published surface the rejection lands one layer *earlier* than
/// the unit test `save_rejects_a_new_section_that_is_present_but_not_an_object`
/// sees it: `dossier` is declared `{"type": "object"}`, so the compiled
/// validator refuses `null` and the handler is never reached. Both layers are
/// asserted, because either one alone would leave the other free to drift.
#[tokio::test]
async fn a_null_section_is_rejected_and_leaves_the_record_alone() {
    let (ctx, defs) = loaded_toolset().await;
    let project = tempfile::tempdir().expect("tempdir");
    let project_dir = project.path().canonicalize().expect("canonical project");
    let map_id = "null-section";
    mint_map_dir(&project_dir, map_id);
    let dir = project_dir.to_string_lossy().to_string();

    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({ "project_dir": dir, "map": base_map(map_id) }),
        "save the base map",
    )
    .await;
    ok(
        &ctx,
        &defs,
        "approve_photo_review_map",
        json!({ "project_dir": dir, "map_id": map_id }),
        "approve",
    )
    .await;

    let save = defs
        .iter()
        .find(|def| def.name == "save_photo_review_map")
        .expect("save_photo_review_map is in the toolset");

    for key in ["dossier", "design_brief"] {
        let mut map = base_map(map_id);
        map[key] = serde_json::Value::Null;
        let args = json!({ "project_dir": dir, "map": map });

        // Layer one: the published schema.
        assert!(
            save.input_validator.validate(&args).is_err(),
            "the published schema admits {key}: null"
        );

        // Layer two: the handler, reached directly so the schema cannot mask
        // it. `validate_incoming_map` must refuse the same value and write
        // nothing.
        let result = (save.handler)(&args, ctx.clone())
            .await
            .expect("handler returns a result");
        assert!(result.is_error, "{key}: null must be rejected");
        let message = text(&result);
        assert!(
            message.contains(key) && message.contains("Nothing was written"),
            "{key}: {message}"
        );
    }

    assert!(
        approval_valid(&ctx, &defs, &dir, map_id).await,
        "a rejected save must not have touched the record, so the approval stands"
    );
}
