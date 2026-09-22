# Handoff Template — Returning a Phase

Agents hand back documents, not conversation. Every delegated run ends with
this handoff. In a job, the agent persists it with
`flow_log(project_dir, job_id, kind, message, role)` — `kind` `handoff`,
`message` the whole handoff — which writes a new numbered
`handoffs/<job_id>/<NN>-<role>.md`, and then returns the same text as its
final message. A job-less run (the single-agent lane) only returns it.

The photo agents keep their five-field block (`stage`, `map_id`, `produced`,
`verdict`, `blockers`): they run before the photo lane's job opens.

## Shape

```
---
job_id: <job_id>
phase: <phase token>
role: <role slug>
verdict: DONE | FIX | BLOCKED
failing_layer: requirement | architecture | implementation
---

## Result
## Evidence
## For the next agent
## Deferred findings
## Questions
```

## Header

- **verdict**
  - `DONE` — the phase's exit condition holds, and a producer's
    `flow_advance` was accepted.
  - `FIX` — something already produced is wrong and must be redone.
  - `BLOCKED` — the run cannot finish without something only the session or
    the user can supply: a product answer, a missing capability, more budget.
- **`failing_layer`** is required on `FIX`, and absent otherwise. It names the
  layer the defect lives in — for a reviewer, the layer of what it found, not
  its own:
  - `requirement` — a constraint is wrong or missing; the session rewinds to
    `requirements`.
  - `architecture` — a block, the power budget, the pin plan, a part choice
    or a worst-case record is wrong; the session rewinds to `architecture`.
  - `implementation` — the artifact does not match an approved
    architecture; the session rewinds to the phase that produced it.

## Sections

- **Result** — the records supplied and files changed, and the decisions
  made, one line each. For `BLOCKED`, the drafts, so the re-run starts from
  them.
- **Evidence** — each tool call and its result: the call and the numbers it
  returned (ERC and DRC counts with parity, render paths, search results). A
  producer lists every call cited here in its `evidence_calls`; the session
  confirms them against the call log, and an unconfirmed call turns the
  handoff into a `FIX`.
- **For the next agent** — what the records do not show: decisions and
  their reasons, approaches rejected, gotchas.
- **Deferred findings** — incidental issues outside the phase, each also
  recorded with `flow_defer`; `(none)` when empty.
- **Questions** — `BLOCKED` only: every question at once, each with numbers
  and a recommended option.

## Example — a pre-fabrication review returning `FIX`

```
---
job_id: usb-serial-converter-with-esp32-20260921-140000
phase: prefab_review
role: review
verdict: FIX
failing_layer: implementation
---

## Result
- Ledger: 1 FIX_BEFORE_FAB, 3 DOC_ONLY, 41 blocks checked correct.
- F-1 (FIX_BEFORE_FAB): the VBUS track from J1 to F1 is 0.20 mm for 0.5 A;
  the power netclass asks 0.40 mm. routing.md's width audit missed it.

## Evidence
- get_drc_violations (schematic parity): 0 errors; 2 warnings, both in the
  accepted baseline.
- Width audit rerun on the saved board: 1 track below its netclass (F-1).

## For the next agent
- Re-route F-1 rather than widening in place: the track passes between two
  pads at 0.15 mm clearance.

## Deferred findings
(none)
```
