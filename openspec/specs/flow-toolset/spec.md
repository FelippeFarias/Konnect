# flow-toolset Specification

## Purpose
TBD - created by archiving change konnect-orchestrator. Update Purpose after archive.

## Requirements

### Requirement: flow toolset registration
The system SHALL provide a `flow` MCP toolset registered in `ALL_TOOLSETS`
with exactly six tools, in this order: `flow_status`, `flow_start`,
`flow_advance`, `flow_gate`, `flow_log`, `flow_defer`. The toolset's
`tool_count` SHALL match the number of tools `tools_for("flow")` returns,
and every tool SHALL be callable through its published input schema.

#### Scenario: the flow toolset is discoverable
- **WHEN** `list_toolboxes` is called
- **THEN** the response includes a `flow` toolset entry, and
  `load_toolset("flow")` exposes exactly the six named tools in order

#### Scenario: calls pass the published schema
- **WHEN** a client sends each tool's documented arguments
- **THEN** the compiled input schema accepts them before the handler runs

### Requirement: single writer of `.konnect/flow/` state
The system SHALL treat `<project_dir>/.konnect/flow/` as state written only
by the six `flow_*` tools, with every file under it written by exactly one
tool and call shape. No flow tool SHALL accept a caller-supplied path below
`project_dir`, and `project_dir` SHALL be a directory containing a
`*.kicad_pro` file at its top level.

#### Scenario: no caller path reaches the flow directory
- **WHEN** a `flow_status` `read` entry names a path outside the readable
  record, gate, handoff, memory and log names
- **THEN** the tool returns an `invalid_argument` error and reads nothing

#### Scenario: a folder that is not a KiCad project is refused
- **WHEN** any flow tool is called with a `project_dir` that has no
  top-level `*.kicad_pro`
- **THEN** the tool returns an error and creates nothing

### Requirement: `flow_status` reads state and reality
The system SHALL provide `flow_status(project_dir, read?)` returning the
job and its phase (or none), the current `design_state_hash` and the files
it covered, every `~<name>.lck` lock file beside those files, the gate
approvals — a recomputed validity for the gate the job currently stands at,
and `passed` with the hashes it was approved at for every gate already
left — `pending_approvals`, `deferred_findings`, `queue`, the FIX-round
count per review phase, the
newest transition, the job's handoffs, a `next_step`, and the contents of
each requested readable name. `flow_status` SHALL NOT refuse because of flow
state and SHALL create nothing.

#### Scenario: status on a project with no active job
- **WHEN** `flow_status` is called against a project directory with no
  `.konnect/flow/` directory
- **THEN** the response reports no job and no phase, without error, and no
  directory is created

#### Scenario: status reports the current design hash and any lock files
- **WHEN** `flow_status` is called against a project whose KiCad editor
  holds `~<name>.kicad_pro.lck` and `~<name>.kicad_pcb.lck`
- **THEN** the response includes the current `design_state_hash` and names
  both lock files

#### Scenario: an unreadable state file is reported, not fatal
- **WHEN** `STATE.md`'s front matter does not parse
- **THEN** `flow_status` succeeds and reports a `state_error`

#### Scenario: an agent reads its input records through the tool
- **WHEN** `flow_status` is called with `read` naming `constraints.md`
- **THEN** the response carries that record's content, or lists it as
  missing

#### Scenario: a gate already left reports passed, not a recomputed validity
- **WHEN** `flow_status` is called for a job whose phase has moved past a
  gate it approved
- **THEN** that gate's entry reports `status: passed` with the
  `design_hash_at_approval` and `package_hash_at_approval` it was approved
  at, and carries no recomputed `valid` field

### Requirement: `flow_start` opens exactly one job per project
The system SHALL provide `flow_start(project_dir, objective, lane, phases?,
mode?)` opening a new job with `phase` set to the first entry of the
resolved phase sequence. `lane` SHALL be one of `new_board`,
`board_revision`, `review_only`, `fab_only`, `photo_to_kicad`; `mode` SHALL
be `guided` (default) or `autonomous`. `phases` SHALL be required for every
lane except `new_board` (which defaults to the full canonical sequence) and
SHALL be a non-empty, strictly increasing subsequence of the canonical
order that never starts with a gate and that includes each gate whose
phase it includes (`architecture`, `placement`, `manufacturing`). The
converse SHALL also hold: `phases` that includes a gate SHALL include the
phase that produces it (`gate:architecture` requires `architecture`,
`gate:placement` requires `placement`, `gate:purchase` requires
`manufacturing`), so the gate's package (bound at approval) always contains
a record produced in this job, never one left over from a closed or
earlier job.

#### Scenario: a job cannot start while one is already active
- **WHEN** `flow_start` is called against a project whose `STATE.md` names
  a job that is not `closed`
- **THEN** the tool returns a `conflict` error and `STATE.md` is unchanged

#### Scenario: an invalid phases array is rejected
- **WHEN** `flow_start` is called with a `phases` array that is out of
  order, repeats an entry, names an unknown token, starts with a gate, or
  includes `placement` without `gate:placement`
- **THEN** the tool returns an error naming the invalid entry, and no job is
  opened

#### Scenario: a gate without its producing phase is rejected
- **WHEN** `flow_start` is called with a `phases` array that names a gate
  (`gate:architecture`, `gate:placement`, `gate:purchase`) without the phase
  that produces it (`architecture`, `placement`, `manufacturing`)
- **THEN** the tool returns an error naming the missing phase, and no job is
  opened

#### Scenario: an unrecognized lane is rejected
- **WHEN** `flow_start` is called with a `lane` outside the five lanes
- **THEN** the input schema rejects the call and nothing is written

#### Scenario: a closed job lets the next one start
- **WHEN** the project's job is `closed` and `flow_start` is called
- **THEN** a new job opens

### Requirement: `flow_advance` moves forward only with the phase's records
The system SHALL provide `flow_advance(project_dir, job_id, to_phase,
records?, evidence_calls?, reason?)`. A forward transition (to the next
entry of the job's sequence, or to `closed` from its last entry) SHALL
require every record the phase being left produces to be supplied in that
same call's `records`; a record already on disk SHALL NOT satisfy it, and a
supplied record SHALL belong to the phase being left. Leaving
`architecture` SHALL require `architecture.md`'s last non-empty line to be
`Readiness: PASS`. Records SHALL be written only when the whole transition
is accepted.

#### Scenario: advancing without the required record is refused
- **WHEN** `flow_advance` is called to leave `architecture` without
  `architecture.md` in `records`, even though an earlier `architecture.md`
  exists on disk
- **THEN** the tool returns an error naming the missing record, nothing is
  written, and the phase does not change

#### Scenario: advancing writes the supplied records and moves the phase
- **WHEN** `flow_advance` is called with the current phase's records and the
  next phase as `to_phase`
- **THEN** the records exist under `records/` with the supplied content,
  `STATE.md`'s phase is `to_phase`, and a history entry records the design
  hash

#### Scenario: skipping a phase is refused
- **WHEN** `to_phase` is a later entry that is not the next one
- **THEN** the tool returns an error and the phase does not change

#### Scenario: a blocked readiness line cannot leave architecture
- **WHEN** the supplied `architecture.md` ends with `Readiness: BLOCKED —
  <reason>`
- **THEN** the tool returns an error and the phase stays `architecture`

### Requirement: `flow_advance` rewinds, abandons and closes
The system SHALL let `flow_advance` move to any earlier entry of the job's
sequence (a rewind) or to `closed` from a phase that is not the last (an
abandon) only with a non-empty `reason` and no `records`. A rewind SHALL
remove the approvals of every gate at or after its target, and a rewind out
of `schematic_review` or `prefab_review` SHALL count as one FIX round for
that phase. A rewind target SHALL NOT be a gate phase; the caller SHALL
rewind to the phase that produces that gate instead, whose forward
re-advance re-supplies the records and recomputes the gate's package keys.

#### Scenario: a FIX round is a rewind with a reason
- **WHEN** `flow_advance` rewinds from `schematic_review` to `schematic`
  with a `reason`
- **THEN** the phase is `schematic`, later gate approvals are gone, and
  `flow_status` reports one FIX round for `schematic_review`

#### Scenario: a rewind without a reason is refused
- **WHEN** a rewind or abandon is requested with an empty `reason`
- **THEN** the tool returns an error and the phase does not change

#### Scenario: a rewind cannot target a gate
- **WHEN** `flow_advance` is asked to rewind to a gate phase
- **THEN** the tool returns an error naming the gate and the phase that
  produces it, and the job's phase does not change

### Requirement: `flow_advance` records an evidence check
The system SHALL compare the tool names given in `evidence_calls` with the
server's recent-call record at the moment of the `flow_advance` call and
store which were confirmed by a successful call, which appeared only with
another status, and which were absent, in the transition's history entry.
The check SHALL be reported, never used to refuse the transition.

#### Scenario: an uncalled tool is reported absent
- **WHEN** `flow_advance` cites `run_erc` and `render_schematic_png` and
  only `run_erc` succeeded in the recent-call record
- **THEN** the history entry confirms `run_erc` and lists
  `render_schematic_png` as absent, and the transition still happens

### Requirement: `flow_gate` records a hash-bound approval of what was shown
The system SHALL provide `flow_gate(project_dir, job_id, gate_name,
decision, summary, user_words)`, `gate_name` one of `architecture`,
`placement`, `purchase`, callable only while the job is at that gate. On
`decision: approve` it SHALL record `design_hash_at_approval` and the
package hash (over the records of the phases since the previous gate), and
SHALL refuse when either differs from the value recorded when the job
entered the gate, when the gate is `architecture` and `records/
architecture.md` does not end with `Readiness: PASS`, or when `user_words`
is empty and either the job is in `guided` mode or the gate is `purchase`.

#### Scenario: an architecture gate approval requires a passing readiness line
- **WHEN** `flow_gate` approves `architecture` while `records/
  architecture.md` ends with `Readiness: BLOCKED — <reason>`
- **THEN** the tool returns an error naming the blocked readiness line, and
  no gate record is written

#### Scenario: an approval without the user's own words is refused
- **WHEN** `flow_gate` is called with `decision: approve` and an empty
  `user_words` in a guided job
- **THEN** the tool returns an error, and no gate record is written

#### Scenario: autonomous mode still stops at purchase
- **WHEN** an autonomous job approves `placement` with empty `user_words`
  and then approves `purchase` with empty `user_words`
- **THEN** the placement approval is recorded as approved by the session,
  and the purchase approval is refused

#### Scenario: an approval binds to the design hash at that moment
- **WHEN** `flow_gate` approves with every precondition satisfied
- **THEN** `records/gates/<gate_name>.md` and `STATE.md` record
  `design_hash_at_approval` equal to the `design_state_hash` computed during
  the call

#### Scenario: a design changed after the package was produced cannot be approved
- **WHEN** a design file changes after the job entered `gate:placement` and
  `flow_gate` then approves `placement`
- **THEN** the tool returns a `stale_target` error and records no approval

### Requirement: leaving a gate requires a still-valid approval
The system SHALL refuse a forward `flow_advance` out of a gate phase unless
that gate was approved during the job's current visit to it and both the
current `design_state_hash` and the current package hash still equal the
values recorded at approval.

#### Scenario: advancing past a gate whose approval hash is stale is refused
- **WHEN** a design file changes after `flow_gate` approved `placement` and
  `flow_advance` is called to move into `routing`
- **THEN** the tool returns an error naming the stale gate, and the phase
  does not change

#### Scenario: a rewind and return asks again
- **WHEN** a job rewinds from after `gate:architecture` to `architecture`
  and advances back into `gate:architecture`
- **THEN** leaving the gate requires a new approval

### Requirement: `flow_log` requires a reason and a rollback for a decision
The system SHALL provide `flow_log(project_dir, job_id, kind, message,
why?, rollback?, role?, scope?)`, `kind` one of `decision`, `evidence`,
`lesson`, `handoff`, appending one entry to the file its kind owns: the job
log for `decision` and `evidence`; `memory/<role>.md` for a `lesson` scoped
`project`; `records/lessons-candidates.md` for a `lesson` scoped `role` or
`technology`; a new `handoffs/<job_id>/<NN>-<role>.md` for a `handoff`.
`decision` SHALL require non-empty `why` and `rollback`; `lesson` SHALL
require `role` and `scope`; `handoff` SHALL require `role`; a parameter
that does not apply to the kind SHALL be refused.

#### Scenario: a decision without a reason is refused
- **WHEN** `flow_log` is called with `kind: decision` and an empty `why`
- **THEN** the tool returns an error, and nothing is appended

#### Scenario: a decision without a rollback is refused
- **WHEN** `flow_log` is called with `kind: decision` and an empty
  `rollback`
- **THEN** the tool returns an error, and nothing is appended

#### Scenario: evidence needs no reason or rollback
- **WHEN** `flow_log` is called with `kind: evidence` and only `message`
- **THEN** the entry is appended to the job log without error

#### Scenario: each handoff gets its own file
- **WHEN** two `kind: handoff` entries are logged with `role: review`
- **THEN** `handoffs/<job_id>/01-review.md` and `02-review.md` both exist

### Requirement: `flow_defer` never refuses for flow state
The system SHALL provide `flow_defer(project_dir, job_id, kind,
description, owner?)`, `kind` one of `finding`, `queue_item`,
`pending_approval`, appending one entry to `STATE.md`'s matching list in
any phase, `closed` included. It SHALL refuse only an argument error: a
`job_id` that is not the project's job, or an empty `description`.

#### Scenario: a deferred finding is recorded
- **WHEN** `flow_defer` is called with `kind: finding` and a `description`
- **THEN** `STATE.md`'s `deferred_findings` list gains the entry

### Requirement: `STATE.md` round-trips and serializes concurrent writers
The system SHALL keep `STATE.md` as machine-parsed front matter plus a body
regenerated on every write, SHALL reproduce any stored value unchanged
through a write and a read, SHALL refuse to mutate a job whose front matter
does not parse, and SHALL serialize concurrent flow calls on one project so
that no accepted change is lost. Writing `STATE.md` SHALL be the one commit
point for a transition or a gate decision: a derived side file (a gate
record, a log entry) SHALL be written only after `STATE.md` already holds
the change, and a failure of that later write SHALL be reported as a
success with a `warning` field, never as an error that could invite a
retry to re-apply an already-committed decision.

#### Scenario: hostile text survives a round trip
- **WHEN** a job's objective contains `---`, quotes, a newline, a colon,
  accented letters and backticks
- **THEN** reading `STATE.md` back yields the identical objective

#### Scenario: concurrent calls do not lose entries
- **WHEN** two `flow_defer` calls on the same project run concurrently
- **THEN** both entries are present in `STATE.md` afterwards

#### Scenario: a side-file failure after `STATE.md` commits is a warning, not a lost decision
- **WHEN** `STATE.md` is updated with a gate decision or a phase transition
  and the gate record or log entry cannot then be written
- **THEN** the tool reports success with a `warning` field naming the failed
  write, `STATE.md` already holds the decision, and no error asks the
  caller to repeat it
