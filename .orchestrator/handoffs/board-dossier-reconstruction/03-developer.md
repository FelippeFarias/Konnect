---
change: board-dossier-reconstruction
task: "round A tasks 1.1, 2.1-2.4, 1.2-1.6"
agent: developer
verdict: DONE
failing_layer: n/a
---

## Result

1. All ten tasks implemented in D14's order, three commits on
   `orc/board-dossier-reconstruction`: `2740ab8` (1.1), `487579c` (2.1-2.4),
   `b2eda51` (1.2-1.6). `cargo test -p konnect-core`, `cargo fmt --check` and
   `cargo clippy --all-targets` all exit 0; worktree clean; nothing pushed.
2. **All three "most likely done wrong" items were done right and are pinned
   by a mutated test.** `skip_serializing_if` is on all four new `Option`
   fields; the conditional insert is a second loop inside
   `review_map_content_hash` plus a new `hashed_fields_with_optional` (
   `hashed_fields` is byte-for-byte untouched); every array of objects is
   declared with a prose `description` and no `items`, so the allowlist is
   exactly D7's six paths — no seventh was named.
3. **The pinned digest `18afc7b999d88ad5…` is unchanged.** `fixture_review_map()`
   deliberately keeps `dossier`/`design_brief`/`mm_per_px`/`evidence` absent,
   which is what makes that literal still prove a pre-change map hashes as it
   always did. Two consequences I had to resolve, both argued in the code:
   `every_schema_key_is_either_hashed_or_deliberately_not` now reads
   `review_map_schema()` instead of the fixture (the fixture cannot carry the
   keys whose coverage most needs guarding), and it gained a second assertion
   over `scale_reference`'s own two lists.
4. `prepare_board_photo` renders in memory and only then creates `views/`, so a
   rejected call leaves no file **and** no directory — a stronger contract than
   "no file was written" and the thing three of the four bound tests assert.
   `validate_map_id` now delegates to `validate_path_token(raw, field)` so
   `label` gets its own field name in the error with `map_id`'s text unchanged.
5. `-p konnect` is red in exactly the 4 expected `doc_tool_counts` tests and
   nothing else; `asset_references` (12) still passes. Real reference photos
   verified manually through the tool (probe removed, not committed): both are
   900x1600 baseline JPEG with EXIF stripped (`"NoTransforms"`), and a
   400x400 crop at scale 2.0 produced an 800x800 PNG.

## Evidence

1. **1.1** `grep -n "jpeg" Cargo.toml` → `90:image = { … features = ["png", "jpeg"] }`;
   `grep -n "image.workspace" crates/konnect-core/Cargo.toml` → one hit, `37`,
   inside `[dependencies]` (9-39); `cargo build -p konnect-core` → `Finished`.
   Mutation (drop `"jpeg"`): `a_jpeg_photo_goes_through_the_same_pipeline … FAILED`.
2. **2.1-2.4**, `cargo test -p konnect-core --lib photo_intake::` → `59 passed; 0 failed`,
   including `a_map_with_both_new_sections_round_trips_through_save_and_load … ok`,
   `save_rejects_a_new_section_that_is_present_but_not_an_object … ok`,
   `the_map_schema_declares_both_sections_optional_and_open … ok`,
   `the_content_hash_of_the_fixture_map_is_pinned … ok`,
   `every_schema_key_is_either_hashed_or_deliberately_not … ok`, and the five
   D6 cases `adding_a_dossier_to_an_approved_map_revokes_its_approval`,
   `adding_a_design_brief_revokes_a_re_approved_dossier_only_map`,
   `resolving_a_scale_reference_changes_the_hash_and_leaving_it_unresolved_does_not`,
   `removing_an_approved_dossier_drops_the_section_and_revokes_approval`,
   `a_map_with_neither_new_section_survives_a_save_round_trip_unchanged` — all `ok`.
   **2.2** `cargo test -p konnect-core --lib fixed_records_are_closed` → `1 passed`.
3. **Mutations run on group 2** (each restored): drop `skip_serializing_if` on
   `dossier` → 8 red, target fails with `an absent dossier must stay absent, never
   serialize as null`; make the optional insert unconditional → only
   `the_content_hash_of_the_fixture_map_is_pinned` fails (the canary works);
   drop `"dossier"` from `OPTIONAL_CONTENT_KEYS` → the guard prints
   `left … "dossier" … right` naming it; give `component_survey` an `items`
   subschema → schema test says `must declare no items subschema (design D7)`
   **and** the router names the seventh path
   `…/dossier/properties/component_survey/items`, exactly as D7 predicted;
   delete the six allowlist paths → `unreviewed open input record: …/design_brief`;
   delete the non-object check → `save_rejects_a_new_section…` fails.
4. **1.2-1.6**, `cargo test -p konnect-core --lib photo_intake::board_view_tests` →
   `12 passed; 0 failed`: `the_view_schema_requires_the_three_paths_and_computes_the_rest`,
   `a_cropped_and_scaled_view_is_saved_at_exactly_the_size_it_reports`,
   `exif_orientation_is_applied_before_the_crop_is_measured`,
   `a_jpeg_photo_goes_through_the_same_pipeline`,
   `unlabelled_views_are_numbered_from_what_the_directory_already_holds`,
   `a_map_id_no_scan_ever_assigned_is_rejected_and_mints_nothing`,
   `a_label_that_is_not_a_token_is_rejected_before_any_path_join`,
   `mm_per_px_is_reported_only_when_the_saved_map_resolved_one`,
   `an_out_of_bounds_crop_names_the_oriented_dimensions_and_writes_nothing`,
   `a_photo_over_the_megapixel_cap_is_rejected_before_it_is_decoded`,
   `a_scale_outside_the_range_is_rejected_rather_than_clamped`,
   `a_view_longer_than_the_side_cap_is_rejected_before_it_is_allocated`.
   **1.6** `registry_tool_counts_match_reality … ok`,
   `photo_intake_exposes_exactly_its_six_tools_in_order … ok`,
   `photo_intake_exposes_its_tools_by_name … ok`.
   **Mutations on group 1:** remove `apply_orientation` → the EXIF test fails
   `left [40,20] / right [20,40]`; move `prepare_views_dir` above `render_view`
   → the 3 "creates nothing" bound tests fail; clamp `scale`/`crop` instead of
   rejecting → those two tests fail.
5. **Gate**: `cargo test -p konnect-core` exit 0 (`1384` lib + `4` + `12` + `5`
   e2e, 16 ignored); `cargo fmt --check` exit 0; `cargo clippy --all-targets`
   exit 0 with 0 warnings; `git status --short` empty.

## For the next agent

1. **New tool `prepare_board_photo`; its seven top-level input properties are
   `image_path`, `project_dir`, `map_id`, `crop`, `rotate`, `scale`, `label`.
   None of these may go into `NOT_TOOLS`** — `crop`/`rotate`/`scale`/`label`
   are single words `snake_words` never collects, and the other three resolve
   as real top-level tool properties. Response fields: `view_path`,
   `source_size_px`, `source_rect_px`, `output_size_px`, `exif_orientation`,
   and `mm_per_px` (present only when the saved map resolved one).
2. **Task 5.2 candidates — two-part snake_case names only** (`snake_words`
   ignores single words like `confidence`, `evidence`, `candidates`,
   `hypotheses`, `circuits`, `bom`, `unresolved`, `stackup`, `enclosure`, and
   ignores 3+-part names like `source_size_px`, `board_size_mm`,
   `design_brief_seed`, `photo_views_used`, `retrace_component_ids`,
   `search_terms_used`, `derived_from_dossier`, `user_facing_parts`,
   `why_user_facing`, `rise_time_ns`, `max_component_height_mm`,
   `depends_on_open_questions`, `mm_per_px`, `board_size_status`,
   `board_size_px`). Response: `view_path`, `exif_orientation`. Dossier:
   `component_survey`, `silkscreen_markings`, `topology_claims`,
   `retrace_correlation`, `open_questions`, `rect_px`, `scale_status`,
   `mounting_holes`, `layers_visible`, `position_px`, `location_px`,
   `visual_class`, `count_method`, `count_confidence`, `count_alternatives`,
   `claim_id`, `resolution_path`, `overlap_confidence`, `component_id`,
   `bbox_px`. Design brief: `block_diagram`, `physical_constraints`,
   `calculated_values`, `derating_notes`, `value_ohms`, `kicad_symbol`,
   `kicad_footprint`, `resolution_status`, `connector_edges`, `net_currents`,
   `net_voltages`, `signal_speeds`, `sensitive_nets`, `layer_count`,
   `assembly_notes`, `keep_outs`, `pitch_mm`, `position_mm`, `diameter_mm`,
   `continuous_a`, `peak_a`, `nominal_v`, `surge_v`, `frequency_hz`,
   `region_mm`, `region_px`. **Add only the ones the test actually flags** —
   this is a superset derived from the schemas, not from a failure run, and
   `component_id`/`bbox_px` may already be exempt.
3. **The six allowlist paths are in and the test is green at exactly six**
   (`router/mod.rs`): `save_photo_review_map/properties/map/properties/` +
   `dossier`, `dossier/properties/identity`, `dossier/properties/physical`,
   `dossier/properties/design_brief_seed`, `design_brief`,
   `design_brief/properties/physical_constraints`. If a later task makes the
   test name a seventh, something added an `items` subschema or a nested
   object — fold that node's fields into the reference doc, do not widen.
4. **Red pending docs — 4 tests, all in `crates/konnect/tests/doc_tool_counts.rs`:**
   `tool_directory_section_headings_match_the_registry`
   (`tool-directory.md:390` says 5 tools, registry says 6),
   `tool_directory_lists_every_registered_tool` (`prepare_board_photo` undocumented),
   `docs_quote_the_registry_tool_counts` (README/DEV need **232** registered and
   **239** with meta-tools), `no_file_quotes_a_stale_catalogue_total`
   (`DEV.md:331`, `:416` say "231 tools", `DEV.md:421` says "238 tools").
   Task 5.3's hand-edited files (`docs/TROUBLESHOOTING.md`,
   `packaging/metadata.json`, `plugin/plugin.json`) are **not** covered by that
   test — nothing will fail if they are missed.
5. **Two code shapes the skill docs must match.** `evidence` entries are
   `{view, rect_px, note?}` objects and `view` must be a file in the map's
   `views/` directory or a `source_images` entry — the schema cannot enforce
   either, so `dossier-schema.md` is the only place it is stated. And on disk a
   section is an object or absent: `save_photo_review_map` **rejects**
   `"dossier": null` with "Omit the key entirely to remove the section", so any
   skill text telling an agent to null a section out is wrong.

## Deferred findings

1. `review_map_schema()` had to be split into `dossier_schema()` and
   `design_brief_schema()`: one `json!` for the whole map exceeds the macro
   recursion limit. Do not inline them back, and do not raise `recursion_limit`
   on the crate to make that possible.
2. D6's literal `Limits { max_alloc: …, ..Limits::no_limits() }` does not
   compile — `image::Limits` is `#[non_exhaustive]`, which blocks functional
   update syntax outside the defining crate. Implemented as a `mut` binding
   plus a field assignment; same caps, same behaviour.
3. D1 step 8 says "then `resize_exact`" unconditionally; the code skips it when
   the computed size already equals the source size. A Lanczos3 resample at
   1:1 is not a no-op (it rings edges), and `output_size_px` is unaffected. If
   a reviewer wants D1 followed literally, the pixel assertions in
   `exif_orientation_is_applied_before_the_crop_is_measured` become fragile.
4. `prepare_board_photo` does the decode/encode synchronously inside the async
   handler — no `spawn_blocking`. Bounded by D1's caps rather than by a
   scheduler, as D1 argues; a 50 MP source still occupies a runtime worker for
   the duration. Worth revisiting only if the server grows a latency SLO.
5. Nothing verifies that a `dossier.evidence[].view` names a file that exists,
   or that its `rect_px` is inside that view — the architect's Open Question 1
   and D12 item 1 both land here. It is checkable in code (the map directory is
   known at save time) if a later change wants it.
