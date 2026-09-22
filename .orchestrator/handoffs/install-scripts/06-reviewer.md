---
change: install-scripts
task: re-review of the repaired installers at 334a8ac (round-1 items 1-10 + qa R2, new defects in the repair diff, whole-file safety read)
agent: reviewer
verdict: DONE
---

## Result

Verdict: **DONE (ship)**. Every round-1 item and qa's R2 are closed. I checked each one with a probe or by tracing the code path. The repair introduced no SERIOUS defect.

- **The `trap` handlers.** Each fires once. Neither prints a rollback when nothing was renamed. The ps1 always exits 1, the same code as the uncaught error it replaces. The sh keeps the original exit status.
- **`[CmdletBinding(PositionalBinding=$false)]`.** It rejects `-WhatIf`, `-Confirm`, typos and stray words. The common parameters it adds do no harm. `-ErrorAction SilentlyContinue` or `Ignore` cannot disarm the script: line 48 resets `$ErrorActionPreference` to `Stop` after binding. I held the target open to make the rename fail, and the script still stopped with "nothing was changed".
- **Deletes and overwrites.** Neither script contains a delete. Every file change is one of three kinds:
  - a rename to a free name;
  - a copy that runs only after the target is confirmed absent (ps1:522-525, sh:550-553);
  - a restore that never overwrites.

Two new MINOR findings, neither blocking:

1. **MINOR: the ps1 prints rollback lines with no guard, even in a dry run.** Pasting them from a `-DryRun` (or from an older run's block) moves the working `konnect.exe` out of place. `install.ps1:100-103` (Get-RollbackLines emits `Move-Item target parked` whenever `$DryRun` is set), `:650` (`backup:` names a file a dry run never made) and `:655-659` (the dry-run summary is headed `rollback (PowerShell; safe to paste twice):`).
   - Reproduced on a temp target that held the old binary. After a dry run I pasted the two printed lines:
     - Line 1 renamed the untouched binary to `konnect-0.99.0-new-<stamp>-rolledback.exe.bak`. The file is mislabelled: it is the *old* binary under the *new* build's name.
     - Line 2 failed with `…exe.bak does not exist`.
     - Result: **no `konnect.exe` at the target**. A second paste changes nothing.
   - The stale-block variant (traced): after a real run, pasting the dry run's block instead of the real one does four things:
     - it moves the new build aside;
     - it fails the restore, because the backup name differs by the timestamp;
     - it runs `init` on a missing file;
     - it unsets `RETRACE_PYTHON`.
   - Konnect then will not start until someone renames a file back. On this PC that file would sit next to the older `konnect-0.12.0-2026-09-17.exe.bak`, which invites restoring the wrong one.
   - This is pre-existing: 897c4a0 printed the same unguarded lines in both ports. I missed it in round 1, and so did qa. The repair guarded the sh: a pasted dry-run line prints `rollback stopped: …` and changes nothing (probed). It did not guard the ps1, the script this user runs, and it added "safe to paste twice" to the dry-run header.
   - Why MINOR: nothing is deleted, one rename recovers it, and it needs a paste from a run labelled `DRY RUN - nothing was changed.` The consequence matches round-1 finding 2, which I rated MINOR.
   - Fix, about 5 lines:
     - Print the pair as one guarded line: `if ((Test-Path -LiteralPath 'B') -and -not (Test-Path -LiteralPath 'P')) { Move-Item -LiteralPath 'T' -Destination 'P' -ErrorAction Stop; Move-Item -LiteralPath 'B' -Destination 'T' } else { 'rollback stopped: already done, or the files were moved - check the folder' }`.
     - Guard the fresh-install undo the same way.
     - In a dry run, label the block "the real run will print a rollback like this - nothing to undo now" and print `backup (would be):`.

2. **MINOR: `Format-PsLiteral` doubles only ASCII `'`.** `install.ps1:92`.
   - The PowerShell tokenizer also treats U+2018, U+2019, U+201A and U+201B as single quotes.
   - For a path such as `C:\Users\O’Brien\…`, every printed rollback line fails to parse. `PSParser.Tokenize` reports `The string is missing the terminator: '.` for `Format-PsLiteral "C:\Users\O’Brien\bin\konnect.exe"`.
   - Nothing runs, so it is safe, but the rollback is unusable. Rare. Fix: `$text -replace "['\u2018-\u201B]", '$0$0'`. This can wait for a follow-up.

### Round-1 item → closed? (evidence)

| # | Round-1 item | Closed? | Evidence |
|---|---|---|---|
| 1 | SERIOUS: sh `--target <dir>` overwrote the binary with no .bak (ps1: raw null error) | **Closed** | Real runs with a temp folder as target. ps1: exit 1, `the install target …\t\dir is a folder. Pass the full path to the .exe, e.g. -Target '…\t\dir\konnect.exe'.`. Same with a trailing `\`. A missing `…\sub\` gives `ends with a separator`, and the directory is not created. sh: exit 1 with the same hint. The sentinel is unchanged in all cases (`konnect.exe=dc5887e1`). A generic "target must be absent before copying" guard sits between rename and copy (ps1:522-525, sh:550-553). |
| 2 | sh rollback not safe to paste twice | **Closed** | sh real run to `…\O'Brien\bin`. Paste 1 restores (`konnect.exe=dc5887e1`, `…-rolledback.exe.bak=1fba2b3d`). Paste 2 prints `rollback stopped: already done, or the files were moved - check the folder`, and the state is identical. |
| 3 | A failure after the rename printed no rollback | **Closed** | ps1: `Get-FileHash` injected to throw on call 4 (post-copy). Output: `unexpected error: … `, `== Stopped - changes made before the error`, `previous binary: …`, two `Move-Item` lines. Paste 1 restores the old sha, paste 2 errors and changes nothing. sh: `cp` exported as `exit 7`. Output: `exit=7`, `stopped unexpectedly (exit 7)`, the guarded line. Paste 1 restores, paste 2 prints "rollback stopped". Init failure, traced: the client is added to `$initClients` / `init_clients` *before* init runs (ps1:564, sh:585), so `Fail` / `fail` → recovery prints the restore plus `& '<t>' init`. |
| 4 | A failed restore was reported as success | **Closed** | ps1 with `Copy-Item` injected to leave a `PARTIAL` file and throw, and `Move-Item` injected to fail on its 2nd call (the restore). Output: `…failed (injected copy failure), and putting the previous binary back failed too (injected restore failure). The previous binary is safe at …exe.bak; the lines below restore it.` Pasting parks the partial file (`a93f0e4a`) and restores the old binary (`dc5887e1`). sh: `if ! target_exists && mv …` (sh:556-560) never `mv`s over a partial file (traced; the developer probed it). |
| 5 | `'` in a path broke the rollback lines | **Closed** for ASCII `'` | ps1 prints `…\O''Brien\…`. Pasted twice: first restores, second errors with the state unchanged. sh `'\''` probed the same way (item 2). Residual: typographic quotes (new MINOR 2). |
| 6 | Staleness ignored files outside `crates/` and `CARGO_TARGET_DIR` | **Closed** (6a, 6b env var) | Touching `docs/RELIABILITY_CONTRACT.md` gives `build needed: build (…10:41:37) is older than docs/RELIABILITY_CONTRACT.md (…10:44:29)` (ps1) and `build is older than docs/RELIABILITY_CONTRACT.md` (sh). `CARGO_TARGET_DIR` absolute gives `up to date: …\ctd-abs\release\konnect.exe` and `new build version: 0.11.0-old`. Relative `ctdrel` resolves to `…\repo\ctdrel\release\…` in both. A directory with no build gives `build needed: no build at …\nothing\release\konnect.exe`. Residual (Deferred 2): `build.target-dir` set in a cargo config file is not honoured. |
| 7 | A differing project-level entry was ignored silently | **Closed** | ps1 dry run, fixture with top = `…\t\px\konnect.exe` and three projects: `C:/a` has the same path in `/c/…` form (not listed), `C:/b` has another path (listed), `C:/c` has `cmd /c konnect` (listed). Output: `WARNING: Claude Code gives a project's own konnect entry precedence …` and `This installer still installs to …`. The target stays top-level. The developer showed sh parity. |
| 8 | ps1 discovery: case-insensitive name, POSIX path, relative `-ClaudeConfig` | **Closed** | A `Konnect` key gives `no konnect MCP server registered`. `/c/…` and `/cygdrive/c/…` give `NOTE: read the POSIX-style path … as 'C:\…'` and `-> C:\…\t\px\konnect.exe`. `/usr/bin/konnect` and `\tools\konnect.exe` exit 1 with `a path without a drive letter; … Pass -Target`. Two projects, one written `/c/…` and one upper-case `C:\…`, collapse to one path. In-session `Push-Location fx; & install.ps1 -ClaudeConfig .\rel.json` reads `…\fx\rel.json`. |
| 9 | sh exit code 2 meant both "no reader" and a jq ≤1.6 parse error | **Closed** | Validation now runs first (`config_is_valid_json`). Invalid JSON exits 1 with `bad.json is not valid JSON (checked with jq)`. Behind a jq-1.6 shim (5→2) the output is the same. With python as the reader it is `(checked with python)`. A BOM file is accepted by jq and refused by python, so it fails closed before any change. The ps1 on the same `bad.json` exits 1 with `cannot parse …; pass -Target`. |
| 10a | RETRACE_PYTHON: "restart" was not enough | **Closed** | ps1:664-665 and sh:705-706 now say "close Claude Code completely (every window, and any terminal running it) and start it again from a new terminal or the Start menu". The developer probed the dry-run output. |
| 10b | A fresh install said "nothing to roll back" | **Closed** | Real run into an empty temp dir in both ports. Output: `backup: (none - fresh install …)` and `undo (…) - this was a fresh install, so undoing it moves the new file aside:`. Paste 1 parks the file, paste 2 is harmless (ps1 error, sh `undo stopped`). |
| 10c | Explorer's "Run with PowerShell" loses the summary | **Partial** | Only a note in `-Help` (ps1:73-76). No pause or log file. Not blocking, because the orchestrator tells the user how to run it (Deferred 3). |
| 10d | Raw PowerShell error for a folder target | **Closed** | See item 1. |
| R2 | ps1 ignored unknown options and ran a real install | **Closed** | Each run had a temp target plus all safety flags. `-WhatIf`, `-DryRum`, `-Bogus`, `-Confirm` → exit 1, `A parameter cannot be found that matches parameter name …`. `stray` → exit 1, `A positional parameter cannot be found …`. `-Re x` → exit 1 (ambiguous). The sentinel is unchanged every time. `-D` resolves to `-DryRun`, not `-Debug`, so the output is dry-run only and the state is unchanged. `-Verbose` and `-Debug` produce a normal real install with no prompt or hang. |

### New-defect checks on the repair diff (focus 2)

- **`trap` firing twice.**
  - ps1: no. `$script:recoveryShown` guards the recovery print, and the trap ends in `exit 1`. `Fail` inside `catch` blocks exits without re-entering the trap.
  - sh: no. bash does not pass the EXIT trap to `( … )` or `$( … )` subshells. Probe: one `TRAP rc=7` line, printed from the parent PID, while the `( exit 3 )` subshell ran silently. The build subshell (`sh:395-400`) therefore cannot print a spurious recovery.
- **A rollback printed when nothing was renamed.** No.
  - A hash failure on call 1 (before the rename) prints only `unexpected error: …`, with no block, exit 1, and the target unchanged.
  - A rename failure prints `nothing was changed`, with no block.
  - `$backupPath` / `backup` are set only after a successful `Move-Item`/`mv`.
- **Masking exit codes.**
  - ps1 always exits 1, the same as an uncaught error before the repair.
  - sh keeps the status: a probe shows `final exit=7`. A command failing inside the trap under `set -e` would turn that into 1 (`trap body fails under set -e -> final exit=1`). No command in `on_exit` can fail: `rollback_lines` ends in an `if` with no `else`, and every other line uses `||` or `say` (traced).
- **`PositionalBinding=$false` side effects.** Harmless.
  - `-WhatIf` and `-Confirm` are not added, because there is no `SupportsShouldProcess`.
  - `-ErrorAction` is overridden by line 48, as the locked-rename probe proves.
  - `-Debug` did not prompt.
  - A prefix shared with a common parameter resolves to the script's own parameter.
- **Rename → verify-absent → copy.** Present in both ports. The window between the check and the copy is microseconds, which is negligible.
- **Rollback quoting.** ASCII is correct. Typographic quotes: new MINOR 2.
- **POSIX-path conversion.** Correct. The conversion also applies to `-Target`, so `-Target /a/x.exe` becomes `A:\x.exe`. That is harmless.
- **Invalid-JSON stop.** Both ports fail closed before any change (item 9).
- **CARGO_TARGET_DIR.** Correct (item 6).

### Ship rationale

The ps1 now meets the bar for a non-engineer on their only machine:
- Unknown or misspelled options stop before anything runs.
- A folder target is refused.
- The binary is only ever renamed, never deleted, and never overwritten.
- Every failure after the rename, whether expected (`Fail`) or unexpected (`trap`), prints restore commands that match the current state. I proved that they work, and that pasting them twice changes nothing.
- I found no path that deletes or corrupts a binary, and none that leaves the user without a rollback after a real change.

The one residual hazard (MINOR 1) needs the user to paste lines from a run that says `DRY RUN - nothing was changed.` Its worst case is a misplaced, correctly preserved file that one rename fixes. The fix is five lines and belongs in a follow-up. It is not a reason to run a second repair round in this lane. Until the fix lands, the orchestrator should tell the user never to paste anything from a dry run.

## Evidence

The probe kit is the session scratchpad `…\scratchpad\rv2\`:
- `repo\` is a fresh `git init` holding `git show 334a8ac:` copies of both scripts, plus `manifest.rs`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml` and `docs\RELIABILITY_CONTRACT.md`.
- `repo\target\release\konnect.exe` is a rustc fake, `konnect 0.99.0-new` (sha `1fba2b3d`). `bin\old.exe` is `konnect 0.11.0-old` (sha `dc5887e1`).
- `t\` holds the temp targets and `fx\` the JSON fixtures. The harnesses are `p1`–`p5.ps1`, `wrap-hash.ps1` and `wrap-cm.ps1`.

HARD RULE kept:
- Every run carried `-DryRun`, or a temp `-Target` + `-SkipBuild` + `-SkipInit` + `-NoRetrace` (sh equivalents).
- The only contact with the real binary was the read-only `--version` probe and hash in the dry runs that fell back to the default target.
- Nothing was built into the worktree. The worktree is clean at `334a8ac`.

Real machine before and after (10:40 and 10:49):
- `sha256 DEA8824B1357F6587FA98792D8246C6516CBD5A12F30ECEDB52F31B7A447DEC5`, mtime `2026-09-21T18:30:27.9145510Z`.
- Bin folder: `konnect-0.12.0-2026-09-17.exe.bak, konnect.exe, schematic-viewer.exe`.
- `RETRACE_PYTHON` user and machine both empty.
- `settings.json` mtime `2026-09-22T13:16:45Z`, unchanged during my session. The newest file under `~/.claude/agents` is `2026-09-21T18:30:34Z`.
- `~/.cargo/config.toml` is absent. The repo's `.cargo/config.toml` holds only the `xtask` alias.

Key outputs:
- **MINOR 1 (ps1 dry-run paste).** `DRY RUN - nothing was changed.` / `rollback (PowerShell; safe to paste twice):` / two `Move-Item` lines. Paste 1: `ok`, then `Cannot move item because the item at '…konnect-0.11.0-old-20260922-104254.exe.bak' does not exist.`. The state becomes `konnect-0.99.0-new-STAMP-rolledback.exe.bak=DC5887E1`, and no `konnect.exe` remains. The same probe on the sh gives `paste1: rollback stopped: …`, and the state stays `konnect.exe=dc5887e1`.
- **MINOR 2.** `literal='C:\Users\O’Brien\bin\konnect.exe'  parse errors=1 The string is missing the terminator: '.`
- **R2 and common parameters.** From `p1.ps1` (see the table). `-ErrorAction SilentlyContinue` with a locked target gives `could not rename … (The process cannot access the file because it is being used by another process.); nothing was changed.`, exit 1, and `konnect.exe=DC5887E1`.
- **trap.** From `p2.ps1`. Call 1 gives exit 1, `konnect.exe=DC5887E1` and only the error line. Call 4 gives exit 1 and the block. After paste 1, the state is `…-rolledback.exe.bak=1FBA2B3D konnect.exe=DC5887E1`. Paste 2 gives `Cannot create a file when that file already exists.` and the state is unchanged.
- **The main checkout's build is fresh for the new input list.** `target/release/konnect.exe` was built at 2026-09-22 08:54:52, `konnect 0.12.0`. The newest tracked input is `crates/konnect/tests/doc_tool_counts.rs` at 08:52:14.

## For the next agent

1. **Orchestrator.** The verdict is DONE, so the lane can merge without a second repair round. Queue MINOR 1 as a small follow-up: guarded single-line ps1 rollback, dry-run labelling, and `backup (would be):`. Add MINOR 2 if convenient.
2. **Tell the user before the first real run** (applies until MINOR 1 is fixed): "The rollback lines a `-DryRun` prints are only a preview. Never paste them. If you ever need to undo, paste only the block printed by the real run, or the `Stopped` block after an error."
3. **First real run on this PC.**
   - The main checkout's build is newer than every tracked build input, so no build runs and PROTOC is not needed.
   - Run it from an open PowerShell window, not with Explorer's "Run with PowerShell": `powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -DryRun`, then the same command without `-DryRun`.
   - It renames the running 0.12.0 exe (supported), runs `init` against the real `~/.claude`, and sets User `RETRACE_PYTHON` if `.venv-retrace` imports retrace.
   - The summary then says to close Claude Code completely and start it from a new terminal. Record the real exe's sha256 and RETRACE_PYTHON before and after, as the lane's RULE requires.
4. **To re-probe,** reuse `rv2\`. Only rebuild `repo\` if the scripts change: `git show <sha>:scripts/...` into `repo\scripts\` and commit. Failure injection needs no hooks in the scripts:
   - `wrap-hash.ps1`: `Get-FileHash` override after `Import-Module Microsoft.PowerShell.Utility`.
   - `wrap-cm.ps1`: `Copy-Item`/`Move-Item` overrides.
   - sh: `export -f cp` with `cp() { exit 7; }`.

## Deferred findings

1. When the failure falls between the rename and `$changes.Add` / `add_change` (ps1:547, sh:569), the `== Stopped - changes made before the error` block lists no bullets. The `previous binary:` line and the restore commands cover the case, so this is cosmetic.
2. Only `CARGO_TARGET_DIR` is honoured. A `build.target-dir` in a cargo config file is not, and after a real build the scripts check only that the file exists (ps1:394, sh:401). With such a config, a stale binary would be installed and reported as `built`. Neither config exists on this PC. Cheap guard: require the build's mtime to move during `cargo build`.
3. Item 10c is only partly addressed (see the table).
4. ps1: Ctrl+C after the rename prints no rollback, because a PowerShell `trap` does not run on a pipeline stop. The `renamed A -> B` line does name both paths. Traced, not probed.
5. sh on Windows accepts a drive-less config command (`/usr/bin/konnect` passes `is_konnect_path`, sh:146) and maps it into Git's `usr/bin`, while the ps1 refuses it. Traced, not probed. Such a config is nonsensical on Windows.
6. The fresh-install undo runs `konnect uninstall`. That command deletes konnect's own skills, agents and hook entries (`crates/konnect/src/install.rs:350-392`), which `init` can recreate. "Nothing is ever deleted" (04-developer §3) holds for binaries only. This needs a wording fix, not a code fix.
7. Carried over as before: `.gitattributes` `*.sh text eol=lf`, `schematic-viewer.exe` staleness, `-Client codex` semantics, python `-B`, the Codex `Done:` count, and `.bak` pruning.
