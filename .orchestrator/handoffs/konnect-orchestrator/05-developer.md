---
change: konnect-orchestrator
task: tasks 1.8-1.10 — register flow, published-contract e2e, documented counts
agent: developer
verdict: DONE
failing_layer: null
---

## Result

1. Three commits on `orc/konnect-orchestrator` (worktree `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\konnect-orchestrator`, on top of `71d2cea`), not pushed: `f79bf80` 1.8 register `flow`, `0a486b4` 1.9 `tests/flow_gate_e2e.rs`, `b978aa5` 1.10 counts + `tool-directory.md` section + `SKIP`.
2. Registry after 1.8, read from the test output (not copied from D12): **23 toolsets, 238 registered tools, 245 with the 7 meta-tools, 11 categories** (new category `orchestration`). `flow` is the last `ALL_TOOLSETS` entry, after `manufacturing`, `tool_count: 6`.
3. 1.8: `flow_exposes_exactly_its_six_tools_in_order` (`router/mod.rs`, after the photo_intake test) pins names + order through `tools_for` AND `ToolRouter::load`, `find_toolset_for_tool` for all six, and `category == "orchestration"`. The `fixed_records_are_closed…` allowlist is unchanged.
4. 1.9: 3 tests. (a) the loaded schemas refuse a missing `user_words`, an extra key on `flow_gate`, and an extra key in a `records[]` item. (b) The design chain from the task text. (c) The package case, plus one step the task did not name, explained in "For the next agent" 4.
5. 1.10: counts bumped in README (2 lines), DEV (4), tool-directory (3 + the category count by hand), TROUBLESHOOTING (1), packaging/metadata.json (2), plugin/plugin.json (1). New `## Orchestration` / ``### `flow` · 6 tools`` section before the Appendix. `doc_tool_counts.rs`: only `".orchestrator"` + `"archive"` in `SKIP` and a 3-line comment (rustfmt reflowed the array vertically).

## Evidence

1. `cargo test -p konnect-core router::` → lib `19 passed; 0 failed` (incl. `registry_tool_counts_match_reality`, `fixed_records_are_closed_and_only_reviewed_maps_are_extensible`, the new ordered test), exit 0. RED before the registry edit: the new test panicked on `expect("flow is registered")`.
2. `cargo test -p konnect-core --test flow_gate_e2e` → `3 passed; 0 failed`. `cargo test -p konnect --test doc_tool_counts` → `6 passed`. `cargo test -p konnect --test schema_parameter_usage` → `5 passed` (it iterates `ALL_TOOLSETS`, so it now covers `flow`). `grep -c "flow_status\|…\|flow_defer" tool-directory.md` → `6`.
3. `cargo test -p konnect-core` → lib `1442 passed; 0 failed; 16 ignored`, conformance 4, flow_gate_e2e 3, integration 12, photo_intake_gate_e2e 5 (+1 ignored), prepare_board_photo 6 + 1, two_checkpoint 3, exit 0. `cargo test -p konnect` (whole crate, incl. asset_references 13, not_tools_allowlist 3) exit 0. `cargo fmt --check` exit 0. `cargo clippy --all-targets` → `Finished`, 0 warnings, exit 0.
4. Mutation proofs: 9 mutations, 9 caught. Each one was mutate → run → restore in one script, `restored byte-identical: True`. `flow.rs` was never touched. 1.8: category → `manufacturing` fails the new test (`left: "manufacturing"`); `tool_count` 5 fails `registry_tool_counts_match_reality` and `load_all…`. 1.9: dropping the `.kicad_pro` edit fails "must be refused"; an extra `approved_by` key in the chain makes the harness panic `flow_gate rejected its own arguments at the schema: Additional properties are not allowed`; dropping the hand edit fails the package-only refusal; re-advancing the unchanged architecture fails `assert_ne` on the package hash. 1.10: dropping `"archive"` flags `openspec/changes/archive/…/tasks.md:123: says "22 toolset(s)"`; heading `· 5 tools` and an unbackticked `flow_defer` each fail their sweep.
5. `git diff --name-only 71d2cea..HEAD` → the 10 FS files only. Per-commit stat: 2 files +42; 1 file +524; 7 files +48/-15.

## For the next agent

1. **Names the skills/agents prose may quote.** Toolset: `flow` (load it with `load_toolset("flow")`). Tools in order: `flow_status`, `flow_start`, `flow_advance`, `flow_gate`, `flow_log`, `flow_defer`. Top-level parameters, all of which `snake_words` accepts: `project_dir`, `read`, `objective`, `lane`, `phases`, `mode`, `job_id`, `to_phase`, `records`, `evidence_calls`, `reason`, `gate_name`, `decision`, `summary`, `user_words`, `kind`, `message`, `why`, `rollback`, `role`, `scope`, `description`, `owner`. Nested `filename`/`content` are NOT top-level. Enum values such as `new_board`, `queue_item`, `pending_approval`, `schematic_review` and `prefab_review` are not parameters, so each 2+-part snake word among them needs a `NOT_TOOLS` entry or rewording (task 6.2).
2. **Signature examples are schema-checked.** `flow_gate(…)` must name `user_words`, since the schema requires it even when it is `""`. `flow_log(…)` needs `project_dir, job_id, kind, message` (task 2.3's acceptance). A rewind is `flow_advance` with `reason` and no `records`.
3. **Doc counts after this round:** 23 / 238 / 245 / 11 categories. Any new `.md`/`.json` outside `.claude`, `.orchestrator` and `archive` is swept, so never put a digit directly before "toolset" or a stale three-digit number before "tools". `.orchestrator` was added to `SKIP` but could not be exercised in the worktree, which has no such directory. It will first bite in the main checkout after the merge.
4. **Why the e2e package case has an extra step.** After a rewind, the earlier approval is already deleted, so the refusal to leave comes from the visit rule. A broken `package_hash` would stay green. So after the re-approval the test hand-edits `records/architecture.md`, with the same visit and the same design, and asserts `the package of gate "architecture" changed … architecture.md`. It then restores the approved bytes and leaves. That makes pre-mortem #2 (tasks 1.1/1.6/1.9) executable through the published surface.
5. **Gotcha:** `cargo fmt` writes a file it actually reformats back with LF only, and leaves untouched files CRLF (no `rustfmt.toml`). That is harmless because the index is LF, but a CRLF-matching patch script that runs after `cargo fmt` will miss. Re-detect the file's line endings each time. The e2e dispatches through `ToolDef.handler` directly, so the observer ring stays empty: a future e2e that passes `evidence_calls` will see `evidence_check` report them missing.

## Deferred findings

1. The README/DEV token estimates (`~23K` for the full listing, `~25K` for the full catalogue) are not swept and were not re-derived for the six new tools, which add roughly 0.6K. Separately, `tool-directory.md`'s appendix sentence listing the cross-cutting categories does not mention orchestration. Neither is a count line, so both were left alone.
