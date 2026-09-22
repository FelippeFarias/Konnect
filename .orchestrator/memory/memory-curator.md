# memory-curator — project memory

## Current state
- LEARN pass for `photo-to-kicad-reverse` (2026-09-18): compressed global `developer`/`qa` under cap; created `.orchestrator/memory/orchestrator.md`; extracted playbook `add-external-cli-toolset` (not-yet-validated).
- LEARN pass for `board-dossier-reconstruction` (2026-09-18): all 4 touched globals were genuinely over cap (24-42 lines); merged near-duplicates + de-wrapped to compress; added 1 new qa lesson (literal-count over-counting); validated `add-external-cli-toolset`; extracted + validated `photo-to-board-acceptance-run`.
- LEARN pass for `konnect-orchestrator` (2026-09-22, 2-fix-round change, no playbook extracted per the ≤1-fix rule): most role-level lessons were ALREADY recorded in global MEMORY.md files in real time during the change (agents follow the RECORD rule) — this pass mainly compressed 2 globals back under cap (planner 63->15 via de-wrap, qa 62->59 via a merge), added 2 new lessons (planner: group tasks by file-set; qa: generated/embedded files falsify "does not exist" claims), rewrote every touched project memory's bloated `## Current state` down to ≤10 lines (several had drifted to 20-45 lines via one-bullet-per-round appends — see the new global lesson this added), and updated all 7 project memory files' current state to the final archived/merge-pending status.

## Decisions affecting my role
- This project's L1 role memory files (`.orchestrator/memory/{role}.md`) are already written by
  each agent at handoff time, not by memory-curator from scratch — my pass re-audits them for
  staleness/duplication and decides what promotes to L2 global or a playbook; it does not rewrite
  their prose unless something is actually stale or duplicated.

## Gotchas found here
- Handoff files in this project use a `brief-NN-<role>.md` prefix for the TASK PROMPT and a plain
  `NN-<role>.md` for the finished DELIVERABLE (e.g. `brief-02-researcher.md` vs `02-researcher.md`)
  — don't skim the brief file and mistake it for the agent's actual output.
- Konnect's handoff filenames aren't always `NN-<role>.md` — an orchestrator-owned checklist task
  can be `NN-orchestrator-checklist-<task-id>.md` (e.g. `05-orchestrator-checklist-6.1.md`); Glob
  the handoff directory rather than guessing the exact filename from the CT's role list.
- A brief's "the X/Y/Z files are near cap" framing can understate reality: this round all four
  named globals were already 24-42 lines OVER the 60-body-line cap (Grep `^` for total lines,
  `^$` for blanks, subtract 1 for the title — don't trust a Read tool's last line number as the
  count without also checking for wrapped/blank lines it doesn't distinguish). Always measure
  before triaging, never take "near cap" as license to do a light pass.
