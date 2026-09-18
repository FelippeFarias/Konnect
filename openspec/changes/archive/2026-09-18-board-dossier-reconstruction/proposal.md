## Why

`photo-to-kicad-reverse` (archived) proved that treating a board photo as a CV
problem for `retrace` to solve alone does not work: on the real reference
photos (24 V LED traffic-light module, WhatsApp `.jpeg`, 2026-09-18) retrace's
stock `yolov8n.pt` detector mislabels LED domes as resistors/capacitors,
copper segmentation only sees bare copper (mask-covered traces are invisible),
and 0 nets and 0 markings come back. retrace's boxes are, at best, coarse
position/count hints. The multimodal agent itself can already look at the
photos (`Read` renders images) and reason about what it is looking at the way
a hardware engineer would — it is the primary evidence source this change was
missing, not an add-on.

This change adds that comprehension step. Before any component/net map is
built, the intake agent surveys the photos itself, correlates what it sees
with retrace's hints, and writes a **board dossier**: a structured, evidence-
and-confidence-tagged account of what the board is, physically and
electrically, with every inferred fact labelled as inference and never
passed off as observation. A human approves the dossier. Only then does a
**design reconstruction** step plan a functionally equivalent board —
topology, calculated component values, a BOM of real KiCad library parts,
physical constraints — for a human to approve as a **design brief**, which the
existing `kicad-schematic-build-agent` and `kicad-pcb-layout-agent` build from.
The goal is comprehension and reconstruction, not photogrammetric reverse
engineering.

## What Changes

- Add a `prepare_board_photo` tool to the `photo_intake` toolset: crops,
  rotates, and scales a source photo into a saved PNG view under the map's
  directory, with geometry (source rect, output size, and `mm_per_px` when the
  map's scale is resolved) in the response — the mechanism the intake agent
  uses to zoom into silkscreen, terminals, and component markings.
- Promote the `image` crate from a `konnect-core` dev-dependency to a regular
  dependency (needed to decode real photos, JPEG included, and encode PNG
  views), and add the `jpeg` feature to the workspace's `image` declaration.
- Add two additive, human-approved sections to the persisted review map:
  `dossier` (the board-comprehension record: identity, physical layout,
  component survey by visual class, silkscreen markings with location,
  topology claims with calculations and stated assumptions, retrace
  correlation, open questions) and `design_brief` (the reconstruction plan:
  block diagram, per-block calculated values with derating, a BOM of real
  KiCad library symbols/footprints, physical constraints). Both are covered by
  the existing content hash, giving the one artifact two sequential approval
  checkpoints via the existing `approve_photo_review_map` tool — no new
  approval tool or state machine.
- Widen `scale_reference` so an agent may resolve `mm_per_px` from a known
  physical feature (a user-given board edge, mounting-hole pitch, a known
  package) and record it with `evidence`, alongside the existing user-supplied
  value — **MODIFIED** behavior: the base spec currently requires the scale
  reference to always be user-supplied.
- **BREAKING (docs-only):** the `kicad-photo-intake` skill and
  `pcb-photo-intake-agent` currently state "the scale reference is always
  user-supplied" as a hard rule; this change replaces that rule with the
  agent-resolved-with-evidence option above.
- Extend `pcb-photo-intake-agent` (frontmatter gains `Read`; scope grows to
  own the comprehension/dossier phase) rather than splitting a new agent, to
  keep one owner of the map until dossier approval.
- Add a new agent, `pcb-design-reconstruction-agent`, that owns the
  `design_brief` phase: it never mutates a KiCad file itself and hands its
  approved brief to `kicad-schematic-build-agent` and
  `kicad-pcb-layout-agent`, each of which re-checks `approval_valid` before
  consuming it (mirroring the existing photo-intake handoff pattern).
- Add three new skills: `kicad-board-dossier` (survey/classify/correlate/infer
  methodology, with a reference doc for the dossier schema),
  `kicad-design-reconstruction` (block-diagram-to-BOM methodology, with a
  reference doc for the design-brief schema), and `kicad-photo-to-board` — a
  top-level, single-entry-point workflow skill for "I have photos of a board,
  recreate it": it sequences `check_retrace` → `scan_pcb_photo` → dossier
  (`pcb-photo-intake-agent`) → human approval #1 → design brief
  (`pcb-design-reconstruction-agent`) → human approval #2 →
  `kicad-schematic-build-agent` → `kicad-pcb-layout-agent` →
  `kicad-design-review-agent`, naming what each stage consumes/produces (which
  map sections, which `approval_valid` re-check), what the human is shown at
  each gate, and how a stage reports `INCOMPLETE` instead of inventing past a
  gap in the evidence.
- Update `konnect/SKILL.md`'s decision tree and "Agent Routing and Mutation
  Ownership" section to route "I have photos of a board" through
  `kicad-photo-to-board`, and add consumer notes for an approved design brief
  to `kicad-schematic-build-agent.md` and `kicad-pcb-layout-agent.md`.
- Add `docs/PHOTO_TO_BOARD_WORKFLOW.md`, describing the pipeline, its
  artifacts (`analysis.json`, `views/`, the review map's sections), its two
  approval points, and its honest limits (no inner layers, scale must be
  supplied or resolved with stated evidence, values are inferred unless
  directly read), linked from `README.md`.
- Bump the `photo_intake` toolset's `tool_count` (5 → 6) and every
  registry-derived and hand-written tool-count reference.

## Capabilities

### New Capabilities
(none — every change below extends the existing `photo-intake` capability)

### Modified Capabilities
- `photo-intake`: adds `prepare_board_photo`; adds the `dossier` and
  `design_brief` review-map sections and their approval-gate coverage; widens
  `scale_reference` to allow an agent-resolved value with evidence; extends
  the skill/agent set that owns and consumes the map.

## Impact

- **Code:** `crates/konnect-core/src/tools/photo_intake.rs` (new tool,
  extended schema, extended hash coverage), `crates/konnect-core/src/router/
  registry.rs` (`tool_count`), `crates/konnect-core/Cargo.toml` and the
  workspace root `Cargo.toml` (`image` dependency), `crates/konnect/tests/
  asset_references.rs` (`NOT_TOOLS`, open-schema allowlist), `crates/konnect/
  tests/doc_tool_counts.rs`-swept docs (README.md, DEV.md, tool-directory.md,
  docs/TROUBLESHOOTING.md, packaging/metadata.json, plugin/plugin.json),
  `crates/konnect/src/manifest.rs`.
- **Assets:** `crates/konnect/assets/skills/kicad-board-dossier/**`,
  `crates/konnect/assets/skills/kicad-design-reconstruction/**`,
  `crates/konnect/assets/skills/kicad-photo-to-board/**` (new, top-level
  workflow skill), `crates/konnect/assets/skills/kicad-photo-intake/
  references/review-map-schema.md` (extended), `crates/konnect/assets/agents/
  pcb-photo-intake-agent.md` (extended), `crates/konnect/assets/agents/
  pcb-design-reconstruction-agent.md` (new), `crates/konnect/assets/skills/
  konnect/SKILL.md`, `crates/konnect/assets/agents/
  kicad-schematic-build-agent.md`, `crates/konnect/assets/agents/
  kicad-pcb-layout-agent.md`.
- **Docs:** `docs/PHOTO_TO_BOARD_WORKFLOW.md` (new), `README.md` (linked).
- **Non-Goals (this change):** perfect photogrammetric reconstruction; inner
  copper layers; any silent invention (every dossier/design-brief claim not
  directly observed is labelled inference, with evidence and, where numeric,
  a stated calculation and assumptions); no `.kicad_sch`/`.kicad_pcb`/
  `.kicad_pro` mutation from `pcb-photo-intake-agent` or
  `pcb-design-reconstruction-agent` — that stays with
  `kicad-schematic-build-agent`/`kicad-pcb-layout-agent`, unchanged from the
  archived slice; no change to `retrace`'s own scan/parse behavior, trace
  extraction, or net inference (still Slice 2 of the archived change, still
  out of scope here); no server-side enforcement of the gate inside
  `sch_*`/`pcb_*` handlers (still deferred, per the archived change).
