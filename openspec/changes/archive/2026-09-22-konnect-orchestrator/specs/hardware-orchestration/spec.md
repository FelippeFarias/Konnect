## ADDED Requirements

### Requirement: konnect skill orchestrator router
The `konnect` skill SHALL include an orchestrator router section naming, in
at most two reference hops from the router itself, where to read the
orchestration protocol (`references/orchestration.md`), the brief template
(`references/brief-template.md`), and the handoff template
(`references/handoff-template.md`). The router SHALL be additive to the
skill's existing Decision Tree and "Agent Routing and Mutation Ownership"
sections, which SHALL continue to handle bounded edits and single-agent
requests directly, without opening a flow job.

#### Scenario: the router points to the orchestration reference
- **WHEN** a request is not a bounded edit and not a single-agent ask
- **THEN** the `konnect` skill's router names `references/orchestration.md`
  as the next file to read, reachable in one hop from `SKILL.md`

#### Scenario: a bounded edit stays on the direct lane
- **WHEN** a request is a small, reversible, non-architectural change (e.g.
  "change R5 from 10k to 4.7k")
- **THEN** the skill's existing Decision Tree handles it directly, with no
  flow job opened

### Requirement: orchestration reference names the phase/gate sequence and lane table
`references/orchestration.md` SHALL state the canonical phase/gate sequence,
the per-lane phase subsequences (including the two lanes that open no flow
job: bounded edit and single agent), and the phase-to-required-record
mapping.

#### Scenario: the single-agent lane is named
- **WHEN** a request is exactly one bundled agent's stated purpose (e.g.
  "review my layout")
- **THEN** `references/orchestration.md` names this as the "single agent"
  lane: a complete brief to that one agent, the master checks the returned
  handoff, and no flow job is opened

### Requirement: three-round verification cap
`references/orchestration.md` SHALL state that a `FIX` verdict against the
same ledger (`schematic_review` or `prefab_review`) is sent back at most
three times, counted by `flow_status`'s FIX-round count for that phase,
before the orchestrating session escalates to a `BLOCKED` handoff to the
human, rather than sending a fourth automatic retry.

#### Scenario: a fourth FIX round is not sent automatically
- **WHEN** `flow_status` reports three FIX rounds for a review phase and its
  ledger still has an open `FIX_BEFORE_FAB`
- **THEN** `references/orchestration.md` instructs the session to stop and
  escalate to the human instead of dispatching a fourth fix round

### Requirement: a FIX returns to the layer that failed
`references/orchestration.md` SHALL map a handoff's `failing_layer` to the
phase the session rewinds to with `flow_advance`: `requirement` to
`requirements`, `architecture` to `architecture`, and `implementation` to
the phase that produced the faulty artifact.

#### Scenario: an implementation fault goes back to its producer
- **WHEN** a pre-fabrication review returns `FIX` with `failing_layer:
  implementation` against the routing
- **THEN** the reference instructs the session to rewind the job to
  `routing` with a reason, not to `requirements`

### Requirement: every phase runs without bundled agents
`references/orchestration.md` SHALL state that when the bundled agents are
not installed (a Codex installation receives skills only), the orchestrating
session runs each phase itself with that phase's skills and, as the
producer, calls `flow_advance` itself.

#### Scenario: a Codex session runs the architecture phase
- **WHEN** the session runs under a client with no bundled agents
- **THEN** the reference tells it to follow the `kicad-architecture` skill
  and record the phase with `flow_advance` itself

### Requirement: readiness gate before the schematic phase
`references/orchestration.md` SHALL state that the `architecture` phase's
record must end with a `Readiness: PASS` line before the `gate:architecture`
approval may be granted, and that a `Readiness: BLOCKED` line names the
missing or invented value blocking it.

#### Scenario: an invented value blocks the readiness line
- **WHEN** the architecture record cannot source a value used by a block
  from a datasheet, a measurement, or a stated assumption confirmed with the
  user
- **THEN** the record's `Readiness:` line reads `BLOCKED` and names the
  value, per `references/orchestration.md`'s stated rule

### Requirement: evidence cross-check before accepting DONE
`references/orchestration.md` SHALL instruct the orchestrating session to
confirm every tool call a handoff cites as evidence before accepting a
`DONE` verdict — from the evidence check `flow_advance` recorded for that
transition, or, for a handoff with no transition, from `get_recent_calls`
right after the agent returns and the persistent call log for older calls —
and to treat an unconfirmed citation as a `FIX` with `failing_layer:
implementation` rather than accepting it or silently re-running the call
itself.

#### Scenario: an unverifiable claim is not accepted as DONE
- **WHEN** the recorded evidence check lists a cited call as absent
- **THEN** `references/orchestration.md` instructs the session to record the
  handoff as `FIX` with `failing_layer: implementation`, not to accept the
  `DONE` verdict

### Requirement: brief and handoff templates
The `konnect` skill SHALL ship `references/brief-template.md` with fields
Objective, Context, Output Format, Tools Granted, Tools Blocked, Budget,
Files Scope, Success Criteria, and `references/handoff-template.md` with a
verdict of `DONE`, `FIX`, or `BLOCKED` and a `failing_layer` of
`requirement`, `architecture`, or `implementation`, required whenever the
verdict is `FIX`.

#### Scenario: a FIX verdict names its failing layer
- **WHEN** a delegated agent's handoff verdict is `FIX`
- **THEN** the handoff, per `references/handoff-template.md`, names one of
  `requirement`, `architecture`, `implementation` as `failing_layer`

#### Scenario: a brief names what the agent may and may not touch
- **WHEN** the orchestrating session builds a brief for a delegated agent
- **THEN** the brief, per `references/brief-template.md`, states the files
  the agent may write (Files Scope) separately from the tools it is granted
  and blocked
