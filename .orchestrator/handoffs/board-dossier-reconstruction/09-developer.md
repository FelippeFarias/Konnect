---
change: board-dossier-reconstruction
task: fix round 1
agent: developer
verdict: DONE
failing_layer: none
---

## Result

1. **All four round-1 findings closed**, one commit per finding, on
   `orc/board-dossier-reconstruction` at `f7c329c` (worktree
   `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\board-dossier-reconstruction`),
   working tree clean.
2. SERIOUS (finding 1): `4297198` adds `kicad-pcb-layout-agent.md` (required
   toolsets `photo_intake`, `pcb_export`; markers `load_photo_review_map`,
   `approval_valid`, `INCOMPLETE`) and `pcb-design-reconstruction-agent.md`
   (toolsets `photo_intake`, `library`; markers `load_photo_review_map`,
   `approval_valid`, `search_symbols`, `search_footprints`, `INCOMPLETE`) to
   `agents_make_claimed_evidence_executable`'s `cases`, and
   `kicad-board-dossier/SKILL.md`, `kicad-design-reconstruction/SKILL.md`,
   `kicad-photo-to-board/SKILL.md` to
   `skills_define_the_same_evidence_boundary_as_their_agents`'s `cases`.
3. MINOR (finding 2): `98ddcf3` corrects README.md:345 from "6 skills + 2
   agents bundled" to "10 skills + 5 agents bundled", counted from
   `manifest.rs`'s `SKILLS`/`AGENTS` arrays (10 `SkillManifest {` / 5
   `AgentManifest {`).
4. MINOR (finding 3): `f982824` makes `prepare_board_photo` refuse to
   overwrite an existing view file (error names the path), before the
   `image::save` call in `handle_prepare_board_photo`
   (`photo_intake.rs:651-663`); updates the tool description and
   `kicad-board-dossier/SKILL.md`'s labelling paragraph to say so; adds
   `a_reused_label_is_refused_and_the_original_view_is_unchanged`.
5. MINOR (finding 4): `f7c329c` swaps `kicad-board-dossier/SKILL.md:313`'s
   worked example from the prototype's `SEMAFARO 1.3 24V 03/2020` to the
   6.1 run's actual `SEMAFORO L3 24V 03/2020`, with a parenthetical that a
   marginal glyph is recorded as an alternative, not silently picked.

## Evidence

1. `cargo test -p konnect --test asset_references`: 12/12 green after the
   fix. Each of the five new cases was proven to bite by mutating the asset
   and re-running: deleting `load_photo_review_map` from
   `kicad-pcb-layout-agent.md` → `agents/kicad-pcb-layout-agent.md does not
   prescribe 'load_photo_review_map'`; deleting `search_footprints` from
   `pcb-design-reconstruction-agent.md` → `... does not prescribe
   'search_footprints'`; dropping its `load_toolset("library")` line → `...
   does not load 'library'`; deleting `prepare_board_photo` from
   `kicad-board-dossier/SKILL.md` → `skills/kicad-board-dossier/SKILL.md does
   not define 'prepare_board_photo'`; deleting `physical_constraints` from
   `kicad-design-reconstruction/SKILL.md` → `... does not define
   'physical_constraints'`; deleting `kicad-pcb-layout-agent` from
   `kicad-photo-to-board/SKILL.md` → `... does not define
   'kicad-pcb-layout-agent'`. Every mutation was restored from a `/tmp`
   backup before continuing (`git status --short` showed only the test file
   modified after each restore).
2. `cargo test -p konnect-core --test prepare_board_photo_contract` and the
   `photo_intake::board_view_tests` module (76 tests, includes the new
   reused-label test): all pass. The new test re-saves under the same label
   with a different crop, asserts `result.is_error` and that the error text
   contains the view's path, then re-reads the file and asserts
   `original_bytes == bytes_after` — the refusal is proven to leave the file
   untouched, not merely to return an error.
3. All four required gates, run from the worktree with
   `PROTOC=C:/Users/felip/tools/protoc/bin/protoc.exe` and the VS 2022
   BuildTools CMake `bin` on `PATH`: `cargo test -p konnect` → `EXIT:0`
   (1385+ passed across all binaries, 0 failed); `cargo test -p konnect-core`
   → `EXIT:0` (1385 lib tests + all integration suites, 0 failed, 16+
   ignored as expected — live/manual tests); `cargo fmt --check` → `EXIT:0`;
   `cargo clippy --all-targets` → `EXIT:0`, no warnings.
4. `git log --oneline -5` on the branch: `f7c329c` (docs, finding 4) →
   `f982824` (fix, finding 3) → `98ddcf3` (docs, finding 2) → `4297198`
   (test, finding 1) → `bab650d` (round-1 review's base). `git status
   --short` is empty; nothing pushed.

## For the next agent

1. This closes reviewer handoff `08-reviewer.md`'s four findings in full;
   no further code changes are expected before re-review.
2. Deferred finding 5 in `08-reviewer.md` (the two hard-coded marker-list
   tests could fold into one data-driven table) was deliberately left open —
   it is explicitly out of round-1 scope and larger than a fix round.
3. `08`'s deferred findings 1-4 (the `mm_per_px`/`evidence` null-rejection
   gap, no check that a `dossier.evidence[].view` names a real file, no test
   for a `"dossier": {}` section, and `04`'s four still-open items) all
   still stand — none of them was in this round's four findings.
4. If a reviewer wants to mutate a marker case themselves to re-verify: the
   five new cases are at `crates/konnect/tests/asset_references.rs`'s
   `agents_make_claimed_evidence_executable` (`kicad-pcb-layout-agent.md`,
   `pcb-design-reconstruction-agent.md`) and
   `skills_define_the_same_evidence_boundary_as_their_agents`
   (`kicad-board-dossier`, `kicad-design-reconstruction`,
   `kicad-photo-to-board`).
5. The `pcb_export` toolset was chosen as the layout agent's DRC toolset
   because that is where `get_drc_violations` lives
   (`crates/konnect-core/src/tools/verification.rs:22`), not `verification`
   — worth knowing if a future case names the wrong toolset by guessing from
   the name.

## Deferred findings

(none beyond what `08-reviewer.md` already lists as deferred — see "For the
next agent" above)
