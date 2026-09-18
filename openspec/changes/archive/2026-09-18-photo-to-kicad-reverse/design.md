## Context

Konnect (`crates/konnect-core`) is a Rust MCP server for KiCad 10, organized as named toolsets (`crates/konnect-core/src/router/registry.rs:22` `ALL_TOOLSETS`, `crates/konnect-core/src/router/registry.rs:172` `build_tools_for`; the `ToolsetMeta` struct itself lives at `crates/konnect-core/src/router/mod.rs:20`) that bundled Claude agents load on demand via `load_toolset(name)`. Two existing modules already wrap an external CLI/process as typed Rust: `crates/konnect-core/src/tools/cli.rs` (kicad-cli, `tokio::process::Command`, typed result structs, `cli_failure_diagnostics` at `cli.rs:25`, `run_cli`/`run_cli_captured` at `cli.rs:309`/`cli.rs:314`) and `crates/konnect-core/src/freerouting_mcp.rs` + `crates/konnect-core/src/tools/integration.rs` (`handle_check_freerouting` at `integration.rs:1710`, `handle_route_specctra_dsn` at `integration.rs:1789`, JAR discovery via `find_freerouting_jar` at `integration.rs:1483`, the `probe_local` capability probe at `freerouting_mcp.rs:85`, `command_output` at `integration.rs:1494`). Bundled agents (`crates/konnect/assets/agents/*.md`) can only call `mcp__konnect__*` tools — no `Bash` — so any new external capability must become a real MCP tool, not a skill-only instruction (per the `konnect` skill's Channel-1 mutation rule, `crates/konnect/assets/skills/konnect/SKILL.md`).

The researcher brief (`.orchestrator/handoffs/photo-to-kicad-reverse/02-researcher.md`) establishes the hard facts this design is built against:
- `retrace scan <img> --format json -o <dir>` writes exactly one file per format; JSON mode writes `<dir>/analysis.json` = `{version, timestamp, image, components:[...], traces:[...], pattern_matches:[...], summary:{...}}`. `board_dimensions`/`layer_count_estimate` are never written — a Konnect wrapper needing board px size must read the source image itself.
- `retrace` writes three separate global JSON stores under `Path.home()` on every `scan`/`solve`/`cross-board` run, with **no env-var override** — only `HOME`/`USERPROFILE` redirection at the OS-process level can scope this.
- KiCad has no netlist-import path into eeschema in any version (`sch export netlist` is export-only; `pcb`/`sch` subcommands have no `import netlist`). `export-kicad`'s `.net` uses synthetic, arrival-order pin numbers — never a real pinout.
- Konnect's own copper-mutation primitives (`route_trace`, `add_via`, `add_zone`, `set_component_placements`) already exist and are sufficient for a later PCB slice; this change does not touch them.
- No real-photo fixtures exist anywhere in `retrace` (CC BY-NC-SA teardown photos are not redistributable); `docs/examples/xbox_board.png`/`cisco_board.png` are synthetic PCB renders, plumbing-only.

Two further facts were measured on the target platform (Windows 11, Python 3.12, `pip install git+https://github.com/ericrihm/retrace.git` into `Konnect/.venv-retrace`, retrace 0.3.0, opencv-python 5.0.0) and supersede the researcher brief's inferences where they disagree:
- The base install works on Windows. A contour-only `scan` of an 800x600 synthetic image completed in **0.5 s** and wrote exactly `out/analysis.json`. The captured output is checked in at `openspec/changes/photo-to-kicad-reverse/fixtures/analysis-windows-smoke.json` and is the schema this design targets.
- `analysis.json` writes **empty strings, not `null`**, for `marking`/`part_number`/`datasheet_url`/`value`/`package` on the contour path; `pattern_matches[]` entries are `{pattern_name, description, component_roles:{role:component_id}, score, is_partial}`; `summary` is `{image, components, components_by_type, traces, identified, pattern_matches, duration_seconds}`. Fallback is announced on **stderr** as `WARNING YOLO not available — using contour-based fallback` and `WARNING easyocr is not installed — chip marking OCR is disabled.`. `HOME`+`USERPROFILE` redirection was verified to relocate retrace's stores to `<scoped>/.local/share/retrace/`.

## Goals / Non-Goals

**Goals:**
- Wrap `retrace` as a Konnect-native subprocess tool (`photo_intake` toolset) following the `cli.rs`/`integration.rs` pattern: typed results, a capability probe, graceful absence handling.
- Produce a persisted, human-editable review map as the *only* bridge between vision output and any KiCad mutation, with a hard approval gate enforced by the tools themselves (not just by agent instruction).
- Hand an approved map to schematic build using Konnect's own `sch_batch`/`sch_wiring` tools and real library symbol pin maps — never `retrace`'s synthetic netlist.
- Ship a new skill (`kicad-photo-intake`) and agent (`pcb-photo-intake-agent`) that pass every mechanical check in `crates/konnect/tests/asset_references.rs`.

**Non-Goals (this change):**
- Trace extraction, scale calibration math, net inference (`retrace solve`), or any PCB placement/routing — Slice 2, a later change.
- Multi-photo (top+bottom) fusion or angled-photo identification — Slice 3, a later change.
- Any change to existing toolsets, tool schemas, or agent behavior outside the new additions.
- Making `retrace` a hard dependency of Konnect, or shipping it. `check_retrace` reports absence as a fact.

## Decisions

### D1. Module layout

New file `crates/konnect-core/src/tools/photo_intake.rs`, mirroring `cli.rs`'s shape:
- `pub fn tools() -> Vec<ToolDef>` — the 5 tools in D2.
- Typed result structs (`Serialize`/`Deserialize`) mirroring the captured `analysis.json`:
  - `RetraceAnalysis { version: Option<String>, timestamp: Option<String>, image: Option<String>, components: Vec<RetraceComponent>, traces: Vec<RetraceTrace>, pattern_matches: Vec<RetracePatternMatch>, summary: Value }`
  - `RetraceComponent { id: String, label: Option<String>, confidence: Option<f64>, bbox: [i64; 4], marking, part_number, datasheet_url, value, package }` — the last five are `Option<String>`.
  - `RetraceTrace { id: String, points: Vec<[i64; 2]>, width_px: Option<f64>, from_component: Option<String>, to_component: Option<String> }`
  - `RetracePatternMatch { pattern_name: String, description: Option<String>, component_roles: std::collections::BTreeMap<String, String>, score: Option<f64>, is_partial: Option<bool> }` — the verified shape, not an untyped `Value`.
- **Empty-string normalization is mandatory.** retrace emits `""`, not `null`, for absent `marking`/`part_number`/`datasheet_url`/`value`/`package` (verified fixture). A plain `Option<String>` deserializes `""` to `Some("")`, so every one of those five fields carries `#[serde(default, deserialize_with = "empty_string_as_none")]`, a single local helper that maps `Some("")` to `None`. Without it, "retrace read no marking" and "retrace read a marking" become indistinguishable downstream, and the review map would carry a component whose `value` is the empty string rather than a missing value the reviewer must fill in — a silent-guess of exactly the kind this change exists to prevent.
- A subprocess-runner section: `async fn run_retrace(python: &Path, args: &[&str], scoped_home: &Path, timeout: Duration) -> Result<std::process::Output, RetraceRunError>`, always setting `HOME` **and** `USERPROFILE` via `.env(...)` on the `tokio::process::Command`, always `kill_on_drop(true)` (the `run_java_command` precedent, `integration.rs:1701`), and always capturing both streams. Timeout per D12.
- `cli_failure_diagnostics` (`cli.rs:25`) is a **private** `fn` in `cli.rs` and `command_output` (`integration.rs:1494`) is private to `integration.rs`. `photo_intake.rs` therefore defines its own three-line equivalent rather than promoting either to `pub(crate)`: two private helpers with the same shape already coexist in this codebase, and widening a module's surface to save three lines trades a real API boundary for a cosmetic one.
- Review-map persistence: `<project_dir>/.konnect/photo_intake/<map_id>/review_map.json`, matching the project-scoped-data convention `config.rs` already uses (`project_config_path`, `config.rs:96`, = `<project_dir>/.konnect/project.json`).

### D2. Toolset name and tool list

Register a new toolset `"photo_intake"` in `ALL_TOOLSETS` (`registry.rs:22`, category `"integration"`, **`tool_count: 5`**) and in `build_tools_for` (`registry.rs:172`) as `"photo_intake" => Some(photo_intake::tools())`. `ToolsetMeta.tool_count` (`router/mod.rs:20`) is hand-written and asserted against reality by `registry_tool_counts_match_reality` (`router/mod.rs:335`); `MAX_TOOLS_PER_TOOLSET` is 20 (`router/mod.rs:331`) and five tools is well under it.

Kept as its own toolset rather than folded into `integration` because its capability probe, config keys, and review-map lifecycle are a distinct concern with its own hard gate, and `integration` is already a mixed bag (JLCPCB + datasheets + Freerouting).

| Tool | Input (required in **bold**) | Output (key fields) |
|---|---|---|
| `check_retrace` | `python_path` (opt.), `project_dir` (opt. — the project whose config supplies `photo_intake.retrace_python_path`; pass the same `project_dir` you will pass to `scan_pcb_photo`, or the two calls may resolve different interpreters) | `available`, `python_path`, `retrace_version`, `extras` (`detection`, `ocr`), `candidates_tried`, `note` |
| `scan_pcb_photo` | **`image_path`**, **`project_dir`**, `python_path` (opt.), `timeout_seconds` (opt.) | `map_id`, `components`, `traces`, `pattern_matches`, `analysis_json_path`, `duration_seconds`, `used_fallback`, `fallback_evidence`, `python_path`, `candidates_tried` — the last two spelled exactly as `check_retrace` spells them, so a scan run by an interpreter the caller did not name is visible in the scan's own result, not only in a separate probe call |
| `save_photo_review_map` | **`project_dir`**, **`map`** (full review-map JSON; `review_map_schema()` spells out every D8 field's nested `properties` and sets `"additionalProperties": true` on the map object, each component object, and each net object, so out-of-schema keys are accepted rather than rejected by the compiled schema — see D5/D8) | `map_id`, `saved_path`, `approved` (always `false` unless D5's hash matches) |
| `load_photo_review_map` | **`project_dir`**, **`map_id`** | the full persisted review map, plus a server-computed `approval_valid` boolean (D5/D16) |
| `approve_photo_review_map` | **`project_dir`**, **`map_id`** | `approved: true`, `approved_at`, `content_hash_at_approval` |

The planner's `output_dir` argument on `scan_pcb_photo` is **removed** — see D13.

All five tools are declared with the `tool!` macro exactly as it exists at `crates/konnect-core/src/tools/mod.rs:425`: **four arguments** — `tool!(name, description, schema_json, handler)` — expanding to `ToolDef::new(...)`. The macro has no board-access parameter. `BoardAccess::None` (`mod.rs:79-82`) is the `#[default]` variant and is the correct one here: none of these five tools touch a live KiCad board, and the gate they enforce is entirely file-based. `ToolDef::with_board_access` (`mod.rs:115`) is therefore **not called** on any of them. *Rejected alternative:* writing `.with_board_access(BoardAccess::None)` explicitly for documentation value — rejected because it is a no-op that reads as a per-tool decision, inviting a later reader to think the variant was chosen tool-by-tool; the toolset's board-independence is stated once, here and in the module doc-comment. *Trade-off:* a future tool in this toolset that genuinely needs a live board will not stand out by contrast — accepted, because such a tool belongs in `pcb_components`/`sch_wiring`, not here.

Every handler has the house signature `async fn handle_x(args: &serde_json::Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult>`, returning `CallToolResult::json(&json!({...}))` on success (`integration.rs:1737`) and `CallToolResult::error("...")` on failure (`integration.rs:1795`).

### D3. Config keys and how Rust reads them

Extend `default_user_config()` (`config.rs:16`) with a new top-level object:
```json
"photo_intake": {
  "retrace_python_path": null,
  "retrace_timeout_seconds": 120
}
```
Precedence for every key: explicit tool argument > project config > user config > built-in default — the same order `deep_merge` (`config.rs:117`) already produces for every other key, and the same "argument overrides config" rule `find_freerouting_jar` (`integration.rs:1483`) applies to `jar_path`.

**`config.rs` exposes only `pub fn tools()` (`config.rs:193`).** `load_user_config`/`load_project_config` are MCP *tools*, not Rust functions — no other module in the crate reads configuration today, so there is nothing for `photo_intake.rs` to call. This change therefore adds one function to `config.rs`:
```rust
pub(crate) async fn effective_config(project_dir: Option<&Path>) -> serde_json::Value
```
built from the existing private `read_config`/`default_user_config`/`default_project_config`/`deep_merge` pieces, with no new file format; the only behavior change to an existing tool is `handle_get_effective_config`, which now layers built-in defaults via the same shared `layer_configs` path (below) — a strict narrowing of what it already returned, not a new disagreement. *Rejected alternative:* leave `config.rs` untouched and require the agent to call `load_user_config` and pass `python_path` on every `scan_pcb_photo` call — rejected because it turns a persisted user preference into a per-call obligation of prose; the first agent that forgets it silently falls through to PATH discovery and may run a different interpreter than the one the user configured, and the failure is invisible in the result. *Trade-off:* a second reader of the config document now exists, so a future change to config layout has two call sites instead of one — accepted, and bounded by keeping the accessor untyped (`Value`) so layout changes do not ripple into a struct.

No new `ServerConfig` field (`mod.rs:352`): that struct holds process-wide startup config (`kicad_cli`, `ipc_address`, `project_dir`, …); retrace's path is a user preference, like `fab_constraints`.

**`check_retrace` and `scan_pcb_photo` resolve the project the same way.** `config_project_dir(argument: Option<&Path>, config: &ServerConfig) -> Option<PathBuf>` returns the tool's own `project_dir` argument when given, else `ctx.config.project_dir` (the server's configured project); both handlers call it before reading `photo_intake.*` through `effective_config`, so the two calls can no longer resolve two different interpreters for what the caller believes is one configured preference. `handle_get_effective_config` (the `config` toolset's own inspection tool) shares the same `layer_configs(default, user, project)` helper `effective_config` calls internally, so the configuration a user inspects through that tool is always the one every reader of `effective_config` — including `photo_intake` — actually acts on.

### D4. Error / diagnostic contract

Mirrors `handle_check_freerouting`'s layered-boundary shape (`integration.rs:1710`: jar found? → java runs? → version checked? → bridge probed?) rather than a single pass/fail:
- `check_retrace` returns a *successful* `CallToolResult` with `available: false` for retrace absence or a failed `import retrace` probe — matching the "missing engine is a reported fact, not a tool failure" convention — and an error result only when an argument fails D13's path validation. Probe stdout/stderr is included when the `import retrace` probe exits non-zero. Both `check_retrace` and `scan_pcb_photo` resolve the project the same way (D3's `config_project_dir`) and report the same `python_path`/`candidates_tried` diagnostic shape, so a scan cannot be run by an interpreter the caller could not have predicted from a prior `check_retrace` call.
- `scan_pcb_photo` returns `CallToolResult::error` when no interpreter with `retrace` resolves (spec: "retrace is unavailable"), when the subprocess times out, when it exits non-zero, or when `analysis.json` is absent or unparseable after a zero exit. A zero exit is necessary but not sufficient — the same lesson `verify_nonempty_file` (`cli.rs:357`) encodes for kicad-cli.
- `save_photo_review_map`/`load_photo_review_map`/`approve_photo_review_map` return `CallToolResult::error` on schema-invalid input (the compiled `jsonschema::Validator` attached by `ToolDef::new`, `mod.rs:104`), on a path-validation failure (D13), or on a missing map file — never a silently empty or defaulted map.

### D5. Approval-state machine

`approve_photo_review_map` is the only tool that can set `approved: true`. It records `approved_at` (RFC 3339 UTC) and `content_hash_at_approval` (D16). `save_photo_review_map` computes the same hash over the incoming map and writes `approved: true` **only** when that hash equals the stored `content_hash_at_approval` of the map already on disk; in every other case it writes `approved: false`. A client-supplied `approved` field in the payload is ignored and overwritten — never read.

`save_photo_review_map`'s schema (D2/D8) is open (`additionalProperties: true` on the map, each component, and each net), and the handler layers its D8-normalized value over the caller's incoming object rather than serializing the normalized value alone — so a key the schema does not name (a hand-added reviewer annotation, for instance) survives a save unchanged instead of being silently dropped. `save_photo_review_map` also rejects every client-supplied value for the five reserved bookkeeping keys — `approved`, `approved_at`, `content_hash_at_approval`, `saved_at`, and `approval_valid` — by name before persisting, deriving each one only from the record already on disk (never from the incoming `map`), so a caller cannot smuggle a forged `approval_valid` into the stored map that disagrees with the server-computed one in the response. The content hash (D16) is computed over the D8-normalized, schema-known fields of `source_images`/`scale_reference`/`components`/`nets` only — never the raw subtrees the caller sent — so a key not named by D8, nested anywhere inside a component, a net, or `scale_reference`, is outside the hash and cannot revoke an approval by being added or edited later, while editing any D8-named field (`type`, `value`, `ref`, `confidence`, `bbox_px`, `footprint_suggestion`, a net's `connections`/`source`, `scale_reference.kind`/`.value`, …) always moves the hash.

Schematic build (D6) recomputes the hash over the map it loaded and compares it to the stored `content_hash_at_approval` before consuming anything, so a map cannot be approved, edited on disk, and then consumed as if still approved, even if every agent in the chain skips re-reading it.

### D6. Schematic-build handoff

Not a new tool — a documented calling convention in the new skill/agent, executed by `kicad-schematic-build-agent` with its existing toolsets:
1. `pcb-photo-intake-agent` calls `load_photo_review_map` and confirms `approved: true`.
2. It writes a plain handoff note (the map's `saved_path`, component count, net count) and delegates, per the existing "one design owner at a time" rule (`konnect` SKILL.md, Agent Routing).
3. `kicad-schematic-build-agent` loads the `photo_intake` toolset for this one read, calls `load_photo_review_map` itself, and re-verifies `approved: true` **and** the D16 hash match (defense in depth — it never trusts the caller's claim). It then places one symbol per `approved: true` component, matched by `type`/`value` against real library search exactly as it already does for a from-scratch build, and wires nets from the map's `ref`-to-`ref` entries with `sch_wiring`. It runs `run_erc` and reports the result, per its existing evidence contract.
4. `kicad-schematic-build-agent.md` gains one short new section, "Building from an approved photo-intake map", that reads `approval_valid` from `load_photo_review_map` before placing or wiring anything sourced from a photo-intake map, and refuses to proceed when it is false; everything else in the file is unchanged. `agents_make_claimed_evidence_executable` (`asset_references.rs:175`) and `skills_define_the_same_evidence_boundary_as_their_agents` (`asset_references.rs:223`) are hard-coded case lists naming only `kicad-schematic-build-agent`, `kicad-design-review-agent`, `kicad-schematic/SKILL.md` and `kicad-review/SKILL.md`; the new agent and skill are not enrolled in either, so no marker set is imposed on them and no existing file needs editing to satisfy them.

### D7. Test image and parsing fixtures

Two separate needs, two separate answers.

**Parsing fixture (no Python required).** `crates/konnect-core/tests/fixtures/photo_intake/analysis.json` is a verbatim copy of the real Windows capture at `openspec/changes/photo-to-kicad-reverse/fixtures/analysis-windows-smoke.json` — four components, two `pattern_matches`, empty `traces`, `""` (not `null`) for every absent string field. A captured artifact beats a hand-written one precisely because the hand-written version would have used `null` and the empty-string trap in D1 would have shipped undetected.

**Synthetic image (for the `#[ignore]`d live test).** Add `image.workspace = true` to `crates/konnect-core`'s `[dev-dependencies]` and draw the test board with it. Evidence: `image = { version = "0.25", default-features = false, features = ["png"] }` is already declared in the root workspace `Cargo.toml:90`, and `cargo tree -p konnect-core -i image` resolves as `image v0.25.10 <- konnect-render <- konnect-core` — the crate is already compiled, with these exact features, in every build of this workspace. The dev-dependency costs zero new crates, zero new compile time, and no new supply-chain surface. *Rejected alternative:* hand-writing a minimal PNG byte array (stored-mode zlib IDAT plus CRC-32 tables) — rejected because roughly sixty lines of untested CRC/adler arithmetic would sit between the test and its subject, and any bug in it makes a retrace failure indistinguishable from a fixture failure. *Trade-off:* `konnect-core`'s test build now names a crate its library does not — accepted, because the crate is in the graph either way and a dev-dependency is the honest declaration of that.

**Live-path gating.** Tests that actually spawn `retrace` are `#[ignore = "requires Python with retrace installed (set RETRACE_PYTHON)"]` and read `std::env::var_os("RETRACE_PYTHON")` with `.expect(...)` — byte-for-byte the pattern `freerouting_mcp.rs:901`/`:903` uses for `FREEROUTING_JAR`. A default `cargo test -p konnect-core` therefore never needs a Python interpreter; `cargo test -p konnect-core photo_intake:: -- --ignored` runs the live path and fails loudly if the env var is unset rather than skipping silently.

### D8. Review-map JSON schema (canonical form)

```json
{
  "map_id": "uuid-v4, server-assigned",
  "saved_at": "RFC 3339 UTC, rewritten on every save",
  "source_images": ["C:/abs/path/to/top.png"],
  "scale_reference": { "kind": "board_edge_mm | package", "value": "e.g. '50' or '0805'" },
  "components": [
    {
      "component_id": "retrace id, e.g. 'C0000', traceable back to analysis.json",
      "ref": "R1 (assigned during review; null pre-review)",
      "type": "resistor | capacitor | ic | connector | ...",
      "value": "string or null",
      "footprint_suggestion": "string or null",
      "confidence": 0.5,
      "bbox_px": [295, 195, 51, 31],
      "approved": false
    }
  ],
  "nets": [
    { "connections": ["R1.1", "C2.2"], "source": "traced | inferred | manual" }
  ],
  "subcircuit_hints": [
    {
      "pattern_name": "pull_up_resistor",
      "description": "Pull-up resistor from VCC to a signal line",
      "component_roles": { "resistor": "C0000" },
      "score": 0.7,
      "is_partial": false
    }
  ],
  "approved": false,
  "approved_at": null,
  "content_hash_at_approval": null
}
```
`bbox_px` is `[x, y, w, h]` in source-image pixels, copied verbatim from retrace's `bbox`. `saved_at` and `subcircuit_hints` are outside the content hash (D16). `subcircuit_hints` is optional and may be absent entirely.

`review_map_schema()` declares every field shown above as a named `properties` entry and sets `"additionalProperties": true` on the map object, each component object, and each net object, rather than leaving them at the `close_input_schema` default of `false`. A key not shown above — a hand-added reviewer note, for example — therefore passes the compiled validator and is preserved verbatim on save (D5), not rejected at the schema boundary and not silently dropped by round-tripping through the typed struct. The five reserved bookkeeping keys (`approved`, `approved_at`, `content_hash_at_approval`, `saved_at`, `approval_valid`) are the one exception: `save_photo_review_map` strips any client-supplied value for these by name and derives them only from the on-disk record (D5), so a caller cannot write a second, forged `approval_valid` into the persisted map. The content hash (D16) walks the D8-normalized form of `source_images`/`scale_reference`/`components`/`nets` — exactly the fields named above, not the raw JSON subtree a caller sent — so an annotation key added anywhere inside those four does not move the hash, while editing any field named above always does.

### D9. New skill and agent

- `crates/konnect/assets/skills/kicad-photo-intake/SKILL.md` — triggers on "photo of a board", "reverse engineer this PCB", "identify components from a photo". Documents `load_toolset('photo_intake')`, the `check_retrace` → `scan_pcb_photo` → build-map → `save_photo_review_map` → user review → `approve_photo_review_map` → handoff flow; the hard gate in prose matching D5's tool-enforced behavior; confidence flagging below 0.6; that the scale reference is always user-supplied; and that `subcircuit_hints` are advisory and never evidence.
- `crates/konnect/assets/skills/kicad-photo-intake/references/review-map-schema.md` — D8's schema field by field, named from `SKILL.md` (required by `every_reference_is_reachable_from_its_parent_skill`, `asset_references.rs:45`).
- `crates/konnect/assets/agents/pcb-photo-intake-agent.md` — frontmatter matching the house shape (`kicad-pcb-layout-agent.md:1-11`): `name`, `description` with triggers, `model: sonnet`, `skills: [konnect, kicad-photo-intake]`, `tools: [mcp__konnect__*]`, `maxTurns`. Scope: photo → reviewed map only; hands off to `kicad-schematic-build-agent`; never calls a schematic- or board-mutating tool itself.
- Update `crates/konnect/assets/skills/konnect/SKILL.md`'s "Agent Routing and Mutation Ownership" section with a bullet naming `pcb-photo-intake-agent` — required by `top_level_skill_routes_every_bundled_agent` (`asset_references.rs:128`), which reads `assets/agents/` and fails if any file stem is absent from that one file's text.
- Update `crates/konnect/src/manifest.rs` with a skill entry (content + one reference) and an agent entry, following the `include_str!` pattern at `manifest.rs:58-86` and `manifest.rs:137-138`.

### D10. Asset-guard implications

`crates/konnect/tests/asset_references.rs` enforces the following against the new assets:
- `every_reference_is_reachable_from_its_parent_skill` (`:45`) → `review-map-schema.md` is named in `kicad-photo-intake/SKILL.md`.
- `agents_preload_existing_skills` (`:81`) → the agent's `skills:` list contains only `konnect` and `kicad-photo-intake`.
- `top_level_skill_routes_every_bundled_agent` (`:128`) → `konnect/SKILL.md` contains the string `pcb-photo-intake-agent`.
- `documented_toolsets_exist_in_the_registry` (`:396`) / `tools_listed_beside_a_toolset_belong_to_it` (`:427`) → every `load_toolset('photo_intake')` example lists only D2's five tool names.
- `call_examples_name_real_parameters` (`:484`) → every `tool(arg, arg)`-shaped example uses D2's real property names and includes every required one.
- **`backticked_tool_names_in_prose_exist_in_the_registry` (`:632`) is the one that will actually fail first.** Its `snake_words` helper (`:770`) flags *every* two-part snake_case word in *every* asset file, backticked or bare, unless the word is a registered tool name, a **top-level** input-schema property of some tool, or listed in the test's `NOT_TOOLS` array. Nested schema properties are not collected. So `project_dir`, `python_path`, `image_path`, `map_id` and `timeout_seconds` pass automatically (top-level properties of the new tools, and `project_dir` is already in `NOT_TOOLS`), while every response and review-map field the skill must name — `used_fallback`, `fallback_evidence`, `retrace_version`, `analysis_json_path`, `duration_seconds`, `pattern_matches`, `subcircuit_hints`, `component_id`, `bbox_px`, `footprint_suggestion`, `source_images`, `scale_reference`, `approved_at`, `content_hash_at_approval`, `saved_at`, `part_number`, `retrace_python_path`, `retrace_timeout_seconds` — does not. Decision: add these to `NOT_TOOLS` as one commented block ("Structured photo-intake response and review-map fields, not callable tools"), which is exactly how the array already carries `files_generated`, `ownership_status`, `sheet_instance_path` and friends. *Rejected alternative:* writing the field names hyphenated or in prose ("the used-fallback flag") to dodge the matcher — rejected because `snake_words` catches bare occurrences too, and a schema reference whose field names cannot be copy-pasted is not a schema reference.

## Decisions closed by @architect

### D11. `used_fallback` is derived from the subprocess's own stderr, with the extras probe as a conservative backstop

`analysis.json` does not record which detector ran. Two signals exist: the `check_retrace` extras probe (can the ML path run?) and retrace's stderr, which states what did run — verified literals `WARNING YOLO not available — using contour-based fallback` and `WARNING easyocr is not installed — chip marking OCR is disabled.`.

Decision: `scan_pcb_photo` captures stderr from its own invocation and reports
```
used_fallback      = yolo_warning_seen || ocr_warning_seen || !extras.detection || !extras.ocr
fallback_evidence  = { yolo_warning_seen, ocr_warning_seen, extras_detection, extras_ocr }
```
where `extras.*` come from an in-call probe reusing `check_retrace`'s logic.

Argument: the probe answers a capability question and the stderr answers a history question, and only the second is what the reviewer needs. retrace can import `ultralytics` and still fall back — a model file missing, a CUDA init failure — and a probe-only derivation would then report `used_fallback: false` over a result produced with no detection and no OCR at all. The `||` makes stderr the primary signal and the probe the backstop: if a future retrace release rewords or silences the warning, the expression degrades to the probe's answer and errs toward `true` rather than silently reporting a false negative.

Trade-off: the stderr literal is an unversioned contract, so a reword turns a precise signal into a coarse one without any test failing. Accepted, and bounded — `fallback_evidence` publishes both raw signals, so a disagreement between them is visible in the tool's own output rather than collapsed into one bit. The `#[ignore]`d live test (task 2.4) asserts the marker is still found.

*Rejected alternative:* drop `used_fallback` entirely and let the agent call `check_retrace` separately. Rejected because the spec scenario "scan runs without ML extras installed" requires the *scan response itself* to flag that marking/value/part_number were not attempted. Without it, an agent cannot distinguish "retrace looked and found no marking" from "retrace never looked" — and an empty `value` then reads as a finding instead of a gap, which is the exact silent-guess failure this change exists to prevent.

### D12. Timeout: 120 s default, configurable, per-call override, always `kill_on_drop`

`scan_pcb_photo` runs with `tokio::time::timeout`, duration resolved as: `timeout_seconds` argument > `photo_intake.retrace_timeout_seconds` config > **120**. Clamped to `5..=1800`. Probe subprocesses (`check_retrace` and the in-call probe) use a fixed **10 s**, matching `run_java_command` (`integration.rs:1702`). Every `Command` sets `kill_on_drop(true)`.

Argument: the measured contour-only scan is 0.5 s and the Slice 0 success metric is "<30 s on a known sample image", so 120 s is roughly 4x headroom over the target the change is judged by, while staying well below `LONG_TIMEOUT`'s 600 s (`cli.rs:23`), which exists for whole-board KiCad renders rather than a single-image CV pass. It is configurable rather than constant because the one legitimate long case — a first EasyOCR run downloading model weights over a slow link — is a property of the user's machine, not of the tool.

`kill_on_drop(true)` is not decoration here: retrace writes to the scoped `HOME` throughout its run, so a timed-out process that survives the call keeps writing into a directory the caller believes is finished, and on Windows keeps a handle that blocks cleanup of the scoped directory.

Trade-off: a killed scan leaves a partial or absent `analysis.json` under the map directory. `scan_pcb_photo` therefore reports the timeout as an error naming the elapsed limit and the directory, and never parses whatever is there — a truncated file must not become a half-populated component list.

*Rejected alternative:* reuse `LONG_TIMEOUT` (600 s) as-is for consistency with `cli.rs`. Rejected because a wedged interpreter would then block the MCP call for ten minutes with no output and no way for the agent to distinguish "still working" from "hung"; at 20x the Slice 0 metric it would essentially never fire on the failure it exists to catch.

### D13. Path safety: canonicalize the project, tokenize the map id, and delete the free-form output path

`get_path` (`mod.rs:868`) performs no validation — it is `PathBuf::from(str)` with a missing-argument error — and no path-confinement convention exists in `konnect-core` to inherit. `photo_intake.rs` therefore owns its own, in five rules:

1. **`project_dir`** — `std::fs::canonicalize` must succeed and the result must be a directory. A non-existent or file path is a `CallToolResult::error` naming the path. Canonicalizing first (the `absolutize` rationale, `config_resolution.rs:118-123`) means the confinement check below compares real paths, symlinks resolved.
2. **`map_id`** — a validated token: 1..=64 characters, each `[A-Za-z0-9_-]`. Enforced twice: as `"pattern": "^[A-Za-z0-9_-]{1,64}$"` in the JSON Schema of `load_photo_review_map`/`approve_photo_review_map`, **and** in Rust before any path join. Both, because `save_photo_review_map` takes the whole `map` object and the schema validator's top-level pattern does not reach `map.map_id`. The token rule rejects `/`, `\`, `.`, `..`, `:` and NUL by construction rather than by blacklist.
3. **Server-assigned ids** — a new map's `map_id` is `uuid::Uuid::new_v4()` (`uuid` is already a `konnect-core` dependency), produced by `scan_pcb_photo`. A client-supplied `map_id` is honored only when it passes rule 2 *and* its directory already exists, so the tools never mint a caller-chosen directory name.
4. **Output location is computed, never supplied** — always `<canonical project_dir>/.konnect/photo_intake/<map_id>/`, created with `create_dir_all`. After creation, the directory is canonicalized again and asserted to `starts_with` the canonical `project_dir`; failing that is an error. The re-check is two lines and catches the one case rules 1-3 cannot: a pre-existing `.konnect` or `photo_intake` that is a symlink to somewhere else.
5. **`image_path`** — canonicalized, must be an existing file, and deliberately **not** confined to `project_dir`: a photo legitimately lives in the user's Pictures folder. It is opened read-only and its canonical path is what gets recorded in `source_images`.
6. **Rendering a validated path for a subprocess argument or a JSON response must not reopen the confinement question it already closed.** `subprocess_arg` strips only the `\\?\` `VerbatimDisk` form (`\\?\C:\boards\top.png` → `C:\boards\top.png`); it rewrites `\\?\UNC\srv\share\proj\top.png` to the valid UNC root `\\srv\share\proj\top.png`, never to the bare, now-relative `UNC\srv\share\proj\top.png` an unconditional prefix strip produces; and it leaves every other verbatim form — a volume GUID path (`\\?\Volume{GUID}\...`), a device path — untouched rather than mangled. `analysis_json_path`, `saved_path`, and every other path handed back to a caller or into `retrace`'s own argv are rendered through this function.

*Rejected alternative:* keep the planner's optional `output_dir` argument and validate it with `starts_with(project_dir)`. Rejected because it buys the caller nothing the per-`map_id` subdirectory does not already provide, while creating a second path that must stay confined forever — and a confinement check is a thing that can be got wrong, whereas an argument that does not exist cannot be. Removing the argument removes the class of bug instead of guarding it.

Trade-off: a caller who wants the raw `analysis.json` somewhere specific must copy it out afterwards. Accepted: the spec requires the scan output to be project-scoped retrievable evidence, and `analysis_json_path` in the response tells them exactly where it is.

### D14. `pattern_matches` is surfaced, read-only, in both the scan result and the review map

`scan_pcb_photo` returns `pattern_matches` as parsed `RetracePatternMatch` values (D1), and the review map gains an **optional** `subcircuit_hints` array of the same shape (D8). Neither is read by any tool: no component is created, approved, typed, or valued from a pattern match, and no net is inferred from one.

Argument: the matcher is the only part of retrace's output that carries circuit-level meaning a human can check at a glance, and the reference board for this change is an LDO module — precisely the shape its canonical-subcircuit table is built to recognize. The verified capture already produced two `pull_up_resistor` matches with `score` and `component_roles` on a four-component synthetic image. Withholding that from the reviewer makes them slower without making them safer, because the raw `analysis.json` is persisted as evidence regardless (spec: "retrievable evidence, not silently discarded") — the only question is whether they read it with the tool's framing or without it.

It is safe to surface precisely because it is inert: the gate is per-component `approved: true` set by a human, and a hint cannot set it.

Trade-off: anchoring. A reviewer told "pull_up_resistor, score 0.7" may accept a resistor they would otherwise have questioned. Mitigated three ways: `score` and `is_partial` are carried verbatim rather than thresholded into a verdict; `subcircuit_hints` sits outside the content hash (D16) so it can never revoke or confer approval; and `review-map-schema.md` states in the field's own entry that hints are advisory and never evidence.

*Rejected alternative:* omit `pattern_matches` from the typed result and keep the response minimal. Rejected because the data reaches the reviewer either way through the persisted `analysis.json`; omitting it from the typed surface only guarantees they meet it raw, without `score`/`is_partial` framing and without the "never evidence" note.

### D15. Interpreter discovery order, and `python -m retrace` as the invocation

**Discovery** — first candidate that passes the import probe wins:
1. the `python_path` tool argument;
2. `photo_intake.retrace_python_path` from the merged config (D3);
3. the `RETRACE_PYTHON` environment variable (this is also what the `#[ignore]`d live tests read, mirroring `FREEROUTING_JAR` at `freerouting_mcp.rs:903`);
4. PATH discovery, in order: **`py -3`** (Windows only), then `python3`, then `python`.

The probe is `<candidate> -c "import retrace, sys; print(retrace.__version__)"`, 10 s, `kill_on_drop(true)`. `check_retrace` reports which candidate won in its `python_path` field, so the answer to "which interpreter did you use" is always in the result rather than inferred.

Argument for `py -3` first on Windows: the PEP 397 launcher is the only entry point that resolves a registered install without PATH mutation, and on Windows `python` is frequently the Microsoft Store alias stub, which exits non-zero and would otherwise consume a probe and can shadow a real install. `python3` before `python` elsewhere because `python` still means Python 2 on some systems.

**Invocation** — `<python> -m retrace scan <image> --format json -o <dir>`. Verified on this machine: `.venv-retrace/Lib/site-packages/retrace/__main__.py` exists and `<venv python> -m retrace --help` runs.

Argument: `-m` guarantees that the interpreter proved to have `retrace` importable is the interpreter that runs it. There is exactly one resolution step and it is the same one the probe performed.

*Rejected alternative:* the `retrace` console script (`.venv-retrace/Scripts/retrace.exe`, verified to exist). Rejected because its location is not derivable from the interpreter path across layouts — `pipx`, a user-site install and `pip install --target` all put the script somewhere the interpreter's directory does not predict, and conda uses `bin/` rather than `Scripts/` — so finding it is a *second, independent* resolution that can disagree with the probe. That is the "running a binary the user did not name and reporting its results as theirs" defect `resolve_cli_executable` documents at `cli.rs:275-281`.

Trade-off: `-m` pays interpreter startup and package import on every call, roughly 100-200 ms, where a console script pays the same cost anyway. Irrelevant against a 0.5 s floor.

### D16. Content hash: SHA-256 over a field-selected, canonically serialized subset

One function, `pub(crate) fn review_map_content_hash(map: &serde_json::Value) -> String`, in `photo_intake.rs`. Three callers: `save_photo_review_map`, `approve_photo_review_map`, and the schematic-build re-check (D6 step 3).

**Covered fields** — exactly the review content, in this order of definition: `source_images`, `scale_reference` (its D8 fields — `kind`, `value` — only), `components` (each element's D8 fields only — `component_id`, `ref`, `type`, `value`, `footprint_suggestion`, `confidence`, `bbox_px`, `approved` — in D8's order, array order as stored), `nets` (likewise, each element's `connections`/`source` only). Coverage is by D8-normalized field, not by whatever keys happen to be present on a given element — see Canonical serialization.

**Excluded** — `map_id` (identity, not content), `saved_at`, `approved`, `approved_at`, `content_hash_at_approval`, and `subcircuit_hints` (advisory and never consumed; a hint changing must not revoke a human's approval), plus any key not named by D8 anywhere inside `components`, `nets`, or `scale_reference` — a hand-added annotation is content a human added *to* the map, not a change *to* the reviewed content, and must not revoke an approval it did not exist to challenge.

**Canonical serialization** — build a fresh `serde_json::Map` containing only the covered top-level keys. `components`, `nets`, and `scale_reference` are taken from the D8-normalized value (`serde_json::to_value(&parsed)`, the same normalization `save` already computes for D5's overlay) — never cloned from the caller's raw `map` — so a key inside a component, a net, or `scale_reference` that D8 does not name never reaches the object that gets serialized here. `serde_json::to_vec(&covered)` follows. Object keys are emitted sorted at every depth, arrays keep stored order, there is no insignificant whitespace, and numbers use serde_json's shortest round-trip form. This relies on `serde_json`'s `preserve_order` feature being **off**, which makes `serde_json::Map` a `BTreeMap`; verified — `Cargo.toml:30` declares `serde_json = "1"` with no features and `preserve_order` appears nowhere in the workspace or lockfile. The implementation states that dependency in a comment beside the function, because turning `preserve_order` on later would silently change every stored hash and revoke every approval in the field.

**Algorithm** — SHA-256, lowercase hex, `format!("{:x}", Sha256::digest(bytes))` — the exact form `design_hash.rs:47`, `specctra.rs:1638` and `specctra_ses.rs:844` already produce, from the `sha2` crate that is already a `konnect-core` dependency.

**Float discipline** — `confidence` (and any numeric the reviewer edits) is hashed from the parsed `Value` exactly as loaded; no arithmetic, rounding, or re-parsing happens between load and hash on any of the three paths. Both `save` and `approve` hash the same in-memory object they are about to write or just read back, never a re-derived one.

Argument: the gate's meaning is "a human looked at this content and said yes", so the hash must cover content and nothing else. Every excluded field is either bookkeeping the tools themselves write (which would make every save revoke its own approval) or advisory data no consumer reads.

Trade-off: a field added to the review map in a future slice is silently outside the hash until someone adds it to the covered list, and an unhashed field is an unguarded one. Hardened by defining coverage as an explicit named list in one place rather than a denylist, so the omission is visible at the function, and by a unit test that asserts the covered-key list equals the schema's content keys.

*Rejected alternative:* hash the `review_map.json` file's raw bytes. Rejected because `save` writes pretty-printed JSON (the `write_config` convention, `config.rs:110`) and the spec explicitly supports the user hand-editing the file; a whitespace-only or key-reordering edit would then revoke approval, which trains the user to re-approve reflexively — and a gate people click through is worse than no gate, because it looks like one.

## Risks / Trade-offs

- **[Risk] `retrace` on Windows** → the base install is now verified working on the target machine (retrace 0.3.0, 0.5 s scan). The residual risk is the ML extras path, which remains unexercised. Mitigation: `check_retrace` reports extras separately from the base package, and `used_fallback` (D11) states which path actually ran.
- **[Risk] Global-state pollution from retrace's `Path.home()` stores** → every invocation redirects `HOME` and `USERPROFILE` to a per-call scoped directory; verified to relocate `cross_board.json`. A dedicated test asserts nothing appears under the real home.
- **[Risk] Approval-gate bypass by pasting map content into a schematic-build prompt** → D6 step 3's re-verification is defense in depth but is a prose contract for that path. The file-based path is closed by D5+D16; a caller who bypasses the agent entirely and hand-authors a build from unapproved data is outside this change's threat model, as it is for every other Konnect tool that trusts its arguments.
- **[Risk] `analysis.json` schema drift** → every non-identifier field is `Option<...>` and the empty-string normalizer (D1) absorbs the one representational quirk already observed, so an added or removed optional field degrades rather than breaks. The captured fixture pins the exact shape this change was built against, including `version: "0.3.0"`.
- **[Trade-off] `photo_intake` as its own toolset rather than folded into `integration`** → a slightly larger catalogue, in exchange for keeping the probe, config keys and hard gate cohesive and independently loadable, consistent with `sch_export` and `sch_wiring` already being split by concern.

## Migration Plan

Purely additive to behavior — no existing tool schema, config key, or agent file changes meaning. Three existing files gain content: `config.rs` (a new default key block and one `pub(crate)` accessor), `registry.rs` (one `ToolsetMeta` and one match arm), and `asset_references.rs`'s `NOT_TOOLS` array (one commented block of response-field names). Deploy the module, registry entry, config default, assets and manifest embedding in one PR. No data migration: no `photo_intake` state exists for any project until this ships, so a rollback orphans nothing.

## Open Questions

None blocks implementation. Two are deferred by owner and slice:

1. **Owner @architect, Slice 2** — whether `scale_reference` should be validated against the matched library footprint's real dimensions at approval time, to catch an obviously wrong user-supplied calibration before it reaches placement. Deferred because this change persists `scale_reference` without consuming it.
2. **Owner @architect, Slice 2** — whether `subcircuit_hints` should gain any consumer at all once net inference exists, or stay permanently inert. D14 makes it inert for this change; the question is whether that is a slice boundary or a permanent property.

### Analyst open questions resolved

1. **Integration surface** — Rust subprocess wrapper, new `photo_intake` toolset (D1, D2).
2. **Prerequisite handling** — `check_retrace` probe plus three `photo_intake.*` config keys, all defaulting to unset or a safe default (D3, D4, D12, D15).
3. **Scale calibration input** — `scale_reference` is a required review-map field, always user-supplied (D8); persisted here, consumed in Slice 2.
4. **Multi-photo strategy** — out of scope; `source_images` is a list for forward compatibility but this change writes exactly one entry.
5. **Mandatory human-review gate** — tool-enforced content-hash state machine (D5, D16), not an agent instruction.
6. **Accuracy expectations / synthetic pin numbers** — schematic build resolves pins via real library symbol lookup by `type`/`value` (D6); retrace's `.net`/`.kicad_pcb`/`.kicad_sch` output is never read by any Konnect tool, and only `analysis.json` is parsed (D1).

## Pre-mortem

Six months out, this shipped and failed. Here is how, and what in the design above was changed to make each cause survivable.

1. **Every approval in the field was silently revoked by a dependency bump.** Someone enabled `serde_json`'s `preserve_order` for an unrelated reason. `serde_json::Map` became an insertion-ordered `IndexMap`, canonical serialization stopped sorting keys, every stored `content_hash_at_approval` stopped matching, and every project's approved map went back to unapproved with no error anyone could read. **Hardening (D16):** the dependency on `preserve_order` being off is verified in the design and restated in a comment beside `review_map_content_hash`, and coverage is an explicit named key list rather than "whatever the object holds" — so the canonicalizer's assumption is a thing a reviewer of that PR can see, not folklore. A unit test hashes a fixture map to a pinned literal digest, which fails the instant serialization order changes.

2. **A board came back with components whose value was the empty string, and nobody noticed they were never read.** retrace's `""`-for-absent convention flowed through `Option<String>` as `Some("")`, the review map persisted `"value": ""`, the reviewer read a filled field, and the schematic got a symbol with a blank value nobody questioned. **Hardening (D1, D7):** empty-string normalization at the deserialization boundary is a named requirement, not a detail, and the parsing fixture is the *captured real* `analysis.json` rather than a hand-written one — a hand-written fixture would have used `null` and this failure would have shipped with a green test suite.

3. **`scan_pcb_photo` wrote outside the project, and nobody could say when it started.** A caller passed a crafted `map_id` or an `output_dir` climbing out of `.konnect/`, or a symlinked `.konnect` pointed at a shared drive, and scan artifacts landed somewhere no cleanup or rollback covered. **Hardening (D13):** the free-form `output_dir` argument is deleted rather than validated, `map_id` is a token validated both in schema and in Rust, ids are server-minted by default, and the computed directory is re-canonicalized and `starts_with`-checked against the canonical project root — which is the only one of those rules that catches the symlink case.

4. **A wedged Python process ate an agent session, then kept writing after it was given up for dead.** An EasyOCR first-run download stalled, the call blocked for ten minutes on an inherited 600 s timeout, and the abandoned process kept writing into a scoped home the caller believed was finished — on Windows holding a handle that blocked cleanup. **Hardening (D12):** a 120 s default sized against the Slice 0 metric rather than against KiCad renders, configurable for the one legitimate slow case, with `kill_on_drop(true)` on every `Command` and an explicit refusal to parse a truncated `analysis.json` after a timeout.

5. **Konnect ran a different Python than the user configured, and reported its results as theirs.** Discovery found a console script on PATH belonging to one environment while the probe had validated an interpreter in another, so `check_retrace` said "available, version 0.3.0" about an install that was not the one doing the scanning. **Hardening (D15):** invocation is `<python> -m retrace`, so exactly one resolution step exists and it is the one the probe performed; discovery order is fixed and documented; and `check_retrace` reports the winning `python_path` in its result, making "which interpreter" an answer rather than an inference.

6. **`cargo test -p konnect` went red on a machine with no Python, and the fix was to weaken the asset guards.** The new skill's prose named response fields that `backticked_tool_names_in_prose_exist_in_the_registry` had never heard of, the failure looked like a test being pedantic, and someone loosened `snake_words` or deleted the check instead of registering the names. **Hardening (D10):** the failure is predicted by name with its mechanism (top-level schema properties only; nested ones are not collected), the remedy is fixed in advance as a `NOT_TOOLS` block following the array's own existing precedent, and it is an explicit acceptance criterion on the asset-guard task — so the person who hits it finds it already diagnosed rather than discovering it as an obstacle.

7. **The whole thing was trusted more than it earned, because a hint looked like a finding.** `subcircuit_hints` said `pull_up_resistor` with score 0.7, a reviewer approved on that basis, and the resulting schematic carried a circuit nobody had actually verified against the board. **Hardening (D14):** hints are inert by construction — no tool reads them, they sit outside the content hash so they can neither confer nor revoke approval, `score` and `is_partial` are carried verbatim rather than collapsed into a verdict, and the schema reference states in the field's own entry that they are advisory and never evidence.
