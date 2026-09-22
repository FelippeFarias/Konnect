---
change: konnect-orchestrator
task: plan the konnect-orchestrator OpenSpec change (proposal, spec deltas, design, tasks)
agent: planner
verdict: DONE
---

## Result

Created `openspec/changes/konnect-orchestrator/` in the MAIN checkout
(`C:\Users\felip\Documents\FFS-Hardware-Eng\Konnect`), scaffolded via
`orc new change konnect-orchestrator`, then hand-written:

- `openspec/changes/konnect-orchestrator/proposal.md`
- `openspec/changes/konnect-orchestrator/design.md` — flow tool contract
  table (D1), `.konnect/flow/` layout + `STATE.md` shape (D2), canonical
  phase/gate sequence + per-lane subsequences (D3), phase → required-record
  table (D4), the readiness gate before the schematic phase (D5), the
  `get_recent_calls` evidence cross-check as guidance-only text (D6), role
  memory locations (D7), the `konnect` skill router + orchestration
  reference + brief/handoff templates + verification cap (D8), the full
  agent roster table for all eleven agents that exist once this change and
  the pending merge both land (D9), routing (D10), a Non-Goals section
  naming exactly the four out-of-scope items, and a pre-mortem.
- `openspec/changes/konnect-orchestrator/specs/flow-toolset/spec.md`,
  `specs/hardware-orchestration/spec.md`, `specs/agent-roster/spec.md` — all
  `## ADDED Requirements` (no existing spec is modified).
- `openspec/changes/konnect-orchestrator/tasks.md` — 32 tasks across 7
  rounds (flow toolset; konnect skill router/reference/templates; 4 new
  companion skills; 6 new agents; 3 existing agents' new flow-record step;
  manifest/install-wiring/asset-guards; final gates).

No playbook in `~/.orc/playbooks/INDEX.md` matched (checked first, as
required) — this is a new-toolset-plus-skill-plus-agent-roster shape none
of the three existing playbooks cover.

## Evidence

- `openspec validate --changes konnect-orchestrator --json` (run from the
  repo root) → `"valid": true`, zero issues, after one fix (see "For the
  next agent").
- `grep -c "^- \[ \]" tasks.md` = 32; `grep -c "^Stack: none" tasks.md` = 32
  (equal, per SC #1 and the project's own grading convention).
- `design.md` contains: the flow tool contract table (`### D1`), the phase →
  required-record table (`### D4`), the roster table covering all six new
  agents plus the existing/merged ones (`### D9`), and a `Non-Goals` list of
  exactly four items (konnect-vcs checkpoints, Codex cross-reviewer, brain
  integration, firmware-contract hand-off) under `## Goals / Non-Goals`.
- `tasks.md` covers, by round: flow toolset tools/hash-bound
  gates/tests/registry/doc-counts (round 1, tasks 1.1-1.9); the `konnect`
  skill router + orchestration reference + templates (round 2, tasks
  2.1-2.4); all six new agents plus their companion skills and manifest/
  asset-test wiring (rounds 3, 4, 6); the merged photo agents' routing
  (task 2.4, and design D9/D10 name them explicitly).

## For the next agent (architect pass, then implementation)

- **Fixed one `openspec validate` failure during planning**: a requirement
  whose body opened with backtick-quoted agent names before the word SHALL
  (`` `kicad-schematic-build-agent`, ... SHALL each gain ``) failed the
  validator's "must contain SHALL or MUST" check even though the word was
  present in the text. Rewording to open with "The system SHALL …" (the
  same subject every other requirement in this change uses) fixed it. If a
  future spec edit reintroduces a requirement whose first sentence doesn't
  open with a plain-subject SHALL/MUST clause, expect the same false
  negative from the validator, not a real content problem.
- **Key design decision an implementer must not re-litigate**: agents never
  get a generic file-write tool for `.konnect/flow/` records. A phase
  record's Markdown body is passed as a string argument to `flow_advance`
  (design D1's "why `flow_advance` writes records instead of the caller
  pre-writing a file"), and the *agent that owns that phase* calls
  `flow_advance` itself — `flow_start`/`flow_gate` stay with the
  orchestrating session only (a delegated Task sub-agent cannot itself
  collect a human's `user_words`). This resolves an ambiguity the source
  doc leaves implicit; do not have the master session call `flow_advance`
  on an agent's behalf, and do not add a generic write tool later without
  revisiting this decision.
- **Merged-base risk, not yet resolved by this plan**: the
  `board-dossier-reconstruction` worktree's copy of `konnect/SKILL.md`
  predates `main`'s current scripted-board-fallback content, so the pending
  merge must reconcile the two before task 2.1 can proceed. Task 2.1's
  first acceptance criterion (`grep -c "The One Rule\|kicad-photo-to-board"`
  ≥ 2) is the mechanical check that the merge actually kept both; if it
  fails, the merge dropped one side and needs a manual reconciliation pass
  before this change's own edits land.
- **Rejected alternative worth remembering**: per-lane hardcoded `phases`
  arrays in Rust (one `match` arm per lane). Rejected in design D3 because
  "board revision" is explicitly scoped by the source doc as a genuinely
  per-job subset ("só as fases afetadas"), so the validator is a generic
  subsequence check against one canonical order, not a lane-keyed table —
  keep it that way; hardcoding it back in would make adding a lane a
  `flow.rs` change again.
- **Numbers I deliberately did not hardcode**: doc-count bump tasks (1.9)
  reference "whatever `board-dossier-reconstruction`'s merge already left in
  place" rather than literal registered/toolset totals, because the merge
  had not landed at planning time and I could not read its exact final
  counts. The implementer must run `cargo test -p konnect --test
  doc_tool_counts` to get the real numbers at that time, not copy a number
  from this handoff or from the archived change's own D10 (232/239 — that
  was `board-dossier-reconstruction`'s own delta, already superseded once
  it merges and `flow` adds six more).

## Deferred findings

(none — this was a planning-only task with no code execution; nothing was
found outside the requested scope)
