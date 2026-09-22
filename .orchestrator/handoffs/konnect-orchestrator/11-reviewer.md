---
change: konnect-orchestrator
task: adversarial review of bbd5efb..84f7e0f (30 commits, 33 files)
agent: reviewer
verdict: FIX
failing_layer: spec
---

## Result

**FIX. One SERIOUS finding (spec layer), six MINOR.** The tool core holds up against the attacks in the brief. Locking, path confinement, approval binding within a well-formed lane, schemas and the manifest all passed. The one hole is a lane shape that the D3 validator accepts, where the purchase (or architecture) approval does not bind the record the user is shown. The implementation matches D3 exactly, so the fix starts in the design and spec.

1. **SERIOUS — spec (D3 validator rule set; `flow-toolset/spec.md:73-76`).** A gate is accepted without the phase that produces its package. `validate_phases` enforces only phase ⇒ gate (`crates/konnect-core/src/tools/flow.rs:450-457`). `package_records` covers only the phases in the job between the previous gate and this one (`flow.rs:865-877`). `records/` persists across jobs (D2).
   - **Scenario A (reproduced, P1).** Job A (`fab_only`) records `manufacturing.md` ("REV A … cost USD 100") and closes. The board changes. Job B opens as `board_revision`, `phases: [routing, prefab_review, gate:purchase]`, and the validator accepts it. The gate entry's `package_files` is `[ledger-prefab.md, routing.md]`. `flow_status(read: manufacturing.md)` serves rev A's record, and `orchestration.md` §4 tells the session to show exactly that record at the purchase gate. `flow_gate(purchase, approve, user_words: "ok, compra")` is recorded. The record is then overwritten, `valid` stays `true`, and `flow_advance(closed)` is accepted. The user authorized the rev B order while looking at rev A's BOM, cost and checks, and the approval binds none of it.
   - **Scenario B (reproduced, P1).** `[requirements, gate:architecture, schematic]` is accepted. `check_approval` reads the readiness line from whatever `records/architecture.md` is on disk (`flow.rs:2444-2472`). A previous job's `Readiness: PASS` approves the gate. The file is outside the package, so after it is rewritten to `Readiness: BLOCKED` the job still leaves the gate.
   - **Why it matters.** It breaks the intent in `proposal.md:49-52` ("bound to … a hash of the records the user was shown") and pre-mortem #1/#3. `orchestration.md:41` ("never drops a human gate") does not forbid it either. A re-route ECO that treats `manufacturing` as "not affected" is exactly the case the lane table invites ("only the affected phases").
   - **Fix.** Add the converse rule — `gate:architecture ⇒ architecture`, `gate:placement ⇒ placement`, `gate:purchase ⇒ manufacturing` — in:
     - D3 (`design.md:276`);
     - `flow-toolset/spec.md:73-76`, plus a scenario;
     - `orchestration.md` §1;
     - `validate_phases`, plus a unit test naming the entry.

     Every paired phase sits directly before its gate, so the pairing guarantees that each gate's package contains its own phase's record, and D4 forces that record to be re-supplied in this job. No documented lane breaks: new_board, the board_revision example, fab_only, photo_to_kicad and review_only all satisfy it.

2. **MINOR — implementation. `flow_gate` and every transition write their side files before the `STATE.md` commit point.**
   - `apply_gate` writes `records/gates/<gate>.md` and appends the log (`flow.rs:2348-2384`). `commit_transition` appends the log (`flow.rs:2235`). Both happen before `transact_atomic` renames `STATE.md` (`konnect-sexp/src/writer.rs:183`).
   - **Reproduced (P2).** Purchase approved. A reader holds `STATE.md` without `FILE_SHARE_DELETE` (CPython `open`; an editor, indexer or AV behaves the same). `flow_gate(reject, "nao, cancela")` returns an untyped `Could not update …STATE.md (Access is denied. (os error 5))`. But `gates/purchase.md` reads `# Gate purchase — reject`, the log holds `gate purchase reject`, `STATE.md` still holds the approval (`valid: true`), and `flow_advance(closed)` leaves the purchase gate.
   - D2's "harmless, because D4 requires records to be re-supplied" covers records only. The log is a closed job's permanent history (D3), and it can now record decisions and transitions that never happened; a retry duplicates them.
   - **Fix.** Make `STATE.md` the commit point: write the gate file and the log after the state write succeeds, still under the lock.

3. **MINOR — spec/guidance. `valid` misleads for gates the job has already passed.**
   - `flow_status` computes `valid` by comparing the approval with the current design hash (`flow.rs:1216-1219`). The architecture approval therefore turns `false` at the first schematic save, and the placement approval at the first route — the normal state of any passed gate.
   - `orchestration.md:214` says only "`valid: false` — the design or the package changed since that approval". A resuming session can read that as a reason to re-ask or rewind.
   - **Fix.** Add one sentence: `valid` matters only while `phase == gate:<name>`. Alternatively, report a `current` flag.

4. **MINOR — spec. A rewind into a gate phase binds the gate to the design as it is at rewind time.**
   - `move_back` accepts a rewind whose target is a gate and records that gate's keys from the current design (`flow.rs:2137` via `record_gate_keys`).
   - **Scenario.** At `routing`, rewind to `gate:placement`. The approval now binds a routed board, while the package the session shows (`placement.md` and its images) predates the routing.
   - No documented path rewinds to a gate: §5 sends every FIX to a producer.
   - **Fix.** Refuse a gate as a rewind target, or have §4 re-render before asking.

5. **MINOR — implementation (asset). The library agent's disposable placement cannot resolve the new library.**
   - `crates/konnect/assets/agents/kicad-library-agent.md:111`, Step 6. `place_component` does **not** need a live IPC session. It is `LivePreferredWithFallback` (`pcb_components.rs:1844`), and a never-opened scratch board takes the guarded file path when KiCad is unreachable or answers "not open" (`pcb_board.rs:289-299`, `:304-316`).
   - The real obstacle is resolution. `resolve_footprint_path` searches only `<scratch dir>/fp-lib-table` plus the global table (`library.rs:1635-1640`, called from `pcb_components.rs:65`). The library Step 4 registers with `project` scope in the *design* project is invisible to a scratch project outside it, so Step 6 fails with "not found" unless the agent also registers it in the scratch project, and the agent is never told to.
   - The symbol side very likely has the same problem; I did not verify it.
   - **Fix.** One sentence in Step 6.

6. **MINOR — implementation (asset). The manufacture agent's verdict words drift from the skill's.**
   - `kicad-manufacture-agent.md:174-178` makes `READY` mean "every check your tools can run passed and only the purchase-gate checks remain".
   - `kicad-manufacture/SKILL.md:267` still says "Only `READY` permits upload", and `SKILL.md:186` ("Any warning … keeps the result `INCOMPLETE`") is missing from the agent's `INCOMPLETE` definition.
   - **Scenario.** A job-less single-agent run ("is this ready to send to JLCPCB") returns `READY` with the viewer, preview and live-stock checks undone and no gate to discharge them. Separately, an export with warnings but all files present can come out `READY`.
   - **Fix.** Add warnings to `INCOMPLETE`, and say in the job-less path that `READY` here does not permit upload until the three checks are done.

7. **MINOR — implementation (test). The flow-loading guard is a hard-coded list, not the promise the spec states.**
   - `agents_make_claimed_evidence_executable` lists seven agents by hand (`crates/konnect/tests/asset_references.rs:177-252`).
   - `agent-roster/spec.md:23-26` (and commit `4cdb673`'s title) claim it checks "every agent that prescribes `flow_advance`". A new agent told to advance without loading `flow` escapes.
   - The 3.5 confirmed-row rule (`located, not validated`, the evidence entry) has no marker at all (developer 09, deferred #1).
   - **Fix.** Derive the case set from `assets/agents/`: any file containing `flow_advance(` must load `flow`. Add the confirmed-row marker to the architecture agent/skill pair.

## Evidence

**Tests (worktree at `84f7e0f`, target shared).**
- `cargo test -p konnect-core --lib flow`: 58 passed. This includes `racing_starts_open_exactly_one_job`, `two_concurrent_defers_both_land` and `an_approval_after_a_rewind_into_the_gate_binds_to_the_rewind_entry`.
- `cargo test -p konnect-core --test flow_gate_e2e`: 3 passed.
- `cargo test -p konnect`: every suite green (`asset_references` 13, `doc_tool_counts` 6, `not_tools_allowlist` 3, `konnect` main 58 including `manifest_ships_every_asset`, `protocol_stdio` 18, …).
- The `konnect` doctest aborted with "extern location for konnect_core does not exist". That was a concurrent `cargo test -p konnect-core` from another process racing the same `target/`, not the change.

**Probes.** Python MCP stdio client against a copy of the freshly built `target/debug/konnect.exe`. Everything is outside the repo, in the session scratchpad `probe/`, with `KONNECT_STATE_DIR`/`APPDATA` pointed at temp dirs.
- **P1** (`p1_gate_without_phase.py`), finding 1. Output lines: `flow_start ['routing', 'prefab_review', 'gate:purchase'] -> accepted`; `gate:purchase entry package_files: ['ledger-prefab.md', 'routing.md']`; `manufacturing.md served … '# REV A package\nBOM rev A, cost USD 100…'`; `purchase approve -> recorded`; `purchase valid after manufacturing.md changed: True`; `leave gate:purchase -> accepted (close)`; `flow_start [requirements, gate:architecture, schematic] -> accepted`; `architecture approve against the stale on-disk architecture.md -> recorded`; `leave gate:architecture after architecture.md went BLOCKED -> accepted`.
- **P2** (`p2_reject_window.py`), finding 2. Output as quoted in finding 2.
- **P3** (`p3_two_processes.py`). Two server processes × 25 `flow_defer` and 25 `flow_log(handoff)` each, concurrently: `errors: 0`, `deferred: 50 unique: 50`, `handoffs: 50 unique: 50`. The cross-process lock keyed on the canonical parent (`writer.rs:315`) serializes as D2 claims.
- **P4** (`p4_parse_and_paths.py`). A CRLF+BOM `STATE.md` is accepted. An unknown front-matter field gives `state_error` in `flow_status`, a typed `conflict` on `flow_defer`, and leaves the file untouched. Ten hostile `read` names (`../STATE.md`, `gates/../../board.kicad_pcb`, `memory/../STATE.md`, `handoffs/01-../../x.md`, `handoffs/01-review.md/..`, `C:/Windows/win.ini`, `log/../STATE.md`, …) all return `invalid_argument`; `memory/photo-intake.md` → `missing`.

**Attack-surface coverage.**
1. **Locking, atomicity, parsing, Windows — covered.**
   - Every mutator parses and validates `current` inside the `transact_atomic` closure (`transact_state` `flow.rs:1810-1832`; `write_first_state` `:1539-1578`). `flow_log` returns `current` unchanged but runs under the lock, so handoff numbering is race-free (P3).
   - Crash consistency → finding 2.
   - Parsing: P4.
   - Windows: no verbatim-prefix stripping anywhere in `flow.rs` (`\\?\` is echoed and re-canonicalizes); `design_files` use `/`, so the `~<name>.lck` beside nested files is right (`flow.rs:1190-1206`).
2. **Approval binding — covered.**
   - Approval is compared with the gate-*entry* keys (`flow.rs:2474-2512`).
   - Exit is compared with the approval keys plus the current visit (`:1954-2005`).
   - Records are written before `record_gate_keys` hashes them (`:2043-2069`).
   - Rewinds clear every gate at or after the target (`:2119-2130`); abandon clears nothing, and is terminal.
   - D4's same-call rule holds (`:1899-1950`).
   - Developer 04 #2 (`valid` ignores the visit) is reachable only by a hand edit → Deferred.
   - Gap → finding 1.
3. **Path confinement — covered.** Read names (P4). Record filenames come from an enum. `job_id` is validated at parse (`:540-553`) and compared before any path is built. Handoff `role` comes from an enum. Subdirectories are re-canonicalized and confined (`:802-815`). A record write renames over a planted link rather than following it. `project_dir` must hold a top-level `*.kicad_pro`.
4. **The three user gates — covered.**
   - Phase ⇒ gate holds.
   - Purchase needs non-blank `user_words` in every mode (`:2428-2443`).
   - Autonomous mode is fixed at start; no tool changes it.
   - Reject removes the approval.
   - Abandon closes without purchasing, and nothing in `flow` orders anything.
   - Hole → finding 1.
5. **Schemas and errors — covered.**
   - Every object node declares `properties`, and `records.items` is closed; the e2e rejects an extra key (`flow_gate_e2e.rs:306`).
   - The e2e validates each call against the compiled schema of the `ToolRouter`-loaded defs before dispatch (`flow_gate_e2e.rs:24-48`).
   - Typed `conflict`, `stale_target` and `invalid_argument` all observed. The `STATE.md` write failure is untyped, which is acceptable.
6. **Assets — covered.**
   - No agent reads a flow path directly; everything goes through `flow_status(read)`.
   - `flow_gate` and `flow_start` appear in agents only as "never call".
   - The sourcing and library agents never advance (`kicad-sourcing-agent.md:57,138`; `kicad-library-agent.md:66,133`).
   - Layout placement hard stop: the new Phase 3 bullets plus hard rule 11.
   - The readiness / confirmed-row rule is consistent across `kicad-architecture-agent.md:122-162`, `kicad-architecture/SKILL.md:175-184` and `architecture-record-schema.md:79-118`.
   - `orchestration.md` §4 vs "INCOMPLETE is not an exit" is consistent via `## Checks at the purchase gate`; vocabulary drift → finding 6.
   - `place_component` → finding 5.
   - The `konnect/SKILL.md` router change is purely additive (+54/-0): The One Rule and the scripted fallback are untouched.
7. **Manifest and install — covered.**
   - The asset tree has 12 skills and 11 agents, and all 13 new files are in `manifest.rs`.
   - `manifest_ships_every_asset` walks `assets/` with a stack and fails on stray, duplicate, unshipped, phantom or mismatched entries (`install.rs:754-880`).
   - `reliability-contract.md` is installed from `docs/` (`GENERATED_REFERENCES`); developer 06's "file does not exist" is a false alarm.
   - Codex still gets skills only: `install.rs` changed in tests only.

## For the next agent

- **Route finding 1 to the architect first (spec layer).** Add the converse pairing rule to D3, `flow-toolset/spec.md` (requirement text plus a scenario "a gate without its phase is rejected") and `orchestration.md` §1. Then a developer adds the rule to `validate_phases` with a unit test, and extends the e2e or unit tests with P1's two shapes.
- Findings 2–7 are independent, small edits the developer can take in the same round: `flow.rs` ordering; `orchestration.md:214`; the `move_back` gate-target rule; library Step 6; the manufacture verdict text; the guard in `asset_references.rs`.
- Re-run P1 and P2 from the scratchpad `probe/` against a rebuilt binary to confirm. `mcp.py` spawns `konnect-flow.exe`, so copy the new `target/debug/konnect.exe` over it first.
- While this review ran, another agent worked in the same worktree: HEAD moved to `4ab71ba` ("guard the parts-list confirmation rule and the orchestration prose"), and an uncommitted `load_toolset("flo")` mutation appeared in `kicad-design-review-agent.md`. Neither is mine or in the reviewed range. This verdict covers `bbd5efb..84f7e0f` only; `4ab71ba` may partly address finding 7.

## Deferred findings

1. `flow_status` `valid` ignores the visit (developer 04 #2). Reachable only by a hand edit, because a rewind clears every approval at or after its target and a gate is re-entered only after such a rewind.
2. `doc_tool_counts.rs` `SKIP` now matches any directory *named* `archive` or `.orchestrator` anywhere in the repo (name-based, `doc_tool_counts.rs:187`). Today only `openspec/changes/archive` exists. This change caused it, but it is cosmetic.
3. The log, memory and candidates appends (`append_file`, `flow.rs:1327-1335`) follow a symlink planted at the destination name. There is no caller-input path to that — it needs local write access, which already allows direct edits.
4. `records/gates/<gate>.md` persists across jobs, so a new job's `flow_status(read: gates/purchase.md)` returns the previous job's decision. The file names its job; it is only a readability issue.
5. The router's single-agent lane and the Agent Routing bullets name bundled agents that a Codex install does not receive. This is the pre-existing pattern; `orchestration.md` §12 covers jobs only.
