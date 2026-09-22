# reviewer — project memory

## Current state
- `photo-to-kicad-reverse`, `board-dossier-reconstruction`: merged into `main` at `bbd5efb`; archived.
- `konnect-orchestrator`: r1 FIX (`11-reviewer.md`, gate w/o producer) -> r2 FIX (`16`, manufacture "any warning" too wide -> ecc83-pp deadlock) -> r3 DONE (`21-reviewer.md`, ship) at `e2228f9`. Developer applied reviewer 21's 3 MINORs as task 9.7 (`8b17a76`, no 4th verify round, per orchestrator DECISION N). Archived 2026-09-22 at `9a0f1ce`; merge to `main` pending the user's GRANT. Non-blocking MINORs still open: Verdict classes drop DRC-that-never-ran; design.md:626 "DRC error always blocks" vs amended DECISION I; tool-directory flow_advance/gate/defer rows miss DECISION K.
- `install-scripts` (FAST, worktree `~/.orc/worktrees/konnect-3b7e2022/install-scripts`): r1 FIX (`03-reviewer.md`, 897c4a0) -> r2 DONE (`06-reviewer.md`, 334a8ac): all r1 items + qa R2 closed. Open MINORs: ps1 prints UNGUARDED `Move-Item` rollback in -DryRun (paste -> no konnect.exe; sh is guarded), `Format-PsLiteral` misses U+2018-201B. A 2nd FIX would have promoted the lane to PROPOSE (log DECISION) -- severity judged on merits (= r1 #2 class, MINOR).

## install-scripts facts
- This PC: PROTOC/CARGO_TARGET_DIR/RETRACE_PYTHON unset at all scopes; protoc at `~/tools/protoc/bin` (include/ beside bin); cmake only via VS glob; jq 1.7.1 (parse error exit 5), python 3.14; policy RemoteSigned; ~/.claude.json top-level konnect only (0/34 projects). konnect.exe is RUNNING from `~/.konnect/bin` (renaming a running exe works; test with a copied PING.EXE -n 90).
- Probe kit r2: scratchpad `rv2/` (fake git repo + rustc fakes old.exe 0.11.0-old/dc5887e1, new 0.99.0-new/1fba2b3d; `wrap-hash.ps1`/`wrap-cm.ps1` cmdlet overrides; p1-p5.ps1). Real runs only with temp -Target -SkipBuild -SkipInit -NoRetrace. Main build 08:54:52 is fresh vs the new input list (newest doc_tool_counts.rs 08:52:14). No ~/.cargo/config.toml.

## flow toolset facts (konnect-orchestrator)
- `flow.rs`: `transact<T>` + `transact_then_record` (gate/advance/defer):
  STATE.md commits, THEN gate file + log (`record_after_commit`, warning on
  failure, lock already released). `flow_log` still appends inside the lock;
  `flow_start` keeps `log_error`. Phase records still written before commit.
- `validate_phases` enforces phase⇔gate both ways (`GATED_PHASES`); producer
  is always adjacent to its gate, so a gate is entered only forward from it.
  `move_back` refuses gate targets; `gate_validity` gives `status`
  current|passed, `valid` only on current.
- `konnect_sexp::transact_atomic` (writer.rs:176) can return Err AFTER the
  rename (read-back, Unix dir fsync) — pre-existing, deferred.
- Probe kit: session scratchpad `probe/` (r1/r2) and `probe3/` (r3). MSBuild
  (nng-sys) fails on long CARGO_TARGET_DIR paths → use `%TEMP%\k16t`. Test a
  `git archive <sha>` export when another agent is mutating the worktree.
- `place_component` on a scratch board: file fallback, resolves libs from the
  scratch dir's tables + global; `register_*_library` project scope writes an
  absolute URI for a lib outside the project (`library.rs:1952-1975`).
- kicad-cli 10.0.2 at `C:\Program Files\KiCad\10.0\bin`; KiCad demo fixtures
  in `crates/konnect-sexp/tests/fixtures/` give real DRC outputs.

## Guards (crates/konnect/tests/asset_references.rs)
- `missing_markers` = whitespace-flattened `contains`; section-scoped only
  where `section(...)` is used. Count marker occurrences before trusting one.
- Directory-driven (self-extending): `top_level_skill_routes_every_bundled_agent`,
  `agents_preload_existing_skills`, `every_agent_that_names_a_flow_tool_loads_flow`.
  Hard-coded lists: `agents_make_claimed_evidence_executable`,
  `skills_define_the_same_evidence_boundary_as_their_agents`,
  `orchestrator_agents_keep_their_contract` (manufacture markers are generic:
  "INCOMPLETE" etc.).
- `call_examples_name_real_parameters` checks backticked `tool(a, b)` names
  against schemas; deleting the sentence stays green.
- Catalogue: 23 toolsets, 238 registered (245 with meta), 12 skills, 11 agents.
  README "N skills + N agents", TROUBLESHOOTING, packaging/plugin metadata are
  unguarded — count `ls assets/{skills,agents}`.

## Gotchas
- A foreign cargo in the shared worktree `target/` causes LNK1104 / "extern
  location does not exist": wait and re-run, or use your own target dir.
- `review_map_content_hash` mixes key-list projection (fail-open) with whole
  clones (fail-closed); `overlay_known_fields` keeps explicit nulls — the MCP
  jsonschema validator is the real null guard.
- `library.rs:1818` `portable_uri` still strips `\\?\` naively (UNC bug).
- `schema_parameter_usage.rs` drops any fn whose signature has a bare `;`.
- `git archive` here applies autocrlf: exported files are CRLF, so compare a
  restored probe file with the backup copy, not with `git show` (LF).
