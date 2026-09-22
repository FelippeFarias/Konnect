---
change: konnect-orchestrator
task: 9.7 — DECISION N text polish (reviewer 21 MINORs 1 and 3)
agent: developer
verdict: DONE
---

## Result

1. Commit `8b17a76` on `orc/konnect-orchestrator`, on top of `9e848d3`. It changes only the two FS files. Not pushed.
2. `crates/konnect/assets/agents/kicad-manufacture-agent.md:187-189`: one new sentence right after the INCOMPLETE clause's closing "…still open." and before "An adjudicated DRC or preflight warning…": "A DRC or preflight run that did not complete — for example schematic parity not checked — is an open item that keeps the package from leaving the phase, never a warning to adjudicate away." Frontmatter is untouched.
3. `tool-directory.md:507/508/510`: `flow_advance` now says "A validation refusal writes nothing." All three rows (`flow_advance`, `flow_gate`, `flow_defer`) now say "A success always carries `warning`: null, or a string naming … not written after `STATE.md` committed — the move/decision/item stands/is recorded, so never repeat the call." The wording is copied from each tool's published description (`flow.rs:3178-3180`, `:3234-3236`, `:3329-3331`). Each row is still one line.
4. No test file, no `openspec/**` file and no `tasks.md` changed.

## Evidence

1. Baseline before the edit: `did not complete`=0, `never repeat the call`=0, `A refusal writes nothing`=1. After: **1 / 3 / 0** (SC1).
2. `cargo test -p konnect --test asset_references` gives **24 passed**. `cargo test -p konnect --test doc_tool_counts` gives **6 passed**. `cargo test -p konnect-core --test flow_fix_round_two_texts` gives **2 passed** (SC2). The new sentence contains "DRC", and `manufacture_verdict_permits_exit_with_adjudicated_drc_warnings` stays green. That guard fails if "DRC" appears inside the INCOMPLETE clause, so its passing proves the sentence sits outside that clause.
3. `git diff --name-only 9e848d3..HEAD` lists `crates/konnect/assets/agents/kicad-manufacture-agent.md` and `tool-directory.md`, in one commit, `8b17a76` (SC3).
4. `grep -n '^| \`flow_(advance|gate|defer)\` |.*|$'` matches rows 507, 508 and 510, so each row still opens and closes on the same line.

## For the next agent

1. First draft rejected: I wrote the parenthetical "(a null `drc.schematic_parity`)". `backticked_tool_names_in_prose_exist_in_the_registry` failed with "`schematic_parity` reads like a tool but is in no toolset". I dropped the parenthetical, so the sentence now matches the brief's text word for word. No guard was changed.
2. I did not add the reviewer's optional §4 "Show"-row mention (`orchestration.md:116`). It is not part of task 9.7.
3. Reviewer 21 MINOR 2 (`design.md:626-628`, "a DRC error always blocks") is not in task 9.7 and stays open. That file is under `openspec/**`, which this brief did not let me edit.
4. Git warns "LF will be replaced by CRLF" on the agent file. That is the repo's autocrlf setting, and the diff is 3 content lines only, with no line-ending churn.

## Deferred findings

1. Reviewer 21 MINOR 2 (design.md contradicts amended DECISION I on DRC-error waivers) still needs an owner, because it sits in `openspec/**`.
