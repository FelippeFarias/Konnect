---
name: kicad-architecture
description: |
  Method for the two phases before any part is placed: the constraint record
  (requirements) and the architecture — block diagram, power tree and budget,
  interfaces, pin plan, worst-case records and parts that can be bought — ending
  in a readiness line. Triggers on: "start a new board", "what do we need to know
  before designing", "requirements for this board", "design the architecture",
  "block diagram", "power budget", "pin plan", "architecture brief".
argument-hint: "[objective, or the job_id of a flow job]"
---

# KiCAD Architecture — Constraint and Architecture Records

A schematic can pass ERC with a value nobody sourced and a requirement nobody
asked for. This skill turns a request into four records before a single part is
placed: `constraints.md` says what must hold; `architecture.md`, `worst-case.md`
and `pin-plan.md` say how the board meets it. It creates no `.kicad_sch`, no
board and no library part.

It expands the kicad-schematic skill's "Architecture checkpoint" (step 4 of
"Before Placing Parts"). That skill keeps the method for values
(`references/design-calculations.md`, `references/interface-design.md`); this
skill owns the records, their sections, and the readiness rule.

---

## Records and how they are written

| Record | Phase | Leaves the phase with |
|---|---|---|
| `constraints.md` | `requirements` | `flow_advance` into `architecture` |
| `architecture.md`, `worst-case.md`, `pin-plan.md` | `architecture` | `flow_advance` into `gate:architecture` |

- **In a flow job** (the brief names a `job_id`), a record reaches disk only
  through `flow_advance`, which writes each `{filename, content}` it is given.
  Supply every record of the phase in that one call: a record already on disk
  from an earlier visit or an earlier job never satisfies an exit.
- **Without a `job_id`**, call no `flow_*` tool. Return the records in the final
  message.
- Read [`references/constraint-record-schema.md`](references/constraint-record-schema.md)
  before drafting or checking `constraints.md`: every field, its allowed
  sources, and an example.
- Read [`references/architecture-record-schema.md`](references/architecture-record-schema.md)
  before drafting `architecture.md`, `worst-case.md` or `pin-plan.md`, and
  whenever you fill or check the parts list.

## Toolset loading

```
load_toolset('config')        # get_effective_config: derating classes, AVL, fabrication constraints
load_toolset('library')       # search_symbols, search_footprints: read-only lookups
load_toolset('integration')   # search_jlcpcb_parts, suggest_jlcpcb_alternatives, get_datasheet_url
load_toolset('flow')          # flow_status, flow_log, flow_advance: only when the brief names a job_id
```

The `library` toolset also exposes tools that create and edit parts. Calling
them here is out of scope: this phase names parts, the library agent makes them.

In a job, read your inputs with `flow_status(project_dir, read)`, listing the
records the brief names (`constraints.md`, `memory/architecture.md`, …). Never
rebuild a requirement from the conversation.

---

## Requirements — the constraint record

1. **Start from what exists.** In a revision job, the previous `constraints.md`
   is the baseline: read it and change only what the request changes.
2. **Capture every field once**: environment, power, currents, interfaces,
   connectors and sides, size, enclosure, fab/service, quantity, cost. Each
   value names its source — `user`, `datasheet`, `config` or `decision` — and
   the evidence for it.
3. **Decide the obvious and log it.** A value with one clearly right answer
   given the others is a decision, not a question. Record it with
   `flow_log(project_dir, job_id, kind, message, why, rollback)` — `kind` is
   `decision`, and `why` and `rollback` are both non-empty — and mark its
   source `decision`. Ask only about product intent, cost and appearance.
4. **One question round.** When a value that decides the design is missing,
   collect every such question at once, each with numbers and a recommended
   option, and return `BLOCKED` with all of them in the handoff's Questions
   section. The session asks the user once and re-briefs with the answers
   quoted. Never ask one question per turn, and never fill a gap with a guess.
5. **Exit**: no value that decides the design is missing. In a job, call
   `flow_advance(project_dir, job_id, to_phase, records)` with `to_phase`
   `architecture` and `constraints.md` in `records`.

### `constraints.md` — required sections

```
# Constraints — <objective>
## Scope
## Constraints
## Decisions
## Open questions
```

- **Scope**: the objective in one sentence, the lane, and the baseline record
  (the previous `constraints.md`, or "none").
- **Constraints**: one table row per field — field, value, source, evidence.
- **Decisions**: one line per obvious call, matching its `flow_log` entry.
- **Open questions**: "none", or only questions whose answer changes no
  architecture value, each with who answers it.

---

## Architecture — blocks, power, pins, worst cases, parts

Input: `constraints.md`. Each step lands in a record section.

1. **Blocks.** Every functional block, its key part, and the signals and rails
   between blocks.
2. **Power tree and budget.** Each rail from source to load: converter or
   regulator, voltage and tolerance, current per consumer at typical and worst
   case, the total, the dissipation of every regulator and series element at
   the worst corner, and the margin against the derating class from
   `get_effective_config`. Size them with kicad-schematic's
   `references/design-calculations.md` §3–§4.
3. **Interfaces.** Every external connector: function, pinout, board side,
   mating part or cable, protection — designed with kicad-schematic's
   `references/interface-design.md`.
4. **Pin plan.** Every pin of every programmable part (MCU, module, logic
   device): function, net, constraint (strapping, boot state, ADC or touch
   capable, voltage tolerance, reserved), source.
5. **Worst-case records.** One per decisive value, in the fields of
   kicad-schematic's `references/design-calculations.md` §1: both corners, the
   criterion and margin, and what would invalidate the decision.
6. **Parts that can be bought.** The parts list: exact suffix, stock with its
   date and source, lot ceiling, a validated datasheet, AVL and derating
   status. Confirm a stocked part meets a requirement before writing the
   requirement down. For a large BOM the session runs `kicad-sourcing-agent`
   on disjoint part groups; its handoffs hold the rows to copy.
7. **The readiness line**, last (below).

### Required sections

`architecture.md`:

```
# Architecture — <objective>
## Blocks
## Power tree and budget
## Interfaces
## Parts list
## Open questions
Readiness: PASS
```

`worst-case.md`:

```
# Worst-case records — <objective>
## WC-<n> — <the value decided>
```

`pin-plan.md`:

```
# Pin plan — <objective>
## <reference> — <part>
| Pin | Function | Net | Constraint | Source |
```

### The readiness line

The last non-empty line of `architecture.md`, trimmed, is exactly one of:

```
Readiness: PASS
Readiness: BLOCKED — <the missing or invented value, and what would supply it>
```

- `Readiness: PASS` only when every value a block depends on traces to a
  datasheet, a measurement, a configuration key, or an assumption the user
  confirmed, and every parts-list row is confirmed (next bullet).
- A parts-list row is confirmed only when the orchestrating session's
  evidence entry for it exists in the job log: the session checked the live
  stock on the fabricator's current part page and opened the manufacturer's
  datasheet for that exact part, and recorded both with
  `flow_log(project_dir, job_id, kind, message)`, `kind` `evidence` (the
  konnect skill's `references/orchestration.md` §9). Read the log with
  `flow_status(project_dir, read)` naming `log`. A bundled agent can open
  neither source, so a catalogue-only stock or a Datasheet cell reading
  `located, not validated` keeps the line `Readiness: BLOCKED — …`; the agent
  then returns BLOCKED naming the rows the session must confirm, never
  advancing.
- Otherwise `Readiness: BLOCKED`, followed by a reason naming the value:
  `Readiness: BLOCKED — maximum ambient inside the enclosure: not in constraints.md`.
  A BLOCKED line without a reason is malformed.
- Only blank lines may follow it: no footer, no signature.
- The tool enforces it. `flow_advance` refuses to leave `architecture` unless
  the line reads `Readiness: PASS`, and `flow_gate` re-checks it before the
  architecture approval.
- **BLOCKED means return BLOCKED, not advance.** Put the value in the
  handoff's Questions, keep the drafts in its Result, and persist it. The
  session collects the value — usually by rewinding the job to `requirements`.

In a job, the phase ends with
`flow_advance(project_dir, job_id, to_phase, records, evidence_calls)`:
`to_phase` is `gate:architecture`, `records` holds `architecture.md`,
`worst-case.md` and `pin-plan.md`, and `evidence_calls` lists every tool you
cite as evidence (`get_effective_config`, `search_jlcpcb_parts`, …).

---

## Rules

- Never place a part, draw a wire, or create a library item in these phases.
- Every number has a source: a datasheet page or figure, a measurement, a
  configuration key, or a logged decision. A number without one makes the
  readiness line BLOCKED.
- A part substitution resets every number derived from the old part
  (kicad-schematic's `references/design-calculations.md` §1): redo the
  affected worst-case records and the readiness line.
- Log your own mistake the moment you see it, with
  `flow_log(project_dir, job_id, kind, message, role, scope)` and `kind`
  `lesson`; the kicad-curator skill holds the triage for `scope`.
- The handoff follows the konnect skill's `references/handoff-template.md`.
