---
change: board-dossier-reconstruction
task: create the OpenSpec change (proposal, spec delta, design, tasks)
agent: planner
verdict: DONE
failing_layer: n/a
---

## Result

1. Created `openspec/changes/board-dossier-reconstruction/` in the main
   Konnect repo (`orc new change`, schema `spec-driven`) with
   `proposal.md`, `specs/photo-intake/spec.md` (delta), `design.md`,
   `tasks.md` — all four artifacts written, no scaffold placeholders left.
2. `orc validate --changes board-dossier-reconstruction --json` and
   `openspec validate --changes board-dossier-reconstruction --json` both
   report `"valid": true`, zero issues.
3. `tasks.md`: 31 tasks, `grep -c "^Stack: none"` = `grep -c "^- \[ \]"` =
   `grep -c "^Acceptance:"` = 31.
4. `design.md` defines the `dossier` (D4) and `design_brief` (D5) canonical
   JSON schemas in full, and states the hash-coverage decision (D6:
   conditional inclusion — `dossier`/`design_brief`/`scale_reference.
   mm_per_px`/`.evidence` are hashed only when present, so every
   already-approved map's digest is unchanged and the archived pinned-digest
   test needs no edit).
5. Incorporated the coordinator's mid-task scope addendum in full: the
   `kicad-photo-to-board` top-level workflow skill (design D9's stage
   table), `docs/PHOTO_TO_BOARD_WORKFLOW.md` (design D11), and every claim
   type from `.orchestrator/handoffs/board-dossier-reconstruction/
   00-orchestrator-dossier-prototype.md` given a schema field (`basis`+
   `confidence` pair, `count_method`/`count_alternatives`, `hypotheses[]`
   for competing claims, `resolution_path`, `design_brief_seed`,
   `physical.board_size_px`/`scale_status` for the unresolved-scale case).

## Evidence

1. `openspec/changes/board-dossier-reconstruction/{proposal.md,design.md,
   tasks.md,specs/photo-intake/spec.md}` — all four written this session.
2. `orc validate --changes board-dossier-reconstruction --json` →
   `{"valid": true, "issues": []}` (run twice, via both `orc` and `openspec`
   binaries — both resolve and pass).
3. Read in full before writing: archived `photo-to-kicad-reverse`'s
   `design.md` (D1-D16) and `specs/photo-intake/spec.md`; the current
   worktree's `photo_intake.rs` (3415 lines, function inventory grepped);
   `asset_references.rs`'s `NOT_TOOLS` and open-schema allowlist test;
   `doc_tool_counts.rs`; `pcb-photo-intake-agent.md`;
   `kicad-schematic-build-agent.md`'s existing consumer section;
   `review-map-schema.md`; the concept-validation handoff
   (`00-orchestrator-dossier-prototype.md`).
4. Confirmed `image = { version = "0.25", default-features = false,
   features = ["png"] }` at root `Cargo.toml:90`, and that `konnect-core`
   currently has it only as `[dev-dependencies]` (`Cargo.toml:47`) — design
   D2's Cargo.toml edit is stated against the real current lines, not
   inferred.
5. Confirmed current registry counts (231 registered / 238 total,
   `photo_intake` `tool_count: 5`) via `grep` on `registry.rs` and
   `doc_tool_counts.rs`'s `required_phrases()`, so tasks 1.6/5.3/5.4 bump
   from a verified baseline, not the orchestrator's stated 232/239 (which
   matches once `+1` is applied).

## For the next agent

1. `dev` (@dev) starts from task 1.1 in the worktree named in `tasks.md`'s
   header; the worktree's `git status` was clean and already on
   `orc/board-dossier-reconstruction` at this session's start — no branch
   setup needed.
2. Task 2.2's exact allowlist path list in `router/mod.rs` is stated as
   "finalized when `review_map_schema()` is written" (design D7) — the dev
   agent adds any path the test's own failure names, not just the list
   design D7 pre-populates; treat that list as a floor, not a ceiling.
3. Task 5.2's `NOT_TOOLS` list is explicitly derived mechanically from
   `cargo test -p konnect --test asset_references
   backticked_tool_names_in_prose_exist_in_the_registry`'s failures, per the
   `add-external-cli-toolset` playbook's step 8 — design D10's list is a
   starting point, not the final one; do not skip running the test to
   finalize it.
4. Task 6.1 (orchestrator checklist against the real photos) is not `dev`'s
   or `qa`'s to close — it names the orchestrator as the runner explicitly,
   after every other task is implemented and green.
5. The `add-external-cli-toolset` playbook is still "not yet validated" —
   this change is the wrapper's second use (extension, not a fresh wrap).
   Whoever runs the LEARN phase should fold this change's two concrete
   findings (open-schema allowlist path list grows with every new open
   section; conditional hash-field inclusion is the pattern for "extend a
   hash without breaking old approvals") into it or into `memory-curator`'s
   pass.

## Deferred findings

1. Design D9 states `pcb-design-reconstruction-agent` loads the `library`
   toolset read-only (`search_symbols`/`search_footprints` only) but this
   was not cross-checked against whether `library`'s `tool!` registration
   lets an agent load a toolset and call only some of its tools by
   convention alone (no partial-toolset-loading mechanism was found in the
   archived design or this session's reading) — `dev` should verify
   `pcb-design-reconstruction-agent.md`'s Hard Rules are sufficient to keep
   it from calling a mutating `library` tool that loading the whole toolset
   exposes, or flag this to `@architect` if a narrower mechanism is needed.

## Result (verify round 1 spec fix)

1. Fixed the gap QA (`07-qa.md` deferred finding 4) and the reviewer
   (`08-reviewer.md` finding 1) both flagged: `specs/photo-intake/spec.md`'s
   "an unapproved design brief cannot reach build" scenario cited two agent
   files as its verification with no caveat, unlike its siblings. Reworded
   its THEN to name the guard tests a developer is adding
   (`agents_make_claimed_evidence_executable`'s new cases for
   `kicad-pcb-layout-agent.md`/`pcb-design-reconstruction-agent.md`,
   `skills_define_the_same_evidence_boundary_as_their_agents`'s cases for the
   three new skills) as the automated check, citing the `load_photo_review_map`/
   `approval_valid`/`INCOMPLETE` markers the coordinator specified.
2. Swept the whole spec delta for the same pattern rather than stopping at
   the one flagged scenario (the coordinator's rule — "each names either a
   guard test or states 'no automated test in this slice'" — reads as
   file-wide, not scoped to one requirement). Found and fixed three more:
   "design reconstruction is refused before the dossier is approved" (now
   cites `agents_make_claimed_evidence_executable`'s confirmed new case for
   `pcb-design-reconstruction-agent.md`, using only the `approval_valid`
   marker already present in its own text — not the unconfirmed
   `resolution_status` marker the reviewer's fix note speculated about),
   "a count method disagreement is recorded, not silently resolved", and
   "an unresolved question keeps competing hypotheses, not one guess" (both
   now carry the same "no automated test... schema keeps `dossier` open by
   design" caveat their sibling "an inferred topology claim" scenario
   already had). Also fixed "agent-resolved scale reference is recorded with
   evidence" (MODIFIED requirement, same bare-citation gap, not named by
   either handoff but caught by the sweep).
3. `orc validate --changes board-dossier-reconstruction --json` → `valid:
   true` after every edit in this round (checked twice: once after the
   named fix, once after the sweep's three additional fixes).
4. Nothing outside `specs/photo-intake/spec.md` was touched — `proposal.md`,
   `design.md`, `tasks.md` are unchanged from the original planning round.
5. Did not add a marker-name guess for the `skills_define_the_same_evidence_
   boundary_as_their_agents` case beyond what the coordinator's message
   confirmed (it covers "the three new skills" but the message did not name
   per-skill markers) — the "unapproved design brief" scenario cites the
   test by name only, not by an invented marker list for that specific
   guard.

## Result (verify round 2 spec fix)

1. Fixed the mechanism-description error reviewer `10-reviewer.md` flagged:
   the "an unapproved design brief cannot reach build" scenario's THEN
   claimed the guard cases "pin" the three markers "in each file's text ...
   rather than merely asserting the text is present" — both halves were
   wrong. Read both test bodies directly in the worktree
   (`crates/konnect/tests/asset_references.rs`,
   `agents_make_claimed_evidence_executable` at `:175`,
   `skills_define_the_same_evidence_boundary_as_their_agents` at `:246`):
   both are exactly `text.contains(*marker)` per file — a presence
   assertion, nothing more (no mutation test, nothing that proves the prose
   *enforces* anything).
2. Also found the marker list I'd previously written was factually wrong
   per file, not just mischaracterized in mechanism: `kicad-schematic-
   build-agent.md`'s case (`:178-191`) does **not** require
   `load_photo_review_map` (only `approval_valid`/`INCOMPLETE`, among
   ERC-related markers irrelevant to this scenario); only
   `kicad-pcb-layout-agent.md`'s case (`:197-204`) carries all three.
   Corrected the THEN to name each file's actual required markers rather
   than claiming the same three for every cited file.
3. Narrowed "the three new skills" to the two whose cases actually contain
   markers relevant to this specific claim — `kicad-design-reconstruction/
   SKILL.md` (`load_photo_review_map`, `approval_valid`, `INCOMPLETE`) and
   `kicad-photo-to-board/SKILL.md` (`approve_photo_review_map`,
   `approval_valid`, `INCOMPLETE`). Dropped `kicad-board-dossier/SKILL.md`
   from this citation: its case (`:270-279`) requires `prepare_board_photo`/
   `save_photo_review_map`/`observed`/`inferred`/`open_questions`/
   `INCOMPLETE` — none of which back a claim about the *design-brief* build
   gate, since that skill covers the dossier phase, not design
   reconstruction. Citing it here would have repeated the same
   inaccuracy-by-overgeneralization the coordinator asked me to fix, just
   at the file-selection level instead of the marker-list level.
4. `orc validate --changes board-dossier-reconstruction --json` → `valid:
   true` after this edit.
5. Only this one scenario's THEN changed; nothing else in `specs/
   photo-intake/spec.md`, and no other artifact, was touched.
