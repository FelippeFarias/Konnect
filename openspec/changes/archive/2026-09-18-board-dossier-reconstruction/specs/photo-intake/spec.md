## ADDED Requirements

### Requirement: board photo views
The system SHALL provide a `prepare_board_photo` tool that produces a saved
PNG view of a source photo — cropped, rotated, and/or scaled — under the
target review map's directory, for an agent to zoom into silkscreen text,
terminals, and component markings it cannot read at full-photo resolution.
Input SHALL be `image_path`, `project_dir`, `map_id` (an existing map
directory — this tool does not mint a new map), and optionally `crop`
(`x`/`y`/`w`/`h` in source-image pixels), `rotate` (one of `0`/`90`/`180`/
`270`), `scale` (`0.25`..`4.0`), and `label`. Output SHALL be the saved view's
path, the source rectangle actually used (the full image when `crop` is
omitted), the output pixel size, and `mm_per_px` when the map's
`scale_reference` currently carries a resolved value. `map_id` and `label`
SHALL both be validated as `^[A-Za-z0-9_-]{1,64}$` tokens before any path
join, and the output location SHALL be computed as `<project_dir>/.konnect/
photo_intake/<map_id>/views/`, never supplied by the caller.

#### Scenario: a cropped, scaled view is produced
- **WHEN** `prepare_board_photo` is called with `crop` and `scale` against an
  existing map's source photo
- **THEN** the tool returns a saved PNG path under that map's `views/`
  directory, the source rectangle used, and the output pixel size

#### Scenario: a view carries a physical scale when one is resolved
- **WHEN** `prepare_board_photo` is called for a map whose `scale_reference`
  currently has a resolved `mm_per_px`
- **THEN** the response includes `mm_per_px` for the produced view; a map
  with no resolved scale omits the field rather than guessing one

#### Scenario: an out-of-bounds crop is rejected
- **WHEN** `prepare_board_photo` is called with a `crop` rectangle that falls
  outside the source image's pixel dimensions
- **THEN** the tool returns an error result naming the image's actual
  dimensions, and no file is written

#### Scenario: a map directory that does not exist is rejected
- **WHEN** `prepare_board_photo` is called with a `map_id` for which no map
  directory exists under the given `project_dir`
- **THEN** the tool returns an error result and does not create one — a map
  directory is minted only by `scan_pcb_photo`

### Requirement: board dossier section of the review map
The system SHALL allow an optional `dossier` object to be persisted on the
review map via the existing `save_photo_review_map`/`load_photo_review_map`
tools, additive to the fields already defined by the "persisted,
human-editable review map" requirement. The `dossier` object SHALL record:
an `identity` claim (summary, `basis` of `observed` or `inferred`, numeric
`confidence`, and `evidence`); a `physical` description (board size in pixels
always, in millimeters only once `scale_reference` resolves one, mounting
holes, connectors — a dossier MAY be saved with the board size unresolved to
millimeters, listing that gap in `open_questions` rather than estimating it);
a `component_survey` (one entry per visual class — e.g. LED dome, axial
resistor, terminal block — each recording its `count`, the `count_method`
used to reach it, e.g. `manual_count_by_region`/`blob_count`/
`hough_circles`, and MAY record `count_alternatives` from a second method
when methods disagree, plus per-region `locations` with a source photo view,
and traceability to correlated `retrace` component ids where applicable);
`silkscreen_markings` (text, location, source photo view, `basis`, and
`confidence`); `topology_claims` (each naming the open question it answers
and one or more `hypotheses` — a claim SHALL carry more than one hypothesis
when the evidence does not resolve to a single answer, each hypothesis with
its own `confidence`, and, when numeric, a stated `calculation` and its
`assumptions` — plus a `resolution_path` describing how the open question
could be resolved with more evidence, when one exists); a
`retrace_correlation` list tying `dossier` observations to `scan_pcb_photo`
component ids by bounding-box overlap; an optional `design_brief_seed` (a
short preliminary sketch of the implied design direction — topology idea,
rough BOM shape, physical spec — that `pcb-design-reconstruction-agent`
starts from, distinct from the full `design_brief` object it later
produces); and `open_questions`. The schema SHALL keep every level of
`dossier` open (`additionalProperties: true`), matching the same "a human
edits this by hand between calls" rationale already applied to
`components`/`nets`/`scale_reference`. `dossier` SHALL be covered by the
review map's content hash (see "hard human-review gate before any KiCad
mutation") when present, and SHALL NOT be covered when absent, so a map saved
before this change hashes unchanged.

#### Scenario: a dossier persists like any other reviewed section
- **WHEN** `save_photo_review_map` is called with a `map` that includes a
  `dossier` object
- **THEN** the map is written with `dossier` intact and
  `load_photo_review_map` returns it byte-for-byte on the next call

#### Scenario: an inferred topology claim states its calculation and assumptions
- **WHEN** a `dossier.topology_claims` entry has a hypothesis with
  `basis: "inferred"`
- **THEN** `kicad-board-dossier`'s methodology commits the writing agent to
  including a `calculation` and `assumptions` for that hypothesis whenever it
  is numeric — verified by that skill's text in
  `crates/konnect/assets/skills/kicad-board-dossier/SKILL.md`; no automated
  test enforces a non-empty `calculation` field, since the schema keeps
  `dossier` open by design

#### Scenario: a count method disagreement is recorded, not silently resolved
- **WHEN** two counting methods applied to the same visual class disagree
  (e.g. a blob count and a Hough-circle count over the same photo)
- **THEN** `kicad-board-dossier`'s methodology commits the writing agent to
  recording both in `count_method`/`count_alternatives` rather than reporting
  only the one it trusts more — verified by that skill's text in
  `crates/konnect/assets/skills/kicad-board-dossier/SKILL.md`; no automated
  test enforces this, since the schema keeps `dossier` open by design

#### Scenario: an unresolved question keeps competing hypotheses, not one guess
- **WHEN** the photographic evidence does not resolve a topology question to
  a single answer (e.g. string length could be 6 or 8-10 LEDs)
- **THEN** `kicad-board-dossier`'s methodology commits the writing agent to
  recording each candidate as its own entry in `topology_claims.hypotheses`
  with its own confidence, rather than picking one and discarding the other —
  verified by that skill's text in
  `crates/konnect/assets/skills/kicad-board-dossier/SKILL.md`; no automated
  test enforces this, since the schema keeps `dossier` open by design

#### Scenario: adding a dossier to a map changes its content hash
- **WHEN** `save_photo_review_map` is called for a map whose previously
  computed `review_map_content_hash` did not cover a `dossier` field, and the
  incoming `map` now includes one
- **THEN** the newly computed hash differs from the old one, and the map's
  `approved` state resets to `false` per the existing edit-revokes-approval
  behavior

### Requirement: design brief section of the review map
The system SHALL allow an optional `design_brief` object to be persisted on
the review map, additive to `dossier` and the base review-map fields. The
`design_brief` object SHALL record: a `block_diagram` (named functional
blocks with inputs/outputs); `circuits` (per-block description, calculated
values with the formula and assumptions used, and derating notes); a `bom`
(one entry per part role, naming a real KiCad library symbol and footprint —
obtained via the `search_symbols`/`search_footprints` tools, never invented —
plus the calculated or matched value, quantity, and whether it was `matched`,
`equivalent`, or `calculated`); `physical_constraints` (board size, mounting
holes, connector edges, keep-outs); and `open_questions`. The schema SHALL
keep every level of `design_brief` open (`additionalProperties: true`), for
the same reason as `dossier`. `design_brief` SHALL be covered by the review
map's content hash when present, and SHALL NOT be covered when absent.

#### Scenario: a design brief persists additively
- **WHEN** `save_photo_review_map` is called with a `map` that includes a
  `design_brief` object alongside an already-approved `dossier`
- **THEN** the map is written with `design_brief` intact and
  `load_photo_review_map` returns it byte-for-byte on the next call

#### Scenario: design reconstruction is refused before the dossier is approved
- **WHEN** `load_photo_review_map` reports `approval_valid: false` for a map
  that has no `design_brief` yet
- **THEN** `pcb-design-reconstruction-agent`'s text commits it to refuse to
  write `design_brief` content for that map and to report `INCOMPLETE`
  instead — verified by `agents_make_claimed_evidence_executable`'s marker
  case for `pcb-design-reconstruction-agent.md`
  (`crates/konnect/tests/asset_references.rs`), which pins the
  `approval_valid` marker in its text against deletion

#### Scenario: every BOM entry names a real library part
- **WHEN** `kicad-design-reconstruction`'s methodology adds a `bom` entry
- **THEN** that skill's text commits the writing agent to resolving
  `kicad_symbol`/`kicad_footprint` via `search_symbols`/`search_footprints`
  before the entry is added, never from memory alone — verified by that
  skill's text in `crates/konnect/assets/skills/kicad-design-reconstruction/
  SKILL.md`; no automated test exercises this end-to-end in this slice

### Requirement: approved design brief hands off to schematic and PCB build
The system SHALL define, in "Building from an approved design brief" sections
of `kicad-schematic-build-agent.md` and `kicad-pcb-layout-agent.md`, how an
approved `design_brief` becomes build input: `kicad-schematic-build-agent`
places one symbol per `bom` entry (matched via `search_symbols`, using the
entry's own `kicad_symbol` as the starting search term) and wires the
`circuits` topology, exactly as it already does for an approved photo-intake
map's `components`/`nets`; `kicad-pcb-layout-agent` reads `physical_
constraints` (board size, mounting holes, connector edges, keep-outs) as hard
placement/outline constraints for that layout. Both SHALL call
`load_photo_review_map` themselves and proceed only when the response's
`approval_valid` is true, never trusting a caller's summary or the map's own
`approved` field, exactly as the existing photo-intake handoff already
requires for `components`/`nets`.

#### Scenario: schematic build places one symbol per BOM entry
- **WHEN** an approved `design_brief` with N `bom` entries is handed to
  schematic build
- **THEN** the "Building from an approved design brief" section of
  `kicad-schematic-build-agent.md` commits the agent to placing one symbol per
  entry, matched via `search_symbols` — verified by that section's text; no
  automated test exercises this end-to-end in this slice

#### Scenario: PCB layout treats physical constraints as hard limits
- **WHEN** an approved `design_brief`'s `physical_constraints` names a board
  size and mounting-hole positions
- **THEN** the "Building from an approved design brief" section of
  `kicad-pcb-layout-agent.md` commits the agent to using those as the board
  outline and fixed mounting-hole placements rather than choosing its own —
  verified by that section's text; no automated test exercises this
  end-to-end in this slice

#### Scenario: an unapproved design brief cannot reach build
- **WHEN** `load_photo_review_map` returns `approval_valid: false` for a map
  carrying a `design_brief`
- **THEN** both consumer sections commit their agents to refusing to place or
  route anything sourced from that map and reporting `INCOMPLETE` instead —
  verified by `agents_make_claimed_evidence_executable`'s per-file
  marker-presence assertions on `kicad-schematic-build-agent.md`
  (`approval_valid`, `INCOMPLETE`) and `kicad-pcb-layout-agent.md`
  (`load_photo_review_map`, `approval_valid`, `INCOMPLETE`), and by
  `skills_define_the_same_evidence_boundary_as_their_agents`'s equivalent
  per-file assertions on `kicad-design-reconstruction/SKILL.md`
  (`load_photo_review_map`, `approval_valid`, `INCOMPLETE`) and
  `kicad-photo-to-board/SKILL.md` (`approve_photo_review_map`,
  `approval_valid`, `INCOMPLETE`) (`crates/konnect/tests/
  asset_references.rs`) — each case only asserts that the named marker
  string is present somewhere in that one file's text, not that the file's
  prose actually enforces the gate

## MODIFIED Requirements

### Requirement: persisted, human-editable review map
The system SHALL define a review-map JSON schema and provide `save_photo_review_map` / `load_photo_review_map` tools to persist and reload it as a project-scoped file (not held only in-memory / conversation state). The schema SHALL include: a list of components, each with `ref` (assigned during review, may be absent pre-review), `type`, `value`, `confidence`, `footprint_suggestion`, source `bbox_px`, and an `approved` boolean; a list of nets, each as an array of `ref`-to-`ref`-or-pin connections tagged `traced`, `inferred`, or `manual`; a `scale_reference` object (`kind` plus a physical `value`, either given directly by the user or resolved by the agent from a known physical feature — a user-stated board edge length, a mounting-hole pitch, or a known package outline — in which case the object SHALL also carry a resolved `mm_per_px` and an `evidence` string naming the feature and the reasoning used; a scale the agent cannot tie to a stated physical feature SHALL remain unresolved rather than estimated); and a `source_images` list (paths to the photo(s) the map was derived from). Any component with `confidence` below 0.6 SHALL be persisted with that confidence intact (never silently rounded up or hidden) so the review UI can flag it.

#### Scenario: a freshly scanned map persists as editable JSON
- **WHEN** `save_photo_review_map` is called with a map derived from a `scan_pcb_photo` result
- **THEN** the map is written to a project-scoped JSON file and `load_photo_review_map` returns byte-for-byte the same structure on the next call

#### Scenario: low-confidence components are flagged, never auto-corrected
- **WHEN** a review map is saved containing a component with `confidence` below 0.6
- **THEN** the persisted file retains that confidence value unchanged, and does not synthesize a `part_number`/`value` for it that the scan did not produce

#### Scenario: the review map survives across sessions
- **WHEN** a review map was saved in a previous session and the user starts a new session against the same project
- **THEN** `load_photo_review_map` returns the previously saved map, including any manual edits the user made to the JSON file directly

#### Scenario: agent-resolved scale reference is recorded with evidence
- **WHEN** an agent resolves `scale_reference.mm_per_px` from a known
  physical feature rather than the user directly stating a millimeter value
- **THEN** the saved `scale_reference` carries both `mm_per_px` and an
  `evidence` string naming the feature relied on, and
  `kicad-board-dossier`'s methodology commits the agent to never inventing
  a `mm_per_px` it cannot name evidence for — verified by that skill's text
  in `crates/konnect/assets/skills/kicad-board-dossier/SKILL.md`; no
  automated test enforces this, since the schema does not reject an
  unevidenced `mm_per_px`

### Requirement: hard human-review gate before any KiCad mutation
The system SHALL provide an `approve_photo_review_map` tool that sets the persisted map's approval state. `load_photo_review_map` SHALL report `approval_valid: true` for a map only when `approve_photo_review_map` was called explicitly for it and the map's current content hash still equals the hash recorded at that approval. The content hash SHALL cover `source_images`, `scale_reference`, `components`, `nets`, and, when present on the map, `dossier` and `design_brief` — a covered section absent from the map contributes nothing to the hashed bytes, so a map saved before this change, or one that never gains a `dossier`/`design_brief`, hashes exactly as it did under the base requirement. Because `dossier` and `design_brief` join the hash only once present, one review map carries two sequential approval checkpoints through the same mechanism: an approval recorded while only `dossier` is present covers the board-comprehension content; adding `design_brief` afterward changes the covered content and therefore revokes that approval like any other tracked-field edit, requiring `approve_photo_review_map` again before design-brief content is trusted. `save_photo_review_map` SHALL never persist `approved: true` from client-supplied input, and SHALL ignore every client-supplied bookkeeping key (`approved`, `approved_at`, `content_hash_at_approval`, `saved_at`, `approval_valid`) present in the incoming `map`, deriving each one only from the record already on disk. Approval SHALL be explicit and per-map (an `approved_at` timestamp and, when available, an approving identity), never inferred from the map merely existing or from a scan having completed. The bundled `kicad-photo-intake`, `kicad-board-dossier`, and `kicad-design-reconstruction` skills, and the `pcb-photo-intake-agent`, `pcb-design-reconstruction-agent`, `kicad-schematic-build-agent`, and `kicad-pcb-layout-agent` agents, SHALL NOT invoke a schematic-, board-, or library-mutating tool for a map whose `approval_valid` is false. Server-side enforcement of this rule inside the `sch_*`/`pcb_*` tool handlers themselves is explicitly out of scope for this slice and is deferred to a later change (see proposal.md Non-Goals).

#### Scenario: unapproved map cannot reach schematic build
- **WHEN** `load_photo_review_map` returns `approval_valid: false` for a map
- **THEN** `kicad-schematic-build-agent.md`'s "Building from an approved photo-intake map" section instructs the agent to refuse to place or wire any component sourced from that map, and no `.kicad_sch` is created or modified — verified by that section's text in `crates/konnect/assets/agents/kicad-schematic-build-agent.md`, and by `load_reports_approval_valid_false_after_an_out_of_band_edit` (`crates/konnect-core/src/tools/photo_intake.rs`), which proves `approval_valid` goes false the instant on-disk content no longer matches the hash recorded at approval

#### Scenario: approval requires an explicit call
- **WHEN** `approve_photo_review_map` is called for a saved map
- **THEN** the persisted map's `approved` state becomes true and an `approved_at` timestamp is recorded, and only this action (never `save_photo_review_map` alone) can produce that state

#### Scenario: editing an approved map revokes approval
- **WHEN** `save_photo_review_map` is called again for a map that was previously approved, with different component or net content
- **THEN** the persisted map's `approved` state resets to false and `approve_photo_review_map` must be called again before schematic build may consume it

#### Scenario: adding a design brief revokes a dossier-only approval
- **WHEN** a map was approved while it carried only `dossier` (no
  `design_brief`), and `save_photo_review_map` is then called with a
  `design_brief` added
- **THEN** the recomputed content hash no longer matches
  `content_hash_at_approval`, `approved` resets to `false`, and
  `approve_photo_review_map` must be called again before either
  `kicad-schematic-build-agent` or `kicad-pcb-layout-agent` may consume the
  `design_brief`
