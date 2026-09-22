---
change: install-scripts
task: DECISION O post-verify fix - guarded ps1 rollback, dry-run preview, all five PowerShell single quotes
agent: developer
verdict: DONE
---

## Result

Commit `6a2f7d2` on `orc/install-scripts` (parent `334a8ac`). Only `scripts/install.ps1` changed (+25/-10). The file is still ASCII (0 bytes > 127) and LF (0 CR bytes, `i/lf w/lf`). Nothing pushed.

1. **Guarded rollback (MINOR 1).** `Get-RollbackLines` prints the pair as ONE statement, followed by the separate `& '<t>' init` line(s):
   `if ((Test-Path -LiteralPath B) -and (Test-Path -LiteralPath T) -and -not (Test-Path -LiteralPath P)) { Move-Item T P -ErrorAction Stop; Move-Item B T } else { 'rollback stopped: nothing to undo, already undone, or the files were moved - check the folder' }`.
   - If the target is missing when the rollback is printed (copy failed and restore failed), it prints `if ((B) -and -not (T)) { Move-Item B T } else {…}`.
   - The fresh-install move is guarded as `if ((T) -and -not (P)) {…} else { 'undo stopped: …' }`, the same wording as the sh.
2. **Dry run = preview.** `Get-RollbackLines` returns nothing under `-DryRun`, so neither the summary nor `Show-FailureRecovery` can print a pasteable command in a dry run. The summary now reads:
   - `DRY RUN (preview) - nothing was changed; this is what a real run would do.`
   - `backup (would be):  <path>`
   - `undo: preview only - nothing to undo now. After a real run, the summary will show how to undo it.`
3. **Quotes (MINOR 2).** `Format-PsLiteral` doubles each character in `$psSingleQuotes = @([char]0x27, [char]0x2018, [char]0x2019, [char]0x201A, [char]0x201B)`.
4. `install.sh` is untouched.

## Evidence

The harness is `<session scratchpad>\t7.ps1`. It runs install.ps1 in-process and captures `6>&1`, then pastes each line through `& powershell -NoProfile -Command $line`. It has fakes from `rv2\bin`: old = `DC5887E1` (0.11.0-old), new = `1FBA2B3D` (0.99.0-new, via `CARGO_TARGET_DIR=<tmp>\ctd` + `-SkipBuild`). Every real run used a temp `-Target` + `-SkipInit` + `-NoRetrace`. The dry run used `-DryRun` + a temp `-Target`.
1. **Paste twice (A).** After the run: `konnect.exe=1FBA2B3D …-old-…exe.bak=DC5887E1`.
   - paste1: `(no output)`, then `konnect.exe=DC5887E1 …-new-…-rolledback.exe.bak=1FBA2B3D`, `restored exact: True`.
   - paste2 and paste3: `rollback stopped: …`, with the state identical.
   - **Nothing to undo (B).** A real run, then the backup moved out of the folder, then a paste: `rollback stopped: …`. `konnect.exe=1FBA2B3D` before and after. The old unguarded pair left no konnect.exe in this case.
2. **Dry run (C)** against target `a`, which holds a binary: `Move-Item lines: 0`, `'safe to paste twice' count: 0`, `state unchanged: True`. It prints the three preview lines quoted in Result 2.
3. **O'Brien (D).** Target `…\d7\O<U+2019>Brien\konnect.exe`:
   - `line has U+2019 doubled: True`, `parse errors (new line): 0` (ParseInput).
   - The old rule on the same path gives `parse errors (old Format-PsLiteral): 1`.
   - paste1: `restored exact: True`. paste2: `rollback stopped`, state unchanged.
   - A string holding all five quote characters gives `parse errors=0 roundtrip=True`.
   - **Fresh install (E).** paste1 parks the new file. paste2: `undo stopped: …`, state unchanged.
4. **Parser.** `ParseFile` under PS 5.1.26100.9444 gives `file parse errors: 0`. `git diff --name-only 334a8ac..HEAD` = `scripts/install.ps1`, one commit. The temp dir `d7\` was removed (`exists=False`), and `CARGO_TARGET_DIR` was removed from the harness process.
5. **Real machine, before and after.** `~\.konnect\bin\konnect.exe` sha256 `DEA8824B1357F6587FA98792D8246C6516CBD5A12F30ECEDB52F31B7A447DEC5` both times. `RETRACE_PYTHON` user, machine and process were empty both times. The bin folder is unchanged (`konnect-0.12.0-2026-09-17.exe.bak, konnect.exe, schematic-viewer.exe`).

## For the next agent

1. The guard also checks `-not (Test-Path P)`, which the brief's example left out. That mirrors the sh (`[ ! -e parked ]`) and gives the friendly message instead of a Move-Item error. `-ErrorAction Stop` on the first move aborts the whole statement if the park fails, so the restore never runs over a file that is still there.
2. **Stale block.** An older real run's block pasted after a newer real run still passes the guard: its backup exists, the target exists and its parked name is free. It parks the newer binary under the older run's name and restores the pre-first-run binary. Nothing is lost, but the label is wrong. A hash guard would stop this, but it was rejected because it would also block the legitimate restore over a PARTIAL file (round-1 item 4).
3. The `Show-FailureRecovery` label "safe to paste twice" is still printed, but only after a real change. A dry-run failure now prints only the error line.
4. **Harness gotcha.** `Out-String` wraps captured Write-Host lines at 120 characters. Use `ForEach-Object { "$_" } | Out-String -Width 8192`. Capture in-process: an out-of-process capture turns U+2019 into `?`.

## Deferred findings

1. The top-of-run banner still says `DRY RUN - nothing will be changed.` (pre-existing, not touched). It is consistent with the new preview label.
2. The stale-block relabel (For the next agent item 2) is cosmetic. It needs two real runs and a paste from the older run.
