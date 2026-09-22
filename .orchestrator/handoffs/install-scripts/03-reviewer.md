---
change: install-scripts
task: adversarial review of scripts/install.ps1 + scripts/install.sh (commit 897c4a0)
agent: reviewer
verdict: FIX
failing_layer: implementation
---

## Result

Verdict: **FIX**. There is one SERIOUS finding, in `install.sh`. The PowerShell script is the one this user will run, and I found nothing that blocks it. Its default run on this machine is safe: I dry-ran it against the real config, renamed a binary while its process was running, and ran the printed rollback lines, all in temp dirs. The shell script has a path that overwrites the installed binary with no backup. That breaks the headline promise, and fixing it takes about three lines.

Would I let a non-engineer run them on their only machine? The ps1 yes, once the MINORs below are considered. The sh no, not until finding 1 is fixed.

### Findings

1. **SERIOUS: `install.sh` overwrites the existing binary with no backup when `--target` is a directory.** Lines `install.sh:411-441`.
   - What happens: `--target ~/.konnect/bin` is a natural reading of "install path". `[ -f "$target_u" ]` is false for a directory, so the script takes no backup (`backup=""`). Then `cp -- "$source_bin" "$target_u"` copies *into* the directory and replaces `bin/konnect.exe` in place. `same_file_content` then fails against the directory, and the script prints `ERROR: … does not match the build after copying; previous binary kept at (none).` and exits 1.
   - Reproduced in `rv/tdir-sh`: `bin/konnect.exe` held `OLD-BINARY-SENTINEL` (19 bytes). After the run it holds the 36,292,096-byte build (`MZ…`). No `.bak` exists, and the old binary cannot be recovered.
   - On macOS the same `cp` over a *running* binary rewrites its pages in place.
   - The ps1 with the same input (`-Target …\bin`) crashes at `install.ps1:122` with a raw `You cannot call a method on a null-valued expression`. That happens because `Get-FileHash` returns nothing for a directory. Nothing changes, but only by accident, and the message tells the user nothing they can act on.
   - Fix, both scripts:
     - Refuse a target that is an existing directory, with a message such as "pass the path of the konnect.exe file, e.g. …\bin\konnect.exe".
     - Add a generic guard between rename and copy: *the target must not exist at copy time*. If it still exists, no backup was taken, so stop. That closes the whole class of bugs, not just the directory case.
   - failing_layer: implementation.

2. **MINOR: the sh rollback is not safe to paste twice.** Lines `install.sh:446-447`.
   - What happens: the second paste runs `mv target parked`, and POSIX `mv` replaces silently. The parked new build is overwritten by the old binary, then `mv backup target` fails because the backup is gone. The result is **no `konnect.exe` at the target**, so Claude Code's konnect will not start. The old binary survives only under the `-rolledback` name.
   - Reproduced in `rv/run-sh`: after the second paste, the directory holds only `konnect-0.12.0-…-rolledback.exe.bak` (old sha `e4224d18…`), and `rb exit=1`.
   - The ps1 is safe here: `Move-Item` without `-Force` refuses to overwrite. Reproduced as well: both lines error and the state is intact.
   - Fix: print the sh pair as one guarded line: `[ ! -e 'parked' ] && [ -e 'backup' ] && mv -- 'target' 'parked' && mv -- 'backup' 'target'`.

3. **MINOR: a failure after the rename exits without rollback commands.** Lines `install.ps1:403, 435` and `install.sh:441, 472, 538`.
   - What happens: `Fail`/`fail` is `Write-Host …; exit 1` (`install.ps1:71-74`, `install.sh:72`). The summary and rollback block (`install.ps1:505-525`) can only be reached on success.
   - Realistic trigger: `konnect init` exits non-zero after the binary was already swapped. For example, `patch_claude_settings` returns an error on a hand-edited `~/.claude/settings.json`. `run_install_at` runs it with `verbose=true` and propagates the error (`crates/konnect/src/install.rs:284-289`).
   - The user then sees only `ERROR: konnect init failed`. The backup path appears only in the earlier `renamed … -> …` line, and no restore command is printed. The same applies to the post-copy hash mismatch at `:403`: it names the backup but does not restore it.
   - Fix: move the summary into a function and call it from `Fail` once any change has happened.

4. **MINOR: a restore failure is swallowed and reported as success.** Lines `install.ps1:398-399` and `install.sh:436-437`.
   - What happens: `try { Move-Item $backupPath $targetPath } catch { }` (and `|| true` in the sh) is followed by an unconditional "the previous binary was renamed back". If the failed `Copy-Item` left a file behind, `Move-Item` without `-Force` fails. I reproduced that part: `Cannot create a file when that file already exists.` The message then lies.
   - Low probability, since Windows `CopyFile` usually cleans up after itself. Report the real outcome instead.

5. **MINOR: rollback lines break on a `'` in a path.** Lines `install.ps1:409-410, 499` and `install.sh:446-447, 542`.
   - Reproduced with a `…\O'Brien\bin\konnect.exe` target: both printed `Move-Item` lines fail with `Cannot move item because the item at '…\apos\O' does not exist.`
   - Windows profile folders like `C:\Users\O'Brien` exist.
   - Fix: escape `'` as `''` for PowerShell, and as `'\''` for sh.

6. **MINOR: the staleness check watches too little and ignores the cargo target dir.**
   - (a) `git ls-files -- crates Cargo.lock` misses build inputs outside `crates/`:
     - `docs/RELIABILITY_CONTRACT.md`, which is `include_str!`'d into the konnect skill payload (`crates/konnect/src/manifest.rs:40`);
     - the root `Cargo.toml` (workspace deps and profiles);
     - `rust-toolchain.toml`.

     A commit that changes only those files gives "up to date", and the installer ships stale embedded guidance. The count check cannot see content changes. No docs-only commit to that file exists in history yet, so this is latent. Add `docs Cargo.toml rust-toolchain.toml` to the list.
   - (b) With `CARGO_TARGET_DIR` (or `build.target-dir`) set, cargo builds elsewhere. The old `target/release/konnect.exe` still exists, passes `Test-Path`, and gets installed and reported as `built …` (`install.ps1:276-285`, `install.sh:306-313`). Pass `--target-dir "<repo>/target"` explicitly, or check that the binary's mtime moved. Neither variable is set on this machine.

7. **MINOR: a top-level entry silently wins over differing project-level entries.** Lines `install.ps1:305-311` and `install.sh:344-350`.
   - Claude Code resolves a name clash local (project) scope first, then user (top-level). So in a project that registers konnect at another path, Claude keeps running the binary this installer did not touch.
   - Reproduced with `fx/shadow.json` (top `…\top\konnect.exe`, project `C:/work/board` → `…\proj\konnect.exe`): both scripts pick top and print no warning.
   - This follows the design order, so it is not a deviation. A warning that lists the shadowing entries would close it. This user is unaffected: 0 of 34 projects register konnect.

8. **MINOR: ps1 target discovery diverges from sh and from the Claude config.**
   - `Get-JsonMember` matches property names case-insensitively (`install.ps1:144`). Reproduced: a server named `Konnect` is taken as `konnect` by the ps1 and ignored by the sh.
   - A POSIX-form command `/c/Users/…/konnect.exe` resolves to `C:\c\Users\…` in the ps1 (reproduced). The sh is correct.
   - A relative `-ClaudeConfig` is read with `[IO.File]::ReadAllText` against the *process* directory, not the PS location (`install.ps1:155`). Reproduced in-session: `cannot parse .\shadow.json (… Could not find file 'C:\Users\felip\Documents\FFS-Hardware-Eng\Konnect\shadow.json')`. Normalise it with `Get-NormalizedPath` as `-Target` already is.

9. **MINOR: the sh exit code 2 means two different things.** In `read_config_commands`, `return 2` means "no JSON reader" (`install.sh:185`), but it is also jq's exit status on an input parse error in jq ≤ 1.6 (Ubuntu 22.04 ships 1.6).
   - A malformed `~/.claude.json` would then be reported as "neither jq nor python is available", and the script would fall back to the default target.
   - Not reproduced: jq 1.7.1 here exits 5, which the script correctly treats as "cannot parse". Use a sentinel that no reader can return, such as 90.

10. **MINOR: some messages a non-engineer cannot act on.**
    - (a) `Restart Claude Code (all windows)` is not enough for `RETRACE_PYTHON` when Claude Code runs in a terminal. A User-scope variable reaches only processes started after the change, so restarting `claude` in the same terminal tab keeps the old environment. Say "close the terminal, open a new one, then start Claude Code".
    - (b) After a fresh install that created a binary and ran init, the summary says `rollback: nothing to roll back.` (`install.ps1:411-412, 522-523`). It should say what to delete or uninstall.
    - (c) "Run with PowerShell" from Explorer closes the window at `exit`, and the rollback text is lost. Consider writing the summary to a log beside the backup, or pausing when the host is not interactive.
    - (d) The raw PowerShell error for a directory target (finding 1).

### PROTOC probing: my answer

**Keep the clear failure. Do not add a probe of `%USERPROFILE%\tools\protoc` or the package-manager shims.**
- `%USERPROFILE%\tools\protoc` is this user's personal convention. Shipped in a repo script, it would pick up an unrelated protoc on someone else's machine.
- Chocolatey, scoop and winget put `protoc` on PATH by construction, so probing their folders adds nothing. None are present here: `C:\ProgramData\chocolatey\bin\protoc.exe`, `~\scoop\shims\protoc.exe` and `…\WinGet\Links\protoc.exe` all come back `False`.
- The script's pre-check (PROTOC, then PATH) mirrors `crates/konnect-ipc/build.rs` `resolve_protoc` one to one. Script and build therefore agree, and a probe would make the script accept a protoc the build does not see unless it also exports it.
- The failure fires **before any change**. Reproduced as a real run in a temp git repo with a stale build: `ERROR: cannot build: - protoc not found …`, exit 1, and the target directory was never created.
- The first real run needs no build. The main checkout is fresh: newest tracked file `crates/konnect/tests/doc_tool_counts.rs` at 08:52:14, build at 08:54:52.
- Optional polish: add the persistent form to the message, `[Environment]::SetEnvironmentVariable('PROTOC','C:\…\protoc.exe','User')`. This user's protoc is `C:\Users\felip\tools\protoc\bin\protoc.exe`, and `include\google\protobuf\any.proto` sits beside `bin`, so build.rs derives the include dir.

### Attack areas

1. **Data safety.** Covered.
   - Renaming a *running* exe works on Windows. Tested with a copied `PING.EXE -n 90` as the ps1 target (process kept running, `.bak` created, new build copied) and the same for the sh under Git Bash.
   - The printed ps1 and sh rollback lines both restore correctly (sha `e4224d18…` back at `konnect.exe`).
   - The backup-name collision loop is correct in both scripts. The `-rolledback` name has no collision check. That is harmless in the ps1 and destructive in the sh (finding 2).
   - Directory target: finding 1. Failures after the rename: findings 3 and 4. Brackets and spaces in the path (`br[1] y\bin`): rename and copy both work.
   - A konnect.exe is running now (PID 30656) from the real target, so the running-server case is the default for the first real run.
2. **Dry-run purity.** Covered.
   - Snapshot before and after both scripts' `--dry-run`/`-DryRun -Client both -RetracePython <main .venv-retrace>` against the real `~/.claude.json`. It covered `~/.konnect/bin` (listing, mtimes, sha `dea8824b…`), `~/.konnect`, anything newer in `~/.claude/skills|agents`, the mtimes of `settings.json` and `.claude.json`, the retrace `.pyc` count, `HKCU\Environment` RETRACE_PYTHON, and the probe repo tree. Result: **identical**.
   - `konnect --version` returns before any other work (`main.rs:62, 79-82`).
   - The retrace import probe wrote no `.pyc` because retrace is pip-installed with bytecode already compiled. `-B` would make that certain (deferred).
3. **Target discovery.** Covered.
   - Real config: top-level `C:\Users\felip\.konnect\bin\konnect.exe`, no args, no env.
   - Fixtures: shadow (finding 7), `Konnect` capitalised and POSIX form (finding 8), malformed (both scripts give "cannot parse" and exit 1), empty file (both fall back to the default plus the `claude mcp add` note).
   - jq and python fallbacks: jq 1.7.1 exits 5 on a parse error and python exits 1 (finding 9 covers 1.6).
   - Spaces and case/separator normalisation: the developer's fixtures, plus my bracket-and-space path.
4. **Build staleness, PROTOC and CMake.** Covered.
   - Finding 6 and the answer above.
   - CMake is not on PATH here. The VS glob resolves it (the real run reached no CMake problem).
   - PROTOC, CARGO_TARGET_DIR and RETRACE_PYTHON are unset at every scope (User, Machine, process).
5. **RETRACE_PYTHON.** Covered.
   - User scope only. The previous value is recorded for rollback. The sh `powershell -NoProfile -Command '…$null…'` rollback quoting survives Git Bash (tested with a harmless equivalent: `RETRACE_PYTHON|True|User`).
   - konnect reads the variable as the third candidate, after the explicit argument and `photo_intake.retrace_python_path` (`photo_intake.rs:302-320`). No config key is set here, so the variable will take effect.
   - The terminal-restart gap is finding 10a.
6. **PS 5.1, encoding, portability, exit codes.** Covered.
   - Every ps1 run above used Windows PowerShell 5.1.26100.9444 (ASCII/LF file, parses and runs).
   - A CRLF copy of install.sh completed a dry run under Git Bash 5.2.37 (msys), exit 0. Git for Windows' bash tolerates CR. Linux bash would not; see Deferred.
   - Exit codes: success 0, `Fail` 1, sh unknown argument 2.
   - bash 3.2 (macOS): no empty-array expansion under `set -u`, BRE `\{1,\}`, `read -d ''` and `<<<` are all fine. `timeout` is absent there, so probes run without a timeout.
   - Execution policy here is CurrentUser RemoteSigned, so the ps1 runs.
7. **Messages.** Covered in finding 10 and in finding 1's ps1 crash.
   - The PROTOC failure text is clear and fires before any change.

## Evidence

Probe kit: session scratchpad `…\scratchpad\rv\`. `repo\scripts\` holds `git show 897c4a0:` copies of both scripts, `repo\target\release\konnect.exe` is a copy of the main checkout's build (`konnect 0.12.0`, sha `d0720347…`), and `repo\crates\konnect\src\manifest.rs` is copied from the worktree. Fixtures in `fx\`. Every real (non-dry) run used a temp `-Target` with `-SkipBuild -SkipInit -NoRetrace`, or failed at the build step. The worktree was not touched, and the real `konnect.exe` sha is `dea8824b…` before and after.

- Finding 1, sh: `tdir-sh/bin/konnect.exe` = `OLD-BINARY-SENTINEL`, then `install: ERROR: …/tdir-sh/bin does not match the build after copying; previous binary kept at (none).` `EXIT=1`. The directory then holds only `konnect.exe` at 36292096 bytes, starting `M Z 220`.
- Finding 1, ps1: `You cannot call a method on a null-valued expression. At …install.ps1:122 char:45`, `EXIT=1`. Directory unchanged (`konnect.exe` 19 bytes, `schematic-viewer.exe` 15 bytes).
- Running exe: `running pid=26816 hasExited=False` … `renamed …konnect.exe -> …konnect-unknown-20260922-100024.exe.bak`, `copied …`, `still running: True`. Rollback Move-Item lines give `konnect.exe E4224D18C3C9` and `…-rolledback.exe.bak D07203477671`. The sh equivalent gives the same with `rb exit=0`.
- Finding 2: second sh paste gives `mv: cannot stat '…konnect-unknown-20260922-100042.exe.bak'`, `rb exit=1`, and the directory holds only `konnect-0.12.0-20260922-100042-rolledback.exe.bak e4224d18c3c9`. Second ps1 paste gives `Cannot create a file when that file already exists.`, and the state is unchanged.
- Finding 5: `…\apos\O'Brien\bin` gives `Move-Item : Cannot move item because the item at '…\apos\O' does not exist.` on both lines.
- Finding 8: `capital` gives sh `no konnect MCP server registered` and ps1 `-> …\capital\konnect.exe`. `posix` gives sh `-> C:\Users\felip\konnect-probe-posix\konnect.exe` and ps1 `-> C:\c\Users\felip\konnect-probe-posix\konnect.exe`. Relative config: `Could not find file 'C:\Users\felip\Documents\FFS-Hardware-Eng\Konnect\shadow.json'`.
- Dry-run purity: `SNAPSHOT IDENTICAL`, both scripts exit 0. The ps1 plan: rename to `konnect-0.12.0-20260922-100119.exe.bak`, copy, `init`, `status --client claude`, `init --client codex`, `status --client codex`, and `would set RETRACE_PYTHON (user): (unset) -> …\.venv-retrace\Scripts\python.exe`. Five rollback lines.
- PROTOC: `PROTOC user=[] machine=[] process=[]`, `protoc on PATH: (none)`, `C:\Users\felip\tools\protoc\bin\protoc.exe exists=True`, all shims `False`. Real run in a temp git repo: `build needed: build (…09:58:54) is older than crates/konnect/src/manifest.rs (…10:04:32)`, then `install: ERROR: cannot build: - protoc not found …`, `exit=1`, and `stale-tgt` was never created.
- Finding 6: `include_str!("../../../docs/RELIABILITY_CONTRACT.md")` at `crates/konnect/src/manifest.rs:40`. `.cargo/config.toml` holds only the `xtask` alias.
- CRLF: `install-crlf.sh: … with CRLF line terminators`, then `EXIT=0` on `--dry-run`.

## For the next agent

1. Developer fix round, minimum for DONE:
   - Finding 1 in both scripts: refuse a directory target, and assert that the target does not exist between rename and copy.
   - Finding 2: a guarded single-line sh rollback.
   - Findings 3 to 5 are cheap to fix in the same pass. My recommendation: a summary function called from `Fail`, the real restore outcome, and quote escaping.
2. Re-verify with the `rv` kit pattern: fake repo in temp, temp `-Target`, `-SkipBuild -SkipInit -NoRetrace`.
   - Directory target: expect a refusal and an unchanged sentinel.
   - A double rollback paste.
   - An `O'Brien` path rollback.
   - For finding 3, use a temp-target stand-in whose `init` exits non-zero, e.g. a copy of `where.exe`, where `where init` exits 1. **Only** with a temp target: never run the real konnect's init.
3. The first real run on this machine needs no build. It renames the running 0.12.0 exe (supported), runs init against the real `~/.claude`, and sets User `RETRACE_PYTHON`. Tell the user to open a new terminal before restarting Claude Code (finding 10a).

## Deferred findings

1. `.gitattributes` has only `* text=auto`, and `core.autocrlf=true` here, so the main checkout will get a CRLF `install.sh`. Git Bash tolerates it (reproduced), Linux/WSL bash does not. Add `*.sh text eol=lf`. The developer's deferred item 2 agrees.
2. `~/.konnect/bin/schematic-viewer.exe` (2026-09-17) sits beside konnect.exe, and neither script builds or updates it. The main checkout has no release build of it either. Outside this brief, but "install it the way this PC runs it" leaves it stale.
3. `-Client codex` runs only the codex init, so the Claude guidance is not refreshed even though the Claude MCP binary was replaced. This is the developer's declared reading and needs an orchestrator decision.
4. An existing, *working* User `RETRACE_PYTHON` is overwritten with the repo venv. That matches the intake. It is announced, and it is in the rollback.
5. Add `-B` to the `import retrace` probe so a dry run can never write `__pycache__` (for example with an editable retrace install).
6. Staleness fails open when `git ls-files` fails (zip checkout, `safe.directory` "dubious ownership"). It warns, then installs a possibly stale build.
7. The sh exports PROTOC in `cygpath -m` form. If earlier builds used a backslash string, `build.rs` `rerun-if-env-changed=PROTOC` reruns the konnect-ipc build script (costs time only).
8. `.bak` files (about 34 MB each) are never pruned, by design. The summary could mention the folder.
