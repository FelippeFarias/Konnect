//! `prepare_board_photo` driven through the surface an MCP client reaches:
//! the toolset is loaded through `ToolRouter`, every call is validated against
//! the tool's own **compiled** input schema, and only then is the `ToolDef`'s
//! handler invoked.
//!
//! Every test of this tool inside `tools/photo_intake.rs` calls
//! `handle_prepare_board_photo` directly, so none of them meets the validator
//! the MCP dispatcher runs first (`mcp/handler.rs:366`). That gap is not
//! hypothetical here: `ToolDef::new` runs `close_input_schema`
//! (`tools/mod.rs:134`), which recurses into every object subschema and
//! inserts `additionalProperties: false` — the same rewrite that once made
//! `save_photo_review_map`'s `map` parameter accept `{}` and nothing else
//! while its unit tests were all green. `prepare_board_photo` publishes a
//! nested object (`crop`) and two constrained scalars (`scale`'s range,
//! `label`'s token pattern), so where the rejection happens — validator or
//! handler — is part of the contract and is asserted here, not assumed.
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

/// Load `photo_intake` the way an LLM does, so the tool under test is the one
/// the registry actually publishes, with its schema already closed.
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

/// Validate against the published schema, then dispatch. A schema rejection is
/// a panic rather than a returned error: these are the calls the contract is
/// supposed to admit.
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

// ─── Fixtures ─────────────────────────────────────────────────────────────────

fn mint_map_dir(project: &Path, map_id: &str) -> PathBuf {
    let dir = project.join(".konnect").join("photo_intake").join(map_id);
    std::fs::create_dir_all(&dir).expect("create map dir");
    dir
}

fn views_dir(project: &Path, map_id: &str) -> PathBuf {
    project
        .join(".konnect")
        .join("photo_intake")
        .join(map_id)
        .join("views")
}

/// A `width` x `height` image whose top-left `block` x `block` corner is red
/// and whose remainder is black, so a crop landing on the wrong corner shows
/// up as a colour and not only as a size.
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

/// A little-endian TIFF block carrying exactly one IFD0 entry: tag 0x0112
/// (Orientation) = `orientation`. This is what a camera writes and what
/// `ImageDecoder::orientation()` parses.
fn exif_orientation_block(orientation: u8) -> Vec<u8> {
    let mut tiff = vec![
        0x49, 0x49, 0x2A, 0x00, // little-endian TIFF magic
        0x08, 0x00, 0x00, 0x00, // IFD0 begins at offset 8
        0x01, 0x00, // one entry
        0x12, 0x01, // tag 0x0112, Orientation
        0x03, 0x00, // type SHORT
        0x01, 0x00, 0x00, 0x00, // count 1
    ];
    tiff.extend_from_slice(&[orientation, 0x00, 0x00, 0x00]); // value, padded
    tiff.extend_from_slice(&[0x00; 4]); // no next IFD
    tiff
}

/// Splice an `Exif\0\0` APP1 segment straight after a JPEG's SOI marker — a
/// real phone photo's layout, produced in-test so no camera and no checked-in
/// binary are needed. `image`'s JPEG encoder writes no metadata of its own.
fn jpeg_with_exif_orientation(jpeg: &[u8], orientation: u8) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "starts with SOI");
    let tiff = exif_orientation_block(orientation);
    let mut segment = vec![0xFF, 0xE1];
    let length = (2 + 6 + tiff.len()) as u16;
    segment.extend_from_slice(&length.to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(&tiff);

    let mut spliced = jpeg[..2].to_vec();
    spliced.extend_from_slice(&segment);
    spliced.extend_from_slice(&jpeg[2..]);
    spliced
}

/// Sorted `(relative path, length)` for every file under `root`, empty when it
/// does not exist. Enough to catch a file appearing, disappearing or growing.
fn fingerprint(root: &Path) -> Vec<(String, u64)> {
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

fn decoded_size(path: &Path) -> (u32, u32) {
    let view = image::open(path).expect("the saved view decodes");
    (view.width(), view.height())
}

/// Every pixel is unambiguously red rather than black. Tolerant on purpose:
/// the source is a lossy JPEG and the view is resampled with Lanczos3, so
/// exact `[255, 0, 0]` would be testing the codec. The gap this has to
/// distinguish is red-vs-black, which is not a close call.
fn all_red(path: &Path) -> bool {
    image::open(path)
        .expect("the saved view decodes")
        .to_rgb8()
        .pixels()
        .all(|pixel| {
            let [red, green, blue] = pixel.0;
            red > 200 && green < 60 && blue < 60
        })
}

/// A schema-complete review map. `mm_per_px`/`evidence` are added only when
/// `resolved`, which is the whole point of the scale test below.
fn review_map(map_id: &str, image: &str, resolved: bool) -> Value {
    let scale = if resolved {
        json!({
            "kind": "board_edge_mm",
            "value": "50",
            "mm_per_px": 0.125,
            "evidence": "the 50 mm board edge in boardA_full.png spans 400 px"
        })
    } else {
        json!({ "kind": "board_edge_mm", "value": "50" })
    };
    json!({
        "map_id": map_id,
        "source_images": [image],
        "scale_reference": scale,
        "components": [
            {
                "component_id": "C0000",
                "ref": "U1",
                "type": "ic",
                "value": "AMS1117-3.3",
                "footprint_suggestion": "Package_TO_SOT_SMD:SOT-223",
                "confidence": 0.91,
                "bbox_px": [10, 10, 40, 20],
                "approved": true
            }
        ],
        "nets": [ { "connections": ["U1.1", "C0000"], "source": "traced" } ]
    })
}

/// A project with a minted map directory and one source photo beside it.
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

    fn views(&self) -> PathBuf {
        views_dir(&self.project_dir, &self.map_id)
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

/// `crop` is the parameter `close_input_schema` could have broken: it is the
/// one nested object in the schema. A client sending `{x, y, w, h}` must get
/// past the validator, and the crop must be measured in the **oriented**
/// space — proven by colour, not only by size, and against a JPEG whose EXIF
/// orientation the tool has to apply itself.
#[tokio::test]
async fn a_jpeg_crop_rotate_and_scale_survives_the_published_schema() {
    let (ctx, defs) = loaded_toolset().await;
    let fixture = Fixture::new("contract-crop");

    // 120x60 stored, red 30x30 block at the stored top-left. Orientation 6 is
    // "rotate 90 CW to display", so the oriented image is 60x120 and that
    // block lands at the oriented top-RIGHT, x in [30, 60), y in [0, 30). The
    // crop below sits strictly inside it, clear of the JPEG block boundary.
    let stored = corner_marked(120, 60, 30);
    let jpeg = jpeg_with_exif_orientation(&encode(&stored, image::ImageFormat::Jpeg), 6);
    let photo = write_source(&fixture.project_dir, "board.jpg", &jpeg);

    let before = fingerprint(&fixture.views());
    assert!(before.is_empty(), "no views exist before the call");

    let mut args = fixture.base_args(&photo);
    args["crop"] = json!({ "x": 42, "y": 2, "w": 16, "h": 16 });
    args["rotate"] = json!(90);
    args["scale"] = json!(2.0);
    args["label"] = json!("oriented_corner");

    let response = ok(&ctx, &defs, "prepare_board_photo", args, "cropped view").await;

    assert_eq!(
        response["source_size_px"],
        json!([60, 120]),
        "source_size_px is the EXIF-oriented size, not the stored one: {response}"
    );
    assert_eq!(response["source_rect_px"], json!([42, 2, 16, 16]));
    assert_eq!(
        response["output_size_px"],
        json!([32, 32]),
        "16x16 crop at scale 2.0"
    );
    assert_eq!(
        response["exif_orientation"], "Rotate90",
        "the applied orientation is reported: {response}"
    );

    let saved = fixture.views().join("oriented_corner.png");
    assert!(saved.is_file(), "the view is saved under the map's views/");
    assert_eq!(
        decoded_size(&saved),
        (32, 32),
        "the file on disk is the size the response claims"
    );
    assert!(
        all_red(&saved),
        "the crop was measured in the oriented space — a crop applied to the \
         stored orientation would have cut black pixels"
    );
    let view_path = response["view_path"]
        .as_str()
        .expect("view_path is a string");
    assert!(
        Path::new(view_path).is_file() && view_path.ends_with("oriented_corner.png"),
        "the response points at the file it wrote: {view_path}"
    );

    // The fingerprint that the error tests below assert is *unchanged* has to
    // be able to change, or those assertions prove nothing.
    assert_ne!(
        before,
        fingerprint(&fixture.views()),
        "a successful call moves the fingerprint"
    );
}

/// An out-of-bounds crop is the handler's call, not the schema's: the schema
/// cannot know the image's size. It must come back as an error *result*
/// naming the oriented dimensions, with nothing written.
#[tokio::test]
async fn an_out_of_bounds_crop_is_an_error_result_that_writes_nothing() {
    let (ctx, defs) = loaded_toolset().await;
    let fixture = Fixture::new("contract-bounds");
    let stored = corner_marked(120, 60, 20);
    let jpeg = jpeg_with_exif_orientation(&encode(&stored, image::ImageFormat::Jpeg), 6);
    let photo = write_source(&fixture.project_dir, "board.jpg", &jpeg);

    let before = fingerprint(&fixture.views());

    let mut args = fixture.base_args(&photo);
    // Inside the *stored* 120x60, outside the *oriented* 60x120.
    args["crop"] = json!({ "x": 80, "y": 0, "w": 30, "h": 30 });

    let result = call(&ctx, &defs, "prepare_board_photo", args).await;
    assert!(result.is_error, "an out-of-bounds crop is an error result");
    let message = text(&result);
    assert!(
        message.contains("60x120"),
        "the error names the image's actual oriented dimensions: {message}"
    );
    assert_eq!(
        before,
        fingerprint(&fixture.views()),
        "no file was written: {message}"
    );
}

/// `scale`'s `0.25..=4.0` range is published in the schema, so the contract's
/// answer to an out-of-range scale is a *validator* rejection: the handler is
/// never reached and cannot write. Asserted at both layers so the bound
/// survives whichever one is edited.
#[tokio::test]
async fn a_scale_outside_the_published_range_never_reaches_the_handler() {
    let (ctx, defs) = loaded_toolset().await;
    let fixture = Fixture::new("contract-scale");
    let photo = write_source(
        &fixture.project_dir,
        "board.png",
        &encode(&corner_marked(120, 60, 20), image::ImageFormat::Png),
    );
    let def = def_for(&defs, "prepare_board_photo");
    let before = fingerprint(&fixture.views());

    for out_of_range in [json!(4.5), json!(0.1)] {
        let mut args = fixture.base_args(&photo);
        args["scale"] = out_of_range.clone();
        assert!(
            def.input_validator.validate(&args).is_err(),
            "the published schema admits scale {out_of_range}, which design D1 caps"
        );
        // Belt and braces: were the schema bound ever dropped, the handler
        // still rejects rather than clamping.
        let result = (def.handler)(&args, ctx.clone())
            .await
            .expect("handler returns a result");
        assert!(result.is_error, "scale {out_of_range} is rejected");
        let message = text(&result);
        assert!(
            message.contains("0.25") && message.contains('4'),
            "the rejection names the range: {message}"
        );
    }
    assert_eq!(
        before,
        fingerprint(&fixture.views()),
        "a rejected scale writes nothing"
    );

    // 4.0 and 0.25 are the inclusive edges and must still be admitted.
    for inside in [json!(4.0), json!(0.25)] {
        let mut args = fixture.base_args(&photo);
        args["scale"] = inside.clone();
        assert!(
            def.input_validator.validate(&args).is_ok(),
            "scale {inside} is inside the published range"
        );
    }
}

/// `label` becomes a file name, so the contract has to refuse a traversal
/// token before any path join. The published pattern refuses it at the
/// validator; the handler refuses it again.
#[tokio::test]
async fn a_label_that_is_not_a_token_is_refused_by_the_published_schema() {
    let (ctx, defs) = loaded_toolset().await;
    let fixture = Fixture::new("contract-label");
    let photo = write_source(
        &fixture.project_dir,
        "board.png",
        &encode(&corner_marked(40, 40, 10), image::ImageFormat::Png),
    );
    let def = def_for(&defs, "prepare_board_photo");
    let project_before = fingerprint(&fixture.project_dir);

    for bad in ["../escape", "sub/dir", "back\\slash", "", &"x".repeat(65)] {
        let mut args = fixture.base_args(&photo);
        args["label"] = json!(bad);
        assert!(
            def.input_validator.validate(&args).is_err(),
            "the published schema admits the label {bad:?}"
        );
        let result = (def.handler)(&args, ctx.clone())
            .await
            .expect("handler returns a result");
        assert!(result.is_error, "the handler admits the label {bad:?}");
        assert!(
            text(&result).contains("label"),
            "the rejection names the parameter: {}",
            text(&result)
        );
    }
    assert_eq!(
        project_before,
        fingerprint(&fixture.project_dir),
        "a rejected label wrote nothing anywhere under the project"
    );

    // The same `map_id` guard, through the same surface: a map directory that
    // was never minted is refused and none is created.
    let mut args = fixture.base_args(&photo);
    args["map_id"] = json!("never-scanned");
    let result = call(&ctx, &defs, "prepare_board_photo", args).await;
    assert!(result.is_error, "an unknown map_id is refused");
    assert!(
        !fixture
            .project_dir
            .join(".konnect")
            .join("photo_intake")
            .join("never-scanned")
            .exists(),
        "prepare_board_photo minted a map directory: {}",
        text(&result)
    );
}

/// Design D3 through the contract: the view's `mm_per_px` is read from the
/// map that `save_photo_review_map` actually persisted, and is omitted — not
/// guessed — when that map's `scale_reference` has none.
#[tokio::test]
async fn mm_per_px_appears_only_after_the_saved_map_resolves_one() {
    let (ctx, defs) = loaded_toolset().await;
    let fixture = Fixture::new("contract-scale-ref");
    let photo = write_source(
        &fixture.project_dir,
        "board.png",
        &encode(&corner_marked(80, 40, 10), image::ImageFormat::Png),
    );

    // (1) No map on disk at all.
    let mut args = fixture.base_args(&photo);
    args["label"] = json!("before_any_map");
    let response = ok(&ctx, &defs, "prepare_board_photo", args, "view, no map").await;
    assert!(
        response.get("mm_per_px").is_none(),
        "mm_per_px was invented with no map on disk: {response}"
    );

    // (2) A saved map whose scale_reference is unresolved.
    ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({
            "project_dir": fixture.project_arg,
            "map": review_map(&fixture.map_id, "board.png", false),
        }),
        "save an unresolved map",
    )
    .await;
    let mut args = fixture.base_args(&photo);
    args["label"] = json!("unresolved_scale");
    let response = ok(&ctx, &defs, "prepare_board_photo", args, "view, unresolved").await;
    assert!(
        response.get("mm_per_px").is_none(),
        "mm_per_px reported for a map that resolved none: {response}"
    );

    // (3) The same map, re-saved with mm_per_px resolved and its evidence.
    let saved = ok(
        &ctx,
        &defs,
        "save_photo_review_map",
        json!({
            "project_dir": fixture.project_arg,
            "map": review_map(&fixture.map_id, "board.png", true),
        }),
        "save a resolved map",
    )
    .await;
    assert!(!saved.to_string().is_empty());
    let mut args = fixture.base_args(&photo);
    args["label"] = json!("resolved_scale");
    let response = ok(&ctx, &defs, "prepare_board_photo", args, "view, resolved").await;
    assert_eq!(
        response["mm_per_px"],
        json!(0.125),
        "mm_per_px comes from the persisted scale_reference: {response}"
    );

    // And the load path agrees the evidence string was persisted alongside it.
    let loaded = ok(
        &ctx,
        &defs,
        "load_photo_review_map",
        json!({ "project_dir": fixture.project_arg, "map_id": fixture.map_id }),
        "load the resolved map",
    )
    .await;
    let scale = &loaded["map"]["scale_reference"];
    assert_eq!(scale["mm_per_px"], json!(0.125), "loaded: {loaded}");
    assert!(
        scale["evidence"]
            .as_str()
            .is_some_and(|evidence| evidence.contains("50 mm board edge")),
        "the evidence string round-tripped: {scale}"
    );
}

/// The toolset the registry publishes is the six-tool one, and the new tool is
/// reachable by name through the router — the property task 1.6 states, read
/// from the router rather than from the registry constant.
#[tokio::test]
async fn the_loaded_toolset_publishes_six_tools_including_the_new_one() {
    let (_ctx, defs) = loaded_toolset().await;
    let mut names: Vec<String> = defs.iter().map(|def| def.name.to_string()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "approve_photo_review_map",
            "check_retrace",
            "load_photo_review_map",
            "prepare_board_photo",
            "save_photo_review_map",
            "scan_pcb_photo",
        ],
        "photo_intake's published tool list"
    );
}
