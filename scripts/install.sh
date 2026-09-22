#!/usr/bin/env bash
# Build Konnect from this checkout and install it where Claude Code runs it
# (macOS/Linux and Git Bash/MSYS/Cygwin port of install.ps1).
#
# Local-build installer for developers and testers. The end-user path stays
# the KiCAD PCM zip built by packaging/build-pcm.sh. Steps:
#
#   1. build     cargo build --release -p konnect, when target/release/konnect[.exe]
#                is missing, older than the newest git-tracked file under crates/
#                or Cargo.lock, or --rebuild is given
#   2. target    --target, else mcpServers.konnect.command in the Claude config
#                (top level first, then a single distinct project-level entry),
#                else ~/.konnect/bin/konnect[.exe]; JSON read with jq, else
#                python3/python
#   3. install   identical binary -> skip; otherwise RENAME the old binary to
#                <name>-<version>-<yyyyMMdd-HHmmss>[.exe].bak (never deleted),
#                then copy the new build into place
#   4. init      <target> init (Claude) and/or <target> init --client codex,
#                then <target> status --client <c>
#   5. retrace   point RETRACE_PYTHON at a Python that can `import retrace`
#                (default: <repo>/.venv-retrace): setx on Windows shells; on
#                macOS/Linux the export line is printed, rc files are not edited
#
# It never registers or edits an MCP server entry, never touches KiCAD
# projects and never deletes a binary.
#
# Usage:
#   scripts/install.sh [--client claude|codex|both] [--target PATH]
#                      [--claude-config PATH] [--skip-build | --rebuild]
#                      [--skip-init] [--retrace-python PATH | --no-retrace]
#                      [--dry-run] [--help]
#
# --dry-run prints every action and changes nothing.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

client="claude"
target=""
claude_config=""
skip_build=0
rebuild=0
skip_init=0
retrace_python=""
no_retrace=0
dry_run=0

usage() {
    cat <<'EOF'
Usage: scripts/install.sh [--client claude|codex|both] [--target PATH]
                          [--claude-config PATH] [--skip-build | --rebuild]
                          [--skip-init] [--retrace-python PATH | --no-retrace]
                          [--dry-run] [--help]

Builds Konnect from this checkout, installs it where Claude Code runs it,
installs the bundled guidance (konnect init) and points RETRACE_PYTHON at the
photo-intake Python.

  --client          guidance to install: claude (default), codex or both
  --target          install path of the konnect binary (default: the command
                    registered in the Claude config, else ~/.konnect/bin/konnect)
  --claude-config   Claude config to read the target from (default ~/.claude.json)
  --skip-build      use the existing target/release/konnect
  --rebuild         build even when the existing build is current
  --skip-init       do not run konnect init / status
  --retrace-python  Python to use for RETRACE_PYTHON (default <repo>/.venv-retrace)
  --no-retrace      leave RETRACE_PYTHON alone
  --dry-run         print every action, change nothing
EOF
}

fail() { echo "install: ERROR: $*" >&2; exit 1; }
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
        -h|--help)        usage; exit 0;;
        *) echo "Unknown argument: $1" >&2; usage >&2; exit 2;;
    esac
done

case "$client" in
    claude|codex|both) ;;
    *) fail "--client must be claude, codex or both (got '$client').";;
esac
[ "$skip_build" = 1 ] && [ "$rebuild" = 1 ] && fail "--skip-build and --rebuild contradict each other; pass one."
[ "$no_retrace" = 1 ] && [ -n "$retrace_python" ] && fail "--retrace-python and --no-retrace contradict each other; pass one."

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

# Prints "top<TAB>command" and "project<TAB>name<TAB>command" lines for every
# mcpServers.konnect.command in the config. Exit 2 = no JSON reader available.
read_config_commands() {
    local config="$1"
    if command -v jq >/dev/null 2>&1; then
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
    local py=""
    for candidate in python3 python; do
        if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c 'import json' >/dev/null 2>&1; then
            py="$candidate"; break
        fi
    done
    [ -n "$py" ] || return 2
    "$py" -c '
import json, sys
data = json.loads(sys.stdin.buffer.read().decode("utf-8"))
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

changes=""
rollback=""
add_change() { changes="${changes}$1"$'\n'; }
add_rollback() { rollback="${rollback}$1"$'\n'; }

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

# ---- 1. build --------------------------------------------------------------
step "Build"
source_bin="$repo_root/target/release/konnect$exe"
build_reason=""
stale_reason=""
if [ -f "$source_bin" ]; then
    newest=""
    while IFS= read -r -d '' rel; do
        f="$repo_root/$rel"
        [ -f "$f" ] || continue
        if [ -z "$newest" ] || [ "$f" -nt "$newest" ]; then newest="$f"; fi
    done < <(git -C "$repo_root" ls-files -z -- crates Cargo.lock 2>/dev/null || true)
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
        problems="${problems}protoc not found: export PROTOC=/path/to/protoc or put protoc on PATH (macOS: brew install protobuf; Linux: apt install protobuf-compiler; Windows: https://github.com/protocolbuffers/protobuf/releases)"$'\n'
    fi

    cmake_dir=""
    if ! command -v cmake >/dev/null 2>&1; then
        if [ "$windows" = 1 ]; then
            for root_var in 'ProgramFiles(x86)' 'ProgramFiles'; do
                root="$(printenv "$root_var" 2>/dev/null || true)"
                [ -n "$root" ] || continue
                root="$(cygpath -u -- "$root")"
                for d in "$root"/"Microsoft Visual Studio"/*/*/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin; do
                    if [ -f "$d/cmake.exe" ]; then cmake_dir="$d"; fi
                done
                [ -z "$cmake_dir" ] || break
            done
        fi
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
        rc=0
        entries="$(read_config_commands "$claude_config")" || rc=$?
        if [ "$rc" = 2 ]; then
            warn "neither jq nor python is available to read $claude_config; falling back to the default target."
            entries=""
            unreadable=1
        elif [ "$rc" != 0 ]; then
            fail "cannot parse $claude_config; pass --target."
        fi
        top=""
        while IFS=$'\t' read -r kind cmd _; do
            if [ "$kind" = "top" ] && [ -z "$top" ]; then top="$cmd"; fi
        done <<<"$entries"
        if [ -n "$top" ]; then
            is_konnect_path "$top" || fail "the top-level mcpServers.konnect.command in $claude_config is '$top', not a path to a konnect executable (a wrapper or a command with arguments). Pass --target <path to konnect$exe>."
            found="$(abs_path "$top")"
            found_where="top-level mcpServers.konnect.command in $claude_config"
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
            target_note="If Konnect is not registered with Claude Code yet (this installer never does it), run:"
        else
            say "no konnect MCP server registered in the Claude config; using the default $(to_native "$target_u")"
            target_note="Konnect is not registered with Claude Code. To register it (this installer never does), run:"
        fi
        target_note="$target_note
    claude mcp add --scope user konnect -- \"$(to_native "$target_u")\""
        say "NOTE: $target_note"
    fi
fi
target_shown="$(to_native "$target_u")"

# ---- 3. backup + copy ------------------------------------------------------
step "Install binary"
backup=""
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
    if [ -f "$target_u" ]; then
        old_version="$(konnect_version "$target_u")"
        say "existing binary: version $old_version, sha256 $(file_hash "$target_u")"
        backup="$target_dir/$base-$old_version-$stamp$exe.bak"
        n=2
        while [ -e "$backup" ]; do
            backup="$target_dir/$base-$old_version-$stamp-$n$exe.bak"
            n=$((n + 1))
        done
        if [ "$dry_run" = 1 ]; then
            say "would rename $target_u -> $backup"
        else
            mv -- "$target_u" "$backup" || fail "could not rename $target_u to $backup; nothing was changed."
            say "renamed $target_u -> $backup"
        fi
    fi
    if [ "$dry_run" = 1 ]; then
        say "would copy $source_bin -> $target_u"
    else
        if ! { mkdir -p -- "$target_dir" && cp -- "$source_bin" "$target_u"; }; then
            if [ -n "$backup" ]; then
                mv -- "$backup" "$target_u" || true
                fail "copy to $target_u failed; the previous binary was renamed back."
            fi
            fail "copy to $target_u failed."
        fi
        same_file_content "$source_bin" "$target_u" || fail "$target_u does not match the build after copying; previous binary kept at ${backup:-(none)}."
        say "copied $source_bin -> $target_u"
    fi
    if [ -n "$backup" ]; then
        add_change "$verb $target_shown (old binary $old_version renamed to $(to_native "$backup"))"
        add_rollback "mv -- '$target_u' '$target_dir/$base-$new_tag-$stamp-rolledback$exe.bak'"
        add_rollback "mv -- '$backup' '$target_u'"
    else
        add_change "$verb $target_shown (new file, no previous binary)"
    fi
fi

# ---- 4. init + status ------------------------------------------------------
clients="$client"
[ "$client" != both ] || clients="claude codex"
step "Guidance (konnect init)"
if [ "$skip_init" = 1 ]; then
    say "skipped (--skip-init)"
else
    for c in $clients; do
        if [ "$c" = codex ]; then init_args=(init --client codex); else init_args=(init); fi
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
        if [ "$c" = claude ]; then
            counts="$(printf '%s\n' "$init_out" | sed -n 's/^Done: \([0-9]\{1,\}\) skills, \([0-9]\{1,\}\) agents, \([0-9]\{1,\}\) hooks installed for Claude\..*/\1 \2/p' | tail -1)"
            if [ -z "$counts" ]; then
                warn "konnect init printed no 'Done: ... installed for Claude.' line; counts not checked."
            elif [ -z "$expected_skills" ]; then
                warn "no manifest at $manifest; counts not checked."
            else
                got_skills="${counts% *}"
                got_agents="${counts#* }"
                if [ "$got_skills" != "$expected_skills" ] || [ "$got_agents" != "$expected_agents" ]; then
                    warn "init installed $got_skills skills / $got_agents agents but manifest.rs lists $expected_skills / $expected_agents - is the installed build current?"
                else
                    say "counts match manifest.rs: $got_skills skills, $got_agents agents"
                fi
            fi
        fi
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
                add_change "$verb RETRACE_PYTHON (user): $shown -> $py_native"
                if [ -n "$current" ]; then add_rollback "setx RETRACE_PYTHON '$current'"
                else add_rollback "powershell -NoProfile -Command '[Environment]::SetEnvironmentVariable(\"RETRACE_PYTHON\", \$null, \"User\")'"
                fi
            fi
        else
            current="${RETRACE_PYTHON:-}"
            if [ "$current" = "$py" ]; then
                say "RETRACE_PYTHON already $current - unchanged"
            else
                say "RETRACE_PYTHON: ${current:-(unset)} -> $py"
                say "add this line to your shell profile (this script never edits rc files):"
                say "    export RETRACE_PYTHON='$py'"
                retrace_note="RETRACE_PYTHON is not set to $py; add: export RETRACE_PYTHON='$py'"
            fi
        fi
    fi
fi

# ---- summary ---------------------------------------------------------------
step "Summary"
[ "$dry_run" = 0 ] || say "DRY RUN - nothing was changed."
[ -n "$changes" ] || say "nothing changed."
while IFS= read -r line; do [ -z "$line" ] || say "- $line"; done <<<"$changes"
say "target:  $target_shown (new build version $source_version)"
if [ -n "$backup" ]; then say "backup:  $(to_native "$backup")"; else say "backup:  (none)"; fi
[ -z "$target_note" ] || say "NOTE: $target_note"
[ -z "$retrace_note" ] || say "NOTE: $retrace_note"
if [ -n "$rollback" ]; then
    say "rollback (this shell):"
    while IFS= read -r line; do [ -z "$line" ] || say "    $line"; done <<<"$rollback"
    if [ -n "$backup" ]; then
        for c in $clients; do
            if [ "$c" = codex ]; then say "    '$target_u' init --client codex"; else say "    '$target_u' init"; fi
        done
    fi
else
    say "rollback: nothing to roll back."
fi
say "Restart Claude Code (all windows) so the MCP server and the skills reload."
exit 0
