---
name: kicad-photo-to-board
description: |
  The single entry point for turning photographs of a physical board into a rebuilt
  KiCad design: scan, dossier, human approval, design brief, human approval, schematic,
  layout, review. Sequences the agents that own each stage and states what each gate
  refuses. Triggers on: "I have photos of a board, recreate it", "reverse engineer and
  rebuild this board", "clone this PCB from photos", "rebuild this board in KiCad",
  "photo to board".
argument-hint: "[board photos and the target project directory]"
---

# Photo to Board — The Whole Pipeline

You have photographs of a physical board and you want a KiCad design. That is
eight stages, two human approvals, and one artifact that carries the state
between them: the **review map** at
`<project_dir>/.konnect/photo_intake/<map_id>/review_map.json`.

This skill is addressed to the **orchestrating conversation** — the session
that loaded it. It is not a script any single agent runs: see "Delegation"
below.

Nothing here invents. Every stage either produces evidence for what it claims
or reports `INCOMPLETE` and names what is missing. A photo cannot show an
inner layer, cannot give a scale, and cannot state a forward voltage; the
pipeline is built to say so rather than to fill those in.

---

## Toolset Loading

The session itself needs the intake toolset only for the opening capability
check; each stage's agent loads its own.

```
load_toolset('photo_intake')   # check_retrace, scan_pcb_photo, prepare_board_photo, save_photo_review_map, load_photo_review_map, approve_photo_review_map
```

### References by decision

- The methodology for each middle stage lives in its own skill: the
  kicad-board-dossier skill (comprehension) and the kicad-design-reconstruction
  skill (design brief). Their `references/dossier-schema.md` and
  `references/design-brief-schema.md` are the field-level contracts for the two
  sections this pipeline writes onto the map.
- The kicad-photo-intake skill's `references/review-map-schema.md` describes
  the map itself and the approval hash both gates run on.

---

## The stages

| Stage | Owner | Consumes | Produces | Gate | `INCOMPLETE` when |
|---|---|---|---|---|---|
| 1. Capability + scan | `pcb-photo-intake-agent` | the photo(s), the project directory | `map_id`, raw `analysis.json`, the base review map | — | `check_retrace` reports `available: false` |
| 2. Comprehension | `pcb-photo-intake-agent` | the photos (via `Read` and `prepare_board_photo`), the scan result | `dossier` | — | a claim has no `{view, rect_px}` evidence, or a question carries one hypothesis where the evidence supports two |
| **3. Approval #1** | **the human** | `dossier` | `approved: true` over the dossier's content | `approve_photo_review_map` | the human does not approve |
| 4. Design reconstruction | `pcb-design-reconstruction-agent` | the approved `dossier` | `design_brief` | — | `approval_valid` is false, or a `bom` entry stays `resolution_status: unresolved` |
| **5. Approval #2** | **the human** | `design_brief` | `approved: true` over dossier + brief | `approve_photo_review_map` | the human does not approve |
| 6. Schematic build | `kicad-schematic-build-agent` | the approved `design_brief` | `.kicad_sch`, ERC result | ERC | `approval_valid` is false, or any `bom` entry is `unresolved` |
| 7. PCB layout | `kicad-pcb-layout-agent` | the approved `design_brief`, the built schematic | `.kicad_pcb` | DRC | `approval_valid` is false, or `physical_constraints.unresolved` names `board_size_mm` or a load-bearing row |
| 8. Design review | `kicad-design-review-agent` | the built board | review findings | — | the board is not routed |

Stages 1 and 2 are one agent and one invocation. Every other row is its own
invocation.

---

## Delegation: agents cannot spawn agents

An agent runs in its own isolated context and returns to the session that
invoked it. **No agent in this pipeline invokes another one.** A skill that
told one to would produce an agent that either silently does nothing at that
step or narrates a handoff that never happened — and the human gate between
two stages would be crossed by a sentence.

So the table above is a list of things **you**, the session, do, one at a time:

1. Invoke the stage's agent with the map's `map_id`, the project directory, and
   what the previous stage returned.
2. Read the five-field block every agent ends with:

   | Field | Meaning |
   |---|---|
   | `stage` | Which row of the table above just ran. |
   | `map_id` | The map it worked on. |
   | `produced` | The artifact path, or the map section it wrote. |
   | `verdict` | `DONE` or `INCOMPLETE`. |
   | `blockers` | Empty on `DONE`; otherwise what stopped it, by name. |

3. On `INCOMPLETE`, show the blockers to the human and stop that branch. Do
   not invoke the next stage "to see how far it gets".
4. On a row with a **Gate**, do what the gate says before the next invocation —
   for the two approval rows, that means showing the human the artifact and
   getting an explicit answer.

Every stage re-checks `approval_valid` itself by calling
`load_photo_review_map`. None of them trusts your summary of the previous
stage, and you should not expect them to.

---

## What the human sees at each approval

### Approval #1 — the dossier

The question is "does this describe your board?", and the answer has to be
checkable. Show, from `dossier`:

- the `identity` claim and the silkscreen it was read from;
- every `component_survey` class with its `count`, its `count_method`, and any
  `count_alternatives` — so a disagreement between two counting methods is
  visible, not averaged away. Add the per-group `locations` counts up yourself:
  the `observation` entries must sum to the class total, and a class where they
  do not is the first thing to send back;
- `physical`: hole positions, connector positions and edges, and the
  `scale_status` — **say plainly whether the board size in millimeters is
  known**, and what measurement would resolve it;
- every `topology_claims` question with each hypothesis, its `confidence` and
  its `calculation`;
- `open_questions` in full.

Then `approve_photo_review_map(project_dir, map_id)` — only on an explicit
yes about that dossier.

### Approval #2 — the design brief

The question is "is this what we should build?". Show, from `design_brief`:

- the blocks, and which `topology_claims` hypothesis the brief chose;
- every `calculated_values` entry with its `formula` and its `assumptions`;
- the BOM, split into resolved entries and `unresolved` ones with their
  `candidates` — an unresolved entry is a decision the human is being asked to
  make, not a footnote;
- `physical_constraints.unresolved` in full, flagging any load-bearing row;
- `open_questions`.

Then `approve_photo_review_map(project_dir, map_id)` again. The first approval
does not carry over: adding the brief changed the map's hashed content and
revoked it. That is the mechanism, working.

---

## Honest limits

Say these out loud early, not after the user is invested:

- **Inner layers are invisible.** A photograph shows the outer copper and the
  silkscreen. A 4-layer board is reconstructed as what its outside implies, and
  the dossier's `layers_visible` says so.
- **Scale does not come from pixels.** Millimeters exist only when the user
  states a dimension, or when a pixel distance is tied to a feature whose real
  size is fixed by a part identified in the photo. Otherwise `board_size_mm`
  stays `null` and the gap is an open question. Nothing in the pipeline
  estimates it.
- **Component values are usually inferences.** Colour bands and part markings
  are frequently unreadable at photo resolution. A resistor value derived from
  a supply voltage and an assumed LED forward voltage is a calculation with
  stated assumptions, not a reading — and it carries a `confidence` saying so.
- **The scan is a hint generator.** `scan_pcb_photo` without its optional
  extras is a contour pass: coarse boxes, no markings, no values. It never
  produces a netlist, and its synthetic KiCad output is never read.
- **Input bounds.** `prepare_board_photo` refuses a source over 50 MP before
  decoding it, refuses a `scale` outside `0.25`-`4.0` rather than clamping it,
  and refuses a view whose long side would exceed 4096 px. A modern phone photo
  at full resolution can hit the first of those; downscale it before intake.
- **Both gates are text-enforced, not server-enforced.** The tools compute
  `approval_valid` honestly, and every agent in this pipeline is instructed to
  refuse without it — but the `sch_*` and `pcb_*` handlers themselves do not
  check it. Do not route around the agents.

---

## Rules

1. **One stage at a time, invoked by the session.** No agent invokes another.
2. **Never skip an approval**, and never treat "the artifact exists" or a
   finished stage as approval.
3. **Never pass a summary in place of the map.** Every stage loads it itself.
4. **Stop on `INCOMPLETE`** and show the blockers; a later stage cannot recover
   evidence an earlier one did not collect.
5. **Never mutate a KiCad file before stage 6.** Stages 1-5 produce documents
   only.
