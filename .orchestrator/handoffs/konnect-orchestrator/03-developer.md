---
change: konnect-orchestrator
task: tasks 1.1-1.4 — flow foundations, flow_status, flow_start, flow_advance forward transitions
agent: developer
verdict: DONE
failing_layer: null
---

## Result

1. Four commits on `orc/konnect-orchestrator` (worktree `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\konnect-orchestrator`, base `bbd5efb`), not pushed: `529081b` 1.1 foundations, `bdef831` 1.2 `flow_status`, `68e5dda` 1.3 `flow_start`, `ac389d7` 1.4 `flow_advance` forward.
2. Files: `crates/konnect-core/src/tools/flow.rs` (new; implementation above line 2157, tests below), `tools/mod.rs` (`pub mod flow;` only), `tools/photo_intake.rs` (`now_rfc3339_utc` → `pub(crate)` only).
3. `flow::tools()` publishes `flow_status`, `flow_start`, `flow_advance` in D1 order; NOT registered (task 1.8). Every object schema node declares `properties`; a schema test validates the documented call shapes against the closed, compiled schemas.
4. 38 `flow::` tests: 13 foundations, 7 status, 8 start, 9 advance, 1 schema — covering every acceptance line of 1.1-1.4.
5. Leaving a gate (D11 check against the approval of the current visit, naming changed package records) is already implemented in `flow_advance` — 1.6 adds only `flow_gate`.

## Evidence

1. `cargo test -p konnect-core flow::` (worktree, HEAD `ac389d7`) → `test result: ok. 38 passed; 0 failed; 0 ignored; … 1401 filtered out`, exit 0 (integration binaries: 0 matched, all ok).
2. `cargo fmt --check` → exit 0 (no output); `cargo clippy -p konnect-core --all-targets` → `Finished`, zero warnings, exit 0 (no `allow` left in `flow.rs`).
3. `git diff --name-only bbd5efb..HEAD` → exactly `flow.rs`, `mod.rs`, `photo_intake.rs`; `git log --oneline bbd5efb..HEAD` → the four commits above.
4. Mutation proofs (mutate → run → restore in one script, `restored byte-identical: True` each): gated-phase rule off → `the_validator_names_the_entry_it_rejects` FAILED; status calling `create_flow_dir` → `status_without_a_flow_directory…` FAILED; active-job check off → `a_second_start…` and `racing_starts_open_exactly_one_job` FAILED; D4 same-call rule off → `a_stale_record_on_disk_does_not_satisfy_leaving_architecture` FAILED; foreign-record rule off → `a_record_belonging_to_another_phase_is_refused` FAILED.
5. Wider run at `ac389d7`: `cargo test -p konnect-core` → lib `1423 passed; 0 failed; 16 ignored`, conformance 4, integration 12, photo_intake_gate_e2e 5 (+1 ignored), prepare_board_photo contracts 6 + 1, two_checkpoint 3 — exit 0; `cargo test -p konnect --test schema_parameter_usage --test doc_tool_counts --test asset_references --test not_tools_allowlist` → 5 / 6 / 13 / 3 passed, exit 0 (the source-scanning guards tolerate the unregistered `flow.rs`).

## For the next agent

1. **1.5 entry point**: `apply_advance`'s `Transition::Rewind | Transition::Abandon` arm is a placeholder refusal (`invalid_argument` on `to_phase`, "forward only") — replace it; `classify_transition` already classifies both. `a_skip_ahead_or_foreign_target_is_refused` sends `closed` from `requirements` WITH records and asserts only `is_error`, so 1.5 must keep refusing records on an abandon. `EvidenceCheck` and `HistoryEntry.evidence_check` exist (always `None` today); `evidence_calls` is already stored.
2. **Gate keys (architect's mistake #2)**: the entry INTO a gate carries `package_hash` + `package_files` (per-record sha256, `null` = absent); `current_visit(state)` = last history index whose `to` is the current phase. 1.6 must compare the current design/package against `history[current_visit]`, never now-vs-now, and store `visit`. A REWIND whose target is a gate token must also fill `package_hash`/`package_files` on its entry, or an approval after that rewind has no keys to compare.
3. **Reusable helpers**: `existing_state_path` (absent → `stale_target`, creates nothing); the transact pattern (closure returns `(current, Err(refusal))`, side files written only after every check); `append_log`/`log_entry` (appended inside the lock); `ensure_flow_subdir`; `list_handoffs` + `is_handoff_file_name` (1.7: NN = count + 1); `stale`/`conflict`; test fixtures in `test_support` plus `advance_tests::at_architecture_gate`.
4. **Decisions not in the diff's prose**: phases stay `String`s re-validated on every `parse_state` (a hand-edited unknown token is `state_error`/`conflict`); `flow_status` always returns `state_error` (null when fine) and both `fix_rounds` keys; a `read` name resolving outside the flow directory is `invalid_argument`; a bare `Readiness: BLOCKED` with no reason is Malformed; `package_hash` framing = `name\0` + `+` + u64-LE length + LF-normalized bytes, or `-` when absent (unambiguous for empty/absent/NUL); `flow_start` reports a failed log append as `log_error` (state already committed), `flow_advance` refuses instead (log is appended before `STATE.md` is replaced).
5. **Gotchas**: `transact_atomic` reads before its closure, so it cannot create a file — the first `STATE.md` uses `write_new_atomic` (`AlreadyExists` arrives as `SexpError::Io`); `design_state_hash` already skips `*.lck`; unit tests call handlers directly, so 1.9 must dispatch through `ToolRouter`; in Git Bash never pass a `//`-prefixed marker as argv (MSYS rewrites it to `/`).

## Deferred findings

1. `design_hash.rs`'s `normalize_eol` is private, so `flow.rs` carries a copy; a shared text-normalization helper would remove it (same drift review as the architect's `now_rfc3339_utc` note).
2. `flow_start`'s `STATE.md` write and its log append are not one unit: if the append fails, the job is open and only `log_error` says the start went unlogged.
