Implementation worktree: `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\photo-to-kicad-reverse` (branch `orc/photo-to-kicad-reverse`, base `ea74faf`). All file paths below are relative to that worktree's repo root. Truth commands: `cargo test -p konnect`, `cargo test -p konnect-core` (with `PROTOC=C:/Users/felip/tools/protoc/bin/protoc.exe` and the VS 2022 BuildTools CMake extension on `PATH`); lint gate: `cargo fmt --check`, `cargo clippy --all-targets`. `stacks: none` for this project — every task below is `Stack: none`.

## 1. Subprocess runner, config keys, capability probe

- [x] 1.1 Add a `"photo_intake"` config object (`retrace_python_path: null`, `retrace_timeout_seconds: 120`) to `default_user_config()` in `crates/konnect-core/src/tools/config.rs`, per design D3/D12.
Stack: none
Acceptance: `default_user_config()` in `crates/konnect-core/src/tools/config.rs` returns a `"photo_intake"` key with both fields; `cargo test -p konnect-core config::` passes.

- [x] 1.2 Create `crates/konnect-core/src/tools/photo_intake.rs` with the typed structs `RetraceAnalysis`, `RetraceComponent`, `RetraceTrace`, `RetracePatternMatch` (per design D1/D8), including the `empty_string_as_none` deserializer on `marking`/`part_number`/`datasheet_url`/`value`/`package` — retrace writes `""`, not `null`, for absent fields.
Stack: none
Acceptance: `crates/konnect-core/src/tools/photo_intake.rs` exists, is declared in `crates/konnect-core/src/tools/mod.rs`, and `cargo build -p konnect-core` succeeds.

- [x] 1.3 Implement the `run_retrace` subprocess runner in `photo_intake.rs`: `tokio::process::Command`, `HOME` **and** `USERPROFILE` redirected to a scoped-per-call directory, `kill_on_drop(true)`, the D12 timeout, and a local stdout/stderr diagnostics helper (`cli_failure_diagnostics` in `cli.rs:25` is private and is not promoted), per design D1/D4/D12.
Stack: none
Acceptance: a unit test in `crates/konnect-core/src/tools/photo_intake.rs` asserts a subprocess invoked through `run_retrace` never writes under the real test-process `HOME`/`USERPROFILE` (spec scenario "scan does not pollute the real user's home directory").

- [x] 1.4 Implement the `check_retrace` tool: resolves the interpreter in design D15's order (`python_path` arg > config > `RETRACE_PYTHON` > `py -3` on Windows > `python3` > `python`), probes with `<python> -c "import retrace, sys; print(retrace.__version__)"` at a 10 s timeout, reports `available`, the winning `python_path`, `retrace_version`, `extras` (`detection`, `ocr`), and never returns an error result for absence, per spec `photo-intake`'s "retrace capability probe" requirement and design D4/D15.
Stack: none
Acceptance: unit tests in `crates/konnect-core/src/tools/photo_intake.rs` cover the three spec scenarios under "retrace capability probe" (fully installed, missing, base-only-no-extras); `cargo test -p konnect-core photo_intake::` passes.

- [x] 1.5 Add `pub(crate) async fn effective_config(project_dir: Option<&Path>) -> serde_json::Value` to `crates/konnect-core/src/tools/config.rs`, built from the existing private `read_config`/`default_user_config`/`default_project_config`/`deep_merge` helpers, and read `photo_intake.*` through it from `photo_intake.rs`, per design D3. `config.rs` exposes only `pub fn tools()` today, so there is no other way for a Rust module to read configuration.
Stack: none
Acceptance: `grep -n "pub(crate) async fn effective_config" crates/konnect-core/src/tools/config.rs` matches, a unit test asserts user-config values are overridden by project-config values for a `photo_intake.*` key, and no existing `config` tool changes behavior (`cargo test -p konnect-core config::` passes).

## 2. Photo scan tool and typed parsing

- [x] 2.1 Implement the `scan_pcb_photo` tool: runs `<python> -m retrace scan <image> --format json -o <dir>` via `run_retrace` with `<dir>` computed (never supplied) as `<canonical project_dir>/.konnect/photo_intake/<map_id>/`, and reads back `analysis.json`, per spec "photo scan produces a typed, project-scoped analysis" and design D2/D13/D15. The planner's `output_dir` argument is deleted, not validated.
Stack: none
Acceptance: `crates/konnect-core/src/tools/photo_intake.rs` defines `handle_scan_pcb_photo`; its input schema requires `image_path` and `project_dir`, offers only `python_path` and `timeout_seconds` beyond them, and has no `output_dir` property (design D2's table).

- [x] 2.2 Parse `analysis.json` into `RetraceAnalysis`/`RetraceComponent`/`RetraceTrace`/`RetracePatternMatch`, treating every field the captured fixture does not guarantee as `Option<...>` (design D1).
Stack: none
Acceptance: a unit test deserializes the checked-in fixture and asserts it round-trips through `RetraceAnalysis`, and a second test asserts a component whose `marking`/`value`/`part_number` are `""` in JSON deserializes to `None`, not `Some("")`.

- [x] 2.3 Add the fixture used by 2.2 as `crates/konnect-core/tests/fixtures/photo_intake/analysis.json`, copied verbatim from the real Windows capture at `openspec/changes/photo-to-kicad-reverse/fixtures/analysis-windows-smoke.json` (retrace 0.3.0; 4 components, 2 `pattern_matches`, empty `traces`, `""` for absent strings) — no `retrace` install required to run this test.
Stack: none
Acceptance: `crates/konnect-core/tests/fixtures/photo_intake/analysis.json` exists and matches the captured file byte-for-byte; `cargo test -p konnect-core photo_intake::` passes without a Python interpreter present.

- [x] 2.4 Add an `#[ignore = "requires Python with retrace installed (set RETRACE_PYTHON)"]` integration test that runs a real `retrace scan` against an in-test-synthesized PNG (design D7) when `RETRACE_PYTHON` resolves to an interpreter with `retrace` installed, asserting `scan_pcb_photo` returns within its timeout and that `used_fallback` is derived per design D11 (stderr marker `WARNING YOLO not available` OR the extras probe reporting `detection: false`).
Stack: none
Acceptance: `cargo test -p konnect-core photo_intake:: -- --ignored` runs this test to completion on a machine with `retrace` installed and asserts both `used_fallback` and the `fallback_evidence` sub-fields; `cargo test -p konnect-core` (without `--ignored`) does not require it.

- [x] 2.5 Add `image.workspace = true` to `crates/konnect-core`'s `[dev-dependencies]` and write the `#[cfg(test)]` synthetic-board PNG helper used by 2.4, per design D7. `image` is already in this crate's tree via `konnect-render` (root `Cargo.toml:90`), so this adds no new crate to the build.
Stack: none
Acceptance: `crates/konnect-core/Cargo.toml` lists `image.workspace = true` under `[dev-dependencies]`, the helper produces a readable PNG in a `tempfile` directory, and `cargo test -p konnect-core` still compiles without a Python interpreter present.

## 3. Review-map persistence and approval gate

- [x] 3.1 Define `PhotoReviewMap`, `ReviewComponent`, `ReviewNet`, `ScaleReference` structs in `photo_intake.rs` matching design D8's canonical JSON shape, including the optional `subcircuit_hints: Vec<RetracePatternMatch>` field (design D14) and `saved_at`.
Stack: none
Acceptance: `cargo build -p konnect-core` succeeds with these structs `Serialize`/`Deserialize`-derived and referenced by the tools added in 3.2-3.4; `subcircuit_hints` is optional and absent-tolerant.

- [x] 3.2 Implement `save_photo_review_map`: validates `project_dir` and `map.map_id` per design D13, writes `<project_dir>/.konnect/photo_intake/<map_id>/review_map.json`, and writes `approved: true` only when `review_map_content_hash(&map)` equals the stored `content_hash_at_approval` — otherwise `approved: false`, ignoring any client-supplied `approved` field, per spec "persisted, human-editable review map" and design D5/D16.
Stack: none
Acceptance: unit tests cover the spec scenarios "a freshly scanned map persists as editable JSON" and "low-confidence components are flagged, never auto-corrected", plus a test that a client-supplied `"approved": true` in the payload is overwritten.

- [x] 3.3 Implement `load_photo_review_map`, returning the persisted map unchanged, including manual out-of-band edits to the JSON file. `map_id` is validated against `^[A-Za-z0-9_-]{1,64}$` in both the JSON Schema and Rust before any path join (design D13).
Stack: none
Acceptance: unit test covers spec scenario "the review map survives across sessions" (save, mutate the file directly, load, assert the mutation is visible), plus a test that `map_id` values containing `..`, `/` or `\` are rejected with an error and touch no filesystem path.

- [x] 3.4 Implement `approve_photo_review_map`: sets `approved: true`, records `approved_at` (RFC 3339 UTC) and `content_hash_at_approval` from `review_map_content_hash`, per spec "hard human-review gate before any KiCad mutation" and design D5/D16.
Stack: none
Acceptance: unit tests cover spec scenarios "approval requires an explicit call" and "editing an approved map revokes approval".

- [x] 3.5 Add a unit test asserting `save_photo_review_map` alone (without `approve_photo_review_map`) never produces `approved: true`, per spec scenario "unapproved map cannot reach schematic build".
Stack: none
Acceptance: `cargo test -p konnect-core photo_intake::` includes and passes a test named to reflect this (e.g. `save_alone_never_approves`).

- [x] 3.6 Implement `pub(crate) fn review_map_content_hash(map: &serde_json::Value) -> String` per design D16: SHA-256 lowercase hex over `serde_json::to_vec` of a fresh object holding only `source_images`, `scale_reference`, `components`, `nets` — excluding `map_id`, `saved_at`, `approved`, `approved_at`, `content_hash_at_approval` and `subcircuit_hints` — with a comment stating the dependency on `serde_json`'s `preserve_order` feature being off.
Stack: none
Acceptance: unit tests assert (1) the digest of the checked-in fixture-derived map equals a pinned hex literal, (2) changing `saved_at` or `subcircuit_hints` leaves the digest unchanged, and (3) changing any `components`/`nets` field changes it.

## 4. Toolset registration

- [x] 4.1 Register the `photo_intake` toolset: add a `ToolsetMeta` entry to `ALL_TOOLSETS` in `crates/konnect-core/src/router/registry.rs` (category `"integration"`, `tool_count: 5` — the struct itself lives at `crates/konnect-core/src/router/mod.rs:20`) and a `"photo_intake" => Some(photo_intake::tools())` arm in `build_tools_for`, per design D2.
Stack: none
Acceptance: `grep -n "photo_intake" crates/konnect-core/src/router/registry.rs` shows both the `ALL_TOOLSETS` entry and the `build_tools_for` match arm, and `cargo test -p konnect-core registry_tool_counts_match_reality` passes (the hand-written `tool_count` is asserted against `tools_for()` at `router/mod.rs:335`).

- [x] 4.2 Verify every tool in `photo_intake::tools()` compiles a valid JSON Schema (via the existing `tool!` macro's `compile_input_validator`) and that required/optional fields match design D2's table exactly. All five use the 4-argument `tool!(name, desc, schema, handler)` form with no `.with_board_access(...)` call — `BoardAccess::None` is the `#[default]` (design D2).
Stack: none
Acceptance: `cargo build -p konnect-core` succeeds (a schema compile failure panics at `tool_catalogue()` init, which a `cargo test -p konnect-core` run that touches `tools_for("photo_intake")` will surface).

- [x] 4.3 Add an integration test that loads the `photo_intake` toolset through `ToolRouter`/`tools_for` and asserts it returns exactly the 5 tools named in design D2 (`check_retrace`, `scan_pcb_photo`, `save_photo_review_map`, `load_photo_review_map`, `approve_photo_review_map`).
Stack: none
Acceptance: `cargo test -p konnect-core` includes and passes a test asserting `registry::tools_for("photo_intake")` returns a 5-element `Vec` with those exact names.

## 5. Skill, agent, and manifest wiring

- [x] 5.1 Write `crates/konnect/assets/skills/kicad-photo-intake/SKILL.md` documenting the `check_retrace` → `scan_pcb_photo` → build-map → `save_photo_review_map` → user-review → `approve_photo_review_map` → handoff flow, the hard approval gate, confidence-flagging (<0.6), that the scale reference is always user-supplied, and that `subcircuit_hints` are advisory and never evidence (design D9/D14).
Stack: none
Acceptance: `crates/konnect/assets/skills/kicad-photo-intake/SKILL.md` exists and every `load_toolset('photo_intake')` example in it lists only tool names from design D2's table in its trailing comment.

- [x] 5.2 Write `crates/konnect/assets/skills/kicad-photo-intake/references/review-map-schema.md` (design D8's schema, field-by-field, with the `subcircuit_hints` entry stating it is advisory and outside the content hash), and name `review-map-schema.md` inside `SKILL.md`'s text.
Stack: none
Acceptance: `crates/konnect/assets/skills/kicad-photo-intake/references/review-map-schema.md` exists and `grep -c "review-map-schema.md" crates/konnect/assets/skills/kicad-photo-intake/SKILL.md` is at least 1.

- [x] 5.3 Write `crates/konnect/assets/agents/pcb-photo-intake-agent.md` with YAML frontmatter matching the house shape (`name`, `description` with triggers, `model`, `skills: [konnect, kicad-photo-intake]`, `tools: [mcp__konnect__*]`, `maxTurns`), scoped to photo-to-approved-map only, explicitly handing off to `kicad-schematic-build-agent` and never itself calling a schematic-mutating tool, per design D9.
Stack: none
Acceptance: `crates/konnect/assets/agents/pcb-photo-intake-agent.md` exists; its frontmatter `skills:` list contains only `konnect` and `kicad-photo-intake`.

- [x] 5.4 Update `crates/konnect/assets/skills/konnect/SKILL.md`'s "Agent Routing and Mutation Ownership" section with a bullet naming `pcb-photo-intake-agent` and its scope, per design D9.
Stack: none
Acceptance: `grep -c "pcb-photo-intake-agent" crates/konnect/assets/skills/konnect/SKILL.md` is at least 1.

- [x] 5.5 Update `crates/konnect/src/manifest.rs` to embed `kicad-photo-intake/SKILL.md`, its `review-map-schema.md` reference, and `pcb-photo-intake-agent.md` via `include_str!`, following the existing `kicad-pcb`/`kicad-pcb-layout-agent` pattern.
Stack: none
Acceptance: `grep -n "kicad-photo-intake\|pcb-photo-intake-agent" crates/konnect/src/manifest.rs` shows the new `include_str!` entries.

- [x] 5.6 Run the full asset-guard suite and fix any failure.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references` passes, including `every_reference_is_reachable_from_its_parent_skill`, `agents_preload_existing_skills`, `top_level_skill_routes_every_bundled_agent`, `documented_toolsets_exist_in_the_registry`, `tools_listed_beside_a_toolset_belong_to_it`, `call_examples_name_real_parameters`, and `backticked_tool_names_in_prose_exist_in_the_registry`.

- [x] 5.7 Add the new response and review-map field names to `NOT_TOOLS` in `crates/konnect/tests/asset_references.rs` as one commented block ("Structured photo-intake response and review-map fields, not callable tools"), per design D10: `used_fallback`, `fallback_evidence`, `retrace_version`, `analysis_json_path`, `duration_seconds`, `pattern_matches`, `subcircuit_hints`, `component_id`, `bbox_px`, `footprint_suggestion`, `source_images`, `scale_reference`, `approved_at`, `content_hash_at_approval`, `saved_at`, `part_number`, `retrace_python_path`, `retrace_timeout_seconds`. Top-level tool parameters (`python_path`, `image_path`, `map_id`, `timeout_seconds`) are exempted from the schemas and must NOT be added.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references backticked_tool_names_in_prose_exist_in_the_registry` passes, and no name added to `NOT_TOOLS` is already a top-level property of a registered tool's input schema.

## 6. Documentation

- [x] 6.1 Add a short note (new subsection or paragraph) to the project's top-level `README.md` (or `docs/` equivalent, matching wherever Freerouting's/kicad-cli's external-dependency note already lives) describing the new `photo_intake` toolset and its optional Python/`retrace` prerequisite (install with `pip install git+https://github.com/ericrihm/retrace.git`; point at `check_retrace` for diagnosis).
Stack: none
Acceptance: `grep -rn "photo_intake\|retrace" README.md` (or the matched docs file) returns at least one line added by this task.

- [x] 6.2 Update the registry tool/toolset counts quoted in `README.md`, `DEV.md` and `tool-directory.md` for the new `photo_intake` toolset (22 toolsets, 231 registered tools + 7 meta = 238), and add a `photo_intake` section to `tool-directory.md` listing its 5 tools, so the `crates/konnect/tests/doc_tool_counts.rs` guards pass.
Stack: none
Acceptance: `cargo test -p konnect --test doc_tool_counts` exits 0 (all five tests pass) and `grep -c "photo_intake" tool-directory.md` is at least 2.

## 7. Final gate

- [x] 7.1 Run `cargo fmt --check` across the workspace and fix any formatting drift introduced by this change.
Stack: none
Acceptance: `cargo fmt --check` exits 0.

- [x] 7.2 Run `cargo clippy --all-targets` and resolve any new warning introduced by this change.
Stack: none
Acceptance: `cargo clippy --all-targets` exits 0 with no warnings attributable to files touched in this change.

- [x] 7.3 Run the full truth-command gate.
Stack: none
Acceptance: `cargo test -p konnect` and `cargo test -p konnect-core` both exit 0.
