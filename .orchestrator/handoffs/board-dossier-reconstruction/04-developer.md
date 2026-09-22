---
change: board-dossier-reconstruction
task: "round B tasks 3.x, 4.x, 5.x, 7.x"
agent: developer
verdict: FIX
failing_layer: guard (crates/konnect/tests/schema_parameter_usage.rs heuristic) — pre-existing since round A's b2eda51, outside this round's write set
---

## Result

1. All 17 assigned tasks implemented in four commits on
   `orc/board-dossier-reconstruction`: `19a61ba` (3.1-3.6), `c7d331b`
   (4.1-4.6 + 5.2), `e3d79a9` (5.1, 5.3-5.5), `a099d9e` (the 6.1 dry-run
   addendum, folded into 3.1/3.2). Worktree clean, nothing pushed.
   `cargo test -p konnect-core`, `cargo fmt --check`, `cargo clippy
   --all-targets` all exit 0; `asset_references` (12), `doc_tool_counts` (6),
   `not_tools_allowlist` (3) and the 15 `install` tests all pass.
   **Addendum landed** (`a099d9e`): `component_survey[].locations[]` now has a
   documented element contract — `region`, `view`, `rect_px`, an **integer**
   `count`, and `kind` (`observation` | `cross_check`) — with the class `count`
   equal to the sum over `observation` entries, which must partition the class;
   `cross_check` is the re-count of already-covered ground and is excluded from
   the sum, while `count_alternatives` keeps its job as a rival total for the
   whole class. `mounting_holes[].role` must literally contain `plated` or
   `unplated`; `connectors[].edge` must start with `top`/`bottom`/`left`/
   `right`. All three rules are in `dossier-schema.md` **and** in the skill's
   methodology (§3, §6, Rule 2), plus a reconciliation instruction in
   `kicad-photo-to-board`'s approval-#1 checklist. No code change — the schema
   node is open by design. `NOT_TOOLS` grew by two (`cross_check`,
   `plated_standoff`) to **73**.
2. **`cargo test -p konnect` does NOT exit 0, and this round did not cause
   it.** `schema_parameter_usage::every_declared_parameter_is_read_by_its_
   registered_handler` fails with `prepare_board_photo.crop: handle_prepare_
   board_photo never reads this declared parameter`. Verified red at `b2eda51`
   (round A's own last commit) with nothing of mine in the tree. Root cause and
   the two candidate one-line fixes are in *For the next agent* item 1 — both
   land outside my write set (konnect-core, or a second test file), so I left
   it red rather than widening the boundary silently.
3. **The round-A/architect claim that `snake_words` only collects two-part
   names is wrong**, and it changes task 5.2's answer: the guard's filter is
   `parts.count() >= 2`, so `design_brief_seed`, `max_component_height_mm` and
   `depends_on_open_questions` are all flagged. The block I added is the guard's
   own failure list: **73 names**, not the ~50 D10 predicted. No top-level tool
   property is among them (`not_tools_allowlist` re-checks that from the
   schemas, and it now covers my block because its parser runs from the
   photo-intake marker to the end of the array).
4. `install.rs` needed **no** change: its agent tests iterate `AGENTS`, and the
   only hard-coded agent list (`claude_installs_the_canonical_reliability_
   contract_offline`) names two agents for a different assertion. Adding
   `pcb-design-reconstruction-agent` to `manifest.rs` was sufficient, and the
   idempotence test now asserts it installs.
5. README.md and DEV.md **did** need hand edits, contrary to task 5.3's note
   that they are "verified by task 1.6's registry change, not edited by hand" —
   `docs_quote_the_registry_tool_counts` compares literal phrases. Six files
   moved together to 232 registered / 239 with meta-tools.

## Evidence

1. **3.1-3.6, 4.1-4.5** `cargo test -p konnect --test asset_references` →
   `12 passed; 0 failed`, including `every_reference_is_reachable_from_its_
   parent_skill`, `agents_preload_existing_skills`, `top_level_skill_routes_
   every_bundled_agent`, `call_examples_name_real_parameters`,
   `tools_listed_beside_a_toolset_belong_to_it` and
   `backticked_tool_names_in_prose_exist_in_the_registry`. Acceptance greps:
   `dossier-schema.md` in its SKILL.md = 1; `design-brief-schema.md` = 2;
   `dossier\|design_brief` in `review-map-schema.md` = 13;
   `kicad-photo-to-board\|pcb-design-reconstruction-agent` in konnect/SKILL.md
   = 3; `Building from an approved design brief` = 1 in each build agent;
   `approval_valid` in `kicad-pcb-layout-agent.md` = 1.
2. **Mutation on 4.3** (restored): renaming all 2 occurrences of
   `pcb-design-reconstruction-agent` in konnect/SKILL.md →
   `top_level_skill_routes_every_bundled_agent` FAILED with `does not route
   bundled agent(s): pcb-design-reconstruction-agent`. **Mutation on 5.2** is
   the guard's own before/after, twice: the pre-block run FAILED naming exactly
   the 71 words I then added, and the addendum's run FAILED naming exactly
   `cross_check` and `plated_standoff`; both post-block runs pass.
3. **4.6 / 5.2** `cargo test -p konnect --test not_tools_allowlist` →
   `3 passed`: `no_photo_intake_allowlist_entry_is_a_real_tool_parameter`,
   `no_photo_intake_allowlist_entry_is_a_registered_tool`,
   `the_photo_intake_tool_parameters_stay_out_of_the_allowlist`. Its parser
   spans marker→`];`, so all 71 new names are inside its scope.
4. **5.1, 5.3-5.5** `cargo test -p konnect --test doc_tool_counts` →
   `6 passed`, including the four previously red
   (`tool_directory_section_headings_match_the_registry`,
   `tool_directory_lists_every_registered_tool`,
   `docs_quote_the_registry_tool_counts`,
   `no_file_quotes_a_stale_catalogue_total`).
   `grep -rn "231\|238"` over README/DEV/tool-directory/TROUBLESHOOTING/
   metadata.json/plugin.json → no match. `cargo test -p konnect --bins install`
   → `15 passed`. `ls` confirms the 3 SKILL.md, 2 references, 1 agent and
   `docs/PHOTO_TO_BOARD_WORKFLOW.md`; `PHOTO_TO_BOARD_WORKFLOW.md` in README = 1.
5. **7.1-7.3** `cargo fmt --check` exit 0; `cargo clippy --all-targets` exit 0,
   0 warnings; `cargo test -p konnect-core` exit 0 (1384 lib + 4 + 12 + 5 e2e,
   18 ignored); `cargo test -p konnect` → every binary green except
   `schema_parameter_usage` (`4 passed; 1 failed`). `git status --short` empty.

## For the next agent

1. **The one red test, with its diagnosis.**
   `schema_parameter_usage.rs`'s `all_function_bodies` skips any `fn` whose
   signature text contains `;`, to ignore trait-method declarations. Both
   `parse_crop` (`-> Result<Option<[u32; 4]>, String>`) and `render_view`
   (`crop: Option<[u32; 4]>`) contain one **inside an array type**, so neither
   enters the transitive index and the `args.get("crop")` literal in
   `parse_crop` is never seen. `parse_rotate`/`parse_scale` have no `;` and
   pass. Two one-line fixes, both outside my write set — **pick one, do not
   weaken the guard**: (a) `crates/konnect-core/.../photo_intake.rs`: add
   `type CropRect = [u32; 4];` and use it in both signatures (behaviour
   unchanged, `[u32; 4]` stays the type); (b)
   `crates/konnect/tests/schema_parameter_usage.rs`: only treat a `;` at
   bracket depth 0 as a declaration marker. (b) is the real bug and fixes every
   future `[T; N]` signature; (a) is the smaller diff.
2. **Running task 6.1 (orchestrator's).** Invoke `pcb-photo-intake-agent` — it
   now owns comprehension. It needs, and will ask for: the **absolute paths of
   the photos** (`C:\Users\felip\Downloads\WhatsApp Unknown 2026-09-18 at
   12.00.58\*.jpeg`, never copied into the repo), the **`project_dir`** of a
   real KiCad project, and the **scale reference**. For D12 item 8 to score,
   answer the scale question with "not supplied / I don't have a measurement" —
   the agent is instructed to leave `board_size_mm` null, fill `scale_status`,
   and list the gap in `open_questions`. The agent runs `check_retrace` first:
   set `RETRACE_PYTHON=C:/…/Konnect/.venv-retrace/Scripts/python.exe` or it
   will stop at Phase 0 and never reach the dossier. Its phases are now
   0 capability, 1 scan, 2 review map, **3 comprehension**, 4 persist, 5
   approval, 6 handoff.
3. **Where the D12 checklist values live in the produced JSON**, now all
   machine-checkable: `dossier.identity.summary` / `.evidence[0].view`;
   `dossier.silkscreen_markings[].text` + `.basis`;
   `dossier.component_survey[]` keyed by `visual_class` (the skill's two worked
   tokens are `led_5mm_clear` and `axial_resistor_tht`), with `count`,
   `count_method`, and **item 4's sum = `sum(l.count for l in locations if
   l.kind == "observation")`**; `dossier.physical.mounting_holes[]` filtered by
   `"plated" in role and "unplated" not in role`, each with `.position_px`;
   `dossier.physical.connectors[].edge.startswith("bottom")`;
   `dossier.topology_claims[].hypotheses[].calculation`/`.assumptions`;
   `dossier.physical.board_size_mm`/`.scale_status`; `scale_reference.mm_per_px`.
   If a re-run still writes group counts in prose, that is the skill text
   losing, not the schema — §3 and Rule 2 are the exact lines to cite.
4. **For verifiers.** The four asset guards that bite hardest on this change,
   in the order they fire: `backticked_tool_names_in_prose_exist_in_the_
   registry` (every 2-**or-more**-part snake word in any `assets/**/*.md`,
   backticked or bare), `call_examples_name_real_parameters` (a `tool(a, b)`
   whose args are all bare lowercase identifiers must name real properties
   **and every required one** — the five signatures used in the new assets are
   `prepare_board_photo(image_path, project_dir, map_id, crop, rotate, scale,
   label)`, `save_photo_review_map(project_dir, map)`,
   `load_photo_review_map(project_dir, map_id)`,
   `approve_photo_review_map(project_dir, map_id)`, `search_symbols(query)` /
   `search_footprints(query)`), `tools_listed_beside_a_toolset_belong_to_it`
   (the `# …` after a `load_toolset('x')` may name only x's tools), and
   `no_file_quotes_a_stale_catalogue_total`, which sweeps **every** `.md`/
   `.json` under the repo root — a new doc quoting a three-digit count before
   the word "tools" fails it on the day it is written.
5. **Spec scenarios that are text-verified, and where the text is**: "an
   inferred topology claim states its calculation and assumptions", "a count
   method disagreement is recorded", "an unresolved question keeps competing
   hypotheses" → `kicad-board-dossier/SKILL.md` §3, §7 and Rules 2-3;
   "agent-resolved scale reference is recorded with evidence" → §6 and Rule 4;
   "every BOM entry names a real library part" →
   `kicad-design-reconstruction/SKILL.md` §4 and Rule 2; "design
   reconstruction is refused before the dossier is approved" →
   `pcb-design-reconstruction-agent.md` Phase 0 and Hard rule 1.

## Deferred findings

1. `design.md` D9 says the intake agent's Phase 5 is "Handoff"; with the
   comprehension phase inserted the file now runs 0-6 and the handoff is Phase
   6. Renumbering was unavoidable — `openspec` artifacts are not in my write
   set, so D9's phase numbers are now one behind the asset.
2. `kicad-photo-to-board` has no `references/` directory, so
   `every_reference_is_reachable_from_its_parent_skill` has nothing to check
   there, and its `SkillManifest` carries `references: &[]`. If a later slice
   gives it one, the manifest entry must gain it or the file installs nowhere.
3. Nothing in the asset guards checks a **cross-skill** reference pointer: the
   new SKILL.md files name `kicad-photo-intake/references/review-map-schema.md`
   and `kicad-pcb/references/layout-methodology.md` in prose, and a rename of
   either would leave a dangling instruction no test catches. Same class as the
   round-A finding about `dossier.evidence[].view`.
4. `agents_make_claimed_evidence_executable` still hard-codes only
   `kicad-schematic-build-agent` and `kicad-design-review-agent`. The two new
   consumer sections and the whole of `pcb-design-reconstruction-agent` carry
   no marker list, so their `approval_valid` / `INCOMPLETE` promises can be
   deleted without a test noticing. Adding two cases there is a small,
   self-contained follow-up.
5. The layout agent's new section reproduces D5's nine-row mapping table
   verbatim, and so does `design-brief-schema.md`. Two copies of one table now
   exist in `assets/`, with `layout-methodology.md` as the third source of the
   row names. Nothing keeps them in sync.
