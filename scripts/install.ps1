# Build Konnect from this checkout and install it where Claude Code runs it.
#
# Local-build installer for developers and testers (Windows). The end-user
# path stays the KiCAD PCM zip built by packaging/build-pcm.ps1. Steps:
#
#   1. build     cargo build --release -p konnect, when target/release/konnect.exe
#                is missing, older than the newest git-tracked file under crates/
#                or Cargo.lock, or -Rebuild is given
#   2. target    -Target, else mcpServers.konnect.command in the Claude config
#                (top level first, then a single distinct project-level entry),
#                else ~/.konnect/bin/konnect.exe
#   3. install   identical binary -> skip; otherwise RENAME the old binary to
#                <name>-<version>-<yyyyMMdd-HHmmss>.exe.bak (never deleted),
#                then copy the new build into place
#   4. init      <target> init (Claude) and/or <target> init --client codex,
#                then <target> status --client <c>
#   5. retrace   point the user-level RETRACE_PYTHON at a Python that can
#                `import retrace` (default: <repo>/.venv-retrace)
#
# It never registers or edits an MCP server entry, never touches KiCAD
# projects and never deletes a binary.
#
# Usage:
#   ./scripts/install.ps1 [-Client claude|codex|both] [-Target PATH] `
#       [-ClaudeConfig PATH] [-SkipBuild | -Rebuild] [-SkipInit] `
#       [-RetracePython PATH | -NoRetrace] [-DryRun] [-Help]
#
# -DryRun prints every action and changes nothing. Runs on Windows PowerShell
# 5.1 and later; use scripts/install.sh on macOS/Linux or from Git Bash.

param(
    [ValidateSet("claude", "codex", "both")][string]$Client = "claude",
    [string]$Target = "",
    [string]$ClaudeConfig = "",
    [switch]$SkipBuild,
    [switch]$Rebuild,
    [switch]$SkipInit,
    [string]$RetracePython = "",
    [switch]$NoRetrace,
    [switch]$DryRun,
    [switch]$Help
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$userHome = [Environment]::GetFolderPath("UserProfile")

function Show-Usage {
    Write-Host @"
Usage: scripts/install.ps1 [-Client claude|codex|both] [-Target PATH]
                           [-ClaudeConfig PATH] [-SkipBuild | -Rebuild] [-SkipInit]
                           [-RetracePython PATH | -NoRetrace] [-DryRun] [-Help]

Builds Konnect from this checkout, installs it where Claude Code runs it,
installs the bundled guidance (konnect init) and points RETRACE_PYTHON at the
photo-intake Python.

  -Client         guidance to install: claude (default), codex or both
  -Target         install path of konnect.exe (default: the command registered
                  in the Claude config, else ~/.konnect/bin/konnect.exe)
  -ClaudeConfig   Claude config to read the target from (default ~/.claude.json)
  -SkipBuild      use the existing target/release/konnect.exe
  -Rebuild        build even when the existing build is current
  -SkipInit       do not run konnect init / status
  -RetracePython  Python to use for RETRACE_PYTHON (default <repo>/.venv-retrace)
  -NoRetrace      leave RETRACE_PYTHON alone
  -DryRun         print every action, change nothing
"@
}

function Fail([string]$message) {
    Write-Host "install: ERROR: $message" -ForegroundColor Red
    exit 1
}
function Warn([string]$message) { Write-Host "install: WARNING: $message" -ForegroundColor Yellow }
function Step([string]$title) { Write-Host ""; Write-Host "== $title" -ForegroundColor Cyan }
function Say([string]$message) { Write-Host "  $message" }

if ($Help) { Show-Usage; exit 0 }
if ($SkipBuild -and $Rebuild) { Fail "-SkipBuild and -Rebuild contradict each other; pass one." }
if ($NoRetrace -and $RetracePython) { Fail "-RetracePython and -NoRetrace contradict each other; pass one." }
if (-not $ClaudeConfig) { $ClaudeConfig = Join-Path $userHome ".claude.json" }

$changes = New-Object System.Collections.ArrayList
$rollback = New-Object System.Collections.ArrayList
$verb = "Changed"
if ($DryRun) { $verb = "Would change" }

# Run a program with stdin closed and a timeout, capturing its output. Used for
# read-only probes (--version, import retrace) so an old binary that does not
# know a flag cannot hang the installer waiting on stdin. $null = did not run.
function Invoke-Probe([string]$exe, [string]$arguments, [int]$timeoutMs = 15000) {
    try {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $exe
        $psi.Arguments = $arguments
        $psi.UseShellExecute = $false
        $psi.RedirectStandardInput = $true
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.CreateNoWindow = $true
        $proc = [System.Diagnostics.Process]::Start($psi)
        $proc.StandardInput.Close()
        $outTask = $proc.StandardOutput.ReadToEndAsync()
        $errTask = $proc.StandardError.ReadToEndAsync()
        if (-not $proc.WaitForExit($timeoutMs)) {
            try { $proc.Kill() } catch { }
            return $null
        }
        return [pscustomobject]@{ Code = $proc.ExitCode; Out = $outTask.Result.Trim(); Err = $errTask.Result.Trim() }
    } catch {
        return $null
    }
}

function Get-KonnectVersion([string]$exe) {
    $probe = Invoke-Probe $exe "--version"
    if ($probe -and $probe.Code -eq 0 -and $probe.Out -match 'konnect\s+(\S+)') { return $Matches[1] }
    return "unknown"
}

function Get-Sha256([string]$path) { return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLower() }

# Absolute, backslashed path; relative paths and ~ resolve against the current
# PowerShell location (not the process directory, which may differ).
function Get-NormalizedPath([string]$path) {
    try {
        $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path)
        return [System.IO.Path]::GetFullPath($resolved.Replace('/', '\'))
    } catch {
        return $path
    }
}

# ---- JSON access that works for both ConvertFrom-Json objects and the
# ---- dictionaries JavaScriptSerializer returns (fallback parser).
function Get-JsonMember($obj, [string]$name) {
    if ($null -eq $obj) { return $null }
    if ($obj -is [System.Collections.IDictionary]) {
        if ($obj.ContainsKey($name)) { return $obj[$name] }
        return $null
    }
    if ($obj -is [System.Management.Automation.PSCustomObject]) {
        $prop = $obj.PSObject.Properties[$name]
        if ($prop) { return $prop.Value }
    }
    return $null
}
function Get-JsonNames($obj) {
    if ($obj -is [System.Collections.IDictionary]) { return @($obj.Keys) }
    if ($obj -is [System.Management.Automation.PSCustomObject]) { return @($obj.PSObject.Properties | ForEach-Object { $_.Name }) }
    return @()
}
function Read-JsonFile([string]$path) {
    $text = [System.IO.File]::ReadAllText($path)
    try {
        return ($text | ConvertFrom-Json)
    } catch {
        # Windows PowerShell's ConvertFrom-Json rejects keys that differ only in
        # case (project paths can); JavaScriptSerializer keeps them apart.
        Add-Type -AssemblyName System.Web.Extensions
        $serializer = New-Object System.Web.Script.Serialization.JavaScriptSerializer
        $serializer.MaxJsonLength = [int]::MaxValue
        return $serializer.DeserializeObject($text)
    }
}
function Get-KonnectCommand($container) {
    $servers = Get-JsonMember $container "mcpServers"
    $konnect = Get-JsonMember $servers "konnect"
    $command = Get-JsonMember $konnect "command"
    if ($command -is [string] -and $command.Trim()) { return $command.Trim() }
    return $null
}
function Test-KonnectExecutablePath([string]$command) {
    try {
        if (-not [System.IO.Path]::IsPathRooted($command)) { return $false }
        return ([System.IO.Path]::GetFileName($command) -match '^konnect(\.exe)?$')
    } catch {
        return $false  # characters no path can hold: a shell command line
    }
}

# ---------------------------------------------------------------------------
Write-Host "Konnect local installer ($repoRoot)"
if ($DryRun) { Write-Host "DRY RUN - nothing will be changed." -ForegroundColor Yellow }

$manifestPath = Join-Path $repoRoot "crates\konnect\src\manifest.rs"
$expectedSkills = $null
$expectedAgents = $null
if (Test-Path -LiteralPath $manifestPath) {
    $expectedSkills = @(Select-String -LiteralPath $manifestPath -Pattern '^\s+SkillManifest \{' -CaseSensitive).Count
    $expectedAgents = @(Select-String -LiteralPath $manifestPath -Pattern '^\s+AgentManifest \{' -CaseSensitive).Count
}

# ---- 1. build --------------------------------------------------------------
Step "Build"
$source = Join-Path $repoRoot "target\release\konnect.exe"
$buildReason = $null
$staleReason = $null
if (Test-Path -LiteralPath $source) {
    $builtAt = [System.IO.File]::GetLastWriteTimeUtc($source)
    $tracked = @()
    try { $tracked = @(& git -C $repoRoot ls-files -- crates Cargo.lock 2>$null) } catch { $tracked = @() }
    if ($LASTEXITCODE -ne 0 -or $tracked.Count -eq 0) {
        Warn "could not list git-tracked sources; treating the existing build as current."
    } else {
        $newest = $null
        $newestAt = [datetime]::MinValue
        foreach ($rel in $tracked) {
            $full = Join-Path $repoRoot $rel
            if (-not [System.IO.File]::Exists($full)) { continue }
            $at = [System.IO.File]::GetLastWriteTimeUtc($full)
            if ($at -gt $newestAt) { $newestAt = $at; $newest = $rel }
        }
        if ($newestAt -gt $builtAt) {
            $staleReason = "build ($($builtAt.ToLocalTime().ToString('yyyy-MM-dd HH:mm:ss'))) is older than $newest ($($newestAt.ToLocalTime().ToString('yyyy-MM-dd HH:mm:ss')))"
        }
    }
}
if ($Rebuild) { $buildReason = "-Rebuild given" }
elseif (-not (Test-Path -LiteralPath $source)) { $buildReason = "no build at $source" }
elseif ($staleReason) { $buildReason = $staleReason }

if ($SkipBuild) {
    if (-not (Test-Path -LiteralPath $source)) { Fail "-SkipBuild given but there is no build at $source." }
    if ($staleReason) { Warn "-SkipBuild: installing a stale build - $staleReason." }
    Say "skipped (-SkipBuild); using $source"
} elseif (-not $buildReason) {
    Say "up to date: $source"
} else {
    Say "build needed: $buildReason"
    $problems = New-Object System.Collections.ArrayList
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) { [void]$problems.Add("cargo not found on PATH; install Rust from https://rustup.rs") }

    $protoc = $null
    if ($env:PROTOC) {
        if (Test-Path -LiteralPath $env:PROTOC) { $protoc = $env:PROTOC }
        else { [void]$problems.Add("PROTOC is set to '$($env:PROTOC)' but that file does not exist") }
    } else {
        $protocCmd = Get-Command protoc -ErrorAction SilentlyContinue
        if ($protocCmd) { $protoc = $protocCmd.Path }
        else { [void]$problems.Add("protoc not found: set PROTOC to protoc.exe (e.g. `$env:PROTOC = 'C:\path\to\protoc\bin\protoc.exe') or put protoc on PATH - https://github.com/protocolbuffers/protobuf/releases") }
    }

    $cmakeDir = $null
    if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
        $roots = @(${env:ProgramFiles(x86)}, $env:ProgramFiles) | Where-Object { $_ }
        foreach ($root in $roots) {
            $pattern = Join-Path $root "Microsoft Visual Studio\*\*\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
            $hit = @(Get-Item -Path $pattern -ErrorAction SilentlyContinue |
                Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "cmake.exe") } |
                Sort-Object FullName -Descending) | Select-Object -First 1
            if ($hit) { $cmakeDir = $hit.FullName; break }
        }
        if (-not $cmakeDir) { [void]$problems.Add("cmake not found on PATH nor in a Visual Studio install; install CMake (https://cmake.org/download/) or the VS Build Tools C++ workload") }
    }

    if ($problems.Count -gt 0) {
        if ($DryRun) { foreach ($p in $problems) { Warn "the build would fail: $p" } }
        else { Fail ("cannot build:`n  - " + ($problems -join "`n  - ")) }
    }
    if ($protoc) { Say "PROTOC = $protoc" } else { Say "PROTOC = (not found)" }
    if ($cmakeDir) { Say "CMake  = $cmakeDir (prepended to PATH for the build only)" } else { Say "CMake  = on PATH" }

    if ($DryRun) {
        Say "would run: cargo build --release -p konnect (in $repoRoot)"
    } else {
        $savedPath = $env:PATH
        $savedProtoc = $env:PROTOC
        $code = 1
        Push-Location $repoRoot
        try {
            $env:PROTOC = $protoc
            if ($cmakeDir) { $env:PATH = "$cmakeDir;$env:PATH" }
            & cargo build --release -p konnect
            $code = $LASTEXITCODE
        } finally {
            Pop-Location
            $env:PATH = $savedPath
            $env:PROTOC = $savedProtoc
        }
        if ($code -ne 0) { Fail "cargo build --release -p konnect failed (exit $code)." }
        if (-not (Test-Path -LiteralPath $source)) { Fail "the build finished but $source is missing." }
        [void]$changes.Add("built $source")
    }
}
$sourceVersion = "(built by the real run)"
if ((Test-Path -LiteralPath $source) -and -not ($DryRun -and $buildReason -and -not $SkipBuild)) {
    $sourceVersion = Get-KonnectVersion $source
}
Say "new build version: $sourceVersion"

# ---- 2. install target -----------------------------------------------------
Step "Install target"
$targetNote = $null
if ($Target) {
    $targetPath = Get-NormalizedPath $Target
    Say "from -Target: $targetPath"
} else {
    $found = $null
    $foundWhere = $null
    if (Test-Path -LiteralPath $ClaudeConfig) {
        try { $config = Read-JsonFile $ClaudeConfig } catch { Fail "cannot parse $ClaudeConfig ($($_.Exception.Message)); pass -Target." }
        $top = Get-KonnectCommand $config
        if ($top) {
            if (-not (Test-KonnectExecutablePath $top)) {
                Fail "the top-level mcpServers.konnect.command in $ClaudeConfig is '$top', not a path to a konnect executable (a wrapper or a command with arguments). Pass -Target <path to konnect.exe>."
            }
            $found = Get-NormalizedPath $top
            $foundWhere = "top-level mcpServers.konnect.command in $ClaudeConfig"
        } else {
            $projects = Get-JsonMember $config "projects"
            $distinct = [ordered]@{}
            $projectOf = @{}
            foreach ($name in (Get-JsonNames $projects)) {
                $cmd = Get-KonnectCommand (Get-JsonMember $projects $name)
                if (-not $cmd) { continue }
                if (-not (Test-KonnectExecutablePath $cmd)) {
                    Fail "project '$name' in $ClaudeConfig registers konnect as '$cmd', not a path to a konnect executable (a wrapper or a command with arguments). Pass -Target <path to konnect.exe>."
                }
                $norm = Get-NormalizedPath $cmd
                $key = $norm.ToLowerInvariant()
                if (-not $distinct.Contains($key)) { $distinct[$key] = $norm; $projectOf[$key] = $name }
            }
            if ($distinct.Count -gt 1) {
                $listing = @($distinct.Keys | ForEach-Object { "$($distinct[$_])   (project $($projectOf[$_]))" })
                Fail ("$ClaudeConfig registers konnect at several paths:`n  - " + ($listing -join "`n  - ") + "`nPass -Target <path> to choose one.")
            }
            if ($distinct.Count -eq 1) {
                $found = @($distinct.Values)[0]
                $foundWhere = "the only project-level mcpServers.konnect.command in $ClaudeConfig"
            }
        }
    } else {
        Say "no Claude config at $ClaudeConfig"
    }
    if ($found) {
        $targetPath = $found
        Say "from $foundWhere"
        Say "-> $targetPath"
    } else {
        $targetPath = Join-Path $userHome ".konnect\bin\konnect.exe"
        Say "no konnect MCP server registered in the Claude config; using the default $targetPath"
        $targetNote = "Konnect is not registered with Claude Code. To register it (this installer never does), run:`n    claude mcp add --scope user konnect -- `"$targetPath`""
        Say "NOTE: $targetNote"
    }
}

# ---- 3. backup + copy ------------------------------------------------------
Step "Install binary"
$backupPath = $null
$targetExisted = Test-Path -LiteralPath $targetPath
$oldVersion = $null
# Only a dry run can reach here without a current build (a real run built it or
# failed); the hash of a build that is about to be replaced proves nothing.
$pending = (-not (Test-Path -LiteralPath $source)) -or ($DryRun -and $buildReason -and -not $SkipBuild)
$newTag = $sourceVersion
if ($pending) { $newTag = "new"; Say "the new build does not exist yet (dry run) - showing what the real run would do" }
$identical = $false
if (-not $pending) {
    $srcHash = Get-Sha256 $source
    Say "new build sha256: $srcHash"
    $identical = $targetExisted -and ((Get-Sha256 $targetPath) -eq $srcHash)
}
if ($identical) {
    Say "identical binary already at $targetPath - skipping backup and copy"
} else {
    $targetDir = Split-Path -Parent $targetPath
    if ($targetExisted) {
        $oldVersion = Get-KonnectVersion $targetPath
        Say "existing binary: version $oldVersion, sha256 $(Get-Sha256 $targetPath)"
        $base = [System.IO.Path]::GetFileNameWithoutExtension($targetPath)
        $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
        $backupPath = Join-Path $targetDir "$base-$oldVersion-$stamp.exe.bak"
        $n = 2
        while (Test-Path -LiteralPath $backupPath) {
            $backupPath = Join-Path $targetDir "$base-$oldVersion-$stamp-$n.exe.bak"
            $n++
        }
        if ($DryRun) {
            Say "would rename $targetPath -> $backupPath"
        } else {
            try { Move-Item -LiteralPath $targetPath -Destination $backupPath }
            catch { Fail "could not rename $targetPath to $backupPath ($($_.Exception.Message)); nothing was changed." }
            Say "renamed $targetPath -> $backupPath"
        }
    }
    if ($DryRun) {
        Say "would copy $source -> $targetPath"
    } else {
        try {
            if (-not (Test-Path -LiteralPath $targetDir)) { New-Item -ItemType Directory -Path $targetDir -Force | Out-Null }
            Copy-Item -LiteralPath $source -Destination $targetPath
        } catch {
            $why = $_.Exception.Message
            if ($backupPath) {
                try { Move-Item -LiteralPath $backupPath -Destination $targetPath } catch { }
                Fail "copy to $targetPath failed ($why); the previous binary was renamed back."
            }
            Fail "copy to $targetPath failed ($why)."
        }
        if ((Get-Sha256 $targetPath) -ne $srcHash) { Fail "$targetPath does not match the build after copying; previous binary kept at $backupPath." }
        Say "copied $source -> $targetPath"
    }
    if ($backupPath) {
        [void]$changes.Add("$verb $targetPath (old binary $oldVersion renamed to $backupPath)")
        $parked = Join-Path $targetDir "$base-$newTag-$stamp-rolledback.exe.bak"
        [void]$rollback.Add("Move-Item -LiteralPath '$targetPath' -Destination '$parked'")
        [void]$rollback.Add("Move-Item -LiteralPath '$backupPath' -Destination '$targetPath'")
    } else {
        [void]$changes.Add("$verb $targetPath (new file, no previous binary)")
    }
}

# ---- 4. init + status ------------------------------------------------------
$clients = @($Client)
if ($Client -eq "both") { $clients = @("claude", "codex") }
Step "Guidance (konnect init)"
if ($SkipInit) {
    Say "skipped (-SkipInit)"
} else {
    foreach ($c in $clients) {
        $initArgs = @("init")
        if ($c -eq "codex") { $initArgs = @("init", "--client", "codex") }
        if ($DryRun) {
            Say "would run: `"$targetPath`" $($initArgs -join ' ')"
            Say "would run: `"$targetPath`" status --client $c"
            continue
        }
        Say "running: `"$targetPath`" $($initArgs -join ' ')"
        $initOut = @(& $targetPath @initArgs)
        $initCode = $LASTEXITCODE
        $initOut | ForEach-Object { Write-Host "    $_" }
        if ($initCode -ne 0) { Fail "konnect $($initArgs -join ' ') failed (exit $initCode)." }
        [void]$changes.Add("installed $c guidance ($targetPath $($initArgs -join ' '))")
        if ($c -eq "claude") {
            $done = $initOut | Where-Object { $_ -match '^Done: (\d+) skills, (\d+) agents, (\d+) hooks installed for Claude\.' } | Select-Object -Last 1
            if (-not $done) {
                Warn "konnect init printed no 'Done: ... installed for Claude.' line; counts not checked."
            } else {
                [void]($done -match '^Done: (\d+) skills, (\d+) agents, (\d+) hooks')
                $gotSkills = [int]$Matches[1]
                $gotAgents = [int]$Matches[2]
                if ($null -eq $expectedSkills) {
                    Warn "no manifest at $manifestPath; counts not checked."
                } elseif ($gotSkills -ne $expectedSkills -or $gotAgents -ne $expectedAgents) {
                    Warn "init installed $gotSkills skills / $gotAgents agents but manifest.rs lists $expectedSkills / $expectedAgents - is the installed build current?"
                } else {
                    Say "counts match manifest.rs: $gotSkills skills, $gotAgents agents"
                }
            }
        }
        Say "running: `"$targetPath`" status --client $c"
        & $targetPath status --client $c | ForEach-Object { Write-Host "    $_" }
        if ($LASTEXITCODE -ne 0) { Warn "konnect status --client $c exited $LASTEXITCODE." }
    }
}

# ---- 5. RETRACE_PYTHON -----------------------------------------------------
Step "Photo intake (RETRACE_PYTHON)"
$retraceNote = $null
if ($NoRetrace) {
    Say "skipped (-NoRetrace)"
} else {
    $py = $RetracePython
    if (-not $py) {
        $venvPy = Join-Path $repoRoot ".venv-retrace\Scripts\python.exe"
        if (Test-Path -LiteralPath $venvPy) { $py = $venvPy }
    }
    $retraceVersion = $null
    if ($py -and (Test-Path -LiteralPath $py)) {
        $py = Get-NormalizedPath $py
        $probe = Invoke-Probe $py "-c `"import retrace; print(getattr(retrace, '__version__', 'unknown'))`""
        if ($probe -and $probe.Code -eq 0) { $retraceVersion = $probe.Out }
        elseif ($probe) { Warn "$py cannot import retrace: $($probe.Err)" }
        else { Warn "$py did not run." }
    } elseif ($py) {
        Warn "-RetracePython $py does not exist."
    }
    if (-not $retraceVersion) {
        $retraceNote = "No Python with retrace found; photo intake stays unconfigured. See docs/PHOTO_TO_BOARD_WORKFLOW.md to create .venv-retrace, then re-run this script."
        Say "NOTE: $retraceNote"
    } else {
        Say "retrace $retraceVersion importable from $py"
        $current = [Environment]::GetEnvironmentVariable("RETRACE_PYTHON", "User")
        if ($current -and ((Get-NormalizedPath $current).ToLowerInvariant() -eq $py.ToLowerInvariant())) {
            Say "RETRACE_PYTHON (user) already $current - unchanged"
        } else {
            $shown = "(unset)"
            if ($current) { $shown = $current }
            if ($DryRun) {
                Say "would set RETRACE_PYTHON (user): $shown -> $py"
            } else {
                [Environment]::SetEnvironmentVariable("RETRACE_PYTHON", $py, "User")
                Say "set RETRACE_PYTHON (user): $shown -> $py"
            }
            [void]$changes.Add("$verb RETRACE_PYTHON (user): $shown -> $py")
            if ($current) { [void]$rollback.Add("[Environment]::SetEnvironmentVariable('RETRACE_PYTHON', '$current', 'User')") }
            else { [void]$rollback.Add("[Environment]::SetEnvironmentVariable('RETRACE_PYTHON', `$null, 'User')") }
        }
    }
}

# ---- summary ---------------------------------------------------------------
Step "Summary"
if ($DryRun) { Say "DRY RUN - nothing was changed." }
if ($changes.Count -eq 0) { Say "nothing changed." }
foreach ($line in $changes) { Say "- $line" }
Say "target:  $targetPath (new build version $sourceVersion)"
if ($backupPath) { Say "backup:  $backupPath" } else { Say "backup:  (none)" }
if ($targetNote) { Say "NOTE: $targetNote" }
if ($retraceNote) { Say "NOTE: $retraceNote" }
if ($rollback.Count -gt 0) {
    Say "rollback (PowerShell):"
    foreach ($line in $rollback) { Say "    $line" }
    if ($backupPath) {
        foreach ($c in $clients) {
            if ($c -eq "codex") { Say "    & '$targetPath' init --client codex" } else { Say "    & '$targetPath' init" }
        }
    }
} else {
    Say "rollback: nothing to roll back."
}
Say "Restart Claude Code (all windows) so the MCP server and the skills reload."
exit 0
