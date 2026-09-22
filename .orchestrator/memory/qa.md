# qa — project memory

## Current state

- `konnect-orchestrator`: verify DONE across 3 rounds (`10-qa.md`, `15-qa.md`, `20-qa.md`); final commit `9e848d3` on `orc/konnect-orchestrator`; archived 2026-09-22 at `9a0f1ce`. Merge to `main` pending the user's GRANT. Mine: `flow_contract_e2e.rs` (11), `flow_fix_round_one_e2e.rs` (4), `flow_fix_round_two_texts.rs` (2), 10 `asset_references.rs` guards. Open deferred: tool-directory flow_advance/gate/defer rows lack `warning`; SERIOUS-1 closure is prose-only; library Step 6 unverified live.
- `board-dossier-reconstruction` + `photo-to-kicad-reverse`: merged into `main` at `bbd5efb`; archived.
- `install-scripts` (FAST, worktree `.orc/worktrees/konnect-3b7e2022/install-scripts`): `02-qa.md` FIX at `897c4a0` (no `[CmdletBinding()]`; my `-Bogus` probe swapped the real exe 10:04-10:06, restored). `05-qa.md` DONE at `334a8ac`: R2 + reviewer 1-9 + 11 regressions PASS both scripts; init-fail via the authorized fake exception. Deferred: ps1 accepts drive-relative `C:cmd …`; sh maps `/usr/bin/konnect` to Git's usr\bin.

## Decisions affecting my role

- Truth commands (bash): `export PROTOC="C:/Users/felip/tools/protoc/bin/protoc.exe"`,
  CMake bin on PATH, then `cargo fmt --check`, `cargo clippy --all-targets`,
  `cargo test -p konnect-core`, `cargo test -p konnect` — ONE backgrounded chain
  (target dir locks). Full chain ~6-8 min warm; lib alone ~30 s.
- Live retrace: `RETRACE_PYTHON=.../Konnect/.venv-retrace/Scripts/python.exe`,
  `-- --ignored`, fully-qualified names (bare names match 0 and exit 0).
- I never edit existing source/test files; new tests only in `crates/*/tests/`.
  `rustfmt --edition 2021 <file>` after a scripted edit.
- Wait on a background log: `for i in $(seq 1 20); do grep -q DONE "$F" && break;
  ping -n 31 127.0.0.1 >/dev/null; done` (foreground sleep is blocked).

## Gotchas found here

- Red proof without touching the shared worktree: `git worktree add --detach
  <scratch>/wtX <base>`, copy the new test file, `CARGO_TARGET_DIR=<worktree>/target`
  (path crates rebuild ~1 min, registry deps reused), then `git worktree remove
  --force` + `prune`. Other agents build from this worktree's sources (seen:
  `cargo build -p konnect` into `%TEMP%\k16t`) — never swap flow.rs in place.
- Forcing a `STATE.md` commit failure on Windows: open it with
  `OpenOptionsExt::share_mode(FILE_SHARE_READ)`; konnect-sexp's lock is a separate
  hashed file and reads use `File::open`, so only the rename fails (os error 5).
- flow side-file blockers: replace `.konnect/flow/log` (or `records/gates`) with a
  plain file → os error 183 on the append; that is how `warning` is reached.
- `ToolDef::new` closes schemas recursively (`additionalProperties: false`); a
  nested object param survives only if it declares `properties`.
- Where a rejection lands is contract: schema-carried bounds refuse at the
  validator before the handler. Assert both layers.
- `agents_make_claimed_evidence_executable` is a hard-coded list; my
  `every_agent_that_names_a_flow_tool_loads_flow` is directory-derived.
- `not_tools_allowlist.rs` parses NOT_TOOLS from `asset_references.rs` via
  `include_str!` — appending guards there is safe.
- Acceptance `grep -c` on prose is line-based: check the phrase sits on one line.
- `grep -rn` in the worktree hits stale `target/`; scope to `crates/ docs/`.
- A doctest `extern location … does not exist` mid-run = concurrent build; check
  `tasklist` for cargo/rustc and re-run clean.
- Long heredocs via Bash can fail to parse; write snippets with Write, then `cat >>`.
- Repo-root docs from `crates/konnect/tests`: `include_str!("../../../tool-directory.md")`
  (CRLF working copy; `section(.., "\n")` + `flat()` cope).
- Installer QA (scripts/install.*): snapshot real `~\.konnect\bin\konnect.exe` sha/mtime/listing and
  user RETRACE_PYTHON before/after. Fake konnect + cargo via `rustc` (write nothing); shadow `setx` /
  `uname` first on PATH. Git Bash PATH entries must be `/c/...` (a `C:/` entry splits, REAL tool runs).
  EVERY ps1 call gets `-SkipInit -NoRetrace -Target <tmp>`, even ones expected to fail at parsing.
  Build Windows paths with `cygpath -w`: the Bash tool collapses `\\`→`\` (so `"$X\\$n"` = literal `$n`).
  Wrapper guard: prefix-compare `${t:0:len}`, not `case "$SW"\\*` (never matched); prove it refuses the real path first.
  Git Bash rewrites `/c/…` env/args to `C:/…` for native python/jq — write POSIX fixtures with `printf`.
  `powershell -File x.rb` silently won't run (needs .ps1). ps1 injection: `function global:Copy-Item` in a harness.
