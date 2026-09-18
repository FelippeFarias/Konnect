//! The photo-intake block of the phantom-tool allowlist must not shadow a real
//! parameter.
//!
//! `backticked_tool_names_in_prose_exist_in_the_registry` (in
//! `asset_references.rs`) exempts two kinds of name: parameters, derived from
//! the registry so a name only escapes by being a real property of a real tool,
//! and `NOT_TOOLS`, a hand-written list. The hand-written list is the dangerous
//! half. A name on it that *is* a real top-level parameter turns the guard into
//! a permanent no-op for that name — the phantom-allowlist failure that guard's
//! own doc comment describes.
//!
//! The `photo_intake` change's task list states this as an acceptance criterion
//! ("no name added to `NOT_TOOLS` is already a top-level property of a
//! registered tool's input schema") but left it to a one-off check that was
//! deleted afterwards. This test makes it permanent, reading the block out of
//! the guard's own source so the two cannot drift into separate copies.
//!
//! Scoped to the photo-intake block on purpose: nine pre-existing entries
//! elsewhere in `NOT_TOOLS` (`project_dir`, `lib_id`, `pin_x`, `pin_y`,
//! `footprint_path`, `new_number`, `match_all`, `replace_existing`,
//! `sheet_instance_path`) already violate the same rule. That is older debt
//! with its own owner; widening this test would only turn a real invariant into
//! a known-red one.

use konnect_core::router::registry;
use std::collections::BTreeSet;

const GUARD_SOURCE: &str = include_str!("asset_references.rs");

/// The marker comment that opens the photo-intake block inside `NOT_TOOLS`.
const BLOCK_MARKER: &str = "// Structured photo-intake response and review-map fields";

/// The string literals of the photo-intake block, comment lines skipped.
fn photo_intake_allowlist() -> Vec<String> {
    let start = GUARD_SOURCE
        .find(BLOCK_MARKER)
        .expect("asset_references.rs still carries the photo-intake NOT_TOOLS block");
    let body = &GUARD_SOURCE[start..];
    let end = body
        .find("\n    ];")
        .expect("the NOT_TOOLS array still closes with `    ];`");
    let mut names = Vec::new();
    for line in body[..end].lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        // `"name",` → ["", "name", ","]
        let mut parts = line.split('"');
        parts.next();
        if let Some(name) = parts.next() {
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    assert!(
        names.len() >= 19,
        "parsed only {} names from the photo-intake block — the parser has drifted from the \
         source, or names were removed: {names:?}",
        names.len()
    );
    names
}

/// Every top-level input property of every registered tool: exactly the set the
/// guard already exempts from the registry side.
fn registered_parameters() -> BTreeSet<String> {
    registry::ALL_TOOLSETS
        .iter()
        .flat_map(|toolset| registry::tools_for(toolset.name).unwrap_or_default())
        .filter_map(|def| {
            def.input_schema
                .get("properties")
                .and_then(|properties| properties.as_object())
                .map(|object| object.keys().cloned().collect::<Vec<_>>())
        })
        .flatten()
        .collect()
}

#[test]
fn no_photo_intake_allowlist_entry_is_a_real_tool_parameter() {
    let parameters = registered_parameters();
    let clashes: Vec<String> = photo_intake_allowlist()
        .into_iter()
        .filter(|name| parameters.contains(name))
        .collect();

    assert!(
        clashes.is_empty(),
        "these photo-intake NOT_TOOLS entries are real top-level tool parameters, already \
         exempted by the registry-derived set; listing them by hand hides any future prose \
         that names them wrongly: {clashes:?}"
    );
}

/// The five `photo_intake` parameters the task list explicitly refuses to
/// allowlist must stay out of the block, whatever else is added to it.
#[test]
fn the_photo_intake_tool_parameters_stay_out_of_the_allowlist() {
    let listed: BTreeSet<String> = photo_intake_allowlist().into_iter().collect();
    for parameter in [
        "python_path",
        "image_path",
        "map",
        "map_id",
        "timeout_seconds",
    ] {
        assert!(
            !listed.contains(parameter),
            "{parameter:?} is a photo_intake tool parameter; the schemas exempt it and the \
             block's own comment says it must not be listed"
        );
    }
}

/// A name that is a registered tool does not belong in the block either: the
/// guard would find it anyway, and the entry would outlive the tool.
#[test]
fn no_photo_intake_allowlist_entry_is_a_registered_tool() {
    let tools: BTreeSet<String> = registry::ALL_TOOLSETS
        .iter()
        .flat_map(|toolset| registry::tools_for(toolset.name).unwrap_or_default())
        .map(|def| def.name.to_string())
        .collect();

    let redundant: Vec<String> = photo_intake_allowlist()
        .into_iter()
        .filter(|name| tools.contains(name))
        .collect();

    assert!(
        redundant.is_empty(),
        "these photo-intake NOT_TOOLS entries are registered tools and need no allowlisting: \
         {redundant:?}"
    );
}
