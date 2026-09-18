# Photo review map — schema reference

The canonical JSON a photo intake produces, reviewed by a human, and approved
before any KiCad file is built from it. It lives at
`<project_dir>/.konnect/photo_intake/<map_id>/review_map.json`, beside the raw
`analysis.json` the scan produced.

Read this before building a map by hand, before editing one, and before
consuming one.

```json
{
  "map_id": "server-assigned by scan_pcb_photo",
  "saved_at": "RFC 3339 UTC, rewritten on every save",
  "source_images": ["C:/abs/path/to/top.png"],
  "scale_reference": { "kind": "board_edge_mm", "value": "50" },
  "components": [
    {
      "component_id": "C0000",
      "ref": null,
      "type": "resistor",
      "value": null,
      "footprint_suggestion": null,
      "confidence": 0.5,
      "bbox_px": [295, 195, 51, 31],
      "approved": false
    }
  ],
  "nets": [
    { "connections": ["R1.1", "C2.2"], "source": "traced" }
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

## Top-level fields

| Field | Owner | Meaning |
|---|---|---|
| `map_id` | server | Assigned by `scan_pcb_photo`. Names the directory holding the map and its evidence. Matches `^[A-Za-z0-9_-]{1,64}$`. Never invent one: a save is refused for a directory no scan minted. |
| `saved_at` | server | RFC 3339 UTC, rewritten on every save. **Outside the approval hash.** |
| `source_images` | you | Absolute paths to the photo(s) the map was derived from. Part of the hash: swapping the photo revokes approval. |
| `scale_reference` | **user**, or an agent with evidence | `kind` plus the physical `value`. Either the user states it, or an agent resolves it from a named physical feature — in which case the object also carries `mm_per_px` and `evidence`. Never estimated from pixels. See below. |
| `components` | you, then the user | One entry per detected component. See below. |
| `nets` | you, then the user | Connection entries. See below. |
| `subcircuit_hints` | scan | Optional, advisory. See below. |
| `dossier` | you, then the user | Optional. What the photos say this board is. See below. |
| `design_brief` | you, then the user | Optional. What to build from an approved dossier. See below. |
| `approved` | server | Written only by `approve_photo_review_map`. A client-supplied value is ignored and overwritten. |
| `approved_at` | server | RFC 3339 UTC of the approving call. |
| `content_hash_at_approval` | server | Hash of exactly the content that was approved. |

## `components[]`

| Field | Meaning |
|---|---|
| `component_id` | The scan's own id, copied verbatim, so the row is traceable back to `analysis.json`. |
| `ref` | The KiCad reference designator (`R1`, `C2`, `U3`). `null` until a human assigns it. An unassigned `ref` cannot be wired. |
| `type` | Coarse class: resistor, capacitor, ic, connector, … This plus `value` is what schematic build matches a real library symbol against. |
| `value` | String, or `null` when the scan read nothing. **Leave it null rather than guessing.** |
| `footprint_suggestion` | A suggestion only. Schematic build resolves the real footprint from the real library. |
| `confidence` | The scan's own number, copied unchanged. **Below 0.6 means flag the row for manual identification** — never round it up, never hide the row. |
| `bbox_px` | `[x, y, w, h]` in source-image pixels, copied verbatim from the scan. This is how a reviewer finds the part in the photo. |
| `approved` | Per-component human decision. Components left `false` are skipped by schematic build, not guessed at. |

A contour-only scan (`used_fallback: true`) never attempted marking or
`value` — the flag is true whenever either optional extra was missing. An empty
field then means "not read", not "nothing printed on the part": say which in
the review. There is no part-number field in this map; retrace's own
`part_number` stays in `analysis.json` as evidence.

## `nets[]`

| Field | Meaning |
|---|---|
| `connections` | Array of `ref`-and-pin endpoints, e.g. `R1.1`. Two or more per net. |
| `source` | `traced` (the scan followed copper), `inferred` (reasoned from the layout), or `manual` (the user stated it). One of exactly these three. |

Tag honestly. `inferred` is the tag that tells a reviewer where to look hardest,
and mislabelling it as `traced` is the fastest way to solder a wrong net.

retrace also emits a synthetic netlist with arrival-order pin numbering. It is
never a source for this list.

## `scale_reference`

| Field | Meaning |
|---|---|
| `kind` | `board_edge_mm`, `package`, `mounting_hole_pitch`, or another named feature. Free-form; the schema does not enumerate it. |
| `value` | The physical value relied on, however it was obtained: `"50"`, `"0805"`, `"5.08"`. |
| `mm_per_px` | Optional. Millimeters per pixel, written **only** once it is resolved from a named physical feature. |
| `evidence` | Optional string. The feature and the reasoning `mm_per_px` rests on: "21px between M3 hole centers, 5mm actual pitch stated by the user". |

`mm_per_px` and `evidence` are **always a pair**. An agent that cannot name the
evidence for a millimeter value leaves both absent, and the board size stays
unresolved. Both are optional and both join the hash only when present, so a
map that never resolves a scale hashes exactly as it did before they existed.

## `dossier`

Optional. The board-comprehension document: `identity`, `physical`,
`component_survey`, `silkscreen_markings`, `topology_claims`,
`retrace_correlation`, `photo_views_used`, `design_brief_seed` and
`open_questions`. Every claim carries `basis` (`observed` or `inferred`), a
numeric `confidence`, and `evidence` entries shaped
`{view, rect_px, note?}` naming a file in the map's `views/` directory or one
of `source_images`.

Field by field: the kicad-board-dossier skill's `references/dossier-schema.md`.
Its methodology is that skill's `SKILL.md`; `pcb-photo-intake-agent` writes it.

## `design_brief`

Optional. What to build from an **approved** dossier: `block_diagram`,
`circuits` with their calculated values, a `bom` whose `kicad_symbol` and
`kicad_footprint` come from `search_symbols`/`search_footprints` or stay
`null` with `resolution_status: "unresolved"`, `physical_constraints`,
`assumptions` and `open_questions`.

Field by field: the kicad-design-reconstruction skill's
`references/design-brief-schema.md`. `pcb-design-reconstruction-agent` writes
it, and only while the dossier's `approval_valid` is true.

Both sections are **objects or absent**. `save_photo_review_map` rejects a
`null` for either one with "Omit the key entirely to remove the section", and
every level of both stays open (`additionalProperties: true`) for the same
reason the rest of the map does: a human edits this file by hand between calls.

## `subcircuit_hints[]`

Optional; may be absent entirely. Carries the scan's `pattern_matches`
verbatim: `pattern_name`, `description`, `component_roles`, `score`,
`is_partial`.

**Advisory, never evidence.** No component is created, typed, valued or
approved from a hint; no net is inferred from one. Hints sit **outside the
approval hash**, so they can neither grant nor revoke approval — which is
exactly what makes them safe to surface. `score` and `is_partial` are carried
through unthresholded so a reader judges the match instead of a verdict.

## The approval hash

`content_hash_at_approval` covers four fields always — `source_images`,
`scale_reference`, `components`, `nets` — and two more **only when the map
carries them**:

| Field | In the hash? |
|---|---|
| `source_images`, `scale_reference`, `components`, `nets` | always |
| `scale_reference.mm_per_px`, `scale_reference.evidence` | when present |
| `dossier` | when present, whole |
| `design_brief` | when present, whole |
| `map_id`, `saved_at`, `subcircuit_hints` | never |
| `approved`, `approved_at`, `content_hash_at_approval` | never |

A section that is absent contributes nothing to the hashed bytes. That is what
makes a map saved before these sections existed, or one that never gains them,
hash exactly as it always did.

It is also what gives one map **two sequential approval checkpoints through one
mechanism**: an approval recorded while only `dossier` is present covers the
board-comprehension content; adding `design_brief` afterwards changes the
covered content and revokes that approval, exactly like any other tracked edit.

Consequences worth knowing before editing a file by hand:

- Reformatting the JSON, or a save that changes only `saved_at`, does not
  revoke approval — the hash is over the field values, not the file bytes.
- Changing any component or net field, a source image, the scale reference, or
  anything inside `dossier` or `design_brief` revokes it.
  `approve_photo_review_map` must be called again.
- A consumer reads `approval_valid` from `load_photo_review_map`, which the
  server computes from the stored hash. The map's own `approved` field is not
  the gate: it stays `true` on a map that was edited after approval.
