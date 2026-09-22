## Context

Source of intent: `docs/ORQUESTRADOR_DO_KONNECT.html` (PT-BR, 2026-09-21).
Its seven "Decisões" were answered by the user the same day and are binding:
`/konnect <request>` through the `konnect` skill; guided mode by default
(three user gates: architecture, placement, purchase) plus an optional
per-job autonomous mode that stops only at purchase; a Rust `flow` toolset
inside `konnect.exe` whose approvals bind to `design_state_hash`; state in
`<project>/.konnect/flow/`; global role memory at
`~/.konnect/agents/<role>/MEMORY.md` until the brain exists; all six new
agents; built through orc. This change builds the doc's order items
"1 · Mestre e estado" and "2 · Elenco"; see Non-Goals for the rest.

**Merged base, verified.** Implementation branches from `main` after
`orc/board-dossier-reconstruction` (1653e79) merges.
`git merge-tree --write-tree ee55a12 1653e79` exits 0 with tree
`a4351038` and no conflicts; every citation below marked "merged" was read
from that tree. The planner's merge risk is resolved: the merged
`crates/konnect/assets/skills/konnect/SKILL.md` carries both The One Rule's
scripted board fallback (merged `:8-44`) and the photo-to-board row
(merged `:144`) and routing bullets (`:199-217`). The merged agents keep
`main`'s budgets: schematic `maxTurns: 200`, layout `300`, review `300`
(review also preloads `kicad-schematic`).

**Code facts this design is built on** (merged tree unless noted):

| Fact | Where | Consequence here |
|---|---|---|
| Registered toolsets: 22; `photo_intake` is the 17th entry | `router/registry.rs:22`, `:120` | `flow` is the **23rd** toolset, not the 21st |
| `ToolsetMeta.tool_count` is hand-written and asserted; cap 20 | `router/mod.rs:20`, `:358`, `:355` | `tool_count: 6`; add an ordered-membership test like `:381` |
| Every object schema node is closed (`additionalProperties:false`) unless it opts in; an object param **without** `properties` becomes uncallable | `tools/mod.rs:134` | every `flow` object node declares `properties`; no open maps |
| Typed errors exist: `InvalidArgument`, `Conflict`, `StaleTarget` | `mcp/error.rs:49`, `:54`, `:80` | no new `ToolErrorKind` variant |
| `get_path` does no validation; path confinement is per tool | `tools/mod.rs:869`; pattern `photo_intake.rs:725-741` | `flow` takes no caller path below `project_dir` |
| Cross-process locked, atomic writers exist and accept any file type | `konnect-sexp/src/writer.rs:106` (`write_atomic`), `:176` (`transact_atomic`), `:198` (`read_consistent`), `:397` (`write_new_atomic`), lock dir `:315` | single-writer `STATE.md` without new locking code |
| No YAML crate and no date crate in `Cargo.lock`; RFC 3339 helper is private | `photo_intake.rs:1159` | JSON front matter; widen `now_rfc3339_utc` to `pub(crate)` |
| `design_state_hash` has zero callers; hashes every design file recursively; skips only `.git`, `.konnect`, `*-backups`, autosave, `.lck` | `design_hash.rs:34`, `:63`, `:70` | first consumer; semantics and cost measured in D11 |
| Observer ring keeps the last **100** calls, tool name + status only, no args; JSONL at `%APPDATA%/konnect/logs/calls.jsonl` | `observability.rs:27`, `:221`; `mcp/handler.rs:89` | D6 checks evidence inside `flow_advance`, not after the agent returns |
| Codex gets skills only (`~/.agents/skills`); agents install for Claude only | `crates/konnect/src/install.rs:239`, `:243`, `:276` | every phase must be runnable from skills alone (D8) |
| Agent `skills:` is parsed as a **block** list (`  - name`); a flow list `[a, b]` parses as empty | `asset_references.rs:151` | new agent frontmatter copies the existing block shape |
| Doc sweeps check every `.md`/`.json` except `target`, `node_modules`, `.git`, `.claude`, `dist`, `build` — including `openspec/` and `.orchestrator/` — for a stale 3-digit catalogue total and for **any** number before "toolset" | `doc_tool_counts.rs:126`, `:165`, `:225` | D12; no planning doc may put a digit right before those nouns |

**Existing pieces reused, not reinvented:** the photo-intake approval
pattern (stored hash, recomputed and compared on every read); the `tool!`
macro and static registry; the manifest/install/asset-guard chain;
`review-orchestration.md`'s ledger vocabulary (`FIX_BEFORE_FAB`).
`gates.rs`'s `GateStatus` is a different concept (a composite of automated
checks) and is not used by `flow.rs`; it is named only so the two "gate"
words are not conflated.

## Goals / Non-Goals

**Goals:**
- `flow` is the only writer of `<project>/.konnect/flow/`; every file in
  D2's tree has exactly one tool and one call shape that writes it.
- A human approval binds to the exact design **and** the exact package
  records it was shown, and a later change to either revokes it (D11).
- A job survives a session restart: a new session calls `flow_status` and
  gets phase, gate validity, lock files, deferred items, handoffs and the
  next step without asking the user anything.
- The FIX loop ("volta à camada que falhou") and an abandoned job are
  representable states, not dead ends (D3).
- `/konnect <request>` picks a lane; bounded edits and single-agent asks
  stay on today's direct path.
- All six new agents ship with a real method; every phase also runs under
  Codex, where no bundled agent is installed.

**Non-Goals (four, unchanged):**
1. **`konnect-vcs` checkpoints.** The crate is an unwired scaffold;
   `flow_advance` neither creates nor requires a checkpoint. Revisit when
   `konnect-vcs` has a tool-facing API.
2. **A Codex cross-reviewer** (doc phase "5 · Depois").
3. **Brain integration.** Lessons scoped to a technology stay queued in
   `records/lessons-candidates.md`; nothing reads or writes a brain path.
4. **Firmware-contract hand-off to `orc`.** `manufacturing.md` carries
   release notes for this project only.

## Decisions

### D1. `flow` toolset: six tools, exact contract

`crates/konnect-core/src/tools/flow.rs`, registered as the 23rd entry of
`ALL_TOOLSETS` (`name: "flow"`, `category: "orchestration"` — a new, 11th
category; nothing hardcodes the category list), `tool_count: 6`, tools in
this order: `flow_status`, `flow_start`, `flow_advance`, `flow_gate`,
`flow_log`, `flow_defer`. Not in `STARTER_KIT` (`registry.rs:20`): callers
`load_toolset("flow")`. `BoardAccess::None` for all six (default; no hook).

**Input schemas** (every object node has `properties`, so
`close_input_schema` closes it and `fixed_records_are_closed…`,
`router/mod.rs:185`, needs no allowlist entry; enums are schema-enforced,
so an unknown value is rejected before the handler runs):

| Tool | Required | Optional |
|---|---|---|
| `flow_status` | `project_dir` (string) | `read` (array of string, D4's readable names) |
| `flow_start` | `project_dir`, `objective` (string), `lane` (enum: `new_board`, `board_revision`, `review_only`, `fab_only`, `photo_to_kicad`) | `phases` (array of enum: the 12 canonical tokens, D3), `mode` (enum: `guided`, `autonomous`; default `guided`) |
| `flow_advance` | `project_dir`, `job_id` (string), `to_phase` (enum: 12 tokens + `closed`) | `records` (array of `{filename: enum of D4's 10 record names, content: string}`, both required), `evidence_calls` (array of string), `reason` (string) |
| `flow_gate` | `project_dir`, `job_id`, `gate_name` (enum: `architecture`, `placement`, `purchase`), `decision` (enum: `approve`, `reject`), `summary` (string), `user_words` (string) | — |
| `flow_log` | `project_dir`, `job_id`, `kind` (enum: `decision`, `evidence`, `lesson`, `handoff`), `message` (string) | `why`, `rollback` (string), `role` (enum, D7's 12 role slugs), `scope` (enum: `role`, `technology`, `project`) |
| `flow_defer` | `project_dir`, `job_id`, `kind` (enum: `finding`, `queue_item`, `pending_approval`), `description` (string) | `owner` (string) |

A signature-shaped example in any asset (`flow_gate(project_dir, job_id,
…)`) must name every required parameter above, or
`call_examples_name_real_parameters` (`asset_references.rs:598`) fails.

**Behaviour and refusals** (error kind in brackets; "refused" always means
nothing was written):

| Tool | Does | Refuses when |
|---|---|---|
| `flow_status` | Reads `STATE.md` (shared lock) and reality: `job` (null or `{job_id, objective, lane, mode, started_at, phases}`), `phase` (null, a token, or `closed`), `design_hash` + `design_files` (D11), `lock_files`, `gate_approvals` (each entry `status: "current"` with a recomputed `valid` for the gate the job stands at now, `status: "passed"` with no `valid` field for every other approved gate — Fix round 1, DECISION D), `pending_approvals`, `deferred_findings`, `queue`, `fix_rounds`, `last_transition` (newest history entry, incl. `evidence_check`), `handoffs` (names), `next_step` (`{phase, required_records, next_phase, is_gate}`), `contents` (for each `read` name found) and `missing` (requested, absent). Creates nothing. | Never for a flow-state reason: no `.konnect/flow/` → empty state; unparseable `STATE.md` → `state_error` field. Only argument errors: `project_dir` invalid (D2) or a `read` name outside D4's readable set [`invalid_argument`]. |
| `flow_start` | Validates `phases` (D3), writes a new `STATE.md` with `phase` = first entry, appends a `start` entry to the job log. `job_id` = slug(objective) + `-YYYYMMDD-HHMMSS` UTC (slug: lowercase ASCII alphanumerics, other runs → `-`, trimmed, max 40, `job` if empty). | an active job exists (`phase` ≠ `closed`) [`conflict`, path of `STATE.md`]; `STATE.md` unparseable [`conflict`]; `phases` omitted for a lane other than `new_board`, or invalid per D3 [`invalid_argument`, naming the entry]; empty `objective` [`invalid_argument`]. |
| `flow_advance` | Classifies `to_phase` against the job's sequence as forward, rewind, close or abandon (D3); on forward, writes the supplied `records`; appends a history entry with `design_hash` (and `package_hash` when entering a gate), `evidence_calls` and `evidence_check` (D6); writes `STATE.md`; appends a log entry. | `job_id` ≠ the project's job, or the job is closed [`stale_target`]; any D3 rule [`invalid_argument`, field `to_phase`/`reason`]; forward without every record D4 requires of the phase being left **in this call's `records`**, or with a record belonging to another phase [`invalid_argument`, field `records`]; leaving `architecture` when `architecture.md` fails D5 [`invalid_argument`]; leaving a gate phase without an approval recorded during the current visit to it, or with `design_hash`/`package_hash` ≠ the approval's [`stale_target`, target `gate:<name>`]. |
| `flow_gate` | On `approve`: records `gate_approvals[<gate>]` = `{decision, approved_by, approved_at, design_hash_at_approval, package_hash_at_approval, visit, summary, user_words}` in `STATE.md`; writes `records/gates/<gate>.md`; appends a log entry. On `reject`: removes any approval for that gate, writes the gate file with `decision: reject`, logs it; phase unchanged. | `job_id` mismatch [`stale_target`]; current phase ≠ `gate:<gate_name>` [`invalid_argument`]; empty `summary` [`invalid_argument`]; `approve` with empty `user_words` when `mode` is `guided` **or** the gate is `purchase` [`invalid_argument`]; `approve` for `architecture` when `records/architecture.md` fails D5 [`invalid_argument`]; `approve` when the current `design_hash` or package hash differs from the values recorded when the job entered this gate phase — the design or the package changed after it was produced, so the human did not see this state [`stale_target`, naming changed or missing package files]. |
| `flow_log` | Appends one timestamped Markdown entry; `kind` picks the file: `decision`/`evidence` → `log/<date>-<job_id>.md`; `lesson` + `scope: project` → `memory/<role>.md`; `lesson` + `scope: role`/`technology` → `records/lessons-candidates.md` (tagged with role and scope); `handoff` → new file `handoffs/<job_id>/<NN>-<role>.md` (NN = existing count + 1, two digits minimum). Allowed on a closed job (journal only). | `job_id` ≠ the project's job (active or closed) [`stale_target`]; `decision` without non-empty `why` and `rollback`; `lesson` without `role` and `scope`; `handoff` without `role`; a parameter that does not apply to `kind` (e.g. `scope` on `evidence`); empty `message` [all `invalid_argument`]. |
| `flow_defer` | Appends `{description, owner?, added_at, phase}` to `deferred_findings`, `queue` or `pending_approvals` in `STATE.md`, and a log entry. Lists are per job and append-only (resolution is recorded with `flow_log`). | Never for a flow-state reason (any phase, closed included); only `job_id` mismatch [`stale_target`] or empty `description` [`invalid_argument`]. |

**Who calls which tool.** The rule is "the party that produced the output
records it"; the orchestrating session never calls `flow_advance` **on an
agent's behalf** (planner decision, kept).
- `flow_status`: anyone, at session start, job resume, and by every agent to
  read its input records (`read`) — agents have only `mcp__konnect__*`, and
  the three existing agents' frontmatter is frozen, so a tool read is the
  one channel that reaches all of them (the photo pipeline's precedent:
  consumers load the approved map through `load_photo_review_map`, never the
  caller's summary).
- `flow_start`, `flow_gate`: the orchestrating session only (a gate needs
  the user's own words; a Task sub-agent cannot talk to the user).
- `flow_advance` forward out of a work phase: the agent that produced that
  phase's records, as its last action before returning. The orchestrating
  session is the producer — and so the caller — in exactly three cases:
  it merged a multi-reviewer ledger itself (`review-orchestration.md`
  Phase 4 assigns the merge to the orchestrator), it ran the phase itself
  (Codex, D8), or it is leaving a gate phase (gates are the session's).
- `flow_advance` rewind/abandon: the orchestrating session — routing a FIX
  to the failing layer is the orchestrator's decision.
- `flow_log`: whoever generates the entry, when it happens ("erro próprio se
  registra na hora"); every delegated agent persists its own handoff with
  `kind: handoff` before returning it.
- `flow_defer`: whoever finds the item.

*Rejected:* the master calling `flow_advance` from an agent's handoff text —
a paraphrase between the agent's evidence and the state that outlives it.

### D2. `.konnect/flow/` layout, `STATE.md` format, writes

```
<kicad project>/.konnect/
├── project.json                          # config toolset (exists)
└── flow/
    ├── STATE.md                          # flow_start/advance/gate/defer
    ├── records/<D4 record>.md            # flow_advance (forward)
    ├── records/lessons-candidates.md     # flow_log lesson (role|technology)
    ├── records/gates/<gate>.md           # flow_gate
    ├── handoffs/<job_id>/<NN>-<role>.md  # flow_log handoff
    ├── log/<date>-<job_id>.md            # flow_log decision|evidence + every state change
    └── memory/<role>.md                  # flow_log lesson (project)

~/.konnect/agents/<role>/MEMORY.md        # orchestrating session only (D7)
```

`<date>` is the job's `started_at` UTC date, so one job has one log file.
`records/` persists across jobs (a revision job reads the previous board's
constraint record as its baseline); D4's same-call rule keeps an old record
from satisfying a new exit.

**`project_dir` resolution** (all six tools): `canonicalize()` the argument
(Windows yields a `\\?\` path — used for joins and `starts_with`, echoed in
responses); it must be a directory containing at least one `*.kicad_pro`
directly — refused otherwise, which also stops a caller from passing a
parent folder and hashing a whole document tree on every call. Only
`flow_start` creates the flow directory, and only after its validation
passed; the other mutating tools require an existing `STATE.md` (absent →
`stale_target`, nothing created) and create just the subdirectory their
file needs. The creating call re-canonicalizes the directory and refuses
unless it `starts_with` the canonical project (a `.konnect` symlinked
elsewhere) — the `prepare_map_dir` pattern (`photo_intake.rs:725-741`),
re-implemented privately (the photo helpers are private; the repo's
convention is a local helper, not a widened API). Every file name below the
flow directory comes from an enum, a server-computed number, or the
validated `job_id` (`^[a-z0-9][a-z0-9-]*$`); there is no caller path.

**`STATE.md` format.** No YAML crate exists, and a hand-rolled YAML
writer is the round-trip risk this format must not have. The front matter
is pretty-printed JSON between `---` fences (JSON is valid YAML 1.2, so
Markdown tooling still sees front matter); `serde_json` guarantees the
round trip for any string (objective with `---`, quotes, newlines, `:`,
PT-BR accents, backticks — JSON escapes newlines, so no front-matter line
can equal `---`). The body is regenerated from the struct on every write
and states that it is: hand edits below the front matter are discarded;
hand edits inside it are refused as `state_error`/`conflict` until fixed —
never guessed at. The parser normalizes CRLF first (a user's git autocrlf).

```
---
{
  "schema": 1,
  "job_id": "conversor-usb-serial-esp32-20260921-140000",
  "objective": "Conversor USB-serial com ESP32",
  "lane": "new_board",
  "mode": "guided",
  "phases": ["requirements", "architecture", "gate:architecture", "schematic", "schematic_review", "placement", "gate:placement", "routing", "prefab_review", "manufacturing", "gate:purchase", "learn"],
  "phase": "architecture",
  "started_at": "2026-09-21T14:00:00Z",
  "gate_approvals": {},
  "pending_approvals": [],
  "deferred_findings": [],
  "queue": [],
  "history": [
    {"kind": "start", "to": "requirements", "at": "2026-09-21T14:00:00Z", "design_hash": "9f2c…"},
    {"kind": "advance", "from": "requirements", "to": "architecture", "at": "2026-09-21T14:20:05Z", "design_hash": "9f2c…", "records": ["constraints.md"]}
  ]
}
---
# Flow state — conversor-usb-serial-esp32-20260921-140000

> Written by the Konnect `flow` toolset. This body is regenerated on every write.
(phase and next step, gates with validity, pending approvals, deferred findings, queue, history table)
```

Structs (`JobState`, `HistoryEntry`, `GateApproval`, `DeferredItem`) use
`#[serde(deny_unknown_fields)]` (a newer binary's field is refused, not
silently dropped by an older one's rewrite) and
`skip_serializing_if = "Option::is_none"` / `Vec::is_empty` on optional
fields, so absent stays absent. `schema` > 1 is refused. Round-trip
property: `parse(render(s)) == s`.

**Single writer, concurrent calls.** Every mutation runs inside
`tokio::task::spawn_blocking` (blocking I/O and a blocking lock wait) and
inside `konnect_sexp::transact_atomic(STATE.md, …)` (`writer.rs:176`): an
OS lock in the per-user Konnect state directory keyed on the canonical path
(`writer.rs:315`), so two server processes (a Claude and a Codex session,
or parallel sub-agents) serialize; the replacement is scratch file + fsync +
rename (`writer.rs:116`). Inside the closure: (1) parse and validate
everything — a refusal returns the content unchanged, so nothing is
written; (2) compute hashes; (3) write side files; (4) return the new
`STATE.md`. Lock order is always `STATE.md` first. Side files: records and
gate files via `write_atomic`; handoff files via `write_new_atomic`
(no-clobber, `writer.rs:397`); log, memory and candidates appends via one
`OpenOptions::append` `write_all` per entry (the observer's own JSONL
pattern). The first `STATE.md` is created with `write_new_atomic`, so two
racing `flow_start` calls cannot both open a job — the loser falls through
to the locked path and sees the active job. `flow_log` returns the content
unchanged (no rewrite) but still runs under the lock. `flow_status` reads
with `read_consistent` (`writer.rs:198`).

**Fix round 1 (reviewer 11 minor 1, DECISION C).** Step 3 above is wrong
for `flow_gate` and the two transitions that carry a log entry
(`flow_advance` forward/rewind/abandon, via `commit_transition`): today
`apply_gate` writes `records/gates/<gate>.md` and `commit_transition`
appends the log **before** `transact_atomic`'s rename commits the new
`STATE.md` (`flow.rs:2348-2384`, `:2235`). Reviewer 11 reproduced the
failure this invites: a `STATE.md` write that fails after the gate file
and the log already say `reject` leaves a permanent record (D3: the log is
a closed job's history) of a decision `STATE.md` never held — a retry then
risks re-applying it. `STATE.md`'s successful rename is now the one commit
point:
- **Before** the commit, unchanged: a forward `flow_advance`'s phase
  records (`records/<D4 name>.md`) via `write_atomic`, inside the closure
  as today — D4's re-supply rule already makes a stray copy on disk
  harmless on a retry (D2's original "harmless" argument, which covers
  records only).
- **After** the commit succeeds, new: the gate file (`flow_gate`) and the
  job's log entry (`flow_gate`, and every `flow_advance` transition).
  `apply_gate` and `commit_transition` stop writing them inside the
  closure; they return the data to write (the gate file's text and path,
  the log line) alongside the new `STATE.md` text, and the caller writes
  them only once `transact_atomic`/`write_new_atomic` has returned `Ok`.
- A failure of that post-commit write is reported as **success**, with a
  `warning` field on the response carrying the error text — the same shape
  `flow_start` already uses for `log_error` (`write_first_state` succeeds,
  `append_log` may then fail, and the response still returns the new job
  with `log_error` set, `flow.rs:1519-1532`). `flow_gate`/`flow_advance`
  name the field `warning`.
- This makes reviewer 11's P2 impossible in the direction that matters: a
  `STATE.md` failure can no longer leave a gate file or a log entry ahead
  of what `STATE.md` holds, because those writes have not happened yet
  when `STATE.md` fails. A post-commit side-write failure is now the only
  possible drift, and it is reported, not hidden or turned into a retryable
  error.

If step 4 fails after step 3 (the unchanged, before-commit case — phase
records only), the call reports the error and the records may already hold
the new bytes — harmless, because D4 requires records to be re-supplied on
the retry.

*Rejected:* `STATE.json` plus a rendered `STATE.md` (two files, one truth,
a drifting render step); hand-rolled YAML (lossy round trip); a new YAML
dependency (`serde_yaml` is unmaintained; the need is met by `serde_json`).

**Fix round 2 (reviewer 16 minors 3–4, qa 15 deferred 1 and 5, DECISION
K — text-only).** Three published texts still described the
pre-DECISION-C/D behaviour. `flow_status`'s own tool description
(`flow.rs:~3105`) and the `STATE.md` body's own rendered sentence
(`flow.rs:~664`) both said validity is recomputed for every approval, not
only for the gate the job stands at now — DECISION D already fixed the
computation; only these two *texts* lagged. `flow_gate`'s,
`flow_advance`'s and `flow_defer`'s published tool descriptions never
named the `warning` field DECISION C/H added, so a client with no
`konnect` skill loaded has no way to learn "never repeat the call".
`flow_advance`'s description also said "a refusal writes nothing", true
only of a *validation* refusal — a successful transition can still carry
a `warning` about an unwritten side file, which is not a refusal. All
five are text-only edits to `flow.rs`'s tool-registration strings and
`orchestration.md` §4 (which had the same "recomputed for every approval"
error, contradicting the correct sentence its own §7 already carries from
task 8.5); tracked in tasks.md §9, no behaviour change.

**Fix round 2 (reviewer 16 minor 5, DECISION L — accepted, not fixed).**
The two post-commit side writes DECISION C/H added (the gate file and the
job's log entry) run after `STATE.md`'s lock is released, so two opposing
`flow_gate` calls, or two `flow_defer` calls, can in principle land their
side write in the reverse of their commit order — an inversion needs one
caller descheduled for longer than another caller's whole transaction,
fsync included. Reviewer 16 probed this directly: 200 approve/reject
rounds and 400 concurrent `flow_defer` calls across two server processes,
0 divergences. `STATE.md` stays the single source of truth and no asset
ever reads `records/gates/*.md` or the log to decide anything, so the
trade-off is cosmetic ordering only. DECISION L accepts it rather than
exporting `konnect_sexp`'s lock to also cover the side write — widening
that crate's API is its own change (this file's "existing pieces reused,
not reinvented" already treats the lock as `konnect-sexp`'s to own, and
this is already a deferred finding in qa 15). No task.

### D3. Phase sequence, lanes, transitions

Canonical order (12 tokens):

```
requirements → architecture → gate:architecture → schematic →
schematic_review → placement → gate:placement → routing →
prefab_review → manufacturing → gate:purchase → learn
```

The doc's phase "0 · Pedido" is `flow_start` itself.

| Lane | `phases` | Doc lane |
|---|---|---|
| `new_board` | omitted → the full sequence (a given `phases` is validated like any other) | Placa nova |
| `board_revision` | caller-chosen subsequence, e.g. `[architecture, gate:architecture, schematic, schematic_review, prefab_review, manufacturing, gate:purchase]` | Revisão de placa (ECO) |
| `review_only` | `[prefab_review]` | Só revisão |
| `fab_only` | `[manufacturing, gate:purchase]` | Só fabricação |
| `photo_to_kicad` | `[schematic, schematic_review, placement, gate:placement, routing, prefab_review, manufacturing, gate:purchase, learn]`, opened only after the photo agents' two approvals | Foto → KiCad |

Bounded edit and single agent never open a job. **Validator** (generic,
no per-lane arm — planner decision, kept): non-empty; every token known;
strictly increasing in canonical order (no repeat, no reorder); a gate is
never the first entry; and **a gated phase brings its gate**
(`architecture` ⇒ `gate:architecture`, `placement` ⇒ `gate:placement`,
`manufacturing` ⇒ `gate:purchase`) — a lane subset cannot silently drop a
human gate.

**Fix round 1 (reviewer 11 SERIOUS, DECISION A).** The validator also
enforces the converse: a gate entry requires its phase
(`gate:architecture` ⇒ `architecture`, `gate:placement` ⇒ `placement`,
`gate:purchase` ⇒ `manufacturing`). Without it, `current_package`'s
per-gate slice (D11) can be non-empty while still missing the phase that
was supposed to produce the reviewed record, and `records/`'s persistence
across jobs (D2) then lets a closed job's or an earlier job's leftover
record stand in for this job's evidence at approval time — reproduced by
reviewer 11: `[routing, prefab_review, gate:purchase]` approved a purchase
against a previous job's `manufacturing.md`, and
`[requirements, gate:architecture, schematic]` approved architecture
against a stale on-disk `architecture.md` that was never re-produced.
`validate_phases` gains a second loop over `GATED_PHASES`, checked the
other direction, naming the missing phase and its gate in the same error
shape as the existing rule. Every documented lane already satisfies both
directions (`new_board`, the `board_revision` example, `fab_only`,
`photo_to_kicad`, `review_only`), so no lane table entry changes.

**Transitions** (`flow_advance`, `to_phase` relative to the job's
`phases`):
- **forward** — the entry right after the current one, or `closed` from the
  last entry. D4's record rule and D11's gate rule apply.
- **rewind** — any earlier entry. Requires non-empty `reason`; `records`
  must be absent; removes `gate_approvals` of every gate at or after the
  target (they must be re-granted on the way back). A rewind whose `from`
  is `schematic_review` or `prefab_review` counts as one FIX round in
  `flow_status.fix_rounds`. **Fix round 1 (reviewer 11 minor 3, DECISION
  B).** The target SHALL NOT be a gate: `move_back` now refuses
  `is_gate(to_phase)` before doing anything else, naming the gate and the
  phase that produces it (`GATED_PHASES`, reversed — the same table
  DECISION A reads). Reviewer 11 reproduced a rewind from `routing` to
  `gate:placement` that recorded that gate's keys against the *routed*
  board while the package shown at the (still pre-routing) approval was
  `placement.md`'s images — an approval bound to evidence the human never
  saw. No documented path rewinds to a gate (design §5 sends every FIX to a
  producer), so the refusal costs no lane. The caller rewinds to the
  producing phase instead (`placement` for `gate:placement`); its forward
  re-advance re-supplies the records and `record_gate_keys` recomputes the
  package hash fresh, so the next approval is never stale by construction.
- **abandon** — `closed` from any entry but the last. Requires `reason`.
- anything else (skip ahead, same phase, a token outside the job) is refused.

`closed` is terminal; `flow_start` may then open the next job (the closed
job's history lives on in its log file).

**Autonomous mode** (decision 2): `mode: autonomous` is set at
`flow_start` and changes one rule — `flow_gate` may approve `architecture`
and `placement` with empty `user_words`, recording `approved_by: session`
(the `summary` carries the session's reasoning). `purchase` always needs
the user's words. Readiness, package and hash rules are unchanged, so an
autonomous approval is exactly as stale-proof as a human one.

*Rejected:* one `match` arm per lane (a revision is "só as fases
afetadas", a per-job subset); forward-only transitions (the FIX loop and an
abandoned job would have no representable state — the first abandoned job
would block `flow_start` until someone hand-deleted `STATE.md`).

### D4. Phase → required record; readable names

Leaving a work phase forward requires its records **in the same call's
`records`**. "Already on disk" does not count: records are written only by
`flow_advance`, so a file already on disk was written by an earlier visit
or an earlier job — accepting it is how a stale `architecture.md` from the
previous board would let a revision skip its architecture work. A supplied
record must belong to the phase being left (one writer per record).

| Phase | Records to supply (the 10-name enum) | Default producer |
|---|---|---|
| `requirements` | `constraints.md` | `kicad-requirements-agent` |
| `architecture` | `architecture.md` (ends with D5's line), `worst-case.md`, `pin-plan.md` | `kicad-architecture-agent` |
| `schematic` | `schematic-evidence.md` (final ERC, shorted-net, render, cross-sheet reference results and the layout handoff) | `kicad-schematic-build-agent` |
| `schematic_review` | `ledger-schematic.md` (verdict and action per finding) | `kicad-design-review-agent`, or the session after a multi-reviewer merge |
| `placement` | `placement.md` (per-layer images, placement-gate result) | `kicad-pcb-layout-agent` |
| `routing` | `routing.md` (DRC with parity, width audit) | `kicad-pcb-layout-agent` |
| `prefab_review` | `ledger-prefab.md` (verdicts, readiness level) | as `schematic_review` |
| `manufacturing` | `manufacturing.md` (package summary, BOM integrity, indicative cost, release notes) | `kicad-manufacture-agent` |
| `gate:*` | none — the approval in `STATE.md` (D11) | orchestrating session |
| `learn` | none — `flow_advance(to_phase: closed)` | `kicad-curator-agent` |

Content rules beyond D5 ("no open `FIX_BEFORE_FAB`") are the producer's
and the orchestrator's to check, not the tool's: parsing every record
format would make each format change a `flow.rs` change.

**Readable names** for `flow_status(read)`: the 10 records,
`lessons-candidates.md`, `gates/architecture.md`, `gates/placement.md`,
`gates/purchase.md`, `log` (the current job's log), `memory/<role>.md`
(role from D7's enum) and `handoffs/<NN>-<role>.md` (any name
`flow_status.handoffs` lists). Anything else is `invalid_argument`; the
resolved path must stay under the flow directory. No size cap: records are
Markdown pages, and a large log is the caller's choice to read.

### D5. The readiness line

`architecture.md`'s last non-empty line, trimmed, is exactly
`Readiness: PASS`, or starts with `Readiness: BLOCKED` followed by a reason
naming the missing or invented value. One parser, two callers:
`flow_advance` refuses to leave `architecture` unless it reads `PASS`
(BLOCKED or malformed → the agent returns BLOCKED and the session collects
the value — typically a rewind to `requirements`), and `flow_gate` re-checks
it before approving `architecture` (the file may have been hand-edited). It
is the one record whose content the tool parses: the doc's single stated
exit condition for the phase every later phase depends on.

### D6. Evidence cross-check, recorded at the moment it can be checked

`flow_advance` compares `evidence_calls` against the observer ring
(`ctx.observer.recent(0)`, `observability.rs:221`) during the call and
stores `evidence_check: {confirmed, not_ok, absent, ring_calls}` in the
history entry: `confirmed` = a cited tool with at least one `ok` call in
the ring; `not_ok` = present only with another status; `absent` = not in
the ring. It reports; it never refuses (a resumed job's evidence may
predate this server process, and refusing on a call log is the doc's
phase-3 guarantee, out of scope).

Why inside the tool and not the planner's "master calls `get_recent_calls`
after the agent returns": the ring holds 100 calls (`observability.rs:27`);
a 200-turn build or parallel reviewers evict the evidence long before the
session looks, which would turn honest handoffs into FIX loops. At
`flow_advance` time the producer's evidence calls are its most recent
ones, and the check runs in the very server process that served them.
Limits, named: the ring has no caller identity or arguments, so this
catches "never ran", not "ran on another project" or "another agent ran it".

`orchestration.md` then instructs: before accepting a `DONE`, read
`flow_status.last_transition.evidence_check`; any cited call in `absent` or
`not_ok` → treat the handoff as `FIX` with `failing_layer: implementation`
and ask the agent to re-run and re-report; never re-run it yourself in the
agent's name. For a handoff with no transition (FIX, BLOCKED, sourcing,
library): `get_recent_calls(limit: 0)` right after the agent returns, and
for calls older than the ring, the lines of `calls.jsonl` newer than the
dispatch time.

### D7. Memory: tiers, readers, writers

Role slugs (12, the `role` enum): `requirements`, `architecture`,
`sourcing`, `schematic`, `library`, `layout`, `review`, `manufacture`,
`photo-intake`, `design-reconstruction`, `curator`, `orchestrator`.

| Tier | Path | Written by | Read by |
|---|---|---|---|
| Global role (decision 5) | `~/.konnect/agents/<role>/MEMORY.md`, ≤60 lines | the orchestrating session's own editor, only when promoting a curator-marked `role` candidate at `learn`; it compresses in place before exceeding 60 | the session, pasted verbatim into each brief's Context |
| Project role | `.konnect/flow/memory/<role>.md` | `flow_log(kind: lesson, scope: project)` — the agent, the moment it happens | the session (into the brief); agents via `flow_status(read)` |
| Candidates queue | `.konnect/flow/records/lessons-candidates.md` | `flow_log(kind: lesson, scope: role\|technology)` | the curator; the user at `learn`; `technology` entries wait for the brain (Non-Goal 3) |

Triage ("uma lição, um destino"), stated once in the `kicad-curator` skill:
about how a role works → `role`; about a technology or part (ESP32,
TPIC6B595, JLCPCB) → `technology`; else → `project`. No flow tool writes
under the user's home: the global tier sits outside `.konnect/flow/`, so the
single-writer rule does not apply, and a tool writing another person's home
directory from a project call is a larger trust step than this change needs.

*Rejected:* the curator writing `MEMORY.md` itself (it has no file-write
tool, and should not get one); a seventh `flow_*` memory tool (the doc's
contract is six tools, and `flow_log` already owns "append an entry").

### D8. `konnect` skill router, orchestration reference, templates

**Router** (additive; the Decision Tree and "Agent Routing and Mutation
Ownership" stay as they are): a short "Orchestrator — `/konnect <request>`"
section placed before the Decision Tree — pick the lane; bounded edit and
single agent fall through to today's behaviour; everything else reads
`references/orchestration.md`, then `load_toolset("flow")` and
`flow_status(project_dir)`. A three-row "Need → Read" table names
`references/orchestration.md`, `references/brief-template.md`,
`references/handoff-template.md` (one hop). The "Available Toolsets" table
(merged `:171-181`) gains `| Orchestration | flow |`. Agent Routing gains
one bullet per new agent (D10). `/konnect <request>` is Claude Code's skill
invocation; under Codex the same skill loads by name.

**`references/orchestration.md`** sections: lanes and the canonical
sequence (D3, including the two job-less lanes); phase playbook — per
phase: producer, the records its brief tells it to `read`, the records it
must supply, exit condition (D4); who calls which tool (D1); gates — what to
show the human (the package records, rendered images for placement,
cost/BOM for purchase), that `user_words` is the user's message quoted
verbatim, autonomous-mode rules (D3); the FIX loop — `failing_layer`
`requirement` → rewind to `requirements`, `architecture` → `architecture`,
`implementation` → the phase that produced the artifact; the three-round
cap: when `fix_rounds` for a review phase reaches 3 and the ledger still has
an open `FIX_BEFORE_FAB`, stop and escalate to the user instead of a fourth
round (guidance; the tool counts, the session decides); the evidence
cross-check (D6); resume (`flow_status` → lock files, gate validity —
**Fix round 1 (DECISION D)**: `valid` matters only while `phase ==
"gate:<name>"`; an approval of a gate already left reports `status:
passed` with the hashes it was approved at, not a reason to re-ask or
rewind —, `next_step`, latest handoff); requirements' one question round (the agent
returns BLOCKED with every product question at once; the session asks the
user once and re-briefs); sourcing inside `architecture` (parallel
`kicad-sourcing-agent` runs on disjoint part groups for a large BOM, their
handoffs named in the architecture brief); the library agent inside
`schematic`, before the build that needs the part; learn and memory
promotion (D7); **Codex**: no bundled agent is installed
(`install.rs:239-276`), so the session runs each phase itself with that
phase's skills and, as producer, calls `flow_advance` itself.

**`references/brief-template.md`**: orc's fields verbatim in name —
Objective, Context, Output Format, Tools Granted, Tools Blocked, Budget
(small / normal / research; return BLOCKED instead of grinding), Files
Scope, Success Criteria — with Konnect guidance: Context names `job_id`,
`project_dir`, the phase, the records and handoffs to `read`, and pastes
both memory tiers verbatim; Files Scope lists the design files the agent may
mutate and the records it must supply, never `STATE.md`; Tools Granted
includes `load_toolset("flow")` for a job; Success Criteria is the phase's
exit condition.

**`references/handoff-template.md`**: header `job_id`, `phase`, `role`,
`verdict` (`DONE` | `FIX` | `BLOCKED`), `failing_layer` (`requirement` |
`architecture` | `implementation`, required when `FIX`); sections Result,
Evidence (each tool call and its result), For the next agent, Deferred
findings, Questions (BLOCKED only). The agent persists it with
`flow_log(kind: handoff)` and returns the same text as its final message;
a job-less run (single-agent lane) only returns it. The photo agents keep
their five-field block (they run before the photo lane's job opens).

### D9. Agent roster and companion skills

The orchestrator is the main session running the `konnect` skill, not a
bundled agent. Eleven agents once this change and the merge land:

| Role | Agent file | Status | Triggers (abridged) | Anti-triggers | Tools | maxTurns | Skills (block list) | Records / writes |
|---|---|---|---|---|---|---|---|---|
| requirements | `kicad-requirements-agent.md` | new | "start a new board", "what do we need to know before designing X" | a bounded edit; requirements already recorded for this job | `mcp__konnect__*` | 60 | `konnect`, `kicad-architecture`, `kicad-schematic` | `constraints.md` |
| architecture | `kicad-architecture-agent.md` | new | "design the architecture for X", "block diagram and power budget" | no constraint record yet; a single bounded schematic edit | `mcp__konnect__*` | 150 | `konnect`, `kicad-architecture`, `kicad-schematic` | `architecture.md`, `worst-case.md`, `pin-plan.md` |
| sourcing | `kicad-sourcing-agent.md` | new | "check stock for these parts", "find a source for X", "confirm AVL/derating for this BOM" | placing or wiring a part | `mcp__konnect__*` | 150 | `konnect`, `kicad-architecture`, `kicad-manufacture`, `kicad-review` | no record; parts list in its handoff |
| schematic | `kicad-schematic-build-agent.md` | exists | unchanged | unchanged | unchanged | 200 | unchanged | `schematic-evidence.md` (new step) |
| library | `kicad-library-agent.md` | new | "make a symbol/footprint for X" | a search already returns a usable part | `mcp__konnect__*` | 100 | `konnect`, `kicad-library` | project libraries; handoff |
| layout | `kicad-pcb-layout-agent.md` | exists | unchanged | unchanged | unchanged | 300 | unchanged | `placement.md`, `routing.md` (new steps) |
| review | `kicad-design-review-agent.md` | exists | unchanged | unchanged | unchanged | 300 | unchanged | `ledger-schematic.md` / `ledger-prefab.md` (new step, single-reviewer mode) |
| manufacture | `kicad-manufacture-agent.md` | new | "prepare the fab package", "is this ready to send to JLCPCB" | the board has not passed `prefab_review` in this job | `mcp__konnect__*` | 150 | `konnect`, `kicad-manufacture` | `manufacturing.md` |
| photo-intake | `pcb-photo-intake-agent.md` | merged | unchanged | unchanged | `mcp__konnect__*`, `Read` | 40 | unchanged | review map `dossier` |
| design-reconstruction | `pcb-design-reconstruction-agent.md` | merged | unchanged | unchanged | `mcp__konnect__*` | 40 | unchanged | review map `design_brief` |
| curator | `kicad-curator-agent.md` | new | "close out this job and record lessons" (at `learn`) | any phase before `learn` | `mcp__konnect__*` | 60 | `konnect`, `kicad-curator` | lessons via `flow_log`; closes the job |

New agents copy the existing frontmatter shape exactly (`name`,
`description` with "Triggers:", `model: sonnet`, `skills:` and `tools:` as
block lists, `maxTurns`). Each flow step is conditional: "when the brief
names a `job_id`" — a job-less run calls no `flow_*` tool.

**Fix round 1 (reviewer 11 minor 4, DECISION E — library agent).**
`kicad-library-agent.md` Step 4 registers the created symbol/footprint
library with `project` scope in the **design** project. Step 6's
disposable placement happens in a **scratch** project created outside the
design project's directory. `resolve_footprint_path` (and the symbol
side, unverified but almost certainly the same) searches only the calling
project's own `fp-lib-table` plus the global table
(`library.rs:1635-1640`) — a `project`-scope registration in the design
project is invisible from the scratch project, so `place_component` there
fails "not found" today, and the agent is never told why or what to do
about it. Fix: Step 6 gains one instruction, before placing — register the
same library (the nickname and path Step 4 used) in the scratch project's
own table too, with `register_symbol_library`/`register_footprint_library`
(`scope: "project"`, `project: <scratch project path>`), for both the
symbol and the footprint library. This is the scratch board's own
one-time setup, not a change to Step 4's registration in the design
project.

**Fix round 1 (reviewer 11 minor 5 + developer 07 finding 2, DECISION F —
manufacture agent and skill vocabulary).** `kicad-manufacture-agent.md`'s
`Verdict` bullet today defines `READY` as "every check your tools can run
passed **and only the purchase-gate checks remain**" — a phase-local
loosening that lets `READY` mean something the `kicad-manufacture` skill's
own vocabulary (`SKILL.md:186`, "Any warning … keeps the result
`INCOMPLETE`"; `:267`, "Only `READY` permits upload") never allows. The
skill's vocabulary is unchanged by this fix round; the agent's prose is
brought back in line with it, and the phase gets a narrow, explicitly
named exception to leave through instead:
- `READY` means exactly what the skill says: every **artifact** check the
  agent's tools can run passed **with no warning** — scoped to the
  export's `warnings` array and the rest of the artifact acceptance gate
  (`SKILL.md:186`, "Any warning or missing requested artifact type keeps
  the result `INCOMPLETE`"). `INCOMPLETE` gains the skill's missing
  clause there: an artifact check that did not run, failed to execute,
  left an artifact missing, **or passed with a warning**.
- DRC and the preflight are adjudicated, not warning-gated: a DRC
  **error** blocks until it is resolved or covered by a deliberate,
  reviewable waiver (`SKILL.md:80-81`, "resolve every error or record a
  deliberate, reviewable waiver"; DECISION I as amended); a DRC run that
  did not complete (for example parity not checked) is an open item, never
  adjudicated away; a DRC **warning** is
  adjudicated — fixed, or accepted with its reason recorded in
  `manufacturing.md`'s Design evidence section — and an adjudicated
  warning is not an open item. A `validate_for_manufacturing` (preflight)
  issue may be adjudicated the same way (`SKILL.md:97`, "an unadjudicated
  issue blocks release"); an adjudicated one is not an open item either.
- The phase may still exit to `gate:purchase` — but now on `READY`, or on
  `INCOMPLETE` whose only open items are exactly the three entries under
  `## Checks at the purchase gate` (the Gerber/drill viewer, the
  fabricator's order preview, the live stock/price re-check) named and
  nothing else. Any other `INCOMPLETE`, or `NOT READY`, is still not an
  exit.
- The purchase approval already requires those three items discharged
  before `flow_gate` for `purchase` (task 3.4's orchestration.md §4 text,
  unchanged by this fix round: the session shows `## Checks at the
  purchase gate`, runs or asks for each, and records the result with
  `flow_log` before asking for the decision).
- `orchestration.md` §2's `manufacturing` row ("an `INCOMPLETE` package is
  not an exit") gains this one, named exception, additive to the existing
  sentence.
- "Only `READY` permits upload" (the skill's own upload gate, `SKILL.md`)
  is untouched: the exception is scoped to *leaving the phase into the
  gate*, not to what happens after the gate — the purchase gate approval
  is what authorizes the order, and `READY` is still what the skill
  requires before anything is actually sent to a fabricator. **Fix round
  2 (DECISION J):** nobody was assigned to say so. After `flow_gate`
  approves `purchase` — its `user_words` confirming the three
  purchase-gate checks were discharged — the orchestrating session, not
  this agent, records the package as `READY` in the skill's sense with
  `flow_log(kind: evidence)` naming that approval; placing or uploading
  the order is the user's own action, and no agent, this one included,
  ever uploads.

**Fix round 2 (reviewer 16 SERIOUS 1, DECISION I — this Fix round 1 note
was itself too wide).** The bullets above originally read "every check
the agent's tools can run passed with no warning"; the log's own
DECISION F said "every **artifact** check", and the wider wording made a
warnings-only board unable to leave `manufacturing`. Reproduction: KiCad's
own `ecc83-pp` demo (`kicad-cli 10.0.2 pcb drc --severity-all`, repo
fixture `crates/konnect-sexp/tests/fixtures/ecc83-pp.kicad_pcb`) has 0 DRC
errors and 17 DRC warnings; under the wide wording every one of them
forced `INCOMPLETE` with no waiver path, so the phase could never exit.
DECISION I restores the log's scope and adds the DRC/preflight
adjudication rule the skill already states (`SKILL.md:80-81`, `:97`) but
the agent's prose never carried — the bullets above are the corrected
text. `kicad-manufacture-agent.md`'s own Verdict bullet, its "Ending the
run" section (DECISION J's pointer above), and QA's guard
`manufacture_agent_verdicts_match_the_skill`
(`crates/konnect/tests/asset_references.rs:2068`) change together in
tasks.md §9: the guard's pinned marker text changes with the prose it
pins, so this is a literal edit to the guard, not an additive assertion
the way round 1's guards were.

**Companion skills — YAGNI verdicts** (the planner proposed four):
- **`kicad-requirements` — merged into `kicad-architecture`.** Phases 1–2
  are one method split across two records; the architecture agent must read
  the constraint schema anyway to consume it, and requirements' own content
  (ask once, decide the obvious and log it, fields to capture) is a short
  section. The doc sources requirements' method from `kicad-schematic`'s
  `design-calculations.md`/`interface-design.md`, which both agents preload.
- **`kicad-architecture` — kept (new).** The method is today five lines
  inside `kicad-schematic` ("Architecture checkpoint"); the record schemas,
  the readiness rule, the parts-that-can-be-bought procedure and the "ask
  once" discipline exist nowhere, and a skill is the only unit Codex
  receives. `SKILL.md` + `references/constraint-record-schema.md` +
  `references/architecture-record-schema.md` (architecture, worst-case —
  reusing `design-calculations.md` §1's record fields by reference —
  pin plan, and the parts-list section sourcing fills).
- **`kicad-sourcing` — cut.** Its method already exists: `kicad-manufacture`
  §2b BOM integrity and `jlcpcb-rules.md`, `kicad-review`'s
  `datasheet-audit.md` and the BOM/procurement dimension of
  `review-orchestration.md`, `kicad-schematic`'s "Parts that can be
  bought"; the parts-list shape lives in `architecture-record-schema.md`
  because the doc puts sourcing inside phase 2 ("arquitetura + pesquisa").
  The agent file sequences those; it preloads the three skills.
- **`kicad-curator` — kept, small, no reference file.** The triage rule and
  candidate format need one home that the curator agent preloads and the
  session reads under Codex; no existing skill has it, and the agent file
  alone is not installed for Codex.
- `kicad-library` and `kicad-manufacture` are reused unchanged; the new
  agents are their missing owners.

Every rule a new agent must follow to produce a passing record is in a
preloaded body (the agent file or a skill's `SKILL.md`), with references
holding field detail and examples: whether a sub-agent limited to
`mcp__konnect__*` can open a preloaded skill's `references/` is unverified
(existing agents already assume it — see the handoff's deferred findings),
so nothing new depends on it.

*Rejected:* one shared `kicad-orchestrator-support` skill (four unrelated
topics behind one name, none discoverable); keeping all four (two would be
thin copies of existing content that then drift).

### D10. Routing and the asset guards, remedies pre-written

1. `top_level_skill_routes_every_bundled_agent` (`asset_references.rs:128`)
   substring-matches every agent file stem in `konnect/SKILL.md`: the router
   edit names all six new stems; the two photo stems are already there.
2. `backticked_tool_names_in_prose_exist_in_the_registry` (`:746`) flags
   every lowercase two-part snake word, backticked or bare (`:1013`), unless
   it is a tool, a toolset, a **top-level** input property of any tool, or
   in `NOT_TOOLS` (`:773`). Registering `flow` exempts its parameters
   (`job_id`, `to_phase`, `gate_name`, `user_words`, `evidence_calls`, …).
   Expected additions, in one commented block — add only names the test
   actually flags: lane values `new_board`, `board_revision`,
   `review_only`, `fab_only`, `photo_to_kicad`; phase tokens
   `schematic_review`, `prefab_review`; kinds `queue_item`,
   `pending_approval`; response fields `design_hash`, `design_files`,
   `lock_files`, `gate_approvals`, `pending_approvals`, `deferred_findings`,
   `fix_rounds`, `last_transition`, `next_step`, `design_hash_at_approval`,
   `package_hash_at_approval`, `approved_by`, `evidence_check`,
   `state_error`; `failing_layer`; `design_state_hash`.
3. `agents_make_claimed_evidence_executable` (`:175`) is a hard-coded case
   list — the one guard that catches "told to call `flow_advance`, never
   loads `flow`". Enroll: add `flow` to the required toolsets and
   `flow_advance` to the markers of the schematic, layout and review
   tuples, and add tuples for the requirements, architecture, manufacture
   and curator agents (`flow` + `flow_advance`).
4. `call_examples_name_real_parameters` (`:598`): D1's required columns are
   what every signature example must list.
5. **New guard**, `crates/konnect/src/install.rs` tests: every
   `assets/skills/*/SKILL.md`, every `assets/skills/*/references/*.md` and
   every `assets/agents/*.md` is registered in `SKILLS`/`AGENTS`
   (`manifest.rs:33`, `:188`). Nothing checks this today, the asset tests
   read `assets/` directly, and this change adds 13 files to the manifest.
6. `every_reference_is_reachable_from_its_parent_skill` (`:45`): each new
   reference file is named in its parent `SKILL.md`.

### D11. What a gate binds to; `design_state_hash` cost

A gate binds to two content keys, both compared by equality:
- `design_hash` — `design_state_hash(project_dir)`: every `.kicad_pro`,
  `.kicad_sch`, `.kicad_pcb`, `.kicad_dru` under the project, LF-normalized,
  sorted, framed.
- `package_hash` — the same framing over the job's package records: the
  records required (D4) of every phase in the job's sequence after the
  previous gate (or the start) and before this gate. For `new_board`:
  architecture ← `constraints.md`, `architecture.md`, `worst-case.md`,
  `pin-plan.md`; placement ← `schematic-evidence.md`, `ledger-schematic.md`,
  `placement.md`; purchase ← `routing.md`, `ledger-prefab.md`,
  `manufacturing.md`. Derived from the sequence, so every lane gets the
  right package with no per-gate code. A missing file is framed as absent,
  so deleting a record changes the key. Needed because `design_state_hash`
  excludes `.konnect/` by construction (`design_hash.rs:63`): at the
  architecture gate the reviewed content is records only, and the design
  hash alone would let an edited architecture keep its approval.

Two checks: at **approval**, both keys must equal the values recorded when
the job entered the gate phase (the human approved what was produced and
shown, not whatever the files became meanwhile); at **leaving the gate**,
both must equal the approval's (nothing changed between approval and
continuation — including across days and sessions). The approval also
stores `visit` (the history index of the entry into the gate phase);
leaving requires the approval of the current visit, so a rewind and return
always re-asks.

**Fix round 1 (reviewer 11 minor 2, DECISION D).** These two checks (at
approval and at leaving the gate) are unchanged — they still compare
against the entering visit's keys. What changes is only how `flow_status`
*reports* an approval it already granted: `gate_validity` (D1's
`flow_status` row) recomputes `valid` against the *current* design and
package only for the gate whose token equals `state.phase` right now — the
gate the job is still standing at. Every other approval (the job has moved
on) reports `status: passed` with the hashes recorded at approval, and no
`valid` field: recomputing `valid` there was always going to read `false`
the moment normal work continued past the gate (a later schematic save
flips the architecture approval), which read as "something is wrong" when
nothing was. DECISION B guarantees an approval can only be `passed` or
cleared, never regain `current`, since a rewind can no longer target a
gate.

**Cost, measured** (a Python probe of the identical walk-and-hash; the
Rust loop is I/O-bound and not slower): `Load-Cell-Digitizer/kicad` — 21
files, 2.2 MB → 9–16 ms; `display-remoto` (the doc's 344-part, 11-sheet
reference) — 53 files, 23.9 MB → 69–180 ms cold. At most one hash per call
(`flow_status`, `flow_advance`, `flow_gate`). **No cache**: a cache needs a
key, and an mtime/size key is the classic stale-key bug; the content hash is
the point.

**Semantics, measured on the same projects:** the walk includes
`.history/` (an editor's local history) and `backup-YYYYMMDD-*`/
`_backup_pre_rebuild/` copies — 39 of display-remoto's 53 covered files
(12 under `.history/`, 27 in two `backup-*` folders) are not the live
design. A new backup copy therefore flips the hash: a false "stale" costs
a re-approval — **Fix round 2 (reviewer 16 minor 4, DECISION K):** since
DECISION B (D3) requires rewinding to the producing phase rather than
straight back into the gate, the real cost is a re-run of the producing
agent plus a re-approval, not a re-approval alone; never a false pass.
Accepted; `design_hash.rs` is
not changed here (it is a shared identity; narrowing it is its own
decision). KiCad's own locks are `~<file>.lck` beside the design files,
including `~<name>.kicad_pro.lck` (present in display-remoto) — so
`lock_files` checks `~<name>.lck` next to **every** covered file, not
`kicad_editor_lock_path` (`writer.rs:261`), which ignores `.kicad_pro`.

### D12. Documentation counts

At the merged base: toolsets 22, registered tools 232, plus 7 meta. After
this change: toolsets 23, registered 238, with meta 245, categories 11.
Re-derive from the test at implementation time; never copy these numbers
over a newer registry. Places the sweeps will name: `README.md:16`, `:73`;
`DEV.md:78`, `:331`, `:416`, `:421`; `tool-directory.md:15-16` (its
category count too — not swept, edit it by hand), `:31`, plus a new
`### \`flow\` · 6 tools` section (`tool_directory_section_headings_
match_the_registry` checks the heading); `docs/TROUBLESHOOTING.md:369`;
`packaging/metadata.json:4-5`; `plugin/plugin.json:4`; and
`openspec/changes/archive/2026-09-18-photo-to-kicad-reverse/tasks.md:123`,
a historical record the toolset sweep will flag. Historical records answer
to their own commit (the reason `.claude` is already skipped,
`doc_tool_counts.rs:165`), so `.orchestrator` and `archive` join `SKIP`
rather than rewriting history; the main checkout's untracked
`.orchestrator/` handoffs quote the old counts and would otherwise fail the
suite there after the merge. This change's own planning files never put a
digit directly before "tools" or "toolset".

## Implementation order

1. **flow core, TDD, one file** (tasks 1.1–1.7, sequential — all edit
   `flow.rs`): state + validators + parsers first, then `flow_status`, then
   each mutating tool with unit tests calling handlers directly.
2. **Registry, then the published-contract test** (1.8, 1.9): register,
   then drive the whole approval chain through the compiled schemas via
   `ToolRouter` (the `photo_intake_gate_e2e.rs` harness).
3. **Counts** (1.10) right after registration: the doc tests go red the
   moment `flow` registers.
4. **Companion skills** (2.1–2.3, parallel: separate files).
5. **`konnect` skill** (3.1–3.3): references before the router; the router
   task alone edits `SKILL.md`.
6. **New agents** (4.1–4.6, parallel) after skills exist
   (`agents_preload_existing_skills`).
7. **Existing agents' flow steps** (5.1–5.3, parallel).
8. **Wiring and guards** (6.1–6.4): manifest + completeness test, then
   `NOT_TOOLS`, then enrollment, then the full asset suite. Earlier asset
   tasks run targeted tests only; the phantom-name test cannot pass before
   6.2.
9. **Final gates** (7.1–7.3).

## Pre-mortem

Assume it shipped and failed. Each cause names what prevents it.

1. **An approval covers a design the user never saw** — the session showed
   placement images, the board changed, the approval stamped the new hash.
   → D11 approval-time check against the gate-entry keys; task 1.6.
2. **A changed architecture keeps its approval** because `design_state_hash`
   excludes `.konnect/`. → D11 `package_hash`; tasks 1.1, 1.6, 1.9.
3. **A stale record passes a phase exit** (the previous board's
   `architecture.md`, or the pre-FIX evidence). → D4 same-call rule; task
   1.4's acceptance feeds a stale record on disk and expects refusal.
4. **The FIX loop or an abandoned job has no state**, bricking `flow_start`
   until someone hand-deletes `STATE.md`. → D3 rewind/abandon/close; task
   1.5.
5. **A lane subset silently drops a human gate.** → D3 gated-phase rule;
   task 1.1.
6. **Concurrent calls lose or corrupt `STATE.md`** (parallel reviewers
   logging, a Claude and a Codex session). → D2 `transact_atomic` +
   `write_new_atomic`; task 1.7 runs two concurrent `flow_defer` calls and
   expects both entries; task 1.3 covers the double `flow_start`.
7. **A tool is uncallable through its published schema** (the
   `save_photo_review_map` bug) while unit tests pass. → D1 schema rules;
   task 1.9 validates every call against the compiled schema before
   dispatch.
8. **Evidence checks misfire** — the 100-call ring evicts honest evidence
   (FIX loops) or the check is skipped. → D6 check inside `flow_advance`;
   task 1.5; guidance in task 3.1.
9. **Agents cannot read their inputs** (mcp-only tools, frozen frontmatter)
   and rebuild requirements from the chat. → D1 `flow_status(read)`; tasks
   1.2, 4.x, 5.x.
10. **An agent is told to call `flow_advance` but never loads `flow`**, or a
    new asset is never installed. → D10 items 3 and 5; tasks 6.3, 6.1.
11. **The suite goes red on counts** — stale numbers in docs, in archived
    changes, in `.orchestrator/`, or in this change's own files. → D12;
    task 1.10; this design writes no such phrase.
12. **Under Codex the router delegates to agents that do not exist.** → D8
    Codex rule; task 3.1.
13. **A hand-edited `STATE.md` is silently "repaired" or lost.** → D2:
    body regenerated and declared so; front matter refused with
    `state_error`/`conflict`, never guessed; task 1.2.
14. **Accepted, not prevented:** a session can type invented `user_words`
    (no tool can tell — D8 requires the verbatim quote); a new backup copy
    forces a re-approval (D11, fail-safe); the evidence ring cannot tell
    which agent ran a call (D6).
