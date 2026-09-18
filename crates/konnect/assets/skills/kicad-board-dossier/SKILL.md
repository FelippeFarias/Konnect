---
name: kicad-board-dossier
description: |
  Methodology for reading a physical board out of photographs into an evidence-backed
  dossier on its review map: what the board is, what is on it, how many of each, what
  the copper implies, and what the photos cannot settle. Triggers on: "understand this
  board", "what is this board", "survey this board photo", "what parts are on this
  board", "count the LEDs on this board", "read the silkscreen", "board dossier".
argument-hint: "[board photos to survey]"
---

# KiCAD Board Dossier — Comprehension Methodology

This skill turns photographs of a physical board into a **`dossier` section on
its review map** — a written, evidence-backed account of what the board is. It
produces claims, the pixels each claim came from, and the questions the photos
could not answer. It never creates or edits a `.kicad_sch` or a `.kicad_pcb`,
and it never places or wires anything.

It runs **after** `scan_pcb_photo` has minted a `map_id` and **before** any
design work. The design brief is the next skill's job
(`kicad-design-reconstruction`), and it may not start until a human has
approved this dossier.

Two words carry the whole document: **observed** (you can point at the pixels)
and **inferred** (you reasoned to it). Every claim states which it is, in
`basis`, with a numeric `confidence`. A reviewer must be able to tell them
apart at a glance, not by reading your tone.

---

## Prerequisites

- A `map_id` from `scan_pcb_photo`. This workflow never mints a map directory:
  `prepare_board_photo` refuses a `map_id` whose directory does not exist.
- The absolute path of every source photo, already in the map's
  `source_images`.
- `Read` — the host's own file reader. It renders an image into the
  conversation, which is the one thing no MCP tool does:
  `prepare_board_photo` *writes* a PNG, it cannot *show* you one. You only
  ever `Read` two sets of files: the exact source photos in `source_images`,
  and the views under
  `<project_dir>/.konnect/photo_intake/<map_id>/views/` that you produced
  yourself.

---

## Toolset Loading

```
load_toolset('photo_intake')   # check_retrace, scan_pcb_photo, prepare_board_photo, save_photo_review_map, load_photo_review_map, approve_photo_review_map
```

That is the whole surface. Do NOT load a schematic, PCB or library toolset
here — comprehension ends at an approved dossier, and loading a mutating
toolset only invites you to use it.

### References by decision

- Read [`references/dossier-schema.md`](references/dossier-schema.md) before
  writing or editing a `dossier`: every field, every array element's own field
  list, which fields are required on a claim, and the exact shape of an
  evidence pointer.
- Read the kicad-photo-intake skill's `references/review-map-schema.md` for the
  map the dossier sits inside: what the server owns, and what the approval
  hash covers.

---

## Workflow

Each step ends with evidence. A step whose evidence you could not collect
leaves the dossier `INCOMPLETE`; say which step and why, rather than filling
the gap with a plausible value.

### 1. Survey — look at every photo before zooming into any of it

`Read` every entry in `source_images`, one at a time. Before you crop
anything, write down for each photo: which side of the board it shows, the
overall outline and its rough proportions, which distinct part classes are
visible, where the connectors and mounting holes are, and whether the
silkscreen is legible at this resolution. This first pass decides what is
worth zooming into; a survey that starts with crops zooms into the parts you
already expected and misses the ones you did not.

### 2. Zoom — make the views every later claim will point at

```
prepare_board_photo(image_path, project_dir, map_id, crop, rotate, scale, label)
```

One image in, one view out; call it once per view. Make a view for each of:

- the silkscreen identity strip — board name, revision, voltage, date code;
- every connector and terminal block, with its silkscreen legend;
- each group of same-class parts you intend to count, one view per group, with
  the groups disjoint so their counts can be summed;
- the corners, for mounting holes and their plating;
- the copper side whole, plus one view per trace chain you intend to follow.

`label` each view for what it shows (`boardA_top_topleft_resistors`), because
that label is the name every evidence pointer will carry. Unlabelled views are
numbered, which makes an evidence pointer unreadable a week later. Labels are
one-shot: reusing a label, or a number that collides after you deleted an
earlier view, is refused rather than silently replacing the file — pick a
new label if you meant a different crop.

Coordinates: `crop` is interpreted in the **EXIF-oriented** space the response
reports as `source_size_px`, not in whatever your own reader displayed. Call
the tool once without `crop` for each new photo, read `exif_orientation` and
`source_size_px`, and pick every rectangle in that space.

Bounds worth knowing before you hit them: a source over 50 MP is refused
before it is decoded; `scale` outside `0.25`-`4.0` is refused rather than
clamped, so `output_size_px` is always the size you asked for; a computed view
whose long side exceeds 4096 px is refused before it is allocated; an
out-of-bounds `crop` is an error naming the image's actual oriented size, and
writes nothing.

### 3. Classify and count by visual class

A **visual class** is what the part *looks like*, not what you think it does:
5 mm clear-lens LED, axial through-hole resistor, 2-position screw terminal,
0805 SMD passive, 8-pin DIP IC, HC-49 crystal, radial electrolytic, TO-220
regulator. Name each class in `visual_class` as a short token
(`led_5mm_clear`, `axial_resistor_tht`). You classify from the photograph; you
never infer a class from the circuit you expect the board to be.

Count each class with a method you can name, and put that name in
`count_method`:

| `count_method` | Use when | Known failure mode |
|---|---|---|
| `manual_count_by_region` | fewer than ~40 of a class, or an irregular layout | miscounts at region boundaries — keep the regions disjoint and sum them |
| `blob_count` | many identical parts against a contrasting background | merges touching parts; sensitive to the threshold |
| `hough_circles` | round parts — LED domes, electrolytic cans | undercounts a staggered grid badly |

If none fits, state your own method name and describe it in `notes`. A count
with no stated method is not a count.

Then:

- `locations[]` carries one entry per group, each with `region`, the `view` it
  was counted in, the `rect_px` bounding it in that view, an **integer**
  `count`, and `kind` — `observation` or `cross_check`.

  **The class `count` equals the sum of `count` over the `observation`
  entries.** That is what makes a total checkable: a reviewer adds the groups
  up and either reconciles or finds the discrepancy. So keep the
  `observation` regions **disjoint** — overlapping ones break the sum
  silently — and mark every re-count of ground already covered (a second
  method, a zoomed re-count, the same parts seen from the other side) as
  `cross_check`, which is excluded from the sum.

  Never write the per-group number in prose. "a row of 5 near the terminal" in
  `region` with no `count` gives a document nobody can reconcile, even when
  the total is right.
- `count_confidence` is yours, unrounded.
- When a second method gives a different number for the **whole class**, record
  it in `count_alternatives` with its own method, its number and a note saying
  why you trust the one you put in `count`. **Two methods disagreeing is itself
  evidence.** Collapsing it into the trusted number alone throws away the only
  signal a reviewer has that the count is soft. (A second method over one
  *region* is a `cross_check` entry in `locations[]`; `count_alternatives` is
  for a rival total.)

### 4. Read the silkscreen, and only what is legible

One `silkscreen_markings` entry per legible string: the text **verbatim**
(spacing, revision and all), `location_px`, `basis: "observed"`, a
`confidence`, and evidence naming the view and the rectangle it was read from.

Never repair a string you cannot read. A half-legible `SEMAF…` is recorded as
what you actually saw, plus an entry in `open_questions` — never completed
into the word you expect. A supply voltage printed on the board (`24V` beside
the terminal) is **observed** and belongs here; the same number reached by
arithmetic is **inferred** and belongs in `topology_claims`.

### 5. Correlate the scan's boxes — as hints, never as classifications

`scan_pcb_photo`'s components are coarse boxes, often from a contour pass with
no marking read at all. Correlate them to your own survey by bounding-box
overlap: for a scan box that lands on a part you classified, add a
`retrace_correlation` entry `{component_id, bbox_px, visual_class,
overlap_confidence}`, where `overlap_confidence` is how much of the box you
believe actually covers that part.

Three rules, and they are the same rules `subcircuit_hints` already carries:

- a scan box never creates a `component_survey` entry;
- a scan box never changes a `count`;
- a `pattern_matches` hint never establishes a topology claim.

A correlation makes the scan searchable from your dossier. It is not evidence
for anything.

### 6. Physical — pixels always, millimeters only once a scale is resolved

Record `board_size_px`, `mounting_holes[]`, `connectors[]`, and
`layers_visible` — which layers these photographs actually show, so nobody
later reads silence about inner layers as an absence of them.

Two strings in here are read by machine as well as by people, so they have a
fixed shape:

- **`mounting_holes[].role` literally contains `plated` or `unplated`** —
  `plated_corner`, `unplated`, `plated_standoff`. Plating is the property a
  reader filters on, and "corner hole" does not state it. Each entry also
  carries `position_px`, a `count` when it stands for several identical holes,
  and evidence.
- **`connectors[].edge` starts with `top`, `bottom`, `left` or `right`** —
  `bottom`, `bottom-left`, `right edge near the holes`. Any refinement follows
  the side, so the side stays parseable. A connector that is not on an edge is
  `interior`. Each entry also carries its `type`, `location_px`, `basis`,
  `confidence` and evidence.

**`board_size_mm` stays `null` until a scale is resolved**, and a scale is
resolved in exactly two ways:

1. the user states a physical dimension — a board edge, a hole pitch, an
   overall size;
2. you measure a pixel distance between two features whose real dimension is
   fixed by a part you have **identified from the photo** — the 5.08 mm pitch
   of a terminal block you can read, an M3 hole, a 0805 body — and the user
   confirms the identification.

There is no third way. When one of those resolves, write **both**
`mm_per_px` and `evidence` on `scale_reference` — they are always a pair, and
a millimeter figure you cannot name evidence for must not be written at all.
Otherwise `scale_status` states exactly what is missing ("needs the hole pitch
or a board edge length in mm"), and `open_questions` carries the same gap in
the user's words. Never estimate a scale from the pixels alone, and never back
one out of a part's "typical" size.

### 7. Topology claims — competing hypotheses, each with its arithmetic

Where the copper matters and the photos do not settle it, write a
`topology_claims` entry: the `question` in plain words, `basis`, the
`evidence` that establishes what you *can* see, one entry per candidate answer
in `hypotheses[]`, and a `resolution_path` saying how the question could be
closed with more evidence.

Worked shape, from a 24 V LED lamp board:

- **Observed.** Silkscreen `24V` beside a 2-pin screw terminal: the supply is
  24 V. Survey: 107 clear 5 mm LEDs, 19 axial resistors. Copper side: serpentine
  mask-covered traces chaining LED pads, which establishes *series strings* and
  says nothing about their length.
- **Hypothesis A** (`confidence` 0.6) — 19 strings of 6 LEDs, one series
  resistor each. `calculation`: `6 x Vf 2.1 V = 12.6 V; 24 V - 12.6 V = 11.4 V
  across R; at 20 mA -> 570 ohm -> nearest E24 620 ohm; P = I^2 R = 0.25 W ->
  a 1/2 W axial part`. `assumptions`: Vf averaged at 2.1 V for a red LED,
  20 mA target per string, the 24 V rail regulated externally.
- **Hypothesis B** (`confidence` 0.3) — fewer, longer strings of 8-10 LEDs,
  with some strings sharing a resistor; no calculation yet, because the string
  count is not fixed.
- **`resolution_path`** — count the LEDs along one continuous serpentine trace
  on the copper view, or measure one resistor, and hypothesis A or B falls.

The rules a hypothesis obeys:

1. **Every numeric hypothesis states its `calculation` and its
   `assumptions`.** The calculation is the arithmetic written out with units,
   not its result. The assumptions list every value *you* supplied that the
   board did not — a forward voltage, a target current, a tolerance, a
   regulated supply.
2. **Competing answers stay as separate hypotheses**, each with its own
   `confidence`. You never pick one and delete the other, never average two
   confidences into a third, and never promote a hypothesis to an observation
   because it is the only one you could calculate.
3. A value taken from a datasheet, a standard series or your own experience is
   **inferred**, however standard it is. The board only observes what is
   printed on it.

### 8. Open questions, and the seed

`open_questions[]` is the list the human is being asked to close: the missing
scale, the resistor bands nobody could read, the string length, every value a
hypothesis had to assume. A question belongs here even when a hypothesis
already guesses at it — the hypothesis is the guess, the open question is the
admission.

`design_brief_seed` is a short sketch of the direction the board implies —
topology idea, rough BOM shape, physical spec — plus
`depends_on_open_questions` naming the `claim_id` values it rests on. It is a
sketch, not the design brief: `kicad-design-reconstruction` starts from it and
replaces it.

### 9. Save, then hand the dossier to the human

- `save_photo_review_map(project_dir, map)` — the dossier travels inside
  `map`, as `map.dossier`. A section is an object or it is **absent**: a
  `"dossier": null` is rejected with "Omit the key entirely to remove the
  section".
- The save **revokes any existing approval** the moment the dossier changes
  the map's hashed content. That is the gate working, not a failure.
- Print the returned `saved_path` and say in plain words that the JSON is
  theirs to edit by hand; hand edits survive a reload and are the expected
  workflow.
- Walk them through, in this order: the identity claim and what it was read
  from; each class count with its method and its alternatives; every
  `topology_claims` question with its competing hypotheses; the scale gap and
  exactly what would close it; `open_questions` in full.
- `approve_photo_review_map(project_dir, map_id)` **only** after they have
  explicitly approved that dossier. A finished survey, a plausible table, or
  silence is not approval.
- Re-check with `load_photo_review_map(project_dir, map_id)` that
  `approval_valid` is true before you report the stage `DONE`.

---

## Observed vs inferred

| Claim | `basis` | Why |
|---|---|---|
| "The silkscreen reads `SEMAFARO 1.3 24V 03/2020`" | `observed` | You can point at the rectangle it was read from. |
| "There are 19 axial resistors" | `observed` | A count from views, with a stated method and per-region sums. |
| "The supply is 24 V" (printed beside the terminal) | `observed` | Read off the board. |
| "The LEDs are in strings of 6" | `inferred` | The copper shows series chaining, not string length. |
| "The series resistor is about 620 ohm" | `inferred` | Arithmetic over an assumed Vf and target current. |
| "The board is 100 x 100 mm" with no scale supplied | **not written at all** | `board_size_mm` stays `null`; the gap goes in `open_questions`. |

---

## Rules

1. **Never write a claim without evidence** — every claim-bearing object
   carries `basis`, `confidence`, and evidence entries shaped
   `{view, rect_px}` naming a view file or a `source_images` entry.
2. **Never invent a count** — no method, no count; every `locations[]` entry
   carries an integer `count` and a `kind`, and the `observation` entries sum
   to the class `count`.
3. **Never collapse disagreement** — two counts go in `count_alternatives`,
   two answers go in `hypotheses[]`, both with their own confidence.
4. **Never estimate a scale** — `mm_per_px` and its `evidence` are written
   together or not at all, and `board_size_mm` stays `null` until one exists.
5. **Never complete a silkscreen string** you could not read; record what you
   saw and open a question.
6. **Never treat a scan box or a `subcircuit_hints` entry as a
   classification** — correlate them, then keep counting from the photos.
7. **Never place, wire, route or edit a KiCad file** in this workflow. The
   dossier is the only artifact.
8. **Never approve on the user's behalf**, and re-check `approval_valid`
   rather than the map's own `approved` field.
9. **Report `INCOMPLETE` rather than filling a gap** — name the step, the
   missing evidence, and what would close it.
