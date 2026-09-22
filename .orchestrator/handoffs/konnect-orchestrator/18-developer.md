---
change: konnect-orchestrator
task: fix round 2 — tasks 9.1, 9.2, 9.3+9.4 (one commit), 9.5 regression
agent: developer
verdict: DONE
---

## Result

1. `5fa099d` **9.1** (`flow.rs`, strings only). The `flow_status` description and the `STATE.md` body sentence now say `valid` is recomputed "only for the gate the job stands at now". The descriptions of `flow_gate`, `flow_advance` and `flow_defer` each name `warning` and "never repeat the call". `flow_advance`'s description now says "a validation refusal writes nothing".
2. `b1bc45a` **9.2** (`orchestration.md` §4). Added the READY-ownership bullet between the purchase-checks bullet and the rejection bullet, and replaced the "reports every approval's validity" sentence. Both texts match the task's wording once whitespace is normalized (checked by script).
3. `8965f00` **9.3 + 9.4** in one commit.
   - The agent's Verdict bullet is replaced word for word, and the pointer sentence is added under "Ending the run".
   - Guard `manufacture_agent_verdicts_match_the_skill`: the marker now reads "every artifact check", with two new markers, a second stale phrase, and a new Ending-the-run marker.
   - New test `manufacture_verdict_permits_exit_with_adjudicated_drc_warnings`.
4. **9.5**: the full battery was green at `8965f00` with no fix needed, so there is no commit. `git diff --name-only 1c05a85..HEAD` lists exactly the 4 files in scope. `kicad-manufacture/SKILL.md` and the three flow e2e files have no diff. Nothing was pushed.

## Evidence

1. **9.1 greps** on `flow.rs`: `only for the gate the job stands at now` = 2, `never repeat the call` = 3, `a validation refusal writes nothing` = 1. The flow suites needed no change and passed: `--lib flow::` 61 passed; `flow_contract_e2e` 11, `flow_fix_round_one_e2e` 4, `flow_gate_e2e` 4.
2. **9.2 greps** on `orchestration.md`:
   - `records the package as \`READY\` in the skill's sense` = 1 in the whole file, and 1 inside `## 4. Gates`…`## 5.`.
   - `reports every approval's validity` = 0.
   - `only for the gate the job stands at now` = 1.
   - `orchestration_reference_keeps_its_protocol_rules` and `orchestration_reference_states_the_fix_round_one_rules` both pass.
3. **9.3 / 9.4 greps** on the agent:
   - `every artifact check your tools can run passed` = 1, inside the Verdict bullet.
   - `every DRC warning and every preflight issue is adjudicated` = 1.
   - `and only the purchase-gate checks remain` = 0.
   - `the orchestrating session declares that` = 1, inside `### Ending the run`…`### Hard rules`.
   - On `asset_references.rs`: `left an artifact missing, or passed with a warning` = 2.
   - RED first: before the agent edit, both tests FAILED and named every missing phrase.
4. **Mutation proof** (`scratchpad/mutate.py`): one script swapped only the Verdict bullet back to the pre-9.3 text, ran the two tests, then restored the file in `finally`.
   - Both tests FAILED (exit 101).
   - The guard's failure: `again says \`every check your tools can run passed with no warning\`` plus the 3 lost markers.
   - The new test's failure: the "not an open item" sentence is missing, the READY clause lost both adjudication phrases, the NOT READY clause has no unwaived DRC error, and the INCOMPLETE clause is "no longer scoped to artifact checks".
   - After restore, sha `af103ccc…` matched before and after (`byte-identical: True`).
5. **Full battery at `8965f00`**:
   - `cargo test -p konnect`: 146 passed, 0 failed across 15 suites (`asset_references` 22).
   - `cargo test -p konnect-core`: 1497 passed, 0 failed across 11 suites.
   - `cargo fmt --check` exited 0; `cargo clippy --all-targets` exited 0 with no warnings.
   - The digit-before-"toolset" / three-digit-before-"tools" rule has 0 hits in the added `.md` lines.

## For the next agent

1. **VERIFY round 3 should check this first.** Walk the ecc83-pp shape (0 DRC errors, 17 warnings) through the new Verdict bullet together with `:205-207` ("an `INCOMPLETE` one with any other open item (a warning, …) is not an exit"). That older sentence was left as it is (out of scope). Its "a warning" now reads correctly only through the new "An adjudicated DRC or preflight warning is not an open item". Decide whether that is clear enough.
2. **The new test checks clauses, not just markers.** It splits the flattened Verdict at the next `; ` or `. ` after each verdict word and asserts:
   - INCOMPLETE starts with "an artifact check" and names neither DRC nor the preflight;
   - NOT READY names "an unwaived DRC error" and no "DRC warning".
   Adding a semicolon inside a clause will break the split. The failure message prints the clause, so this is easy to spot.
3. **Correction in 9.1.** The `flow_advance` text names only "the log entry", not "the gate file": `commit_transition` builds `Committed { gate_file: None }` (`flow.rs:~2353`). §7's generic "(the gate file or the job's log entry)" applies only to `flow_gate`.
4. **Wording in the agent.** The agent carries the skill's full quote ("Any warning or missing requested artifact type keeps the result `INCOMPLETE`") on one physical line. It replaces the old one with an ellipsis, and the new test pins it.

## Deferred findings

1. `tool-directory.md:505` (outside this task's files) still says "gate approvals with a recomputed `valid`" for `flow_status`. It is the same residue DECISION K removed from `flow.rs`, and no guard checks it.
2. The agent's frontmatter description (`kicad-manufacture-agent.md:3`) still says the run ends "in READY, NOT READY or INCOMPLETE", but the agent never emits READY by itself. Reviewer 16 noted this under minor 2. 9.3 did not touch it.
