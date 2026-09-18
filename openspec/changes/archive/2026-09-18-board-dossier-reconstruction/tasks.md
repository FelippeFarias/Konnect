Implementation worktree: `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\board-dossier-reconstruction` (branch `orc/board-dossier-reconstruction`, base `1815948` = head of the archived `photo-to-kicad-reverse` change). All file paths below are relative to that worktree's repo root. Truth commands: `cargo test -p konnect`, `cargo test -p konnect-core` (with `PROTOC=C:/Users/felip/tools/protoc/bin/protoc.exe` and the VS 2022 BuildTools CMake extension on `PATH`); lint gate: `cargo fmt --check`, `cargo clippy --all-targets`; live retrace: `RETRACE_PYTHON=C:/Users/felip/Documents/FFS-Hardware-Eng/Konnect/.venv-retrace/Scripts/python.exe`. `stacks: none` for this project — every task below is `Stack: none`.

## 1. `image` dependency and `prepare_board_photo`

- [x] 1.1 Change the workspace root `Cargo.toml`'s `image` declaration from `features = ["png"]` to `features = ["png", "jpeg"]`, move `crates/konnect-core/Cargo.toml`'s `image.workspace = true` from `[dev-dependencies]` to `[dependencies]` (delete the now-redundant dev-only line), per design D2.
Stack: none
Acceptance: `grep -n "jpeg" Cargo.toml` matches the `image` line, `grep -n "image.workspace" crates/konnect-core/Cargo.toml` shows exactly one occurrence under `[dependencies]`, and `cargo build -p konnect-core` succeeds.

- [x] 1.2 Add the `prepare_board_photo` tool definition to `crates/konnect-core/src/tools/photo_intake.rs`: input schema per design D1's table (`image_path`, `project_dir`, `map_id` all required; `crop`/`rotate`/`scale`/`label` optional), registered via the existing four-arg `tool!` macro alongside the other five tools in `photo_intake::tools()`.
Stack: none
Acceptance: `cargo build -p konnect-core` succeeds with `handle_prepare_board_photo` defined and wired into `photo_intake::tools()`, and its input schema requires exactly `image_path`/`project_dir`/`map_id`.

- [x] 1.3 Implement the decode → EXIF-orient → crop → rotate → scale → PNG-encode pipeline in `handle_prepare_board_photo` using `image::ImageReader` + `DynamicImage` (design D1's nine numbered steps, each with its verified `image` 0.25.10 call site), writing to `<canonical project_dir>/.konnect/photo_intake/<map_id>/views/<label-or-n>.png`, computed per archived D13 rule 4 (confinement re-check) and never supplied by the caller.
Stack: none
Acceptance: a unit test calls the handler with a real decoded test image, a `crop` and a `scale`, and asserts the saved PNG's dimensions match `output_size_px` in the response; the response also carries `source_size_px` and `exif_orientation`, and `crop` is interpreted in the post-orientation coordinate space.

- [x] 1.4 Enforce design D8's path rule: `map_id` and `label` both validated as `^[A-Za-z0-9_-]{1,64}$` tokens before any path join (reusing `validate_map_id`, `photo_intake.rs:949`, for both), and `map_id`'s directory must already exist (`existing_map_dir`, `photo_intake.rs:969`) — `prepare_board_photo` never mints a new map directory.
Stack: none
Acceptance: unit tests assert (1) a `map_id` naming a directory that does not exist returns an error and creates no file, and (2) a `label` containing `..`, `/`, or `\` is rejected before any path join.

- [x] 1.5 Implement `mm_per_px` in the response (included only when the map's persisted `scale_reference` currently has a resolved `mm_per_px`, design D3) and design D1's four bounds: an out-of-bounds `crop` error naming the source image's actual oriented dimensions, a >50 MP input rejected before decoding, a `scale` outside `0.25..=4.0` rejected rather than clamped, and a computed output whose long side exceeds 4096 px rejected before allocating.
Stack: none
Acceptance: unit tests cover the spec scenarios "a view carries a physical scale when one is resolved" (present and absent cases) and "an out-of-bounds crop is rejected", plus one test per bound asserting the error names both the actual value and the cap and that no file was written.

- [x] 1.6 Register the toolset's new `tool_count: 6` in `ALL_TOOLSETS` (`crates/konnect-core/src/router/registry.rs`) and add an integration test asserting `registry::tools_for("photo_intake")` returns exactly 6 tools including `prepare_board_photo`.
Stack: none
Acceptance: `cargo test -p konnect-core registry_tool_counts_match_reality` passes, and `cargo test -p konnect-core photo_intake::` includes and passes a test asserting the 6-element tool list.

## 2. Review-map schema extension and hash coverage

- [x] 2.1 Extend `PhotoReviewMap` and `review_map_schema()` in `crates/konnect-core/src/tools/photo_intake.rs` with the optional `dossier` (design D4) and `design_brief` (design D5) objects, and extend `ScaleReference` with optional `mm_per_px`/`evidence` (design D3). All four new struct fields are `Option<...>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` (design D6 — without it every existing map's digest changes on its next save); `dossier`/`design_brief` are `Option<serde_json::Value>`, not typed structs. In the schema, object-valued nodes get `properties` + `"additionalProperties": true`, and every array of objects is declared `{"type": "array", "description": ...}` with no `items` subschema, following `subcircuit_hints` (`photo_intake.rs:1146`) — design D7.
Stack: none
Acceptance: `cargo build -p konnect-core` succeeds, `dossier`/`design_brief` are absent from `review_map_schema()`'s `required` array, `validate_incoming_map` rejects a `dossier`/`design_brief` that is present but not a JSON object (including `null`), and a unit test round-trips a map containing both sections through `save_photo_review_map`/`load_photo_review_map` unchanged.

- [x] 2.2 Add design D7's six allowlist paths (`.../dossier`, `.../dossier/properties/identity`, `.../dossier/properties/physical`, `.../dossier/properties/design_brief_seed`, `.../design_brief`, `.../design_brief/properties/physical_constraints`) to `fixed_records_are_closed_and_only_reviewed_maps_are_extensible` (`crates/konnect-core/src/router/mod.rs:210-213`), each with the same one-line "human edits this by hand" rationale as its neighbors.
Stack: none
Acceptance: `cargo test -p konnect-core fixed_records_are_closed_and_only_reviewed_maps_are_extensible` passes with exactly those six paths added; a seventh path named by the test means the schema grew an `items` subschema or a nested object beyond design D4/D5 — fold that node's fields into the reference doc instead of widening the allowlist further.

- [x] 2.3 Apply design D6's three edits inside `review_map_content_hash` (`photo_intake.rs:860`), not inside `hashed_fields`/`hashed_elements` (`:800`/`:820`), whose unconditional insert must stay exactly as it is: (1) add `const OPTIONAL_CONTENT_KEYS: [&str; 2] = ["dossier", "design_brief"]` and a second loop after the `CONTENT_KEYS` loop that inserts each only when `map.get(key)` is `Some`, cloning the whole section with no field projection; (2) add `SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS` + a new `hashed_fields_with_optional` helper and call it for `"scale_reference"`; (3) chain `OPTIONAL_CONTENT_KEYS` into `every_schema_key_is_either_hashed_or_deliberately_not` (`:2759-2767`).
Stack: none
Acceptance: the archived pinned-digest unit test (task 3.6 of the archived change) passes unmodified against its existing fixture, proving the digest for a map with none of the new fields is unchanged; `cargo test -p konnect-core every_schema_key_is_either_hashed_or_deliberately_not` passes.

- [x] 2.4 Add unit tests for the two-checkpoint approval mechanism (design D6's "gate closes in both directions" proof): (1) saving a `dossier` onto a previously approved map (no `design_brief`) changes the hash and resets `approved` to `false`; (2) approving that map again, then adding a `design_brief`, again resets `approved` to `false`; (3) a `scale_reference` gaining `mm_per_px`/`evidence` changes the hash, while a map with neither field hashes identically to before; (4) saving a map with `dossier` omitted after it was approved *with* one removes the section from the record and revokes approval; (5) a map with neither new section serializes without `dossier`/`design_brief` keys and hashes identically before and after a save round trip — the `skip_serializing_if` guard, mirroring `a_review_map_round_trips_and_tolerates_absent_subcircuit_hints` (`photo_intake.rs:2773`).
Stack: none
Acceptance: `cargo test -p konnect-core photo_intake::` includes and passes tests named to reflect each of the five cases above.

## 3. Skills: dossier, design reconstruction, and the top-level workflow

- [x] 3.1 Write `crates/konnect/assets/skills/kicad-board-dossier/SKILL.md`: methodology for survey (`Read` every photo), zoom (`prepare_board_photo`), classify/count by visual class with a stated `count_method`, correlate with `scan_pcb_photo` boxes by bbox overlap, write competing `hypotheses` when evidence does not resolve a question, and get human approval via `approve_photo_review_map` — triggers on "understand this board", "what is this board", "survey this board photo".
Stack: none
Acceptance: `crates/konnect/assets/skills/kicad-board-dossier/SKILL.md` exists and every `load_toolset(...)` example in it names only real toolsets.

- [x] 3.2 Write `crates/konnect/assets/skills/kicad-board-dossier/references/dossier-schema.md` (design D4's schema, field by field — including every array element's field list, since design D7 keeps those out of the JSON Schema — stating that `basis`/`confidence` are required on every claim-bearing object, that every `evidence` entry is a `{view, rect_px, note?}` pointer whose `view` names a file under the map's `views/` directory or a `source_images` entry, and that `hypotheses`/`count_alternatives` exist to hold disagreement, not resolve it), named inside `kicad-board-dossier/SKILL.md`'s text.
Stack: none
Acceptance: `crates/konnect/assets/skills/kicad-board-dossier/references/dossier-schema.md` exists and `grep -c "dossier-schema.md" crates/konnect/assets/skills/kicad-board-dossier/SKILL.md` is at least 1.

- [x] 3.3 Write `crates/konnect/assets/skills/kicad-design-reconstruction/SKILL.md`: methodology from an approved dossier — block diagram, per-block calculated values with derating, a BOM resolved via `search_symbols`/`search_footprints` (never invented — an unresolved part is `kicad_symbol`/`kicad_footprint` `null` plus `resolution_status: "unresolved"` and `candidates[]`, per design D5), a `physical_constraints` record covering every row of design D5's table with `unresolved[]` naming the gaps, and a hand-off back to the session for `kicad-schematic-build-agent`/`kicad-pcb-layout-agent` — triggers on "design a board from this dossier", "reconstruct this board".
Stack: none
Acceptance: `crates/konnect/assets/skills/kicad-design-reconstruction/SKILL.md` exists and every `tool(arg, ...)`-shaped example in it uses real parameter names.

- [x] 3.4 Write `crates/konnect/assets/skills/kicad-design-reconstruction/references/design-brief-schema.md` (design D5's schema, field by field — including every array element's field list — stating the `kicad_symbol`/`kicad_footprint` resolution rule and its `resolution_status`/`candidates`/`search_terms_used` representation for an unresolved part, and reproducing design D5's row-by-row mapping from `physical_constraints` to `kicad-pcb/references/layout-methodology.md:30-38` plus the `unresolved[]` convention), named inside `kicad-design-reconstruction/SKILL.md`'s text.
Stack: none
Acceptance: `crates/konnect/assets/skills/kicad-design-reconstruction/references/design-brief-schema.md` exists and `grep -c "design-brief-schema.md" crates/konnect/assets/skills/kicad-design-reconstruction/SKILL.md` is at least 1.

- [x] 3.5 Write `crates/konnect/assets/skills/kicad-photo-to-board/SKILL.md`: the single-entry-point workflow skill sequencing `check_retrace` → `scan_pcb_photo` → dossier (`pcb-photo-intake-agent`) → approval #1 → `design_brief` (`pcb-design-reconstruction-agent`) → approval #2 → `kicad-schematic-build-agent` → `kicad-pcb-layout-agent` → `kicad-design-review-agent`, using design D9's stage table (stage, owner, consumes, produces, gate, `INCOMPLETE` conditions), stating that every stage reports `INCOMPLETE` rather than inventing past a closed gate or missing evidence, and stating design D9's delegation rule: agents cannot spawn agents, so the table is addressed to the orchestrating session, which invokes one agent per stage and reads each agent's five-field return block (`stage`, `map_id`, `produced`, `verdict`, `blockers`) before invoking the next — triggers on "I have photos of a board, recreate it", "reverse engineer and rebuild this board".
Stack: none
Acceptance: `crates/konnect/assets/skills/kicad-photo-to-board/SKILL.md` exists and names all four agents (`pcb-photo-intake-agent`, `pcb-design-reconstruction-agent`, `kicad-schematic-build-agent`, `kicad-pcb-layout-agent`) plus `kicad-design-review-agent`.

- [x] 3.6 Update `crates/konnect/assets/skills/kicad-photo-intake/references/review-map-schema.md` to document the additive `dossier`/`design_brief` sections and the widened `scale_reference` (design D3/D4/D5), including the "outside/inside the hash" table entries for the two new sections.
Stack: none
Acceptance: `grep -c "dossier\|design_brief" crates/konnect/assets/skills/kicad-photo-intake/references/review-map-schema.md` is at least 2.

## 4. Agents, routing, and consumer notes

- [x] 4.1 Extend `crates/konnect/assets/agents/pcb-photo-intake-agent.md`: frontmatter `tools:` gains `Read`, `skills:` gains `kicad-board-dossier`; insert the comprehension/dossier phase between the existing scan and persist-and-review phases (survey, zoom, classify, correlate, write `dossier`, get approval #1); add the two Hard Rules of design D9 — `Read` is used only on files under `<project_dir>/.konnect/photo_intake/<map_id>/views/` or on the exact user-supplied source photos listed in the map's `source_images`, and no `dossier` claim is written without an `evidence` `{view, rect_px}` pointer.
Stack: none
Acceptance: `crates/konnect/assets/agents/pcb-photo-intake-agent.md`'s frontmatter `tools:` list contains `Read` and `mcp__konnect__*`, its `skills:` list contains exactly `konnect`, `kicad-photo-intake`, and `kicad-board-dossier`, and its Hard Rules name both the `Read` confinement and the evidence-pointer requirement.

- [x] 4.2 Write `crates/konnect/assets/agents/pcb-design-reconstruction-agent.md`: frontmatter `skills: [konnect, kicad-design-reconstruction]`, `tools: [mcp__konnect__*]` (no `Read`); scope is dossier-in, `design_brief`-out only; refuses to write `design_brief` content until the dossier's `approval_valid` is true; never calls a schematic-, board-, or library-*mutating* tool — loading the `library` toolset exposes all 17 of its tools and there is no partial-toolset-loading mechanism, so this restriction is a Hard Rule in the agent file (design D9, closing the planner's deferred finding); ends by returning the five-field block (`stage`, `map_id`, `produced`, `verdict`, `blockers`) to the session, which is what invokes `kicad-schematic-build-agent`/`kicad-pcb-layout-agent` after approval #2.
Stack: none
Acceptance: `crates/konnect/assets/agents/pcb-design-reconstruction-agent.md` exists; its frontmatter `skills:` list contains only `konnect` and `kicad-design-reconstruction`; its Hard Rules name `search_symbols`/`search_footprints` as the only `library` tools it may call.

- [x] 4.3 Update `crates/konnect/assets/skills/konnect/SKILL.md`: change the "I have photos of a board" decision-tree row to route through `kicad-photo-to-board`, and add an "Agent Routing and Mutation Ownership" bullet naming `pcb-design-reconstruction-agent` and its scope, per design D9.
Stack: none
Acceptance: `grep -c "kicad-photo-to-board\|pcb-design-reconstruction-agent" crates/konnect/assets/skills/konnect/SKILL.md` is at least 2.

- [x] 4.4 Add a "Building from an approved design brief" section to `crates/konnect/assets/agents/kicad-schematic-build-agent.md`, mirroring its existing "Building from an approved photo-intake map" section: `load_photo_review_map` itself, proceed only when `approval_valid` is true, place one symbol per `bom` entry (matched via `search_symbols` using the entry's `kicad_symbol`), wire the `circuits` topology, per spec "approved design brief hands off to schematic and PCB build".
Stack: none
Acceptance: `grep -c "Building from an approved design brief" crates/konnect/assets/agents/kicad-schematic-build-agent.md` is 1.

- [x] 4.5 Add a "Building from an approved design brief" section to `crates/konnect/assets/agents/kicad-pcb-layout-agent.md`: `load_photo_review_map` itself, proceed only when `approval_valid` is true, treat `physical_constraints`'s rows as hard placement and outline constraints (design D5 maps them one-to-one onto this skill's own section-1 constraint record, `kicad-pcb/references/layout-methodology.md:30-38`), and report `INCOMPLETE` rather than choosing its own value when `physical_constraints.unresolved` contains `board_size_mm` or one of the four rows `layout-methodology.md:56-57` calls load-bearing (current, voltage, connector position, enclosure), per spec "approved design brief hands off to schematic and PCB build" and design D5.
Stack: none
Acceptance: `grep -c "Building from an approved design brief" crates/konnect/assets/agents/kicad-pcb-layout-agent.md` is 1.

- [x] 4.6 Run the full asset-guard suite and fix any failure surfaced by the additions in tasks 3.1-4.5.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references` passes, including `every_reference_is_reachable_from_its_parent_skill`, `agents_preload_existing_skills`, `top_level_skill_routes_every_bundled_agent`, `call_examples_name_real_parameters`, and `backticked_tool_names_in_prose_exist_in_the_registry`.

## 5. Manifest, NOT_TOOLS, doc counts, and the workflow doc page

- [x] 5.1 Update `crates/konnect/src/manifest.rs` to embed (via `include_str!`, following the existing `kicad-pcb`/`kicad-pcb-layout-agent` pattern): `kicad-board-dossier/SKILL.md` + `dossier-schema.md`, `kicad-design-reconstruction/SKILL.md` + `design-brief-schema.md`, `kicad-photo-to-board/SKILL.md`, and `pcb-design-reconstruction-agent.md`.
Stack: none
Acceptance: `grep -n "kicad-board-dossier\|kicad-design-reconstruction\|kicad-photo-to-board\|pcb-design-reconstruction-agent" crates/konnect/src/manifest.rs` shows all four new `include_str!` entries.

- [x] 5.2 Run `cargo test -p konnect --test asset_references backticked_tool_names_in_prose_exist_in_the_registry` and add every response/schema field name it flags (design D10 states the starting set) to `NOT_TOOLS` in `crates/konnect/tests/asset_references.rs` as one commented block; add only names the test actually flags, and do not add any top-level tool parameter (`image_path`, `project_dir`, `map_id`).
Stack: none
Acceptance: `cargo test -p konnect --test asset_references backticked_tool_names_in_prose_exist_in_the_registry` passes, and no name added is already a top-level property of a registered tool's input schema.

- [x] 5.3 Bump the "231"/"238" tool-count mentions to "232"/"239" by hand in `docs/TROUBLESHOOTING.md`, `packaging/metadata.json`, and `plugin/plugin.json` (not covered by `doc_tool_counts.rs`, per design D10 and the playbook's step 7); the registry-derived counts in README.md/DEV.md/tool-directory.md are verified by task 1.6's registry change, not edited by hand.
Stack: none
Acceptance: `grep -rn "231\|238" docs/TROUBLESHOOTING.md packaging/metadata.json plugin/plugin.json` returns no match.

- [x] 5.4 Add a `prepare_board_photo` row to `tool-directory.md`'s `photo_intake` section and change its header from "5 tools" to "6 tools".
Stack: none
Acceptance: `cargo test -p konnect --test doc_tool_counts` exits 0 (all checks pass, including `tool_directory_lists_every_registered_tool`), and `grep -c "prepare_board_photo" tool-directory.md` is at least 1.

- [x] 5.5 Write `docs/PHOTO_TO_BOARD_WORKFLOW.md` per design D11 (pipeline stages, artifact trail, the two approval points, honest limits) and link it from `README.md` near the existing Freerouting/kicad-cli external-dependency note.
Stack: none
Acceptance: `docs/PHOTO_TO_BOARD_WORKFLOW.md` exists, and `grep -c "PHOTO_TO_BOARD_WORKFLOW.md" README.md` is at least 1.

## 6. Orchestrator acceptance checklist

- [x] 6.1 With the change fully implemented, the orchestrator runs `pcb-photo-intake-agent`'s comprehension phase against the real reference photos (`C:\Users\felip\Downloads\WhatsApp Unknown 2026-09-18 at 12.00.58\*.jpeg`, never copied into the repo) and scores the produced `dossier` against design D12's eight-item checklist, each item measured against the worked example in `.orchestrator/handoffs/board-dossier-reconstruction/00-orchestrator-dossier-prototype.md`: (1) `identity.summary` carries the silkscreen identity string `SEMAFARO 1.3 24V 03/2020` and `identity.evidence[0]`'s `view` exists on disk with `rect_px` inside its bounds; (2) `24V` appears in `silkscreen_markings[].text` with `basis: "observed"`; (3) the LED-class `component_survey.count` is within ±10 % of 107 (board A) / 122 (board B), i.e. 97-118 / 110-135, with a non-empty `count_method`; (4) the axial-resistor class counts 19 on board A with `locations[]` summing to 19; (5) `physical.mounting_holes` totals 4 plated corner holes, each with a `position_px`; (6) `physical.connectors` has one 2-pin screw terminal on the bottom edge; (7) at least one `topology_claims[].hypotheses[]` entry has a non-empty `calculation` naming a resistor value and a current, plus a non-empty `assumptions`; (8) with no scale supplied, `physical.board_size_mm` is `null`, `scale_status` says so, the gap is in `open_questions`, and `scale_reference` carries no `mm_per_px`.
Stack: none
Acceptance: all eight checklist items in this task's description are individually scored pass/fail against the produced `dossier` and recorded in the orchestrator's own handoff for this task; there is no automated command for this task. If items 3 or 4 fail because the photo set differs from the worked example's, the runner records the manual count it used as the new reference rather than widening the tolerance.

## 7. Final gates

- [x] 7.1 Run `cargo fmt --check` across the workspace and fix any formatting drift introduced by this change.
Stack: none
Acceptance: `cargo fmt --check` exits 0.

- [x] 7.2 Run `cargo clippy --all-targets` and resolve any new warning introduced by this change.
Stack: none
Acceptance: `cargo clippy --all-targets` exits 0 with no warnings attributable to files touched in this change.

- [x] 7.3 Run the full truth-command gate.
Stack: none
Acceptance: `cargo test -p konnect` and `cargo test -p konnect-core` both exit 0.
