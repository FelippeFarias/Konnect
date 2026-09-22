---
change: board-dossier-reconstruction
task: VERIFY round 2
agent: reviewer
verdict: DONE
failing_layer: none
---

## Result

1. **All four round-1 findings are CLOSED at `f7c329c`.** The four fix commits
   are +121/-4 across exactly four files, touch no hashing, no schema, no
   `NOT_TOOLS`, no `router/mod.rs` allowlist and no manifest; every suite I ran
   is green (`asset_references` 12/12, `photo_intake` lib 72/72,
   `prepare_board_photo_contract` 6/6, `photo_intake_gate_e2e` 5/5,
   `two_checkpoint_approval_contract` 3/3). One new MINOR, in the planner's
   spec prose, is the only finding of this round.

2. **SERIOUS-1 (guard cases) — CLOSED.** Proving line:
   `kicad-pcb-layout-agent.md` contains `load_photo_review_map` **exactly
   once** (`:62`) and `approval_valid` **exactly once** (`:64`), both inside
   the step 1-2 gate, and `asset_references.rs:201-205` now requires both.
   The deletion round 1 said would pass green — removing that agent's gate —
   now fails the suite. `pcb-design-reconstruction-agent.md` is covered at
   `:206-214` (its load-bearing marker is `load_photo_review_map`, 2
   occurrences, both in the gate apparatus at `:53` and `:157`), and the three
   new skills at `:270-299`. Residual (Deferred 1): the guard is a file-wide
   `text.contains` (`:229`, `:307`), so it is section-blind wherever a marker
   repeats.

3. **MINOR-2 (README) — CLOSED and correct.** `README.md:345` reads "10 skills
   + 5 agents bundled"; `crates/konnect/assets/skills/` holds exactly 10
   directories and `assets/agents/` exactly 5 `.md` files, matching
   `manifest.rs`'s `SKILLS` (names at `:35-150`) and `AGENTS` (filenames at
   `:160-176`). Counted from the asset tree, not from the handoff's number.

4. **MINOR-3 (silent view overwrite) — CLOSED.** `photo_intake.rs:660-670`
   refuses when `view_path.exists()`, before `image.save`, with
   `view_path.display()` in the message; `a_reused_label_is_refused_and_the_
   original_view_is_unchanged` (`:4643`) asserts `is_error`, the path in the
   text, **and** byte-identity of the file already on disk. Documented in the
   tool description (`:1866-1868`) and `kicad-board-dossier/SKILL.md:103-106`.
   **MINOR-4 (stale example) — CLOSED:** `SKILL.md:316` now quotes `SEMAFORO
   L3 24V 03/2020`, verbatim the 6.1 run (`05-orchestrator-checklist-6.1.md`
   item 1), and its parenthetical matches that item's "both recorded as
   marginal".

5. **NEW MINOR — `specs/photo-intake/spec.md:201-204` claims more than the
   guard delivers.** It says the cited cases "pin the `load_photo_review_map`,
   `approval_valid`, and `INCOMPLETE` markers in **each** file's text against
   deletion, rather than merely asserting the text is present". Three of the
   five cited cases do not pin `load_photo_review_map` at all
   (`kicad-schematic-build-agent.md` `:179-193`, `kicad-board-dossier/SKILL.md`
   `:270-279` — which pins neither it nor `approval_valid` — and
   `kicad-photo-to-board/SKILL.md` `:292-299`); and the guard **is** a presence
   assertion (`if !text.contains(*marker)`), so the closing clause is false as
   written. The sibling citation at spec `:145-151`, which claims only the
   `approval_valid` marker for `pcb-design-reconstruction-agent.md`, is
   accurate.

## Evidence

1. **(a) Are the new markers load-bearing?** Occurrence counts per file
   (`grep -c`): layout agent — `load_photo_review_map` 1, `approval_valid` 1,
   `INCOMPLETE` 6 ⇒ **yes, the gate cannot be deleted green**. Reconstruction
   agent — `load_photo_review_map` 2 (`:53` Phase 0, `:157` Phase 7),
   `approval_valid` 7, `search_symbols`/`search_footprints` 2 each ⇒ deleting
   Phase 0 (`:51-60`) **alone** stays green; only stripping the gate apparatus
   including `:157` bites. Skills — every marker occurs 2-7 times
   (`approval_valid` 6 in kicad-design-reconstruction, 5 in
   kicad-photo-to-board; `observed`/`inferred`/`open_questions` 6 each in
   kicad-board-dossier) ⇒ **vocabulary guards, not sentence guards**. Same
   standard as the two pre-existing cases round 1 accepted, so this is parity,
   not a new hole — but see Deferred 1.

2. **(b) The overwrite refusal.** Placement is after `prepare_views_dir`
   (`:642`) and after `render_view` (`:637`), before `save` — no partial state
   is left by a refusal, and the new test proves byte-identity.
   **The numbered path does refuse:** `file_name` is computed at `:647-650`
   and the check is on `view_path`, so an auto-number collision
   (`next_view_number` = `1 + read_dir().count()`, `:1495-1500`, after any
   view was deleted) hits the same branch — code-correct, **no test covers it**
   (the new unit test and QA's untracked contract test both exercise the
   labelled path only). Error names the path ✔. `exists()`→`save` is a TOCTOU
   window; `prepare_board_photo` is the only writer of `views/` (sole
   `VIEWS_DIR` consumers are `:1474` and a test at `:4437`), so it is
   single-process-acceptable but not atomic (`create_new` would be).

3. **(b, cont.) The stale-partial-file question.** A `save` that fails
   mid-write leaves a file that now blocks that label forever. The escape is
   in the error string ("remove the existing file yourself if you mean to
   replace it") and the cause is in the tool description; it is **not** in
   `SKILL.md:103-106`, which only says "pick a new label". Recoverable and
   fail-closed, so Deferred 3 rather than a finding.

4. **(c) README count.** Verified from the source of truth, not the diff:
   `ls crates/konnect/assets/skills` = 10 entries, `ls crates/konnect/assets/
   agents` = 5 entries; `manifest.rs` `SKILLS` names run `konnect`,
   `kicad-schematic`, `kicad-pcb`, `kicad-manufacture`, `kicad-review`,
   `kicad-library`, `kicad-photo-intake`, `kicad-board-dossier`,
   `kicad-design-reconstruction`, `kicad-photo-to-board` = 10; `AGENTS`
   filenames = 5. `doc_tool_counts` 6/6 still green (it never covered this row).

5. **(d) No regression of the earlier-clean surface.** `git diff bab650d..
   f7c329c` touches only `README.md`, `photo_intake.rs`,
   `kicad-board-dossier/SKILL.md`, `asset_references.rs`; `hashed_fields`,
   `review_map_content_hash`, the four `skip_serializing_if` options, the
   pinned digest, `TOOL_OWNED_KEYS`, `NOT_TOOLS` and `router/mod.rs:184`'s
   allowlist are byte-untouched, so round 1's verification of them still
   holds. `not_tools_allowlist` 3/3 green (scope not widened, as instructed).
   The new SKILL.md prose adds no backticked snake_case token
   (`SEMAFORO L3 24V 03/2020` has no `_`, so `snake_words` skips it) and
   `backticked_tool_names_in_prose_exist_in_the_registry` is green.

## For the next agent

1. The only actionable item is the spec sentence: `specs/photo-intake/
   spec.md:201-204`. Narrow it to what the cases actually pin — e.g. "which
   pin the `approval_valid` and `INCOMPLETE` markers in both consumer agents'
   text, and `load_photo_review_map` in `kicad-pcb-layout-agent.md`" — and
   drop "rather than merely asserting the text is present", which is exactly
   what `text.contains` does. Do **not** fix it by adding markers to the
   cases without re-checking occurrence counts first (see Deferred 1).

2. The worktree HEAD is `f7c329c` but the tree is **no longer clean**: QA
   added an untracked `crates/konnect-core/tests/prepare_board_photo_reuse_
   contract.rs` (8.5 KB, one test, labelled path only). Decide before archive
   whether it ships (commit it) or is scratch (remove it) — an untracked test
   file is neither reviewed nor run by CI.

3. Nothing in this round changed behaviour reachable by an existing caller
   except `prepare_board_photo`, which went from overwrite-on-collision to
   refuse-on-collision. That is a breaking change for any script that re-ran
   the same label deliberately; it is the intended fix, but it belongs in the
   change's release note, not only in the tool description.

4. Round-1 "For the next agent" items 2, 4 and 5 (do not widen
   `not_tools_allowlist`; re-derive the digest from the canonical form rather
   than re-pinning what the code emits; extend
   `every_schema_key_is_either_hashed_or_deliberately_not` rather than the
   fixture) are all still the right advice and none was violated this round.

5. If a future case names a toolset, take `09-developer.md` item 5 seriously:
   `get_drc_violations` lives in `pcb_export`, not `verification` — the layout
   agent's case is correct for that reason, not by name similarity.

## Deferred findings

1. **The two marker guards are section-blind.** `text.contains` over the whole
   file means a marker that appears elsewhere cannot protect the sentence that
   matters. Concretely: `kicad-schematic-build-agent.md`'s **new** design-brief
   gate (`:83-85`) can be deleted entirely with a green suite, because the
   pre-existing components/nets gate (`:47-53`) keeps `load_photo_review_map`
   and `approval_valid` in the file; likewise Phase 0 of
   `pcb-design-reconstruction-agent.md`. A per-section guard (require the
   marker within N lines of a named heading) is the durable fix, and is the
   same work as round 1's Deferred 5 (folding both hard-coded lists into one
   data-driven table).

2. **The auto-numbered collision path has no test.** `next_view_number` is
   `1 + file count`, so one deleted view makes every subsequent unlabelled
   call refuse the same path indefinitely until the caller passes a `label`,
   deletes the colliding file, or adds a file. Fail-closed and documented, but
   the dead-end is permanent by construction and is proven only by reading.

3. **`view_path.exists()` → `image.save()` is not atomic**, and a file left by
   a failed encode blocks that label permanently. `SKILL.md:103-106` gives only
   "pick a new label"; the remove-the-file escape exists solely in the error
   string.

4. **`README.md:345` is still hand-edited and unguarded** — it was stale at
   7+4 before this change and nothing sweeps it. `doc_tool_counts.rs` covers
   tool counts only; extending it to the skills/agents row from
   `manifest.rs`'s array lengths is a few lines.

5. All of `08-reviewer.md`'s Deferred 1-5 still stand unchanged (the
   `scale_reference.mm_per_px`/`evidence` null-rejection gap, no check that a
   `dossier.evidence[].view` names a real file with an in-bounds `rect_px`, no
   test for a `"dossier": {}` section, `04`'s four open items, and the
   two-hard-coded-lists refactor), and `09-developer.md` correctly reports
   them as untouched.
