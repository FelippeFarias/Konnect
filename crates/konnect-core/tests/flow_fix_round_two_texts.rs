//! Fix round 2 of the konnect-orchestrator change, task 9.1 (DECISION K;
//! reviewer 16 minor 3): the texts an MCP client receives without the
//! `konnect` skill loaded. The published tool descriptions come from the
//! `ToolDef`s the router hands back for the `flow` toolset — exactly what
//! `tools/list` serves — and the `STATE.md` body is rendered by a real
//! `flow_start` validated against its own schema. Nothing here changes
//! behaviour; it keeps the texts from drifting back to "recomputed for every
//! approval" or from dropping the `warning` / "never repeat the call" rule.

use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::json;
use std::sync::Arc;

async fn loaded_toolset() -> (Arc<ToolContext>, Vec<ToolDef>) {
    let router = Arc::new(ToolRouter::new());
    let defs = router
        .load("flow")
        .await
        .expect("flow is a registered toolset");
    let ctx = Arc::new(ToolContext::new(ServerConfig::default(), router));
    (ctx, defs)
}

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The description as `tools/list` publishes it.
fn published(defs: &[ToolDef], name: &str) -> String {
    let def = defs
        .iter()
        .find(|def| def.name == name)
        .unwrap_or_else(|| panic!("{name} is in the flow toolset"));
    flat(&def.to_mcp_description().description)
}

#[tokio::test]
async fn published_flow_descriptions_scope_valid_and_name_the_warning() {
    let (_ctx, defs) = loaded_toolset().await;
    let mut broken = Vec::new();

    let status = published(&defs, "flow_status");
    for needed in [
        "only for the gate the job stands at now",
        "a passed gate reports status: passed and no `valid`",
    ] {
        if !status.contains(needed) {
            broken.push(format!("flow_status lost `{needed}`: {status}"));
        }
    }
    if status.contains("gate approvals with a recomputed") {
        broken.push(format!(
            "flow_status again says `valid` is recomputed for every approval: {status}"
        ));
    }

    for (tool, stands) in [
        ("flow_gate", "the decision stands, so never repeat the call"),
        ("flow_advance", "the move stands, so never repeat the call"),
        (
            "flow_defer",
            "the item is recorded, so never repeat the call",
        ),
    ] {
        let description = published(&defs, tool);
        for needed in [
            "A success always carries `warning`: null",
            "after STATE.md committed",
            stands,
        ] {
            if !description.contains(needed) {
                broken.push(format!("{tool} lost `{needed}`: {description}"));
            }
        }
    }

    let advance = published(&defs, "flow_advance");
    if !advance.contains("a validation refusal writes nothing") {
        broken.push(format!(
            "flow_advance lost `a validation refusal writes nothing`: {advance}"
        ));
    }
    if advance.contains("accepted; a refusal writes nothing") {
        broken.push(format!(
            "flow_advance again says any refusal writes nothing: {advance}"
        ));
    }
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}

#[tokio::test]
async fn state_body_scopes_validity_to_the_current_gate() {
    let (ctx, defs) = loaded_toolset().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical project");
    std::fs::write(path.join("demo.kicad_pro"), "{\"version\": 1}\n").expect("write project");
    std::fs::write(path.join("demo.kicad_sch"), "(kicad_sch)\n").expect("write schematic");

    let def = defs
        .iter()
        .find(|def| def.name == "flow_start")
        .expect("flow_start is in the flow toolset");
    let args = json!({
        "project_dir": path.to_string_lossy(),
        "objective": "USB sensor board",
        "lane": "new_board",
    });
    def.input_validator
        .validate(&args)
        .unwrap_or_else(|error| panic!("flow_start rejected its own arguments: {error}"));
    let result = (def.handler)(&args, ctx.clone())
        .await
        .unwrap_or_else(|error| panic!("flow_start handler returned Err: {error}"));
    assert!(!result.is_error, "flow_start failed: {:?}", result.content);

    let state = std::fs::read_to_string(path.join(".konnect").join("flow").join("STATE.md"))
        .expect("flow_start wrote STATE.md");
    let body = flat(&state);
    assert!(
        body.contains(
            "Validity is recomputed by `flow_status` only for the gate the job stands at now; \
             this body shows what was recorded."
        ),
        "STATE.md body lost the current-gate validity sentence:\n{state}"
    );
    assert!(
        !body.contains("Validity is recomputed by `flow_status`; this body"),
        "STATE.md body again says validity is recomputed for every approval:\n{state}"
    );
}
