//! Must-check (c) of VERIFY round 2 for `board-dossier-reconstruction`:
//! `a_reused_label_is_refused_and_the_original_view_is_unchanged`
//! (`crates/konnect-core/src/tools/photo_intake.rs`) proves the round-1 fix
//! by calling `handle_prepare_board_photo` directly. This file proves the
//! same property through the surface an MCP client actually reaches —
//! `ToolRouter::load` → the compiled input validator → the `ToolDef`
//! handler — the way `prepare_board_photo_contract.rs` does for every other
//! `prepare_board_photo` guarantee.
//!
//! Added by QA for VERIFY round 2; nothing in the crate is modified by this
//! file.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ─── Harness (duplicated from prepare_board_photo_contract.rs: QA does not
// edit existing files, and these helpers are private to that file) ─────────

async fn loaded_toolset() -> (Arc<ToolContext>, Vec<ToolDef>) {
    let router = Arc::new(ToolRouter::new());
    let defs = router
        .load("photo_intake")
        .await
        .expect("photo_intake is a registered toolset");
    let ctx = Arc::new(ToolContext::new(ServerConfig::default(), router));
    (ctx, defs)
}

fn def_for<'a>(defs: &'a [ToolDef], name: &str) -> &'a ToolDef {
    defs.iter()
        .find(|def| def.name == name)
        .unwrap_or_else(|| panic!("{name} is in the photo_intake toolset"))
}

/// Validate against the published schema, then dispatch. A schema rejection
/// is a panic rather than a returned error: this call is one the contract is
/// supposed to admit (whether it then succeeds or comes back as an error
/// *result* is for the caller to assert).
async fn call(ctx: &Arc<ToolContext>, defs: &[ToolDef], name: &str, args: Value) -> CallToolResult {
    let def = def_for(defs, name);
    if let Err(error) = def.input_validator.validate(&args) {
        panic!(
            "{name} rejected its own arguments at the published schema: {error}\nargs: {}",
            serde_json::to_string_pretty(&args).unwrap()
        );
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

struct Fixture {
    _project: tempfile::TempDir,
    project_dir: PathBuf,
    project_arg: String,
    map_id: String,
}

impl Fixture {
    fn new(map_id: &str) -> Self {
        let project = tempfile::tempdir().expect("tempdir");
        let project_dir = project.path().canonicalize().expect("canonical project");
        mint_map_dir(&project_dir, map_id);
        let project_arg = project_dir.to_string_lossy().to_string();
        Self {
            _project: project,
            project_dir,
            project_arg,
            map_id: map_id.to_string(),
        }
    }

    fn base_args(&self, image_path: &Path) -> Value {
        json!({
            "image_path": image_path.to_string_lossy(),
            "project_dir": self.project_arg,
            "map_id": self.map_id,
        })
    }
}

// ─── The contract ─────────────────────────────────────────────────────────────

/// A second `prepare_board_photo` call reusing a label that already named a
/// saved view must come back as an error naming that view's path, and the
/// first view's bytes on disk must be byte-for-byte unchanged — through the
/// same `ToolRouter::load` → validator → handler surface
/// `prepare_board_photo_contract.rs` drives every other guarantee through,
/// not through `handle_prepare_board_photo` directly.
#[tokio::test]
async fn a_reused_label_is_refused_and_the_original_view_is_unchanged_through_the_contract() {
    let (ctx, defs) = loaded_toolset().await;
    let fixture = Fixture::new("contract-reuse");

    // First view: a red-cornered 40x40 PNG, whole-image crop under a fixed
    // label.
    let first_source = write_source(
        &fixture.project_dir,
        "first.png",
        &encode(&corner_marked(40, 40, 10), image::ImageFormat::Png),
    );
    let mut first_args = fixture.base_args(&first_source);
    first_args["label"] = json!("reused_label");
    let first_response = ok(
        &ctx,
        &defs,
        "prepare_board_photo",
        first_args,
        "first view under reused_label",
    )
    .await;
    let view_path_str = first_response["view_path"]
        .as_str()
        .expect("view_path is a string")
        .to_string();
    let view_path = Path::new(&view_path_str);
    assert!(view_path.is_file(), "the first view was written to disk");
    let original_bytes = std::fs::read(view_path).expect("read the first view's bytes");
    assert!(!original_bytes.is_empty(), "the first view is not empty");

    // Second call: a different source image and a different crop, same
    // label, same map. Through the published contract this must be an error
    // result, not a silent replace.
    let second_source = write_source(
        &fixture.project_dir,
        "second.png",
        &encode(&corner_marked(80, 80, 80), image::ImageFormat::Png),
    );
    let mut second_args = fixture.base_args(&second_source);
    second_args["label"] = json!("reused_label");
    second_args["crop"] = json!({ "x": 0, "y": 0, "w": 80, "h": 80 });
    let second_result = call(&ctx, &defs, "prepare_board_photo", second_args).await;
    assert!(
        second_result.is_error,
        "a reused label must be refused, not silently overwritten"
    );
    let message = text(&second_result);
    assert!(
        message.contains(&view_path_str),
        "the refusal names the existing view's path: {message}"
    );

    let bytes_after = std::fs::read(view_path).expect("read the view's bytes after the refusal");
    assert_eq!(
        original_bytes, bytes_after,
        "the first view's bytes on disk are unchanged by the refused second call"
    );

    // A second, different label in the same map still succeeds — the
    // refusal is scoped to the reused label, not to the map or the tool.
    let mut third_args = fixture.base_args(&second_source);
    third_args["label"] = json!("second_label");
    let third_response = ok(
        &ctx,
        &defs,
        "prepare_board_photo",
        third_args,
        "second view under a fresh label",
    )
    .await;
    assert_ne!(
        third_response["view_path"], first_response["view_path"],
        "a fresh label writes a distinct file"
    );
}
