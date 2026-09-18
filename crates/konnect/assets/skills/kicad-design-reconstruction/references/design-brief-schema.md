# Design brief — schema reference

The `design_brief` section of a photo review map: what to build from an
approved dossier, with what parts, to what calculated values, inside what
physical constraints. It is an **optional** top-level key of the map stored at
`<project_dir>/.konnect/photo_intake/<map_id>/review_map.json`, written beside
the `dossier` it was derived from.

Read this before writing or editing a `design_brief`, and before consuming one.

The same two things the JSON Schema cannot enforce apply here:

- **Array elements have field lists too.** Every array below is declared in the
  tool schema as a plain `{"type": "array"}` with a prose description and no
  `items` subschema. The per-element tables in this file are the contract.
- **A section is an object, or it is absent.** `save_photo_review_map` rejects
  `"design_brief": null` with "Omit the key entirely to remove the section".

Every level is open (`additionalProperties: true`): a human edits this file by
hand between calls, and an annotation no tool reads must survive a save.

---

## Top-level fields

| Field | Type | Meaning |
|---|---|---|
| `derived_from_dossier` | boolean | `true` when this brief was written from that map's **approved** dossier. It is a statement about provenance, not a gate — the gate is `approval_valid`. |
| `block_diagram` | array | Named functional blocks with their inputs and outputs. |
| `circuits` | array | Per-block topology, calculated values and derating. |
| `bom` | array | One entry per part role. |
| `physical_constraints` | object | The layout constraint record. |
| `assumptions` | array of string | What every calculation above rests on. |
| `open_questions` | array of string | What the brief could not settle, including any dossier question it had to choose past. |

---

## `block_diagram[]`

| Field | Meaning |
|---|---|
| `block` | Short stable name, referenced by `circuits[].block`: `power_input`, `led_string_1`. |
| `function` | What the block does, in one sentence. |
| `inputs` | Array of net names entering the block. |
| `outputs` | Array of net names leaving it, or `[]`. |

A block the dossier gives no evidence for does not belong here. The brief
reconstructs an observed board; it does not improve one.

## `circuits[]`

| Field | Meaning |
|---|---|
| `block` | The `block_diagram[].block` this implements. |
| `description` | The topology in one or two sentences. |
| `calculated_values` | Array — see below. |
| `derating_notes` | The dissipation, voltage and current margins, and the part rating chosen for each. `0.25 W calculated -> 1/2 W axial` is a derating note; `use a 1/2 W part` is not. |

### `circuits[].calculated_values[]`

| Field | Meaning |
|---|---|
| `parameter` | What the value fixes: `R1`, `C_bulk`, `I_string`. |
| a value field | The number with its unit in the field name — `value_ohms`, a farad or ampere field named the same way. Never a bare `value` whose unit lives only in the formula. |
| `formula` | The arithmetic that produced it, written out: `(24V - 6*2.1V) / 0.02A`. |
| `assumptions` | Array of strings: every input the board did not supply — a forward voltage, a target current, a regulated rail, a tolerance. |

## `bom[]`

| Field | Meaning |
|---|---|
| `role` | What the part is for, in words: "LED (5mm, clear lens, through-hole)". |
| `kicad_symbol` | KiCad lib_id (`Library:Symbol`, e.g. `Device:LED`), or `null`. |
| `kicad_footprint` | Footprint id (`Library:Footprint`), or `null`. |
| `resolution_status` | `resolved` or `unresolved`. |
| `search_terms_used` | Array of the strings actually passed to `search_symbols`/`search_footprints`, whether or not they worked. |
| `candidates` | Array of `{kicad_symbol, kicad_footprint, why}` near misses. Empty on a resolved entry; **non-empty is what makes an unresolved entry useful**. |
| `value` | The calculated or matched value, with units. |
| `quantity` | From the dossier's counts. Not recounted here, and not rounded. |
| `source` | `matched` (a search found the exact part), `equivalent` (a stand-in), or `calculated` (the value came from `circuits[]`). |

### The resolution rule

`kicad_symbol` and `kicad_footprint` are obtained by calling `search_symbols`
and `search_footprints` — never from memory, never by pattern-matching a
library name that "looks right".

```json
{
  "role": "2-pin 5.08mm screw terminal",
  "kicad_symbol": null,
  "kicad_footprint": null,
  "resolution_status": "unresolved",
  "search_terms_used": ["screw terminal", "TerminalBlock 5.08"],
  "candidates": [
    {
      "kicad_symbol": "Connector_Generic:Conn_01x02",
      "kicad_footprint": "TerminalBlock:TerminalBlock_bornier-2_P5.08mm",
      "why": "pitch matches; body style not verified against the photo"
    }
  ],
  "value": "5.08mm pitch, 2 positions",
  "quantity": 1,
  "source": "matched"
}
```

**Writing a non-null id that no search returned is the single forbidden act of
this schema.** It is the one error that survives review: schematic build places
the symbol without complaint and the wrong part reaches the board.
`kicad-schematic-build-agent` reports `INCOMPLETE` for every `unresolved`
entry rather than placing one of its candidates.

## `physical_constraints`

The layout constraint record, written ahead of layout. **Every key below is
always present.** A row with no answer is `null` (or `[]`) **and** is named in
`unresolved[]` — that pair is what distinguishes "asked, unknown" from "never
considered", and an absent key cannot make that distinction.

| Field | Type | Meaning |
|---|---|---|
| `board_size_mm` | `[w, h]` or `null` | `null` until the dossier's scale resolves. Never estimated. |
| `board_size_status` | string or `null` | Why it is `null`, pointing at `dossier.physical.scale_status`. |
| `mounting_holes` | array of `{position_mm, position_px, diameter_mm}` | `position_mm` is `null` before a scale resolves; `position_px` always travels. |
| `enclosure` | string or `null` | The enclosure constraint, if any. |
| `max_component_height_mm` | number or `null` | Available height above the board. |
| `connector_edges` | array of `{edge, type, pitch_mm, position_px}` | Where each connector must sit, and its pitch. |
| `user_facing_parts` | array of `{role, why_user_facing}` | Parts whose position the product's use dictates — an LED array behind a lens, a switch, a display. |
| `net_currents` | array of `{net, continuous_a, peak_a, basis, note}` | Continuous and peak current per net. |
| `net_voltages` | array of `{net, nominal_v, surge_v, basis, note}` | Nominal and surge voltage per net. |
| `signal_speeds` | array of `{net, frequency_hz, rise_time_ns}` | `[]` on a board with no fast edges — which is an answer, not a gap. |
| `sensitive_nets` | array of `{net, why}` | Nets needing distance from noise sources. |
| `layer_count` | integer or `null` | |
| `stackup` | string or `null` | |
| `fabricator` | string or `null` | The fabricator whose capability limits apply. |
| `assembly_notes` | string or `null` | Assembly and test process constraints. |
| `keep_outs` | array of `{region_mm, region_px, why}` | Regions nothing may occupy. Not a methodology row — this change's addition. |
| `unresolved` | array of string | Every field above still unanswered, by name. |

### Row-by-row mapping onto the layout methodology

`physical_constraints` is built to fill the constraint table in the kicad-pcb
skill's `references/layout-methodology.md`, section 1 — so
`kicad-pcb-layout-agent` fills its own section-1 record from the brief with no
translation step:

| `layout-methodology.md` section 1 row | `physical_constraints` field(s) |
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
| (not a methodology row — this change's addition) | `keep_outs[]` |

That file's own completion criterion is "a written constraint record covering
every row above, with 'unknown — asked the user' where the answer is missing",
and it names four rows as load-bearing: **current, voltage, connector
position, enclosure**. So:

- `unresolved[]` containing `board_size_mm`, `net_currents`, `net_voltages`,
  `connector_edges` or `enclosure` makes the brief `INCOMPLETE` for layout.
  `kicad-pcb-layout-agent` reports `INCOMPLETE` rather than choosing a value of
  its own.
- Any other unresolved row lets layout proceed **with a stated assumption**,
  which it writes into its own constraint record.

## `assumptions[]` and `open_questions[]`

`assumptions[]` gathers what every calculation in the brief rests on, so a
reviewer can disagree with one input rather than re-deriving the document.

`open_questions[]` carries what the brief could not settle — including every
dossier `topology_claims` question it had to choose past. Choosing a hypothesis
is allowed; choosing one without naming the `claim_id` and recording the
alternative here is not.

---

## The approval hash

`design_brief` joins the map's content hash **the moment it is present**, and
contributes nothing when absent.

- Saving a brief onto a map approved for its dossier alone **revokes that
  approval**. That is the second review checkpoint, through the same mechanism
  as the first: `approve_photo_review_map` must be called again before either
  build agent may consume the brief.
- Editing any field above revokes it again.
- Both consumers read `approval_valid` from `load_photo_review_map`, never the
  map's own `approved` field.
