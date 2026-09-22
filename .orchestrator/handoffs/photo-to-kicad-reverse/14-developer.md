---
change: photo-to-kicad-reverse
task: "fix round 2"
agent: developer
verdict: DONE
failing_layer: none
---

## Result

Both SERIOUS findings of reviewer 13, the code-side MINOR and the asset guard are
closed on top of `39d4b4d`, four commits, every gate green. The pinned digest
`18afc7b9…` is unchanged and passing — the canonical object I hash is the one
D16 describes.

1. **SERIOUS 1 — annotation revoked approval.** `7ae849e`. `review_map_content_hash`
   now projects each covered key onto the design D8 fields of its own shape
   (`COMPONENT_CONTENT_KEYS`, `NET_CONTENT_KEYS`, `SCALE_REFERENCE_CONTENT_KEYS`)
   instead of cloning the subtree, with D1's `""` → `null` applied to `value` and
   `footprint_suggestion` so the value retrace wrote and the value `save` persists
   are one value to the gate. `design.md:98`/`:159` are now true. A non-object
   shape is hashed verbatim (only reachable from `approval_is_valid` on a
   hand-broken file, where no digest can match anyway).
2. **SERIOUS 2 — forged reserved names persisted.** `676c8ce`. `TOOL_OWNED_KEYS`
   (`approval_valid`, `approved`, `approved_at`, `content_hash_at_approval`,
   `saved_at`) are removed from the incoming `map` before the overlay, so the
   record states only what the server decided and `load` can never answer with two
   `approval_valid` keys. Ignored rather than rejected — a map that came from
   `load` legitimately carries four of them — and both the tool description and
   the `map` schema's description now say so.
3. **MINOR — `check_retrace` vs `scan_pcb_photo`.** `12a2545`. The probe now runs
   its `project_dir` through `canonical_existing_dir` exactly as the scan does, so
   the two cannot resolve different config files, and the new test drives the two
   handlers rather than the resolver they share. Same commit: the doc comment that
   named the renamed `the_map_schema_names_every_review_map_field` test, and
   `tool-directory.md`'s `get_effective_config` row (now "built-in defaults, then
   user defaults, then project overrides").
4. **Asset guard.** `3d77ed5`. `agents_make_claimed_evidence_executable` requires
   `load_toolset("photo_intake")` and the `approval_valid` marker for
   `kicad-schematic-build-agent.md`, so the section `spec.md:57` cites by name is
   now executable. The agent asset itself needed no change — it already has both.
5. **Not done, deliberately:** the three doc-layer MINORs the reviewer listed
   (`design.md:81`'s "no behavior change", `spec.md:53`'s vacuous clause,
   `design.md:159`'s wording) are openspec artifacts, outside my write set.

## Evidence

All commands in `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\photo-to-kicad-reverse`,
branch `orc/photo-to-kicad-reverse`, HEAD `3d77ed5`, tree clean, nothing pushed.

1. **Gates at HEAD.** `cargo test -p konnect-core` → exit 0 (`1364 passed; 0 failed;
   16 ignored`, plus `4`, `12`, `5 passed; 1 ignored`, `0 passed; 1 ignored`) ·
   `cargo test -p konnect` → exit 0 (15 result lines, incl. `asset_references` 12,
   `doc_tool_counts` 6) · `cargo fmt --check` → exit 0 · `cargo clippy --all-targets`
   → 0 lines matching `^(warning|error)`, exit 0. With `RETRACE_PYTHON` set:
   `scan_pcb_photo_runs_a_real_retrace_scan ... ok` (4.16 s) and
   `a_real_scan_feeds_the_gate_and_leaves_the_home_directory_alone ... ok` (1.19 s).
   `the_content_hash_of_the_fixture_map_is_pinned ... ok`, literal untouched.
2. **Finding 1 — red → green.** New: `unknown_keys_are_outside_the_content_hash_at_every_depth`
   and `an_empty_string_hashes_as_the_null_save_writes_for_it` (`photo_intake.rs`),
   `annotations_added_after_approval_keep_the_approval_valid`
   (`photo_intake_gate_e2e.rs`, which annotates *after* `approve` — the ordering
   `out_of_schema_keys_survive_a_save_and_load_round_trip` cannot reach — and then
   edits one schema field to prove revocation still works). Before:
   `a per-component note must not move the content hash / left: "f0f38de0…" right:
   "18afc7b9…"`; `"" and null are the same reading / left: "18afc7b9…" right:
   "788fbeb3…"`; `an annotation is not review content … left: Bool(false) right:
   Bool(true)`. After: all ok. **Mutation:** replacing the `"components"` arm with
   `field.cloned().unwrap_or(Null)` (the old whole-subtree clone) failed exactly
   those three tests while the pinned digest stayed ok; restored from a copy taken
   first, suite green.
3. **Finding 2 — red → green.** New: `a_payload_cannot_forge_its_own_approval`.
   It approves a map under one `map_id`, then saves the *same content* under a
   never-approved `map_id` carrying `approved: true`, `approval_valid: true`,
   `approved_at` and that genuine digest (`map_id` is outside the hash, so the
   stolen hash is arithmetically correct for this content). Before:
   `approval_valid is computed by load and never persisted: left: Some(Bool(true))
   right: None` — printing the persisted map with `"approval_valid":true` in it,
   which is the naive "persist the incoming value" behaviour this test forbids.
   After: ok, with `load` → `approval_valid: false` and the stored object's
   `approv`-matching keys asserted to be exactly
   `["approved", "approved_at", "content_hash_at_approval"]`, `approved` false.
4. **MINOR — red → green.** New: `check_retrace_rejects_a_project_dir_the_scan_would_reject`
   drives `handle_check_retrace` and `handle_scan_pcb_photo` with the same missing
   directory and asserts both error texts are equal and name `'project_dir' does not
   resolve`. Before: `check_retrace` returned a *successful* capability report
   (`{"available":false,…,"candidates_tried":[…]}`) — it had silently probed with
   the wrong config. After: ok. `cargo test -p konnect --test doc_tool_counts` → 6
   passed after the `tool-directory.md` edit.
5. **Asset guard — mutation, because it was green on the first run.** Renaming
   `approval_valid` → `approved_flag` and `load_toolset("photo_intake")` →
   `load_toolset("project")` in `kicad-schematic-build-agent.md` made
   `agents_make_claimed_evidence_executable` fail naming both:
   `agents/kicad-schematic-build-agent.md does not load 'photo_intake'` /
   `does not prescribe 'approval_valid'`. Asset restored from a copy; `git diff` on
   it is empty and all 12 asset guards pass.

## For the next agent

1. **Re-derive the pinned digest if you doubt it.** It is unchanged because the
   fixture map carries no out-of-schema key and every D8 field is present, so the
   projection reproduces the old object byte for byte. If you change any of the
   three `*_CONTENT_KEYS` lists, that test is the one that must be re-derived in a
   second tool (Python `json.dumps(…, sort_keys=True, separators=(',',':'))`), not
   pasted from the failure message.
2. **`every_schema_key_is_either_hashed_or_deliberately_not` only guards the top
   level.** A new field added *inside* a component or net is silently unhashed
   until it is added to `COMPONENT_CONTENT_KEYS`/`NET_CONTENT_KEYS`. That is D16's
   trade-off pushed one level down; there is no test forcing the decision there.
3. **`check_retrace` now errors on a `project_dir` that does not resolve**, where
   before it fell through to the server's configured project. Its description
   still says "Absence is reported as a field, never as an error" — that sentence
   is about retrace, not about the path, but a reader could take it either way.
4. **Three doc-layer MINORs from reviewer 13 remain open** and I could not touch
   them: `design.md:81` ("no behavior change to the existing tools", contradicted
   by `design.md:85` and by `handle_get_effective_config`), `spec.md:53`'s vacuous
   "never report `approval_valid: true`" clause for `save`, and `design.md:159`,
   whose sentence is now true but still says "the four D16-hashed keys" without
   mentioning the per-element projection.
5. **Verification recipe:** `cargo test -p konnect-core`, `cargo test -p konnect`,
   `cargo fmt --check`, `cargo clippy --all-targets`, then the two `--ignored`
   tests with `RETRACE_PYTHON`. Env for every cargo call:
   `PROTOC=C:/Users/felip/tools/protoc/bin/protoc.exe` and the VS 2022 BuildTools
   CMake `bin` on `PATH`.

## Deferred findings

1. `portable_uri` (`crates/konnect-core/src/tools/library.rs:1818-1821`) still has
   the UNC defect `subprocess_arg:277-292` lost. Unchanged since round 1.
2. `read_config` (`config.rs:111-116`) still substitutes defaults for a config file
   that exists but does not parse, so a typo reads as "no preferences set".
3. `install.rs`'s `claude_installs_the_canonical_reliability_contract_offline` still
   names only two agents, while `pcb-photo-intake-agent.md:27` reads the contract.
4. `agents_make_claimed_evidence_executable` is still a hard-coded marker list per
   agent — I extended it rather than making it derive markers from the file. A new
   section in any other agent still gets no guard for free.
5. Task 6.2's `grep -c "photo_intake" tool-directory.md >= 6` remains unreachable;
   my edit to that file was to a different row and did not change the count.
