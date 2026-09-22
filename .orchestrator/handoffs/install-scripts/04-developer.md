---
change: install-scripts
task: single repair round - qa R2 + reviewer SERIOUS 1, MINORs 2-10, PROTOC hint (scripts/install.ps1, scripts/install.sh)
agent: developer
verdict: DONE
---

## Result

1. Commit `334a8ac` on `orc/install-scripts` (parent `897c4a0`), one commit, `fix(scripts): harden the installers after qa and review findings`. `git diff --name-only 897c4a0..HEAD` = `scripts/install.ps1`, `scripts/install.sh`; `git rev-list --count` = 1. Not pushed.
2. All 9 brief items fixed in both scripts (items 1 and 7 are ps1-only, item 8 is sh-only). TDD: I wrote a 59-check suite first. RED against `897c4a0` was 10 pass / 47 fail. GREEN against `334a8ac` is 59/59.
3. Syntax: PS 5.1.26100.9444 `Parser.ParseFile` 0 errors. `bash -n` clean. Both files are ASCII with LF (ps1 669 lines, sh 710 lines).
4. Machine safety (SC2): the real exe and RETRACE_PYTHON are byte-identical before and after. `~/.claude/settings.json` mtime moved, but not because of me. See Deferred 1.
5. HARD TEST RULE kept. Every invocation went through wrappers that append `-Target <temp> -SkipBuild -SkipInit -NoRetrace` (sh: `--target <temp> --skip-build --skip-init --no-retrace`), or it carried `-DryRun`/`--dry-run`. The temp harness is removed.

## Evidence

Safety snapshot (real machine):
- Before (10:10): `sha256 DEA8824B1357F6587FA98792D8246C6516CBD5A12F30ECEDB52F31B7A447DEC5`, mtime `2026-09-21T18:30:27.9145510Z`, 34333696 bytes, bin `konnect-0.12.0-2026-09-17.exe.bak, konnect.exe, schematic-viewer.exe`, `RETRACE_PYTHON(user): []`, `(machine): []`.
- After (10:32): identical on every line, and `konnect --version` → `konnect 0.12.0`.

Per item. `<S>` is the temp harness, which held a fake repo, fake `konnect-old.exe` (0.11.0-old, sha `debe687c`) / `konnect-new.exe` (0.99.0-new, sha `8561d6a4`) built with rustc, and JSON fixtures.
1. `ps1_file_safe <tmp>\konnect.exe -SkipBuild -WhatIf|-DryRum|-Bogus|stray` gives exit 1 each time with `A parameter cannot be found that matches parameter name 'WhatIf'` (and `'DryRum'`, `'Bogus'`), or `A positional parameter cannot be found…` for `stray`. The folder still holds only `konnect.exe` with the old sha. RED at 897c4a0 was exit 0 plus a rename. Still working: `-Dry` → DryRun (exit 0), `-Help` → exit 0, `-Client foo` → exit 1, `-SkipBuild -Rebuild` → exit 1.
2. A directory as `-Target`/`--target` gives exit 1 in both scripts with `the install target …\b2dir-ps1 is a folder. Pass the full path to the .exe, e.g. -Target '…\konnect.exe'`, and the sentinel is unchanged. A "rename" that leaves the target in place (ps1: `Move-Item` shadowed to copy; sh: `mv` shim that copies) gives exit 1 with `…\konnect.exe still exists after renaming it to …; stopping before the copy so nothing is overwritten`, and the target keeps the old sha.
3. Every case is exit 1. Copy fails but the restore works (ps1: `Copy-Item` throws; sh: `cp` shim exits 1) → `…the previous binary was renamed back - nothing changed.`, and only the old `konnect.exe` remains. Copy fails AND the restore fails → `…and putting the previous binary back failed too (injected restore failure). The previous binary is safe at …exe.bak; the lines below restore it.` plus the commands, and running those commands restores the old sha. A partial copy blocking the restore gives two lines, which restore the old sha. A hash mismatch after the copy prints the rollback, which works. An unexpected error after the copy (ps1: `Get-FileHash` throws on its 4th call) is caught by `trap` → `unexpected error: … (at <script>:<line>)` plus the rollback, which works. sh unexpected abort (`export -f cp` with `cp() { exit 7; }`) → rc 7, `install: ERROR: stopped unexpectedly (exit 7)`, and the rollback is printed. The first paste restores the old sha. The second paste prints `rollback stopped: already done…`.
4. Target `<S>\t\apos\O'Brien\bin\konnect.exe`, a real run of each script. ps1 prints `Move-Item -LiteralPath '…\O''Brien\bin\konnect.exe' …`. sh prints `[ ! -e '…/O'\''Brien/…rolledback.exe.bak' ] && [ -e '…' ] && mv -- … && mv -- … || echo …`. Each set was pasted twice. Both folders end as `konnect.exe debe687c8463` + `…-rolledback.exe.bak 8561d6a40373`, and the listing after the first and second paste is identical.
5. Dry runs to a temp target. Touching `docs/RELIABILITY_CONTRACT.md`, `Cargo.toml` or `rust-toolchain.toml` after the build → `build needed: … older than <that file>` in both scripts (6/6 checks). With `CARGO_TARGET_DIR=ctd` (relative, ps1) or an absolute path (sh): `up to date: …\repo\ctd\release\konnect.exe`, `new build version: 0.11.0-old`.
6. The `shadow.json` fixture has a top-level entry, a project with a different path, and a project with the same path in another case/separator. Both scripts still pick `-> …\t\top\konnect.exe` and print `WARNING: Claude Code gives a project's own konnect entry precedence inside that project, so these projects keep running a different konnect than the top-level one (…\t\top\konnect.exe):` / `- project C:/work/board -> …\t\proj\konnect.exe` / `This installer still installs to …`. The same-path project is not listed. The real `~/.claude.json` produces no warning (dry runs of both scripts: `from top-level … -> C:\Users\felip\.konnect\bin\konnect.exe`).
7. ps1 dry runs. `"Konnect"` → `no konnect MCP server registered`. `/c/Users/…/t/posix/konnect.exe` → `NOTE: read the POSIX-style path '/c/…' as 'C:\Users\…'` and `-> C:\Users\…\t\posix\konnect.exe`, never `C:\c\…`. `/usr/bin/konnect` → exit 1 `…a path without a drive letter; Windows cannot tell where it points. Pass -Target…`. `Set-Location <S>\fx; & install.ps1 -ClaudeConfig .\shadow.json -DryRun -SkipInit -NoRetrace` → `from top-level … in <S>\fx\shadow.json`, exit 0. A wrapper command still gives `not a path to a konnect executable (a wrapper or a command with arguments)`.
8. sh dry runs on `bad.json`. jq 1.7.1 → exit 1 `bad.json is not valid JSON (checked with jq); fix the file or pass --target.` The same happens behind a jq-1.6 shim that turns exit 5 into exit 2 (no "neither jq nor python"), and with python as the reader (jq removed from PATH). python with an empty file → default target, exit 0. PATH with no reader → `WARNING: neither jq nor python is available…`, and it falls back.
9. ps1 `-Help` contains `Run it from an open PowerShell window…` and `not with Explorer's "Run with PowerShell"`. A PROTOC failure (stale build, no protoc) → `…To keep PROTOC for every new window: [Environment]::SetEnvironmentVariable('PROTOC','<path to protoc.exe>','User')`, and sh on Windows gives the same text after `run in PowerShell:`. A fresh install (empty temp dir, real run) → `backup: (none - fresh install: there was no konnect at … before)` and `undo (…) - this was a fresh install, so undoing it moves the new file aside:` plus a `Move-Item`/guarded `mv` line, and pasting it moves the file aside. A dry run with `-RetracePython <main .venv-retrace>` → `RETRACE_PYTHON only reaches programs started after it is set: close Claude Code completely` / `(every window, and any terminal running it) and start it again from a new terminal or the Start menu.`

## For the next agent

1. Safe re-run recipe:
   - Fake repo in temp: copy `scripts/`, `crates/konnect/src/manifest.rs`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml` and `docs/RELIABILITY_CONTRACT.md`, then `git init` + commit.
   - AFTER the commit, place `target/release/konnect.exe`, built from a 10-line Rust main that answers `--version` (`option_env!("FAKE_VER")`, `FAKE_VER=0.99.0-new rustc -O fake.rs`).
   - Wrappers must append the flags themselves. A later `--target` wins in sh. ps1 rejects a duplicate `-Target`, so the wrapper's own `-Target` fails safe.
   - Discovery cases use `-DryRun -SkipInit -NoRetrace -ClaudeConfig <fixture>` (no `-Target`, or discovery is skipped).
2. Failure injection needs no hooks in the scripts:
   - ps1: run install.ps1 with `&` from a harness .ps1 that defines `function global:Copy-Item/Move-Item {…}`, because functions shadow cmdlets. For `Get-FileHash`, run `Import-Module Microsoft.PowerShell.Utility` BEFORE defining the override. It is a module function in 5.1, and autoload silently replaces the override.
   - sh: `cp`/`mv` shims first on PATH, in POSIX form. For an abort, use `export -f cp` with `cp() { exit 7; }`.
   - Extract rollback lines with `^\s+Move-Item ` / `^\s+\[ ` only, never `init`/`uninstall` lines.
3. Decisions the diff does not explain:
   - I used `[CmdletBinding(PositionalBinding = $false)]` rather than a bare `[CmdletBinding()]`. Otherwise a stray word binds positionally to `-Client`/`-Target`.
   - The rollback is computed from state when it is printed (`$backupPath` is set only after a real rename, plus `$freshInstall`, `$initClients`, `$retraceChanged`). Failure output therefore lists only what really happened.
   - The fresh-install undo is `uninstall` for each client whose init ran, then moving the file aside. Nothing is ever deleted.
4. Scope choices:
   - The shadow warning is emitted only during discovery. `-Target` still never reads the config, so it stays the escape hatch for an unparsable config.
   - `/c/...` is converted, with a NOTE, to match the sh. I did not verify how Claude Code itself spawns a `/c/...` command on Windows; node may resolve it to `\c\...` on the current drive.
   - sh restores after a failed copy only when the target is free. It never `mv`s over a partial file, which mirrors ps1 `Move-Item` without `-Force`.
5. Gotcha: the init-failure path cannot run under the HARD TEST RULE, because `-SkipInit` is mandatory for real runs. It goes through the same `Fail`→`Show-FailureRecovery` (ps1) and `fail`→EXIT-trap (sh) path that the copy, hash and unexpected-error cases prove. A direct init-failure test needs an orchestrator-sanctioned exception: a fake binary whose `init` exits non-zero, with a temp target only.

## Deferred findings

1. Observation for the orchestrator's re-verify: `~/.claude/settings.json` mtime moved from `2026-09-21T18:30:34Z` to `2026-09-22T13:16:45Z` (10:16:45 local) during my session. It was not the installer:
   - `~/.claude/agents|skills` mtimes are unchanged (2026-09-21 18:30:34Z), and `konnect init` would have rewritten them.
   - Diffing against the scratchpad's `claude-settings.before-init.json` (2026-09-21) shows only `"model": "opus"` removed and `effortLevel` `xhigh`→`high`. Claude Code's own session files were written in the same minutes. The konnect hooks are identical.
   - No invocation of mine ran init.
2. `build.target-dir` set in a cargo config file (not the env var) is still not honoured; only `CARGO_TARGET_DIR` is, as briefed. An edit to the root `Cargo.toml` that does not relink konnect makes every later run call `cargo build`, which is a quick no-op. I accepted that.
3. Still open from qa: the Codex `Done:` count is not checked against manifest.rs. The `-rolledback` parked name has no collision loop (harmless, because the moves are guarded or refuse to overwrite). `.bak` files are never pruned.
4. The ps1 rollback stays as two separate `Move-Item` lines, as briefed ("keep it"). If line 1 fails for a reason other than "already done", line 2 still runs. That is safe, since `Move-Item` never overwrites, but a single guarded `if` line would be clearer.
5. Carried over (not done, per the brief): `.gitattributes` `*.sh eol=lf`, schematic-viewer.exe, `-Client codex` semantics, python `-B`.
