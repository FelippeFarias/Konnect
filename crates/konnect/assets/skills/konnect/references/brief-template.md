# Brief Template — Delegating a Phase

A delegated agent starts with no memory and no view of the conversation: the
brief is everything it knows. Every brief carries eight fields, named as orc
names them — Objective, Context, Output Format, Tools Granted, Tools Blocked,
Budget, Files Scope, Success Criteria — with the Konnect guidance below.

## The eight fields

| Field | Holds |
|---|---|
| Objective | One sentence: the phase's outcome for this job. |
| Context | `job_id`, `project_dir` and the phase. The records and handoffs to `read` with `flow_status(project_dir, read)`, by name. Both memory tiers pasted verbatim: the role's `~/.konnect/agents/<role>/MEMORY.md` and the project's `memory/<role>.md`. The user's answers and decisions already taken, quoted verbatim. |
| Output Format | The handoff template (`handoff-template.md`): persisted with `flow_log` as `kind` `handoff`, and returned as the final message. |
| Tools Granted | The toolsets the phase needs, `load_toolset("flow")` included for a job, and the flow calls the agent makes: `flow_status`, `flow_log`, and `flow_advance` for the phase's producer. |
| Tools Blocked | `flow_start` and `flow_gate` always (the session's). `flow_advance` for an agent that never advances (sourcing, library, a reviewer whose report the session merges). Mutating tools in a read-only phase. Library-authoring tools outside the library agent. |
| Budget | `small`, `normal` or `research`. Past it, the agent returns `BLOCKED` with what is left instead of grinding. |
| Files Scope | The design files the agent may mutate (named sheets, the board, project libraries) and the records it must supply in its `flow_advance`. Files Scope never includes `STATE.md`, or anything else under `.konnect/flow/`: only the flow tools write there. |
| Success Criteria | The phase's exit condition from [the orchestration reference](orchestration.md) §2, and the tools expected in `evidence_calls`. |

- **Name records, never retell them.** A brief that paraphrases
  `constraints.md` hands the agent a copy that can be wrong; name the record
  and let the agent read it. State any premise the brief does carry as
  "believed; verify" (the kicad-review skill's
  `references/review-orchestration.md`, Phase 3 prompt rules).
- **Job-less brief** (the single-agent lane): no `job_id` in Context. The
  agent calls no `flow_*` tool and only returns its handoff.

## Example — an architecture brief

```
Objective: Produce the architecture records for the USB-serial converter so
the job can reach gate:architecture.

Context:
- job_id: usb-serial-converter-with-esp32-20260921-140000
- project_dir: C:/boards/usb-serial
- phase: architecture
- Read with flow_status(project_dir, read): constraints.md,
  handoffs/03-sourcing.md, memory/architecture.md
- Global memory (~/.konnect/agents/architecture/MEMORY.md), verbatim:
  <pasted>
- Project memory (memory/architecture.md), verbatim:
  <pasted>
- The user's answers from the question round, verbatim: "<quoted>"

Output Format: the handoff template, persisted with flow_log (kind handoff,
role architecture) and returned as the final message.

Tools Granted: load_toolset("flow"), load_toolset("config"),
load_toolset("library") for search only, load_toolset("integration").

Tools Blocked: flow_start, flow_gate, and every schematic, board and
library-authoring tool.

Budget: normal. Return BLOCKED instead of grinding past it.

Files Scope: no design file. Records to supply: architecture.md,
worst-case.md, pin-plan.md. Never STATE.md.

Success Criteria: flow_advance into gate:architecture accepted, with
architecture.md ending in "Readiness: PASS", a worst-case record for every
decisive value, and every cited tool listed in evidence_calls.
```
