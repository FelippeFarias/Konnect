---
change: konnect-orchestrator
task: fix round 1 tasks 8.9, 8.5, 8.6, 8.7, 8.8 (DECISIONs H, A+D+G, E, F; regression)
agent: developer
verdict: DONE
---

## Result

Four commits on `orc/konnect-orchestrator`, on top of `421aa56`, in task order. Nothing was pushed. Task 8.8 needed no commit.

1. **`401c2e2` 8.9 (DECISION H).**
   - `apply_defer` no longer takes `flow_dir` and no longer appends the log. It returns `Committed { response, gate_file: None, log }`.
   - `handle_flow_defer` now runs through `transact_then_record`, so the append happens after `STATE.md` is saved. A failure there comes back as `warning` on a success, and a clean defer returns `warning: null`.
   - New test: `journal_tests::a_defer_log_failure_after_state_commits_is_a_warning`.
2. **`72ffc25` 8.5 (DECISIONs A, D, G).** `orchestration.md`, insert-only (15 lines added, 0 removed):
   - §1: new lines after "A gate is never first." carry "a gate's phase must be present too" and say `flow_start` refuses a list that breaks either rule.
   - §7, `gate_approvals` bullet: `valid` matters only at the job's current gate (`status: current`). A gate already left reports `status: passed` with no `valid` field.
   - §7, new last bullet: `warning` on a successful `flow_gate`, `flow_advance` or `flow_defer` means the change committed — "never repeat the call; repair only what the warning names".
3. **`542ed28` 8.6 (DECISION E).** Library agent Step 6, before placing: register the libraries from Step 4 in the scratch project, with `register_symbol_library(nickname, library_path, scope, project)` and `register_footprint_library(nickname, library_path, scope, project)`, `scope: "project"`, and `project` set to the scratch project's path. Both tools are in the `library` toolset, which the agent already loads.
4. **`94d2944` 8.7 (DECISION F).**
   - Manufacture agent: the Verdict bullet, the opening and "is not an exit" sentences of "Ending the run", and the "Record the phase" bullet are rewritten.
   - The §2 `manufacturing` row of `orchestration.md` gains ", except when its only open items are the three `## Checks at the purchase gate` entries, which may exit to `gate:purchase`".
   - `kicad-manufacture/SKILL.md` is untouched.
5. **8.8.** The full battery passes at `94d2944` with no fix needed.
   - `git diff --name-only 421aa56..HEAD` lists exactly the four files in FS.
   - `asset_references.rs`, `flow_contract_e2e.rs` and `flow_gate_e2e.rs` each have 0 diff lines.

## Evidence

1. **8.9, red and then green.**
   - At `421aa56` (test added, fix absent) it panicked with `Could not create \\?\C:\…\.konnect\flow\log (Cannot create a file when that file already exists. (os error 183))`.
   - A probe line, run and restored in one script (`restored identical: True`), printed `PROBE is_error=true findings_in_state=0`: an error, with the item missing from `STATE.md`.
   - After the fix the test reports `ok`. `cargo test -p konnect-core flow::` gives `61 passed; 0 failed`, and `--test flow_gate_e2e --test flow_contract_e2e` gives `4 passed` and `11 passed`.
   - Mutation proof, restored byte-identical: M1 (defer drops its log, back to `transact_state`) FAILED with `a warning field: {…no warning…}`; M2 (`warning` forced to null) FAILED the same way.
2. **8.5 greps** on `orchestration.md`:
   - "a gate's phase must be present too": 1 in the whole file, 1 inside §1 (between `## 1.` and `## 2.`).
   - "status: passed": 1, inside §7.
   - "never repeat the call": 1.
   - `orchestration_reference_keeps_its_protocol_rules`: ok.
3. **8.6.**
   - Inside Step 6 (from `**Step 6:` to before `**Step 7:`), `register_symbol_library\|register_footprint_library` matches 2 lines and `scope: "project"` 1. The sentence carries both `scope: "project"` and "the scratch project's path".
   - `asset_references`: 18 passed.
   - Mutation check, restored byte-identical: renaming `library_path` to `lib_path` fails `call_examples_name_real_parameters` with `names \`lib_path\`, which is not in its schema`.
4. **8.7.**
   - `git diff 421aa56 -- …/kicad-manufacture/SKILL.md` is 0 lines.
   - "passed with a warning" = 1 in the agent, and "which may exit to \`gate:purchase\`" = 1 in `orchestration.md`.
   - The three named guards pass: `orchestrator_agents_keep_their_contract`, `orchestration_reference_keeps_its_protocol_rules` and `record_sections_are_stated_where_their_writer_loads_them`.
5. **Full battery at `94d2944`:** every command exits 0.
   - `cargo test -p konnect`: asset_references 18, doc_tool_counts 6, not_tools_allowlist 3, schema_parameter_usage 5, protocol_stdio 18, and every other binary ok.
   - `cargo test -p konnect-core`: lib 1447 passed / 16 ignored; flow_contract_e2e 11; flow_gate_e2e 4; every other binary ok.
   - `cargo fmt --check`: exit 0.
   - `cargo clippy --all-targets`: exit 0 with 0 warnings.

## For the next agent

1. **Interpretation in 8.7, the one VERIFY round 2 should judge.**
   - Read literally, "READY = every check your tools can run passed with no warning" still yields READY when the three purchase-gate checks were never done. That leaves reviewer minor 5's job-less scenario open, and it makes the task's own exception ("INCOMPLETE whose only open items are the three") unreachable.
   - Following the skill (line 15: an unestablished required check means INCOMPLETE; line 267: "Only `READY` permits upload") and DECISION F's intent, READY here also requires "no purchase-gate check is still open", and INCOMPLETE also covers an open purchase-gate check.
   - The agent never marks those checks done, so its normal success is INCOMPLETE naming the three, which exits to `gate:purchase`. This is the old "READY, only gate checks remain" case, renamed.
   - The trade-off: the agent can no longer reach READY in practice.
2. **SC3 nuance.** 8.7 appends inside a markdown table row, and a row cannot continue on the next line. So `orchestration.md`'s cumulative diff shows exactly 1 `-` line: the `manufacturing` row.
   - A scripted check confirms the old row minus its closing ` |` is a strict prefix of the new row. Only the exception text was inserted before the pipe; no text was removed.
   - The 8.5 commit alone is +15 / -0.
3. **What does not follow the commit.** `flow_log` still appends inside the closure. Its content is the log itself and `STATE.md` is not rewritten, so there is no commit to follow. `flow_start` keeps `log_error`, not `warning`.
4. **Places to probe:**
   - the §7 `warning` bullet against `record_after_commit`'s real text ("Do not repeat the call; flow_status shows the committed state.");
   - the library Step 6 flow, live. Not verified: whether registering a design-project library in a scratch project outside it writes an absolute URI or a `${KIPRJMOD}` one. The skill says only project-local and same-repository sibling libraries become portable. I also did not confirm that `add_schematic_component` resolves symbols through the scratch project's `sym-lib-table`; reviewer 11 left that unverified too.
5. **Gotcha.** An acceptance `grep -c` on hard-wrapped prose is line-based. The 8.7 phrase first read 0 after I re-wrapped the paragraph. Every acceptance phrase now sits on one physical line.

## Deferred findings

1. **Unlocked post-commit writes.** Carried over from 13, deferred #2 and still open: post-commit side writes (now including `flow_defer`'s log) run outside the `STATE.md` lock. Two concurrent defers can therefore append their log entries in the opposite order to their `STATE.md` commits. `STATE.md` stays the truth.
2. **Tool descriptions omit `warning`.** The published descriptions of `flow_gate`, `flow_advance` and `flow_defer` do not mention the `warning` response field; only `orchestration.md` §7 does now. A client without the konnect skill would not know to read it.
