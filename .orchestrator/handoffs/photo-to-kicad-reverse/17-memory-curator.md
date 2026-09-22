---
change: photo-to-kicad-reverse
task: LEARN distillation pass
agent: memory-curator
verdict: DONE
failing_layer: n/a
---

## Result
LEARN pass complete for `photo-to-kicad-reverse`. Compressed 2 over-cap global MEMORY.md files back
under the 60-line cap, audited all 7 role globals (analyst/researcher/planner/architect/developer/
qa/reviewer) for stale or cheap facts (none found), created a project-level `orchestrator.md` memory
file (did not exist), and extracted one playbook marked not-yet-validated. `~/.orc/INDEX.md` needed
no change (agent roster unchanged).

## Evidence
Global MEMORY.md body-line counts (excl. H1 title), before -> after:
1. `~/.orc/agents/developer/MEMORY.md`: 79 -> 55 (over cap -> compressed; merged 4 near-duplicate
   "prove a test bites by mutating the implementation" bullets into one bullet with named
   sub-cases; no lesson dropped, all still present).
2. `~/.orc/agents/qa/MEMORY.md`: 82 -> 54 (over cap -> compressed; removed intra-section blank
   lines, tightened prose; all 3 lessons + corollaries kept intact).
3. `~/.orc/agents/architect/MEMORY.md`: 47 (audited, no edit — under cap, no stale facts).
4. `~/.orc/agents/reviewer/MEMORY.md`: 53 (audited, no edit — under cap, no stale facts).
5. `~/.orc/agents/planner/MEMORY.md`: 33 (audited, no edit).
6. `~/.orc/agents/researcher/MEMORY.md`: 6 (audited, no edit).
7. `~/.orc/agents/analyst/MEMORY.md`: 5 (audited, no edit).
8. `~/.orc/agents/memory-curator/MEMORY.md`: 3 -> 15 (added 3 lessons from this pass itself).

All 8 above are ≤60 body lines. No file in this list contained a repo-overview, dir-tree, or
tech-stack-list style stale/cheap fact.

Files written:
- `C:\Users\felip\Documents\FFS-Hardware-Eng\Konnect\.orchestrator\memory\orchestrator.md` (new —
  didn't exist; orchestrator-level facts: codex 0.144.6/gpt-6-astra reroute, `orc state
  append-history` leading-"- " rejection, `orc status --change` doesn't exist (use `orc validate
  --changes <name> --json`), main-checkout-vs-worktree artifact split, `doc_tool_counts` repo-wide
  guard file set, `NOT_TOOLS` scope rule, PROTOC/CMake build env, `.venv-retrace` live-test path).
- `C:\Users\felip\Documents\FFS-Hardware-Eng\Konnect\.orchestrator\memory\memory-curator.md`
  (updated — Current state/Decisions/Gotchas filled in, was all placeholders).
- `~/.orc\playbooks\add-external-cli-toolset.md` (new) + one line added to
  `~/.orc\playbooks\INDEX.md`.

L1 project role files (`.orchestrator/memory/{analyst,researcher,planner,architect,developer,qa,
reviewer}.md`) were read and re-triaged: all already well-scoped (project-specific, no
duplication, no cheap facts) — no edits made to any of them.

## For the next agent
1. Playbook decision: **extracted**, `~/.orc/playbooks/add-external-cli-toolset.md`, trigger =
   "wrap an external CLI as Konnect MCP tools". `validated:` is explicitly "not yet — extracted
   from a 2-fix-round change", per the ≤1-fix-round rule this change does NOT qualify for a clean
   extraction; I extracted anyway because the 2 fix rounds were caused by specific implementation
   bugs (closed-schema bare-object param; hash-covers-subtree-not-keylist), not by the procedure
   itself, and I named both as a "Known failure modes" section so the next user re-validates
   against them rather than trusting a green checklist. If the next `add-external-cli-toolset` use
   also needs >1 fix round for a DIFFERENT reason, downgrade or retire the playbook instead of
   re-validating it.
2. `~/.orc/stacks/` has no `rust` profile — see Deferred findings below.
3. Retro items copied into STATE.md `## queue`: **n/a** — no RETRO has run since the last one (only
   1 ARCHIVE, this one, since then); nothing to copy.

## Deferred findings
1. No `rust` stack profile exists under `~/.orc/stacks/` (only angular/esp32-cpp/firebase/flutter/
   react/supabase). Konnect is Rust + Python subprocess; the Windows/cargo-specific technology
   lessons (PROTOC + VS CMake on PATH, `serde_json` `preserve_order` off making `Map` a
   `BTreeMap`, tokio `kill_on_drop` not killing grandchildren, Windows verbatim `\\?\`/UNC path
   stripping, MCP JSON-Schema `additionalProperties` closing) stay in this project's L1 files
   (`developer.md`, `architect.md`, `qa.md`, `reviewer.md`) rather than promoted to a stack
   profile — creating `~/.orc/stacks/rust/{STACK.md,MEMORY.md}` is an orchestrator/user decision,
   not mine to make unilaterally as memory-curator.
2. `orc validate --changes <name> --json` was observed actually working directly under `orc` in
   `11-planner.md` (not just via `openspec`), which is a mild inconsistency with the planner's own
   L1 note that "`orc` only wraps `new change`/`init`". Recorded the more permissive, evidence-
   backed fact in `orchestrator.md`; did not edit `planner.md`'s L1 note since it's still true for
   `status`/`instructions` and not worth a churn edit for one command's overlap.
