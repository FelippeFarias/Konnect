# architect — project memory

## Current state

`photo-to-kicad-reverse` and `board-dossier-reconstruction` merged into `main` at `bbd5efb`
(one `orc merge board-dossier-reconstruction`, stacked, carries both); both archived.
`konnect-orchestrator`'s design (D1-D12, fix-round DECISIONS A-N) shipped through 50/50
tasks and 3 verify rounds; archived 2026-09-22 at commit `9a0f1ce` on `orc/konnect-orchestrator`
(history `bbd5efb..9a0f1ce`). Merge to `main` pending the user's GRANT.

## Decisions affecting my role

- New toolsets go in `router/registry.rs` `ALL_TOOLSETS` (`:22`); `ToolsetMeta` is
  `router/mod.rs:20`, its hand-written `tool_count` asserted at `:358`, cap 20 (`:355`).
  At the merged base the registry holds 22 entries (`photo_intake` is `:120`); `flow` is the 23rd.
- `tool!` (`tools/mod.rs:426`) takes four args; `BoardAccess::None` is `#[default]`.
- Typed errors: `InvalidArgument`/`Conflict`/`StaleTarget` (`mcp/error.rs:49/54/80`) cover
  argument, filesystem-state and stale-approval refusals — no new variant was needed.
- `tools/config.rs` exposes `tools()` and `effective_config()` (merged). Content hashes are
  SHA-256 lowercase hex; `serde_json` `preserve_order` is OFF workspace-wide.
- Any-file atomic writers with a cross-process lock already exist in `konnect-sexp/src/writer.rs`:
  `write_atomic` `:106`, `transact_atomic` `:176` (needs the file to exist), `read_consistent`
  `:198`, `write_new_atomic` `:397` (no-clobber); lock files live in the per-user Konnect state
  dir (`:315`, `KONNECT_STATE_DIR` override). Use them before writing any new locking.
- No YAML crate and no date crate in `Cargo.lock`; the RFC 3339 helper is `photo_intake.rs:1159`
  (private; the orchestrator design widens it to `pub(crate)`).
- Codex installs skills only (`~/.agents/skills`); agents install for Claude only
  (`install.rs:239`, `:276`) — a method that must work under Codex belongs in a skill.

## Gotchas found here

1. `backticked_tool_names_in_prose_exist_in_the_registry` (`asset_references.rs:746`) flags every
   lowercase two-part snake word, backticked or bare (`snake_words` `:1013`), except registered
   tools/toolsets, **top-level** input properties, and `NOT_TOOLS` (`:773`). Never add a
   single word; never pre-add names the test has not flagged.
2. `agents_make_claimed_evidence_executable` (`:175`) and
   `skills_define_the_same_evidence_boundary_as_their_agents` are hard-coded case lists — they
   do not auto-enroll; enroll deliberately when an agent gains a toolset-dependent step.
3. `yaml_list` (`asset_references.rs:151`) parses only block lists (`  - name`); `[a, b]`
   parses as empty. No test parses `tools:`; `install.rs` writes agent bytes verbatim.
4. `doc_tool_counts.rs` sweeps EVERY `.md`/`.json` except `target`, `node_modules`, `.git`,
   `.claude`, `dist`, `build` (`:165`) — so `openspec/` and `.orchestrator/` are swept: no digit
   right before "toolset" (any value, `:225`) and no stale three-digit number right before
   "tools" (`:126`) in planning docs or handoffs. The orchestrator change adds `.orchestrator`
   and `archive` to `SKIP` (task 1.10).
5. `fixed_records_are_closed_and_only_reviewed_maps_are_extensible` (`router/mod.rs:185`)
   `.expect()`s an explicit `additionalProperties` on every object node; an array with no `items`
   subschema is never entered. Object params need `properties` or they become uncallable.
6. Observer ring = last 100 calls, tool + status only, no args (`observability.rs:27`, `:63`);
   JSONL at `%APPDATA%/konnect/logs/calls.jsonl` (`mcp/handler.rs:89`).
7. `design_state_hash` (`design_hash.rs:34`) walks `.history/` and `backup-*` folders too
   (display-remoto: 39 of 53 covered files are copies) and costs 9–180 ms on real projects.
   KiCad writes `~<name>.kicad_pro.lck`; `kicad_editor_lock_path` (`writer.rs:261`) ignores it.
8. Agents limited to `mcp__konnect__*` are told to read skill `references/` — unverified that a
   sub-agent without `Read` can; keep any rule a new agent needs in a preloaded body.
9. External-tool absence is a successful result with `available: false`
   (`handle_check_freerouting`, `integration.rs:1710`); tests needing it are `#[ignore]`.
