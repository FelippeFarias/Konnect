# Photo to board — the reverse-engineering workflow

You have photographs of a physical PCB and you want a KiCad design. Konnect
does that in eight stages with **two human approvals**, and it is built to tell
you what a photograph cannot answer rather than to fill those answers in.

This page is the map of the pipeline: what runs, what it writes, what each gate
refuses, and where it will honestly stop. The AI-facing instructions live in the
bundled `kicad-photo-to-board` skill; this is the version for the person
holding the board.

---

## The pipeline

```
    photos ──► 1. capability + scan ──► map_id, analysis.json
                        │
                        ▼
               2. comprehension ──────► dossier        (what this board IS)
                        │
                 ╔══════▼══════╗
                 ║ APPROVAL #1 ║  you read the dossier and approve it
                 ╚══════╤══════╝
                        ▼
            3. design reconstruction ─► design_brief   (what to BUILD)
                        │
                 ╔══════▼══════╗
                 ║ APPROVAL #2 ║  you read the brief and approve it
                 ╚══════╤══════╝
                        ▼
               4. schematic build ────► .kicad_sch  + ERC
                        │
                        ▼
                 5. PCB layout ───────► .kicad_pcb  + DRC
                        │
                        ▼
                6. design review ─────► findings
```

| Stage | Who runs it | Produces | Stops when |
|---|---|---|---|
| Capability + scan | `pcb-photo-intake-agent` | `map_id`, the raw `analysis.json`, the base review map | the optional `retrace` package is not installed |
| Comprehension | `pcb-photo-intake-agent` | the `dossier` section | a claim has no evidence pointer, or a question has one answer where the evidence supports two |
| **Approval #1** | **you** | the approval stamp over the dossier | you do not approve |
| Design reconstruction | `pcb-design-reconstruction-agent` | the `design_brief` section | the dossier is not approved, or a BOM entry will not resolve to a real library part |
| **Approval #2** | **you** | the approval stamp over dossier + brief | you do not approve |
| Schematic build | `kicad-schematic-build-agent` | `.kicad_sch`, ERC result | the brief is not approved, or a BOM entry is unresolved |
| PCB layout | `kicad-pcb-layout-agent` | `.kicad_pcb`, DRC result | the brief is not approved, or a load-bearing constraint is unknown |
| Design review | `kicad-design-review-agent` | review findings | the board is not routed |

Each stage runs as its own agent and hands back to the conversation. **No agent
starts the next one** — that is what keeps the two approvals real rather than
narrated.

---

## The artifact trail

Everything lives under your project, and all of it is readable:

```
<project_dir>/.konnect/photo_intake/<map_id>/
├── analysis.json       the raw scan output, kept as evidence and never edited
├── views/              the zoomed PNGs the AI made to read the board
│   ├── boardA_top_topleft_resistors.png
│   └── boardA_zoom3.png
└── review_map.json     the review map: components, nets, dossier, design_brief
```

`review_map.json` is **yours to edit by hand**. Hand edits survive a reload and
are the expected way to correct a count, a value or a claim. Every edit to
reviewed content revokes the approval that covered it, which is the point: the
approval is over specific content, not over the file's existence.

The two sections this workflow adds:

- **`dossier`** — what the photos say the board *is*. An identity claim, a
  survey of every visual class with its count and the method behind it, the
  silkscreen read verbatim, the physical layout, the topology questions with
  their competing answers, and the open questions. Every claim says whether it
  was `observed` or `inferred`, carries a confidence, and points at the exact
  rectangle of the exact view it came from.
- **`design_brief`** — what to *build*. Functional blocks, per-block circuits
  with the arithmetic and the assumptions behind each value, a BOM whose every
  part resolves to a real KiCad library symbol and footprint, and the physical
  constraint record the layout stage works from.

---

## The two approvals

Approval is a tool call (`approve_photo_review_map`) that stamps the hash of
exactly the content you approved. Nothing else grants it, and any later edit to
that content revokes it until you approve again.

### Approval #1 — "does this describe my board?"

You are shown the dossier: the identity and the silkscreen it was read from,
every part count with the method that produced it and any second method that
disagreed, the hole and connector positions, whether the board size in
millimeters is known, every topology question with each competing answer and its
arithmetic, and the open questions in full.

Approve when the description matches the board in your hand. Correct the JSON
where it does not — that is faster than arguing, and the revoked approval makes
the correction visible.

### Approval #2 — "is this what we should build?"

You are shown the design brief: the blocks, which competing hypothesis the
design chose and why, every calculated value with its formula and assumptions,
the BOM split into resolved parts and unresolved ones with their candidates, and
every physical constraint still unanswered.

An **unresolved BOM entry is a decision you are being asked to make**, not a
footnote. The schematic stage will refuse to place it rather than pick one of
its candidates.

The first approval does not carry over to the second: adding the design brief
changes the approved content and revokes the first stamp. That is the mechanism
working.

---

## Honest limits

**Inner layers are invisible.** A photograph shows outer copper and silkscreen.
A four-layer board is reconstructed as what its outside implies, and the dossier
says which layers the photos actually showed.

**Scale does not come out of pixels.** Millimeters exist only when you state a
dimension — a board edge, a hole pitch, an overall size — or when a pixel
distance is tied to a feature whose real size is fixed by a part identified in
the photo (a 5.08 mm terminal pitch, an M3 hole). Otherwise the board size stays
`null` and the gap is listed as an open question. Nothing in the pipeline
estimates it, and a millimeters-per-pixel figure is never written without the
evidence it rests on.

**Most component values are inferences.** Resistor colour bands and part
markings are usually unreadable at photo resolution. A resistor value derived
from the supply voltage and an assumed LED forward voltage is a calculation with
stated assumptions, not a reading — and it carries a confidence that says so.
Check those values against the physical parts before ordering a board.

**The scan is a hint generator.** `scan_pcb_photo` needs the optional `retrace`
Python package; without its machine-learning extras it is an OpenCV contour
pass that produces coarse boxes, no markings and no values. It never produces a
netlist, and its synthetic KiCad output is never read by anything.

**Both approval gates are enforced in the agents' instructions, not inside the
schematic and board tools.** Every agent in this pipeline is required to call
`load_photo_review_map` itself and check the server-computed `approval_valid`
flag. Do not route around the agents by handing a map's contents to a build
tool directly.

---

## Input bounds you can hit

`prepare_board_photo` makes the zoomed views the AI reads the board from. It
refuses rather than degrading silently:

| Bound | Value | Why |
|---|---|---|
| Source image size | 50 megapixels | Checked from the file header, before a pixel is decoded. A modern phone at full resolution can exceed this — downscale before intake. |
| `scale` | 0.25 to 4.0 | Rejected outside that range rather than clamped, so the reported output size is always the size that was asked for. |
| Output long side | 4096 px | Checked before the buffer is allocated. |
| `crop` rectangle | must fit the image | An out-of-bounds crop is an error naming the image's real oriented size, and writes nothing. |

EXIF orientation is applied **before** the crop, and the response reports which
orientation it applied and the resulting image size — so the coordinate space
every rectangle is measured in is the one the tool actually used.

---

## Getting started

1. Install the optional dependency: `pip install
   git+https://github.com/ericrihm/retrace.git` (Python 3.10+), and point
   Konnect at that interpreter with the `photo_intake.retrace_python_path`
   config key or the `RETRACE_PYTHON` environment variable.
2. Take the photos: the component side and the copper side, square-on, in even
   light, plus a close-up of any silkscreen legend and of each connector.
3. Measure one thing — a board edge or the mounting-hole pitch — in
   millimeters. It is the single input that turns the whole reconstruction from
   proportional to physical.
4. Ask for it: "I have photos of a board, recreate it", with the photo paths
   and your KiCad project directory.

See also: [TROUBLESHOOTING.md](TROUBLESHOOTING.md) for `retrace` install
problems, and the `kicad-photo-to-board`, `kicad-board-dossier` and
`kicad-design-reconstruction` skills for the methodology each stage follows.
