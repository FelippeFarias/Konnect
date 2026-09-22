# Orchestration — Running a Flow Job

The orchestrator is the main session running the konnect skill, not a
bundled agent. It picks the lane, writes each brief, checks each handoff
against evidence, records gates and decisions, and sends a FIX back to the
layer that failed. It does not do an agent's phase for it. A job's state
lives in `<project>/.konnect/flow/`, written only by the `flow` toolset, and a
human approval binds to the exact design and the exact records the human was
shown.

```
load_toolset("flow")   # flow_status, flow_start, flow_advance, flow_gate, flow_log, flow_defer
```

Every brief follows [the brief template](brief-template.md); every handoff
follows [the handoff template](handoff-template.md).

## 1. Lanes and the canonical sequence

The canonical sequence; a job runs a strictly increasing part of it:

```
requirements → architecture → gate:architecture → schematic → schematic_review →
placement → gate:placement → routing → prefab_review → manufacturing →
gate:purchase → learn
```

| Lane | When | Job | How |
|---|---|---|---|
| Bounded edit | A few objects, no architecture change, reversible ("change R5 to 4.7k") | none | The konnect skill's Decision Tree, the applicable skill, verify |
| Single agent | The request is exactly one bundled agent's stated purpose ("review my layout", "prepare the JLCPCB files") | none | One complete brief to that agent; check its handoff (§6) |
| New board | "Create a board that …" | `new_board` | `phases` omitted: the full sequence |
| Board revision (ECO) | A change to an existing project | `board_revision` | Only the affected phases, e.g. `[architecture, gate:architecture, schematic, schematic_review, prefab_review, manufacturing, gate:purchase]` |
| Review only | An audit of a finished project | `review_only` | `[prefab_review]` |
| Fab only | A package for a finished project | `fab_only` | `[manufacturing, gate:purchase]` |
| Photo → KiCad | Reverse-engineering a physical board | `photo_to_kicad` | `[schematic, schematic_review, placement, gate:placement, routing, prefab_review, manufacturing, gate:purchase, learn]`, opened only after `pcb-photo-intake-agent` and `pcb-design-reconstruction-agent` each have their human approval (the kicad-photo-to-board skill's stage table) |

- Open the job with `flow_start(project_dir, objective, lane, phases, mode)`.
  `mode` is `guided` unless the user asked for autonomous mode for this job
  (§4). `phases` may be omitted only for `new_board`.
- A job's `phases` never drops a human gate: `architecture` brings
  `gate:architecture`, `placement` brings `gate:placement`, `manufacturing`
  brings `gate:purchase`. A gate is never first.
  A phase brings its gate, and a gate's phase must be present too:
  `gate:architecture` requires `architecture`, `gate:placement` requires
  `placement`, `gate:purchase` requires `manufacturing` — a lane cannot show
  an approval bound to a record from a closed or earlier job. `flow_start`
  refuses a `phases` list that breaks either rule.
- Record the lane and why at once, with
  `flow_log(project_dir, job_id, kind, message, why, rollback)` and `kind`
  `decision`.
- One active job per project. A request that does not belong to it waits,
  or the user decides to abandon the job (§5).
- Only read-only work runs in parallel (reviewers, sourcing). Agents that
  write the project run one at a time, and agents never invoke agents.

## 2. Phase playbook

| Phase | Producer | Its brief names to `read` | Supplies in its `flow_advance` | Exit condition |
|---|---|---|---|---|
| `requirements` | `kicad-requirements-agent` | The previous `constraints.md` (a revision's baseline), `memory/requirements.md` | `constraints.md` | No value that decides the design is missing; the missing ones were asked in one round (§8) |
| `architecture` | `kicad-architecture-agent`, with `kicad-sourcing-agent` runs (§9) | `constraints.md`, the sourcing handoffs, `memory/architecture.md` | `architecture.md`, `worst-case.md`, `pin-plan.md` | `architecture.md` ends with `Readiness: PASS` |
| `gate:architecture` | the session | — | an approval (§4) | The user approved this design and package |
| `schematic` | `kicad-schematic-build-agent`, after any `kicad-library-agent` run (§10) | `architecture.md`, `pin-plan.md`, `worst-case.md`, the library handoffs | `schematic-evidence.md` | Final ERC, shorted-net check, rendered inspection, cross-sheet references, the layout handoff |
| `schematic_review` | `kicad-design-review-agent`, or the session after a multi-reviewer merge | `schematic-evidence.md`, `architecture.md`, `worst-case.md`, `pin-plan.md` | `ledger-schematic.md` | A verdict and action per finding; no open `FIX_BEFORE_FAB` |
| `placement` | `kicad-pcb-layout-agent` | `constraints.md`, `schematic-evidence.md` | `placement.md` | The placement gate's result and per-layer images; the agent stops there |
| `gate:placement` | the session | — | an approval (§4) | The user approved the images |
| `routing` | `kicad-pcb-layout-agent` | `constraints.md`, `placement.md` | `routing.md` | DRC with schematic parity, widths audited against current |
| `prefab_review` | as `schematic_review` | `routing.md`, `placement.md`, `ledger-schematic.md`, `constraints.md` | `ledger-prefab.md` | Verdicts and a readiness level; no open `FIX_BEFORE_FAB` |
| `manufacturing` | `kicad-manufacture-agent` | `constraints.md`, `architecture.md` (its parts list), `ledger-prefab.md` | `manufacturing.md` | Package summary, BOM integrity, indicative cost, release notes; an `INCOMPLETE` package is not an exit |
| `gate:purchase` | the session | — | an approval (§4) | The user authorized the order in their own words |
| `learn` | `kicad-curator-agent` | `log`, `lessons-candidates.md`, the handoffs, `memory/<role>.md` | none; the job ends with `flow_advance` to `closed` | Lessons recorded, promote list returned (§11) |

- "Supplies" means in the same `flow_advance` call. A record already on disk
  was written by an earlier visit or an earlier job and never satisfies an
  exit.
- The tool checks that the records are present and, for `architecture.md`,
  the readiness line. Everything else in the exit column is the session's to
  check before accepting DONE: read the record with
  `flow_status(project_dir, read)` after the transition.
- In a lane whose last phase is not `learn`, the forward transition out of
  the last phase is `closed`, and it ends the job.

## 3. Who calls which tool

The party that produced an output records it. The session never calls
`flow_advance` on an agent's behalf from its handoff text: that is a
paraphrase between the evidence and the state that outlives it.

- `flow_status` — anyone: the session at start and on resume, every agent to
  read its input records.
- `flow_start`, `flow_gate` — the session only. A gate needs the user's own
  words, and a sub-agent cannot talk to the user.
- `flow_advance` forward out of a work phase — the agent that produced the
  phase's records, as its last action. The session is the caller in exactly
  three cases: it merged a multi-reviewer ledger itself, it ran the phase
  itself (§12), or it is leaving a gate.
- `flow_advance` rewind or abandon — the session: routing a FIX is the
  orchestrator's decision.
- `flow_log` — whoever generates the entry, when it happens. Every delegated
  agent persists its own handoff with `kind` `handoff` before returning it.
- `flow_defer` — whoever finds the item:
  `flow_defer(project_dir, job_id, kind, description, owner)` with `kind`
  `finding`, `queue_item` or `pending_approval`. Incidental findings are
  deferred, never fixed out of scope.

## 4. Gates

At `gate:<name>` the session shows the human the package and asks for the
decision in one message:

| Gate | Show |
|---|---|
| architecture | `constraints.md`, `architecture.md` (blocks, power budget, parts with stock, open questions, the readiness line), `worst-case.md`, `pin-plan.md` |
| placement | `placement.md` with the rendered per-layer images, and what `schematic-evidence.md` and `ledger-schematic.md` concluded |
| purchase | `manufacturing.md`: indicative cost, BOM with stock and lot ceilings, the order settings; the readiness level from `ledger-prefab.md`; its `## Checks at the purchase gate` section — the session shows it to the user and runs or asks for each listed check before `flow_gate` for `purchase` |

- Approve or reject with
  `flow_gate(project_dir, job_id, gate_name, decision, summary, user_words)`.
  `summary` says what was shown. `user_words` is the user's message quoted
  verbatim — never paraphrased, never words the user did not send.
- The architecture approval is refused unless `architecture.md` ends with
  `Readiness: PASS`. A `Readiness: BLOCKED` line names the missing or
  invented value: collect it, usually by rewinding to `requirements`.
- An approval is also refused when the design or the package changed after
  the job entered the gate — the human did not see that state. Rewind to the
  producing phase and come back through it.
- After approving, the session leaves the gate itself:
  `flow_advance(project_dir, job_id, to_phase)` to the next phase. It is
  refused if anything changed since the approval; show the new state and ask
  again.
- At `gate:purchase`, `manufacturing.md`'s `## Checks at the purchase gate`
  lists what `kicad-manufacture-agent` could not run with its tools: the
  Gerber and drill viewer check, the fabricator's order preview while the
  placement orientation reads `PREVIEW_REQUIRED`, and the live stock and price
  re-check. The session shows that section to the user, runs each check it
  can (it fetches pages and runs a terminal) and asks the user for the rest,
  and records each result with `flow_log(project_dir, job_id, kind, message)`,
  `kind` `evidence`. Only then does it ask for the `purchase` decision with
  `flow_gate`. A check that failed is a FIX (§5), not an approval.
- A rejection records the decision; route its reason as a FIX (§5).
- `flow_status` reports every approval's validity against the current files
  in `gate_approvals`.
- **Autonomous mode** (chosen at `flow_start`, per job, only on the user's
  request): the session may approve `architecture` and `placement` with empty
  `user_words`, recorded as `approved_by: session`, with its reasoning in
  `summary`. `purchase` always needs the user's words. Readiness, package and
  design rules are unchanged, and product intent, cost and appearance still
  go to the user.

## 5. The FIX loop

A handoff with verdict `FIX` names its `failing_layer`. Rewind to that layer,
never further than needed:

| `failing_layer` | Rewind to |
|---|---|
| `requirement` | `requirements` |
| `architecture` | `architecture` |
| `implementation` | The phase that produced the faulty artifact — `routing` for a routing defect found at `prefab_review`, `schematic` for a schematic defect |

- Rewind with `flow_advance(project_dir, job_id, to_phase, reason)` and no
  records. A rewind clears every gate approval at or after its target, so the
  way back re-asks each gate.
- Re-brief the producer with the findings to fix (the review handoff's
  ledger) and send the job forward through the review again.
- A target outside the job's `phases` (a `review_only` job that finds a
  routing defect) cannot be rewound to: leave the job with its ledger, and
  open a `board_revision` job with the affected phases.
- **Three rounds at most.** `flow_status`'s `fix_rounds` counts the rewinds
  out of `schematic_review` and `prefab_review`. When a review phase's count
  reaches 3 and its latest ledger still has an open `FIX_BEFORE_FAB`, stop:
  do not dispatch a fourth round. Report `BLOCKED` to the user with the
  ledger, what each round changed, and the options. The tool counts; the
  session decides.
- Abandoning a job is `flow_advance` to `closed` with a `reason`, only on the
  user's decision.
- A multi-reviewer review follows the kicad-review skill's
  `references/review-orchestration.md`; the session merges the ledger and, as
  its producer, records it with `flow_advance`. Use it for a board going to
  production (the `constraints.md` quantity row); a prototype gets
  `kicad-design-review-agent` alone.

## 6. Evidence cross-check before accepting DONE

A producer lists the tools it cites as evidence in `evidence_calls`.
`flow_advance` compares them with the server's recent-call ring during the
call and stores `evidence_check` (`confirmed`, `not_ok`, `absent`) in the
history entry. It reports; it never refuses.

- Before accepting a `DONE` that came with a transition, read
  `flow_status(project_dir)` → `last_transition.evidence_check`. Any cited
  call in `absent` or `not_ok` makes the handoff a `FIX` with
  `failing_layer: implementation`: send the agent back to re-run and
  re-report. Never re-run the call yourself in the agent's name.
- A call the handoff cites but `evidence_calls` left out escaped that check:
  confirm it the way below.
- A handoff with no transition (FIX, BLOCKED, sourcing, library, the
  single-agent lane): call `get_recent_calls(limit: 0)` right after the agent
  returns. For calls older than the ring's 100, read the lines of
  `calls.jsonl` newer than the dispatch time (Windows
  `%APPDATA%\konnect\logs\`, macOS
  `~/Library/Application Support/konnect/logs/`, Linux `~/.konnect/logs/`).
- The ring holds tool names and statuses, not arguments or callers: it
  catches "never ran", not "ran on another project" or "another agent ran
  it". The records' content is still the session's to read.

## 7. Resume

A new session on a project with a job: `load_toolset("flow")`, then
`flow_status(project_dir)`, and continue without asking the user anything the
state already answers.

- `job` null — no job; the request picks a lane.
- `state_error` — `STATE.md` was edited by hand and no longer parses. Show
  the error to the user; never guess and never rewrite it.
- `lock_files` — KiCad has the project open: read-only work until it is
  closed (the operating notes, §1).
- `gate_approvals` with `valid: false` — the design or the package changed
  since that approval.
  `valid` matters only while `phase == "gate:<name>"` (the job's current gate,
  reported as `status: current`); an approval of a gate already left reports
  `status: passed` with the hashes it was approved at and no `valid` field —
  not a reason to re-ask or rewind.
- `next_step` — the phase, the records it must supply, and the phase after it.
- `handoffs` — read the newest with `flow_status(project_dir, read)`; then
  `pending_approvals`, `deferred_findings` and `queue`.
- A successful `flow_gate`, `flow_advance` or `flow_defer` response always
  carries `warning`: `null` when every derived side file landed. A string
  means the state change committed in `STATE.md` but a side file (the gate
  file or the job's log entry) was not written: never repeat the call, which
  would apply the change again; repair only what the warning names, and read
  `flow_status(project_dir)` for the committed state.

## 8. Requirements: one question round

`kicad-requirements-agent` returns `BLOCKED` with every product question at
once, each with numbers and a recommended option. The session asks the user
once, in one message, and re-briefs the agent with the answers quoted
verbatim; the agent then advances into `architecture` with `constraints.md`.
A second round only happens when an answer opened a new deciding question.

## 9. Sourcing inside `architecture`

`kicad-sourcing-agent` is read-only and never calls `flow_advance`; it
persists its handoff with `flow_log` and returns parts-list rows in the shape
of the kicad-architecture skill's `references/architecture-record-schema.md`.
For a large BOM, run several in parallel on disjoint part groups (by block)
once a first architecture draft names the candidate parts. The next
architecture brief names their handoffs (`handoffs/<NN>-sourcing.md`) in its
Context; the architecture agent copies the rows and closes the readiness line.

**The session confirms live stock and datasheets.** The bundled agents reach
the local catalogue and a datasheet URL, never the live page or the document:
a sourcing row arrives with catalogue-only stock and a datasheet marked
`located, not validated`. The orchestrating session — which can fetch pages
and run a terminal, unlike the bundled agents — confirms each parts-list row
before the architecture gate, inside the `architecture` phase: the live stock
on the fabricator's current part page (quantity, retrieval date, the page),
and the manufacturer's datasheet for that exact part and suffix, opened as a
real document. It records every confirmation with
`flow_log(project_dir, job_id, kind, message)`, `kind` `evidence`, one entry
per row naming the part number, the stock figure and date, and the datasheet
revision. The next architecture brief names those entries in its Context
(the job log, read with `flow_status(project_dir, read)` naming `log`). A row
without the session's evidence entry is not confirmed and keeps the readiness
line BLOCKED (the kicad-architecture skill's
`references/architecture-record-schema.md`, "Parts list").

## 10. Library inside `schematic`

A parts-list row reading `needs library` gets a `kicad-library-agent` run
inside the `schematic` phase, before the build that places the part. It makes
the symbol and footprint to the kicad-library skill's physical pin-map
acceptance contract, never edits a schematic sheet or the board, never calls
`flow_advance`, and names the resulting IDs in its handoff. The build brief
names that handoff; the build starts after the library run returns.

## 11. Learn and memory promotion

- At `learn`, brief `kicad-curator-agent` with the global role memory pasted.
  It reads the log, handoffs, candidates and project memory, records lessons
  with `flow_log`, returns the promote list, and closes the job.
- Show the user the promote list. Promote each approved `role` candidate into
  `~/.konnect/agents/<role>/MEMORY.md` with your own editor, one bullet per
  lesson, keeping the file at or under 60 lines. The triage, the entry format
  and the cap are the kicad-curator skill's.
- Every brief's Context pastes both memory tiers verbatim: the role's global
  `MEMORY.md` and the project's `memory/<role>.md`.

## 12. Codex, or any client without bundled agents

A Codex installation receives the skills and no bundled agent. The session
then runs each phase itself with that phase's skills — kicad-architecture
for `requirements` and `architecture`, kicad-schematic (with kicad-library)
for `schematic`, kicad-pcb for `placement` and `routing`, kicad-review for the
reviews, kicad-manufacture for `manufacturing`, kicad-curator for `learn` —
and, as the producer, calls `flow_advance` itself with the phase's records
and its `evidence_calls`. The brief becomes the session's own checklist, the
handoff is still persisted with `flow_log`, the evidence check applies to the
session's own claims, and the gates are unchanged.
