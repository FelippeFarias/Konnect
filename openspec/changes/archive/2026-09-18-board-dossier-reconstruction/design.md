## Context

This change extends the `photo_intake` toolset and its assets, shipped and
archived by `photo-to-kicad-reverse` (`openspec/changes/archive/
2026-09-18-photo-to-kicad-reverse/design.md`, decisions D1-D16; base spec now
at `openspec/specs/photo-intake/spec.md`). That slice's own reference-photo
test (2026-09-18, traffic-light LED module, WhatsApp `.jpeg`) showed retrace
alone cannot carry this: stock `yolov8n.pt` mislabels LED domes, copper
segmentation only sees bare copper (mask-covered traces are invisible), and
the run produced 0 nets, 0 markings, coarse merged boxes. A manual
concept-validation run against the same photos
(`.orchestrator/handoffs/board-dossier-reconstruction/
00-orchestrator-dossier-prototype.md`) proved the alternative works: an LLM
reading zoomed crops itself — not retrace — read the silkscreen identity,
counted components by visual class with a stated method, and produced
labelled hypotheses for what retrace's boxes alone could not resolve
(string length). That document is this change's worked example; every claim
type in it (observed silkscreen text with location, counted inventory with
method and disagreeing alternatives, physical constraints with an unresolved
scale, competing numbered hypotheses with calculations, and a design-brief
seed) has a named field in the schemas below.

The five existing tools, the review-map schema (archived D8), the approval
state machine (archived D5), and the content hash (archived D16) are kept
exactly as they behave today; this change is additive to all three except
where a decision below says otherwise (D8's `scale_reference`, D16's covered
fields).

This design's own decisions are labeled D1-D16, distinct from the archived
change's D1-D16, which are cited by number with "archived" when referenced.

Every file:line citation below was read in the implementation worktree
`C:\Users\felip\.orc\worktrees\konnect-3b7e2022\board-dossier-reconstruction`
at base commit `1815948`.

## Goals / Non-Goals

**Goals:**
- Give the intake agent a way to look at the photos itself at the
  resolution it needs (`prepare_board_photo`), so retrace's boxes become one
  input among several rather than the only one.
- Persist a structured, evidence-and-confidence-tagged board dossier as an
  additive review-map section, human-approved before any design work starts.
- Persist a structured design brief (topology, calculated values, a real
  KiCad-library BOM, physical constraints) as a second additive section,
  human-approved before `kicad-schematic-build-agent`/
  `kicad-pcb-layout-agent` consume it.
- Reuse the existing approval mechanism for both checkpoints — no new tool,
  no new state machine.
- Provide one top-level workflow skill (`kicad-photo-to-board`) that a
  session can follow end to end, naming every stage's inputs, outputs, human
  gate, and `INCOMPLETE` fallback.

**Non-Goals:**
- Any change to `retrace`'s own scan/parse behavior, trace extraction, or net
  inference — still Slice 2 of the archived change.
- Inner copper layers, or any claim about them.
- Server-side enforcement of the approval gate inside `sch_*`/`pcb_*` tool
  handlers — still deferred (archived change's Non-Goals).
- `.kicad_sch`/`.kicad_pcb`/`.kicad_pro` mutation from `pcb-photo-intake-agent`
  or `pcb-design-reconstruction-agent` — build and layout stay with
  `kicad-schematic-build-agent`/`kicad-pcb-layout-agent`.
- Any automatic resolution of a disagreement between two dossier hypotheses
  or two counting methods — the schema carries both; a human or a later
  measurement resolves it, not this change.
- Any image-analysis algorithm inside `prepare_board_photo`. It decodes,
  orients, crops, rotates, scales and re-encodes. Counting, classification
  and OCR are the LLM's job, from the views this tool produces.

## Decisions

### D1. `prepare_board_photo`: contract, bounds, and where it lives

Sixth tool in `crates/konnect-core/src/tools/photo_intake.rs`, registered
beside the existing five in `photo_intake::tools()` (`photo_intake.rs:1158`);
`ALL_TOOLSETS`'s `photo_intake` entry moves `tool_count` from `5` to `6`
(`router/registry.rs:123`; cap is 20 per toolset, archived D2).

| Field | Type | Notes |
|---|---|---|
| `image_path` (**required**) | string | canonicalized, existing file — same rule as `scan_pcb_photo`'s `image_path` (archived D13 rule 5, `canonical_existing_file`, `photo_intake.rs:623`); not confined to `project_dir` |
| `project_dir` (**required**) | string | canonicalized existing directory (`canonical_existing_dir`, `photo_intake.rs:612`; archived D13 rule 1) |
| `map_id` (**required**) | string, `^[A-Za-z0-9_-]{1,64}$` | must name a directory that already exists — see D8 |
| `crop` | object `{x,y,w,h}` (px, integers; `x`/`y` ≥ 0, `w`/`h` ≥ 1) | omitted = full oriented image |
| `rotate` | integer, `enum [0, 90, 180, 270]` | applied after crop, on top of any EXIF orientation already applied |
| `scale` | number, `minimum: 0.25`, `maximum: 4.0` | applied after rotate |
| `label` | string, `^[A-Za-z0-9_-]{1,64}$` | optional; same token rule as `map_id` |

Output: `view_path` (the saved PNG, under `<canonical project_dir>/.konnect/
photo_intake/<map_id>/views/<label-or-n>.png` — `n` is `1 + ` the current
count of files already in that `views/` directory when `label` is omitted),
`source_size_px` (`[w,h]` of the decoded image **after** EXIF orientation),
`source_rect_px` (`[x,y,w,h]` actually used, in that same oriented space — the
full image's dimensions when `crop` was omitted), `output_size_px` (`[w,h]` of
the saved PNG), `exif_orientation` (the `image::metadata::Orientation` variant
name that was applied, e.g. `"NoTransforms"`/`"Rotate90"`), and `mm_per_px` —
present only when D3's `scale_reference` on the currently saved map has a
resolved value, absent otherwise (never estimated by this tool).

**Pipeline, with the API verified against `image` 0.25.10 as locked
(`Cargo.lock:2349-2351`):**

1. `image::ImageReader::open(path)?.with_guessed_format()?`
   (`image_reader_type.rs:344`/`:254`).
2. `.limits(Limits { max_alloc: Some(512 * 1024 * 1024), ..Limits::no_limits()
   })` — set explicitly rather than relying on the crate's default, so the cap
   this design states is the cap in the code (`image_reader_type.rs:137`,
   `io/limits.rs:35-42`, `:62`).
3. `.into_decoder()?` (`image_reader_type.rs:219`), then `decoder.dimensions()`.
   If `w as u64 * h as u64 > 50_000_000` (50 MP), return
   `CallToolResult::error` naming the actual `w x h` and the cap, **before**
   decoding a pixel.
4. `let orientation = decoder.orientation()?` (`io/decoder.rs:53` — defaults to
   `Orientation::NoTransforms` for a format or file with no EXIF tag, so this
   call is safe on a WhatsApp JPEG that has been stripped).
5. `DynamicImage::from_decoder(decoder)?` (`images/dynimage.rs:243`), then
   `image.apply_orientation(orientation)` (`dynimage.rs:1161`).
6. Bounds-check `crop` against the **oriented** dimensions. An out-of-bounds
   `crop` is a `CallToolResult::error` naming the image's actual oriented size,
   never a panic and never a silently clamped rectangle. Then
   `crop_imm(x, y, w, h)` (`dynimage.rs:508`).
7. `rotate90()`/`rotate180()`/`rotate270()` (`dynimage.rs:1116`/`:1124`/
   `:1135`); `0` is a no-op.
8. Compute the output size as `round(w * scale) x round(h * scale)`, each
   clamped to a minimum of 1 px. If either side exceeds **4096 px**, return an
   error naming the computed size and the cap — checked before allocating the
   destination buffer. Then `resize_exact(nw, nh, FilterType::Lanczos3)`
   (`dynimage.rs:892`) so the reported `output_size_px` is exactly what was
   written, with no aspect-ratio rounding of its own.
9. `save(view_path)` (`dynimage.rs:1394`), PNG by extension — the same encoder
   path `konnect-render` already uses.

**Why these bounds, and why no timeout.** The handler is synchronous
CPU-and-memory work with no network and no subprocess, so a wall-clock timeout
would guard nothing a size cap does not already guard. 50 MP at RGBA8 is
200 MB decoded; a 4096 x 4096 output is 67 MB; peak resident is therefore
under ~300 MB per call, which the 512 MB `max_alloc` backstops for a header
that lies about its own dimensions. `scale` is **rejected** outside
`0.25..=4.0`, not clamped: a silently clamped scale makes `output_size_px`
disagree with what the agent asked for, and the agent then reasons about pixel
coordinates in a space that does not exist.

*Rejected alternative (EXIF):* ignore orientation entirely and document
`rotate` as the user's tool for upside-down photos. Rejected because
orientation is exactly what makes a `crop` rectangle land on the wrong part of
the board: a phone original carries the tag, the reader the agent used to pick
the rectangle may or may not have honoured it, and the agent cannot detect the
mismatch from the cropped result. Applying the tag first and reporting
`exif_orientation` and `source_size_px` makes the coordinate space the tool
used inspectable, and leaves `rotate` for the residual case (a photo taken
sideways with no tag at all).

*Rejected alternative (batching):* let the agent request multiple
crops/rotates/scales in one call. Rejected because every other `photo_intake`
tool is one image in, one artifact out (archived D2's table), and a batch
complicates the out-of-bounds/error contract (partial success across a batch)
for a convenience the agent can already get by calling the tool once per view
— the real board photos in the reference case needed roughly half a dozen
zoomed views, not dozens.

### D2. `image` crate: dev-dependency to regular dependency, `jpeg` feature added

Root `Cargo.toml:90` changes from
`image = { version = "0.25", default-features = false, features = ["png"] }`
to `features = ["png", "jpeg"]` — `prepare_board_photo` must decode the real
board photos, which are JPEG (the reference case's WhatsApp captures), and
still encodes PNG views. The feature is named `jpeg` and pulls
`dep:zune-core` + `dep:zune-jpeg` (verified in the locked crate's
`[features]` table, `image-0.25.10/Cargo.toml`); it is a pure-Rust decoder, so
it adds no system dependency to the build.

`crates/konnect-core/Cargo.toml` moves `image.workspace = true` from
`[dev-dependencies]` (line 47, added by archived D7 for the `#[ignore]`d
live-scan test's synthetic PNG helper) to `[dependencies]`; the dev-only line
and its two-line comment are deleted, not duplicated — a crate available to
the whole build is trivially available to its own tests.
`crates/konnect-render`'s two `image.workspace = true` lines
(`Cargo.toml:12` and `:16`) are untouched: Cargo feature unification means the
`jpeg` feature reaches every consumer of the workspace `image` dependency in
one build regardless, so `konnect-render` gains JPEG-decode capability as a
side effect — a strict widening of what it can already do (decode-only feature
addition, no API or behavior change to any existing `konnect-render`
function), not a behavior change worth gating separately.

*Rejected alternative:* keep `image` a dev-dependency and hand-roll a minimal
JPEG baseline decoder, or shell out to `kicad-cli`'s bundled tooling.
Rejected for the same reason archived D7 rejected hand-rolling PNG bytes: an
untested decoder between the tool and its subject makes a decode bug
indistinguishable from a photo problem, and `kicad-cli` has no image-decode
subcommand to shell out to.

### D3. Resolved `scale_reference`

`ScaleReference` (`photo_intake.rs:709-713`) gains two optional fields,
populated only when the agent ties a scale to a named physical feature rather
than the user stating a millimeter value directly:

```json
"scale_reference": {
  "kind": "board_edge_mm | package | mounting_hole_pitch",
  "value": "the physical value relied on, however it was obtained",
  "mm_per_px": 0.24,
  "evidence": "mounting-hole pitch measured at 21px between M3 hole centers in boardA_top.png, stated by the user as 5mm actual pitch"
}
```

Both new fields are `#[serde(default, skip_serializing_if = "Option::is_none")]
Option<...>` — see D6 for why `skip_serializing_if` is load-bearing here and
not cosmetic. `mm_per_px`/`evidence` are always a pair: an agent that cannot
name evidence for a millimeter value MUST leave both absent (spec MODIFIED
requirement, "agent-resolved scale reference is recorded with evidence").
`kind` gains `mounting_hole_pitch` as a documented value alongside the two
archived D8 values; the schema does not enumerate `kind` (`review_map_schema`,
`photo_intake.rs:1094` — a free-form string with an `e.g.` description), so
this is a documentation change, not a schema change.

### D4. Board dossier schema

Optional `dossier` object on the review map, keyed at the top level beside
`components`/`nets`/`scale_reference` (archived D8), additive per the spec's
"board dossier section of the review map" requirement.

**Evidence pointers (the shape used everywhere `evidence` appears).** Every
`evidence` entry is an object, never a sentence, so a reviewer can open it:

```json
{ "view": "boardA_zoom3.png", "rect_px": [30, 430, 180, 24], "note": "silkscreen line, bottom edge" }
```

- `view` (**required**) is either a bare file name inside the map's `views/`
  directory (a `prepare_board_photo` output) or a string equal to one of the
  map's `source_images` entries. Nothing else is a valid `view`.
- `rect_px` (**required**) is `[x, y, w, h]` in the coordinate space of that
  view — for a `views/` file, the space `prepare_board_photo` reported as
  `output_size_px`; for a `source_images` entry, the oriented source space it
  reported as `source_size_px`.
- `note` is optional prose.

```json
"dossier": {
  "identity": {
    "summary": "24 V LED traffic-light lamp module (silkscreen 'SEMAFARO 1.3 24V 03/2020')",
    "basis": "observed",
    "confidence": 0.95,
    "evidence": [ { "view": "boardA_zoom3.png", "rect_px": [30, 430, 180, 24], "note": "silkscreen identity line" } ]
  },
  "physical": {
    "board_size_px": [410, 445],
    "board_size_mm": null,
    "scale_status": "unresolved — needs hole pitch or board edge mm from the user, or a resolved scale_reference",
    "mounting_holes": [
      { "position_px": [18, 20], "role": "plated_corner", "count": 4, "evidence": [ { "view": "boardA_top.png", "rect_px": [8, 10, 24, 24] } ] },
      { "position_px": [380, 15], "role": "unplated", "count": 1, "evidence": [ { "view": "boardA_top_topright_resistors_hole.png", "rect_px": [120, 4, 20, 20] } ] }
    ],
    "connectors": [
      { "type": "2-pin screw terminal, 5.08mm pitch", "location_px": [20, 420], "edge": "bottom", "basis": "observed", "confidence": 0.9, "evidence": [ { "view": "boardA_bottom.png", "rect_px": [10, 400, 90, 60] } ] }
    ],
    "layers_visible": "top silkscreen + bottom copper only; inner layers out of scope"
  },
  "component_survey": [
    {
      "visual_class": "led_5mm_clear",
      "count": 107,
      "count_method": "blob_count_hsv",
      "count_confidence": 0.8,
      "count_alternatives": [ { "method": "hough_circles", "count": 63, "note": "undercounts on a staggered grid" } ],
      "locations": [ { "region": "full board", "view": "boardA_top.png", "count": 107 } ],
      "retrace_component_ids": [],
      "notes": "silkscreen 'LEDnn' per position"
    },
    {
      "visual_class": "axial_resistor_tht",
      "count": 19,
      "count_method": "manual_count_by_region",
      "count_confidence": 0.9,
      "locations": [
        { "region": "top-left", "view": "boardA_top_topleft_resistors.png", "count": 5 },
        { "region": "top-right", "view": "boardA_top_topright_resistors_hole.png", "count": 5 },
        { "region": "near terminal", "view": "boardA_bottom_left.png", "count": 3 },
        { "region": "bottom-right", "view": "boardA_bottom_right.png", "count": 3 },
        { "region": "right edge", "view": "boardA_right.png", "count": 3 }
      ],
      "notes": "colour bands not legible at this resolution"
    }
  ],
  "silkscreen_markings": [
    { "text": "SEMAFARO 1.3 24V 03/2020", "location_px": [30, 440], "basis": "observed", "confidence": 0.95, "evidence": [ { "view": "boardA_zoom3.png", "rect_px": [30, 430, 180, 24] } ] },
    { "text": "24V", "location_px": [15, 410], "basis": "observed", "confidence": 0.9, "evidence": [ { "view": "boardA_bottom_left.png", "rect_px": [8, 396, 60, 28] } ] }
  ],
  "topology_claims": [
    {
      "claim_id": "string-length",
      "question": "How many LEDs per series string, and how many strings?",
      "basis": "inferred",
      "evidence": [ { "view": "boardA_bottom.png", "rect_px": [40, 60, 380, 340], "note": "serpentine mask-covered traces chaining LED pads — establishes series topology, not string length" } ],
      "hypotheses": [
        {
          "label": "A",
          "description": "19 strings x 6 LEDs + 1 series resistor each",
          "confidence": 0.6,
          "calculation": "6 x Vf(2.0-2.2V) = 12.0-13.2V; 24V - 12.6V(avg) = 11.4V across R at 20mA -> ~570 ohm; nearest E24 620 ohm; P = I^2R = 0.25W -> use 1/2W axial",
          "assumptions": ["Vf averaged at 2.1V for the calc", "target 20mA per string", "24V supply regulated externally"]
        },
        {
          "label": "B",
          "description": "fewer, longer strings (8-10 LEDs), correspondingly fewer strings",
          "confidence": 0.3,
          "calculation": null,
          "assumptions": ["resistor count need not equal string count if some strings share a resistor"]
        }
      ],
      "resolution_path": "count LEDs along one continuous serpentine trace on the bottom-copper view, or test continuity, to resolve string length directly"
    }
  ],
  "retrace_correlation": [
    { "component_id": "C0031", "bbox_px": [100, 120, 20, 20], "visual_class": "led_5mm_clear", "overlap_confidence": 0.7 }
  ],
  "photo_views_used": ["boardA_top.png", "boardA_bottom.png", "boardA_zoom3.png"],
  "design_brief_seed": {
    "summary": "N strings of 6x 5mm LEDs + 1x ~620R 1/2W axial resistor per string; 1x 2-pin 5.08mm screw terminal; 4x M3 mounting holes; board ~100x100mm (unresolved); single-sided routing feasible in principle (source is 2-layer, bottom traces only)",
    "depends_on_open_questions": ["string-length"]
  },
  "open_questions": [
    "exact board size in mm — scale reference not yet supplied",
    "resistor color bands unreadable — value derived from the Vf/I calculation, not read off the part",
    "string length (6 vs 8-10 LEDs) — see topology_claims.string-length"
  ]
}
```

Every claim-bearing object carries `basis` (`observed` | `inferred`) and a
numeric `confidence` — the categorical/numeric pair the archived change
never needed (its components/nets are always `observed`-only, machine-read
fields) and this dossier always does, because an LLM survey mixes both in
one document and a reviewer must be able to tell which is which at a glance,
not infer it from prose. `topology_claims[].hypotheses` is an array, not a
single `claim`/`calculation` pair, specifically so a question the evidence
does not resolve keeps both candidates on the record with their own
confidences, rather than the writing agent silently picking one (Non-Goal:
no automatic resolution). `component_survey[].count_alternatives` exists for
the same reason on the counting side — two methods disagreeing is itself
evidence, and collapsing it to the trusted number's `count` field alone
throws that away.

*Rejected alternative (evidence as prose):* keep `evidence` a list of
sentences, as the concept-validation run wrote them by hand. Rejected because
the whole point of the gate is that a human checks a claim against the pixels
it came from; a sentence makes that a search, and a reviewer who cannot cheaply
open the evidence approves on the claim's tone instead. `{view, rect_px}` is
also exactly what `prepare_board_photo` already returns, so producing it costs
the agent nothing it did not already compute.

### D5. Design brief schema

Optional `design_brief` object, additive beside `dossier`:

```json
"design_brief": {
  "derived_from_dossier": true,
  "block_diagram": [
    { "block": "power_input", "function": "24V DC input, reverse-polarity/fusing", "inputs": ["24V_IN", "GND"], "outputs": ["24V_RAIL"] },
    { "block": "led_string_1", "function": "6x red LED series string with current-limit resistor", "inputs": ["24V_RAIL", "GND"], "outputs": [] }
  ],
  "circuits": [
    {
      "block": "led_string_1",
      "description": "6 LEDs in series + 1 series resistor from 24V_RAIL to GND",
      "calculated_values": [
        { "parameter": "R1", "value_ohms": 620, "formula": "(24V - 6*2.1V) / 0.02A", "assumptions": ["Vf=2.1V avg", "I=20mA"] }
      ],
      "derating_notes": "resistor power = I^2*R = 0.25W; use 1/2W axial for margin"
    }
  ],
  "bom": [
    {
      "role": "LED (5mm, clear lens, through-hole)",
      "kicad_symbol": "Device:LED",
      "kicad_footprint": "LED_THT:LED_D5.0mm",
      "resolution_status": "resolved",
      "search_terms_used": ["LED", "LED_D5.0mm"],
      "candidates": [],
      "value": "red/green/yellow per position, Vf~2.0-3.4V",
      "quantity": 107,
      "source": "matched"
    },
    {
      "role": "2-pin 5.08mm screw terminal",
      "kicad_symbol": null,
      "kicad_footprint": null,
      "resolution_status": "unresolved",
      "search_terms_used": ["screw terminal", "TerminalBlock 5.08"],
      "candidates": [
        { "kicad_symbol": "Connector_Generic:Conn_01x02", "kicad_footprint": "TerminalBlock:TerminalBlock_bornier-2_P5.08mm", "why": "pitch matches; body style not verified against the photo" }
      ],
      "value": "5.08mm pitch, 2 positions",
      "quantity": 1,
      "source": "matched"
    }
  ],
  "physical_constraints": {
    "board_size_mm": null,
    "board_size_status": "pending scale resolution — see dossier.physical.scale_status",
    "mounting_holes": [ { "position_mm": null, "position_px": [18, 20], "diameter_mm": 3.2 } ],
    "enclosure": null,
    "max_component_height_mm": null,
    "connector_edges": [ { "edge": "bottom", "type": "2-pin screw terminal", "pitch_mm": 5.08, "position_px": [20, 420] } ],
    "user_facing_parts": [ { "role": "LED array", "why_user_facing": "the lamp face; must stay on the top side and inside the lens aperture" } ],
    "net_currents": [ { "net": "24V_RAIL", "continuous_a": 0.38, "peak_a": null, "basis": "inferred", "note": "19 strings x 20mA" } ],
    "net_voltages": [ { "net": "24V_RAIL", "nominal_v": 24, "surge_v": null, "basis": "observed", "note": "silkscreen '24V'" } ],
    "signal_speeds": [],
    "sensitive_nets": [],
    "layer_count": null,
    "stackup": null,
    "fabricator": null,
    "assembly_notes": null,
    "keep_outs": [],
    "unresolved": ["board_size_mm", "enclosure", "max_component_height_mm", "layer_count", "stackup", "fabricator", "assembly_notes"]
  },
  "assumptions": ["Vf averaged at 2.1V", "target 20mA per string", "24V supply regulated externally"],
  "open_questions": ["string length not yet resolved — see the source dossier's topology_claims"]
}
```

**BOM library resolution.** `kicad_symbol` carries a KiCad **lib_id**
(`Library:Symbol`, e.g. `Device:LED`) and `kicad_footprint` a footprint id
(`Library:Footprint`). Both are obtained by calling `search_symbols`
(`library.rs:349`) / `search_footprints` (`library.rs:392`) — never a
remembered or plausible-looking id — per the spec's "every BOM entry names a
real library part" scenario. An entry the search does not settle is written
with `resolution_status: "unresolved"`, `kicad_symbol`/`kicad_footprint` both
`null`, `search_terms_used` naming what was tried, and `candidates[]` holding
the near misses with a `why`. **Writing a non-null `kicad_symbol` or
`kicad_footprint` that no search returned is the single forbidden act of this
schema**; `kicad-schematic-build-agent` reports `INCOMPLETE` for every
`unresolved` entry rather than placing a symbol from a candidate.

*Rejected alternative (field naming):* rename the pair to `lib_id` /
`footprint`, which is what KiCad and this project's own tool arguments call
them. Rejected because `specs/photo-intake/spec.md` already cites
`kicad_symbol`/`kicad_footprint` by name in two scenarios ("every BOM entry
names a real library part"; "schematic build places one symbol per BOM
entry"), and a design that renames a spec-cited path makes one of the two
wrong. The requirement — a resolvable symbol lib_id and a resolvable footprint
id, per component — is met by value; only the spelling defers to the spec.

**`physical_constraints` is the layout constraint record, one field per row of
`crates/konnect/assets/skills/kicad-pcb/references/layout-methodology.md:28-38`**,
so `kicad-pcb-layout-agent` can fill its section-1 record from the brief
without a translation step:

| layout-methodology.md row (`:30`-`:38`) | `physical_constraints` field(s) |
|---|---|
| Board dimensions, holes, enclosure, available height | `board_size_mm`, `board_size_status`, `mounting_holes[]`, `enclosure`, `max_component_height_mm` |
| Connector positions and access to controls | `connector_edges[]`, `user_facing_parts[]` |
| Continuous current, peaks, and transients per net | `net_currents[]` |
| Operating voltages and possible surges | `net_voltages[]` |
| Frequencies and rise/fall times | `signal_speeds[]` |
| Signal sensitivity | `sensitive_nets[]` |
| Layer count and stackup | `layer_count`, `stackup` |
| Fabricator capability | `fabricator` |
| Assembly and test process | `assembly_notes` |
| (not a row — this change's addition) | `keep_outs[]` |

Every row is always present as a key. A row with no answer is `null` (or `[]`)
**and** is named in `physical_constraints.unresolved`, which is what
distinguishes "asked, unknown" from "never considered" — the distinction
layout-methodology.md:53-57 makes when it demands "a written constraint record
covering every row above, with 'unknown — asked the user' where the answer is
missing". `kicad-pcb-layout-agent` reports `INCOMPLETE` rather than proceeding
when `unresolved` contains any of the four rows that file calls load-bearing
(`layout-methodology.md:56-57`: current, voltage, connector position,
enclosure) or `board_size_mm`, and proceeds with a stated assumption for the
rest. `physical_constraints` carries pixel-space values alongside millimeter
ones (mirroring `dossier.physical`) so a brief can be written and reviewed
before a scale is resolved.

*Rejected alternative:* a free-form `physical_constraints` blob plus prose in
the skill telling the layout agent what to look for. Rejected because the
layout agent's own completion criterion is a per-row record; a blob makes
"which row is missing" a judgement call at exactly the moment the agent is
deciding whether it may place parts, and an agent that cannot name the missing
row reports `DONE` with a guessed board size.

### D6. Content-hash coverage: conditional inclusion, whole-section coverage, old digests unchanged

This is the decision the predecessor's own pre-mortem named as its most
dangerous ("a fail-open key list"), so it is stated as code shapes, not prose.

**Where the change goes.** `review_map_content_hash` (`photo_intake.rs:860`)
builds a fresh object by iterating `CONTENT_KEYS` (`:749`) and inserting
**every** covered key, defaulting to `Value::Null` (`:862-880`). The
conditional behaviour therefore belongs in *that loop*, not in `hashed_fields`
(`:800`) or `hashed_elements` (`:820`). `hashed_fields`' unconditional
`covered.insert(key, … unwrap_or(Null))` (`:810-814`) **must not change**:
making it skip absent keys would drop the `"ref": null` member every
component without a reference designator contributes today, which changes the
canonical bytes of every existing map and invalidates every approval in the
field. The planner's draft of this decision named those two helpers as the
edit site; that is the bug this paragraph exists to prevent.

**The three edits:**

```rust
/// Sections this change adds to the gate. Covered only when the saved map
/// actually carries them, so a map written before this change — which never
/// had them — hashes to exactly the bytes it always did.
const OPTIONAL_CONTENT_KEYS: [&str; 2] = ["dossier", "design_brief"];

/// D3's additions to the scale reference. Same conditional rule, same reason.
const SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS: [&str; 2] = ["mm_per_px", "evidence"];

/// [`hashed_fields`] plus keys that are covered only when present. Separate
/// from `hashed_fields` on purpose: that function's unconditional insert is
/// what keeps an absent `ref`/`value` contributing a `null` member, and every
/// stored digest depends on it.
fn hashed_fields_with_optional(
    value: &serde_json::Value,
    keys: &[&str],
    optional_keys: &[&str],
) -> serde_json::Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let serde_json::Value::Object(mut covered) = hashed_fields(value, keys, &[]) else {
        return value.clone();
    };
    for key in optional_keys {
        if let Some(field) = object.get(*key) {
            covered.insert((*key).to_string(), field.clone());
        }
    }
    serde_json::Value::Object(covered)
}
```

1. In `review_map_content_hash`'s `match`, `"scale_reference"` calls
   `hashed_fields_with_optional(value, &SCALE_REFERENCE_CONTENT_KEYS,
   &SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS)` instead of `hashed_fields(...)`.
2. After the `CONTENT_KEYS` loop, a second loop:
   `for key in OPTIONAL_CONTENT_KEYS { if let Some(section) = map.get(key) {
   covered.insert(key.to_string(), section.clone()); } }`.
3. `every_schema_key_is_either_hashed_or_deliberately_not`
   (`photo_intake.rs:2751`) asserts the fixture map's key set equals
   `CONTENT_KEYS ∪ UNHASHED_KEYS` (`:2759-2767`). It gains
   `.chain(OPTIONAL_CONTENT_KEYS.iter())`. This is the guard that makes a
   future section added to the schema without a hash decision a **build
   failure**, and it is why the new list is a named `const` rather than two
   inline string literals.

**`dossier` and `design_brief` are hashed whole — `section.clone()`, no field
projection.** The archived projection onto `COMPONENT_CONTENT_KEYS`/
`NET_CONTENT_KEYS`/`SCALE_REFERENCE_CONTENT_KEYS` exists to keep two classes
of field out of the gate: bookkeeping the tools write themselves, and reviewer
annotations nested inside a machine-written record (`photo_intake.rs:837-848`).
Neither class exists inside `dossier`/`design_brief`: no tool writes into
them (`TOOL_OWNED_KEYS`, `:1493`, is a top-level list, and
`prepare_board_photo` writes a PNG, never the map), and there is no
machine-written record for a human to annotate — the sections *are* the
reviewed content. A projection would therefore buy nothing and cost the exact
failure mode the predecessor warned about: a field added to D4/D5 and
forgotten in the key list is silently outside the gate, and the "iterate the
key list against the struct" test that would catch it cannot exist, because
`dossier`/`design_brief` are `serde_json::Value`-shaped open objects with no
struct to iterate. Whole-section coverage is fail-closed by construction and
has no list to drift.

*Rejected alternative:* an explicit nested covered-key projection for
`dossier`/`design_brief` (`DOSSIER_CONTENT_KEYS`, `DOSSIER_PHYSICAL_CONTENT_
KEYS`, `COMPONENT_SURVEY_CONTENT_KEYS`, …), mirroring the archived pattern.
Rejected on the reasoning above. The guard the orchestrator asked for — a test
that fails when a schema field escapes hash coverage — is provided instead by
edit 3 at the section level, where it can actually be enforced, plus the
round-trip test in task 2.4 case (5).

**The `skip_serializing_if` landmine.** `PhotoReviewMap` (`:675`) gains
`#[serde(default, skip_serializing_if = "Option::is_none")] pub dossier:
Option<serde_json::Value>` and the same for `design_brief` — exactly the
attribute pair `subcircuit_hints` already carries (`:691-692`) and for exactly
the reason stated there. `handle_save_photo_review_map` writes
`overlay_known_fields(without_tool_owned_keys(map_arg), to_value(&parsed))`
(`:1553-1556`), so a field serialized as `null` by the struct is **inserted
into the record on disk**. Without `skip_serializing_if`, every save of a map
that has no dossier would write `"dossier": null`, `map.get("dossier")` would
return `Some(Null)`, the conditional insert would fire, and every pre-existing
approved map would be revoked on its next save — the precise outcome this
decision exists to prevent. `a_review_map_round_trips_and_tolerates_absent_
subcircuit_hints` (`:2773-2788`) is the existing test of this property; task
2.4 case (5) extends the same assertion to the two new fields.

`validate_incoming_map` (`:1033`) rejects a `dossier` or `design_brief` that is
present but not a JSON object — `null` included — with a message telling the
caller to **omit** the key to remove the section. That removes the "null vs
absent" ambiguity from the gate entirely: on disk, a section is either an
object or not there.

**Proof that the gate closes in both directions.** Write `H(M)` for the
canonical bytes `review_map_content_hash` serializes.

- *Approved without a dossier, then a dossier is added.* `H(M)` has members
  `components`, `nets`, `scale_reference`, `source_images` and no `dossier`
  member at all. `H(M')` has all of those **plus** a `dossier` member, so the
  byte strings differ and the SHA-256 digests differ.
  `handle_save_photo_review_map` recomputes the hash before writing and keeps
  `approved` only when the stored `content_hash_at_approval` equals it
  (`:1557-1567`), so `approved` becomes `false`, `approved_at` and
  `content_hash_at_approval` are cleared, and `approval_is_valid` (`:890`)
  answers `false` for every later consumer. The same argument applies verbatim
  to `design_brief`, and to `scale_reference` gaining `mm_per_px`/`evidence`
  (the scale reference's projected object gains two members).
- *Approved with a dossier, then the dossier is removed.* The approval was
  recorded over `H(M')`, which contains a `dossier` member. After the removing
  save, the record is `H(M)`, which does not. `H(M) ≠ H(M')`, so approval is
  revoked. A removal is a real removal, because the object written is
  `overlay_known_fields` over the *incoming* map (`:1553`), not a merge onto
  the stored one — a key the caller omits is genuinely absent afterwards.
- *Never had one, never gains one.* Neither conditional insert fires, the
  scale reference projects to exactly `{kind, value}` as before, and the bytes
  are identical to what the function produced before this change. The archived
  pinned-digest unit test (archived tasks 3.6) therefore needs **no change**,
  and that is the acceptance criterion of task 2.3.

Key **insertion** order is irrelevant to all of this: `serde_json::Map` is a
`BTreeMap` because `preserve_order` is off workspace-wide, so `to_vec` emits
members sorted at every depth — the dependency the function's own doc comment
already states (`:854-859`). This design adds no new assumption; it inherits
that one, and inherits its failure mode (see Pre-mortem cause 2).

This conditional-inclusion rule is also the mechanism behind the two
sequential approval checkpoints (spec "hard human-review gate" MODIFIED
requirement): approving while only `dossier` is present covers exactly that
content; adding `design_brief` afterward changes what the hash covers, so the
stored `content_hash_at_approval` no longer matches, `approved` resets to
`false` on the next save (existing archived D5 behavior, unmodified), and a
second `approve_photo_review_map` call is required — with no new field, tool,
or state to track.

*Rejected alternative:* a `dossier_approved`/`design_brief_approved` pair of
booleans, independent of the existing `approved` flag. Rejected because it
duplicates the entire state machine archived D5 already built (explicit
call required, hash re-check, edit-revokes-approval) for a distinction
(which section was approved) that conditional hash coverage already gives
for free, and a second machine is a second place the two can drift apart.

*Rejected alternative:* accept the break — hash the new fields
unconditionally and update the archived pinned-digest literal with a stated
reason. Rejected because nothing about existing approved maps justifies
invalidating them for a field they never had; the "a gate people click through
is worse than no gate" argument archived D16 already made about hashing raw
file bytes applies just as directly to revoking an approval over a field the
map was never asked about.

### D7. Review-map schema shape and the open-record allowlist

`fixed_records_are_closed_and_only_reviewed_maps_are_extensible`
(`crates/konnect-core/src/router/mod.rs:185`) walks every registered tool's
input schema (`:257-261`). It treats a node as a *record* when
`type == "object"`, when `type` is an array containing `"object"`, or when the
node has `properties`/`patternProperties` (`:187-193`), demands an explicit
`additionalProperties` on every such node (`:194-196`, an `.expect` — a
missing policy is a panic, not a nice failure), and asserts any node whose
policy is not `false` appears in a hard-coded allowlist (`:198-216`). It
recurses through `properties`/`patternProperties`/`$defs`/`definitions`/
`dependentSchemas` (`:220-232`), through `allOf`/`anyOf`/`oneOf`/`prefixItems`
(`:233-239`), and through `items`/`additionalProperties`/`contains`/
`propertyNames`/`not`/`if`/`then`/`else`/`unevaluated*` (`:240-255`), building
a path like `save_photo_review_map/properties/map/properties/dossier`.

**Decision: `review_map_schema()` (`photo_intake.rs:1070`) declares the two new
sections' object-valued nodes with `properties`, and declares every
array-of-objects as `{"type": "array", "description": "<element shape in
prose>"}` with no `items` subschema** — the shape `subcircuit_hints` already
uses (`:1146-1149`). The walker never descends into an array with no `items`,
so the number of new allowlist paths is finite, known now, and does not grow
when D4/D5 gain an element field. The per-field documentation lives where the
writing agent actually reads it: `kicad-board-dossier/references/
dossier-schema.md` (task 3.2) and `kicad-design-reconstruction/references/
design-brief-schema.md` (task 3.4).

The allowlist therefore gains **exactly these six paths**, each with the same
one-line "a human edits this by hand between two tool calls" rationale as its
neighbors (`router/mod.rs:205-213`):

```
"save_photo_review_map/properties/map/properties/dossier",
"save_photo_review_map/properties/map/properties/dossier/properties/identity",
"save_photo_review_map/properties/map/properties/dossier/properties/physical",
"save_photo_review_map/properties/map/properties/dossier/properties/design_brief_seed",
"save_photo_review_map/properties/map/properties/design_brief",
"save_photo_review_map/properties/map/properties/design_brief/properties/physical_constraints",
```

These are every singleton object node in D4/D5:
`dossier.{identity, physical, design_brief_seed}` and
`design_brief.physical_constraints`, plus the two section roots. Everything
else in D4/D5 is either a scalar, a string array, or an array of objects
declared without `items`. If the test names a seventh path, the schema has an
`items` subschema or a nested object the implementer added beyond D4/D5 — the
fix is to fold that node's field list into the reference doc and drop the
subschema, or to add the path with its own rationale line, never to relax
`additionalProperties` on a record the test does not already allow.

*Rejected alternative:* enumerate D4/D5 fully in `review_map_schema()`,
including `items` subschemas for `component_survey`, `topology_claims`,
`hypotheses`, `bom`, `mounting_holes`, `connectors`, `locations`,
`count_alternatives`, `calculated_values`, `connector_edges`,
`net_currents`, … Rejected because it turns a six-entry allowlist into a
twenty-plus-entry one that must be re-derived by test failure every time a
schema field moves, and buys no validation: every one of those nodes would be
`additionalProperties: true` anyway, so the schema would constrain nothing
while making the guard test the de-facto schema documentation. The planner's
draft list took this route and was missing at least eight of its own paths —
evidence for how the list drifts.

### D8. Path rule for `prepare_board_photo`'s `map_id`: existing-directory-required, unchanged from archived D13

`prepare_board_photo`'s `map_id` follows archived D13 rule 3 as-is: a
client-supplied `map_id` is honored only when it passes the token pattern
**and** its directory already exists — `existing_map_dir`
(`photo_intake.rs:969`), which calls `validate_map_id` (`:949`), canonicalizes,
and re-checks `starts_with(project_dir)` (`:988`). This tool calls
`existing_map_dir`, never `prepare_map_dir` (`:591`, the creating variant
`scan_pcb_photo` uses). `label` is validated by the same token rule before any
path join, which rejects `..`, `/`, `\`, `:` and NUL by construction rather
than by blacklist (`:946-948`); only the `views/` subdirectory itself is
created, with `create_dir_all` under the already-canonical map directory.

In practice this means a dossier session's first `photo_intake` call is always
`scan_pcb_photo` (which does mint a `map_id`), even on a board where
retrace's *component* output will end up mostly discarded as unreliable — the
reference machine's retrace 0.3.0 install is verified working (archived
Context, 0.5 s contour scan), so a scan producing low-value components is
still a successful call that mints a usable map directory. `check_retrace`
reporting `available: false` (retrace entirely absent) still blocks the
whole flow at the same point it always did — this change does not widen that
case, because nothing in the orchestrator's brief asks for it, and doing so
would mean `prepare_board_photo` minting map directories through a second,
independent path from `scan_pcb_photo`'s — the exact "two ways to name a
new map directory" shape archived D13's own rejected-alternative note
already warned against for the deleted `output_dir` argument.

### D9. Skill and agent set

**Extend, don't split.** `pcb-photo-intake-agent` gains the dossier phase
(one owner of the map through dossier approval, matching the CT's own stated
preference) rather than a new `pcb-board-dossier-agent`:

- Frontmatter `tools:` becomes exactly:

  ```yaml
  tools:
    - mcp__konnect__*
    - Read
  ```

  `Read` is Claude Code's own file/image reader: it renders an image into the
  conversation, which is the one capability no `mcp__konnect__*` tool provides
  (`prepare_board_photo` writes a PNG; it cannot show one). **No guard test
  parses `tools:`** — `asset_references.rs`'s `yaml_list` (`:151`) is called
  once, with the key `"skills"` (`:103`), and `install.rs` writes each agent
  file's bytes verbatim into the client's agents directory (`:276-277`)
  without parsing its frontmatter. Adding the key is therefore a pure asset
  edit with no test to update.

  The agent's Hard Rules confine what it may `Read` to two sets: files under
  `<project_dir>/.konnect/photo_intake/<map_id>/views/` (its own
  `prepare_board_photo` outputs) and the exact paths the user supplied as
  source photos, which are also the map's `source_images` entries. It never
  `Read`s anything else — no source file, no config, no other project. This is
  a text rule, not an enforced boundary; it is written as a Hard Rule so a
  reviewer of the agent file can check it, and it is the reason `Read` goes to
  this agent only.

- `skills:` becomes exactly `konnect`, `kicad-photo-intake`,
  `kicad-board-dossier` (`agents_preload_existing_skills`,
  `asset_references.rs:81`, requires each to be a real bundled skill
  directory with a `SKILL.md`).
- New phases inserted between the existing Phase 1 (Scan) and Phase 3
  (Persist and review): survey every photo with `Read`, call
  `prepare_board_photo` for zoomed views of silkscreen/terminals/markings,
  classify and count by visual class, correlate with `scan_pcb_photo`'s
  boxes by bbox overlap, write `dossier`, get human approval #1 via the
  existing `approve_photo_review_map` call.
- Hard rules gain: never write a `dossier` claim without an `evidence`
  `{view, rect_px}` pointer; never collapse two disagreeing hypotheses or
  count methods into one.

*Rejected alternative (`Read`):* give the agent no `Read` and have
`prepare_board_photo` return the view as a base64 image content block, the way
some MCP tools return images. Rejected because it doubles every view's cost
through the tool-result channel for the whole session, the tool would then own
an image-encoding contract it otherwise does not have, and `Read` is already
the reader the host provides and the user already trusts with their files.

**New agent** `pcb-design-reconstruction-agent`:
- `skills: [konnect, kicad-design-reconstruction]`, `tools: [mcp__konnect__*]`
  (no `Read` — it works from the approved `dossier`, not the photos again).
- Loads `photo_intake` only to `load_photo_review_map`/`save_photo_review_map`/
  `approve_photo_review_map` the one map it is building `design_brief` onto;
  loads `library` to call `search_symbols`/`search_footprints` for every
  `bom` entry.
- Never calls a schematic-, board-, or library-*mutating* tool (search is
  read-only and stays in scope); refuses to write `design_brief` content
  until `load_photo_review_map`'s `approval_valid` is true for the dossier it
  is building on (spec scenario "design reconstruction is refused before the
  dossier is approved").
- Hands its result back to the session (never to another agent — see the
  delegation rule below) for `kicad-schematic-build-agent` (`bom`/`circuits`)
  and `kicad-pcb-layout-agent` (`physical_constraints`) after human approval
  #2, mirroring the existing archived D6 handoff pattern.
- There is no partial-toolset-loading mechanism in the router: loading
  `library` exposes all 17 of its tools (`registry.rs:111`). The restriction
  to `search_symbols`/`search_footprints` is a Hard Rule in the agent file,
  the same mechanism every other bundled agent's mutation boundary uses. This
  closes the planner's deferred finding; no narrower mechanism is added by
  this change, because introducing per-tool load filtering touches the router
  for every toolset and is a change of its own.

**New top-level skill** `kicad-photo-to-board` — the single entry point for
"I have photos of a board, recreate it." Not preloaded into any one agent's
`skills:` frontmatter (like `kicad-pcb`/`kicad-schematic`/`kicad-review`,
none of which are preloaded by a specific bundled agent either); referenced
from `konnect/SKILL.md`'s decision tree and Agent Routing section, the same
way those three already are. Content: one stage per row —

| Stage | Owner | Consumes | Produces | Gate | `INCOMPLETE` when |
|---|---|---|---|---|---|
| Capability + scan | `pcb-photo-intake-agent` | photo(s) | `map_id`, raw `analysis.json` | — | `check_retrace` reports `available: false` |
| Comprehension | `pcb-photo-intake-agent` | photos (via `Read`/`prepare_board_photo`), scan result | `dossier` | — | a claim has no `{view, rect_px}` evidence, or a question has one hypothesis where the evidence supports two |
| **Approval #1** | human | `dossier` | `approved: true` over `dossier`'s content | `approve_photo_review_map` | the human does not approve |
| Design reconstruction | `pcb-design-reconstruction-agent` | approved `dossier` | `design_brief` | — | `approval_valid` is false, or a `bom` entry stays `resolution_status: unresolved` |
| **Approval #2** | human | `design_brief` | `approved: true` over `dossier` + `design_brief` | `approve_photo_review_map` | the human does not approve |
| Schematic build | `kicad-schematic-build-agent` | approved `design_brief` | `.kicad_sch`, ERC result | ERC | `approval_valid` is false, or any `bom` entry is `unresolved` |
| PCB layout | `kicad-pcb-layout-agent` | approved `design_brief`, built schematic | `.kicad_pcb` | DRC | `approval_valid` is false, or `physical_constraints.unresolved` contains `board_size_mm` or a load-bearing row (D5) |
| Design review | `kicad-design-review-agent` | built board | review findings | — | the board is not routed |

**Delegation rule: agents cannot spawn agents.** Every stage above is invoked
by the orchestrating *conversation* — the session that loaded
`kicad-photo-to-board` — one agent at a time. No agent file instructs another
agent to run; each ends by returning to the session. So the skill's stage
table is written as instructions **to the session**, and every agent's final
message is required to state, in this order: `stage`, `map_id`, `produced`
(the artifact path or map section), `verdict` (`DONE` or `INCOMPLETE`), and
`blockers` (empty on `DONE`). The session reads that block, shows the human
what the "Gate" column names, and only then invokes the next stage's agent.
Every stage re-checks `approval_valid` itself by calling
`load_photo_review_map` (never a caller's summary) and reports `INCOMPLETE` —
never a guess — when a gate is closed or evidence is missing.

*Rejected alternative:* let `pcb-photo-intake-agent` invoke
`pcb-design-reconstruction-agent` directly once approval #1 lands, so the
whole pipeline runs from one prompt. Rejected because it is not a capability
these agents have, and writing it into the skill produces an agent that either
silently does nothing at that step or narrates a delegation that never
happened — the human gate between the two stages would be crossed by a
sentence. The five-field return block is the mechanism that makes the session
able to do the handoff instead.

### D10. Asset-guard and doc-count implications

- `NOT_TOOLS` (`crates/konnect/tests/asset_references.rs`) gains one
  commented block for the new response/schema field names the skills/agents
  must name in prose: `view_path`, `source_rect_px`, `source_size_px`,
  `output_size_px`, `exif_orientation`, `mm_per_px`, `basis`, `evidence`,
  `rect_px`, `count_method`, `count_alternatives`, `count_confidence`,
  `visual_class`, `hypotheses`, `resolution_path`, `claim_id`,
  `retrace_correlation`, `design_brief_seed`, `open_questions`,
  `block_diagram`, `circuits`, `calculated_values`, `derating_notes`, `bom`,
  `kicad_symbol`, `kicad_footprint`, `resolution_status`, `search_terms_used`,
  `physical_constraints`, `board_size_mm`, `board_size_px`, `board_size_status`,
  `scale_status`, `mounting_holes`, `keep_outs`, `connector_edges`,
  `user_facing_parts`, `net_currents`, `net_voltages`, `signal_speeds`,
  `sensitive_nets`, `layer_count`, `max_component_height_mm`,
  `assembly_notes`, `photo_views_used`, `silkscreen_markings`,
  `component_survey`, `topology_claims`, `layers_visible`,
  `derived_from_dossier`, `retrace_component_ids`, `overlap_confidence`.
  Two names on this list are already tool argument names elsewhere
  (`confidence`, `candidates` are single words and are not collected by
  `snake_words` at all; `image_path`, `map_id`, `project_dir` **are**
  two-part snake_case and **are** top-level properties of a registered tool,
  so they resolve without a `NOT_TOOLS` entry). Task 5.2 derives the final
  list mechanically from `cargo test -p konnect --test asset_references
  backticked_tool_names_in_prose_exist_in_the_registry` failures, per the
  playbook's own step 8; the list above is the starting set, and a name that
  the test does not flag is not added.
- Registry-derived doc counts (README.md, DEV.md, tool-directory.md — the
  three `doc_tool_counts.rs` checks against `registry::ALL_TOOLSETS`)
  update themselves once `tool_count: 6` lands; the required phrases just
  need their numbers to match, which `cargo test -p konnect --test
  doc_tool_counts` verifies directly. The verified baseline is 231
  registered / 238 total, so the new numbers are 232/239.
- `docs/TROUBLESHOOTING.md`, `packaging/metadata.json`, `plugin/plugin.json`
  are not covered by `doc_tool_counts.rs` (it checks the first three files
  only) but state the same "231"/"238" counts today; task 5.3 bumps them by
  hand for consistency, per the playbook's step 7.
- `tool-directory.md`'s `photo_intake` section (currently "5 tools") gains a
  `prepare_board_photo` row and its header becomes "6 tools" — required
  separately by `tool_directory_lists_every_registered_tool`.

### D11. `docs/PHOTO_TO_BOARD_WORKFLOW.md`

New docs page (not covered by any asset guard — it lives outside
`crates/konnect/assets/`), linked from `README.md` near the existing
Freerouting/kicad-cli external-dependency notes (archived task 6.1's
location). Content: the pipeline diagram (same stages as D9's table), the
artifact trail (`analysis.json`, `views/*.png`, the review map's `dossier`/
`design_brief` sections), the two approval points and what "approved" means
at each, and the honest limits — no inner layers, scale is either
user-supplied or agent-resolved-with-evidence and otherwise stays
unresolved, every value not directly read from silkscreen is an inference
carrying a stated confidence, and the input bounds of D1 (50 MP in, 4096 px
out, `scale` 0.25-4.0) so a user with a 108 MP phone photo reads why the tool
refused it rather than filing a bug.

### D12. Acceptance for the dossier methodology: an orchestrator-run checklist, not a test

An `#[ignore]`d Rust test cannot judge LLM-authored dossier content
(archived Context already established retrace has no redistributable
real-photo fixtures, and a synthesized image cannot exercise reading real
silkscreen). Acceptance for `kicad-board-dossier`'s methodology is therefore
a checklist the orchestrator runs against the real reference photos
(`C:\Users\felip\Downloads\WhatsApp Unknown 2026-09-18 at 12.00.58\*.jpeg`,
never copied into the repo), each item scored **pass/fail against a stated
value from the worked example** in `.orchestrator/handoffs/
board-dossier-reconstruction/00-orchestrator-dossier-prototype.md`:

1. **Identity string.** `dossier.identity.summary` contains the silkscreen
   identity `SEMAFARO 1.3 24V 03/2020` (or the exact string the run reads off
   the board, if the photos differ), and `dossier.identity.evidence[0]` names
   a `view` that exists on disk with a `rect_px` inside that view's bounds.
2. **Supply voltage, read not assumed.** `24V` appears in
   `dossier.silkscreen_markings[].text` with `basis: "observed"` — not only in
   an `inferred` claim or a calculation.
3. **LED count.** The `component_survey` entry whose `visual_class` names the
   LED class has `count` within ±10 % of the worked example's manual count for
   the same board (107 for board A, 122 for board B; so 97-118 and 110-135),
   and a non-empty `count_method`.
4. **Resistor count.** A `component_survey` entry for the axial-resistor class
   has `count == 19` for board A, with `locations[]` summing to 19.
5. **Mounting holes.** `dossier.physical.mounting_holes` totals 4 plated
   corner holes (the worked example's `role: plated_corner` count), each with
   a `position_px`.
6. **Connector edge.** `dossier.physical.connectors` has one entry whose
   `type` names a 2-pin screw terminal and whose `edge` is the bottom edge.
7. **A hypothesis with a calculation.** At least one
   `dossier.topology_claims[].hypotheses[]` entry has a non-empty
   `calculation` naming a resistor value and a current, and a non-empty
   `assumptions` array.
8. **Scale stays unresolved unless supplied.** If the run was given no hole
   pitch or edge length, `dossier.physical.board_size_mm` is `null`,
   `scale_status` says so, the gap is listed in `dossier.open_questions`, and
   `scale_reference` carries no `mm_per_px`.

Task 6.1 states this checklist verbatim as its own acceptance (there is no
separate command to name). Items 3 and 4 are the two that can fail for a
legitimate reason — a different photo set — in which case the runner records
the manual count it used as the new reference in its handoff rather than
lowering the tolerance.

*Rejected alternative:* an `#[ignore]`d integration test that runs the agent
and asserts on the produced JSON. Rejected because the assertion would either
be so loose it passes on an empty dossier or so tight it pins one model's
phrasing; and it would need the user's private photos checked into the repo,
which archived D13's source-image rule already forbids.

### D13. Cap check

`photo_intake`'s `tool_count` moves to 6, still under `MAX_TOOLS_PER_TOOLSET`
(20, archived D2). No other toolset changes.

### D14. Implementation order

The dependency order below is not arbitrary; each step's guard test cannot
pass before the one above it lands.

1. Task 1.1 (`Cargo.toml`) — nothing else compiles without it.
2. Tasks 2.1, 2.3, 2.4 (schema + hash + hash tests) **before** 1.2-1.5, because
   `prepare_board_photo`'s `mm_per_px` output reads `scale_reference`'s new
   fields, and because the hash change is the one that can break existing
   tests: finding that out first is cheaper.
3. Task 2.2 (allowlist) immediately after 2.1 — `fixed_records_are_closed_...`
   fails the moment the schema gains an open node, and a failing guard that
   sits for three tasks gets "fixed" by weakening it.
4. Tasks 1.2-1.6 (the tool).
5. Tasks 3.x-4.x (assets), then 4.6 and 5.2 (the asset guards), then 5.1, 5.3,
   5.4, 5.5.
6. Task 6.1 last, by the orchestrator, against the real photos.

## Pre-mortem

Assume this change shipped and the board dossier flow failed in the field.
Five causes, each paired with the decision that hardens against it.

1. **A map that was approved before this change was revoked the first time
   anyone saved it, and users learned to re-approve without reading.** Cause:
   `PhotoReviewMap`'s new `dossier`/`design_brief` fields were declared
   `Option<...>` without `skip_serializing_if = "Option::is_none"`, so
   `serde_json::to_value(&parsed)` emitted `"dossier": null`,
   `overlay_known_fields` (`photo_intake.rs:1452`) wrote that null into the
   record, `map.get("dossier")` returned `Some(Null)`, and D6's conditional
   insert fired for every map in existence. **Hardening:** D6 states the
   attribute pair as part of the decision, not as an implementation detail,
   names `subcircuit_hints` (`:691-692`) as the precedent, and task 2.4 case
   (5) asserts that a map with neither section hashes identically before and
   after a save round trip — the same assertion
   `a_review_map_round_trips_and_tolerates_absent_subcircuit_hints` (`:2773`)
   already makes for the hints field.

2. **Every stored approval silently became invalid and no error said why.**
   Cause: someone enabled `serde_json`'s `preserve_order` feature (directly, or
   transitively through a new dependency that turns it on), `serde_json::Map`
   stopped being a `BTreeMap`, and the canonical byte order every stored digest
   was computed under changed. Nothing fails loudly; approvals just stop
   matching. **Hardening:** the dependency is already stated in
   `review_map_content_hash`'s doc comment (`:854-859`) and D6 restates it as
   an inherited assumption rather than a new one; task 7.3's truth-command gate
   catches it because the archived pinned-digest test compares against a hex
   literal, which is the only test in the suite that would fail. The design
   names that test as the canary so a developer who sees it fail looks at the
   lockfile instead of re-pinning the literal.

3. **A reviewer approved a dossier full of claims nobody could check, and the
   design brief inherited every one of them.** Cause: `evidence` stayed
   free-text (the planner's draft shape), so checking a claim meant hunting for
   the crop it came from; a reviewer facing forty sentences skimmed and clicked
   approve, and `approval #1` became a formality. **Hardening:** D4 makes every
   `evidence` entry a `{view, rect_px}` object whose `view` must exist under
   the map's `views/` directory or in `source_images`, D9 makes "never write a
   claim without an evidence pointer" a Hard Rule of
   `pcb-photo-intake-agent`, and D12 item 1 makes "the cited view exists on
   disk and the rect is inside it" a scored acceptance item — so a dossier that
   cannot be checked fails the change's own acceptance before a user ever sees
   one.

4. **The layout agent built a board at the wrong size, or with the terminal on
   the wrong edge, from a brief that looked complete.** Cause:
   `physical_constraints` carried `board_size_mm: null` with no way to tell
   "unknown" from "not considered", so the agent treated the null as an
   invitation to pick a size. **Hardening:** D5 pins `physical_constraints` to
   the nine rows of `layout-methodology.md:30-38` with every row always present
   as a key, adds `unresolved[]` as the explicit "asked, unknown" marker, and
   makes `kicad-pcb-layout-agent` report `INCOMPLETE` when `unresolved`
   contains `board_size_mm` or one of the four rows that file itself calls
   load-bearing (`:56-57`). Task 4.5's acceptance names that section.

5. **A schematic was built from invented library ids and every symbol resolved
   to the wrong part — or to nothing.** Cause: the reconstruction agent could
   not find a terminal-block symbol, wrote a plausible-looking
   `Connector:Screw_Terminal_01x02` anyway, and the build agent trusted the
   brief because the field was non-null. **Hardening:** D5 makes `null` +
   `resolution_status: "unresolved"` + `candidates[]` the only legal
   representation of an unresolved part, states "writing a non-null
   `kicad_symbol`/`kicad_footprint` that no search returned" as the schema's
   single forbidden act, and makes `kicad-schematic-build-agent` report
   `INCOMPLETE` per unresolved entry rather than placing a candidate. D9's
   stage table lists that condition in the `INCOMPLETE` column for two separate
   stages, so it is checked before and after approval #2.

6. **A new dossier field shipped outside the approval gate and nobody
   noticed.** Cause: D4 gained a field, the hash's covered-key list did not,
   and the gate quietly stopped covering part of what the human read — the
   predecessor's own named pre-mortem cause, repeated. **Hardening:** D6
   removes the key list entirely for these two sections (whole-section
   `clone()`, fail-closed by construction) and adds `OPTIONAL_CONTENT_KEYS` to
   `every_schema_key_is_either_hashed_or_deliberately_not`
   (`photo_intake.rs:2751`), so a *third* section added later without a hash
   decision fails the test suite rather than shipping unguarded.

7. **The tool became a denial-of-service on the server process.** Cause: a
   user pointed `prepare_board_photo` at a 200 MP panorama or a crafted JPEG
   whose header claims 65535 x 65535, and the handler allocated until the
   process died — taking every other toolset with it. **Hardening:** D1
   checks `decoder.dimensions()` against a 50 MP cap *before* decoding, sets
   `Limits { max_alloc: Some(512 MiB) }` on the reader as a second line for a
   lying header, rejects a `scale` outside `0.25..=4.0` instead of clamping it,
   and rejects a computed output whose long side exceeds 4096 px before
   allocating the destination — with every one of those four an error result
   naming the actual number and the cap, so the user can act on it.

## Risks / Trade-offs

- **[Trade-off] `dossier`/`design_brief` are hashed whole, so any hand edit
  revokes approval** → a reviewer who adds a marginal note to an approved
  dossier must approve again. Accepted, and it is the safe direction: the
  alternative (a projection that lets some edits through) is the fail-open
  shape D6 rejects, and re-approval is one tool call against a document the
  reviewer has just read.
- **[Risk] `dossier`/`design_brief` schemas are large and open** → a
  reviewer facing a big free-form JSON document may skim rather than read.
  Mitigation: `kicad-photo-to-board` states what the human is shown at each
  gate (D9's table) as prose, not raw JSON; `kicad-board-dossier` requires an
  openable `{view, rect_px}` on every claim; and D12 item 1 scores that
  requirement at acceptance time.
- **[Risk] `prepare_board_photo`'s Lanczos3 resize is a fresh, unprecedented
  choice** → no existing Konnect code sets a resize filter to match. Accepted:
  it is a one-line, swappable choice with no data-format consequence (the
  output is still a PNG the LLM reads); a future change can pick differently
  without touching the schema.
- **[Trade-off] Applying EXIF orientation changes the coordinate space a
  `crop` is expressed in** → for a tagged phone photo, `source_size_px` may be
  the transpose of the file's stored dimensions. Accepted: the response reports
  both `exif_orientation` and `source_size_px`, so the space is inspectable,
  and the alternative (ignoring the tag) puts the crop rectangle silently on
  the wrong part of the board.
- **[Trade-off] Conditional hash-field inclusion (D6) adds a branch to
  `review_map_content_hash`** → two sections and two scale-reference fields now
  have "insert if present" behavior where the other four keys always insert.
  Accepted: it is the only way to satisfy "keep old maps' digests stable," and
  the branch is one `if let Some` per key, tested directly (task 2.4).
- **[Risk] `prepare_board_photo`'s `map_id`-must-already-exist rule (D8)
  blocks the dossier flow if retrace is entirely absent** → accepted as
  out of scope; the reference machine's retrace install is verified working,
  and widening map-id minting to a second tool reopens archived D13's
  already-rejected "two ways to name a new map directory" problem.
- **[Risk] The `library`-toolset read-only restriction is text, not a
  mechanism** → `pcb-design-reconstruction-agent` can see all 17 `library`
  tools once the toolset loads. Accepted for this slice: every bundled agent's
  mutation boundary already works this way, and per-tool load filtering is a
  router change with its own blast radius.

## Migration Plan

Purely additive to behavior, same shape as the archived change's Migration
Plan: no existing tool schema, config key, or agent file changes *meaning*
(only gains content). `Cargo.toml` (root and `konnect-core`), `registry.rs`
(`tool_count`), `router/mod.rs`'s allowlist, and `asset_references.rs`'s
`NOT_TOOLS` each gain entries; no data migration, since D6 guarantees no
existing on-disk map's stored hash or approval state changes meaning.
Deploy the tool, schema extension, hash-coverage change, three skills, one
new agent, two agent consumer-note sections, and the docs page in one PR.

## Open decisions resolved

1. **Extend `pcb-photo-intake-agent` vs. split a new dossier agent** —
   extend (D9). One owner of the map through dossier approval; a new agent
   only for the phase that has a genuinely different scope (design
   reconstruction, which starts from an approved dossier rather than raw
   photos and needs `library` tools the intake agent does not).
2. **Hash-coverage break vs. stability for old maps** — stability:
   conditional inclusion at the `review_map_content_hash` level, whole-section
   coverage for the two new sections (D6). The archived pinned-digest test
   needs no change; no already-approved map is invalidated by gaining fields it
   never had; and there is no per-field list to drift.
3. **`prepare_board_photo`'s `map_id` minting rule** — unchanged from
   archived D13 rule 3: must already exist, minted only by `scan_pcb_photo`
   (D8). Not widened; no orchestrator instruction asked for it, and doing so
   reopens a rejected archived alternative.
4. **Where `kicad-photo-to-board` lives in the routing structure** — a
   top-level skill referenced from `konnect/SKILL.md`, not preloaded into any
   one agent's frontmatter, matching `kicad-pcb`/`kicad-schematic`/
   `kicad-review`'s existing pattern (D9).
5. **EXIF orientation** — honoured (D1). `image` 0.25.10 exposes
   `ImageDecoder::orientation()` and `DynamicImage::apply_orientation`; both
   were read in the locked crate source. `rotate` remains the manual tool for
   an untagged sideways photo.
6. **Input and output bounds** — 50 MP input, 512 MiB reader allocation cap,
   `scale` rejected outside `0.25..=4.0`, 4096 px output long side, no timeout
   (D1).
7. **Agent frontmatter `tools:` gaining `Read`** — safe; no guard test parses
   that key (D9, verified at `asset_references.rs:103`/`:151` and
   `install.rs:276`).
8. **BOM field naming** — `kicad_symbol`/`kicad_footprint` kept (D5), because
   the spec delta already cites those paths; the lib_id/footprint requirement
   is met by value, with `resolution_status`/`candidates` for unresolved parts.
9. **Schema depth vs. allowlist size** — object nodes enumerated, arrays
   declared without `items`, six new allowlist paths (D7).
10. **`library` read-only scope for `pcb-design-reconstruction-agent`** — a
    Hard Rule in the agent file; no partial-toolset-loading mechanism exists
    and none is added (D9). This closes the planner's deferred finding.

## Open Questions

Nothing below blocks implementation of any of the 31 tasks. Each names an
owner and the slice that would settle it.

1. **Should the review map record which `views/*.png` files exist, so a
   reviewer opening an old map can tell a missing view from a renamed one?**
   `dossier.photo_views_used` names them but nothing verifies they are still
   there. Owner: `@architect`, in whichever slice first ships a review UI —
   this slice's reviewer reads the JSON beside the directory.
2. **Is a count helper (`count_blobs`) worth a seventh `photo_intake` tool?**
   The concept-validation run used an ad-hoc HSV blob script to get within
   ±10 % on LED count; the LLM's own estimate from views was not measured
   against that. Owner: `@pm`, after task 6.1 reports whether the checklist's
   item 3 passes on LLM estimation alone.
3. **Server-side enforcement of the approval gate inside `sch_*`/`pcb_*`
   handlers** — still deferred from the archived change's Non-Goals, and this
   change adds a second gate that is also text-enforced. Owner: `@architect`,
   as its own change.
