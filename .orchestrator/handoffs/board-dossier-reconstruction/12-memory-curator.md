---
change: board-dossier-reconstruction
task: LEARN pass
agent: memory-curator
verdict: DONE
failing_layer: n/a
---

## Result
1. Compressed the 4 over-cap global role files by merging near-duplicate bullets into themed
   multi-case bullets and removing inherited ~85-char hard-wrapping — no content deleted, every
   distinct scenario still findable. Total lines: `architect` 85→54, `developer` 102→21, `qa`
   95→38, `reviewer` 89→37. All bodies (total−1 title line) now well under the 60-line cap.
2. Added one new cross-project lesson to `qa` global (new "##" section): a doc "N things bundled"
   claim verified by `grep -c "<Type> {"` over-counts when the literal is reused elsewhere — QA
   and reviewer both hit this independently this round (manifest struct literal counted 16
   against a true 10); count the real source of truth (line-by-line read, or the actual
   directory/table) instead.
3. `add-external-cli-toolset.md` marked `validated: 2026-09-18`, provenance
   `Konnect/board-dossier-reconstruction` (re-used the full shape, shipped in one fix round).
   Folded in 3 new steps/corrections (snake_words is 2+-part not exactly-two; doc-count ground
   truth is the asset directory, not a struct-literal grep; hard-coded guard-array marker cases
   needed for every new agent/skill promise) and 3 new failure modes (`;`-in-signature guard trap
   → `type` alias; `skip_serializing_if` required on conditional-hash Option fields; open arrays
   need no `items` to keep the allowlist finite).
4. Extracted new playbook `photo-to-board-acceptance-run.md` from handoff
   `05-orchestrator-checklist-6.1.md`, marked `validated: 2026-09-18`. Both playbooks added to
   `~/.orc/playbooks/INDEX.md`.
5. Updated project L1 `orchestrator.md` (35→54 lines) with this round's facts: the stacked-branch
   `git checkout -B <predecessor-head>` pattern, the stale-binary trap in a hybrid acceptance run
   (session's live MCP connection is the OLD binary — build+launch the release binary fresh),
   the scratch client's harmless daemon-thread stderr trace on exit, `orc state append-history`
   taking no leading `- `, and codex 0.144.6 still unavailable. Retro items: **n/a** — no RETRO
   ran this round (2 ARCHIVE since last RETRO per the L0 log).

## Evidence
1. Line counts verified with `Grep pattern:"^"` (total lines) and `pattern:"^$"` (blanks) before
   and after every edited file — see the table under "For the next agent."
2. New qa lesson traced to `.orchestrator/memory/qa.md`'s Round-B gotcha
   (`grep -c "SkillManifest {" manifest.rs` = 16, true count 10, counted by reading line numbers)
   and `.orchestrator/memory/reviewer.md`'s matching independent "Ground truth ... `ls
   crates/konnect/assets/{skills,agents}` = 10/5" finding — genuinely cross-project (any codebase
   with a reused struct/type literal), not Konnect-specific, so promoted rather than left in L1.
3. Playbook failure modes 3-5 traced to `.orchestrator/memory/developer.md`'s
   `schema_parameter_usage` `;`-trap gotcha (fixed by `type CropRect = [u32; 4];`) and the
   `architect.md`/`developer.md`/`reviewer.md` `skip_serializing_if` / open-array lessons, all
   already generalized in the global files — the playbook now cites them by name instead of
   re-deriving.
4. `photo-to-board-acceptance-run.md` content is drawn verbatim from
   `05-orchestrator-checklist-6.1.md`'s Result/Evidence/For-the-next-agent sections (7/8 PASS on
   design checklist D12; the stale-binary and daemon-stderr details come from this task's own CT).
5. L0 log `2026-09-18-board-dossier-reconstruction.md` line 29: "ARCHIVE -> LEARN: 2 ARCHIVE since
   last RETRO (no retro); memory-curator pass" — matches the "record n/a" instruction exactly.

## For the next agent
1. Line-count table (Grep-verified, total lines / body = total−1, cap 60):
   `architect` 85→54 (53 body) · `developer` 102→21 (20 body) · `qa` 95→38 (37 body, includes the
   new section) · `reviewer` 89→37 (36 body) · `planner` 60→60 unchanged (59 body, already under
   cap — no genuinely new cross-project planner lesson this round) · `memory-curator` global
   16→23 (22 body, added the wrap-width compression technique for future LEARN passes).
2. Cap check: all touched global MEMORY.md files are within the 60-body-line cap; none need a
   follow-up compression pass.
3. Second-playbook decision: **extracted** (`photo-to-board-acceptance-run.md`). The acceptance-
   run shape (release binary over stdio, Read-capable subagent for comprehension, checklist-score
   against the design doc, route substantive-but-not-blocking gaps to a later round) is reusable
   for the still-queued Slices 2-3 of this photo-to-board line of work (`planner.md`'s L1 note).
4. Retro items: **n/a** — no RETRO ran this round; nothing to verify landed in a STATE.md queue.
5. Untouched by design: `planner`/`orchestrator`/`analyst`/`researcher` global MEMORY.md — this
   change's cross-project lessons were either already folded into the 4 compressed files during
   the prior (`photo-to-kicad-reverse`) LEARN pass, or are Konnect/Rust-specific and correctly
   stay in L1 (already a standing deferred finding, not re-actioned).

## Deferred findings
1. No `rust` stack profile exists yet in this orc installation, so Rust/serde/image-crate/Windows
   lessons (CRLF patching, `image` 0.25 API quirks, EXIF chunk splicing, verbatim-path stripping)
   stay parked in L1 `developer.md`/`architect.md` rather than a stack-scoped global — a standing
   deferred finding from the prior pass, still true, not re-actioned here.
2. `qa` and `reviewer` L1 project files both independently hit the same doc-count over-counting
   defect this round (duplicate discovery, not duplicate work) — whether QA/reviewer verify steps
   should cross-check each other's in-flight L1 notes is a workflow question outside LEARN's
   Read/Edit/Write/Grep tool grant; not actioned.
3. Both playbooks assume Konnect's specific `asset_references.rs`/MCP-stdio shape — reusing this
   pattern outside Konnect would need a generalization pass first; not needed today.
