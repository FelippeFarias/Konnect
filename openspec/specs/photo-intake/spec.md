# photo-intake Specification

## Purpose
TBD - created by archiving change photo-to-kicad-reverse. Update Purpose after archive.

## Requirements

### Requirement: retrace capability probe
The system SHALL provide a `check_retrace` tool that reports whether the `retrace` Python package is usable, without requiring it. It SHALL resolve the Python interpreter from an explicit `python_path` argument if given, else from the configured `photo_intake.retrace_python_path` user-config key, else by discovery (`python3`/`python` on PATH). It SHALL report interpreter presence, `retrace` package importability, package version when available, and which optional extras (`detection`, `ocr`) are importable, and it SHALL NOT fail (error result) merely because `retrace` or its extras are absent — absence is a reported field, not an error.

#### Scenario: retrace and extras are fully installed
- **WHEN** `check_retrace` is called and the resolved interpreter has `retrace` and both `detection`/`ocr` extras importable
- **THEN** the tool returns `available: true`, `retrace_version`, and `extras: {detection: true, ocr: true}`

#### Scenario: retrace is missing entirely
- **WHEN** `check_retrace` is called and no resolved interpreter has `retrace` importable
- **THEN** the tool returns `available: false` with a `note` explaining how to install it, and does NOT return an error result

#### Scenario: base retrace present, ML extras absent
- **WHEN** `check_retrace` is called and `retrace` imports but `detection`/`ocr` extras do not
- **THEN** the tool returns `available: true`, `extras: {detection: false, ocr: false}`, and a `note` that `scan_pcb_photo` will use the OpenCV contour-only fallback

### Requirement: photo scan produces a typed, project-scoped analysis
The system SHALL provide a `scan_pcb_photo` tool that runs `retrace scan <image> --format json -o <dir>` as a subprocess and parses the resulting `analysis.json` back into a typed result: components (id, label, confidence, pixel bbox `[x,y,w,h]`, marking, part_number, value, package, datasheet_url when present) and traces (id, pixel points, width_px, from/to component). The output directory SHALL be a path under the current Konnect project's directory (never a bare OS temp directory outside project scope), so the raw `analysis.json` and the source image are retrievable evidence, not silently discarded. Every subprocess invocation SHALL run with `HOME` (POSIX) / `USERPROFILE` (Windows) redirected to a directory scoped to that single invocation, so `retrace`'s own global stores (`knowledge.json`, `cross_board.json`, `learned_components.json` under `~/.local/share/retrace`) are never read from or written to the real OS user's home directory.

#### Scenario: scan succeeds on a supported image
- **WHEN** `scan_pcb_photo` is called with a valid image path and an open Konnect project
- **THEN** the tool returns the parsed component and trace list with pixel-space bboxes, the path to the saved `analysis.json`, and elapsed duration, within the tool's timeout

#### Scenario: scan runs without ML extras installed
- **WHEN** `scan_pcb_photo` is called and only the OpenCV contour fallback is available (per `check_retrace`)
- **THEN** the tool still returns a result (components with `confidence: 0.5` and coarse `label` buckets, no crash), and flags in its response that marking/value/part_number were not attempted

#### Scenario: scan does not pollute the real user's home directory
- **WHEN** `scan_pcb_photo` runs a `retrace scan` subprocess
- **THEN** no file under the real `~/.local/share/retrace` (or Windows equivalent) is created or modified as a result of that call

#### Scenario: retrace is unavailable
- **WHEN** `scan_pcb_photo` is called and `check_retrace` would report `available: false`
- **THEN** the tool returns an error result naming that `retrace` is not available and pointing to `check_retrace` for diagnosis, rather than a partial or fabricated result

### Requirement: persisted, human-editable review map
The system SHALL define a review-map JSON schema and provide `save_photo_review_map` / `load_photo_review_map` tools to persist and reload it as a project-scoped file (not held only in-memory / conversation state). The schema SHALL include: a list of components, each with `ref` (assigned during review, may be absent pre-review), `type`, `value`, `confidence`, `footprint_suggestion`, source `bbox_px`, and an `approved` boolean; a list of nets, each as an array of `ref`-to-`ref`-or-pin connections tagged `traced`, `inferred`, or `manual`; a `scale_reference` object (kind + user-supplied physical value, e.g. board edge mm or known package); and a `source_images` list (paths to the photo(s) the map was derived from). Any component with `confidence` below 0.6 SHALL be persisted with that confidence intact (never silently rounded up or hidden) so the review UI can flag it.

#### Scenario: a freshly scanned map persists as editable JSON
- **WHEN** `save_photo_review_map` is called with a map derived from a `scan_pcb_photo` result
- **THEN** the map is written to a project-scoped JSON file and `load_photo_review_map` returns byte-for-byte the same structure on the next call

#### Scenario: low-confidence components are flagged, never auto-corrected
- **WHEN** a review map is saved containing a component with `confidence` below 0.6
- **THEN** the persisted file retains that confidence value unchanged, and does not synthesize a `part_number`/`value` for it that the scan did not produce

#### Scenario: the review map survives across sessions
- **WHEN** a review map was saved in a previous session and the user starts a new session against the same project
- **THEN** `load_photo_review_map` returns the previously saved map, including any manual edits the user made to the JSON file directly

### Requirement: hard human-review gate before any KiCad mutation
The system SHALL provide an `approve_photo_review_map` tool that sets the persisted map's approval state. `load_photo_review_map` SHALL report `approval_valid: true` for a map only when `approve_photo_review_map` was called explicitly for it and the map's current D8-normalized content hash still equals the hash recorded at that approval (D16). `save_photo_review_map` SHALL never persist `approved: true` from client-supplied input, and SHALL ignore every client-supplied bookkeeping key (`approved`, `approved_at`, `content_hash_at_approval`, `saved_at`, `approval_valid`) present in the incoming `map`, deriving each one only from the record already on disk. Approval SHALL be explicit and per-map (an `approved_at` timestamp and, when available, an approving identity), never inferred from the map merely existing or from a scan having completed. The bundled `kicad-photo-intake` skill, `pcb-photo-intake-agent`, and `kicad-schematic-build-agent` SHALL NOT invoke a schematic- or board-mutating tool for a map whose `approval_valid` is false. Server-side enforcement of this rule inside the `sch_*`/`pcb_*` tool handlers themselves is explicitly out of scope for this slice and is deferred to a later change (see proposal.md Non-Goals).

#### Scenario: unapproved map cannot reach schematic build
- **WHEN** `load_photo_review_map` returns `approval_valid: false` for a map
- **THEN** `kicad-schematic-build-agent.md`'s "Building from an approved photo-intake map" section instructs the agent to refuse to place or wire any component sourced from that map, and no `.kicad_sch` is created or modified — verified by that section's text in `crates/konnect/assets/agents/kicad-schematic-build-agent.md`, and by `load_reports_approval_valid_false_after_an_out_of_band_edit` (`crates/konnect-core/src/tools/photo_intake.rs`), which proves `approval_valid` goes false the instant on-disk content no longer matches the hash recorded at approval

#### Scenario: approval requires an explicit call
- **WHEN** `approve_photo_review_map` is called for a saved map
- **THEN** the persisted map's `approved` state becomes true and an `approved_at` timestamp is recorded, and only this action (never `save_photo_review_map` alone) can produce that state

#### Scenario: editing an approved map revokes approval
- **WHEN** `save_photo_review_map` is called again for a map that was previously approved, with different component or net content
- **THEN** the persisted map's `approved` state resets to false and `approve_photo_review_map` must be called again before schematic build may consume it

### Requirement: approved map hands off to real schematic build
The system SHALL define, in the "Building from an approved photo-intake map" section of `kicad-schematic-build-agent.md`, how an approved review map becomes schematic-build input: one Konnect library symbol placement per `approved: true` component (matched by `type`/`value`, not by any pin data from `retrace`), and one wire/net-label operation per net entry in the approved map, built using Konnect's own `sch_batch`/`sch_wiring` tools, with `run_erc` invoked and its result reported. The system SHALL NOT read or trust any `retrace`-generated `.kicad_sch`, `.kicad_pcb`, or `.net` file; retrace's synthetic, arrival-order pin numbering SHALL never be used as a real pinout. This slice delivers the documented calling convention only: no new Rust code in this change enforces component count, wiring completeness, or ERC invocation for a photo-intake handoff specifically — that enforcement is `kicad-schematic-build-agent`'s existing build behavior, applied to a new input source.

#### Scenario: approved map builds a component-complete schematic
- **WHEN** an approved review map with N approved components is handed to schematic build
- **THEN** the "Building from an approved photo-intake map" section of `kicad-schematic-build-agent.md` commits the agent to placing exactly N symbols, one per approved component, each using its real library symbol's own pin map — verified by that section's text in `crates/konnect/assets/agents/kicad-schematic-build-agent.md`; no automated test exercises this end-to-end in this slice

#### Scenario: approved nets become real wiring
- **WHEN** an approved review map's net list is handed to schematic build
- **THEN** the same section commits the agent to turning each net entry into a wire or net-label connection between the corresponding real symbol pins, and to invoking `run_erc` and reporting its result alongside the build — verified by that section's text in `crates/konnect/assets/agents/kicad-schematic-build-agent.md`; no automated test exercises this end-to-end in this slice

#### Scenario: unapproved components are excluded, not guessed
- **WHEN** a review map contains components the user did not mark `approved: true`
- **THEN** the same section commits the agent to excluding those components from placement and listing them as skipped in the build result, rather than including them with a guessed value — verified by that section's text in `crates/konnect/assets/agents/kicad-schematic-build-agent.md`; no automated test exercises this end-to-end in this slice
