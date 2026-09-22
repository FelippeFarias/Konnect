---
change: konnect-orchestrator
task: LEARN — distill L0 (change log + 22 handoffs) into L1 project facts, promote cross-project truths to L2 global role memories, audit/compress memory files, check playbook INDEX consistency
agent: memory-curator
verdict: DONE
---

## Result

Read the full change log (`.orchestrator/log/2026-09-21-konnect-orchestrator.md`, 71 entries)
and every handoff's "For the next agent" + "Deferred findings" section (01-planner.md through
22-developer.md). Finding: almost every role-level lesson this change taught was **already**
recorded in the global `MEMORY.md` files in real time during the work (agents followed the
RECORD rule) — reviewer's one-directional-pairing and rule-imported-from-another-text-keeps-its-
scope bullets, developer's restricted-tool-list and grep-line-based-acceptance bullets, and
planner's fix-round/portable-repro bullets all match this change's findings verbatim. This LEARN
pass therefore focused on (a) two genuinely new cross-project lessons, (b) compressing global
files that were at/over the 60-body-line cap, and (c) rewriting every touched project memory's
stale/bloated `## Current state` to the final archived/merge-pending status.

Files touched (total lines incl. title, via `Grep ^`; global cap = 60 BODY lines = total − 1):

Global `~/.orc/agents/<role>/MEMORY.md`:
- `planner/MEMORY.md`: 62 → 15 total (61 → 14 body). Was 1 line over cap; de-wrapped every
  bullet to one physical line (zero content loss) and added 1 new lesson (group a round's tasks
  by file-set, not by decision, from planner 17's fix-round-2 grouping call).
- `qa/MEMORY.md`: 59 → 59 total (58 → 58 body, unchanged net). Added 1 new lesson (a "file does
  not exist" claim needs the build/install pipeline checked, from developer 06 + qa 10 both
  wrongly claiming `reliability-contract.md` was missing) merged as a corollary into the existing
  "grepped literal-count claim" bullet to stay under cap.
- `memory-curator/MEMORY.md`: 32 → 39 total (31 → 38 body). Added 1 new lesson: a project
  memory's `## Current state` can drift far past its line target via one-bullet-per-round
  appends even when the rest of the file is well kept — always Grep its real line count before
  a LEARN pass, never trust "looks current".
- `architect/MEMORY.md` (55), `developer/MEMORY.md` (60), `reviewer/MEMORY.md` (58): **unchanged**
  — already under/at cap, already reflect this change's findings, no new cross-project lesson
  found for these roles beyond what's already there (checked line-by-line against the log's
  DECISIONS A-N and every handoff's findings; verified no near-duplicates to merge).

Project `.orchestrator/memory/<role>.md` (Current state rewritten to ≤10 lines; total file
line counts before → after):
- `planner.md`: 20 → 19. Current state was 3 bullets describing STALE mid-progress states
  (still said "now mid fix round 1", "Next: @dev from tasks.md task 1.1") — replaced with 2
  bullets reflecting final archived/merge-pending status for all 3 changes.
- `architect.md`: 58 → 57. Current state said "design is closed... READY" for konnect-orchestrator
  (the pre-implementation state); updated to final archived status + `bbd5efb` merge commit.
- `developer.md`: 246 → 204. Current state was ~44 lines of commit-by-commit history across every
  round/handoff — collapsed to 3 lines (final commit, archive status, merge-pending, final
  registry/skill/agent counts); the commit history is superseded now the change is archived, and
  the durable shapes/decisions were already preserved in "## Decisions affecting my role" below it.
- `qa.md`: 59 → 47. Current state compressed from 2 verbose bullets to 2 concise ones with the
  same facts (final commit, archive status, open deferred items kept).
- `reviewer.md`: 58 → 51. Current state compressed similarly; kept the 3 non-blocking MINORs.
- `orchestrator.md`: 54 → 57 (grew: added 3 new Decisions). Current state updated to final
  archived/merge-pending status for all 3 changes + "3 ARCHIVE since last RETRO, none due" (was
  stale at "2 ARCHIVE"). Added 3 new Decisions: `orc merge`'s main-clean check blocking on
  orc-init's own `.claude/` + a local venv (fix: `.git/info/exclude`, park planning dirs);
  stacked changes merge with ONE `orc merge <top-change>` (carries every predecessor's commits);
  splitting an oversized round into sequential same-file sub-deliveries keeps diffs verifiable.
- `memory-curator.md`: 43 → 25. Current state's first two LEARN-pass paragraphs (each 6-16 lines)
  collapsed to one line each; added this pass's one-line summary.

Playbooks: `~/.orc/playbooks/INDEX.md` read and confirmed consistent (3 existing entries, all
still accurate); no extraction for `konnect-orchestrator` per the ≤1-fix-round rule (this change
had 2 fix rounds).

## Evidence

- `Grep "^"` (count mode) on all 6 global `MEMORY.md` files and all 7 project memory files,
  before AND after every edit, confirming: no global file exceeds 60 body lines (planner 14,
  architect 54, developer 59, qa 58, reviewer 57, memory-curator 38); every project memory's
  `## Current state` section is now ≤10 lines (verified by `Read`ing the first 10-12 lines of
  each file after editing — see the six `Read` calls in-session, all show `## Decisions...`
  starting at or before line 12).
- Cross-checked each of the CT's 6 "what this change taught" claims against the log/handoffs
  before writing anything: reviewer-11's one-directional gate rule (log entry 33/36, reviewer
  memory already has it), reviewer-16's scope-widening (log entry 50/53, reviewer memory already
  has it), the reliability-contract.md refutation (log entries 25, 34 — developer 06 and qa 10
  handoffs, confirmed both claimed "does not exist"), `orc merge` main-clean blocks (log entries
  16-17), one-grant stacked merge (log entry 18, "it carries photo-to-kicad-reverse's rebased
  commits"), codex unavailable (log entry 32, already in orchestrator.md), QA's mutation-proof
  guard pattern (already in qa global memory), split-by-file delegations (log entries 19/21/44/59).
- `openspec/changes/archive/2026-09-22-konnect-orchestrator` and commit `9a0f1ce` (log entry 70)
  are the sha/name every updated Current state cites; `bbd5efb` (log entry 18) is the merge
  commit every file's photo/dossier bullet now cites.

## For the next agent

- No project code, STATE.md, or `openspec/` artifacts were touched — this was memory-only.
- The developer/qa/reviewer/architect global `MEMORY.md` files were deliberately left unedited
  where already compliant; don't re-derive lessons from this change's log a second time in a
  future pass — check the global files first, they're current as of `9a0f1ce`.
- `konnect-orchestrator`'s merge to `main` is still pending the user's GRANT (per CT); once
  `orc merge konnect-orchestrator` lands, the "merge to `main` pending the user's GRANT" clause
  in all 6 role project-memory files' Current state (plus `orchestrator.md`) becomes stale and
  should be updated to the merge commit in the next LEARN pass (or sooner if convenient).
- `developer.md`'s dropped commit-by-commit history (03-22-developer.md) is still fully readable
  in the handoffs directory if a future audit needs the exact per-round commit shas; it was not
  deleted from the repo, only removed from the compressed Current state summary.

## Deferred findings

(none — this was a memory-only LEARN pass with no code execution; the change's own deferred
findings, e.g. tool-directory rows missing `warning`/DECISION K text, library Step 6 unverified
live, `flow_defer`'s post-commit lock race, remain owned as recorded in `21-reviewer.md`'s and
`20-qa.md`'s own Deferred findings sections — not restated here since they are code-layer, not
memory-layer, findings)
