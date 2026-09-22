---
change: board-dossier-reconstruction
task: VERIFY round 1 diff audit
agent: reviewer
verdict: FIX
failing_layer: guard (crates/konnect/tests/asset_references.rs — the hard-coded per-agent/per-skill marker cases)
---

## Result

1. **`1815948..8be8d66` is a strong diff.** The gate mechanism (D6), the image
   pipeline (D1/D8), the six-path allowlist (D7) and the doc counts are all
   correct and provably so; no CRITICAL, no false handoff claim. One SERIOUS
   and three MINOR below. The pinned digest
   `18afc7b999d88ad5ec0320760e920b7099c257d809b5961b0e5a83d1afb0908a`
   reproduces independently from `json.dumps(covered, sort_keys=True,
   separators=(',',':'))` over a covered object with **no** `dossier`, **no**
   `design_brief` and `scale_reference == {kind, value}` — the direct proof
   that old digests are unchanged.

2. **SERIOUS — `crates/konnect/tests/asset_references.rs:177-197`.**
   `agents_make_claimed_evidence_executable`'s `cases` array still names only
   `kicad-schematic-build-agent.md` and `kicad-design-review-agent.md`. The
   change's two *new* gate promises carry no marker case at all:
   `kicad-pcb-layout-agent.md:50-101` step 2 (`approval_valid` /
   `INCOMPLETE` / `place nothing`) and `pcb-design-reconstruction-agent.md:51-60`
   (Phase 0) with `:162-163` (Hard rule 1). The sibling promise at
   `kicad-schematic-build-agent.md:79-83` *is* guarded, by the literals
   `"approval_valid"`/`"INCOMPLETE"` at `asset_references.rs:188-189`. The spec
   scenario "an unapproved design brief cannot reach build" says "verified by
   their text in … `kicad-schematic-build-agent.md` and
   `kicad-pcb-layout-agent.md`" **without** the "no automated test exercises
   this in this slice" caveat four neighbouring scenarios carry explicitly.
   `skills_define_the_same_evidence_boundary_as_their_agents`
   (`asset_references.rs:227-250`) likewise covers none of the three new
   skills. Remedy is two entries in an existing data array; the rule is the
   single most load-bearing one in the change, and today it can be deleted from
   one of the two spec-named files with a green suite. Disclosed as
   `04-developer.md` deferred finding 4 — honest, but still the gap.

3. **MINOR — `README.md:345`** still reads "6 skills + 2 agents bundled".
   `crates/konnect/src/manifest.rs` now ships **10** skills (`:35-150`) and
   **5** agents (`:160-176`). It was already stale at 7+4 before this change
   and `doc_tool_counts.rs` sweeps only tool counts, so nothing catches it.

4. **MINOR — silent view overwrite.**
   `photo_intake.rs:647-651` names the file `<label>.png`, and `:1480-1486`
   `next_view_number` = `1 + file count`. A repeated `label`, or a numbered
   call after any view file was deleted, overwrites an existing view with no
   warning. View bytes are outside the approval hash, so a `{view, rect_px}`
   evidence pointer a human approved can silently come to mean a different
   crop. The numbering rule is D1 verbatim, so this is a documentation gap, not
   a deviation: neither the tool description (`:1845-1850`) nor
   `kicad-board-dossier/SKILL.md:99-103` nor `dossier-schema.md:38` says a
   label is overwrite-on-reuse.

5. **MINOR — `kicad-board-dossier/SKILL.md:313`** teaches the worked example
   `"The silkscreen reads \`SEMAFARO 1.3 24V 03/2020\`"` as the `observed`
   archetype. That is the prototype's reading (`00-orchestrator…md` item 1);
   the change's own acceptance run (`05-orchestrator-checklist-6.1.md` item 1)
   recorded the agent reading `SEMAFORO L3 24V 03/2020` and both as marginal.
   Stale text in the one row that models what "observed" means.

## Evidence

1. **(a) conditional hash — verified, no issue.** `hashed_fields` is
   byte-for-byte untouched (`git diff … | grep '^[-+].*fn hashed_fields\b'` →
   empty); the conditional work is the new `hashed_fields_with_optional`
   (`photo_intake.rs:988-1003`, which calls `hashed_fields(value, keys, &[])`
   — the *same* empty `empty_string_is_null` the old scale_reference arm passed
   at old `:232`) plus the second loop at `:1084-1088`. `skip_serializing_if`
   is on all four new `Option`s (`:810`, `:823`, `:851`, `:856`), asserted at
   `:3881-3893`. `"dossier": null` is refused at `:1273-1282` and also blocks
   `approve` via `handle_approve_photo_review_map`'s `validate_incoming_map`
   call (`:2350`). **An empty `{}` cannot slip through:** the insert at `:1085`
   is `section.clone()`, so the canonical bytes gain a literal `"dossier":{}`
   member that an absent section does not have — adding and removing it both
   move the digest, and `removing_an_approved_dossier_drops_the_section…`
   (`:3845`) proves the removal direction. `TOOL_OWNED_KEYS` (`:2177-2183`)
   correctly stays at the five bookkeeping names; `dossier`/`design_brief` are
   content and are carried through by `overlay_known_fields`.
2. **(b) `prepare_board_photo` — verified, no issue** except MINOR-4.
   `existing_map_dir` (never `prepare_map_dir`) at `:604`; token rule on
   `label` before any join at `:624-628`; `prepare_views_dir` re-canonicalizes
   and `starts_with`es at `:1456-1476`. `apply_orientation` at `:1390` runs
   **before** the bounds check and `crop_imm` at `:1393-1410`; the error names
   the oriented size and the applied variant (`:1403-1406`). Header MP check
   before decode at `:1370-1379`; `Limits::no_limits()` + `max_alloc = 512 MiB`
   at `:1358-1360`; scale rejected not clamped at `:1535-1541`; output side cap
   checked before `resize_exact` at `:1424-1430`. `jpeg` is on at
   `Cargo.toml:90` with `zune-jpeg` in `Cargo.lock`, and `image.workspace`
   moved to `[dependencies]` (`konnect-core/Cargo.toml:37`). `mm_per_px` only
   from the saved map's resolved value (`:675-684`). `view_path` goes through
   `subprocess_arg` (`:687`), whose three-branch VerbatimUNC handling is the
   correct one (`:277-294`, tested `:2586-2607`). Synchronous decode in the
   async handler is bounded by the four caps, as D1 argues (`04`/`03` deferred
   finding 4 — accepted).
3. **(c)+(d) — verified, no issue.** `mm_per_px`/`evidence` enter the hash via
   `SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS` (`:909`, used at `:1056-1060`),
   proved both directions at `:3799-3838`; the drift guard at `:3476-3491`
   makes a third scale field a build failure. The dossier's `scale_status` /
   `board_size_mm: null` rules (`:1657-1659`, `SKILL.md:194-231`) agree with it
   — no contradiction. **No array of objects gained `items`:** the only `items`
   in the two new subschemas are `photo_views_used`,
   `depends_on_open_questions`, `open_questions`, `unresolved`, `assumptions`
   (string) and `board_size_px` (integer), none a record, so
   `router/mod.rs:213-228` stays at exactly D7's six paths.
4. **(e) assets — verified, no issue.** The intake agent's
   `tools:\n  - mcp__konnect__*\n  - Read` is valid YAML (`*` is not the first
   character, so no alias) and `yaml_list` (`asset_references.rs:151-168`)
   parses `  - ` entries. `NOT_TOOLS` grew by exactly **73** names inside the
   photo-intake block, which `not_tools_allowlist.rs:34-64` parses from its
   marker to `];` — so all 73 are in scope of
   `no_photo_intake_allowlist_entry_is_a_real_tool_parameter`, whose
   `registered_parameters()` (`:68-80`) reads **every** registered tool's
   top-level properties. Spot-checked 14 of the riskiest against the sources
   (`position_mm`, `diameter_mm`, `pitch_mm`, `layer_count`, `keep_outs`,
   `board_size_mm`, `mounting_holes`, `net_currents`, `kicad_symbol`,
   `resolution_status`, `open_questions`, `block_diagram`, `mm_per_px`,
   `scale_status`): zero appear as a tool input property; `layer_count` exists
   only as a config value and a `get_board_info` *response* field
   (`pcb_board.rs:1585`). Both consumer sections refuse unambiguously
   (`kicad-pcb-layout-agent.md:60-65`, `kicad-schematic-build-agent.md:79-83`)
   and the layout agent's nine mapping rows are verbatim
   `layout-methodology.md:30-38` with the load-bearing set from `:56-57`.
   `kicad-photo-to-board/SKILL.md:78-104` states "agents cannot spawn agents"
   and that the session invokes each stage; the intake agent repeats it at
   `:157`. Its "Honest limits" (`:139-161`: 50 MP, 0.25-4.0, 4096 px, text-only
   gate) match the code exactly. No contradiction between the dossier skill's
   honesty rules (`SKILL.md:322-344`) and the schema descriptions
   (`photo_intake.rs:1655-1659`).
5. **(f)+(g)+(h) — verified, no issue.** `sum(tool_count)` over
   `registry.rs` = **232** across **22** toolsets; README (`:16`, `:73`), DEV
   (`:331`, `:416`, `:421`), `tool-directory.md:16`+`:390`,
   `TROUBLESHOOTING.md:369`, `metadata.json:4`, `plugin.json:4` all say
   232/239 and no file still says 231/238. `PHOTO_TO_BOARD_WORKFLOW.md:126-172`
   is accurate against the code and cites no brittle line numbers. `8be8d66` is
   a pure `type CropRect = [u32; 4];` alias used in two signatures, +5/-2, zero
   behaviour change. **No false claim found in 03/04/06**: `hashed_fields`
   untouched ✓, digest unchanged and the fixture's four new fields all `None`
   ✓, render-before-`views/` ordering ✓ (`:665` after `:657`), 73 names ✓, the
   `not_tools_allowlist` parser really does span the new block ✓, `install.rs`
   really does iterate `AGENTS` (`install.rs:708`) ✓, `+5/-2` ✓, and
   `04`'s deferred finding 4 is exactly the SERIOUS above ✓.

## For the next agent

1. Fix is two entries in `asset_references.rs`'s `cases` array at `:177`:
   `("kicad-pcb-layout-agent.md", &["photo_intake", …], &["approval_valid",
   "INCOMPLETE", "physical_constraints"])` and
   `("pcb-design-reconstruction-agent.md", &["photo_intake", "library"],
   &["approval_valid", "INCOMPLETE", "resolution_status"])`. Mutate each marker
   out once to prove the case can fail before calling it done.
2. Do **not** widen `not_tools_allowlist.rs`'s scope while fixing anything —
   its doc comment (`:19-23`) names nine pre-existing violators elsewhere in
   `NOT_TOOLS`; widening turns a real invariant into a known-red one.
3. MINOR-3 and MINOR-4 are one-line doc edits (README table row; one sentence
   in the `prepare_board_photo` description or `kicad-board-dossier/SKILL.md`
   §2 saying a reused `label` replaces the file). MINOR-5 is a two-token edit
   at `SKILL.md:313`.
4. The pinned digest reproduces from the canonical form, not from the
   implementation — re-derive it the same way (`json.dumps(..., sort_keys=True,
   separators=(',',':'))` + sha256 over a `covered` object with no optional
   sections) after any future hash change rather than re-pinning what the code
   emits.
5. `every_schema_key_is_either_hashed_or_deliberately_not` (`:3452`) now reads
   `review_map_schema()` and guards `scale_reference`'s own key set too — a
   third optional section, or a fifth `scale_reference` field, is a build
   failure. That guard is the right place to extend, not the fixture.

## Deferred findings

1. `scale_reference.mm_per_px`/`evidence` have **no** null-rejection, unlike
   `dossier`/`design_brief` (`:1273-1282`). `overlay_known_fields` preserves an
   incoming `"mm_per_px": null` onto disk, where `:1000` reads it as
   `Some(Null)` and it joins the hash as a present-but-empty member — the exact
   shape that code comment calls "the dangerous one". Harmless today: the MCP
   layer validates against draft 2020-12 (`tools/mod.rs:440-442`,
   `mcp/handler.rs:366`) and the schema types it `number`/`string`, and both
   directions fail *closed*. Worth a null-check if a second in-process caller
   ever appears.
2. Nothing verifies that a `dossier.evidence[].view` names a file that exists
   or that its `rect_px` fits inside it (`03` deferred finding 5). Pre-mortem
   cause 3's hardening rests on that pointer being openable, and only the
   one-off D12 checklist ever checked it.
3. No test covers a `"dossier": {}` section. The proof that `{}` is
   distinguishable from absent is analytic (BTreeMap emits the member) and
   holds, but it is the one cell of the presence matrix with no assertion.
4. `04`'s deferred findings 1, 2, 3 and 5 (D9 phase renumbering, the
   `kicad-photo-to-board` empty `references:`, unchecked cross-skill reference
   pointers, three copies of the nine-row mapping table) all still stand and
   are all correctly described.
5. `skills_define_the_same_evidence_boundary_as_their_agents`
   (`asset_references.rs:227`) is the second hard-coded marker list in the same
   file; it covers 2 of 10 bundled skills. Folding both lists into one
   data-driven table is the durable fix, larger than this round.
