---
change: konnect-orchestrator
task: fix round 1 code tasks 8.1, 8.2, 8.4, 8.3 (reviewer 11 SERIOUS + minors 1, 2, 4; DECISIONs A-D)
agent: developer
verdict: DONE
---

## Result

The four tasks are committed on `orc/konnect-orchestrator` in the order 8.1 → 8.2 → 8.4 → 8.3, on top of `62d2045`. Nothing was pushed. `git diff --name-only 62d2045..HEAD` lists exactly `crates/konnect-core/src/tools/flow.rs` and `crates/konnect-core/tests/flow_gate_e2e.rs`. `flow_contract_e2e.rs` is untouched (its diff is 0 lines).

1. **`961e749` 8.1 (DECISION A).** `validate_phases` (`flow.rs:419`) gains the converse loop over `GATED_PHASES`. The unit test gains two cases, and `flow_gate_e2e.rs` gains `a_gate_without_its_phase_is_refused_through_flow_start`.
2. **`1dfeb74` 8.2 (DECISION B).** `move_back` (`flow.rs:2175`) refuses `is_gate(to_phase)` before any other check (field `to_phase`, nothing written). It no longer calls `record_gate_keys`. New test: `rewind_tests::a_rewind_cannot_target_a_gate`.
3. **`bc28881` 8.4 (DECISION D).** `gate_validity` (`flow.rs:1224`) emits `status: "current"` plus a recomputed `valid` only for the gate equal to `state.phase`. Every other approval gets `status: "passed"` and no `valid`. New test: `status_tests::a_passed_gates_approval_reports_status_passed_without_a_valid_field`.
4. **`421aa56` 8.3 (DECISION C).**
   - `transact_state` now wraps a generic `transact<T>`.
   - `flow_gate` and `flow_advance` run through `transact_then_record` (`flow.rs:1887`). `apply_gate` and `commit_transition` return a `Committed { response, gate_file, log }` instead of writing anything.
   - `record_after_commit` (`:1906`) writes the gate file and appends the log only after `transact_atomic` returns `Ok`. A failure there goes into `warning` on the success response.
   - Forward phase records are still written before the commit.
   - New tests: `gate_tests::a_gate_file_write_failure_after_state_commits_is_a_warning` and `advance_tests::a_log_append_failure_after_state_commits_is_a_warning`.
5. **Existing tests in `flow.rs` that had to change**, because they pinned behaviour this round forbids (they are rewritten, not deleted):
   - `a_rewind_into_a_gate_records_the_keys_its_next_approval_needs` is now `a_rewind_to_a_gates_phase_re_enters_it_with_the_keys_its_next_approval_needs`.
   - `an_approval_after_a_rewind_into_the_gate_binds_to_the_rewind_entry` is now `an_approval_after_a_rewind_and_re_entry_binds_to_the_re_entry`. Both now rewind to the producing phase and then re-enter the gate forward.
   - `gate_validity_is_recomputed_against_the_current_hashes` keeps its name but now stands at `gate:architecture`. The 8.4 acceptance cites it as "already checks", yet at `schematic` it would report `passed`.
   - `leaving_requirements_with_its_record_writes_it_and_moves_the_phase` gained a `warning: null` assertion.

## Evidence

Every command was run in the worktree with `PROTOC` and the CMake PATH exported.

1. **Red before each fix** (the new test was written first and run against the unfixed code):
   - 8.1 unit: `panicked at flow.rs:3448:57: rejected: ()`, meaning the Scenario A case returned `Ok`.
   - 8.1 e2e: `a purchase gate without manufacturing must be refused: {…"phase":"routing","phases":["routing","prefab_review","gate:purchase"]…}`.
   - 8.2: `expected an error, got {"cleared_approvals":["placement"],…,"package_hash":"adf4…","phase":"gate:placement",…,"transition":"rewind"}`. The rewind was accepted and `record_gate_keys` ran.
   - 8.4: `left: Null right: "passed"`. The payload showed `"architecture":{…"valid":false…}` at `"phase":"schematic"` after one schematic save, which is reviewer 11's symptom.
   - 8.3 advance test: `Could not create \\?\C:\…\.konnect\flow\log (Cannot create a file when that file already exists. (os error 183))`.
   - 8.3 gate test: the same error for `…\flow\records\gates`. Both were untyped errors with nothing committed.
2. **Mutation proof for 8.3.** The script mutates, runs and restores in `finally`. Result: `restored identical: True`.
   - Mutation A (always `None`, so the failure is hidden): both 8.3 tests FAILED.
   - Mutation B (skip the post-commit log append): both FAILED.
3. **Green after:**
   - The named tests: `the_validator_names_the_entry_it_rejects`, `a_rewind_cannot_target_a_gate`, `a_passed_gates_approval_reports_status_passed_without_a_valid_field`, `a_log_append_failure_after_state_commits_is_a_warning` and `a_gate_file_write_failure_after_state_commits_is_a_warning` all report `ok`, as does `a_gate_without_its_phase_is_refused_through_flow_start`.
   - `cargo test -p konnect-core flow::`: `ok. 60 passed; 0 failed` (exit 0).
4. **Other checks:**
   - `cargo test -p konnect-core --test flow_gate_e2e --test flow_contract_e2e`: `flow_contract_e2e 11 passed`, `flow_gate_e2e 4 passed` (exit 0).
   - `cargo fmt --check`: exit 0.
   - `cargo clippy -p konnect-core --all-targets`: exit 0, 0 warnings.
5. **Whole `konnect-core` suite:** `cargo test -p konnect-core` passes. The lib has 1446 passed and 16 ignored, and every integration binary reports `ok`.

## For the next agent

1. **Field names the assets round (8.5–8.7) must describe:**
   - `flow_status.gate_approvals.<gate>.status` is `"current"` or `"passed"`. `valid`, and `validity_error` when the package cannot be read, appear only on the `"current"` entry. A `"passed"` entry keeps `design_hash_at_approval`, `package_hash_at_approval`, `visit`, `user_words` and the rest.
   - `flow_gate` and `flow_advance` responses now always carry `warning`. It is `null` when both side files landed; otherwise it is a string.
   - `flow_start` keeps `log_error`; its name is unchanged.
2. **Exact refusal and warning texts:**
   - Validator: `phases includes "gate:purchase" without its phase "manufacturing"; a gate's package must come from this job, not from a record an earlier job left on disk` (field `phases`).
   - Rewind: `a rewind cannot target the gate "gate:placement"; rewind to "placement", the phase that produces its package, and advance into the gate again so its approval sees a fresh package. Nothing was written.` (field `to_phase`; the refusal comes before the `reason` check).
   - Warning: `STATE.md holds this change, but a derived side file was not written: <error>[; <error>]. Do not repeat the call; flow_status shows the committed state.`
3. **Asset guidance gap.** No 8.5–8.7 task text mentions `warning`. If `orchestration.md` should tell the session "a `warning` on success means do not repeat the call", that is an orchestrator or planner decision, not an asset task that exists today. The 8.5 grep target `status: passed` matches the emitted value `"passed"`.
4. **Decisions not visible in the diff:**
   - A failed gate-file write does not skip the log append. Both are attempted, and their errors are joined with `; `.
   - `gate_file` in the `flow_gate` response is now `flow_dir/records/gates/<gate>.md` built by join. It is the same string as before because `flow_dir` is already canonical.
   - `move_back` lost its now-unused `flow_dir` parameter.
   - `transact_atomic` skips the rename when a reject with no prior approval leaves `STATE.md` identical, and the gate file and log are then still written. This was already true before this round.
5. **Rejected approaches:**
   - Re-locking with a second `transact_atomic` for the post-commit writes. It still cannot order two racing gate files without extra compare-and-skip logic, and the design says "after `Ok`".
   - An automated test for P2's primary direction (`STATE.md` fails, so no side files are written). Forcing a rename failure portably is not possible; the direction is now guaranteed by construction. To confirm, re-run reviewer 11's `probe/p2_reject_window.py` against a rebuilt binary.

## Deferred findings

1. **`flow_defer` still appends its log before the `STATE.md` commit** (`apply_defer`, `flow.rs:3049`, the append at `:3079`). This is reviewer 11 minor 1's class, but for deferred items, and it is outside 8.3's scope (gates and transitions only). A failed state write can leave a log line for a defer that never landed, returned as an error that invites a retry.
2. **Post-commit side writes run outside the `STATE.md` lock.** `konnect_sexp::open_document_lock` is `pub(crate)`, and the only public locked API is `transact_atomic`.
   - Two concurrent `flow_gate` calls on the same gate could land their gate files in reverse commit order. The file would then show the older decision; `STATE.md` stays the truth.
   - Log entries of concurrent calls can interleave out of commit order.
   - The fix would need a lock guard exported by konnect-sexp, or a re-lock that checks the committed text before writing.
