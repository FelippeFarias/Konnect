---
change: photo-to-kicad-reverse
task: INTAKE requirements brief
agent: analyst
verdict: DONE
failing_layer: n/a
---

## Result

**Goal:** Add a Konnect-native "PCB photo -> KiCad replica" workflow that turns user-supplied board photos into a human-reviewed component/net map, then into a real Konnect schematic (and later PCB layout), using `retrace` as a wrapped MCP vision front-end.

### User stories (<=5)

1. As a hardware engineer, I send top-view photos of an unknown board (e.g. an LDO module) and get a component table (type, marking, value/part-number if legible, confidence). AC: low-confidence items are flagged, never silently guessed.
2. As a hardware engineer, I get a proposed net list derived from visible traces. AC: each connection is tagged traced-vs-inferred; nothing is built yet.
3. As a hardware engineer, nothing is built into a KiCad project until I approve/edit the map. AC: the component+net map persists as an editable JSON artifact between runs; no `create_project`/schematic-build call happens before explicit approval.
4. As a hardware engineer, my approved map becomes a real schematic. AC: `kicad-schematic-build-agent` produces a saved `.kicad_sch` with 1 symbol per approved component and nets matching the approved list; ERC run and reported.
5. (Later slice) As a hardware engineer, placement/routing visually approximates the photographed board. AC: footprints placed at photo-derived scaled positions; only user-confirmed nets are auto-routed; DRC run and reported.

### Scope slices (priority order, each with a measurable success metric)

- **Slice 0 (prereq, not user-facing):** wrap `retrace` as a new `photo_intake` toolset (or extend `integration`) exposing `scan_pcb_photo` (JSON out), following the existing kicad-cli/Freerouting subprocess pattern. Metric: `scan_pcb_photo` on a known sample image returns JSON matching retrace's Component/Trace schema in <30s, no crash when YOLO/OCR extras are absent.
- **Slice 1 (MVP):** one top photo -> confirmed component map -> Konnect schematic (no PCB). Metric: on a reference LDO test board (~5-8 components), all components identified (type+rough value) with confidence, user reviews/corrects, and the generated schematic has a 1:1 component-count match with the physical board and passes ERC clean.
- **Slice 2:** add trace extraction, scale calibration, and net inference (`retrace trace`/`solve`) plus PCB placement (no routing yet). Metric: on the same LDO board, the inferred net list matches a hand-built reference netlist >=90% exact (remainder flagged); footprints placed within the board outline at proportionally correct relative positions (visual review).
- **Slice 3:** multi-photo (top+bottom -> F.Cu/B.Cu), angled photos for identification only, plus routing of confirmed nets via the existing Freerouting integration. Metric: a two-layer reference board (e.g. simple buck converter) reconstructed with both copper layers populated correctly, routed and DRC-clean (zero unrouted/errors) after human-confirmed netlist.

### Open questions (recommended defaults — orchestrator decides, no user ask)

1. **Integration surface** — default: Rust subprocess wrapper module (mirrors `crates/konnect-core/src/tools/cli.rs` for kicad-cli and `crates/konnect-core/src/freerouting_mcp.rs`+`tools/integration.rs` for Freerouting: typed structs parsed from JSON, local process orchestration, `check_*` capability-probe tool) exposed as MCP tools in a new/extended toolset registered in `router/registry.rs`. Not skill-only — bundled agents can only call `mcp__konnect__*` (no Bash), per the `konnect` skill's channel rules, so `retrace` must become a real tool.
2. **Prerequisite handling** — default: a `check_retrace` tool (mirrors `check_freerouting`) verifying Python>=3.10 + package presence; config keys `retrace_python_path`/`retrace_extras` in the `config` toolset, defaulting to unset (OpenCV-only fallback), never requiring ML extras.
3. **Scale calibration input** — default: user supplies one known reference (board edge mm, or a component package size e.g. 0805) as a tool arg; agent computes mm/px and passes `--scale` to `export-kicad-pcb`. Never assume scale.
4. **Multi-photo strategy** — default: top photo = source of truth for F.Cu components/traces; bottom photo (if given) maps to B.Cu the same way; angled/oblique photos improve OCR/marking confidence only, never geometry/placement (retrace is single-photo-per-run; no fusion in v1).
5. **Mandatory human review gate** — default: YES, hard-gated. Map persists as JSON the user must approve/edit before any schematic/PCB-build call — matches Article IV (No Invention) and existing mutation-ownership rules.
6. **Accuracy expectations** — default: retrace output is draft-only; confidence <0.6 always flagged for manual ID; `export-kicad`'s synthetic pin numbers are never trusted as real pinouts — schematic build always uses the real library symbol's own pin mapping, matched by component type/value, not retrace's synthetic net XML.

### Non-goals and risks (<=5)

- **Non-goals:** zero-review full auto-recreation; inner/buried layers (no visible copper); trusting synthetic pin numbers as real; 3D multi-angle fusion; exact part numbers from illegible markings (falls back to generic value/footprint + flag).
- **Risks:** (1) OCR/vision confidence too low on cluttered real boards to hit slice metrics — mitigated by the hard review gate; (2) synthetic netlist pin numbers silently producing wrong schematics if not intercepted — mitigated by default #6; (3) placement/scale accuracy depends entirely on user-supplied calibration; (4) new Python runtime dependency (Konnect today only needs kicad-cli/Java) adds install friction; (5) reconstructing a third-party board from photos may raise IP concerns — out of scope to resolve here, but worth a user-facing warning.

## Evidence

- `crates/konnect-core/src/router/registry.rs` — toolset registration pattern (name/category entries, `build_tools_for` dispatch); `integration` toolset already groups external-capability tools (JLCPCB, datasheets, Freerouting discovery).
- `crates/konnect-core/src/tools/cli.rs` — kicad-cli wrapped as a typed `tokio::process::Command` subprocess with JSON/structured result types (`ErcViolation`, `ReportItem`) and diagnostic capture; this is the pattern to copy for `retrace`.
- `crates/konnect-core/src/freerouting_mcp.rs` + `tools/integration.rs` — Freerouting run as a local process Konnect speaks its own documented MCP protocol to (not reimplemented), exposed via `check_freerouting`/`route_specctra_dsn`; same "external tool as local subprocess, typed evidence returned" shape retrace should follow.
- `crates/konnect/assets/skills/konnect/SKILL.md` — Channel 1 (MCP-only mutation) rule and agent-routing table (one design owner at a time, complete task boundary per delegated agent).
- `crates/konnect/assets/agents/kicad-pcb-layout-agent.md` — existing agent methodology (constraint record, placement/routing gates, evidence-only completion claims) that a new photo-intake agent and the later PCB slice should follow.

## For the next agent

- Researcher brief in progress at `.orchestrator/handoffs/photo-to-kicad-reverse/02-researcher.md` — covers retrace's exact JSON schemas, Windows install reality, and KiCad-10 netlist-import behavior. Do not re-derive those; consume that brief for Slice 0/1 tool design.
- Route Slices 1-3 through a new `pcb-photo-intake-agent` (same shape as `kicad-pcb-layout-agent`) that owns photo->map work only and hands off a saved, approved map to the existing `kicad-schematic-build-agent`/`kicad-pcb-layout-agent`. Do not let one agent own both vision intake and KiCad mutation — keeps mutation ownership rules (SKILL.md) intact.
- `crates/konnect/tests/asset_references.rs` will need updating for any new skill/agent markdown (snake_case tool/param references, SKILL.md-named reference files).

## Deferred findings

(none)
