## Why

Konnect can build and edit KiCad projects but cannot start from what most hardware engineers actually have when reverse-engineering an unknown board: photos. There is no path today from a board photo to a Konnect schematic. `retrace` (an open-source Python CV/OCR pipeline) already does photo -> component/trace detection, but its raw output is pixel-space, uses synthetic (non-real) pin numbers, and writes to global per-user state — none of it is safe to feed directly into a KiCad project. This change gives Konnect a first, human-reviewed slice of "photo -> KiCad" so a hardware engineer can identify an unknown board's components and get a real, ERC-clean schematic out, without ever letting unverified vision output silently become design truth.

## What Changes

- Add a new `photo_intake` toolset to konnect-core that wraps `retrace` as a local Python subprocess, following the existing `tools/cli.rs` (kicad-cli) and `freerouting_mcp.rs`/`tools/integration.rs` (Freerouting) subprocess-wrapper pattern.
- Add a `check_retrace` capability-probe tool (mirrors `check_freerouting`): verifies the configured/discovered Python interpreter, that the `retrace` package importable, and reports which optional ML extras (detection/OCR) are present, without requiring them.
- Add a `scan_pcb_photo` tool: runs `retrace scan <img> --format json -o <tmpdir>`, reads back `analysis.json`, and returns a typed result (components with pixel bboxes, confidence, marking/value/package when available) — always invoked with `HOME`/`USERPROFILE` scoped to a project-local directory for that subprocess, to avoid polluting or reading `~/.local/share/retrace`'s global learning/cross-board state.
- Add config keys for the retrace Python path and enabled extras (stored as user-config JSON, read via the existing `config` toolset's `load_user_config`/`save_user_config`), defaulting to unset (OpenCV-only contour fallback, never requiring ML extras).
- Add persistence for a **review map**: an editable JSON artifact (components with type/value/confidence/footprint suggestion/`approved` flag; nets as ref-to-ref connections tagged `traced`/`inferred`/`manual`; the scale reference and source image paths) saved under the project directory. Add `save_photo_review_map` / `load_photo_review_map` tools.
- Add an explicit approval gate: `approve_photo_review_map` flips a persisted map's approval state; no schematic-mutating tool call may be chained from photo-intake tools before this flag is set by the user.
- Add a new skill `kicad-photo-intake` (SKILL.md + references) and a new agent `pcb-photo-intake-agent`, scoped to photo -> reviewed map only. On approval, it hands the approved map file to the existing `kicad-schematic-build-agent`, which builds the schematic using Konnect's own `sch_batch`/`sch_wiring` tools against real library symbols — retrace's synthetic pin numbers and its `.kicad_pcb`/`.net` exports are never read by Konnect.
- Register the new toolset in `crates/konnect-core/src/router/registry.rs`, update `crates/konnect/src/manifest.rs` to embed the new skill/agent assets, and update `crates/konnect/assets/skills/konnect/SKILL.md`'s agent-routing table so the new agent is reachable and passes `crates/konnect/tests/asset_references.rs`.

None of this is **BREAKING**: it is additive (new toolset, new tools, new skill, new agent). No existing tool schema, config key, or agent changes behavior.

## Capabilities

### New Capabilities
- `photo-intake`: photo -> component/net detection (via wrapped `retrace`) -> persisted, human-editable review map -> explicit approval gate -> handoff to schematic build. Covers `check_retrace`, `scan_pcb_photo`, the review-map schema and its persistence/approval tools, and the constraint that no KiCad project mutation happens before approval.

### Modified Capabilities
(none — no existing spec's requirements change)

## Impact

- **New code**: `crates/konnect-core/src/tools/photo_intake.rs` (tool definitions/handlers), a subprocess-runner module (new file or extending `tools/cli.rs`'s pattern) for invoking `retrace`, typed structs for `analysis.json`, and the review-map JSON schema/persistence.
- **Modified code**: `crates/konnect-core/src/router/registry.rs` (register `photo_intake` toolset), `crates/konnect-core/src/tools/config.rs` (new default config keys), `crates/konnect/src/manifest.rs` (embed new skill/agent assets), `crates/konnect/assets/skills/konnect/SKILL.md` (agent routing table).
- **New assets**: `crates/konnect/assets/skills/kicad-photo-intake/SKILL.md` (+ `references/`), `crates/konnect/assets/agents/pcb-photo-intake-agent.md`.
- **New test coverage**: `crates/konnect/tests/asset_references.rs` continues to pass (new skill/agent conform to its checks); unit/integration tests for the subprocess runner, `check_retrace`, `scan_pcb_photo` parsing, and review-map persistence/approval gate.
- **New external dependency**: a Python 3.10+ runtime with `retrace` installed is required to exercise `scan_pcb_photo` for real; `check_retrace` reports absence gracefully (mirrors `check_freerouting`'s graceful-degradation shape) — Konnect itself gains no new hard dependency, only an optional-capability one.
- **Out of scope for this change (queued as later changes)**: Slice 2 (trace extraction, scale calibration, net inference, PCB placement) and Slice 3 (multi-photo top+bottom, angled-photo identification, Freerouting-based routing) per the analyst's slice priority order — this change covers Slice 0 (retrace wrapped as Konnect MCP tools) and Slice 1 (one top photo -> human-reviewed map -> Konnect schematic) only.

### User stories covered by this change

1. As a hardware engineer, I send top-view photos of an unknown board (e.g. an LDO module) and get a component table (type, marking, value/part-number if legible, confidence). AC: low-confidence items are flagged, never silently guessed.
2. As a hardware engineer, I get a proposed net list derived from visible traces. AC: each connection is tagged traced-vs-inferred; nothing is built yet.
3. As a hardware engineer, nothing is built into a KiCad project until I approve/edit the map. AC: the component+net map persists as an editable JSON artifact between runs; no `create_project`/schematic-build call happens before explicit approval.
4. As a hardware engineer, my approved map becomes a real schematic. AC: `kicad-schematic-build-agent` produces a saved `.kicad_sch` with 1 symbol per approved component and nets matching the approved list; ERC run and reported.

Story 5 (photo-derived PCB placement/routing) is deferred to Slice 2/3, a later change.

### Non-goals

- Zero-review full automatic recreation of a board — the human-review/approval gate is mandatory and hard-blocking.
- Inner/buried copper layers (no visible-photo path can see them; out of scope permanently, not just this slice).
- Multi-view (top+bottom+angled) photo fusion — deferred to Slice 3; this change is single top-photo only.
- Trusting `retrace`'s synthetic, arrival-order pin numbers (from its `export-kicad`/`.net` output) as real pinouts — the schematic build always uses the real KiCad library symbol's own pin map, matched by component type/value, never retrace's synthetic net XML.
- Automatically building/mutating a KiCad project without the explicit approval flag being set by the user.
- Server-side enforcement of the approval gate inside the `sch_*`/`pcb_*` tool handlers themselves. This slice enforces the gate at the map layer (`approval_valid`, computed server-side from a hash comparison) and at the agent/skill prose layer (`kicad-photo-intake` skill, `pcb-photo-intake-agent`, and `kicad-schematic-build-agent` all check `approval_valid` before acting); a tool-level check inside the mutating handlers that blocks a mutation even if an agent's prose is bypassed or skipped is deferred to a later slice.

### Success metrics (as scoped by this change, verbatim from the analyst brief)

- **Slice 0 (prereq, not user-facing):** `scan_pcb_photo` on a known sample image returns JSON matching retrace's Component/Trace schema in <30s, no crash when YOLO/OCR extras are absent.
- **Slice 1 (MVP):** on a reference LDO test board (~5-8 components), all components identified (type+rough value) with confidence, user reviews/corrects, and the generated schematic has a 1:1 component-count match with the physical board and passes ERC clean.
