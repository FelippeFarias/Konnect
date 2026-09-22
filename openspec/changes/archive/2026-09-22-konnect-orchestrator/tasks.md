Implementation branches from `main` **after** `orc/board-dossier-reconstruction`
(1653e79) merges; `git merge-tree --write-tree ee55a12 1653e79` is clean
(tree `a4351038`). All paths are relative to the repo root at that merged
base. Truth commands: `cargo test -p konnect`, `cargo test -p konnect-core`
(with `PROTOC=C:/Users/felip/tools/protoc/bin/protoc.exe` and the VS 2022
BuildTools CMake bin on `PATH`); lint gate: `cargo fmt --check`,
`cargo clippy --all-targets`. `stacks: none` for this project — every task
is `Stack: none`. Order and rationale: design.md "Implementation order".
Round 1 tasks all edit `flow.rs` and run sequentially. Never write a digit
directly before the word "toolset", or a three-digit number directly before
"tools", in any `.md`/`.json` (the `doc_tool_counts` sweeps read this file).

## 1. `flow` toolset

- [x] 1.1 Add `crates/konnect-core/src/tools/flow.rs` foundations (design D2, D3, D5, D11): the 12 canonical tokens plus `closed`; `Lane` and `Mode` enums; the D3 sequence validator (non-empty, known tokens, strictly increasing, a gate never first, a gated phase brings its gate); `JobState`/`HistoryEntry`/`GateApproval`/`DeferredItem` with `deny_unknown_fields` and `skip_serializing_if`; `STATE.md` render/parse (pretty JSON front matter between `---` fences, CRLF-tolerant, regenerated body); `project_dir` resolution (canonical, a top-level `*.kicad_pro`, confined flow directory); the D5 readiness parser; the D11 package-record derivation and `package_hash`. Add `pub mod flow;` to `tools/mod.rs`; widen `now_rfc3339_utc` in `tools/photo_intake.rs` to `pub(crate)` (that file's only change).
Stack: none
Acceptance: `cargo test -p konnect-core flow::` passes a round-trip test whose objective contains `---`, a double quote, a newline, `:`, `ç` and a backtick; the validator test rejects an out-of-order, a repeated, an unknown, a gate-first and a gate-less `[placement, routing]` sequence, each error naming the entry; the readiness test accepts only a trimmed last line `Readiness: PASS`, classifies `Readiness: BLOCKED — …` as blocked and a missing line as malformed.

- [x] 1.2 Implement `flow_status(project_dir, read?)` per design D1/D4/D11: job state; `design_hash` and `design_files` from `design_state_hash`; `lock_files` = every `~<name>.lck` beside a covered file; `gate_approvals` with a recomputed `valid`; `fix_rounds`; `last_transition`; `handoffs`; `next_step`; `contents`/`missing` for D4's readable names. Creates nothing and never refuses for a flow-state reason.
Stack: none
Acceptance: a project with no `.konnect/flow/` returns `job: null` without error and the directory still does not exist afterwards; a project holding `~demo.kicad_pro.lck` and `~demo.kicad_pcb.lck` lists both in `lock_files`; a `STATE.md` with broken JSON yields a success result with `state_error` set, while `read: ["../x"]` is an `invalid_argument` error.

- [x] 1.3 Implement `flow_start(project_dir, objective, lane, phases?, mode?)` per design D1/D2/D3: the `job_id` slug rule; the first `STATE.md` via `konnect_sexp::write_new_atomic`, later ones via `transact_atomic`, all inside `spawn_blocking`; a `start` history entry and log entry.
Stack: none
Acceptance: a second `flow_start` while a job is active returns `conflict` and leaves `STATE.md` byte-identical; `phases` omitted for `fab_only`, or an invalid `phases`, is refused and no `.konnect/flow/` directory is created; after the job is `closed` a new `flow_start` succeeds, and a directory without a top-level `*.kicad_pro` is refused.

- [x] 1.4 Implement `flow_advance` forward transitions per design D1/D4/D5: the phase's records must be in this call's `records` and belong to the phase being left; leaving `architecture` requires D5 `PASS`; records via `write_atomic`, then the history entry (`design_hash`, plus `package_hash` when entering a gate), `STATE.md` and the log entry, all under the `STATE.md` lock.
Stack: none
Acceptance: advancing out of `requirements` with `constraints.md` in `records` writes the file and moves the phase; advancing out of `architecture` while an `architecture.md` from an earlier write sits on disk but is absent from `records` is refused, with nothing written and the phase unchanged; a skip-ahead `to_phase`, and a record belonging to another phase, are each refused.

- [x] 1.5 Implement `flow_advance` rewind, abandon and close per design D3, and the D6 `evidence_check` (computed from `ctx.observer.recent(0)`, stored in the history entry, never a refusal).
Stack: none
Acceptance: a rewind from `schematic_review` to `schematic` without `reason` is refused, and with `reason` it moves the phase, clears later gate approvals and makes `flow_status` report `fix_rounds.schematic_review` = 1; `closed` from a middle phase needs `reason` and ends the job; with the observer holding an `ok` record for `run_erc`, `evidence_calls: ["run_erc", "render_schematic_png"]` yields `confirmed: ["run_erc"]` and `absent: ["render_schematic_png"]`.

- [x] 1.6 Implement `flow_gate` per design D1/D3/D11: allowed only at `gate:<gate_name>`; `approve` checks D5 for `architecture`, the `user_words` rule by mode and gate, and both keys against the history entry that entered the gate; records the approval (with `visit`) in `STATE.md` and writes `records/gates/<gate>.md`; `reject` removes the approval and records the rejection.
Stack: none
Acceptance: approving `architecture` against `Readiness: BLOCKED — …` is refused and writes no gate file; approving with empty `user_words` is refused in `guided` mode and for `purchase` in `autonomous` mode, but accepted for `placement` in `autonomous` mode with `approved_by: session`; approving after a `.kicad_pcb` byte changed since the job entered the gate phase is refused with `stale_target`.

- [x] 1.7 Implement `flow_log` and `flow_defer` per design D1/D7: `flow_log`'s `kind` routes the entry to the job log, `memory/<role>.md`, `records/lessons-candidates.md`, or a new `handoffs/<job_id>/<NN>-<role>.md`; `flow_defer` appends to the matching `STATE.md` list and to the log.
Stack: none
Acceptance: `flow_log(kind: decision)` with an empty `why` or `rollback`, and `kind: evidence` carrying a `scope`, are refused with nothing appended; two `kind: handoff` calls with `role: review` create `01-review.md` then `02-review.md`; two `flow_defer` calls issued concurrently with `tokio::join!` both appear in `STATE.md`'s list.

- [x] 1.8 Register the toolset in `crates/konnect-core/src/router/registry.rs`: an `ALL_TOOLSETS` entry after `manufacturing` (`name: "flow"`, category `"orchestration"`, `tool_count: 6`) and the `build_tools_for` arm; add `flow_exposes_exactly_its_six_tools_in_order` to `router/mod.rs` beside the photo_intake test.
Stack: none
Acceptance: `cargo test -p konnect-core router::` passes, including `registry_tool_counts_match_reality`, `fixed_records_are_closed_and_only_reviewed_maps_are_extensible` with its allowlist unchanged, and the new ordered-membership test.

- [x] 1.9 Add `crates/konnect-core/tests/flow_gate_e2e.rs` reusing the `photo_intake_gate_e2e.rs` harness (load through `ToolRouter`, validate every call against the compiled schema, dispatch through the `ToolDef`): `flow_start` → requirements → architecture (`PASS`) → `flow_gate(architecture, approve)` → a `.kicad_pro` byte changes → leaving the gate is refused → rewind to `architecture`, re-advance, re-approve → leaving the gate succeeds; plus a package case where re-advancing a changed `architecture.md` after a rewind leaves the earlier approval unusable.
Stack: none
Acceptance: `cargo test -p konnect-core --test flow_gate_e2e` passes; every call in it is validated against the published schema before dispatch, so an uncallable schema fails the test.

- [x] 1.10 Bump the documented counts per design D12 in every place the sweeps name, add the `flow` section to `tool-directory.md` (heading `### \`flow\` · 6 tools`, all six tools in backticks), and add `".orchestrator"` and `"archive"` to `SKIP` in `crates/konnect/tests/doc_tool_counts.rs` with a comment that historical records answer to their own commit.
Stack: none
Acceptance: `cargo test -p konnect --test doc_tool_counts` exits 0; `grep -c "flow_status\|flow_start\|flow_advance\|flow_gate\|flow_log\|flow_defer" tool-directory.md` is at least 6; the diff of `doc_tool_counts.rs` is only the two `SKIP` names and their comment.

## 2. Companion skills (two — design D9)

- [x] 2.1 Write `crates/konnect/assets/skills/kicad-architecture/SKILL.md` and `references/constraint-record-schema.md` (design D9, D5): a requirements section (capture the constraint record's fields once, decide the obvious and record it with `flow_log(kind: decision)`, one question round through a BLOCKED handoff), an architecture section (block diagram, power tree and budget, pin plan, worst-case records by reference to kicad-schematic's `design-calculations.md` §1, parts that can be bought, the D5 readiness line verbatim), and in the body the required section headings of `constraints.md`, `architecture.md`, `worst-case.md` and `pin-plan.md`. The schema reference lists each constraint field (environment, power, currents, interfaces, connectors and sides, size, enclosure, fab/service, quantity, cost) with its source (user, datasheet, config, decision).
Stack: none
Acceptance: both files exist and `SKILL.md` names `constraint-record-schema.md` and `architecture-record-schema.md`; `SKILL.md` contains `Readiness: PASS` and `Readiness: BLOCKED`; `cargo test -p konnect --test asset_references -- documented_toolsets_exist_in_the_registry call_examples_name_real_parameters` passes.

- [x] 2.2 Write `crates/konnect/assets/skills/kicad-architecture/references/architecture-record-schema.md`: `architecture.md` (blocks, power tree and budget, interfaces, a parts list with stock, lot ceiling, exact suffix, datasheet and AVL/derating status — the section `kicad-sourcing-agent` fills — open questions, readiness line last), `worst-case.md` (one record per decisive value, using `design-calculations.md` §1's fields), `pin-plan.md` (pin, function, net, constraint, source).
Stack: none
Acceptance: the file exists; it names `design-calculations.md`, `jlcpcb-rules.md` and `datasheet-audit.md` instead of restating them; its last-line rule matches design D5.

- [x] 2.3 Write `crates/konnect/assets/skills/kicad-curator/SKILL.md` with no `references/` directory: the triage rule (design D7), the candidate entry format (lesson, evidence, scope, role, promote yes/no), the playbook note when a job had at most one FIX round, the 60-line cap the orchestrating session applies when promoting, and that no brain path is ever read or written (Non-Goal 3).
Stack: none
Acceptance: the file exists; it names all three destinations (`~/.konnect/agents/<role>/MEMORY.md`, `memory/<role>.md`, `records/lessons-candidates.md`); every `flow_log(…)` signature example in it names `project_dir`, `job_id`, `kind` and `message`.

## 3. `konnect` skill: reference, templates, router

- [x] 3.1 Write `crates/konnect/assets/skills/konnect/references/orchestration.md` with every section design D8 lists, including the D4 producer/records table, the FIX-layer → rewind mapping, the three-round cap read from `fix_rounds`, the D6 cross-check and the Codex rule.
Stack: none
Acceptance: `grep -c "evidence_check\|get_recent_calls"` against it is at least 2 and `grep -c "Readiness: PASS"` at least 1; `cargo test -p konnect --test asset_references -- documented_toolsets_exist_in_the_registry call_examples_name_real_parameters tools_listed_beside_a_toolset_belong_to_it` passes; it names every agent design D9 assigns to a phase.

- [x] 3.2 Write `crates/konnect/assets/skills/konnect/references/brief-template.md` and `references/handoff-template.md` per design D8.
Stack: none
Acceptance: both files exist; `handoff-template.md` names `failing_layer` with `requirement`, `architecture` and `implementation` and states it is required on `FIX`; `brief-template.md` names all eight fields and says Files Scope never includes `STATE.md`.

- [x] 3.3 Edit `crates/konnect/assets/skills/konnect/SKILL.md` per design D8/D10: the Orchestrator section before the Decision Tree (lanes, the three-row Need → Read table), a `| Orchestration | flow |` row in Available Toolsets, and one Agent Routing bullet per new agent; nothing else changes.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references -- top_level_skill_routes_every_bundled_agent every_reference_is_reachable_from_its_parent_skill scripted_board_fallback_stays_guarded` passes; `git diff` of the file removes no line; the file names all three new reference files.

- [x] 3.4 Close the two confirmation gaps round 2b found (orchestrator DECISION 2026-09-22): in `crates/konnect/assets/skills/konnect/references/orchestration.md` §9 state that the orchestrating session — which can fetch pages and run a terminal, unlike the bundled agents — confirms each parts-list row's live stock and datasheet before the architecture gate and records every confirmation with `flow_log(kind: evidence)`; in §4's purchase row name the manufacture agent's `## Checks at the purchase gate` section and say the session shows it to the user and runs or asks for each listed check before `flow_gate` for `purchase`; in `crates/konnect/assets/skills/kicad-architecture/references/architecture-record-schema.md` make the readiness criterion explicit: a row is confirmed only when the session's evidence entry exists, and a `located, not validated` datasheet or a catalogue-only stock keeps the readiness line BLOCKED.
Stack: none
Acceptance: `grep -c "Checks at the purchase gate"` against orchestration.md is at least 1 and `grep -c "located, not validated"` against architecture-record-schema.md is at least 1; `cargo test -p konnect --test asset_references -- call_examples_name_real_parameters every_reference_is_reachable_from_its_parent_skill` passes.

- [x] 3.5 Carry task 3.4's confirmation rule into the texts the agents load, and fix the stale asset count (orchestrator DECISION 2026-09-22): in `crates/konnect/assets/skills/kicad-architecture/SKILL.md`'s readiness section and in `crates/konnect/assets/agents/kicad-architecture-agent.md` (body only; Step 6's parts-list bullet no longer implies catalogue stock is acceptable until payment) state that a parts-list row counts as confirmed only when the orchestrating session's live-stock and datasheet evidence entry exists, that a catalogue-only stock or a `located, not validated` datasheet keeps the readiness line BLOCKED, and that the agent then returns BLOCKED naming the rows the session must confirm; update README.md's bundled skills/agents count to what `manifest.rs` ships.
Stack: none
Acceptance: `grep -c "located, not validated"` is at least 1 in both files; the agent's frontmatter is byte-identical; `cargo test -p konnect --test asset_references` and `cargo test -p konnect --test doc_tool_counts` pass.

## 4. Six new agents (frontmatter copies the existing block-list shape — design D9)

- [x] 4.1 Write `crates/konnect/assets/agents/kicad-requirements-agent.md`: `skills:` `konnect`, `kicad-architecture`, `kicad-schematic`; `tools:` `mcp__konnect__*`; `model: sonnet`; `maxTurns: 60`. In a job: read prior records with `flow_status`, draft the constraint record, record obvious decisions with `flow_log`, return BLOCKED with every product question at once, and after the answers call `flow_advance` into `architecture` with `constraints.md`; persist the handoff with `flow_log(kind: handoff)`.
Stack: none
Acceptance: the frontmatter matches with `skills:` and `tools:` as block lists; the file contains `load_toolset("flow")`, `flow_advance` and `flow_log`; it states that a run without a `job_id` calls no `flow_*` tool.

- [x] 4.2 Write `crates/konnect/assets/agents/kicad-architecture-agent.md`: `skills:` `konnect`, `kicad-architecture`, `kicad-schematic`; `tools:` `mcp__konnect__*`; `model: sonnet`; `maxTurns: 150`; ends a job run with `flow_advance` into `gate:architecture` carrying `architecture.md`, `worst-case.md`, `pin-plan.md`.
Stack: none
Acceptance: the frontmatter matches with block lists; the file contains `load_toolset("flow")` and `flow_advance`; it states the D5 readiness line and that a BLOCKED readiness means returning BLOCKED, not advancing.

- [x] 4.3 Write `crates/konnect/assets/agents/kicad-sourcing-agent.md`: `skills:` `konnect`, `kicad-architecture`, `kicad-manufacture`, `kicad-review`; `tools:` `mcp__konnect__*`; `model: sonnet`; `maxTurns: 150`; its procedure sequences BOM integrity, `jlcpcb-rules.md`, `datasheet-audit.md` and the `get_effective_config` sourcing policy, and returns the parts list in the shape of `architecture-record-schema.md`'s parts-list section.
Stack: none
Acceptance: the frontmatter matches with block lists; its Hard Rules name `search_symbols`/`search_footprints` as read-only lookups and forbid placing or wiring a part; it states it never calls `flow_advance` and persists its handoff with `flow_log` in a job.

- [x] 4.4 Write `crates/konnect/assets/agents/kicad-library-agent.md`: `skills:` `konnect`, `kicad-library`; `tools:` `mcp__konnect__*`; `model: sonnet`; `maxTurns: 100`; persists its handoff with `flow_log` in a job.
Stack: none
Acceptance: the frontmatter matches with block lists; it names the physical pin-map acceptance contract from `kicad-library/SKILL.md`; it states it never edits a schematic sheet or the board.

- [x] 4.5 Write `crates/konnect/assets/agents/kicad-manufacture-agent.md`: `skills:` `konnect`, `kicad-manufacture`; `tools:` `mcp__konnect__*`; `model: sonnet`; `maxTurns: 150`; ends a job run with `flow_advance` into `gate:purchase` carrying `manufacturing.md`; never produces or sends a firmware-contract artifact (Non-Goal 4).
Stack: none
Acceptance: the frontmatter matches with block lists; the file contains `load_toolset("flow")`, `flow_advance` and the firmware-contract non-goal; it uses the kicad-manufacture contract vocabulary `INCOMPLETE` and `indicative heuristic`.

- [x] 4.6 Write `crates/konnect/assets/agents/kicad-curator-agent.md`: `skills:` `konnect`, `kicad-curator`; `tools:` `mcp__konnect__*`; `model: sonnet`; `maxTurns: 60`; at `learn` only: read the log, handoffs, candidates and project memory with `flow_status`, record distilled lessons with `flow_log(kind: lesson)`, return the promote list, and close the job with `flow_advance(to_phase: closed)`.
Stack: none
Acceptance: the frontmatter matches with block lists and the file contains `load_toolset("flow")` and `flow_advance`; its anti-triggers name "any phase before `learn`"; it states the brain non-goal and that it never writes a `MEMORY.md` itself.

## 5. Existing agents gain their flow steps

- [x] 5.1 Edit `crates/konnect/assets/agents/kicad-schematic-build-agent.md`: a Setup note (in a job, read `architecture.md`, `pin-plan.md`, `worst-case.md` with `flow_status`; Step 1's architecture brief is then already approved) and a new Step 9 after Step 8 (in a job: `load_toolset("flow")`, `flow_advance` into `schematic_review` with `schematic-evidence.md` holding the final Step 6/7 results and the Step 8 layout handoff, its evidence calls in `evidence_calls`, then the handoff via `flow_log`).
Stack: none
Acceptance: the file contains `load_toolset("flow")` and `flow_advance`; the frontmatter (`model`, `tools`, `maxTurns`, `skills`) is byte-identical; the new step follows Step 8 and applies only when the brief names a `job_id`.

- [x] 5.2 Edit `crates/konnect/assets/agents/kicad-pcb-layout-agent.md`: in a job, read `constraints.md` and `schematic-evidence.md` with `flow_status`; end Phase 3 with `flow_advance` into `gate:placement` carrying `placement.md`; end the routing run with `flow_advance` into `prefab_review` carrying `routing.md`.
Stack: none
Acceptance: `grep -c "flow_advance"` against the file is at least 2 and it contains `load_toolset("flow")`; the frontmatter is byte-identical; the placement step keeps the existing hard stop (it returns after recording and never routes in the same run).

- [x] 5.3 Edit `crates/konnect/assets/agents/kicad-design-review-agent.md`: in a job and in single-reviewer mode, end with `flow_advance` into the next phase carrying `ledger-schematic.md` or `ledger-prefab.md`; under a multi-reviewer merge it returns its report and the session advances (design D1).
Stack: none
Acceptance: the file contains `load_toolset("flow")` and `flow_advance`; the frontmatter is byte-identical; it states it still never mutates the design and does not call `flow_advance` when the session merges several reviewers.

## 6. Manifest, install wiring, asset guards

- [x] 6.1 Update `crates/konnect/src/manifest.rs`: `include_str!` the new skills `kicad-architecture` (two references) and `kicad-curator`, the three new `konnect` references, and the six new agents; add a `manifest_ships_every_asset` test to `crates/konnect/src/install.rs`'s tests (design D10 item 5).
Stack: none
Acceptance: `cargo build -p konnect` succeeds; `cargo test -p konnect manifest_ships_every_asset` passes; the test derives the asset list by walking `crates/konnect/assets/` (skills, references, agents), not from a hand-written list.

- [x] 6.2 Run `cargo test -p konnect --test asset_references backticked_tool_names_in_prose_exist_in_the_registry` and add every flagged non-tool name to `NOT_TOOLS` as one commented block (design D10 item 2).
Stack: none
Acceptance: that test passes; every added name was flagged by a failing run (the list goes in the handoff); no added name is a top-level input property of a registered tool.

- [x] 6.3 Enroll the flow-calling agents in `agents_make_claimed_evidence_executable` (design D10 item 3): extend the schematic, layout and review tuples with `flow`/`flow_advance`, and add tuples for the requirements, architecture, manufacture and curator agents.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references agents_make_claimed_evidence_executable` passes; seven cases require `flow` and `flow_advance`; removing `load_toolset("flow")` from any of those seven agent files makes the test fail.

- [x] 6.4 Run the full asset-guard and install suites and fix any failure surfaced by rounds 2–5.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references` passes in full; `cargo test -p konnect install::` passes.

## 7. Final gates

- [x] 7.1 Run `cargo fmt --check` across the workspace and fix any formatting drift introduced by this change.
Stack: none
Acceptance: `cargo fmt --check` exits 0.

- [x] 7.2 Run `cargo clippy --all-targets` and resolve any new warning introduced by this change.
Stack: none
Acceptance: `cargo clippy --all-targets` exits 0 with no warnings attributable to files touched in this change.

- [x] 7.3 Run the full truth-command gate.
Stack: none
Acceptance: `cargo test -p konnect` and `cargo test -p konnect-core` both exit 0.

## 8. Fix round 1 (reviewer 11)

Reviewer 11's verdict (`.orchestrator/handoffs/konnect-orchestrator/11-reviewer.md`)
plus the orchestrator's DECISIONs A–F
(`.orchestrator/log/2026-09-21-konnect-orchestrator.md`). Reviewer 11 minor 6
is closed by QA's `every_agent_that_names_a_flow_tool_loads_flow` and
`architecture_confirmation_rule_stays_guarded` — no task for it. Tasks
8.1–8.4 all edit `flow.rs` and its own test module; they run sequentially.
8.5–8.7 edit skill/agent/reference assets, disjoint files, and can run in
parallel with each other (not with 8.1–8.4, which change the tool behaviour
those assets describe). 8.8 is the guard/regression pass and runs last. None
of QA's five additive guards (`crates/konnect/tests/asset_references.rs`:
`architecture_confirmation_rule_stays_guarded`,
`orchestration_reference_keeps_its_protocol_rules`,
`orchestrator_agents_keep_their_contract`,
`every_agent_that_names_a_flow_tool_loads_flow`,
`record_sections_are_stated_where_their_writer_loads_them`) or
`crates/konnect-core/tests/flow_contract_e2e.rs` need an assertion changed by
this round: every edit below is additive to a file or a table row those
guards read a substring or a heading from, not a rewording or removal of any
pinned phrase — task 8.8 re-runs them unmodified to confirm.

- [x] 8.1 DECISION A. In `crates/konnect-core/src/tools/flow.rs`, add the converse loop to `validate_phases` (design D3 Fix round 1, DECISION A): for each `(phase, gate)` in `GATED_PHASES`, if `phases` contains `gate` it must also contain `phase`, refused with the same error shape as the existing forward loop (naming the gate and the missing phase).
Stack: none
Acceptance: `foundation_tests::the_validator_names_the_entry_it_rejects` gains two cases — `(&["routing", "prefab_review", "gate:purchase"], "\"gate:purchase\"", "manufacturing")` and `(&["requirements", "gate:architecture", "schematic"], "\"gate:architecture\"", "architecture")` — each `Ok` today (fails the new assertion at `62d2045`) and `Err` naming both quoted strings once the loop is added; `crates/konnect-core/tests/flow_gate_e2e.rs` gains `a_gate_without_its_phase_is_refused_through_flow_start`, sending `flow_start(phases: ["routing", "prefab_review", "gate:purchase"])` through the router and asserting an `invalid_argument` naming `manufacturing` and that no `.konnect/flow/` directory exists afterward; `cargo test -p konnect-core flow::` and `cargo test -p konnect-core --test flow_gate_e2e` both pass.

- [x] 8.2 DECISION B. In `crates/konnect-core/src/tools/flow.rs`'s `move_back`, refuse a rewind whose `to_phase` is a gate (`is_gate(&request.to_phase)`) before any other check, naming the gate and the phase that produces it (reverse `GATED_PHASES` lookup, the same table task 8.1 reads) — `invalid_argument`, field `to_phase`, nothing written.
Stack: none
Acceptance: `rewind_tests` gains `a_rewind_cannot_target_a_gate`: a job standing at `routing` with `gate:placement` in its phases, `flow_advance(to_phase: "gate:placement", reason: "…")` returns `invalid_argument` naming `"gate:placement"` and `"placement"`, and `STATE.md`'s bytes are unchanged — accepted (the phase moves to `gate:placement` and `record_gate_keys` runs) at `62d2045`, refused after the fix; `cargo test -p konnect-core flow::` passes.

- [x] 8.3 DECISION C. In `crates/konnect-core/src/tools/flow.rs`, make `STATE.md`'s successful write the one commit point for `flow_gate` and every `flow_advance` transition (design D2 Fix round 1, DECISION C): `apply_gate` and `commit_transition` stop writing the gate file / appending the log inside the closure that runs before `transact_atomic`'s rename; they return that data instead, and the caller (a thin wrapper around `transact_state` for these two tools) performs the gate-file write / log append only after the rename returns `Ok`, folding a failure there into a `warning` field on the success response (never an error). Phase records (`flow_advance` forward, D4) keep writing before the commit, unchanged.
Stack: none
Acceptance: `gate_tests` gains `a_gate_file_write_failure_after_state_commits_is_a_warning`: with `records/gates` pre-created as a plain file (not a directory) at the architecture gate, `flow_gate(architecture, approve, …)` today (`62d2045`) returns an error and leaves `STATE.md` unapproved; after the fix it returns success with a `warning` field naming the failed write, and `flow_status` shows `gate_approvals.architecture.status: "current"` recorded. `advance_tests` gains `a_log_append_failure_after_state_commits_is_a_warning`: with the job's `log` directory pre-created as a plain file, a forward `flow_advance` today returns an error with the phase unchanged; after the fix it returns success with a `warning` field and `STATE.md`'s phase has moved. `cargo test -p konnect-core flow::` and `cargo test -p konnect-core --test flow_gate_e2e` both pass.

- [x] 8.4 DECISION D. In `crates/konnect-core/src/tools/flow.rs`'s `gate_validity`, report each approval's `status` — `"current"` when its gate token equals `state.phase`, `"passed"` otherwise — and compute/emit the `valid` field only for the `"current"` entry; a `"passed"` entry carries no `valid` field (its `design_hash_at_approval`/`package_hash_at_approval` already state what it was approved against).
Stack: none
Acceptance: `status_tests` gains `a_passed_gates_approval_reports_status_passed_without_a_valid_field`: a job that approved `gate:architecture` and has since advanced to `schematic` reports `gate_approvals.architecture.status == "passed"` and no `valid` key, unchanged `design_hash_at_approval`; a job currently standing at `gate:placement` with an approval recorded reports `gate_approvals.placement.status == "current"` and a recomputed `valid` exactly as `gate_validity_is_recomputed_against_the_current_hashes` already checks. `cargo test -p konnect-core flow::` passes.

- [x] 8.5 DECISION A + D (orchestration.md). In `crates/konnect/assets/skills/konnect/references/orchestration.md`, append (do not reword or remove any existing sentence) — §1, after "A job's `phases` never drops a human gate: …": a sentence stating the converse, "and a gate's phase must be present too: `gate:architecture` requires `architecture`, `gate:placement` requires `placement`, `gate:purchase` requires `manufacturing` — a lane cannot show an approval bound to a record from a closed or earlier job."; §7, after the `gate_approvals` bullet: "`valid` matters only while `phase == "gate:<name>"` (the job's current gate); an approval of a gate already left reports `status: passed` with the hashes it was approved at, not a reason to re-ask or rewind."; and (DECISION G) §7 also states that a `warning` field on a successful `flow_gate`, `flow_advance` or `flow_defer` means the state change committed — never repeat the call; repair only what the warning names.
Stack: none
Acceptance: `grep -c "a gate's phase must be present too"` against orchestration.md is 1, inside §1 (before the next `## 2.` heading); `grep -c "status: passed"` against orchestration.md is at least 1, inside §7 (after the `## 7. Resume` heading); `grep -c "never repeat the call"` against orchestration.md is at least 1; `cargo test -p konnect --test asset_references -- orchestration_reference_keeps_its_protocol_rules` passes.

- [x] 8.6 DECISION E. In `crates/konnect/assets/agents/kicad-library-agent.md` Step 6, before placing, add registering the same library (the nickname and path Step 4 used, both symbol and footprint) in the scratch project's own table with `register_symbol_library`/`register_footprint_library` (`scope: "project"`, `project: <scratch project path>`) — design D9 Fix round 1, DECISION E.
Stack: none
Acceptance: `grep -c "register_symbol_library\|register_footprint_library"` against `kicad-library-agent.md` is at least 1, inside Step 6 (after "**Step 6:" and before "**Step 7:"); the sentence names both `scope: "project"` and the scratch project; `cargo test -p konnect --test asset_references` passes in full.

- [x] 8.7 DECISION F. In `crates/konnect/assets/agents/kicad-manufacture-agent.md`: rewrite the `Verdict` bullet so `READY` requires no warning (drop "and only the purchase-gate checks remain") and `INCOMPLETE` gains "or passed with a warning"; rewrite "Ending the run"'s opening sentence and its "is not an exit" sentence so the run may also end on an `INCOMPLETE` verdict whose only open items are exactly the three `## Checks at the purchase gate` entries, named and nothing else; update the "Record the phase" bullet to match. In `crates/konnect/assets/skills/konnect/references/orchestration.md` §2's `manufacturing` row, append (do not remove) "..., except when its only open items are the three `## Checks at the purchase gate` entries, which may exit to `gate:purchase`" after "an `INCOMPLETE` package is not an exit". `kicad-manufacture/SKILL.md` is unchanged (design D9 Fix round 1, DECISION F: its vocabulary is the one the agent now matches).
Stack: none
Acceptance: `git diff -- crates/konnect/assets/skills/kicad-manufacture/SKILL.md` is empty and `grep -c "passed with a warning"` against `kicad-manufacture-agent.md` is at least 1; `grep -c "which may exit to \`gate:purchase\`"` against `orchestration.md` is 1; `cargo test -p konnect --test asset_references -- orchestrator_agents_keep_their_contract orchestration_reference_keeps_its_protocol_rules record_sections_are_stated_where_their_writer_loads_them` passes.

- [x] 8.8 Guards and full regression. Re-run the complete asset-guard and flow suites after 8.1–8.7 land, and fix any failure they surface (there should be none — every edit above is additive to a passing guard's pinned text).
Stack: none
Acceptance: `cargo test -p konnect --test asset_references` passes in full (all guards, including QA's five additive ones, unmodified); `cargo test -p konnect-core --lib flow::` and `cargo test -p konnect-core --test flow_gate_e2e --test flow_contract_e2e` all pass with zero edits to `flow_contract_e2e.rs`; `cargo fmt --check` and `cargo clippy --all-targets` both exit 0.

- [x] 8.9 DECISION H. In `crates/konnect-core/src/tools/flow.rs`, move `flow_defer`'s log append (`apply_defer`) out of the closure that runs before `transact_atomic`'s rename, exactly as task 8.3 did for `flow_gate` and transitions: the log entry is written only after `STATE.md` is saved, and a failure there becomes the `warning` field on the success response.
Stack: none
Acceptance: a new test `a_defer_log_failure_after_state_commits_is_a_warning` (with the job's `log` directory pre-created as a plain file) fails at `421aa56` (error, item absent from `STATE.md`) and passes after the fix (success with a `warning` field, item present in `STATE.md`); `cargo test -p konnect-core flow::` and `cargo test -p konnect-core --test flow_gate_e2e --test flow_contract_e2e` pass.

## 9. Fix round 2 (reviewer 16)

Reviewer 16's verdict (`.orchestrator/handoffs/konnect-orchestrator/16-reviewer.md`)
plus the orchestrator's DECISIONs I–L
(`.orchestrator/log/2026-09-21-konnect-orchestrator.md`). DECISION L (the
post-commit side writes landing outside the `STATE.md` lock) is accepted as
a trade-off — see design.md's Fix round 2 note under D2 — and needs no task.
Tasks 9.1–9.4 touch disjoint files (`flow.rs`; `orchestration.md`;
`kicad-manufacture-agent.md`; `asset_references.rs`) and can run in
parallel, **except** that 9.3 (the agent's Verdict text) and 9.4 (the guard
that pins it) land together: `manufacture_agent_verdicts_match_the_skill`
fails on 9.3 alone, because — unlike every round-1 asset edit — this one
replaces a marker's literal wording rather than adding beside it (design.md
D9 Fix round 2 note, DECISION I). 9.5 is the regression pass and runs last.

- [x] 9.1 DECISION K (text-only; no behaviour change). In `crates/konnect-core/src/tools/flow.rs`: reword `flow_status`'s published description (~line 3105, "gate approvals with a recomputed `valid`") and the `STATE.md` body's rendered sentence (~line 664, "Validity is recomputed by `flow_status`; this body shows what was recorded.") so both say validity is computed only for the gate the job stands at now, using the same six-word clause in both places: "only for the gate the job stands at now" (the phrase orchestration.md §7 already carries from task 8.5). In `flow_gate`'s, `flow_advance`'s and `flow_defer`'s published descriptions (~lines 3167–3340), each add a clause naming the `warning` field and "never repeat the call" (the same rule orchestration.md §7 already states). In `flow_advance`'s description, change "a refusal writes nothing" to "a validation refusal writes nothing" (a successful transition can still carry a `warning` about an unwritten side file, which is not a refusal).
Stack: none
Acceptance: `grep -c "only for the gate the job stands at now" crates/konnect-core/src/tools/flow.rs` is at least 2; `grep -c "never repeat the call" crates/konnect-core/src/tools/flow.rs` is at least 3, and `grep -c "a validation refusal writes nothing" crates/konnect-core/src/tools/flow.rs` is 1; `cargo test -p konnect-core --lib flow::` and `cargo test -p konnect-core --test flow_gate_e2e --test flow_contract_e2e --test flow_fix_round_one_e2e` all pass with zero behaviour change (only `ToolDef` description strings and the `STATE.md` render string move).

- [x] 9.2 DECISIONS J + K. In `crates/konnect/assets/skills/konnect/references/orchestration.md` §4 (`## 4. Gates`): after "Only then does it ask for the `purchase` decision with `flow_gate`. A check that failed is a FIX (§5), not an approval." and before "A rejection records the decision; route its reason as a FIX (§5).", insert a new bullet — "Once `flow_gate` records the `purchase` approval — its `user_words` confirming those checks were discharged — the session records the package as `READY` in the skill's sense with `flow_log(project_dir, job_id, kind, message)`, `kind` `evidence`, naming that approval. Placing or uploading the order is the user's own action; no agent, including `kicad-manufacture-agent`, ever uploads it." Separately in the same section, replace the sentence "`flow_status` reports every approval's validity against the current files in `gate_approvals`." with "`flow_status`'s `gate_approvals` reports `valid` only for the gate the job stands at now (§7); a passed gate reports `status: passed` with no `valid` field." — matching §7's already-correct text (task 8.5) instead of contradicting it.
Stack: none
Acceptance: `grep -c "records the package as \`READY\` in the skill's sense"` against `orchestration.md` is 1, inside `## 4. Gates` (before the next `## 5.` heading); `grep -c "reports every approval's validity"` against `orchestration.md` is 0, and `grep -c "only for the gate the job stands at now"` against it is 1; `cargo test -p konnect --test asset_references -- orchestration_reference_keeps_its_protocol_rules orchestration_reference_states_the_fix_round_one_rules` passes (both guards' pinned markers are untouched by this edit).

- [x] 9.3 DECISIONS I + J. In `crates/konnect/assets/agents/kicad-manufacture-agent.md`'s `Verdict` bullet, replace the whole bullet so `READY`/`NOT READY`/`INCOMPLETE` scope the warning rule to artifact checks and adjudicate DRC and preflight instead of warning-gating them: "**Verdict**: `READY` when every artifact check your tools can run passed with no warning and no purchase-gate check is still open, every DRC error is resolved or waived, and every DRC warning and every preflight issue is adjudicated (fixed, or accepted with its reason recorded in this file's Design evidence section); `NOT READY` when a design defect, an unwaived DRC error, or an unadjudicated preflight issue blocks the order; `INCOMPLETE` when an artifact check your tools can run did not run, failed to execute, left an artifact missing, or passed with a warning (the skill's \"Any warning or missing requested artifact type keeps the result `INCOMPLETE`\"), or when a purchase-gate check is still open. An adjudicated DRC or preflight warning is not an open item and does not by itself keep the verdict `INCOMPLETE`. You never mark the three purchase-gate checks done, so a package whose own checks all passed reads `INCOMPLETE` and names the three — with or without a job, since \"Only `READY` permits upload\". `READY` means the package matches the board — never that the product is proven." In `### Ending the run`, after "After it is accepted, change nothing — the user's approval binds to this design and this record." add: "This agent's own record never itself becomes `READY` in the skill's global sense: once `flow_gate` approves `purchase`, the orchestrating session declares that and records it with `flow_log(kind: evidence)` (orchestration.md §4) — this agent never uploads."
Stack: none
Acceptance: `grep -c "every artifact check your tools can run passed"` against `kicad-manufacture-agent.md` is 1, inside the `Verdict` bullet; `grep -c "every DRC warning and every preflight issue is adjudicated"` against it is at least 1, and `grep -c "and only the purchase-gate checks remain"` against it is 0 (the round-1 stale phrase never returns); `grep -c "the orchestrating session declares that"` against it is 1, inside `### Ending the run` (after "### Ending the run" and before "### Hard rules").

- [x] 9.4 DECISION I — the guard. In `crates/konnect/tests/asset_references.rs`'s `manufacture_agent_verdicts_match_the_skill` (the guard at `:2068` naming "left an artifact missing, or passed with a warning"): replace the Verdict marker "`READY` when every check your tools can run passed with no warning and no purchase-gate check is still open" with "`READY` when every artifact check your tools can run passed with no warning and no purchase-gate check is still open"; add two markers — "every DRC warning and every preflight issue is adjudicated" and "An adjudicated DRC or preflight warning is not an open item"; add a `stale` check beside the existing "and only the purchase-gate checks remain" one for the phrase "every check your tools can run passed with no warning" (task 9.3's replaced wording must never come back word-for-word); add one marker to the existing "Ending the run" `missing_markers` call for "the orchestrating session declares that". Add a new test in the same file, `manufacture_verdict_permits_exit_with_adjudicated_drc_warnings`, that reads the `Verdict` section (`section(agent, "- **Verdict**:", "**Firmware-contract non-goal.**")`) and asserts, as the prose-level stand-in for reviewer 16's `ecc83-pp` reproduction (0 DRC errors, 17 DRC warnings, `kicad-cli 10.0.2 pcb drc --severity-all` on `crates/konnect-sexp/tests/fixtures/ecc83-pp.kicad_pcb`): (a) the text states an adjudicated DRC/preflight warning is not an open item (so 0 DRC errors plus adjudicated DRC warnings never by themselves force `INCOMPLETE`); (b) the artifact-level clause "left an artifact missing, or passed with a warning" is still present with no adjudication escape — an export `warnings` array entry keeps `INCOMPLETE` regardless.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references -- manufacture_agent_verdicts_match_the_skill manufacture_verdict_permits_exit_with_adjudicated_drc_warnings` passes; mutating the agent's Verdict bullet back to the pre-9.3 wording ("every check your tools can run passed with no warning" without "artifact", and no DRC/preflight adjudication sentence) makes both tests fail; `grep -c "left an artifact missing, or passed with a warning"` against `crates/konnect/tests/asset_references.rs` is at least 2 (the existing marker plus the new test's assertion).

- [x] 9.5 Guards and full regression. Re-run the complete asset-guard and flow suites after 9.1–9.4 land, and fix any failure they surface.
Stack: none
Acceptance: `cargo test -p konnect --test asset_references` passes in full (every guard, including the edited `manufacture_agent_verdicts_match_the_skill` and the new `manufacture_verdict_permits_exit_with_adjudicated_drc_warnings`); `cargo test -p konnect-core --lib flow::` and `cargo test -p konnect-core --test flow_gate_e2e --test flow_contract_e2e --test flow_fix_round_one_e2e` all pass; `cargo fmt --check` and `cargo clippy --all-targets` both exit 0.

- [x] 9.6 DECISION M (text consistency before the last verify round). Align the three remaining texts of this change with the fixed behaviour: (a) `tool-directory.md`'s `flow_status` row says gate approvals carry `valid` only for the gate the job stands at now (passed gates report `status: passed`), not "a recomputed `valid`"; (b) `crates/konnect/assets/agents/kicad-manufacture-agent.md`'s frontmatter `description` no longer promises the agent ends in `READY` by itself — it ends in `NOT READY` or `INCOMPLETE`, and `READY` is declared by the orchestrating session after the purchase approval (orchestration.md §4); (c) the same agent's older sentence that counts "a warning" as an open `INCOMPLETE` item says an artifact warning, consistent with the Verdict bullet (an adjudicated DRC or preflight warning is not an open item). Only these three edits; `model`, `skills`, `tools` and `maxTurns` stay byte-identical.
Stack: none
Acceptance: `grep -c "recomputed \`valid\`" tool-directory.md` is 0 and `grep -c "only for the gate the job stands at now" tool-directory.md` is 1; the agent's frontmatter no longer contains "ending in READY" and its `model`/`skills`/`tools`/`maxTurns` lines are unchanged; `cargo test -p konnect --test asset_references` and `cargo test -p konnect --test doc_tool_counts` pass.

- [x] 9.7 DECISION N (post-verify text polish from reviewer 21's three MINOR findings; no behaviour change). (a) In `crates/konnect/assets/agents/kicad-manufacture-agent.md`, add one sentence after the Verdict bullet's `INCOMPLETE` clause (not inside it — the guard forbids "DRC" there): a DRC or preflight run that did not complete — for example schematic parity not checked — is an open item that keeps the package from leaving the phase, never a warning to adjudicate away. (b) In `tool-directory.md`, the `flow_advance`, `flow_gate` and `flow_defer` rows carry DECISION K: "a validation refusal writes nothing" where a row says a refusal writes nothing, and each of the three names the `warning` field and "never repeat the call". Tables stay one line per row; frontmatter and every guard unchanged.
Stack: none
Acceptance: `grep -c "did not complete"` against `kicad-manufacture-agent.md` is at least 1; `grep -c "never repeat the call" tool-directory.md` is at least 3 and `grep -c "A refusal writes nothing" tool-directory.md` is 0; `cargo test -p konnect --test asset_references` and `cargo test -p konnect --test doc_tool_counts` pass.
