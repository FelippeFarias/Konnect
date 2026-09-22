---
change: photo-to-kicad-reverse
task: "spec revision, fix round 1"
agent: planner
verdict: DONE
failing_layer: n/a
---

## Result

1. Rewrote the gate requirement in `specs/photo-intake/spec.md` to say what this slice delivers: map tools never report `approval_valid: true` without an explicit `approve_photo_review_map` call whose content hash still matches, and the skill/two agents SHALL NOT invoke a mutating tool for a false `approval_valid` — server-side enforcement inside `sch_*`/`pcb_*` is now an explicit Non-Goal, deferred.
2. Rewrote all four "handoff to schematic build" scenarios (1 gate scenario + 3 handoff scenarios) so each names the exact verifying artifact: `kicad-schematic-build-agent.md`'s new "Building from an approved photo-intake map" section, plus one existing test (`load_reports_approval_valid_false_after_an_out_of_band_edit`) for the tool-level half of the gate scenario. The three pure-handoff scenarios are marked explicitly as prose-only, no automated test in this slice — no invented evidence.
3. Task 6.2's acceptance changed from `grep -c "photo_intake" tool-directory.md` ≥ 6 (unreachable) to ≥ 2, per orchestrator decision (b). Task numbering/checkboxes untouched; all 31 tasks still carry `Stack: none` and `Acceptance:`.
4. `retrace_extras_expected` deleted from `design.md` D3's JSON example and D10's `NOT_TOOLS` list, and from tasks.md 1.1/5.7 text (checkboxes/numbering unchanged); task 1.1's acceptance now says "both fields" instead of "all three fields".
5. `design.md` D6 step 4 amended: replaced "is **not modified** by this change" with one sentence describing the new "Building from an approved photo-intake map" section reading `approval_valid`; rest of the paragraph (the `asset_references.rs` case-list argument) kept verbatim. Added a matching Non-Goal bullet to `proposal.md`. Added `approval_valid` to D2's `load_photo_review_map` output row for consistency with the rewritten spec (not explicitly requested, but the spec now depends on that field being documented there).

## Evidence

- Before/after — spec.md gate requirement:
  - Before: `"...and SHALL block any tool that would mutate a KiCad project (schematic or PCB) from consuming a review map that is not approved."`
  - After: `"...The bundled kicad-photo-intake skill, pcb-photo-intake-agent, and kicad-schematic-build-agent SHALL NOT invoke a schematic- or board-mutating tool for a map whose approval_valid is false. Server-side enforcement of this rule inside the sch_*/pcb_* tool handlers themselves is explicitly out of scope for this slice and is deferred to a later change (see proposal.md Non-Goals)."`
- Before/after — tasks.md 6.2 acceptance: `≥ 6` → `at least 2` (`grep -n "at least 2" tasks.md` → line 125).
- Before/after — design.md D3: `"retrace_python_path": null, "retrace_extras_expected": [], "retrace_timeout_seconds": 120` → `"retrace_python_path": null, "retrace_timeout_seconds": 120`.
- Before/after — design.md D6.4: `"kicad-schematic-build-agent.md is **not modified** by this change."` → `"kicad-schematic-build-agent.md gains one short new section, 'Building from an approved photo-intake map', that reads approval_valid from load_photo_review_map before placing or wiring anything sourced from a photo-intake map, and refuses to proceed when it is false; everything else in the file is unchanged."`
- Mechanical checks (all from Konnect repo root):
  - `grep -c "^Stack: none" tasks.md` = 31, `grep -c "^Acceptance:" tasks.md` = 31, `grep -c "^- \[x\]" tasks.md` = 31.
  - `grep -rn "retrace_extras_expected" openspec/changes/photo-to-kicad-reverse` = no matches.
  - `grep -rn "block any tool that would mutate" openspec/changes/photo-to-kicad-reverse` = no matches.
  - `orc validate --changes photo-to-kicad-reverse --json` → `"valid": true`, 1/1 passed.
- Test name cited in spec.md (`load_reports_approval_valid_false_after_an_out_of_band_edit`) confirmed to exist at `crates/konnect-core/src/tools/photo_intake.rs:2351` in the implementation worktree (`C:\Users\felip\.orc\worktrees\konnect-3b7e2022\photo-to-kicad-reverse`), via `grep -n`.

## For the next agent

1. The "Building from an approved photo-intake map" section in `kicad-schematic-build-agent.md` is being added by a developer in parallel (per orchestrator context) — spec.md's four handoff scenarios now cite that exact section name. Whoever lands that edit should use that literal heading text so the spec's citations stay accurate; if the heading differs, spec.md needs a one-line follow-up.
2. `retrace_extras_expected` still exists in code (`config.rs:60`/`:608`, `SKILL.md:37`, `asset_references.rs:761` per reviewer finding 6) — planning docs now describe the target state (key removed), but the actual removal from those asset/code files is unassigned dev work, not done here (out of scope: TB forbids editing code).
3. QA finding 1 (schema bug blocking `save_photo_review_map` entirely) and reviewer findings 1-4, 6 (subprocess/UNC path, `used_fallback`/ocr extra, scan interpreter reporting, silent key-dropping on save) are untouched — none were in this task's scope (spec/task/design text only, per findings 5 and QA items 2-3).
4. Task 6.2's file-content requirement (a `photo_intake` section in `tool-directory.md` listing its 5 tools) is unchanged — only the numeric threshold moved. Verify `tool-directory.md` still has that section before closing 6.2 in a future pass.

## Deferred findings

1. Reviewer finding 6's own remediation options ("delete the key, or give it a consumer") were resolved here as "delete" at the planning-doc level per orchestrator decision (c); the corresponding code/asset deletion is a separate, not-yet-scheduled task.
2. `design.md` D9 (new skill and agent) and D10 (asset-guard implications) were not otherwise touched; both remain accurate against the current `NOT_TOOLS` composition after removing `retrace_extras_expected`.

## Result (design alignment)

Follow-up task: aligned `design.md` with the developer's fix-round-1 behavior at worktree HEAD `39d4b4d` (handoff `10-developer.md`). `design.md` only; no other artifact touched this round.

1. **D11 — `used_fallback` expression (before/after).**
   - Before: `` used_fallback      = yolo_warning_seen || !extras.detection ``
   - After: `` used_fallback      = yolo_warning_seen || ocr_warning_seen || !extras.detection || !extras.ocr ``
   - `fallback_evidence`'s four-signal shape was already correct in the doc; left unchanged.

2. **D2 table — three rows updated.**
   - `check_retrace` input, before: `` `python_path` (opt.) ``; after: `` `python_path` (opt.), `project_dir` (opt. — the project whose config supplies `photo_intake.retrace_python_path`; pass the same `project_dir` you will pass to `scan_pcb_photo`, or the two calls may resolve different interpreters) ``. Output gained `candidates_tried`.
   - `scan_pcb_photo` output, before: `` `map_id`, `components`, `traces`, `pattern_matches`, `analysis_json_path`, `duration_seconds`, `used_fallback`, `fallback_evidence` ``; after: same plus `` `python_path`, `candidates_tried` — the last two spelled exactly as `check_retrace` spells them... ``
   - `save_photo_review_map` input, before: `` **`project_dir`**, **`map`** (full review-map JSON) ``; after: same plus a parenthetical naming `review_map_schema()`'s nested `properties` and `"additionalProperties": true` on the map/component/net objects.

3. **D3 — new paragraph added** (after the `ServerConfig` paragraph) describing `config_project_dir(argument, config)` as the one source both `check_retrace` and `scan_pcb_photo` now read the project through, and noting `handle_get_effective_config` shares `layer_configs` with `effective_config`. **D4 — one sentence appended** to the `check_retrace` bullet tying the diagnostic contract to D3's `config_project_dir` and the shared `python_path`/`candidates_tried` shape.

4. **D5 — one paragraph added** after the approval-state paragraph: schema is open (`additionalProperties: true`), handler layers the normalized value over the caller's payload so unknown keys survive a save, and the content hash still covers only the four D16 keys.

5. **D8 — one paragraph added** after the schema block's closing sentence, same substance as D5's addition but framed from the schema's point of view (`review_map_schema()` names every field and opens the three object levels; only D16's four keys are hashed).

6. **D13 — new point 6 added** after point 5 (`image_path`): `subprocess_arg` strips only `VerbatimDisk`, rewrites `\\?\UNC\srv\share\...` to the valid UNC root `\\srv\share\...` (never to the bare, relative `UNC\srv\share\...` an unconditional strip produced), and leaves other verbatim forms (volume GUID, device paths) untouched.

All six edits verified present by grep (`used_fallback =`, `candidates_tried`, `config_project_dir`, `additionalProperties: true`, `subprocess_arg`). Re-ran `orc validate --changes photo-to-kicad-reverse --json` from the Konnect repo root: `"valid": true`, 1/1 passed, 0 issues.

## Result (round-2 text fixes)

Follow-up task per `13-reviewer.md` (round-2 diff audit, verdict FIX, two new SERIOUS findings both about the hash's actual scope). FS this round: `spec.md` and `design.md` only. Chose the "fix the design text" path the reviewer named as the alternative to a code change (reviewer's own next-agent note 2: "If you fix the design text instead... do not align the skill down to design") — this round states the *target* hash/reserved-key semantics precisely; the corresponding code change in `review_map_content_hash`/`validate_incoming_map` is not yet applied at worktree HEAD `39d4b4d` and is not mine to make (FS: spec.md/design.md only, no code).

1. **spec.md:53 — reworded the vacuous `save_photo_review_map`/`approval_valid` clause.**
   - Before: `` `save_photo_review_map` and `load_photo_review_map` SHALL never report `approval_valid: true` for a map unless `approve_photo_review_map` was called explicitly for it and the map's current content hash still equals the hash recorded at that approval (D16). ``
   - After: `` `load_photo_review_map` SHALL report `approval_valid: true` for a map only when `approve_photo_review_map` was called explicitly for it and the map's current D8-normalized content hash still equals the hash recorded at that approval (D16). `save_photo_review_map` SHALL never persist `approved: true` from client-supplied input, and SHALL ignore every client-supplied bookkeeping key (`approved`, `approved_at`, `content_hash_at_approval`, `saved_at`, `approval_valid`) present in the incoming `map`, deriving each one only from the record already on disk. ``

2. **design.md D5 (:98 in round-1 numbering) — dropped "can never move the hash" and stated the real mechanism.**
   - Before: `` The content hash (D16) still covers only `source_images`/`scale_reference`/`components`/`nets`; an unknown key can never move the hash and can therefore never grant or revoke approval on its own. ``
   - After: added a reserved-bookkeeping-stripping sentence, then: `` The content hash (D16) is computed over the D8-normalized, schema-known fields of `source_images`/`scale_reference`/`components`/`nets` only — never the raw subtrees the caller sent — so a key not named by D8, nested anywhere inside a component, a net, or `scale_reference`, is outside the hash and cannot revoke an approval by being added or edited later, while editing any D8-named field ... always moves the hash. ``

3. **design.md D8 (:159) — same correction, schema-side framing.**
   - Before: `` Only the four D16-hashed keys (`source_images`, `scale_reference`, `components`, `nets`) participate in the approval hash, so an unknown key can never affect approval state. ``
   - After: reserved-key-stripping sentence, then: `` The content hash (D16) walks the D8-normalized form of `source_images`/`scale_reference`/`components`/`nets` — exactly the fields named above, not the raw JSON subtree a caller sent — so an annotation key added anywhere inside those four does not move the hash, while editing any field named above always does. ``

4. **design.md D16 — rewrote "Covered fields"/"Excluded"/"Canonical serialization" to specify normalize-before-hash.**
   - Before ("Covered fields"): `` `components` (every field of every element, array order as stored), `nets` (likewise). ``
   - After: `` `components` (each element's D8 fields only — `component_id`, `ref`, `type`, `value`, `footprint_suggestion`, `confidence`, `bbox_px`, `approved` — in D8's order, array order as stored), `nets` (likewise, each element's `connections`/`source` only). ``
   - "Excluded" gained a clause: unknown keys anywhere inside `components`/`nets`/`scale_reference` are excluded as annotations, "not a change *to* the reviewed content".
   - "Canonical serialization" before: `` build a fresh `serde_json::Map` containing only the covered keys, cloned from the input ``; after: `` `components`, `nets`, and `scale_reference` are taken from the D8-normalized value (`serde_json::to_value(&parsed)`...) — never cloned from the caller's raw `map` ``.

5. **design.md D3 (:81) — fixed the stale "no behavior change" claim** flagged as self-contradicted by the paragraph added two sentences later in the same section.
   - Before: `` ...with no new file format and no behavior change to the existing tools. ``
   - After: `` ...with no new file format; the only behavior change to an existing tool is `handle_get_effective_config`, which now layers built-in defaults via the same shared `layer_configs` path (below) — a strict narrowing of what it already returned, not a new disagreement. ``

Verified no stale phrasing remains: `grep -n "can never move the hash\|never affect approval state\|never move the hash" design.md specs/photo-intake/spec.md` → no matches. Re-ran `orc validate --changes photo-to-kicad-reverse --json` from the Konnect repo root: `"valid": true`, 1/1 passed, 0 issues.

**Note for the next agent:** the design decision now states the hash normalizes before covering nested fields and that reserved bookkeeping keys are stripped by name on save. Neither is implemented yet in `crates/konnect-core/src/tools/photo_intake.rs` at worktree HEAD `39d4b4d` (`review_map_content_hash` still clones the raw `components`/`nets`/`scale_reference` subtrees, and `validate_incoming_map` does not reject reserved names) — reviewer finding 1 and finding 2 from `13-reviewer.md` are still open in code and need a developer pass before this text is fully true of the shipped behavior.

## Result (pre-archive D4 fix)

Last planner follow-up before archive, per `16-qa.md` must-check row (d1) and `15-reviewer.md`/`16-qa.md`'s Deferred 1: the developer's round-3 fix made `check_retrace` return an error result for a `project_dir` that fails D13 path validation (matching `scan_pcb_photo`'s existing behavior), while retrace's own absence or a failed import probe still comes back as a successful result with `available: false`. `design.md`'s D4 first bullet said `check_retrace` "always returns a successful `CallToolResult`", which is now one word too absolute. FS this round: `design.md` only.

1. **design.md D4, first bullet — qualified "always returns a successful result".** Only one occurrence of this phrase existed in `design.md` (the D4 first bullet itself; `design.md:90` and "D4's first bullet" are the same sentence, not two separate edits).
   - Before: `` `check_retrace` always returns a *successful* `CallToolResult` whose `available` field carries the yes/no — matching the "missing engine is a reported fact, not a tool failure" convention. Probe stdout/stderr is included when the `import retrace` probe exits non-zero. Both `check_retrace` and `scan_pcb_photo` resolve the project the same way (D3's `config_project_dir`) and report the same `python_path`/`candidates_tried` diagnostic shape, so a scan cannot be run by an interpreter the caller could not have predicted from a prior `check_retrace` call. ``
   - After: `` `check_retrace` returns a *successful* `CallToolResult` with `available: false` for retrace absence or a failed `import retrace` probe — matching the "missing engine is a reported fact, not a tool failure" convention — and an error result only when an argument fails D13's path validation. Probe stdout/stderr is included when the `import retrace` probe exits non-zero. Both `check_retrace` and `scan_pcb_photo` resolve the project the same way (D3's `config_project_dir`) and report the same `python_path`/`candidates_tried` diagnostic shape, so a scan cannot be run by an interpreter the caller could not have predicted from a prior `check_retrace` call. ``
   - Middle and closing sentences of the bullet kept verbatim, per instruction.

Verified no stale phrasing remains: `grep -n "always returns a" design.md` → no matches. Re-ran `orc validate --changes photo-to-kicad-reverse --json` from the Konnect repo root: `"valid": true`, 1/1 passed, 0 issues.

This closes `16-qa.md` Deferred 1 (the last open doc-layer item both `15-reviewer.md` and `16-qa.md` named) and QA's stated verdict was already DONE — this is a documentation-only tidy-up before archive, not a re-open of an implementation gap.
