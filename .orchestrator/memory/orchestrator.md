# orchestrator — project memory

## Current state
- `photo-to-kicad-reverse` + `board-dossier-reconstruction` merged into `main` at `bbd5efb` (one `orc merge board-dossier-reconstruction`, stacked, carries both); both archived.
- `konnect-orchestrator`: 50/50 tasks, 3 verify rounds (2 FIX + 1 DONE); archived 2026-09-22 as `2026-09-22-konnect-orchestrator` at commit `9a0f1ce` on `orc/konnect-orchestrator` (history `bbd5efb..9a0f1ce`). Merge to `main` pending the user's GRANT (`orc state grant-merge konnect-orchestrator`).
- 3 ARCHIVE records since the last RETRO (cadence 5) — no retro due yet.

## Decisions affecting my role
- codex CLI 0.144.6 fails on model `gpt-6-astra` (`turn.failed`: requires a newer Codex CLI) and
  rejects the `--search` flag from `AGENT.md`'s `codex_flags`. Fix used: reroute the codex-executor
  role(s) to a claude general-purpose agent with WebSearch/WebFetch, injecting that role's own
  `AGENT.md` as its prompt. Upgrading codex globally is Tier 2 — not done mid-change.
- `orc state append-history` rejects a message with a leading `"- "`.
- `orc status --change <name>` does not exist. Use `orc validate --changes <name> --json` (backed
  by `openspec validate --changes <name> --json`; `openspec status --change <name> --json` also
  works). Run all of these from the Konnect repo root — from inside the change directory they fail
  with "Change not found".
- Planning artifacts (`openspec/changes/<name>/`, `.orchestrator/`) live in the MAIN checkout, not
  the worktree; code edits happen in the claimed worktree
  (`C:/Users/felip/.orc/worktrees/<hash>/<change>`, branch `orc/<change>`). The two are separate
  git states — don't assume the worktree branch sees the main checkout's untracked scaffolding.
- Registering a new toolset trips Konnect's `doc_tool_counts` guards repo-wide: bump counts in
  `README.md`, `DEV.md`, `tool-directory.md`, `docs/TROUBLESHOOTING.md`, `packaging/metadata.json`,
  and `plugin/plugin.json` together (this change: 233->238 tools, 226->231, 21->22), or the guard
  fails on a stale total. Add this file set as its own task if the planner didn't already.
- Konnect's asset guard `NOT_TOOLS` (`crates/konnect/tests/asset_references.rs`) must list
  response/map field names that collide with the tool-name grep — never a top-level tool input
  parameter, those are exempted separately by the schema-properties collector.
- `cargo test`/`cargo clippy` for `konnect-core`/`konnect` need `PROTOC` and the VS 2022 BuildTools
  CMake `bin` on `PATH` (kicad-cli's build dependency).
- `.venv-retrace` (Python 3.12) in the Konnect repo root holds a working retrace 0.3.0 install
  (base only, no `ultralytics`/`easyocr`) for live tests via
  `RETRACE_PYTHON=<repo>/.venv-retrace/Scripts/python.exe -- --ignored`.
- Stacking a new change on an unmerged predecessor: `git checkout -B orc/<new-change> <predecessor-head-sha>`
  inside the claimed worktree, done at session start before any planning artifact exists. This
  is the only way to proceed on an extension before the predecessor's PR is merged; the rollback
  is a rebase onto main once that merge lands. Say so explicitly as a DECISION with rollback, not
  a silent branch-point choice.
- A hybrid acceptance run (drive the real MCP binary over stdio to score a design checklist
  against real inputs) needs its OWN process — the session's already-open MCP connection is
  almost always the OLD binary from before the change's build. Build worktree HEAD in release,
  launch it fresh as a separate stdio child (a scratch Python client is fine), and never assume
  the live session tool list reflects uncommitted code.
- That scratch MCP client's background daemon thread printing a stack trace to stderr on process
  exit (pipe closed under it) is harmless noise, not a failure signal — check the actual
  tool-call results/exit code, not stderr content, when judging the run.
- `orc state append-history <message>` takes the message with NO leading `- ` (the CLI adds its
  own list marker); a message starting `"- "` is rejected.
- codex CLI 0.144.6 is still unavailable for this project as of this change too (same failure as
  `photo-to-kicad-reverse`'s entry above) — the claude-general-purpose-agent reroute remains the
  standing workaround; re-check only after a Tier 2 codex upgrade actually happens.
- `orc merge`'s main-clean check blocks on ANY untracked path it doesn't recognize, not just foreign work: it hit orc-init's own `.claude/` (agents/commands/skills + settings.json — `generatedRuntimeFiles` doesn't cover `.claude/agents/orc-*.md`) and a local Python venv (`.venv-retrace/`) alongside the already-known `.venv-kb/`/`.kb-pilot/` pattern. Fix: append such local-only dirs to `.git/info/exclude` (never `.gitignore`, which would ship it) and park the change's OWN `openspec/changes/<name>/` planning dir in the session scratchpad for the merge's duration, moving it back right after.
- Stacked changes merge with ONE `orc merge <top-change>`: it rebases onto the stack's declared base and carries every predecessor's already-rebased commits along, so a single grant covers the whole stack (confirmed: `orc merge board-dossier-reconstruction` also landed `photo-to-kicad-reverse`'s commits).
- When one file (or one whole toolset) is too large for a single delegation's research/token budget, split the round into sequential sub-deliveries that each touch that same file/area (e.g. tasks 1.1-1.4 then 1.5-1.7 then 1.8-1.10) rather than one oversized delegation — a smaller, single-purpose diff is what actually stays verifiable against its acceptance criteria; re-run the full targeted test module after each sub-delivery, not just at the end.

## Gotchas found here
(none beyond the decisions above — the reliability-contract.md "missing file" mistake this change hit twice is a Konnect code fact, kept in `developer.md`'s Gotchas; the generalized verification habit is in the QA global `MEMORY.md`.)
