---
name: kicad-design-reconstruction
description: |
  Methodology for turning an approved board dossier into a buildable design brief:
  functional blocks, per-block circuits with calculated values and derating, a BOM
  whose every part resolves to a real KiCad library symbol and footprint, and the
  layout constraint record. Triggers on: "design a board from this dossier",
  "reconstruct this board", "turn the dossier into a design", "rebuild this board
  from the photos", "design brief".
argument-hint: "[map_id of an approved dossier]"
---

# KiCAD Design Reconstruction — From Dossier to Design Brief

This skill turns an **approved** `dossier` into a `design_brief` section on the
same review map: what to build, with what parts, to what calculated values,
inside what physical constraints. It is a document, not a design — it creates
no `.kicad_sch`, no `.kicad_pcb`, and no library part. The schematic and the
board are built later, by `kicad-schematic-build-agent` and
`kicad-pcb-layout-agent`, from the brief a human has approved.

It reads the dossier, not the photographs. If a question the dossier left open
decides a value here, the answer is `INCOMPLETE` and the question goes back —
not a number you liked the look of.

---

## Prerequisites

- A `map_id` whose dossier is **approved**. You verify that yourself, with
  `load_photo_review_map`, on every run.
- Nothing else. There are no photos to read in this workflow and no host file
  reader in scope; the dossier's claims, evidence and open questions are the
  whole input.

---

## Toolset Loading

```
load_toolset('photo_intake')   # check_retrace, scan_pcb_photo, prepare_board_photo, save_photo_review_map, load_photo_review_map, approve_photo_review_map
load_toolset('library')        # search_symbols, search_footprints
```

The `library` toolset is loaded for **search only**. There is no
partial-toolset mechanism: loading it exposes every tool it has, including the
ones that create and edit symbols and footprints. Calling any of those from
this workflow is out of scope — you are resolving names, not authoring parts.

Do NOT load a schematic or PCB toolset here.

### References by decision

- Read [`references/design-brief-schema.md`](references/design-brief-schema.md)
  before writing or editing a `design_brief`: every field, every array
  element's own field list, the symbol/footprint resolution rule, and the
  row-by-row mapping from `physical_constraints` to the layout methodology's
  constraint table.
- Read the kicad-board-dossier skill's `references/dossier-schema.md` for the
  document you are consuming — in particular what `basis`, `confidence` and a
  competing hypothesis mean.
- Read the kicad-pcb skill's `references/layout-methodology.md` section 1: the
  nine-row constraint table `physical_constraints` is built to fill, and the
  four rows it calls load-bearing.

---

## Workflow

### 1. Gate first — re-check the approval yourself

```
load_photo_review_map(project_dir, map_id)
```

Read `approval_valid` from that response. Not the map's own `approved` field,
which stays `true` on a map edited after approval. Not the caller's summary,
however confident it sounds.

**`approval_valid: false` ends the run.** Report `INCOMPLETE`, name the map,
and say that it needs `approve_photo_review_map` from the human who owns it.
Do not write `design_brief` content for an unapproved dossier — not a draft,
not a partial one, not "so it is ready when they approve".

Then read the whole dossier: `identity`, `physical`, `component_survey`,
`silkscreen_markings`, `topology_claims` with all their hypotheses,
`design_brief_seed`, and `open_questions`. The open questions are your
constraint list, not background reading.

### 2. Functional blocks

Write `block_diagram[]`: one entry per functional block, each with a `block`
name, what it does (`function`), and its `inputs`/`outputs` as net names.
Power entry, protection, conversion, the repeated load blocks, the interfaces.
Blocks come from the dossier's observations and topology claims — a block the
dossier gives no evidence for is not in the design.

Where a topology claim has competing hypotheses, **the brief picks one and
says so**: name the `claim_id`, state which hypothesis you built on, and carry
the alternative into `open_questions`. That is the one place this skill
resolves a disagreement the dossier deliberately kept open, and it is
reversible precisely because it is written down.

### 3. Per-block circuits, with the arithmetic and the derating

One `circuits[]` entry per block: the `block` it implements, a `description` of
the topology, `calculated_values[]`, and `derating_notes`.

Every `calculated_values[]` entry carries the `parameter` it fixes, its value,
the `formula` that produced it, and the `assumptions` that formula rests on —
the same discipline the dossier's hypotheses use, because these numbers are
usually the dossier's hypotheses made concrete:

```
R1 = (24 V - 6 x 2.1 V) / 0.02 A = 570 ohm -> nearest E24 620 ohm
```

Then derate, in `derating_notes`, and derate explicitly:

- **Power**: `P = I^2 R` for a resistor, `P = (Vin - Vout) x I` for a linear
  pass element. State the calculated dissipation and the part rating you chose
  for it (`0.25 W calculated -> 1/2 W axial`). A part rated at its own
  dissipation is a part running at 100 %.
- **Voltage**: capacitor and semiconductor ratings against the rail plus its
  surges, not against the nominal.
- **Current**: connector and trace ratings against the continuous total, with
  the peak stated separately.
- **Temperature**: say when a rating is a room-temperature rating and the board
  will not be at room temperature.

A value you could not calculate because the dossier left the input open is not
guessed: it goes in `open_questions` and its block reports `INCOMPLETE`.

### 4. BOM — every part resolves to a real library id, or it stays unresolved

One `bom[]` entry per part role. `kicad_symbol` is a KiCad **lib_id**
(`Library:Symbol`, e.g. `Device:LED`) and `kicad_footprint` a footprint id
(`Library:Footprint`). Both come from a search you actually ran:

```
search_symbols(query)
search_footprints(query)
```

Record what you searched in `search_terms_used[]` whether or not it worked.

- **Resolved**: the search returned the part, you checked it fits the class the
  dossier observed, `resolution_status` is `resolved`, and both ids are the
  strings the search returned — copied, not retyped from memory.
- **Unresolved**: the search did not settle it. `kicad_symbol` and
  `kicad_footprint` are both `null`, `resolution_status` is `unresolved`, and
  `candidates[]` carries the near misses, each with its own ids and a `why`
  saying what matches and what was not verified.

**Writing a non-null `kicad_symbol` or `kicad_footprint` that no search
returned is the single forbidden act of this document.** A plausible-looking
lib_id is the one error that survives every later review: schematic build
places it without complaint, and the wrong part is on the board. An unresolved
entry is honest and cheap; `kicad-schematic-build-agent` reports `INCOMPLETE`
for each one rather than placing a symbol from a candidate.

Also per entry: the `value` (calculated or read), the `quantity` (from the
dossier's counts, not recounted here), and `source` — `matched` when a search
found the exact part, `equivalent` when it found a stand-in, `calculated` when
the value came from section 3.

### 5. `physical_constraints` — the layout constraint record

`physical_constraints` is the layout agent's section-1 record, written in
advance, one key per row of the kicad-pcb skill's
`references/layout-methodology.md` constraint table. The row-by-row mapping is
in `references/design-brief-schema.md`.

Two rules make it useful:

1. **Every key is always present.** A row with no answer is `null` (or `[]`)
   **and** is named in `unresolved[]`. That is what distinguishes "asked,
   unknown" from "never considered" — the distinction the layout methodology
   demands, and one an absent key cannot make.
2. **Pixel values travel beside millimeter ones.** `position_px` and
   `region_px` let a brief be written and reviewed before a scale is resolved;
   the millimeter fields fill in when it is.

The brief is `INCOMPLETE` — say so, in those words, in your report — when
`unresolved[]` contains `board_size_mm` or any of the four rows the layout
methodology calls load-bearing: net currents, net voltages, connector
position, enclosure. `kicad-pcb-layout-agent` will refuse those rows anyway;
saying it here is what gets the question back to the user while there is still
time to answer it.

### 6. Save, then the second approval

- `save_photo_review_map(project_dir, map)` — the brief travels inside `map`,
  as `map.design_brief`, alongside the dossier it was derived from. A section
  is an object or it is absent: a `"design_brief": null` is rejected with
  "Omit the key entirely to remove the section".
- Saving a `design_brief` onto an approved map **revokes that approval**,
  because the brief joins the hashed content. This is the second checkpoint,
  and it is the same mechanism as the first — not a bug to work around.
- Print `saved_path`. Walk the user through: the blocks, each calculated value
  with its formula and assumptions, every `unresolved` BOM entry and its
  candidates, the `physical_constraints` rows still in `unresolved[]`, and
  which topology hypothesis the brief chose.
- `approve_photo_review_map(project_dir, map_id)` **only** on their explicit
  approval of the brief, then re-read `approval_valid`.

### 7. Handoff

You do not build anything and you do not invoke another agent. Hand back to
the session, which invokes the next stage:

- `kicad-schematic-build-agent` consumes `bom` and `circuits`. It calls
  `load_photo_review_map` itself, re-checks `approval_valid`, places one symbol
  per resolved BOM entry, and reports `INCOMPLETE` for every entry left
  `unresolved`.
- `kicad-pcb-layout-agent` consumes `physical_constraints` as its constraint
  record, and refuses to place when `unresolved[]` names `board_size_mm` or a
  load-bearing row.

Give the session the map's `saved_path`, its `map_id`, the block and BOM
counts, the unresolved list, and the open questions. Nothing else travels: a
summary is not the brief, and both consumers read the map for themselves.

---

## Rules

1. **Never write `design_brief` content while `approval_valid` is false.** The
   gate is checked by you, on this run, from `load_photo_review_map`.
2. **Never invent a `kicad_symbol` or `kicad_footprint`.** Ids come from
   `search_symbols`/`search_footprints`; an entry the search did not settle is
   `unresolved` with `candidates[]`.
3. **Never write a calculated value without its `formula` and its
   `assumptions`**, and never a part choice without its derating.
4. **Never leave a `physical_constraints` row absent** — `null` plus an entry
   in `unresolved[]`, so a missing answer is visible as a missing answer.
5. **Never resolve an open question silently.** Choosing a hypothesis is
   allowed; choosing it without naming the `claim_id` and carrying the
   alternative into `open_questions` is not.
6. **Never mutate a schematic, a board or a library part** in this workflow —
   the `library` toolset is loaded for search, and search only.
7. **Never approve on the user's behalf**, and never hand a brief onward with a
   stale `approval_valid`.
8. **Report `INCOMPLETE` rather than closing a gap yourself** — name the block,
   the row or the BOM entry, and what would resolve it.
