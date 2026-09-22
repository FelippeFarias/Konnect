---
change: konnect-orchestrator
task: 9.6 (DECISION M) — three text alignments, one commit
agent: developer
verdict: DONE
---

## Result

1. `e2228f9` on `orc/konnect-orchestrator` (parent `8965f00`). It touches only `tool-directory.md` (+1/-1) and `crates/konnect/assets/agents/kicad-manufacture-agent.md` (+4/-4). Nothing was pushed.
2. **(a)** `tool-directory.md:505`, the `flow_status` row, stays on one line. "gate approvals with a recomputed `valid`" now reads "gate approvals (`valid` computed only for the gate the job stands at now; a passed gate reports `status: passed`)".
3. **(b)** The agent's frontmatter `description` now reads "…ending in NOT READY or INCOMPLETE; READY is declared by the orchestrating session after the purchase approval. Triggers: …". The Triggers and Anti-triggers text is unchanged, and the `name`/`model`/`skills`/`tools`/`maxTurns` lines are untouched.
4. **(c)** The Ending-the-run exit sentence (now `:216-218`, not `:205-207`) says "(an artifact warning, a check that did not run or failed, a missing artifact)". I reflowed the paragraph so "an artifact warning" sits on one physical line.

## Evidence

1. SC1 greps, run in the worktree:
   - `grep -c "recomputed \`valid\`" tool-directory.md` = 0.
   - `grep -c "only for the gate the job stands at now" tool-directory.md` = 1.
   - The agent's first 12 lines contain `ending in READY` 0 times.
   - `git diff -U0` on the agent shows no `+`/`-` line starting `name|model|skills|tools|maxTurns|  - ` (grep exit 1).
2. SC2: `cargo test -p konnect --test asset_references` gave 22 passed, 0 failed. These include `orchestrator_agents_keep_their_contract`, `manufacture_agent_verdicts_match_the_skill` and `manufacture_verdict_permits_exit_with_adjudicated_drc_warnings`. `cargo test -p konnect --test doc_tool_counts` gave 6 passed, 0 failed. No test file was edited.
3. Extra check: `install::tests::manifest_ships_every_asset` passed (1 passed).
4. SC3: `git diff --name-only 8965f00..HEAD` lists exactly those two files, and `git rev-list --count 8965f00..HEAD` = 1. The added lines have 0 hits for a digit before "toolset" or a three-digit number before "tools".

## For the next agent

1. No guard pinned the old text. The Ending-the-run marker `"A \`NOT READY\` package, or an \`INCOMPLETE\` one with any other open item"` (`asset_references.rs:2091`) still matches after the reflow, because `missing_markers` compares flattened text.
2. I wrote (a) as a parenthesis with a `;` rather than an em-dash clause. The row is a comma-separated list, and a clause containing commas would have run into the next list item.
3. Nothing guards the new texts in (a), (b) or (c). A later reword would pass silently. If VERIFY wants guards, adding them is QA's job.
4. EOLs: the agent file's index and working copy are both LF, and `tool-directory.md`'s working copy is CRLF. The commit preserves both.

## Deferred findings

(none)
