#!/usr/bin/env bash
# Build Konnect from this checkout and install it where Claude Code runs it
# (macOS/Linux and Git Bash/MSYS/Cygwin port of install.ps1).
#
# Local-build installer for developers and testers. The end-user path stays
# the KiCAD PCM zip built by packaging/build-pcm.sh. Steps:
#
#   0. bootstrap --bootstrap only: install cargo, protoc and cmake when they are
#                missing (winget/choco on Windows, brew on macOS,
#                apt-get/dnf/pacman/zypper on Linux), and register the konnect
#                MCP server for each selected client. --with-retrace also
#                creates <repo>/.venv-retrace and pip installs retrace into it
#   1. build     cargo build --release -p konnect, when target/release/konnect[.exe]
#                ($CARGO_TARGET_DIR/release when set) is missing, older than the
#                newest git-tracked build input (crates/, Cargo.lock, Cargo.toml,
#                rust-toolchain.toml, docs/RELIABILITY_CONTRACT.md), or --rebuild
#                is given
#   2. target    --target, else mcpServers.konnect.command in the Claude config
#                (top level first, then a single distinct project-level entry),
#                else ~/.konnect/bin/konnect[.exe]; JSON read with jq, else
#                python3/python
#   3. install   identical binary -> skip; otherwise RENAME the old binary to
#                <name>-<version>-<yyyyMMdd-HHmmss>[.exe].bak (never deleted),
#                then copy the new build into place
#   4. init      <target> init (Claude) and/or <target> init --client <codex|omp>,
#                then <target> status --client <c>
#   5. retrace   point RETRACE_PYTHON at a Python that can `import retrace`
#                (default: <repo>/.venv-retrace): setx on Windows shells; on
#                macOS/Linux the export line is printed, rc files are not edited
#
# Without --bootstrap it never registers or edits an MCP server entry. It never
# touches KiCAD projects and never deletes a binary.
#
# Usage:
#   scripts/install.sh [--client claude|codex|omp|both|all] [--target PATH]
#                      [--claude-config PATH] [--skip-build | --rebuild]
#                      [--skip-init] [--retrace-python PATH | --no-retrace]
#                      [--bootstrap] [--with-retrace] [--dry-run] [--help]
#
# --dry-run prints every action and changes nothing.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

client="claude,omp"
target=""
claude_config=""
skip_build=0
rebuild=0
skip_init=0
retrace_python=""
no_retrace=0
dry_run=0
bootstrap=0
with_retrace=0

usage() {
    cat <<'EOF'
Usage: scripts/install.sh [--client claude|codex|omp|both|all] [--target PATH]
                          [--claude-config PATH] [--skip-build | --rebuild]
                          [--skip-init] [--retrace-python PATH | --no-retrace]
                          [--bootstrap] [--with-retrace] [--dry-run] [--help]

Builds Konnect from this checkout, installs it where Claude Code runs it,
installs the bundled guidance (konnect init) and points RETRACE_PYTHON at the
photo-intake Python.

  --client          guidance to install (default: claude,omp): claude, codex,
                    omp (guidance for the OMP CLI under ~/.omp/agent/),
                    both (= claude codex) or all (= claude codex omp); a comma-
                    or space-separated list also works, e.g. --client claude,omp
  --target          install path of the konnect binary (default: the command
                    registered in the Claude config, else ~/.konnect/bin/konnect)
  --claude-config   Claude config to read the target from (default ~/.claude.json)
  --skip-build      use the existing target/release/konnect
  --rebuild         build even when the existing build is current
  --skip-init       do not run konnect init / status
  --retrace-python  Python to use for RETRACE_PYTHON (default <repo>/.venv-retrace)
  --no-retrace      leave RETRACE_PYTHON alone
  --bootstrap       fresh machine: install cargo, protoc and cmake when they are
                    missing, then register the konnect MCP server for each
                    selected client
  --with-retrace    create <repo>/.venv-retrace and pip install the optional
                    retrace package into it
  --dry-run         print every action, change nothing
EOF
}

# ---- state the rollback is computed from (set only once a change happened)
changes=""
target_u=""
backup=""          # set only after the old binary was really renamed (or would be)
parked=""          # where the rollback moves the new binary aside
fresh_install=0    # a new binary was (or would be) placed where none existed
init_clients=""    # clients whose init ran (or would run)
retrace_changed=0
retrace_previous=""
protoc_changed=0
protoc_previous=""
mcp_added=""       # clients whose MCP entry this run created
mcp_omp_file=""    # the OMP mcp.json that was written, when one was
mcp_omp_backup=""  # its pre-run copy, when the file already existed
bootstrap_installed=""
recovery_shown=0
failing=0

fail() { failing=1; echo "install: ERROR: $*" >&2; exit 1; }
warn() { echo "install: WARNING: $*" >&2; }
step() { printf '\n== %s\n' "$1"; }
say() { printf '  %s\n' "$*"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --client)         client="${2:?--client needs a value}"; shift 2;;
        --target)         target="${2:?--target needs a value}"; shift 2;;
        --claude-config)  claude_config="${2:?--claude-config needs a value}"; shift 2;;
        --skip-build)     skip_build=1; shift;;
        --rebuild)        rebuild=1; shift;;
        --skip-init)      skip_init=1; shift;;
        --retrace-python) retrace_python="${2:?--retrace-python needs a value}"; shift 2;;
        --no-retrace)     no_retrace=1; shift;;
        --dry-run)        dry_run=1; shift;;
        --bootstrap)      bootstrap=1; shift;;
        --with-retrace)   with_retrace=1; shift;;
        -h|--help)        usage; exit 0;;
        *) echo "Unknown argument: $1" >&2; usage >&2; exit 2;;
    esac
done

# ---- --client: one or more of claude, codex and omp, comma- and/or space-
# separated. "both" keeps its legacy meaning (claude codex), "all" adds omp.
# The result is a deduplicated list in a stable order.
# Fills $clients; fail() must run in this shell, so no command substitution.
normalize_clients() {
    local want_claude=0 want_codex=0 want_omp=0 tok
    clients=""
    for tok in $(printf '%s' "$1" | tr ',' ' '); do
        case "$tok" in
            claude) want_claude=1;;
            codex)  want_codex=1;;
            omp)    want_omp=1;;
            both)   want_claude=1; want_codex=1;;
            all)    want_claude=1; want_codex=1; want_omp=1;;
            *) fail "--client must be claude, codex, omp, both or all (got '$tok').";;
        esac
    done
    [ "$want_claude" = 1 ] && clients="$clients claude"
    [ "$want_codex" = 1 ] && clients="$clients codex"
    [ "$want_omp" = 1 ] && clients="$clients omp"
    clients="${clients# }"
}
normalize_clients "$client"
[ -n "$clients" ] || fail "--client needs at least one of claude, codex, omp, both or all (got '$client')."
[ "$skip_build" = 1 ] && [ "$rebuild" = 1 ] && fail "--skip-build and --rebuild contradict each other; pass one."
[ "$no_retrace" = 1 ] && [ -n "$retrace_python" ] && fail "--retrace-python and --no-retrace contradict each other; pass one."
[ "$with_retrace" = 1 ] && [ "$no_retrace" = 1 ] && fail "--with-retrace and --no-retrace contradict each other; pass one."

windows=0
exe=""
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) windows=1; exe=".exe";;
esac
verb="Changed"
[ "$dry_run" = 1 ] && verb="Would change"

# ---- path helpers: work on POSIX paths, show native ones -------------------
to_unix() {
    if [ "$windows" = 1 ]; then cygpath -u -- "$1"; else printf '%s\n' "$1"; fi
}
to_native() {
    if [ "$windows" = 1 ]; then cygpath -w -- "$1"; else printf '%s\n' "$1"; fi
}
lower() { printf '%s' "$1" | tr 'A-Z' 'a-z'; }
# Absolute POSIX path for a user-supplied path (~ and relative paths allowed).
abs_path() {
    local p="$1"
    case "$p" in "~") p="$HOME";; "~/"*) p="$HOME/${p#\~/}";; esac
    p="$(to_unix "$p")"
    case "$p" in /*) ;; *) p="$PWD/$p";; esac
    printf '%s\n' "$p"
}
# Key used to decide whether two paths are the same file location.
path_key() {
    if [ "$windows" = 1 ]; then lower "$(cygpath -m -- "$1")"; else printf '%s' "$1"; fi
}
is_konnect_path() {
    local cmd="$1" leaf
    case "$cmd" in *'"'*) return 1;; esac
    if [ "$windows" = 1 ]; then
        case "$cmd" in [A-Za-z]:[\\/]*|\\\\*|/*) ;; *) return 1;; esac
    else
        case "$cmd" in /*) ;; *) return 1;; esac
    fi
    leaf="${cmd##*[\\/]}"
    if [ "$windows" = 1 ]; then leaf="$(lower "$leaf")"; fi
    case "$leaf" in konnect|konnect.exe) return 0;; *) return 1;; esac
}

have_timeout=0
if command -v timeout >/dev/null 2>&1 && timeout 5 true >/dev/null 2>&1; then have_timeout=1; fi
# Read-only probe: stdin closed and a timeout, so an old binary that does not
# know a flag cannot hang the installer waiting on stdin.
probe() {
    if [ "$have_timeout" = 1 ]; then timeout 15 "$@" </dev/null; else "$@" </dev/null; fi
}
konnect_version() {
    local out
    out="$(probe "$1" --version 2>/dev/null)" || out=""
    out="$(printf '%s\n' "$out" | tr -d '\r' | sed -n 's/^konnect[[:space:]]\{1,\}\([^[:space:]]\{1,\}\).*/\1/p' | head -1)"
    printf '%s\n' "${out:-unknown}"
}
file_hash() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum -- "$1" | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 -- "$1" | cut -d' ' -f1
    else echo "(no sha256 tool)"
    fi
}
same_file_content() {
    if command -v cmp >/dev/null 2>&1; then cmp -s -- "$1" "$2"; else [ "$(file_hash "$1")" = "$(file_hash "$2")" ]; fi
}
target_exists() { [ -e "$target_u" ] || [ -L "$target_u" ]; }

# A shell single-quoted word; a ' in the text becomes '\''.
sh_quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }

# Commands that undo what this run changed, in paste order. The file moves are
# guarded so pasting them twice never overwrites anything.
rollback_lines() {
    local t c
    t="$(sh_quote "$target_u")"
    if [ -n "$backup" ]; then
        if [ "$dry_run" = 1 ] || target_exists; then
            printf '%s\n' "[ ! -e $(sh_quote "$parked") ] && [ -e $(sh_quote "$backup") ] && mv -- $t $(sh_quote "$parked") && mv -- $(sh_quote "$backup") $t || echo 'rollback stopped: already done, or the files were moved - check the folder'"
        else
            printf '%s\n' "[ ! -e $t ] && [ -e $(sh_quote "$backup") ] && mv -- $(sh_quote "$backup") $t || echo 'rollback stopped: already done, or the files were moved - check the folder'"
        fi
        for c in $init_clients; do
            if [ "$c" = claude ]; then printf '%s\n' "$t init"; else printf '%s\n' "$t init --client $c"; fi
        done
    elif [ "$fresh_install" = 1 ]; then
        for c in $init_clients; do
            if [ "$c" = claude ]; then printf '%s\n' "$t uninstall"; else printf '%s\n' "$t uninstall --client $c"; fi
        done
        printf '%s\n' "[ ! -e $(sh_quote "$parked") ] && [ -e $t ] && mv -- $t $(sh_quote "$parked") || echo 'undo stopped: already done, or the file was moved - check the folder'"
    fi
    if [ "$retrace_changed" = 1 ]; then
        if [ -n "$retrace_previous" ]; then printf '%s\n' "setx RETRACE_PYTHON $(sh_quote "$retrace_previous")"
        else printf '%s\n' "powershell -NoProfile -Command '[Environment]::SetEnvironmentVariable(\"RETRACE_PYTHON\", \$null, \"User\")'"
        fi
    fi
    if [ "$protoc_changed" = 1 ]; then
        if [ -n "$protoc_previous" ]; then printf '%s\n' "setx PROTOC $(sh_quote "$protoc_previous")"
        else printf '%s\n' "powershell -NoProfile -Command '[Environment]::SetEnvironmentVariable(\"PROTOC\", \$null, \"User\")'"
        fi
    fi
    for c in $mcp_added; do
        case "$c" in
            claude) printf '%s\n' "claude mcp remove konnect -s user";;
            codex)  printf '%s\n' "codex mcp remove konnect";;
            omp)
                if [ -n "$mcp_omp_backup" ]; then
                    printf '%s\n' "mv -f -- $(sh_quote "$mcp_omp_backup") $(sh_quote "$mcp_omp_file")"
                else
                    printf '%s\n' "rm -f -- $(sh_quote "$mcp_omp_file")"
                fi;;
        esac
    done
}

# On any non-zero exit after a change (fail, or an unexpected error under
# set -e): say what already changed and how to undo it.
on_exit() {
    local rc=$? lines
    [ "$rc" != 0 ] && [ "$recovery_shown" = 0 ] || return 0
    recovery_shown=1
    [ "$failing" = 1 ] || echo "install: ERROR: stopped unexpectedly (exit $rc); see the message above." >&2
    lines="$(rollback_lines)"
    [ -n "$lines" ] || return 0
    printf '\n== Stopped - changes made before the error\n'
    while IFS= read -r line; do [ -z "$line" ] || say "- $line"; done <<<"$changes"
    [ -z "$backup" ] || say "previous binary: $(to_native "$backup")"
    say "to undo, paste these lines into this shell (safe to paste twice):"
    while IFS= read -r line; do say "    $line"; done <<<"$lines"
}
trap on_exit EXIT

json_reader=""
if command -v jq >/dev/null 2>&1; then
    json_reader="jq"
else
    for candidate in python3 python; do
        if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c 'import json' >/dev/null 2>&1; then
            json_reader="$candidate"; break
        fi
    done
fi
# Exit 0 when the file is valid JSON (an empty file counts as an empty config).
config_is_valid_json() {
    if [ "$json_reader" = jq ]; then
        jq empty <"$1" >/dev/null 2>&1
    else
        "$json_reader" -c '
import json, sys
text = sys.stdin.buffer.read().decode("utf-8")
if text.strip():
    json.loads(text)
' <"$1" >/dev/null 2>&1
    fi
}

# Prints "top<TAB>command" and "project<TAB>name<TAB>command" lines for every
# mcpServers.konnect.command in the config (validated JSON, $json_reader set).
read_config_commands() {
    local config="$1"
    if [ "$json_reader" = jq ]; then
        jq -r '
          def cmd: objects | .mcpServers | objects | .konnect | objects | .command | strings
                   | sub("^\\s+"; "") | sub("\\s+$"; "") | select(length > 0);
          objects
          | ( (cmd | "top\t" + .),
              (.projects | objects | to_entries[] | . as $e | $e.value | cmd
               | "project\t" + $e.key + "\t" + .) )
        ' <"$config" | tr -d '\r'
        return "${PIPESTATUS[0]}"
    fi
    "$json_reader" -c '
import json, sys
text = sys.stdin.buffer.read().decode("utf-8")
data = json.loads(text) if text.strip() else None
def cmd(node):
    servers = node.get("mcpServers") if isinstance(node, dict) else None
    konnect = servers.get("konnect") if isinstance(servers, dict) else None
    value = konnect.get("command") if isinstance(konnect, dict) else None
    return value.strip() if isinstance(value, str) and value.strip() else None
lines = []
if isinstance(data, dict):
    top = cmd(data)
    if top:
        lines.append("top\t" + top)
    projects = data.get("projects")
    if isinstance(projects, dict):
        for name, node in projects.items():
            found = cmd(node)
            if found:
                lines.append("project\t" + name + "\t" + found)
sys.stdout.buffer.write("".join(line + "\n" for line in lines).encode("utf-8"))
' <"$config" | tr -d '\r'
    return "${PIPESTATUS[0]}"
}

add_change() { changes="${changes}$1"$'\n'; }

# ---- bootstrap helpers -----------------------------------------------------
have() { command -v "$1" >/dev/null 2>&1; }

# Echo then run, or only echo under --dry-run. The caller decides what a
# non-zero exit means, so this never calls fail() itself.
run_cmd() {
    if [ "$dry_run" = 1 ]; then say "would run: $*"; return 0; fi
    say "running: $*"
    "$@"
}

add_to_path() {
    [ -d "$1" ] || return 0
    case ":$PATH:" in
        *":$1:"*) ;;
        *) PATH="$PATH:$1"; export PATH;;
    esac
}

# A package manager can put a tool where this already-running shell will not
# look: rustup writes ~/.cargo/bin, winget shims live under WinGet/Links, and
# the CMake and protobuf packages install outside both. Re-probe those places
# so the build in this same run can still find what was just installed.
refresh_tool_path() {
    local la d
    add_to_path "$HOME/.cargo/bin"
    [ "$windows" = 1 ] || { hash -r 2>/dev/null || true; return 0; }
    la="$(printenv LOCALAPPDATA 2>/dev/null || true)"
    if [ -n "$la" ]; then
        la="$(cygpath -u -- "$la")"
        add_to_path "$la/Microsoft/WinGet/Links"
        for d in "$la"/Microsoft/WinGet/Packages/Google.Protobuf*/bin \
                 "$la"/Microsoft/WinGet/Packages/Google.Protobuf*/*/bin; do
            add_to_path "$d"
        done
    fi
    add_to_path "/c/Program Files/CMake/bin"
    hash -r 2>/dev/null || true
}

detect_pkg() {
    pkg=""
    if [ "$windows" = 1 ]; then
        if have winget; then pkg=winget; elif have choco; then pkg=choco; fi
        return 0
    fi
    if [ "$(uname -s)" = Darwin ]; then
        have brew && pkg=brew
        return 0
    fi
    local m
    for m in apt-get dnf pacman zypper; do
        if have "$m"; then pkg="$m"; return 0; fi
    done
    return 0
}

# rustup is the supported way in on every platform the package managers do not
# carry a current toolchain; it also owns rust-toolchain.toml's pinned version.
install_rustup() {
    have curl || { warn "curl is needed to install Rust; see https://rustup.rs"; return 1; }
    if [ "$dry_run" = 1 ]; then
        say "would run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
        return 0
    fi
    say "running: the rustup installer from https://sh.rustup.rs"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
}

# $1 = cargo | protoc | cmake. Non-zero means nothing was installed.
install_dep() {
    local dep="$1"
    case "$pkg:$dep" in
        winget:cargo)   run_cmd winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements --silent;;
        winget:protoc)  run_cmd winget install --id Google.Protobuf -e --accept-source-agreements --accept-package-agreements --silent;;
        winget:cmake)   run_cmd winget install --id Kitware.CMake -e --accept-source-agreements --accept-package-agreements --silent;;
        choco:cargo)    run_cmd choco install -y rustup.install;;
        choco:protoc)   run_cmd choco install -y protoc;;
        choco:cmake)    run_cmd choco install -y cmake;;
        brew:cargo)     install_rustup;;
        brew:protoc)    run_cmd brew install protobuf;;
        brew:cmake)     run_cmd brew install cmake;;
        apt-get:cargo|dnf:cargo|pacman:cargo|zypper:cargo) install_rustup;;
        apt-get:protoc) run_cmd $sudo apt-get install -y protobuf-compiler;;
        apt-get:cmake)  run_cmd $sudo apt-get install -y cmake;;
        dnf:protoc)     run_cmd $sudo dnf install -y protobuf-compiler;;
        dnf:cmake)      run_cmd $sudo dnf install -y cmake;;
        pacman:protoc)  run_cmd $sudo pacman -S --needed --noconfirm protobuf;;
        pacman:cmake)   run_cmd $sudo pacman -S --needed --noconfirm cmake;;
        zypper:protoc)  run_cmd $sudo zypper --non-interactive install protobuf-devel;;
        zypper:cmake)   run_cmd $sudo zypper --non-interactive install cmake;;
        *) warn "no $pkg recipe for $dep; install it yourself."; return 1;;
    esac
}

# ---------------------------------------------------------------------------
echo "Konnect local installer ($repo_root)"
[ "$dry_run" = 1 ] && echo "DRY RUN - nothing will be changed."

[ -n "$claude_config" ] || claude_config="$HOME/.claude.json"
claude_config="$(abs_path "$claude_config")"

manifest="$repo_root/crates/konnect/src/manifest.rs"
expected_skills=""
expected_agents=""
if [ -f "$manifest" ]; then
    expected_skills="$(grep -cE '^[[:space:]]+SkillManifest \{' "$manifest" || true)"
    expected_agents="$(grep -cE '^[[:space:]]+AgentManifest \{' "$manifest" || true)"
fi

# The Visual Studio C++ workload ships a cmake.exe that never lands on PATH.
# The build step prepends it for its own run, and the bootstrap step treats it
# as cmake being present, so a machine with Build Tools installs nothing.
vs_cmake_dir() {
    [ "$windows" = 1 ] || return 0
    local root_var root d found=""
    for root_var in 'ProgramFiles(x86)' 'ProgramFiles'; do
        root="$(printenv "$root_var" 2>/dev/null || true)"
        [ -n "$root" ] || continue
        root="$(cygpath -u -- "$root")"
        for d in "$root"/"Microsoft Visual Studio"/*/*/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin; do
            if [ -f "$d/cmake.exe" ]; then found="$d"; fi
        done
        [ -z "$found" ] || break
    done
    [ -z "$found" ] || printf '%s\n' "$found"
}

# ---- 0. bootstrap ----------------------------------------------------------
# Opt-in. Without it the build step keeps its old behaviour: it reports what is
# missing and stops, which is what a developer with an environment wants.
step "Bootstrap"
pkg=""
sudo=""
if [ "$bootstrap" = 0 ]; then
    say "skipped (pass --bootstrap on a machine that has no Rust/protoc/cmake yet)"
else
    refresh_tool_path
    missing=""
    for dep in cargo protoc cmake; do
        if have "$dep"; then say "$dep: already on PATH"; continue; fi
        if [ "$dep" = protoc ] && [ -n "${PROTOC:-}" ]; then say "protoc: PROTOC is set, nothing to install"; continue; fi
        if [ "$dep" = cmake ] && [ -n "$(vs_cmake_dir)" ]; then say "cmake: the Visual Studio copy will be used"; continue; fi
        say "$dep: missing"
        missing="$missing $dep"
    done
    if [ -z "$missing" ]; then
        say "nothing to install"
    else
        detect_pkg
        [ -n "$pkg" ] || fail "--bootstrap needs a package manager for$missing (winget or choco on Windows, brew on macOS, apt-get/dnf/pacman/zypper on Linux). Install them yourself, then re-run without --bootstrap."
        say "package manager: $pkg"
        if [ "$windows" = 0 ] && [ "$(id -u 2>/dev/null || echo 1)" != 0 ] && have sudo; then sudo="sudo"; fi
        apt_updated=0
        for dep in $missing; do
            if [ "$pkg" = apt-get ] && [ "$apt_updated" = 0 ]; then
                run_cmd $sudo apt-get update || warn "apt-get update failed; continuing."
                apt_updated=1
            fi
            if install_dep "$dep"; then
                bootstrap_installed="$bootstrap_installed $dep"
                add_change "installed $dep with $pkg"
            else
                warn "could not install $dep with $pkg."
            fi
            refresh_tool_path
            if [ "$dry_run" = 0 ] && ! have "$dep"; then
                warn "$dep is installed but not visible in this shell; open a new terminal and re-run if the build fails."
            fi
        done
    fi
    # winget's protobuf package is not shimmed into PATH, so a build from a
    # fresh terminal would not find protoc even though it is installed. A
    # user-level PROTOC is what prost-build reads, and it survives the shell.
    # Only Windows needs it: apt, dnf, pacman, zypper and brew all install
    # protoc onto PATH.
    if [ "$windows" = 1 ]; then
        protoc_found="${PROTOC:-}"
        [ -n "$protoc_found" ] || protoc_found="$(command -v protoc 2>/dev/null || true)"
        if [ -n "$protoc_found" ]; then
            protoc_native="$(to_native "$(to_unix "$protoc_found")")"
            current_protoc="$(MSYS2_ARG_CONV_EXCL='*' reg.exe query 'HKCU\Environment' /v PROTOC 2>/dev/null | tr -d '\r' | sed -n 's/^[[:space:]]*PROTOC[[:space:]]\{1,\}REG_[A-Z_]*[[:space:]]\{1,\}//p' | head -1)" || current_protoc=""
            if [ -n "$current_protoc" ] && [ "$(path_key "$(to_unix "$current_protoc")")" = "$(path_key "$(to_unix "$protoc_native")")" ]; then
                say "PROTOC (user) already $current_protoc - unchanged"
            elif [ "$dry_run" = 1 ]; then
                say "would run: setx PROTOC \"$protoc_native\"   (${current_protoc:-(unset)} -> $protoc_native)"
            else
                setx PROTOC "$protoc_native" >/dev/null || fail "setx PROTOC failed."
                protoc_changed=1
                protoc_previous="$current_protoc"
                add_change "set PROTOC (user): ${current_protoc:-(unset)} -> $protoc_native"
                say "set PROTOC (user): ${current_protoc:-(unset)} -> $protoc_native"
            fi
        fi
    fi
fi

# ---- 1. build --------------------------------------------------------------
step "Build"
# cargo puts the build under CARGO_TARGET_DIR when it is set (relative to the
# directory cargo runs in, which is the repo root here).
cargo_target_dir="$repo_root/target"
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    cargo_target_dir="$(to_unix "$CARGO_TARGET_DIR")"
    case "$cargo_target_dir" in /*) ;; *) cargo_target_dir="$repo_root/$cargo_target_dir";; esac
    say "CARGO_TARGET_DIR = $(to_native "$cargo_target_dir")"
fi
source_bin="$cargo_target_dir/release/konnect$exe"
build_reason=""
stale_reason=""
# Build inputs: the workspace sources plus the files outside crates/ that the
# binary depends on (manifest.rs embeds docs/RELIABILITY_CONTRACT.md).
if [ -f "$source_bin" ]; then
    newest=""
    while IFS= read -r -d '' rel; do
        f="$repo_root/$rel"
        [ -f "$f" ] || continue
        if [ -z "$newest" ] || [ "$f" -nt "$newest" ]; then newest="$f"; fi
    done < <(git -C "$repo_root" ls-files -z -- crates Cargo.lock Cargo.toml rust-toolchain.toml docs/RELIABILITY_CONTRACT.md 2>/dev/null || true)
    if [ -z "$newest" ]; then
        warn "could not list git-tracked sources; treating the existing build as current."
    elif [ "$newest" -nt "$source_bin" ]; then
        stale_reason="build is older than ${newest#"$repo_root"/}"
    fi
fi
if [ "$rebuild" = 1 ]; then build_reason="--rebuild given"
elif [ ! -f "$source_bin" ]; then build_reason="no build at $source_bin"
elif [ -n "$stale_reason" ]; then build_reason="$stale_reason"
fi

if [ "$skip_build" = 1 ]; then
    [ -f "$source_bin" ] || fail "--skip-build given but there is no build at $source_bin."
    [ -z "$stale_reason" ] || warn "--skip-build: installing a stale build - $stale_reason."
    say "skipped (--skip-build); using $source_bin"
elif [ -z "$build_reason" ]; then
    say "up to date: $source_bin"
else
    say "build needed: $build_reason"
    problems=""
    command -v cargo >/dev/null 2>&1 || problems="${problems}cargo not found on PATH; install Rust from https://rustup.rs"$'\n'

    protoc=""
    if [ -n "${PROTOC:-}" ]; then
        if [ -f "$(to_unix "$PROTOC")" ]; then protoc="$(to_unix "$PROTOC")"
        else problems="${problems}PROTOC is set to '$PROTOC' but that file does not exist"$'\n'
        fi
    elif command -v protoc >/dev/null 2>&1; then
        protoc="$(command -v protoc)"
    else
        keep="add the export line to your shell profile"
        [ "$windows" = 0 ] || keep="run in PowerShell: [Environment]::SetEnvironmentVariable('PROTOC','<path to protoc.exe>','User')"
        problems="${problems}protoc not found: export PROTOC=/path/to/protoc or put protoc on PATH (macOS: brew install protobuf; Linux: apt install protobuf-compiler; Windows: https://github.com/protocolbuffers/protobuf/releases). To keep PROTOC for every new terminal, $keep"$'\n'
    fi

    cmake_dir=""
    if ! command -v cmake >/dev/null 2>&1; then
        cmake_dir="$(vs_cmake_dir)"
        [ -n "$cmake_dir" ] || problems="${problems}cmake not found on PATH (nor in a Visual Studio install on Windows); install CMake (macOS: brew install cmake; Linux: apt install cmake; Windows: https://cmake.org/download/)"$'\n'
    fi

    if [ -n "$problems" ]; then
        if [ "$dry_run" = 1 ]; then
            while IFS= read -r p; do [ -z "$p" ] || warn "the build would fail: $p"; done <<<"$problems"
        else
            fail "cannot build:"$'\n'"$(printf '%s' "$problems" | sed 's/^/  - /')"
        fi
    fi
    say "PROTOC = ${protoc:-(not found)}"
    if [ -n "$cmake_dir" ]; then say "CMake  = $cmake_dir (prepended to PATH for the build only)"; else say "CMake  = on PATH"; fi

    if [ "$dry_run" = 1 ]; then
        say "would run: cargo build --release -p konnect (in $repo_root)"
    else
        protoc_env="$protoc"
        [ "$windows" = 0 ] || protoc_env="$(cygpath -m -- "$protoc")"
        (
            export PROTOC="$protoc_env"
            if [ -n "$cmake_dir" ]; then export PATH="$cmake_dir:$PATH"; fi
            cd "$repo_root"
            cargo build --release -p konnect
        ) || fail "cargo build --release -p konnect failed."
        [ -f "$source_bin" ] || fail "the build finished but $source_bin is missing."
        add_change "built $source_bin"
    fi
fi
pending=0
if [ ! -f "$source_bin" ]; then pending=1; fi
if [ "$dry_run" = 1 ] && [ -n "$build_reason" ] && [ "$skip_build" = 0 ]; then pending=1; fi
source_version="(built by the real run)"
[ "$pending" = 1 ] || source_version="$(konnect_version "$source_bin")"
say "new build version: $source_version"

# ---- 2. install target -----------------------------------------------------
step "Install target"
target_note=""
if [ -n "$target" ]; then
    target_u="$(abs_path "$target")"
    say "from --target: $(to_native "$target_u")"
else
    found=""
    found_where=""
    unreadable=0
    if [ -f "$claude_config" ]; then
        entries=""
        if [ -z "$json_reader" ]; then
            warn "neither jq nor python is available to read $claude_config; falling back to the default target."
            unreadable=1
        elif ! config_is_valid_json "$claude_config"; then
            fail "$claude_config is not valid JSON (checked with $json_reader); fix the file or pass --target."
        else
            rc=0
            entries="$(read_config_commands "$claude_config")" || rc=$?
            [ "$rc" = 0 ] || fail "cannot read $claude_config ($json_reader exited $rc); pass --target."
        fi
        top=""
        while IFS=$'\t' read -r kind cmd _; do
            if [ "$kind" = "top" ] && [ -z "$top" ]; then top="$cmd"; fi
        done <<<"$entries"
        if [ -n "$top" ]; then
            is_konnect_path "$top" || fail "the top-level mcpServers.konnect.command in $claude_config is '$top', not a path to a konnect executable (a wrapper or a command with arguments). Pass --target <path to konnect$exe>."
            found="$(abs_path "$top")"
            found_where="top-level mcpServers.konnect.command in $claude_config"
            # Inside a project, Claude Code runs that project's own konnect entry.
            shadowed=""
            top_key="$(path_key "$found")"
            while IFS=$'\t' read -r kind name cmd; do
                [ "$kind" = "project" ] || continue
                shown="$cmd"
                if is_konnect_path "$cmd"; then
                    p="$(abs_path "$cmd")"
                    [ "$(path_key "$p")" != "$top_key" ] || continue
                    shown="$(to_native "$p")"
                fi
                shadowed="${shadowed}  - project $name -> $shown"$'\n'
            done <<<"$entries"
            if [ -n "$shadowed" ]; then
                warn "Claude Code gives a project's own konnect entry precedence inside that project, so these projects keep running a different konnect than the top-level one ($(to_native "$found")):"$'\n'"${shadowed}This installer still installs to $(to_native "$found"); update those project entries, or pass --target to install elsewhere."
            fi
        else
            keys=""
            listing=""
            count=0
            while IFS=$'\t' read -r kind name cmd; do
                [ "$kind" = "project" ] || continue
                is_konnect_path "$cmd" || fail "project '$name' in $claude_config registers konnect as '$cmd', not a path to a konnect executable (a wrapper or a command with arguments). Pass --target <path to konnect$exe>."
                p="$(abs_path "$cmd")"
                k="$(path_key "$p")"
                case $'\n'"$keys" in *$'\n'"$k"$'\n'*) continue;; esac
                keys="${keys}${k}"$'\n'
                listing="${listing}  - $(to_native "$p")   (project $name)"$'\n'
                count=$((count + 1))
                found="$p"
            done <<<"$entries"
            if [ "$count" -gt 1 ]; then
                fail "$claude_config registers konnect at several paths:"$'\n'"${listing}Pass --target <path> to choose one."
            fi
            [ "$count" = 0 ] || found_where="the only project-level mcpServers.konnect.command in $claude_config"
        fi
    else
        say "no Claude config at $claude_config"
    fi
    if [ -n "$found" ]; then
        target_u="$found"
        say "from $found_where"
        say "-> $(to_native "$target_u")"
    else
        target_u="$HOME/.konnect/bin/konnect$exe"
        if [ "$unreadable" = 1 ]; then
            say "Claude config not read (install jq or python, or pass --target); using the default $(to_native "$target_u")"
        else
            say "no konnect MCP server registered in the Claude config; using the default $(to_native "$target_u")"
        fi
        # --bootstrap registers the server itself further down, so the manual
        # command is only worth printing when this run will not do it.
        if [ "$bootstrap" = 1 ]; then
            say "the MCP registration step below will register this path with Claude"
        else
            if [ "$unreadable" = 1 ]; then
                target_note="If Konnect is not registered with Claude Code yet (this installer only does it with --bootstrap), run:"
            else
                target_note="Konnect is not registered with Claude Code. To register it (or re-run with --bootstrap), run:"
            fi
            target_note="$target_note
    claude mcp add --scope user konnect -- \"$(to_native "$target_u")\""
            say "NOTE: $target_note"
        fi
    fi
fi
target_shown="$(to_native "$target_u")"
if [ -d "$target_u" ]; then
    fail "the install target $target_shown is a folder. Pass the full path to the konnect$exe file, e.g. --target '$(to_native "${target_u%/}/konnect$exe")'."
fi
case "$target_u" in
    */) fail "the install target $target_shown ends with a separator. Pass the full path to the konnect$exe file, e.g. --target '$(to_native "${target_u%/}/konnect$exe")'.";;
esac

# ---- 3. backup + copy ------------------------------------------------------
step "Install binary"
old_version=""
new_tag="$source_version"
if [ "$pending" = 1 ]; then
    new_tag="new"
    say "the new build does not exist yet (dry run) - showing what the real run would do"
fi
identical=0
if [ "$pending" = 0 ]; then
    say "new build sha256: $(file_hash "$source_bin")"
    if [ -f "$target_u" ] && same_file_content "$source_bin" "$target_u"; then identical=1; fi
fi
if [ "$identical" = 1 ]; then
    say "identical binary already at $target_shown - skipping backup and copy"
else
    target_dir="$(dirname "$target_u")"
    base="$(basename "$target_u")"
    base="${base%.[eE][xX][eE]}"
    stamp="$(date +%Y%m%d-%H%M%S)"
    parked="$target_dir/$base-$new_tag-$stamp-rolledback$exe.bak"
    if target_exists; then
        old_version="$(konnect_version "$target_u")"
        say "existing binary: version $old_version, sha256 $(file_hash "$target_u")"
        backup_name="$target_dir/$base-$old_version-$stamp$exe.bak"
        n=2
        while [ -e "$backup_name" ] || [ -L "$backup_name" ]; do
            backup_name="$target_dir/$base-$old_version-$stamp-$n$exe.bak"
            n=$((n + 1))
        done
        if [ "$dry_run" = 1 ]; then
            say "would rename $target_u -> $backup_name"
        else
            mv -- "$target_u" "$backup_name" || fail "could not rename $target_u to $backup_name; nothing was changed."
            say "renamed $target_u -> $backup_name"
        fi
        backup="$backup_name"
    fi
    if [ "$dry_run" = 1 ]; then
        say "would copy $source_bin -> $target_u"
        [ -n "$backup" ] || fresh_install=1
    else
        # Never overwrite: whatever sits at the target now has no backup.
        if target_exists; then
            [ -z "$backup" ] || fail "$target_shown still exists after renaming it to $(to_native "$backup"); stopping before the copy so nothing is overwritten."
            fail "$target_shown appeared while the installer was running; stopping before the copy so nothing is overwritten."
        fi
        if ! { mkdir -p -- "$target_dir" && cp -- "$source_bin" "$target_u"; }; then
            if [ -n "$backup" ]; then
                if ! target_exists && mv -- "$backup" "$target_u"; then
                    backup=""
                    fail "copy to $target_shown failed; the previous binary was renamed back - nothing changed."
                fi
                fail "copy to $target_shown failed, and putting the previous binary back failed too. The previous binary is safe at $(to_native "$backup"); the lines below restore it."
            fi
            if target_exists; then fresh_install=1; fi
            fail "copy to $target_shown failed."
        fi
        [ -n "$backup" ] || fresh_install=1
        same_file_content "$source_bin" "$target_u" || fail "$target_shown does not match the build after copying."
        say "copied $source_bin -> $target_u"
    fi
    if [ -n "$backup" ]; then
        add_change "$verb $target_shown (old binary $old_version renamed to $(to_native "$backup"))"
    else
        add_change "$verb $target_shown (fresh install: there was no konnect there before)"
    fi
fi

# ---- 4. init + status ------------------------------------------------------
# Compares the counts konnect init reported against manifest.rs.
# $1 = client name as konnect prints it, $2 = "<skills> <agents>" or empty.
check_init_counts() {
    local label="$1" counts="$2" got_skills got_agents
    if [ -z "$counts" ]; then
        warn "konnect init printed no 'Done: ... installed for $label.' line; counts not checked."
    elif [ -z "$expected_skills" ]; then
        warn "no manifest at $manifest; counts not checked."
    else
        got_skills="${counts% *}"
        got_agents="${counts#* }"
        if [ "$got_skills" != "$expected_skills" ] || [ "$got_agents" != "$expected_agents" ]; then
            warn "init installed $got_skills skills / $got_agents agents for $label but manifest.rs lists $expected_skills / $expected_agents - is the installed build current?"
        else
            say "counts match manifest.rs for $label: $got_skills skills, $got_agents agents"
        fi
    fi
}
step "Guidance (konnect init)"
if [ "$skip_init" = 1 ]; then
    say "skipped (--skip-init)"
else
    for c in $clients; do
        # claude is the binary's default client, so it takes no --client flag.
        if [ "$c" = claude ]; then init_args=(init); else init_args=(init --client "$c"); fi
        init_clients="$init_clients $c"
        if [ "$dry_run" = 1 ]; then
            say "would run: \"$target_shown\" ${init_args[*]}"
            say "would run: \"$target_shown\" status --client $c"
            continue
        fi
        say "running: \"$target_shown\" ${init_args[*]}"
        rc=0
        init_out="$("$target_u" "${init_args[@]}")" || rc=$?
        init_out="$(printf '%s\n' "$init_out" | tr -d '\r')"
        printf '%s\n' "$init_out" | sed 's/^/    /'
        [ "$rc" = 0 ] || fail "konnect ${init_args[*]} failed (exit $rc)."
        add_change "installed $c guidance ($target_shown ${init_args[*]})"
        # codex installs skills only, so there is nothing to compare for it.
        case "$c" in
            claude) check_init_counts Claude "$(printf '%s\n' "$init_out" | sed -n 's/^Done: \([0-9]\{1,\}\) skills, \([0-9]\{1,\}\) agents, \([0-9]\{1,\}\) hooks installed for Claude\..*/\1 \2/p' | tail -1)";;
            omp)    check_init_counts OMP "$(printf '%s\n' "$init_out" | sed -n 's/^Done: \([0-9]\{1,\}\) skills, \([0-9]\{1,\}\) agents installed for OMP\..*/\1 \2/p' | tail -1)";;
        esac
        say "running: \"$target_shown\" status --client $c"
        rc=0
        status_out="$("$target_u" status --client "$c")" || rc=$?
        printf '%s\n' "$status_out" | tr -d '\r' | sed 's/^/    /'
        [ "$rc" = 0 ] || warn "konnect status --client $c exited $rc."
    done
fi

# ---- 5. RETRACE_PYTHON -----------------------------------------------------
step "Photo intake (RETRACE_PYTHON)"
retrace_note=""
if [ "$no_retrace" = 1 ]; then
    say "skipped (--no-retrace)"
else
    if [ "$with_retrace" = 1 ] && [ -z "$retrace_python" ]; then
        venv="$repo_root/.venv-retrace"
        venv_py="$venv/bin/python"
        [ "$windows" = 0 ] || venv_py="$venv/Scripts/python.exe"
        if [ -f "$venv_py" ]; then
            say "venv: $venv already exists"
        else
            base_py=""
            for cand in python3 python; do
                if have "$cand"; then base_py="$cand"; break; fi
            done
            [ -n "$base_py" ] || fail "--with-retrace needs python3 (3.10+) on PATH."
            run_cmd "$base_py" -m venv "$venv" || fail "$base_py -m venv $venv failed."
            [ "$dry_run" = 1 ] || add_change "created $venv"
        fi
        if [ "$dry_run" = 1 ]; then
            say "would run: $venv_py -m pip install git+https://github.com/ericrihm/retrace.git"
        elif probe "$venv_py" -c 'import retrace' >/dev/null 2>&1; then
            say "retrace is already importable from the venv"
        else
            run_cmd "$venv_py" -m pip install --quiet --upgrade pip || warn "pip self-upgrade failed; continuing."
            if run_cmd "$venv_py" -m pip install --quiet 'git+https://github.com/ericrihm/retrace.git'; then
                add_change "installed retrace into $venv"
            else
                warn "pip install retrace failed; see docs/PHOTO_TO_BOARD_WORKFLOW.md. Photo intake stays unconfigured."
            fi
        fi
    fi
    py=""
    if [ -n "$retrace_python" ]; then
        py="$(abs_path "$retrace_python")"
    elif [ "$windows" = 1 ] && [ -f "$repo_root/.venv-retrace/Scripts/python.exe" ]; then
        py="$repo_root/.venv-retrace/Scripts/python.exe"
    elif [ -f "$repo_root/.venv-retrace/bin/python" ]; then
        py="$repo_root/.venv-retrace/bin/python"
    fi
    retrace_version=""
    if [ -n "$py" ] && [ -f "$py" ]; then
        rc=0
        out="$(probe "$py" -c 'import retrace; print(getattr(retrace, "__version__", "unknown"))' 2>&1)" || rc=$?
        out="$(printf '%s' "$out" | tr -d '\r')"
        if [ "$rc" = 0 ]; then retrace_version="$(printf '%s\n' "$out" | tail -1)"
        else warn "$py cannot import retrace: $(printf '%s\n' "$out" | tail -1)"
        fi
    elif [ -n "$py" ]; then
        warn "--retrace-python $py does not exist."
    fi
    if [ -z "$retrace_version" ]; then
        retrace_note="No Python with retrace found; photo intake stays unconfigured. See docs/PHOTO_TO_BOARD_WORKFLOW.md to create .venv-retrace, then re-run this script."
        say "NOTE: $retrace_note"
    else
        say "retrace $retrace_version importable from $py"
        if [ "$windows" = 1 ]; then
            py_native="$(cygpath -w -- "$py")"
            current="$(MSYS2_ARG_CONV_EXCL='*' reg.exe query 'HKCU\Environment' /v RETRACE_PYTHON 2>/dev/null | tr -d '\r' | sed -n 's/^[[:space:]]*RETRACE_PYTHON[[:space:]]\{1,\}REG_[A-Z_]*[[:space:]]\{1,\}//p' | head -1)" || current=""
            if [ -n "$current" ] && [ "$(path_key "$(cygpath -u -- "$current")")" = "$(path_key "$py")" ]; then
                say "RETRACE_PYTHON (user) already $current - unchanged"
            else
                shown="${current:-(unset)}"
                if [ "$dry_run" = 1 ]; then
                    say "would run: setx RETRACE_PYTHON \"$py_native\"   ($shown -> $py_native)"
                else
                    setx RETRACE_PYTHON "$py_native" >/dev/null || fail "setx RETRACE_PYTHON failed."
                    say "set RETRACE_PYTHON (user): $shown -> $py_native"
                fi
                retrace_changed=1
                retrace_previous="$current"
                add_change "$verb RETRACE_PYTHON (user): $shown -> $py_native"
            fi
        else
            current="${RETRACE_PYTHON:-}"
            if [ "$current" = "$py" ]; then
                say "RETRACE_PYTHON already $current - unchanged"
            else
                say "RETRACE_PYTHON: ${current:-(unset)} -> $py"
                say "add this line to your shell profile (this script never edits rc files):"
                say "    export RETRACE_PYTHON=$(sh_quote "$py")"
                retrace_note="RETRACE_PYTHON is not set to $py; add: export RETRACE_PYTHON=$(sh_quote "$py") - then close Claude Code completely and start it again from a new terminal."
            fi
        fi
    fi
fi

# ---- 6. MCP registration ---------------------------------------------------
# What makes a fresh clone actually usable: a client that does not know about
# the binary never starts it. Only --bootstrap writes here; the default run
# still touches no client config.
step "MCP server registration"
mcp_command="$(to_native "$target_u")"

# The name is listed by both CLIs; matching a whole word keeps 'konnect-dev'
# from counting as an existing 'konnect'.
mcp_cli_has_konnect() {
    probe "$1" mcp list 2>/dev/null | tr -d '\r' | grep -qE '(^|[^[:alnum:]_-])konnect([^[:alnum:]_-]|$)'
}

# OMP has no `mcp add` subcommand, so its user-level mcp.json is edited in
# place: the file is copied aside first, which is what the rollback restores.
register_omp_mcp() {
    local file="$HOME/.omp/agent/mcp.json" current="" payload tmp stamp
    if [ -z "$json_reader" ]; then
        warn "no jq or python found; add konnect to $(to_native "$file") by hand."
        return 0
    fi
    if [ -f "$file" ]; then
        if [ "$json_reader" = jq ]; then
            current="$(jq -r '.mcpServers.konnect.command // empty' <"$file" 2>/dev/null || true)"
        else
            current="$("$json_reader" -c '
import json, sys
text = sys.stdin.buffer.read().decode("utf-8")
data = json.loads(text) if text.strip() else {}
servers = data.get("mcpServers") if isinstance(data, dict) else None
entry = servers.get("konnect") if isinstance(servers, dict) else None
value = entry.get("command") if isinstance(entry, dict) else None
sys.stdout.write(value if isinstance(value, str) else "")
' <"$file" 2>/dev/null || true)"
        fi
        current="$(printf '%s' "$current" | tr -d '\r')"
    fi
    if [ -n "$current" ] && [ "$(path_key "$(to_unix "$current")")" = "$(path_key "$target_u")" ]; then
        say "omp: konnect already points at $mcp_command"
        return 0
    fi
    if [ "$dry_run" = 1 ]; then
        say "would set mcpServers.konnect.command = $mcp_command in $(to_native "$file")"
        return 0
    fi
    mkdir -p "$(dirname "$file")"
    payload='{}'
    if [ -f "$file" ]; then
        payload="$(cat "$file")"
        [ -n "$(printf '%s' "$payload" | tr -d '[:space:]')" ] || payload='{}'
        stamp="$(date +%Y%m%d-%H%M%S)"
        mcp_omp_backup="$file.$stamp.bak"
        cp -p -- "$file" "$mcp_omp_backup" || fail "could not copy $file aside."
    fi
    mcp_omp_file="$file"
    tmp="$file.konnect-tmp"
    if [ "$json_reader" = jq ]; then
        printf '%s' "$payload" | jq --arg cmd "$mcp_command" \
            '. + {mcpServers: ((.mcpServers // {}) + {konnect: {type: "stdio", command: $cmd}})}' >"$tmp" \
            || fail "could not write $tmp; $file is unchanged."
    else
        KONNECT_MCP_COMMAND="$mcp_command" "$json_reader" -c '
import json, os, sys
text = sys.stdin.buffer.read().decode("utf-8")
data = json.loads(text) if text.strip() else {}
if not isinstance(data, dict):
    raise SystemExit("mcp.json is not a JSON object")
servers = data.get("mcpServers")
if not isinstance(servers, dict):
    servers = {}
servers["konnect"] = {"type": "stdio", "command": os.environ["KONNECT_MCP_COMMAND"]}
data["mcpServers"] = servers
sys.stdout.write(json.dumps(data, indent=2) + "\n")
' <<<"$payload" >"$tmp" || fail "could not write $tmp; $file is unchanged."
    fi
    mv -f -- "$tmp" "$file" || fail "could not replace $file."
    mcp_added="$mcp_added omp"
    add_change "registered the konnect MCP server for OMP in $(to_native "$file")"
    say "omp: mcpServers.konnect -> $mcp_command"
}

if [ "$bootstrap" = 0 ]; then
    say "skipped (pass --bootstrap to register konnect with the selected clients)"
else
    for c in $clients; do
        case "$c" in
            claude|codex)
                if ! have "$c"; then
                    warn "$c is not on PATH; register by hand: $c mcp add konnect -- \"$mcp_command\""
                    continue
                fi
                if mcp_cli_has_konnect "$c"; then
                    say "$c: konnect is already registered"
                    continue
                fi
                if [ "$c" = claude ]; then
                    run_cmd claude mcp add konnect -s user -- "$mcp_command" || { warn "claude mcp add konnect failed."; continue; }
                else
                    run_cmd codex mcp add konnect -- "$mcp_command" --client codex || { warn "codex mcp add konnect failed."; continue; }
                fi
                if [ "$dry_run" = 1 ]; then continue; fi
                mcp_added="$mcp_added $c"
                add_change "registered the konnect MCP server with $c"
                ;;
            omp) register_omp_mcp;;
        esac
    done
    say "restart the client so it picks the server up."
fi

# ---- summary ---------------------------------------------------------------
step "Summary"
[ "$dry_run" = 0 ] || say "DRY RUN - nothing was changed."
[ -n "$changes" ] || say "nothing changed."
while IFS= read -r line; do [ -z "$line" ] || say "- $line"; done <<<"$changes"
say "target:  $target_shown (new build version $source_version)"
if [ -n "$backup" ]; then say "backup:  $(to_native "$backup")"
elif [ "$fresh_install" = 1 ]; then say "backup:  (none - fresh install: there was no konnect at $target_shown before)"
else say "backup:  (none)"
fi
[ -z "$target_note" ] || say "NOTE: $target_note"
[ -z "$retrace_note" ] || say "NOTE: $retrace_note"
case " $init_clients " in
    *" omp "*) say "OMP guidance: ~/.omp/agent/skills (skills) and ~/.omp/agent/agents (agents)";;
esac
rollback="$(rollback_lines)"
if [ -n "$rollback" ]; then
    if [ "$fresh_install" = 1 ]; then say "undo (this shell; safe to paste twice) - this was a fresh install, so undoing it moves the new file aside:"
    else say "rollback (this shell; safe to paste twice):"
    fi
    while IFS= read -r line; do [ -z "$line" ] || say "    $line"; done <<<"$rollback"
else
    say "rollback: nothing to roll back."
fi
if [ "$retrace_changed" = 1 ]; then
    say "RETRACE_PYTHON only reaches programs started after it is set: close Claude Code completely"
    say "(every window, and any terminal running it) and start it again from a new terminal or the Start menu."
else
    say "Restart Claude Code (all windows) so the MCP server and the skills reload."
fi
exit 0
