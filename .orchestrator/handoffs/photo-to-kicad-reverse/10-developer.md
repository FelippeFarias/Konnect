---
change: photo-to-kicad-reverse
task: "fix round 1"
agent: developer
verdict: DONE
failing_layer: none
---

## Result

All eight findings closed in the worktree on top of `1fd059e`, nine commits, every
gate green. Each finding has a test that failed before the change and passes after.

1. **(a) QA BLOCKER — `save_photo_review_map` uncallable.** `65aa984` spells the
   `map` argument's nested `properties` out in a new `review_map_schema()` (in sync
   with `PhotoReviewMap`/D8) and declares `"additionalProperties": true` on the map,
   each component and each net, which `close_input_schema`'s `entry().or_insert()`
   preserves. QA's pinned `save_photo_review_map_accepts_a_real_review_map` is
   un-ignored and the e2e harness's `call_bypassing_schema` is deleted, so `save`
   now routes through the compiled validator like the other four tools. A second
   fallout commit, `06ec920`, enrols the four open subschemas in
   `router/mod.rs`'s `fixed_records_are_closed_and_only_reviewed_maps_are_extensible`
   allowlist — that repo-wide guard is what decides the write set here, and it
   caught the schema change on the first full `-p konnect-core` run.
2. **(b) Reviewer 1 — UNC.** `1df5051`: `subprocess_arg` strips only `VerbatimDisk`
   outright, maps `\\?\UNC\srv\share\…` back to `\\srv\share\…`, and leaves any
   other verbatim form (volume GUID, device path) untouched.
3. **(c) Reviewer 2 — `used_fallback`.** `a705b37`: now
   `yolo_warning_seen || ocr_warning_seen || !extras.detection || !extras.ocr`,
   matching the biconditional the three asset files publish; `fallback_evidence`
   still carries all four raw signals. The live test's tautological
   `used_fallback == yolo || !detection` re-derivation is replaced by a real pin on
   D11's unversioned stderr literal (`!detection` ⇒ the `YOLO not available`
   marker must still appear).
4. **(d) Reviewer 3 — interpreter reporting and config source.** `4a16375`:
   `RetraceCapability` carries `candidates_tried`; `build_scan_response` (extracted
   so the shape is assertable without Python) emits `python_path` and
   `candidates_tried` exactly as `check_retrace` does; `check_retrace` gains an
   optional, schema-documented `project_dir`; both handlers resolve the config
   project through one `config_project_dir(argument, ctx.config)`. D15's discovery
   order is untouched.
5. **(e) Reviewer 4 — dropped annotations.** `cefeeaf`: `save` keeps
   `PhotoReviewMap` as the *validator* and lays the normalized value over the
   incoming one (`overlay_known_fields`), so D8's shape and D1's `""`→`null`
   normalization still win on the struct's keys while every other key — top-level,
   per-component, per-net — is carried through. The content hash still covers only
   the four D16 keys.
6. **(f) Reviewer 6 — dead knob.** `b9b09b2` + `39d4b4d`: `retrace_extras_expected`
   removed from `default_user_config`, its assertion, `NOT_TOOLS`, `SKILL.md:37`
   and every prose mention. The default-config test now pins the exact key set, so
   the next unread knob fails rather than ships.
   `grep -rn retrace_extras_expected <worktree>` returns nothing.
7. **(g) MINORs.** `be9511c`: `handle_get_effective_config` and `effective_config`
   share one `layer_configs`; the `part_number` prose is corrected in all three
   assets (**prose fix chosen, not a schema field** — `value` stays empty and
   retrace's `part_number` stays in `analysis.json` as evidence; adding the field
   would widen the hashed content for data no tool reads); both assets now tell the
   agent to populate the required `source_images` before the first save; the
   misplaced synthetic-board doc comment moved to `mod synthetic_board`.
8. **(h) Consumer side.** `9610e41` adds "Building from an approved photo-intake
   map" to `kicad-schematic-build-agent.md`: loads `photo_intake`, calls
   `load_photo_review_map(project_dir, map_id)` itself, proceeds only on
   `approval_valid` (never the map's own `approved`), places one real library symbol
   per `approved: true` component matched by `type`/`value` through
   `search_symbols`, wires only the map's `ref`-to-`ref` `connections` with
   `sch_wiring`/`sch_batch`, refuses retrace's synthetic netlist, and reports ERC as
   usual. All 12 asset guards and the offline-install test stay green.

## Evidence

All commands in `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\photo-to-kicad-reverse`,
branch `orc/photo-to-kicad-reverse`, HEAD `39d4b4d`. Nothing pushed.

1. **Gates, at HEAD.** `cargo test -p konnect-core` → exit 0
   (`1361 passed; 0 failed; 16 ignored`, plus `4`, `12`, `3 passed; 1 ignored`
   for the e2e binary, `0 passed; 1 ignored`) · `cargo test -p konnect` → exit 0
   (14 `ok` result lines, incl. `asset_references` 12, `doc_tool_counts` 6,
   `not_tools_allowlist` 3) · `cargo fmt --check` → exit 0 ·
   `cargo clippy --all-targets` → 0 lines matching `^(warning|error)`.
2. **Live, `RETRACE_PYTHON` set.**
   `cargo test -p konnect-core photo_intake -- --ignored` →
   `scan_pcb_photo_runs_a_real_retrace_scan ... ok` (1 passed, 2.60 s) ·
   `cargo test -p konnect-core --test photo_intake_gate_e2e -- --ignored` →
   `a_real_scan_feeds_the_gate_and_leaves_the_home_directory_alone ... ok`
   (1 passed, 1.28 s). The live run also exercises the new `python_path` /
   `candidates_tried` assertions and the stderr-marker pin (this machine has
   neither extra, so `!detection` holds and `yolo_warning_seen` was true).
3. **Red→green per finding.** (a) before: `save_photo_review_map_accepts…` FAILED,
   printing `"map": { "additionalProperties": false, "type": "object" }` and
   `False schema does not allow true`; after: ok. (b) before:
   ``left: "UNC\\srv\\share\\proj\\top.png"`` vs ``right: "\\\\srv\\…"``; after: ok.
   (c) before: `OCR never ran, so marking/value/part_number were never attempted:
   {"extras_detection":true,"extras_ocr":false,…}`; after: ok. (d) before: three
   `cannot find function config_project_dir` / `no field candidates_tried` compile
   errors, then `left: ["project_dir","python_path"] right: ["python_path"]` on the
   superseded `check_schema_offers_only_python_path`; after: ok. (e) before:
   `a top-level annotation was dropped by save:` with the whole persisted map
   printed and no `reviewer_note` in it; after: ok. (f) before:
   `left: ["retrace_extras_expected","retrace_python_path","retrace_timeout_seconds"]`;
   after: ok. (g) before: `cannot find function layer_configs`; after: ok.
4. **Mutation proof for (e)'s second half.** Replacing
   `overlay_known_fields(map_arg, serde_json::to_value(&parsed)?)` with the literal
   `map_arg.clone()` (the naive "persist the incoming Value") made
   `out_of_schema_keys_survive_a_save_and_load_round_trip` fail on
   `preserving unknown keys must not stop the schema keys being normalized`
   (`""` was persisted instead of `null`). Restored from a copy taken first;
   suite green after.
5. **Superseded tests, rewritten not restored.** `check_schema_offers_only_python_path`
   → `check_schema_offers_the_interpreter_and_the_project_whose_config_names_one`
   and `default_photo_intake_config_is_present_with_documented_values` →
   `default_photo_intake_config_is_exactly_the_keys_something_reads`; both doc
   comments name the finding that superseded them. New tests:
   `the_map_schema_names_every_review_map_field_and_stays_open`,
   `used_fallback_is_true_whenever_any_extra_did_not_run`,
   `the_scan_response_names_the_interpreter_that_ran_just_as_check_retrace_does`,
   `both_handlers_resolve_the_config_project_the_same_way`,
   `layering_puts_built_in_defaults_under_a_config_file_that_predates_a_key`,
   `out_of_schema_keys_survive_a_save_and_load_round_trip`.
   Greps: `grep -rn retrace_extras_expected .` → nothing ·
   `grep -c approval_valid crates/konnect/assets/agents/kicad-schematic-build-agent.md`
   → 2. Diff vs `1fd059e`: 9 files, +818 / -134.

## For the next agent

1. **Re-check the schema fix at the boundary, not the handler.** The defect existed
   because all unit tests called `handle_*` directly. `photo_intake_gate_e2e.rs` now
   routes *every* call through `def.input_validator.validate` → `def.handler`; the
   quickest confirmation is to delete `"additionalProperties": true` from
   `review_map_schema()` and watch both `save_photo_review_map_accepts…` and
   `out_of_schema_keys_survive…` go red.
2. **`fixed_records_are_closed_and_only_reviewed_maps_are_extensible`
   (`router/mod.rs:184`) is the guard that now licenses the open record.** Its
   allowlist carries four new paths with the reason. Judge that reason, not just
   the test's green: it is the only thing standing between "hand annotations
   survive" and "any garbage is accepted into the map argument".
3. **`used_fallback` is now strictly more conservative than D11's written
   expression.** D11 line ~189 still says `yolo_warning_seen || !extras.detection`.
   The code, the three assets and the tests now agree on the four-signal form; the
   design text is the thing that is stale. Worth a one-line design amendment at
   archive time rather than a code revert.
4. **Two things I did not do, both deliberate.** The write set named
   `asset_references.rs` "NOT_TOOLS block only", so the new
   `kicad-schematic-build-agent.md` section has **no executable guard** — nothing
   keeps it from being deleted (see Deferred 1). And reviewer finding 5 / QA
   finding 3 (the spec's "SHALL block any tool that would mutate a KiCad project")
   remains a spec-layer decision: the gate is now stated on both sides in prose, but
   no `sch_*`/`pcb_*` handler reads a review map.
5. **The live tests still need `RETRACE_PYTHON`** and still fail loudly rather than
   skipping when it is unset. `photo_intake` and `photo_intake_gate_e2e` each own
   exactly one `#[ignore]`d test; nothing else in the change is ignored.

## Deferred findings

1. `agents_make_claimed_evidence_executable` (`asset_references.rs:175`) hard-codes
   a marker list per agent. Adding `approval_valid` to the
   `kicad-schematic-build-agent.md` case would make finding (h) permanent for the
   cost of one array entry; it was outside my declared write set.
2. `portable_uri` (`library.rs:1818-1821`) has the same UNC defect
   `subprocess_arg` just lost, and predates this change (reviewer Deferred 1). The
   two-branch shape in `subprocess_arg` is now a working precedent to copy.
3. `install.rs`'s `claude_installs_the_canonical_reliability_contract_offline`
   still names only two agents; `pcb-photo-intake-agent.md:27` reads the contract
   but nothing keeps that true (QA Deferred 3, unchanged).
4. `read_config` (`config.rs:111`) still substitutes defaults for a config file
   that exists but does not parse, so a typo reads as "no preferences set". Now
   also on `photo_intake`'s path, and `layer_configs` did not change it.
5. Task 6.2's `grep -c "photo_intake" tool-directory.md >= 6` is still unreachable
   (QA finding 2). Untouched: it is an intent-layer criterion for `@pm`/`@po`, not
   a file to pad.
