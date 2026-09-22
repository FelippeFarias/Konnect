---
change: photo-to-kicad-reverse
task: "VERIFY round 2 diff audit"
agent: reviewer
verdict: FIX
failing_layer: implementation
---

## Result

All six round-1 SERIOUS findings and all four MINORs are CLOSED, each with a test
that could fail. The fixes are honest work — the UNC branch, the four-signal
`used_fallback`, the `config_project_dir` collapse and the `overlay_known_fields`
layering are all correct, and handoff 10's mutation proof reproduces from the
source. No CRITICAL. Two SERIOUS findings are **new**, both created by the
finding-4 fix, and both live in the same place: what the now-open map does to the
approval hash.

1. **SERIOUS — the design states a hash rule the code does not implement, and the
   direction is the gate's.** `design.md:98` ("an unknown key can never move the
   hash and can therefore never grant or revoke approval on its own") and
   `design.md:159` ("Only the four D16-hashed keys … participate in the approval
   hash, so an unknown key can never affect approval state") are **false** for any
   key nested inside `components[]`, `nets[]` or `scale_reference` — the exact three
   objects those same sentences declare open. `review_map_content_hash`
   (`crates/konnect-core/src/tools/photo_intake.rs:784-796`) clones
   `map["components"]` / `map["nets"]` / `map["scale_reference"]` **whole**, so a
   reviewer note added to a component after approval changes the digest and
   silently revokes approval. It fails safe (revoke, never grant), which is why
   this is not CRITICAL — but the skill's own workflow (`SKILL.md:126-131`: edit
   the JSON, then re-save) is the path that trips it, and no test covers it:
   `out_of_schema_keys_survive_a_save_and_load_round_trip`
   (`crates/konnect-core/tests/photo_intake_gate_e2e.rs:174-266`) adds every
   annotation **before** the `approve` call, so the one ordering that matters is
   the one it does not exercise.

2. **SERIOUS — `save` now persists client-supplied reserved names inside the map,
   which the pre-fix code deleted.** `validate_incoming_map`
   (`photo_intake.rs:944-961`) rejects nothing by name and `overlay_known_fields`
   (`:1351-1378`) carries every unknown key through, so a `map` containing
   `"approval_valid": true` is written verbatim into `review_map.json`. `load`
   (`:1500-1508`) then returns `{"map": {… "approval_valid": true …},
   "approval_valid": false}` — two keys of that name in one response, one forged
   and one computed. The realistic route is not an attacker but an agent re-saving
   the whole `load` response (which now also carries `saved_path`) as `map`; before
   `cefeeaf`, `serde_json::to_value(&parsed)` silently deleted both, so this is a
   regression of the fix, not a pre-existing hole.
   `kicad-schematic-build-agent.md:51-55` says "the response's `approval_valid`",
   which is the right reading but is the only thing standing between the two
   values. One line in `overlay_known_fields` (or a reserved-name rejection in
   `validate_incoming_map`) closes it; nothing today tests it.

**One change closes both:** hash the D8-normalized subtrees rather than the raw
ones in `review_map_content_hash` (`approve_photo_review_map:1567` must normalize
the same way). Annotations then genuinely stay outside the hash — `design.md:98`
becomes true — and `SKILL.md:136` ("any later edit to the components, nets, source
images or scale reference revokes it") stays true, because an annotation is not a
D8 field. The pinned digest `18afc7b9…` (`:1984` in round-1 numbering) is
unaffected: the fixture carries no out-of-schema key. The alternative is to fix
the two design sentences instead and accept annotation-revokes-approval — cheaper,
but it leaves finding 2 open on its own.

**MINOR (no verdict impact):** `handle_check_retrace:1199` builds its config
project from the **raw** `project_dir` string while `handle_scan_pcb_photo:1228`
canonicalizes first, so a relative or symlinked argument still resolves two
different config files — and `both_handlers_resolve_the_config_project_the_same_way`
(`:560-583`) tests the helper, not the handlers, so its name overclaims. ·
`tool-directory.md:441` still describes `get_effective_config` as "user defaults +
project overrides" after `layer_configs` put built-in defaults under it. ·
`design.md:81` still says the config work lands "with no behavior change to the
existing tools", which `design.md:85` (same section) now contradicts. ·
`photo_intake.rs:974` names `the_map_schema_names_every_review_map_field`; the test
is `…_and_stays_open` (`:2945`). · `spec.md:53` requires
`save_photo_review_map` to "never report `approval_valid: true`" — save's response
has no such field, it reports `approved` (`:1466-1470`), so that clause is
vacuous rather than verified.

## Evidence

**Round-1 findings, judged**

| # | Round-1 finding | Verdict | Proof |
|---|---|---|---|
| 1 | `subprocess_arg` mangles UNC | **CLOSED** | `photo_intake.rs:277-292` returns `format!(r"\\{share}")` for `UNC\`, strips only `X:`-shaped rests, and returns `raw` for every other verbatim form; `:1761-1783` asserts the UNC, volume-GUID and POSIX shapes plus `!starts_with("UNC")`. I walked the remaining shapes myself: `\\?\C:\` (canonicalize never emits a bare `\\?\C:`), lowercase drive letters, and `\\?\UNCfoo` (no trailing `\`, falls through to `raw`) are all correct |
| 2 | `used_fallback` ignores `ocr` | **CLOSED** | `:529` is the four-signal form; `used_fallback_is_true_whenever_any_extra_did_not_run` (`:2164`) covers detection-only, ocr-only and stderr-with-both-extras; the three asset files now say "either extra". Round-1 "next agent" item 4 is also closed: the live test gained a real pin (`if !detection { assert!(yolo) }`) beside the surviving re-derivation |
| 3 | scan hides the interpreter; two config sources | **CLOSED** | `build_scan_response:545-572` emits `python_path`/`candidates_tried` spelled as `build_check_response` spells them, asserted equal field-by-field at `:2049-2096`; `config_project_dir:578` is called by both handlers (`:1200`, `:1233`). Residual canonicalization gap logged as MINOR |
| 4 | `save` deletes unknown keys | **CLOSED**, with consequences | `overlay_known_fields:1351` + `:1420`; e2e `:174-266` proves top-level, per-component and per-net annotations survive **and** that `""`→`null` still lands (`map["components"][1]["value"] == Null`). Handoff 10's mutation proof (`map_arg.clone()` → that assertion fails) is the right probe and reproduces by inspection. New findings 1 and 2 are what the fix opened |
| 5 | spec's "SHALL block any mutating tool" unimplemented | **CLOSED at the spec layer** | `spec.md:53` now scopes the rule to the skill and the two agents and names server-side enforcement an explicit Non-Goal; the four handoff scenarios each cite the verifying artifact and say "no automated test in this slice". `kicad-schematic-build-agent.md:36-71` supplies that artifact under the literal heading `spec.md:57` cites |
| 6 | dead `retrace_extras_expected` | **CLOSED** | `grep -rn retrace_extras_expected` over the worktree returns nothing (code, assets, NOT_TOOLS, openspec). `default_photo_intake_config_is_exactly_the_keys_something_reads` (`config.rs:660`) pins the exact key set, so the next unread knob fails. `not_tools_allowlist.rs:57`'s `names.len() >= 19` still holds after the removal |
| — | four round-1 MINORs | **all CLOSED** | `layer_configs` (`config.rs:153`) shared by both readers + test `:614`; `part_number` prose corrected in all three assets (map has no such field — the right call, a field would widen the hash for data no tool reads); `source_images` now taught as required in both assets; the synthetic-board doc comment moved onto `mod synthetic_board` (`:3014`) |

**Attack items (a)-(g)**

| Item | Result |
|---|---|
| a | `additionalProperties: true` reopens **nothing the gate relies on** — `save` overwrites `approved`/`approved_at`/`content_hash_at_approval` unconditionally at `:1436-1452` *after* the overlay, and `keeps_approval` (`:1425-1433`) reads only the on-disk prior state. An extra key **can** move the hash when nested (finding 1) and a reserved name **can** be persisted (finding 2). `map_id` is token-validated in both layers: `pattern: MAP_ID_PATTERN` at `:988` (save's map) and `:3007`-asserted for load/approve, plus `validate_map_id:857` from `existing_map_dir`/`validate_incoming_map`. `close_input_schema` (`tools/mod.rs:145`) uses `entry().or_insert()` and recurses into `additionalProperties` only when it is an object, so the declared `true` survives |
| b | `overlay_known_fields` cannot overwrite tool-owned bookkeeping: every one of those keys is a `PhotoReviewMap` field, so `normalized` wins the merge and the handler then rewrites it. `""`→`null` still applies — the `(_, normalized)` arm replaces scalars including `Null`, and `normalized`'s array length is authoritative while `validate_incoming_map` guarantees equal length. The hole is by *name*, not by precedence: see finding 2 |
| c | UNC/volume-GUID: no shape I could construct comes out relative. `\\?\Volume{…}` is deliberately left prefixed, which is correct — Python accepts it and stripping it would be meaningless |
| d | four-signal expression and its tests: correct and now conservative in both directions; `fallback_evidence` still carries the four raw signals separately |
| e | `check_retrace` gained an optional `project_dir` (schema `required: []` asserted at `:2091`); the only behavior change to an existing config tool is `handle_get_effective_config` now layering built-in defaults — that is D3's stated precedence and strictly narrows a disagreement, but its `tool-directory.md` row and `design.md:81` were not updated (MINOR) |
| f | the new agent section contradicts nothing in `kicad-schematic-build-agent.md`: `INCOMPLETE` is the file's existing verdict vocabulary (`:112`, `:152`, `:182`) and the extra `load_toolset("photo_intake")` is additive to the Setup list. Guards hold by construction — `backticked_tool_names_in_prose_exist_in_the_registry` builds `known` from tool names **and** `all_toolsets()` names (`asset_references.rs:633-643`), so `sch_wiring`/`sch_batch` pass, and `approval_valid`/`saved_path`/`candidates_tried` are in the photo-intake `NOT_TOOLS` block. The refusal is unambiguous on the `approved`-vs-`approval_valid` axis (`:50-55` names both and says which is the gate); it is *not* armoured against finding 2's second `approval_valid` |
| g | nothing broke with the key removal: no doc count references it, `not_tools_allowlist.rs`'s `>= 19` floor holds, and the two superseded tests were rewritten with doc comments naming the finding rather than deleted |

## For the next agent

1. Fix finding 1 in `review_map_content_hash` (`photo_intake.rs:784`), not in
   `design.md`: hash the D8-normalized subtrees. It closes finding 2's persistence
   half as a side effect only if you also drop unknown keys on the *hashed* path —
   so still reject or strip `approval_valid`/`saved_path`/`approved*` by name in
   `validate_incoming_map`. Add the missing ordering test: annotate **after**
   `approve`, then assert `approval_valid` is still true.
2. If you fix the design text instead, both `design.md:98` and `design.md:159`
   carry the claim, and `SKILL.md:136` is already correct — do not "align" the
   skill down to the design.
3. The four MINORs are one line each and none blocks archive; `design.md:81`'s "no
   behavior change to the existing tools" is the one most likely to mislead a
   future reader, since `handle_get_effective_config` did change.
4. Everything else in this change is sound. The gate, the path rules, the
   subprocess handling, the parsing, the registry and the config layering all
   survived a second pass, and I found no false claim in handoff 10 or 11 —
   handoff 10's red→green transcripts and its `map_arg.clone()` mutation are
   reproducible from the source.
5. I ran no cargo commands; the qa agent owns the suites. Every judgement above is
   source analysis against the revised `design.md`/`spec.md`.

## Deferred findings

1. `portable_uri` (`crates/konnect-core/src/tools/library.rs:1818-1821`) still has
   the UNC defect `subprocess_arg` just lost; `subprocess_arg:277-292` is now the
   two-branch precedent to copy. Unchanged from round 1 / developer Deferred 2.
2. `agents_make_claimed_evidence_executable` (`asset_references.rs:175`) still
   hard-codes a marker list per agent, so the new "Building from an approved
   photo-intake map" section has no executable guard — and `spec.md:57` now cites
   that section by name. One array entry (`approval_valid`) would make the spec's
   citation enforceable.
3. `read_config` (`config.rs:111-116`) still substitutes defaults for a config file
   that exists but does not parse; `layer_configs` did not change it.
