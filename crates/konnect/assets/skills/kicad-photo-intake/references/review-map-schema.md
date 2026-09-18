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
| `scale_reference` | **user** | `kind` plus the physical `value` the user gave — board edge in mm, or a known package on the board. Never estimated from pixels, never filled in by an agent. |
| `components` | you, then the user | One entry per detected component. See below. |
| `nets` | you, then the user | Connection entries. See below. |
| `subcircuit_hints` | scan | Optional, advisory. See below. |
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

`content_hash_at_approval` covers exactly four fields: `source_images`,
`scale_reference`, `components`, `nets`.

Outside it: `map_id`, `saved_at`, `subcircuit_hints`, `approved`,
`approved_at`, `content_hash_at_approval` itself.

Consequences worth knowing before editing a file by hand:

- Reformatting the JSON, or a save that changes only `saved_at`, does not
  revoke approval — the hash is over the field values, not the file bytes.
- Changing any component or net field, a source image, or the scale reference
  revokes it. `approve_photo_review_map` must be called again.
- A consumer reads `approval_valid` from `load_photo_review_map`, which the
  server computes from the stored hash. The map's own `approved` field is not
  the gate: it stays `true` on a map that was edited after approval.
