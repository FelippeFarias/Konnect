# Build Konnect from this checkout and install it where Claude Code runs it.
#
# Local-build installer for developers and testers (Windows). The end-user
# path stays the KiCAD PCM zip built by packaging/build-pcm.ps1. Steps:
#
#   0. bootstrap -Bootstrap only: install cargo, protoc and cmake with winget
#                (or choco) when they are missing, and register the konnect MCP
#                server for each selected client. -WithRetrace also creates
#                <repo>\.venv-retrace and pip installs retrace into it
#   1. build     cargo build --release -p konnect, when target/release/konnect.exe
#                ($env:CARGO_TARGET_DIR\release when set) is missing, older than
#                the newest git-tracked build input (crates/, Cargo.lock,
#                Cargo.toml, rust-toolchain.toml, docs/RELIABILITY_CONTRACT.md),
#                or -Rebuild is given
#   2. target    -Target, else mcpServers.konnect.command in the Claude config
#                (top level first, then a single distinct project-level entry),
#                else ~/.konnect/bin/konnect.exe
#   3. install   identical binary -> skip; otherwise RENAME the old binary to
#                <name>-<version>-<yyyyMMdd-HHmmss>.exe.bak (never deleted),
#                then copy the new build into place
#   4. init      <target> init (Claude) and/or <target> init --client codex|omp,
#                then <target> status --client <c>; the default installs the
#                Claude and the OMP guidance
#   5. retrace   point the user-level RETRACE_PYTHON at a Python that can
#                `import retrace` (default: <repo>/.venv-retrace)
#
# Without -Bootstrap it never registers or edits an MCP server entry. It never
# touches KiCAD projects and never deletes a binary.
#
# Usage:
#   ./scripts/install.ps1 [-Client claude,codex,omp|both|all] [-Target PATH] `
#       [-ClaudeConfig PATH] [-SkipBuild | -Rebuild] [-SkipInit] `
#       [-RetracePython PATH | -NoRetrace] [-Bootstrap] [-WithRetrace] `
#       [-DryRun] [-Help]
#
# -DryRun prints every action and changes nothing. Runs on Windows PowerShell
# 5.1 and later; use scripts/install.sh on macOS/Linux or from Git Bash.
# Unknown or misspelled options are rejected before anything runs.

[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Client = "claude,omp",
    [string]$Target = "",
    [string]$ClaudeConfig = "",
    [switch]$SkipBuild,
    [switch]$Rebuild,
    [switch]$SkipInit,
    [string]$RetracePython = "",
    [switch]$NoRetrace,
    [switch]$Bootstrap,
    [switch]$WithRetrace,
    [switch]$DryRun,
    [switch]$Help
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$userHome = [Environment]::GetFolderPath("UserProfile")

function Show-Usage {
    Write-Host @"
Usage: scripts/install.ps1 [-Client claude,codex,omp|both|all] [-Target PATH]
                           [-ClaudeConfig PATH] [-SkipBuild | -Rebuild] [-SkipInit]
                           [-RetracePython PATH | -NoRetrace] [-Bootstrap]
                           [-WithRetrace] [-DryRun] [-Help]

Builds Konnect from this checkout, installs it where Claude Code runs it,
installs the bundled guidance (konnect init) and points RETRACE_PYTHON at the
photo-intake Python.

  -Client         guidance to install (default claude,omp): a comma- or
                  space-separated list of claude, codex and omp, or both
                  (claude + codex) or all (claude + codex + omp). omp writes
                  the guidance the OMP CLI reads from ~/.omp/agent/.
  -Target         install path of konnect.exe (default: the command registered
                  in the Claude config, else ~/.konnect/bin/konnect.exe)
  -ClaudeConfig   Claude config to read the target from (default ~/.claude.json)
  -SkipBuild      use the existing target/release/konnect.exe
  -Rebuild        build even when the existing build is current
  -SkipInit       do not run konnect init / status
  -RetracePython  Python to use for RETRACE_PYTHON (default <repo>/.venv-retrace)
  -NoRetrace      leave RETRACE_PYTHON alone
  -Bootstrap      fresh machine: install cargo, protoc and cmake when they are
                  missing, then register the konnect MCP server for each
                  selected client
  -WithRetrace    create <repo>\.venv-retrace and pip install the optional
                  retrace package into it
  -DryRun         print every action, change nothing

Run it from an open PowerShell window, for example:
  powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -DryRun
not with Explorer's "Run with PowerShell": that window closes when the script
ends and takes the summary and the rollback commands with it.
"@
}

# ---- state the rollback is computed from (set only once a change happened)
$changes = New-Object System.Collections.ArrayList
$initClients = New-Object System.Collections.ArrayList   # clients whose init ran (or would run)
$targetPath = $null
$backupPath = $null      # set only after the old binary was really renamed (or would be)
$parkedPath = $null      # where the rollback moves the new binary aside
$freshInstall = $false   # a new binary was (or would be) placed where none existed
$retraceChanged = $false
$retracePrevious = $null
$protocChanged = $false
$protocPrevious = $null
$mcpAdded = New-Object System.Collections.ArrayList   # clients whose MCP entry this run created
$mcpOmpFile = $null      # the OMP mcp.json that was written, when one was
$mcpOmpBackup = $null    # its pre-run copy, when the file already existed
$recoveryShown = $false

# A PowerShell single-quoted literal. PowerShell reads each of these five
# characters as a single quote (' and the typographic quotes), so each doubles.
$psSingleQuotes = @([char]0x27, [char]0x2018, [char]0x2019, [char]0x201A, [char]0x201B)
function Format-PsLiteral([string]$text) {
    foreach ($q in $psSingleQuotes) { $text = $text.Replace([string]$q, [string]$q + [string]$q) }
    return "'" + $text + "'"
}

# konnect names every client but Claude, which is the bare default: `init` and
# `uninstall` for claude, `init --client <c>` for codex and omp.
function Get-ClientArgs([string]$verb, [string]$client) {
    if ($client -eq "claude") { return $verb }
    return "$verb --client $client"
}

# Commands that undo what this run changed, in paste order. Each is safe to
# paste twice: the file moves sit behind one Test-Path guard, so a second paste,
# or a paste when there is nothing to undo, moves nothing and says why. A dry
# run changed nothing, so it gets no commands at all.
$rollbackStopped = "'rollback stopped: nothing to undo, already undone, or the files were moved - check the folder'"
function Get-RollbackLines {
    $lines = @()
    if ($DryRun) { return $lines }
    if ($targetPath) { $t = Format-PsLiteral $targetPath }
    if ($parkedPath) { $p = Format-PsLiteral $parkedPath }
    if ($backupPath) {
        $b = Format-PsLiteral $backupPath
        if (Test-Path -LiteralPath $targetPath) {
            $lines += "if ((Test-Path -LiteralPath $b) -and (Test-Path -LiteralPath $t) -and -not (Test-Path -LiteralPath $p)) { Move-Item -LiteralPath $t -Destination $p -ErrorAction Stop; Move-Item -LiteralPath $b -Destination $t } else { $rollbackStopped }"
        } else {
            $lines += "if ((Test-Path -LiteralPath $b) -and -not (Test-Path -LiteralPath $t)) { Move-Item -LiteralPath $b -Destination $t } else { $rollbackStopped }"
        }
        foreach ($c in $initClients) { $lines += "& $t $(Get-ClientArgs 'init' $c)" }
    } elseif ($freshInstall) {
        foreach ($c in $initClients) { $lines += "& $t $(Get-ClientArgs 'uninstall' $c)" }
        $lines += "if ((Test-Path -LiteralPath $t) -and -not (Test-Path -LiteralPath $p)) { Move-Item -LiteralPath $t -Destination $p } else { 'undo stopped: already done, or the file was moved - check the folder' }"
    }
    if ($retraceChanged) {
        if ($retracePrevious) { $lines += "[Environment]::SetEnvironmentVariable('RETRACE_PYTHON', $(Format-PsLiteral $retracePrevious), 'User')" }
        else { $lines += "[Environment]::SetEnvironmentVariable('RETRACE_PYTHON', `$null, 'User')" }
    }
    if ($protocChanged) {
        if ($protocPrevious) { $lines += "[Environment]::SetEnvironmentVariable('PROTOC', $(Format-PsLiteral $protocPrevious), 'User')" }
        else { $lines += "[Environment]::SetEnvironmentVariable('PROTOC', `$null, 'User')" }
    }
    foreach ($c in $mcpAdded) {
        if ($c -eq "claude") { $lines += "claude mcp remove konnect -s user" }
        elseif ($c -eq "codex") { $lines += "codex mcp remove konnect" }
        elseif ($c -eq "omp") {
            if ($mcpOmpBackup) { $lines += "Move-Item -Force -LiteralPath $(Format-PsLiteral $mcpOmpBackup) -Destination $(Format-PsLiteral $mcpOmpFile)" }
            else { $lines += "Remove-Item -LiteralPath $(Format-PsLiteral $mcpOmpFile)" }
        }
    }
    return $lines
}

# After a failure: say what already changed and how to undo it.
function Show-FailureRecovery {
    if ($script:recoveryShown) { return }
    $script:recoveryShown = $true
    $lines = @(Get-RollbackLines)
    if ($lines.Count -eq 0) { return }
    Write-Host ""
    Write-Host "== Stopped - changes made before the error" -ForegroundColor Cyan
    foreach ($line in $changes) { Write-Host "  - $line" }
    if ($backupPath) { Write-Host "  previous binary: $backupPath" }
    Write-Host "  to undo, paste these lines into PowerShell (safe to paste twice):"
    foreach ($line in $lines) { Write-Host "      $line" }
}

function Fail([string]$message) {
    Write-Host "install: ERROR: $message" -ForegroundColor Red
    Show-FailureRecovery
    exit 1
}
function Warn([string]$message) { Write-Host "install: WARNING: $message" -ForegroundColor Yellow }
function Step([string]$title) { Write-Host ""; Write-Host "== $title" -ForegroundColor Cyan }
function Say([string]$message) { Write-Host "  $message" }

# Any unexpected error after a change still prints the rollback.
trap {
    Write-Host "install: ERROR: unexpected error: $($_.Exception.Message) (at $($_.InvocationInfo.ScriptName):$($_.InvocationInfo.ScriptLineNumber))" -ForegroundColor Red
    Show-FailureRecovery
    exit 1
}

if ($Help) { Show-Usage; exit 0 }
if ($SkipBuild -and $Rebuild) { Fail "-SkipBuild and -Rebuild contradict each other; pass one." }
if ($NoRetrace -and $RetracePython) { Fail "-RetracePython and -NoRetrace contradict each other; pass one." }

# ---- the clients to install guidance for. Single values, the legacy "both"
# (claude + codex), "all", or a comma- or space-separated list; matched without
# regard to case and reduced to one stable order so the init, the rollback and
# the summary all agree on it. Validated here, not with a ValidateSet on the
# parameter, because a set cannot describe a list.
$clientOrder = @("claude", "codex", "omp")
$seenClients = @()
foreach ($token in ($Client -split '[,\s]+')) {
    if (-not $token) { continue }
    switch ($token.ToLowerInvariant()) {
        "claude" { $seenClients += "claude" }
        "codex"  { $seenClients += "codex" }
        "omp"    { $seenClients += "omp" }
        "both"   { $seenClients += @("claude", "codex") }
        "all"    { $seenClients += $clientOrder }
        default  { Fail "-Client must be claude, codex, omp, both (claude + codex), all, or a comma-separated list of them (got '$token')." }
    }
}
$clients = @($clientOrder | Where-Object { $seenClients -contains $_ })
if ($clients.Count -eq 0) { Fail "-Client needs at least one of claude, codex, omp, both or all." }

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

# A POSIX drive path from Git Bash/MSYS/Cygwin (/c/Users/..., /cygdrive/c/...)
# as its Windows form (C:\Users\...); any other text comes back unchanged.
function ConvertFrom-PosixDrivePath([string]$path) {
    if ($path -match '^/(?:cygdrive/)?([A-Za-z])/(.*)$') {
        return $Matches[1].ToUpperInvariant() + ":\" + $Matches[2].Replace('/', '\')
    }
    return $path
}

# Absolute, backslashed path; relative paths and ~ resolve against the current
# PowerShell location (not the process directory, which may differ).
function Get-NormalizedPath([string]$path) {
    $path = ConvertFrom-PosixDrivePath $path
    try {
        $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path)
        return [System.IO.Path]::GetFullPath($resolved.Replace('/', '\'))
    } catch {
        return $path
    }
}

# ---- JSON access that works for both ConvertFrom-Json objects and the
# ---- dictionaries JavaScriptSerializer returns (fallback parser). Names match
# ---- exactly, case included, as JSON keys do for Claude Code.
function Get-JsonMember($obj, [string]$name) {
    if ($null -eq $obj) { return $null }
    if ($obj -is [System.Collections.IDictionary]) {
        if ($obj.ContainsKey($name)) { return $obj[$name] }
        return $null
    }
    if ($obj -is [System.Management.Automation.PSCustomObject]) {
        foreach ($prop in $obj.PSObject.Properties) {
            if ($prop.Name -ceq $name) { return $prop.Value }
        }
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
# True for a path with no drive letter and no \\server (\tools\x, /usr/bin/x).
function Test-DrivelessPath([string]$path) { return ($path -match '^[\\/](?![\\/])') }

# The install path a config command names; stops when it is not one.
function Get-ConfigTargetPath([string]$command, [string]$where) {
    $converted = ConvertFrom-PosixDrivePath $command
    if (Test-DrivelessPath $converted) {
        Fail "$where is '$command', a path without a drive letter; Windows cannot tell where it points. Pass -Target <path to konnect.exe>."
    }
    if (-not (Test-KonnectExecutablePath $converted)) {
        Fail "$where is '$command', not a path to a konnect executable (a wrapper or a command with arguments). Pass -Target <path to konnect.exe>."
    }
    if ($converted -ne $command) { Say "NOTE: read the POSIX-style path '$command' as '$converted'" }
    return (Get-NormalizedPath $converted)
}

if (-not $ClaudeConfig) { $ClaudeConfig = Join-Path $userHome ".claude.json" }
$ClaudeConfig = Get-NormalizedPath $ClaudeConfig

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

# ---- bootstrap helpers -----------------------------------------------------
function Test-Cmd([string]$name) { return [bool](Get-Command $name -ErrorAction SilentlyContinue) }

# Echo then run, or only echo under -DryRun. True means the command succeeded.
function Invoke-Run([string]$exe, [string[]]$argList) {
    if ($DryRun) { Say "would run: $exe $($argList -join ' ')"; return $true }
    Say "running: $exe $($argList -join ' ')"
    & $exe @argList
    return ($LASTEXITCODE -eq 0)
}

# A package manager can put a tool where this already-running window will not
# look: rustup writes ~\.cargo\bin, winget shims live under WinGet\Links, and
# CMake and protoc install outside both. Re-probe those places so the build in
# this same run can still find what was just installed.
function Update-ToolPath {
    $candidates = New-Object System.Collections.ArrayList
    [void]$candidates.Add((Join-Path $userHome ".cargo\bin"))
    [void]$candidates.Add((Join-Path $env:ProgramFiles "CMake\bin"))
    if ($env:LOCALAPPDATA) {
        [void]$candidates.Add((Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Links"))
        $packages = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages"
        if (Test-Path -LiteralPath $packages) {
            Get-ChildItem -LiteralPath $packages -Directory -Filter "Google.Protobuf*" -ErrorAction SilentlyContinue |
                ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Directory -Recurse -Depth 1 -Filter "bin" -ErrorAction SilentlyContinue } |
                ForEach-Object { [void]$candidates.Add($_.FullName) }
        }
    }
    foreach ($dir in $candidates) {
        if ($dir -and (Test-Path -LiteralPath $dir) -and (";$env:PATH;" -notlike "*;$dir;*")) { $env:PATH = "$env:PATH;$dir" }
    }
}

function Install-Dep([string]$dep) {
    $wingetIds = @{ cargo = "Rustlang.Rustup"; protoc = "Google.Protobuf"; cmake = "Kitware.CMake" }
    $chocoIds = @{ cargo = "rustup.install"; protoc = "protoc"; cmake = "cmake" }
    if ($script:pkg -eq "winget") {
        return (Invoke-Run "winget" @("install", "--id", $wingetIds[$dep], "-e",
            "--accept-source-agreements", "--accept-package-agreements", "--silent"))
    }
    return (Invoke-Run "choco" @("install", "-y", $chocoIds[$dep]))
}

# The C++ workload ships a cmake.exe that never lands on PATH. The build step
# prepends it for its own run, and the bootstrap step treats it as cmake being
# present, so a machine with Build Tools installs nothing.
function Find-VsCmakeDir {
    $roots = @(${env:ProgramFiles(x86)}, $env:ProgramFiles) | Where-Object { $_ }
    foreach ($root in $roots) {
        $pattern = Join-Path $root "Microsoft Visual Studio\*\*\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
        $hit = @(Get-Item -Path $pattern -ErrorAction SilentlyContinue |
            Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "cmake.exe") } |
            Sort-Object FullName -Descending) | Select-Object -First 1
        if ($hit) { return $hit.FullName }
    }
    return $null
}

# OMP has no `mcp add` subcommand, so its user-level mcp.json is edited in
# place: the file is copied aside first, which is what the rollback restores.
function Register-OmpMcp {
    $file = Join-Path $userHome ".omp\agent\mcp.json"
    $json = $null
    $current = $null
    if (Test-Path -LiteralPath $file) {
        $raw = Get-Content -LiteralPath $file -Raw
        if ($raw -and $raw.Trim()) {
            try { $json = $raw | ConvertFrom-Json } catch { Fail "$file is not valid JSON; fix or move it, then re-run." }
        }
        if ($json -and ($json.PSObject.Properties.Name -contains "mcpServers") -and $json.mcpServers -and
            ($json.mcpServers.PSObject.Properties.Name -contains "konnect")) {
            $current = $json.mcpServers.konnect.command
        }
    }
    if ($current -and ($current.ToLowerInvariant() -eq $targetPath.ToLowerInvariant())) {
        Say "omp: konnect already points at $targetPath"
        return
    }
    if ($DryRun) { Say "would set mcpServers.konnect.command = $targetPath in $file"; return }
    $dir = Split-Path -Parent $file
    if (-not (Test-Path -LiteralPath $dir)) { [void](New-Item -ItemType Directory -Force -Path $dir) }
    if (Test-Path -LiteralPath $file) {
        $script:mcpOmpBackup = "$file.$(Get-Date -Format 'yyyyMMdd-HHmmss').bak"
        Copy-Item -LiteralPath $file -Destination $script:mcpOmpBackup
    }
    $script:mcpOmpFile = $file
    if (-not $json) { $json = [pscustomobject]@{} }
    if (-not ($json.PSObject.Properties.Name -contains "mcpServers") -or -not $json.mcpServers) {
        $json | Add-Member -NotePropertyName mcpServers -NotePropertyValue ([pscustomobject]@{}) -Force
    }
    $json.mcpServers | Add-Member -NotePropertyName konnect `
        -NotePropertyValue ([pscustomobject]@{ type = "stdio"; command = $targetPath }) -Force
    # No BOM: a byte-order mark in front of '{' breaks strict JSON readers.
    [System.IO.File]::WriteAllText($file, ($json | ConvertTo-Json -Depth 20), (New-Object System.Text.UTF8Encoding($false)))
    [void]$script:mcpAdded.Add("omp")
    [void]$script:changes.Add("registered the konnect MCP server for OMP in $file")
    Say "omp: mcpServers.konnect -> $targetPath"
}

# ---- 0. bootstrap ----------------------------------------------------------
# Opt-in. Without it the build step keeps its old behaviour: it reports what is
# missing and stops, which is what a developer with an environment wants.
Step "Bootstrap"
$pkg = $null
if ($WithRetrace -and $NoRetrace) { Fail "-WithRetrace and -NoRetrace contradict each other; pass one." }
if (-not $Bootstrap) {
    Say "skipped (pass -Bootstrap on a machine that has no Rust/protoc/cmake yet)"
} else {
    Update-ToolPath
    $missing = @()
    foreach ($dep in @("cargo", "protoc", "cmake")) {
        if (Test-Cmd $dep) { Say "${dep}: already on PATH"; continue }
        if ($dep -eq "protoc" -and $env:PROTOC) { Say "protoc: PROTOC is set, nothing to install"; continue }
        if ($dep -eq "cmake" -and (Find-VsCmakeDir)) { Say "cmake: the Visual Studio copy will be used"; continue }
        Say "${dep}: missing"
        $missing += $dep
    }
    if ($missing.Count -eq 0) {
        Say "nothing to install"
    } else {
        if (Test-Cmd "winget") { $pkg = "winget" } elseif (Test-Cmd "choco") { $pkg = "choco" }
        if (-not $pkg) { Fail "-Bootstrap needs winget or choco to install $($missing -join ', '). Install them yourself, then re-run without -Bootstrap." }
        Say "package manager: $pkg"
        foreach ($dep in $missing) {
            if (Install-Dep $dep) { [void]$changes.Add("installed $dep with $pkg") }
            else { Warn "could not install $dep with $pkg." }
            Update-ToolPath
            if (-not $DryRun -and -not (Test-Cmd $dep)) {
                Warn "$dep is installed but not visible in this window; open a new PowerShell and re-run if the build fails."
            }
        }
    }
    # winget's protobuf package is not shimmed into PATH, so a build from a
    # fresh window would not find protoc even though it is installed. A
    # user-level PROTOC is what prost-build reads, and it survives the window.
    $protocPath = (Get-Command protoc -ErrorAction SilentlyContinue).Source
    if (-not $protocPath -and $env:PROTOC) { $protocPath = $env:PROTOC }
    if ($protocPath) {
        $currentProtoc = [Environment]::GetEnvironmentVariable("PROTOC", "User")
        if ($currentProtoc -ne $protocPath) {
            $shownProtoc = "(unset)"
            if ($currentProtoc) { $shownProtoc = $currentProtoc }
            if ($DryRun) {
                Say "would set PROTOC (user): $shownProtoc -> $protocPath"
            } else {
                [Environment]::SetEnvironmentVariable("PROTOC", $protocPath, "User")
                $protocChanged = $true
                $protocPrevious = $currentProtoc
                [void]$changes.Add("set PROTOC (user): $shownProtoc -> $protocPath")
                Say "set PROTOC (user): $shownProtoc -> $protocPath"
            }
        } else {
            Say "PROTOC (user) already $protocPath - unchanged"
        }
    }
}

# ---- 1. build --------------------------------------------------------------
Step "Build"
# cargo puts the build under CARGO_TARGET_DIR when it is set (relative to the
# directory cargo runs in, which is the repo root here).
$cargoTargetDir = Join-Path $repoRoot "target"
if ($env:CARGO_TARGET_DIR) {
    $cargoTargetDir = $env:CARGO_TARGET_DIR
    if (-not [System.IO.Path]::IsPathRooted($cargoTargetDir)) { $cargoTargetDir = Join-Path $repoRoot $cargoTargetDir }
    $cargoTargetDir = [System.IO.Path]::GetFullPath($cargoTargetDir)
    Say "CARGO_TARGET_DIR = $cargoTargetDir"
}
$source = Join-Path $cargoTargetDir "release\konnect.exe"
$buildReason = $null
$staleReason = $null
# Build inputs: the workspace sources plus the files outside crates/ that the
# binary depends on (manifest.rs embeds docs/RELIABILITY_CONTRACT.md).
$buildInputs = @("crates", "Cargo.lock", "Cargo.toml", "rust-toolchain.toml", "docs/RELIABILITY_CONTRACT.md")
if (Test-Path -LiteralPath $source) {
    $builtAt = [System.IO.File]::GetLastWriteTimeUtc($source)
    $tracked = @()
    try { $tracked = @(& git -C $repoRoot ls-files -- @buildInputs 2>$null) } catch { $tracked = @() }
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
        else { [void]$problems.Add("protoc not found: set PROTOC to protoc.exe (e.g. `$env:PROTOC = 'C:\path\to\protoc\bin\protoc.exe') or put protoc on PATH - https://github.com/protocolbuffers/protobuf/releases. To keep PROTOC for every new window: [Environment]::SetEnvironmentVariable('PROTOC','<path to protoc.exe>','User')") }
    }

    $cmakeDir = $null
    if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
        $cmakeDir = Find-VsCmakeDir
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
        $projects = Get-JsonMember $config "projects"
        if ($top) {
            $found = Get-ConfigTargetPath $top "the top-level mcpServers.konnect.command in $ClaudeConfig"
            $foundWhere = "top-level mcpServers.konnect.command in $ClaudeConfig"
            # Inside a project, Claude Code runs that project's own konnect entry.
            $shadowed = New-Object System.Collections.ArrayList
            foreach ($name in (Get-JsonNames $projects)) {
                $cmd = Get-KonnectCommand (Get-JsonMember $projects $name)
                if (-not $cmd) { continue }
                $shown = $cmd
                $converted = ConvertFrom-PosixDrivePath $cmd
                if ((Test-KonnectExecutablePath $converted) -and -not (Test-DrivelessPath $converted)) {
                    $norm = Get-NormalizedPath $converted
                    if ($norm.ToLowerInvariant() -eq $found.ToLowerInvariant()) { continue }
                    $shown = $norm
                }
                [void]$shadowed.Add("project $name -> $shown")
            }
            if ($shadowed.Count -gt 0) {
                Warn ("Claude Code gives a project's own konnect entry precedence inside that project, so these projects keep running a different konnect than the top-level one ($found):`n  - " + ($shadowed -join "`n  - ") + "`nThis installer still installs to $found; update those project entries, or pass -Target to install elsewhere.")
            }
        } else {
            $distinct = [ordered]@{}
            $projectOf = @{}
            foreach ($name in (Get-JsonNames $projects)) {
                $cmd = Get-KonnectCommand (Get-JsonMember $projects $name)
                if (-not $cmd) { continue }
                $norm = Get-ConfigTargetPath $cmd "the mcpServers.konnect.command of project '$name' in $ClaudeConfig"
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
        # -Bootstrap registers the server itself further down, so the manual
        # command is only worth printing when this run will not do it.
        if ($Bootstrap) {
            Say "the MCP registration step below will register this path with Claude"
        } else {
            $targetNote = "Konnect is not registered with Claude Code. To register it (or re-run with -Bootstrap), run:`n    claude mcp add --scope user konnect -- `"$targetPath`""
            Say "NOTE: $targetNote"
        }
    }
}
if (Test-Path -LiteralPath $targetPath -PathType Container) {
    Fail "the install target $targetPath is a folder. Pass the full path to the .exe, e.g. -Target '$(Join-Path $targetPath 'konnect.exe')'."
}
if (-not [System.IO.Path]::GetFileName($targetPath)) {
    Fail "the install target $targetPath ends with a separator. Pass the full path to the .exe, e.g. -Target '$($targetPath.TrimEnd('\'))\konnect.exe'."
}

# ---- 3. backup + copy ------------------------------------------------------
Step "Install binary"
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
    $base = [System.IO.Path]::GetFileNameWithoutExtension($targetPath)
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $parkedPath = Join-Path $targetDir "$base-$newTag-$stamp-rolledback.exe.bak"
    if ($targetExisted) {
        $oldVersion = Get-KonnectVersion $targetPath
        Say "existing binary: version $oldVersion, sha256 $(Get-Sha256 $targetPath)"
        $backupName = Join-Path $targetDir "$base-$oldVersion-$stamp.exe.bak"
        $n = 2
        while (Test-Path -LiteralPath $backupName) {
            $backupName = Join-Path $targetDir "$base-$oldVersion-$stamp-$n.exe.bak"
            $n++
        }
        if ($DryRun) {
            Say "would rename $targetPath -> $backupName"
        } else {
            try { Move-Item -LiteralPath $targetPath -Destination $backupName }
            catch { Fail "could not rename $targetPath to $backupName ($($_.Exception.Message)); nothing was changed." }
            Say "renamed $targetPath -> $backupName"
        }
        $backupPath = $backupName
    }
    if ($DryRun) {
        Say "would copy $source -> $targetPath"
        if (-not $backupPath) { $freshInstall = $true }
    } else {
        # Never overwrite: whatever sits at the target now has no backup.
        if (Test-Path -LiteralPath $targetPath) {
            if ($backupPath) { Fail "$targetPath still exists after renaming it to $backupPath; stopping before the copy so nothing is overwritten." }
            Fail "$targetPath appeared while the installer was running; stopping before the copy so nothing is overwritten."
        }
        try {
            if (-not (Test-Path -LiteralPath $targetDir)) { New-Item -ItemType Directory -Path $targetDir -Force | Out-Null }
            Copy-Item -LiteralPath $source -Destination $targetPath
        } catch {
            $why = $_.Exception.Message
            if ($backupPath) {
                $restoreError = $null
                try { Move-Item -LiteralPath $backupPath -Destination $targetPath } catch { $restoreError = $_.Exception.Message }
                if (-not $restoreError) {
                    $backupPath = $null
                    Fail "copy to $targetPath failed ($why); the previous binary was renamed back - nothing changed."
                }
                Fail "copy to $targetPath failed ($why), and putting the previous binary back failed too ($restoreError). The previous binary is safe at $backupPath; the lines below restore it."
            }
            if (Test-Path -LiteralPath $targetPath) { $freshInstall = $true }
            Fail "copy to $targetPath failed ($why)."
        }
        if (-not $backupPath) { $freshInstall = $true }
        if ((Get-Sha256 $targetPath) -ne $srcHash) { Fail "$targetPath does not match the build after copying." }
        Say "copied $source -> $targetPath"
    }
    if ($backupPath) {
        [void]$changes.Add("$verb $targetPath (old binary $oldVersion renamed to $backupPath)")
    } else {
        [void]$changes.Add("$verb $targetPath (fresh install: there was no konnect there before)")
    }
}

# ---- 4. init + status ------------------------------------------------------
Step "Guidance (konnect init)"
if ($SkipInit) {
    Say "skipped (-SkipInit)"
} else {
    foreach ($c in $clients) {
        $initArgs = @("init")
        if ($c -ne "claude") { $initArgs = @("init", "--client", $c) }
        [void]$initClients.Add($c)
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
        # claude and omp both report skills and agents (omp installs no hooks,
        # so its line has no hooks group); codex installs skills only and gets
        # no count check.
        $countPattern = $null
        if ($c -eq "claude") { $countPattern = '^Done: (\d+) skills, (\d+) agents, (\d+) hooks installed for Claude\.' }
        elseif ($c -eq "omp") { $countPattern = '^Done: (\d+) skills, (\d+) agents installed for OMP\.' }
        if ($countPattern) {
            $done = $initOut | Where-Object { $_ -match $countPattern } | Select-Object -Last 1
            if (-not $done) {
                Warn "konnect init printed no 'Done: ... installed' line for $c; counts not checked."
            } else {
                [void]($done -match $countPattern)
                $gotSkills = [int]$Matches[1]
                $gotAgents = [int]$Matches[2]
                if ($null -eq $expectedSkills) {
                    Warn "no manifest at $manifestPath; counts not checked."
                } elseif ($gotSkills -ne $expectedSkills -or $gotAgents -ne $expectedAgents) {
                    Warn "$c init installed $gotSkills skills / $gotAgents agents but manifest.rs lists $expectedSkills / $expectedAgents - is the installed build current?"
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
    if ($WithRetrace -and -not $RetracePython) {
        $venv = Join-Path $repoRoot ".venv-retrace"
        $venvPython = Join-Path $venv "Scripts\python.exe"
        if (Test-Path -LiteralPath $venvPython) {
            Say "venv: $venv already exists"
        } else {
            $basePython = $null
            foreach ($cand in @("python", "python3", "py")) { if (Test-Cmd $cand) { $basePython = $cand; break } }
            if (-not $basePython) { Fail "-WithRetrace needs Python 3.10+ on PATH." }
            if (-not (Invoke-Run $basePython @("-m", "venv", $venv))) { Fail "$basePython -m venv $venv failed." }
            if (-not $DryRun) { [void]$changes.Add("created $venv") }
        }
        if ($DryRun) {
            Say "would run: $venvPython -m pip install git+https://github.com/ericrihm/retrace.git"
        } else {
            $installed = Invoke-Probe $venvPython "-c `"import retrace`""
            if ($installed -and $installed.Code -eq 0) {
                Say "retrace is already importable from the venv"
            } else {
                [void](Invoke-Run $venvPython @("-m", "pip", "install", "--quiet", "--upgrade", "pip"))
                if (Invoke-Run $venvPython @("-m", "pip", "install", "--quiet", "git+https://github.com/ericrihm/retrace.git")) {
                    [void]$changes.Add("installed retrace into $venv")
                } else {
                    Warn "pip install retrace failed; see docs/PHOTO_TO_BOARD_WORKFLOW.md. Photo intake stays unconfigured."
                }
            }
        }
    }
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
            $retraceChanged = $true
            $retracePrevious = $current
            [void]$changes.Add("$verb RETRACE_PYTHON (user): $shown -> $py")
        }
    }
}

# ---- 6. MCP registration ---------------------------------------------------
# What makes a fresh clone actually usable: a client that does not know about
# the binary never starts it. Only -Bootstrap writes here; the default run
# still touches no client config.
Step "MCP server registration"
if (-not $Bootstrap) {
    Say "skipped (pass -Bootstrap to register konnect with the selected clients)"
} else {
    foreach ($c in $clients) {
        if ($c -eq "omp") { Register-OmpMcp; continue }
        if (-not (Test-Cmd $c)) {
            Warn "$c is not on PATH; register by hand: $c mcp add konnect -- `"$targetPath`""
            continue
        }
        # The name is listed by both CLIs; matching a whole word keeps
        # 'konnect-dev' from counting as an existing 'konnect'.
        $listed = Invoke-Probe $c "mcp list"
        if ($listed -and $listed.Code -eq 0 -and $listed.Out -match '(^|[^A-Za-z0-9_-])konnect([^A-Za-z0-9_-]|$)') {
            Say "${c}: konnect is already registered"
            continue
        }
        if ($c -eq "claude") { $added = Invoke-Run "claude" @("mcp", "add", "konnect", "-s", "user", "--", $targetPath) }
        else { $added = Invoke-Run "codex" @("mcp", "add", "konnect", "--", $targetPath, "--client", "codex") }
        if (-not $added) { Warn "$c mcp add konnect failed."; continue }
        if ($DryRun) { continue }
        [void]$mcpAdded.Add($c)
        [void]$changes.Add("registered the konnect MCP server with $c")
    }
    Say "restart the client so it picks the server up."
}

# ---- summary ---------------------------------------------------------------
Step "Summary"
if ($DryRun) { Say "DRY RUN (preview) - nothing was changed; this is what a real run would do." }
if ($changes.Count -eq 0) { Say "nothing changed." }
foreach ($line in $changes) { Say "- $line" }
Say "target:  $targetPath (new build version $sourceVersion)"
if ($backupPath -and $DryRun) { Say "backup (would be):  $backupPath" }
elseif ($backupPath) { Say "backup:  $backupPath" }
elseif ($freshInstall) { Say "backup:  (none - fresh install: there was no konnect at $targetPath before)" }
else { Say "backup:  (none)" }
if ($targetNote) { Say "NOTE: $targetNote" }
if ($retraceNote) { Say "NOTE: $retraceNote" }
$rollbackLines = @(Get-RollbackLines)
if ($DryRun) {
    Say "undo: preview only - nothing to undo now. After a real run, the summary will show how to undo it."
} elseif ($rollbackLines.Count -gt 0) {
    if ($freshInstall) { Say "undo (PowerShell; safe to paste twice) - this was a fresh install, so undoing it moves the new file aside:" }
    else { Say "rollback (PowerShell; safe to paste twice):" }
    foreach ($line in $rollbackLines) { Say "    $line" }
} else {
    Say "rollback: nothing to roll back."
}
if ($initClients -contains "omp") {
    Say "OMP guidance: $(Join-Path $userHome '.omp\agent\skills') and $(Join-Path $userHome '.omp\agent\agents') - start a new omp session to pick it up."
}
if ($retraceChanged) {
    Say "RETRACE_PYTHON only reaches programs started after it is set: close Claude Code completely"
    Say "(every window, and any terminal running it) and start it again from a new terminal or the Start menu."
} else {
    Say "Restart Claude Code (all windows) so the MCP server and the skills reload."
}
exit 0
