---
change: photo-to-kicad-reverse
task: "VERIFY round 3 final audit"
agent: reviewer
verdict: DONE
failing_layer: none
---

## Result

Both round-2 SERIOUS findings and all four MINORs are **CLOSED** at `3d77ed5`, each
with a test that could fail. The fixes are narrow and correct: the projection
matches design D8 field-for-field in all three shapes, the pinned digest still
reproduces D16's canonical object from an independent tool, and nothing in the
four commits weakens a guard. No CRITICAL, no new SERIOUS.

1. **SERIOUS 1 (annotation revoked approval) — CLOSED.** `review_map_content_hash`
   (`photo_intake.rs:857-884`) now projects each covered value onto the D8 fields
   of its own shape. I checked the three lists against the structs field by field:
   `COMPONENT_CONTENT_KEYS:771` = `ReviewComponent`'s eight serialized names
   (`component_id`, `ref`, `type`, `value`, `footprint_suggestion`, `confidence`,
   `bbox_px`, `approved`, `:684-713`), `NET_CONTENT_KEYS:783` = `ReviewNet`'s two
   (`:717-720`), `SCALE_REFERENCE_CONTENT_KEYS:786` = `ScaleReference`'s two
   (`:706-709`). No missing key, no phantom. `design.md:98` and `:159` are now true.
2. **SERIOUS 2 (forged reserved names persisted) — CLOSED.** `TOOL_OWNED_KEYS:1494`
   is stripped from `map_arg` before the overlay (`:1550-1553`). The only
   load-bearing entry is `approval_valid`: the other four are `PhotoReviewMap`
   fields, so `serde_json::to_value(&parsed)` re-injects them and `:1570-1591`
   overwrites all four from the on-disk record — belt and braces, not redundancy
   that hides a hole. `approval_valid` is never persisted
   (`photo_intake_gate_e2e.rs:342-347`) and never a `PhotoReviewMap` field.
3. **Four MINORs — CLOSED.** `handle_check_retrace:1292-1299` canonicalizes via
   `canonical_existing_dir` and `check_retrace_rejects_a_project_dir_the_scan_would_reject`
   (`:2370`) drives both **handlers** and asserts identical error text ·
   `tool-directory.md:441` now reads "built-in defaults, then user defaults, then
   project overrides" · `photo_intake.rs:1063` names the renamed test ·
   `design.md:81` now states the `handle_get_effective_config` change explicitly
   and `spec.md:53` replaced the vacuous clause with "SHALL ignore every
   client-supplied bookkeeping key … deriving each one only from the record
   already on disk", which is exactly what the code does.
4. **New (MINOR, no verdict impact).** `editing_any_reviewed_field_changes_the_content_hash`
   (`photo_intake.rs:2672`) asserts 10 edits but never `components[].type`,
   `components[].footprint_suggestion`, `components[].component_id` or
   `scale_reference.kind` — four of the twelve keys the projection must list.
   Before `7ae849e` coverage was structural (clone the subtree); now an omission
   from a `*_CONTENT_KEYS` list is a silent **fail-open**, and two of the four
   unasserted fields are named by `design.md:98` as ones that "always move the
   hash". The lists are right today; the guard that would catch them going wrong
   is not. · `design.md:90` ("`check_retrace` **always** returns a successful
   `CallToolResult`") is now false for an unresolvable `project_dir`; the spec's
   normative clause (`specs/photo-intake/spec.md:4`, "SHALL NOT fail merely
   because `retrace` or its extras are absent") is correctly scoped and is **not**
   violated, so this is a design-text imprecision, and the behavior change is the
   right one — the probe can no longer read a different project's config than the
   scan it diagnoses.
5. **Verdict DONE.** Every open finding is doc-layer or future-drift; nothing
   blocks archive.

## Evidence

**Round-2 findings, judged**

| # | Round-2 finding | Verdict | Proving line |
|---|---|---|---|
| S1 | hash covers unknown annotation keys | **CLOSED** | `photo_intake.rs:860-878` projects instead of cloning; `unknown_keys_are_outside_the_content_hash_at_every_depth:2608` pins all four depths; `annotations_added_after_approval_keep_the_approval_valid` (`photo_intake_gate_e2e.rs:380`) annotates **after** `approve` through the router, asserts `approval_valid` stays true, then edits `components[0].ref` and asserts it revokes — the ordering round 2 said was untested |
| S2 | save persists client-supplied reserved keys | **CLOSED** | `without_tool_owned_keys:1503` called at `:1550`; `a_payload_cannot_forge_its_own_approval` (`photo_intake_gate_e2e.rs:283`) forges with a **genuine** stolen digest (`map_id` is outside the hash, so it is arithmetically correct for the content) and asserts the stored object's `approv`-matching keys are exactly `["approved","approved_at","content_hash_at_approval"]`, `loaded["map"].get("approval_valid") == None`, `approval_valid == false` |
| M1 | `check_retrace` raw vs canonicalized `project_dir` | **CLOSED** | `:1292-1299` + handler-level test `:2370` (compares both handlers' text, not the shared resolver) |
| M2–M4 | `tool-directory.md:441`, `design.md:81`, doc comment `:1063` | **CLOSED** | one line each, all present at HEAD |
| M5 | `spec.md:53` vacuous clause | **CLOSED** | rewritten to the ignore-and-re-derive rule the code implements |

**Attack items (a)-(e)**

1. **(a) projection.** Lists equal the D8 sets exactly (item 1 above). `""`→`null`
   is inside `review_map_content_hash` itself (`EMPTY_STRING_IS_NULL:789`,
   applied at `:851`), so it lands on **every** path — `save:1554`, `approve:1675`,
   `approval_is_valid:890` — not only on save. `an_empty_string_hashes_as_the_null_save_writes_for_it:2655`
   pins it. I recomputed the pinned digest independently in Python
   (`json.dumps(covered, sort_keys=True, separators=(',',':'))` + sha256 over the
   projected object) → `18afc7b999d88ad5ec0320760e920b7099c257d809b5961b0e5a83d1afb0908a`,
   byte-identical to the literal at `:2565`. The projection is an identity on the
   fixture because `empty_string_as_none` (`:727-729`) already turned retrace's
   `"value": ""` into `null` before the map was built.
2. **(b) stripping.** List complete for the gate: the only response field that can
   collide inside the map and is *read* by anyone is `approval_valid`, and it is
   stripped. Applied before the overlay (`:1550`). Bookkeeping derived only from
   `previous = read_review_map(&map_file)` (`:1566-1575`); the payload's own values
   reach `value` via `parsed` but are unconditionally overwritten at `:1570-1591`
   and none of the four is in `CONTENT_KEYS`, so they cannot move the hash. A
   `map_id` swap is closed by `resolve_map_file:1515` reading the target id's own
   file. A **nested** reserved key is harmless: component-level `approved` is a
   real D8 field and is hashed; a nested `approval_valid` is unread by
   `approval_is_valid:889` (top level only) and sits outside the projection.
3. **(c) `check_retrace`.** Behavior change confirmed: a present-but-unresolvable
   `project_dir` was a silent fall-through to the server's project, now it is
   `CallToolResult::error`. Judged against D4: the *spec* clause is about
   `retrace`'s absence and is untouched; D4's own second sentence ("both resolve
   the project the same way") only became true with this commit. Design text
   imprecision recorded as a MINOR, not a regression.
4. **(d) asset guard.** `asset_references.rs:177-190` is additive only —
   `"photo_intake"` appended to the required toolsets and `"approval_valid"` to the
   marker list for `kicad-schematic-build-agent.md`. No predicate relaxed, no case
   removed. `git diff 39d4b4d..3d77ed5 -- crates/konnect/assets/` is empty; the
   asset already carries `load_toolset("photo_intake")` (`:44`) and
   `approval_valid` (`:49`, `:53`), so the guard tightened around existing content.
5. **(e) regressions.** The diff touches four files and no earlier-closed finding's
   code: `subprocess_arg`, `used_fallback`, `config_project_dir`, `layer_configs`,
   `NOT_TOOLS` and `not_tools_allowlist.rs` are untouched. `overlay_known_fields`
   still carries every non-reserved key through, so round-1 finding 4 stays closed
   (`out_of_schema_keys_survive_a_save_and_load_round_trip`, unchanged, plus the
   new late-annotation e2e). I ran no cargo commands — the qa agent owns the
   suites; every judgement above is source analysis plus the independent digest
   recomputation.

## For the next agent

1. **Archive-ready.** Nothing below blocks it. If you want one cheap hardening
   before archive, it is finding 4's four missing edits in
   `editing_any_reviewed_field_changes_the_content_hash:2672` — four `MapEdit`
   entries, no production change.
2. **`design.md:90`** should read "always returns a successful result *about
   retrace*" or name the `project_dir` rejection; `specs/photo-intake/spec.md:4`
   is already correct and must not be "aligned" down to it.
3. **If you ever touch a `*_CONTENT_KEYS` list**, re-derive the pinned digest in a
   second tool rather than pasting the failure message. The recipe is in the
   Evidence above and reproduces in one command.
4. **The three new lists have no drift guard.** `every_schema_key_is_either_hashed_or_deliberately_not:2751`
   compares only the map's top-level keys against `CONTENT_KEYS`+`UNHASHED_KEYS`;
   nothing compares `COMPONENT_CONTENT_KEYS` to `ReviewComponent`. A field added
   inside a component is silently unhashed and fails **open**.
5. Handoff 14's five evidence claims all reproduce from the source, including the
   unchanged digest and the empty asset diff. I found no false statement in it.

## Deferred findings

1. **No drift guard between the nested key lists and the structs**
   (`photo_intake.rs:771-786` vs `:684-720`). Present code is correct; the risk is
   a future field. The top-level equivalent is guarded, so the asymmetry is the
   real defect. (Developer Deferred 2, confirmed.)
2. **`saved_path` is not in `TOOL_OWNED_KEYS`** (`:1494`) although `load` returns it
   as a computed sibling of `map` (`:1641`). A client can persist a `saved_path`
   key inside the map; it is inert — outside the hash, read by nothing — but it is
   the other half of the name collision the strip closed.
3. `portable_uri` (`crates/konnect-core/src/tools/library.rs:1818-1821`) still has
   the UNC defect `subprocess_arg:277-292` fixed. Unchanged since round 1.
4. `read_config` (`config.rs:111-116`) still substitutes defaults for a config file
   that exists but does not parse, so a typo reads as "no preferences set".
5. `agents_make_claimed_evidence_executable` is still a hard-coded marker list per
   agent, and `install.rs`'s offline-contract test still names only two agents
   while `pcb-photo-intake-agent.md:27` reads that contract.
