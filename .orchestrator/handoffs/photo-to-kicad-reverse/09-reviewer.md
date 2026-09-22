---
change: photo-to-kicad-reverse
task: "VERIFY diff audit"
agent: reviewer
verdict: FIX
failing_layer: implementation
---

## Result

No CRITICAL. The approval gate itself survived every attack I could mount: the hash covers exactly the four content keys, `save` can never mint approval from payload data, `approve` hashes the object it writes, and `map_id` traversal is closed twice. Six SERIOUS findings, all outside the gate.

1. **SERIOUS — `subprocess_arg` corrupts UNC paths, defeating the confinement it was just proved to satisfy.** `crates/konnect-core/src/tools/photo_intake.rs:268-271` strips `\\?\` unconditionally. On a project under a network share, `canonicalize` yields `\\?\UNC\srv\share\proj` (that is what `std::path::Prefix::VerbatimUNC` exists for), so the stripped string is `UNC\srv\share\proj\…` — a **relative** path. `prepare_map_dir`'s `starts_with` check (`:517`) passes on the canonical `PathBuf`, then `:1112` hands retrace the mangled string as `-o`, so the scan writes into the server process's CWD, outside the project. The failure is loud (the later `read_to_string` at `:1104` misses and the tool errors), but `analysis_json_path` (`:1143`), `saved_path` (`:1315`) and `python_path` (`:445`) all emit the same broken string. The only test (`:1519-1526`) exercises `\\?\C:` and a POSIX path — never `\\?\UNC\`.

2. **SERIOUS — `used_fallback` ignores the `ocr` extra, while three asset files state it as a biconditional.** `photo_intake.rs:488` is `yolo_warning_seen || !extras.detection`. With `detection: true, ocr: false` — a plausible install, since `ultralytics` and `easyocr` are separate extras — `used_fallback` is `false` even though chip-marking OCR never ran and `marking`/`value`/`part_number` were not attempted. `SKILL.md:86` ("`used_fallback: true` means marking, `value` and `part_number` were **never attempted**"), `review-map-schema.md:75-77` and `pcb-photo-intake-agent.md:59-60` all teach the agent to read `false` as "the field was read and was empty". That is precisely the silent-guess the spec scenario *scan runs without ML extras installed* exists to prevent. No test covers `detection: true, ocr: false` (`:1770-1802` tests only both-on and both-off).

3. **SERIOUS — `scan_pcb_photo` never reports which interpreter ran, and resolves config from a different source than `check_retrace`.** `resolve_retrace` (`:390-410`) falls through to PATH when an explicit `python_path` or the configured `retrace_python_path` fails the import probe — D15 as written, but the scan response (`:1138-1146`) carries neither `python_path` nor `candidates_tried`, so a scan run by an interpreter the user did not name is indistinguishable from one that honoured them. Pre-mortem cause 5's hardening ("which interpreter did you use is always in the result") therefore holds only for `check_retrace`. Worse, `handle_check_retrace:1010` reads config from `ctx.config.project_dir` while `handle_scan_pcb_photo:1043` reads it from the `project_dir` *argument* — so the agent's Phase-0 report can name an interpreter the scan never used.

4. **SERIOUS — `save_photo_review_map` silently deletes every key not in `PhotoReviewMap`.** `:1184` is `serde_json::to_value(&parsed)`, and the struct has no catch-all and no `deny_unknown_fields`, so unknown top-level *and* per-component keys are dropped without a warning. `SKILL.md:115` promises "hand edits survive and are the expected workflow" and `SKILL.md:117` then instructs the agent to re-save after any edit — which is the call that destroys the user's annotations. The spec's "load returns byte-for-byte the same structure" is only true for schema keys. `the_review_map_survives_across_sessions_with_hand_edits_intact` (`:2325`) proves the *load* path only; nothing covers a save round-trip of an out-of-schema key.

5. **SERIOUS — the spec's "SHALL block any tool that would mutate a KiCad project" is unimplemented.** No `sch_*`/`pcb_*` handler reads a review map or `approval_valid`; the gate exists only in the two new prose assets, and `kicad-schematic-build-agent.md` is deliberately unmodified (design D6.4). Handoff 07 "For the next agent" #5 states this honestly, but archiving publishes `specs/photo-intake/spec.md`'s ADDED requirement as delivered. The scenario *unapproved map cannot reach schematic build* is satisfied at the map layer only.

6. **SERIOUS — `photo_intake.retrace_extras_expected` is a dead knob presented as functional.** Declared at `crates/konnect-core/src/tools/config.rs:60`, asserted at `:608`, allowlisted at `crates/konnect/tests/asset_references.rs:761`, and documented at `SKILL.md:37` as "extras you expect to be present" — but no code reads it. Design D3 introduced it without a consumer; the skill turns that into a promise to the user.

**MINOR (no verdict impact):** `handle_get_effective_config` (`config.rs:443-452`) does not layer built-in defaults under an existing user file, so the tool a user inspects disagrees with the `effective_config` accessor `photo_intake` actually reads (`config.rs:156-164`) — benign today, a trap when a key gets a non-default default. · `SKILL.md:98` / `pcb-photo-intake-agent.md:71` tell the agent that "`value` and `part_number` stay empty" while building the review map, but the map has no `part_number` field (`photo_intake.rs:637-660`); such a key is accepted by serde and then dropped by finding 4. · Neither asset tells the agent to populate the required `source_images` (`:597`), so the first save fails with a serde message. · Misplaced doc comment at `photo_intake.rs:2556` ("The synthetic board the live test scans") sits above `mod review_map_tests`.

## Evidence

**Attack surface coverage (a)-(h)**

| # | Probe | Result |
|---|---|---|
| a | hash covers exactly `source_images`/`scale_reference`/`components`/`nets` | verified — `CONTENT_KEYS` `:653`, `review_map_content_hash` `:689-702`; `every_schema_key_is_either_hashed_or_deliberately_not` `:2098` reads the keys of `serde_json::to_value(PhotoReviewMap)`, so a new struct field fails the test |
| a | `save` persisting `approved: true` | verified — only via `keeps_approval` `:1190-1197`, which reads the **on-disk** prior state; payload `approved`/`approved_at`/`content_hash_at_approval` are overwritten unconditionally `:1200-1215`. Test `save_alone_never_approves` `:2205` supplies a *correctly computed* forged hash and still gets `false` |
| a | forging `approval_valid` through `load` | verified — `approval_is_valid` `:707-714` recomputes the hash from the file; forged fields cannot survive a `save`, and a hand-written file requires a directory only `scan_pcb_photo` mints (`existing_map_dir` `:759`) |
| a | `approve` hashes what it writes | verified — `:1367-1372` hashes the value read at `:1358`, then sets only unhashed keys; no round trip between |
| a | pinned digest is the canonical form, not the impl's habits | **independently reproduced**: `json.dumps(covered, sort_keys=True, separators=(',',':'))` + sha256 over the fixture-derived map → `18afc7b999d88ad5ec0320760e920b7099c257d809b5961b0e5a83d1afb0908a`, matching `:1984` |
| b | `map_id` token in schema AND Rust | verified — `pattern` at `:967`/`:989`, `validate_map_id` `:743` called from `existing_map_dir:760` and `validate_incoming_map:882`. `map_id_traversal_is_rejected_and_reads_nothing` `:2389` plants a real map outside the project and asserts 10 spellings (`..`, `a/b`, `a\b`, `C:`, 65 chars, …) never reach it |
| b | symlinked `.konnect` | verified — `prepare_map_dir:513-524` re-canonicalizes after `create_dir_all` and `starts_with`-checks; `existing_map_dir:765-779` does the same without creating |
| b | Windows `\\?\` in the confinement compare | verified — both sides come from `canonicalize` (`:530`, `:515`), so prefixes match. **Broken for UNC on the way *out*** — finding 1 |
| b | `image_path` outside the project | verified — `canonical_existing_file:539-548`, no confinement; used at `:1029` |
| c | `kill_on_drop`, `HOME`+`USERPROFILE` | verified — `:214-219`; `run_retrace_redirects_home_and_userprofile_away_from_the_real_home` `:1446` writes a uuid-named marker through the child's own `$HOME`/`%USERPROFILE%` and asserts the real home is clean |
| c | timeout clamp, truncated `analysis.json` never parsed | verified — `resolve_scan_timeout:479-486` clamps 5..=1800 (test `:1747`); the `Timeout` arm `:1084-1092` returns before any read |
| c | stderr marker vs em dash / replacement char | verified — `:486-487` matches the ASCII substrings `YOLO not available` / `easyocr is not installed`, never the `—`, so `from_utf8_lossy` mangling of cp1252 `0x97` is harmless. **But** the live test `:2649-2652` asserts `used_fallback == yolo \|\| !detection` from the response's *own* fields — tautological against `derive_fallback`; nothing pins the literal against a real run, which design D11 line 189 promised task 2.4 would do |
| c | `-m retrace`, discovery order, Store alias | verified — `:1107-1117`; `candidate_interpreters:285-310` is arg → config → `RETRACE_PYTHON` → `py -3` (win) → `python3` → `python`, test `:1583`. `VERSION_PROBE:56` prints `sys.executable` and `:1102` runs *that* path, so the Store alias stub's non-zero exit only costs one probe |
| d | `empty_string_as_none` on all five | verified — `:117-126`, test `absent_strings_deserialize_to_none_not_some_empty` `:1363` |
| d | `bbox` shape, `pattern_matches` typed | verified — `bbox: [i64; 4]` `:115`, `RetracePatternMatch` `:142-151` |
| e | merge precedence, existing config tools | verified — `effective_config:156-164` = defaults ⊕ user ⊕ project, matching D3's stated order; tests `:611`/`:639`. `handle_get_effective_config` untouched — see MINOR |
| f | order, `tool_count`, `BoardAccess` | verified — `registry.rs:119-124` `tool_count: 5`; sum of all `tool_count` = **231** across **22** `ToolsetMeta`, matching every doc string; `router/mod.rs:355-379` asserts the five names in order through both `tools_for` and `ToolRouter::load`; `photo_intake.rs:1699-1704` asserts `BoardAccess::None` on all five |
| g | skill prose vs tool behaviour | gate table `SKILL.md:130-137` is accurate on all five rows (consumers read `approval_valid`, `saved_at`+`subcircuit_hints` outside the hash, save re-grants only on an identical hash). Findings 2, 4, 6 and the MINORs are where prose outruns behaviour |
| g | `NOT_TOOLS` vs top-level schema properties | **re-verified mechanically**, not taken on trust: `grep -rn '"<name>": *{' crates/konnect-core/src/**/*.rs` for all 28 added names hits only `photo_intake.rs:1879`, a test fixture — no name is any tool's schema property. The six real parameters are correctly absent |
| g | non-existent `Lib:Symbol` or tool names in prose | verified — no `` `X:Y` `` token in any of the three new assets; every `tool(arg, …)` example names real required parameters |
| h | count strings repo-wide | verified — `grep -rn --include=*.md --include=*.json -E '\b(226\|233)\b'` over the worktree returns only `ROADMAP.md:149` (a GitHub issue number); no `21 toolsets` remains. `openspec/.../tasks.md:123` quotes `22 toolsets` and `231 registered tools` — `counts_in` needs digits immediately before `tools`, so it is inert; the change will not break the sweep after archive |
| h | README note accurate | verified — `README.md:304-310` names the right install command, both config paths and `check_retrace`, and states absence is not a failure, matching `build_check_response:406-457` |

**Handoff deviations, judged**

1. 05(a) probe prints `sys.executable` and that path is what `-m retrace` runs (`:56`, `:345-359`, `:1102`) — **accepted, stronger than D15**: it collapses discovery to the single resolution the probe performed, which is pre-mortem cause 5's whole point.
2. 05(b) `effective_config` layers built-in defaults under the user file (`config.rs:157-158`) — **accepted**: it is D3's stated precedence verbatim ("… > user config > built-in default"). Logged as MINOR only because `handle_get_effective_config` does not do the same.
3. 05 `Cargo.lock` outside the write set — **accepted**: one mechanical `+ "image"` line, verified by `git diff ea74faf..90f1bf5 -- Cargo.lock`.
4. 06(a) revoking save clears `approved_at`/`content_hash_at_approval` (`:1206-1215`) — **accepted, stronger than D5**: closes edit-then-undo. 06(b) save refuses an unminted `map_id` — **accepted**, D13 rule 3. 06(c) `nets[].source` closed, `scale_reference.kind` open — **accepted**, matches D8 and the Slice-2 deferral. `approval_valid` server-side — **accepted**, it is the only thing that makes D6 step 3 checkable by a caller that cannot hash.
5. 07 three doc files outside the declared write set — **accepted and necessary**: `doc_tool_counts.rs:126`/`:225` sweep every `.md`/`.json` under the repo root, skipping only `target`/`node_modules`/`.git`/`.claude`/`dist`/`build`; task 6.2's acceptance is unreachable without them. 07's `NOT_TOOLS` intersection claim re-verified above and is true.

## For the next agent

1. Fix in this order: finding 2 (one line at `:488` plus the three prose sites — it is the one that can put a wrong part on a board), finding 4 (either keep unknown keys or make `save` reject them loudly; silently dropping them is the only option that lies), finding 3 (add `python_path` + `candidates_tried` to the scan response and make `handle_check_retrace` accept the same config source), finding 1 (handle `\\?\UNC\` → `\\` in `subprocess_arg` and add the case to the test at `:1519`).
2. Finding 5 is scope, not a bug: either amend `specs/photo-intake/spec.md` so the requirement reads "at the review-map layer" before archiving, or open the Slice-2 item that puts the check in the `sch_*` handlers. Do not archive the requirement as written while nothing enforces it.
3. Finding 6 is a one-line choice: delete `retrace_extras_expected` from `default_user_config` and `SKILL.md:37`, or give it the consumer D3 implied (compare against the probe and warn). A config key nobody reads trains users to distrust the rest.
4. The live test at `:2649` needs a real assertion, not a re-derivation — on a contour-only machine assert `yolo_warning_seen == true`, which is the unversioned stderr contract D11 said this test would guard.
5. Nothing in the gate, the path rules, the subprocess handling, the parsing or the registry needs rework. The test suite here is unusually honest — three mutation proofs across 05/06 and an independently derived pinned digest — and I could not find a false claim in any of the three developer handoffs.

## Deferred findings

1. `portable_uri` (`crates/konnect-core/src/tools/library.rs:1818-1821`) has the same UNC-stripping defect as finding 1 and predates this change; a lib-table written on a network-share project would carry `UNC\srv\…`.
2. `read_config` (`config.rs:111-116`) silently substitutes defaults when the config file is present but unparseable, so a typo in `config.json` reads as "no preferences set" rather than an error. Pre-existing, now also on `photo_intake`'s path.
3. CONTRIBUTING still names four files as the documents that "move together" while `doc_tool_counts.rs` sweeps the whole repo — the guidance is narrower than the guard (also reported by 07).
