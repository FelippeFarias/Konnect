---
change: konnect-orchestrator
task: 3.5 — confirmed-row rule in the texts the architecture agent loads; README bundled count
agent: developer
verdict: DONE
failing_layer: null
---

## Result

1. One commit, `84f7e0f`, on `orc/konnect-orchestrator` (worktree `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\konnect-orchestrator`, parent `4cdb673`). Not pushed. It touches 3 files, +30/-6.
2. `kicad-architecture-agent.md` (body only). The Step 6 bullet "catalogue stock is discovery … re-checked before payment" is gone. The new text: a row is confirmed only by the session's `flow_log(project_dir, job_id, kind, message)` entry with `kind` `evidence` (live part page plus the manufacturer's datasheet). The agent reads `log` via `flow_status(project_dir, read)` when the brief names it. Catalogue-only stock or `located, not validated` keeps `Readiness: BLOCKED — …`. The PASS bullet now points to Step 6. A new readiness bullet, "An unconfirmed row is a BLOCKED reason", makes the agent name each row (ref + MPN) in the reason and in Questions, return BLOCKED, never advance, and never mark a row confirmed itself.
3. `kicad-architecture/SKILL.md`. The one removed line is the PASS sentence's tail. It is rewritten to "every parts-list row is confirmed (next bullet)". A new bullet follows it with the same rule, citing the konnect skill's `references/orchestration.md` §9. The wording mirrors `architecture-record-schema.md` "A confirmed row".
4. `README.md:345`: `10 skills + 5 agents` → `12 skills + 11 agents`. This comes from `manifest.rs`: 12 `SkillManifest {` entries (lines 34–214, not counting the struct at :9) and 11 `AgentManifest {` entries. The 4 `HookSkillManifest` entries are not counted, same as the old figure of 10.

## Evidence

1. SC1: `grep -c "located, not validated"` gives SKILL.md 1 and agent 1. In the agent, `grep -c "before payment"` = 0 and `grep -c "discovery"` = 0.
2. SC2: frontmatter compared as bytes (`git cat-file -p 4cdb673:<agent>` vs the working file, split on `---\n`) → `frontmatter byte-identical: True` (586 bytes). `cargo test -p konnect --test asset_references` → `13 passed; 0 failed`, exit 0. `cargo test -p konnect --test doc_tool_counts` → `6 passed; 0 failed`, exit 0.
3. Proof that the guard covers the new prose: in SKILL.md's new bullet I changed `read` to `reed` → `call_examples_name_real_parameters` FAILED with "SKILL.md:182: flow_status(…) names `reed` … Has: {"project_dir", "read"}". Mutation, run and restore happened in one call → `restored byte-identical: True` (cmp).
4. SC3: `git diff --name-only 4cdb673..HEAD` lists exactly `README.md`, `crates/konnect/assets/agents/kicad-architecture-agent.md` and `crates/konnect/assets/skills/kicad-architecture/SKILL.md`. `git rev-list --count 4cdb673..HEAD` = 1, and `git status --short` is empty.

## For the next agent

1. The agent no longer cites the kicad-manufacture skill's `references/jlcpcb-rules.md` §2. That citation backed the "re-checked before payment" clause I removed. The purchase-gate re-check itself still lives in the manufacture agent and in orchestration.md §4, so nothing is lost. No guard required that citation.
2. Both assets are LF in the index and in the working tree, and Edit kept them LF. README is CRLF in the working tree; I patched it byte-wise with Python (1 replacement).
3. I kept each `flow_log(...)` / `flow_status(...)` example on one line so `signature_examples` checks them. It scans only one line at a time.

## Deferred findings

1. No marker guards the new rule. `skills_define_the_same_evidence_boundary_as_their_agents` and `agents_make_claimed_evidence_executable` use hard-coded per-file marker lists that do not include `located, not validated` for the kicad-architecture pair, so a later edit could drop the rule without any test going red. Fix: add that marker (and perhaps `kind` `evidence`) to the architecture case in `crates/konnect/tests/asset_references.rs`. Owner: test-hygiene backlog. The test file was outside this FS.
2. No guard checks README's bundled skills/agents count (`doc_tool_counts` counts tools only), so it will go stale again on the next manifest change. Owner: test-hygiene backlog (derive it from `SKILLS.len()`/`AGENTS.len()`).
