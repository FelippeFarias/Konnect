---
change: konnect-orchestrator
task: tasks 1.5-1.7 — flow_advance rewind/abandon + D6 evidence_check, flow_gate, flow_log, flow_defer
agent: developer
verdict: DONE
failing_layer: null
---

## Result

1. Three commits on `orc/konnect-orchestrator` (worktree `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\konnect-orchestrator`, on top of `ac389d7`), not pushed: `b457251` 1.5 rewind/abandon + D6 `evidence_check`, `68ce5d8` 1.6 `flow_gate`, `71d2cea` 1.7 `flow_log` + `flow_defer`.
2. Only file: `crates/konnect-core/src/tools/flow.rs`. `flow::tools()` now publishes all six tools in D1 order (pinned by `the_published_schemas_accept_the_documented_calls`); still NOT registered (task 1.8).
3. 18 new tests (56 `flow::` total): `rewind_tests` 4, `gate_tests` 8, `journal_tests` 6, plus the schema test extended to `flow_gate`/`flow_log`/`flow_defer` and the vocabulary test to `GateDecision`/`ApprovedBy`. Every acceptance line of 1.5–1.7 has a test named after it.
4. Shared plumbing added: `transact_state` + `load_job` (every mutating tool except `flow_start`), `record_gate_keys` (package keys on ANY entry into a gate, forward or rewind), `commit_transition` (log/push/render tail), `changed_package_files`, `append_file`, `blockquote`.

## Evidence

1. `cargo test -p konnect-core flow::` (HEAD `71d2cea`) → `test result: ok. 56 passed; 0 failed; 0 ignored; … 1401 filtered out`, exit 0.
2. `cargo fmt --check` → exit 0; `cargo clippy -p konnect-core --all-targets` → `Finished`, 0 warning/error lines, exit 0.
3. `git diff --name-only ac389d7..HEAD` → `crates/konnect-core/src/tools/flow.rs` only; `git log --oneline ac389d7..HEAD` → the three commits above (per-commit stat 537+/40-, 883+/35-, 883+/6-).
4. Mutation proofs (mutate → run → restore in one script, each `restored byte-identical: True`, then `cmp` against a pre-mutation copy → PRISTINE), 23 mutations, 23 caught: 1.5 — target gate not cleared, later approvals kept, rewind-into-gate records no keys, reason not required, records allowed on a rewind, `any`→`all` ok-status rule, forward stores no check; 1.6 — guided without words, purchase by the session, readiness not re-checked, design key / package key not compared to the gate entry, `approved_by` always user, visit off by one, reject keeps the approval, any phase accepts a decision; 1.7 — decision without why / rollback, scope on any kind, handoff always 01, project lessons to candidates, `queue_item` into findings, and a defer read-modify-write outside the lock (fails `left: ["finding one"]` vs `right: ["finding one", "finding two"]`).
5. Wider runs at `71d2cea`: `cargo test -p konnect-core` → lib `1441 passed; 0 failed; 16 ignored`, conformance 4, integration 12, photo_intake_gate_e2e 5 (+1 ignored), prepare_board_photo contracts 6 + 1, two_checkpoint 3 — exit 0; `cargo test -p konnect --test schema_parameter_usage --test doc_tool_counts --test asset_references --test not_tools_allowlist` → 5 / 6 / 13 / 3 passed, exit 0.

## For the next agent

1. **1.8 registry**: `schema_parameter_usage` iterates only registered tools, so `flow` enters it at 1.8. Its transitive literal walk follows my helper calls (`log_request` → `opt_text`/`opt_token`, `parse_records`, `opt_str_list`), and no new signature contains `;`, so every declared parameter should read as used. Nested object nodes: only `flow_advance.records.items`, which declares `properties` → `fixed_records_are_closed…` should need no allowlist entry.
2. **1.9 e2e, schema traps**: `flow_gate` REQUIRES `user_words` in the schema (it may be `""`); `summary` must be non-blank. A rewind needs `reason` and no `records`. `flow_advance` reads `ctx.observer.recent(0)` only when `evidence_calls` is non-empty, so through `ToolRouter` the evidence check sees the real ring (the flow calls themselves are recorded only after they return).
3. **1.9 chain semantics**: a rewind clears the approvals of gates at OR after its target. So after rewinding to `architecture`, the old `gate:architecture` approval is already gone, and re-entering the gate writes new keys on the new entry. Leaving without re-approval is `stale_target` "no approval recorded during this visit", which is the "earlier approval unusable" package case. `flow_gate` approve compares against `history[current_visit]` (the entry that entered the gate, forward or rewind), never now-vs-now.
4. **Response shapes** (for e2e assertions): `flow_advance` → `{job_id, transition, from, phase, records_written, design_hash, package_hash, evidence_check, cleared_approvals, history_index}`; `flow_gate` → `{job_id, gate, decision, phase, approved_by, approved_at, visit, design_hash_at_approval, package_hash_at_approval, removed_approval, gate_file}`; `flow_log` → `{job_id, kind, at, read_name, file}` (`read_name` is what `flow_status(read)` takes); `flow_defer` → `{job_id, kind, list, count, item}`.
5. **Decisions the diff doesn't spell out**: I followed D1 literally where it left a choice. `flow_gate` on a phase other than the gate, a closed job included, is `invalid_argument` (field `gate_name`); `flow_advance` keeps `stale_target` for closed. A blocked readiness at approval uses field `decision`. `role` is allowed as an optional author tag on decision/evidence. Blank optional strings count as absent. Handoff NN = highest existing + 1 (equals D1's count + 1 unless a file was deleted, and it never collides with the no-clobber create). `evidence_check` dedupes repeated names and is stored on rewinds/abandons too.

## Deferred findings

1. `flow_gate` writes `records/gates/<gate>.md` and appends the log before `STATE.md` is replaced (D2 order). If that final replace fails, the gate file says "approve" while `STATE.md` holds no approval. Only `STATE.md` counts, but the file misleads a human reader. Unlike records, D2's "re-supplied on retry" argument does not cover it.
2. `flow_status`'s `gate_validity` recomputes `valid` from the hashes only, not the visit. An approval from an earlier visit would show `valid: true` although `flow_advance` refuses to leave on it. Only a hand edit can produce one, since a rewind clears it.
3. `render_body`'s history table omits a rewind's `reason` and the `evidence_check`; both are in the front matter and the log only.
