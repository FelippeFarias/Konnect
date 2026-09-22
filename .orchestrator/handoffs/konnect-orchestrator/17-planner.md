---
change: konnect-orchestrator
task: fix round 2 planning — close reviewer 16's SERIOUS + minors via DECISIONS I, J, K, L
agent: planner
verdict: DONE
---

## Result

Revised `openspec/changes/konnect-orchestrator/design.md` and appended
`## 9. Fix round 2 (reviewer 16)` to `tasks.md` (5 new unchecked tasks,
9.1-9.5) in the MAIN checkout. No spec-delta edits: grepped all three
`specs/*/spec.md` files for the old wording each of I/J/K touches
("warning"/"READY"/"INCOMPLETE"/"recomputed"/"refusal writes nothing") —
none state it as a requirement or scenario (flow-toolset's `spec.md:40`
already says validity is recomputed "for the gate the job currently stands
at", so K's fix is code/asset text only, not a spec fix). DECISION L needs
no task per the CT; recorded as an accepted trade-off.

**design.md edits:**
- D9 (`kicad-manufacture-agent.md` / DECISION F block): corrected the
  bullets in place — restored "every **artifact** check" (was "every
  check", the actual bug reviewer 16 found), added a DRC/preflight
  adjudication bullet (DRC errors always block; DRC warnings and preflight
  issues are adjudicated, not warning-gated), and folded DECISION J's
  READY-ownership clause into the existing "READY is still what the skill
  requires..." bullet. Added a "Fix round 2 (DECISION I)" paragraph after
  it explaining the correction, citing the `ecc83-pp` repro (0 errors, 17
  warnings) and naming that the agent's Verdict bullet + the guard at
  `asset_references.rs:2068` change together (a literal edit, not
  additive — unlike every round-1 asset change).
- D2 (`STATE.md` format section): added two "Fix round 2" paragraphs —
  DECISION K (text-only: `flow_status`'s description, the `STATE.md` body
  render sentence, and `flow_gate`/`flow_advance`/`flow_defer`'s
  descriptions never named the `warning` field or matched §7's
  current-gate-only `valid` scoping) and DECISION L (accepted trade-off,
  no task, citing reviewer 16's 600-probe evidence).
- D11 (`Cost, measured`): reworded the "costs a re-approval" sentence to
  "a re-run of the producing agent plus a re-approval" per DECISION K,
  since DECISION B already requires rewinding to the producer.

**tasks.md `## 9`** (all `Stack: none`, ≤3 acceptance clauses each):
9.1 `flow.rs` description/render text (DECISION K, no behaviour change);
9.2 `orchestration.md` §4 (DECISIONS J+K: READY-ownership bullet after the
purchase-checks paragraph, reworded validity sentence); 9.3
`kicad-manufacture-agent.md` (DECISIONS I+J: full Verdict-bullet rewrite,
Ending-the-run pointer sentence); 9.4 the guard
(`crates/konnect/tests/asset_references.rs:2068`,
`manufacture_agent_verdicts_match_the_skill` — literal marker replacement,
new stale-phrase check, a new test
`manufacture_verdict_permits_exit_with_adjudicated_drc_warnings` asserting
the ecc83-pp shape as prose: adjudicated DRC/preflight warnings don't force
`INCOMPLETE`, but the export `warnings` array clause still does); 9.5
full regression. Section intro states 9.1-9.4 touch disjoint files and can
run in parallel, **except** 9.3 and 9.4 must land together (the guard
fails on 9.3 alone, since it's a literal-text dependency, not additive).

## Evidence

- `openspec validate --changes konnect-orchestrator --json` → `"valid": true"`.
- `openspec validate --changes konnect-orchestrator --strict --json` → `"valid": true"`.
- `grep -c "^- \[.\]" tasks.md` = `grep -c "^Stack: none"` = `grep -c "^Acceptance"` = 48.
- `grep -c "^- \[x\]" tasks.md` = 43 (all earlier tasks still checked); `grep -c "^- \[ \]" tasks.md` = 5 (9.1-9.5).
- `grep -nE "[0-9] toolset|[0-9]{3} tools" design.md tasks.md` → no matches (D12's digit-before-noun rule respected).
- Read the worktree at `1c05a85` (read-only) to pin every file:line the tasks
  cite: `kicad-manufacture/SKILL.md:15,80-81,97,186,267` (artifact
  acceptance gate step 1 vs. DRC/preflight adjudication — confirmed the
  skill's own wording, quoted verbatim in design.md and the tasks);
  `kicad-manufacture-agent.md`'s Verdict/Ending-the-run text; `flow.rs:664`
  (`Validity is recomputed by...`), `:3105` (`flow_status` description),
  `:3167-3340` (`flow_advance`/`flow_gate`/`flow_defer` descriptions, none
  currently mention `warning`), `:3174` (`a refusal writes nothing`);
  `orchestration.md` §4 lines 116, 132-143 (found the exact contradicting
  sentence at "flow_status reports every approval's validity against the
  current files", vs. §7's already-correct DECISION-D text at 219-224);
  `crates/konnect/tests/asset_references.rs:2046-2104`
  (`manufacture_agent_verdicts_match_the_skill`, confirmed line 2068's
  exact pinned phrase) and `:1965-2008`
  (`orchestration_reference_states_the_fix_round_one_rules`, confirmed my
  §4 edit doesn't touch any of its pinned §1/§2/§7 markers).
- Confirmed `flat()` in `asset_references.rs` only collapses whitespace
  (doesn't strip `**`/backticks), so every literal marker I specified in
  tasks.md must and does match the exact Markdown (bold markers included)
  I wrote into the agent-text instructions.

## For the next agent

- 9.1-9.4 are almost entirely prose/text edits — no `flow.rs` control-flow
  changes. Do not add new refusal/validation logic; the acceptance criteria
  are grep-based on top of the existing regression suites.
- 9.3's new Verdict-bullet text must go in verbatim (it's quoted in full in
  the task) so 9.4's guard markers match; don't paraphrase it.
- 9.4's new test name (`manufacture_verdict_permits_exit_with_adjudicated_
  drc_warnings`) and its two assertions (adjudicated DRC/preflight warning
  is not an open item; the artifact/export-warning clause has no
  adjudication escape) are the SC #3 requirement — this is a prose-level
  check standing in for reviewer 16's live `kicad-cli` repro, not a live
  KiCad run.
- I did not touch `crates/konnect-core/tests/flow_fix_round_one_e2e.rs` or
  any behavioural test — 9.1's flow.rs edit is documentation strings only,
  confirmed no existing test pins the old literal wording (grepped for
  "Validity is recomputed", "recomputed `valid`", "a refusal writes
  nothing" across `crates/konnect-core/tests/` and `crates/konnect/tests/`).
- Rejected an alternative for 9.1-9.4 grouping: a single combined task per
  DECISION (I, J, K each as one task spanning multiple files) — rejected
  because the CT explicitly asked for grouping "by file set", and mixing
  `flow.rs` with asset `.md` files in one task would force sequential
  execution where none is needed.

## Deferred findings

(none — every item in reviewer 16's verdict is either closed by a task
(I, J, K) or explicitly accepted with no task (L); QA 15's older deferred
findings 1, 3, 5 are the same items reviewer 16 turned into I/J/K.)
