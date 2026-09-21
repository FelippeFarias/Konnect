---
name: kicad-pcb
description: |
  Workflow skill for KiCAD PCB layout and routing via MCP tools. Triggers on: "layout the board",
  "route traces", "PCB", "place footprints", "place components", "placement", "copper pour",
  "board outline", "differential pair", "board setup", "track width", "via", "zone", "design rules",
  "stackup", "silkscreen", "return path", "layout methodology", "spaghetti board".
argument-hint: "[layout task]"
---

# KiCAD PCB Layout Workflow

This skill guides Claude to perform PCB layout using Konnect MCP tools.
ALL modifications go through MCP tools — never edit .kicad_pcb files directly.
The only exception is the konnect skill's scripted board fallback, for a
change no tool can make: KiCad's own Python API with KiCad closed, never a
text tool.

Layout is not "connect the schematic's dots". The order is: understand the
circuit → write the constraints → plan layers and return currents → place by
functional block with pins facing their destinations → validate the critical
paths → route by criticality → review → measure. A visually tidy board is not
necessarily an electrically good one; a good placement removes most routing
problems before the first trace exists. Read
[`references/layout-methodology.md`](references/layout-methodology.md) before
any placement or routing task, use
[`references/routing-playbook.md`](references/routing-playbook.md) for the
techniques that make routing clean (layer plan, planar fan-out, bus lanes,
repeated blocks, dense corners, autorouter policy, ground), and close the two
gates — [`references/placement-gate.md`](references/placement-gate.md) before
routing and [`references/routing-gate.md`](references/routing-gate.md) before
claiming completion.

---

## Prerequisites

Most PCB layout operations require KiCAD to be running with the board file open. The IPC
connection communicates with the running KiCAD instance in real-time.

Some board-construction and component tools have guarded closed-board paths. IPC-first
tools fall back to the file only when the transport is unreachable and the target board
has not been observed live during this server session. File-only operations such as
`flip_component` proceed only when KiCad does not hold the target board open. These
paths use revision-aware atomic writes: placement preserves pads, graphics, attributes,
and models; moves preserve the existing angle; rotations update the footprint and its
child angles; flips mirror supported geometry and swap front/back layers. A reachable
KiCad rejection stays closed instead of racing the editor.

`unsafe_file_fallback` is a stop condition. It means Konnect reached this board live
earlier in the current server session but IPC is now unreachable, so the saved file may
be older than lost editor state. Pause mutation work, tell the user that Konnect left
the file unchanged, and ask them to reopen/recover, reconcile, and save the board in
KiCad. Continue through live IPC afterward. Preserve the guard: do not retry-loop,
restart Konnect automatically, or edit `.kicad_pcb` directly. If the user confirms a
clean close and an authoritative saved file, they may restart Konnect to deliberately
begin a new closed-board session.

If connection fails:
- Tell the user to open KiCAD and load the project
- The board (.kicad_pcb) must be open in the PCB editor
- KiCAD's IPC API must be enabled (default in KiCAD 8+)

---

## Toolset Loading

Before any PCB work, load the required toolsets:

```
load_toolset('pcb_board')        # board outline, layers, setup, stackup
load_toolset('pcb_components')   # place, refresh, move, rotate, align footprints
load_toolset('pcb_routing')      # traces, vias, differential pairs
load_toolset('sch_export')       # update PCB from the saved schematic hierarchy
```

Zones (`pcb_board`: add_zone; `pcb_routing`: add_copper_pour), component/net queries (`pcb_components`: find_component, get_component_list; `pcb_board`: get_board_info), and bulk placement (`pcb_components`: place_component_array, align_components, duplicate_component) are already covered by the toolsets loaded above.

Load additional toolsets as needed:

```
load_toolset('config')           # design rule storage: add_design_rule, list_design_rules
load_toolset('verification')     # run_drc, set_design_rules, set_predefined_sizes, check_clearance
```

Always call `get_active_toolsets()` first to see what is already loaded.

### References by decision

- Read [`references/layout-methodology.md`](references/layout-methodology.md)
  at the start of any placement or routing work: constraint record,
  block-and-flow placement, placement order, return-path planning, routing
  priority, and the final review table.
- Read [`references/routing-playbook.md`](references/routing-playbook.md)
  before placing a board with repeated blocks, interchangeable driver outputs,
  long buses, a module, or a dense connector corner, and again before the
  first trace: it records how a 400 mm production board was routed cleanly
  after two autorouter attempts were rejected.
- Read [`references/placement-gate.md`](references/placement-gate.md) when
  placement is believed done. Routing does not start until it passes.
- Read [`references/routing-gate.md`](references/routing-gate.md) before the
  first trace (pad selection and path rules) and again before reporting the
  layout complete (post-routing evidence).
- Read [`references/layer-reference.md`](references/layer-reference.md) when
  selecting a copper, fabrication, user, or mechanical layer or deciding which
  side owns an item.
- Read [`references/trace-width-table.md`](references/trace-width-table.md) when
  sizing a current-carrying trace, via, or controlled-impedance route. It defines
  the required calculation inputs and acceptance record; it is not a lookup table.
- Read [`references/design-rules.md`](references/design-rules.md) when creating
  netclasses, configuring project constraints, or adjudicating DRC results.

---

## Layout Workflow

Follow this sequence. Each phase ends with evidence; a phase without its
evidence leaves the layout `INCOMPLETE`.

0. **Understand the circuit** — read the net inventory from the saved
   schematic (`export_netlist_summary`, `list_schematic_nets`,
   `get_net_components` in `sch_analysis` / `sch_export`). Name the
   functional blocks and the current path through them. Classify every net:
   power (with current), switching or pulsed, clock/RF/fast edge, sensitive
   analog or reference, ordinary signal. Use the schematic build's layout
   handoff when one exists.
1. **Constraint record** — fill the table in
   `references/layout-methodology.md` section 1. Ask the user for any
   load-bearing row the request does not establish (board size, connector
   sides, enclosure, operating environment, currents, voltages, layers,
   fabricator and panel configuration); do not assume.
2. **Board outline, holes, rules** — `set_board_size` or draw Edge.Cuts geometry. Both outline tools
   append, so resize with `delete_graphics(layer='Edge.Cuts')` first — a second call
   without it leaves two overlapping outlines and a DRC failure. A notch or
   cutout on an edge is drawn by breaking that edge into segments around it; a
   rectangle drawn over the edge line makes a self-intersecting outline (four
   invalid-outline errors). Any outline change needs a zone refill and a
   regeneration of every fabrication file. Add mounting
   holes and keep-outs, then encode the fabricator limits with
   `set_design_rules` and read them back with `get_design_rules`.
   `set_design_rules` covers clearance, trace width, via drill, via size, and
   hole-to-hole only; copper-to-edge, hole clearance, annular ring, minimum
   connection width, silkscreen clearance, and text size need KiCad's Board
   Setup or custom rules. A constraint left at 0 disables its check. While
   KiCad holds the board open, its next save rewrites the project file and
   silently discards rules and netclasses written by tools: write them with
   the board closed, or have the user set them in Board Setup, and read them
   back after KiCad's next save.
3. **Update from schematic** — call `update_pcb_from_schematic` first with
   `dry_run: true`. Review `status`, `coverage`, `diagnostics`, and staged positions.
   Apply only with `dry_run: false` and the exact returned
   `expected_plan_revision` value. The saved schematic hierarchy must be closed in the
   schematic editor, and the target board must be open in KiCad. A conflict is
   non-mutating; resolve it and rerun the dry run. A successful apply is one KiCad
   undo entry, so Ctrl-Z reverses the whole update.
   - Konnect's schematic tools write the files on disk, so an open schematic
     editor holds an older copy: ask the user to close it and choose not to
     save (or to reload), once per phase.
   - A changed footprint library ID is reported as a conflict: delete the
     unrouted footprint and sync again, or have the user run KiCad's Update
     PCB from Schematic (F8).
   - A new series part that splits a routed net can be refused as a routed-pad
     net change, even after the copper on both nets is deleted; do not delete
     routing to satisfy the check. The native path is KiCad's Update PCB from
     Schematic, with "Delete footprints with no symbols" unchecked so
     board-only holes and fiducials survive; then re-route the affected copper
     and run DRC with schematic parity.
   - After F8 the new footprints follow the cursor; the user clicks to drop
     them. Pressing Esc cancels their insertion, and while parts are on the
     cursor IPC answers "not ready".
   - Removing a symbol leaves its footprint, branch tracks, and trunk
     overhangs on the board: delete them, trim trunks back to the last tap,
     and confirm with DRC dangling-track items and schematic parity.
4. **Refresh changed libraries** — when a linked footprint library changed, use
   `update_footprints_from_library`, the MCP equivalent of KiCad **Tools → Update
   Footprints from Library**. This is distinct from `update_pcb_from_schematic`:
   it refreshes supported library-owned pads, graphics, attributes, metadata, and
   3D models without changing references, placement, side, rotation, KIID, symbol
   metadata, instance overrides, or pad nets. Always call it first with
   `dry_run: true`; apply only with `dry_run: false` and the exact returned
   `expected_plan_revision`. The requested board must be open in live KiCad, one
   apply is one undo entry, and unsupported or stale content returns a non-mutating
   conflict instead of silently dropping it.
5. **Place by block and by pins** — placement order: mechanical and
   connectors → large and hot parts → critical circuits → decoupling,
   terminations, feedback, protection → the rest. Read `get_component_pads`
   for every part before choosing its position and rotation, and turn it so
   its pads face their destinations. Assign interchangeable outputs in the
   geometric order of their loads and do the pin-swap pass
   (`references/routing-playbook.md` §2). Apply each batch with
   `set_component_placements` and `save_project` immediately — unsaved IPC
   placement is lost if the editor closes. Score with `score_placement`, render
   with `get_board_2d_view`, and close `references/placement-gate.md`,
   including its silkscreen and assembly-data section. On
   `FAIL`, move parts; never route around a placement mistake.
6. **Return-path plan and netclasses** — write where each critical net's
   current returns; derive widths and vias from the sizing record and encode
   them (`create_netclass`, `assign_net_to_class`, `set_predefined_sizes`),
   read back (`get_netclasses`, `get_predefined_sizes`); write the routing
   order by criticality.
7. **Route by criticality** — supply loops and decoupling → clocks, RF,
   pairs, controlled impedance → sensitive analog → main power → the rest →
   tuning only where timing requires it. Follow the pad selection and path
   rules in `references/routing-gate.md` and the techniques in
   `references/routing-playbook.md`. Iterate on a scratch copy and transfer
   verified copper to the live board in the playbook's order.
8. **Copper pour** — add ground/power zones last, then `refill_zones`. Stitch
   the layers, give every IC ground its own vias, and prove the ground with a
   connectivity graph and articulation points, not with a picture.
9. **Save, then DRC and rendered inspection** — `save_project` first: DRC
   and renders read the saved file. `run_drc` with zero unrouted items and
   zero errors, every warning adjudicated, and schematic parity actually
   checked; before trusting a clean result, rule out the traps in the
   `kicad-review` skill's `references/verification-traps.md` (disabled
   checks, missing parity, unenforced netclass widths, tangential via joints).
   `get_board_2d_view` inspected against the routing gate's rendered list;
   `query_traces` per critical net against the return-path plan. Close
   `references/routing-gate.md`.
10. **Layout review** — for a board that will be fabricated, run the
    `kicad-review` skill's layout-quality branch or delegate to
    `kicad-design-review-agent`.

Do NOT add copper pours before routing is complete — they interfere with interactive routing.

---

## Placement

### Strategy

- Group components by functional block (power, digital, analog, connectors)
  and order the blocks along the circuit's flow: connector → protection →
  filter → conditioning → converter → processor
- Place ICs first, then their associated passives
- Decoupling caps: within 2mm of their IC power pins, on same layer, with a
  short loop to the rail and the return — loop area matters more than
  visual proximity
- Cable/EMI filter caps: on the connector's own pins, and judged against that
  connector rather than the nearest IC
- Connectors: at board edges, accessible for cables; buttons, LEDs, displays,
  and test points where the enclosure lets a person reach them
- High-frequency components: minimize trace lengths between them
- Thermal considerations: power components away from sensitive analog;
  noise sources (inductors, switching nodes, drivers) away from references,
  crystals, and high-impedance inputs
- Place by pins, not by bodies: read `get_component_pads` before choosing a
  rotation and turn each part so its pads face the pads they connect to.
  Two adjacent parts still force a bad route when they are badly oriented
- Reserve routing corridors between blocks; an over-packed board needs
  detours and vias it did not need

### The anchor is not the centre

The placement coordinate is the footprint anchor, and for many footprints
the anchor is pad 1 (axial resistors, pin headers, connectors). A part placed
at x = 40 mm on a 50 mm board can have its other pad at 50.16 mm, outside
the outline. Compute every part's real extent from `get_component_pads`
after placing it, and compare every pad against the outline and the edge
clearance before moving on. `run_drc` on the saved board is the final proof.

### Rotation, origins, and saving

- Confirm which side pad 1 landed on from `get_component_pads` after every
  rotation; do not infer it from the angle. A wrong assumption crossed two
  supply nets without any DRC error.
- IPC rejects non-90° rotation for footprints that contain rounded-rectangle
  graphics; design arrays and glyphs on an orthogonal grid, with diagonals as
  staircases.
- For THT parts whose origin is pad 1 (5 mm LEDs, headers), measure spacing
  and alignment from pad midpoints, never from origins.
- `save_project` after every live batch and read the result back from the
  file.

### Footprints with repeated pad numbers

Tactile switches and some connectors carry two pads with the same number:
two copper islands that the part joins mechanically, not the board. Record
them from `get_component_pads` during placement. Routing must start from the
instance nearest the destination and bridge the pair with copper, or DRC
reports an unconnected item.

### Placement Tools

| Tool                      | Use Case                                    |
|---------------------------|---------------------------------------------|
| `place_component`         | Position one footprint via IPC or safe file fallback |
| `update_footprints_from_library` | Refresh placed definitions from linked libraries |
| `move_component`          | Relocate a footprint via IPC or safe file fallback |
| `rotate_component`        | Rotate a footprint via IPC or safe file fallback |
| `flip_component`          | Set F.Cu/B.Cu on a closed board with geometry mirroring |
| `align_components`        | Align multiple components (top/bottom/left/right/center) |
| `place_component_array`   | Grid placement for repeated elements        |

### Score-first automation

Load `load_toolset('placement')` for the automation loop. The discipline is
score, change, re-score — every planner reports the board's score before and
after its own plan, so a change is judged before it is made:

1. `score_placement` — 0-100 with named deductions; hard failures (courtyard
   overlaps, parts outside the outline) decide the verdict regardless of the
   number, and a board with no outline can never pass. The checks use
   bounding boxes, so confirm a hard failure against KiCad's DRC (real
   courtyard polygons) before moving parts: an L-shaped module courtyard
   produced false failures on a real board. `interface_filter_caps`
   lists caps that were within their family limit of a connector carrying every
   one of their nets: that is cable filtering, so the decoupling rule was
   answered rather than skipped. They are not defects to "fix" by dragging them
   toward an IC.
2. `auto_place_from_schematic` — deterministic first placement by net
   clusters; explicitly a starting point, not a final layout.
3. `refine_placement_force_directed` — deterministic spring embedder; pass
   `locked` for parts that must not move. Same input, same plan.
4. `place_decoupling_caps` — plans a row beside an IC, paired by shared nets.
5. `plan_bga_fanout` — pitch detected from the pad grid; `apply` executes as
   one KiCad undo commit over live IPC.

Every planner is dry-run by default; apply refuses while KiCad holds the
board open live (fanout apply is the inverse: it REQUIRES the live board).

### Placement Tips

- Use mm coordinates (KiCAD default for PCB)
- Standard grid: 0.5mm for placement, 0.25mm for fine adjustment
- Check component courtyard overlaps after placement
- Check every pad of every part against the outline after placement
- Look at the airwires before routing: many crossings mean badly oriented
  parts or badly distributed blocks — fix placement, not routing
- Render with `get_board_2d_view` and inspect the image; a successful render
  command is not placement acceptance
- Reference designator text: F.SilkS layer, 1mm height default

---

## Routing

Before choosing trace approach points, call `get_component_pads` for the
participating footprints. Use its returned board-space position, effective
rotation, shape, size, drill, and per-copper-layer geometry; do not estimate
copper extent from package family or a different pad in the footprint. A null
geometry field is unavailable evidence, not a zero-size pad.

Route in the written criticality order, not in the order the nets happen to
appear. Path rules (full list in `references/routing-gate.md`):

- Start each trace at the pad instance nearest its destination; when a pad
  number appears at two positions, bridge the pair with copper.
- A trace never crosses the body or courtyard of the part it leaves, nor of
  any other part, unless the footprint was designed for it and the clearance
  rules allow it. A diagonal across a switch to reach the next part is a
  pad-choice or placement error.
- Width comes from the netclass; corners at 45° or curved; no acute angles.
- Keep the return path: a shorter trace that leaves its reference plane or
  crosses a plane slot is worse than a longer one that keeps it.
- Fix a wrong segment with `delete_trace` before laying its replacement.

### Routing Tools

| Tool                      | Use Case                                    |
|---------------------------|---------------------------------------------|
| `route_pad_to_pad`        | Direct connection, auto L-bend routing      |
| `route_trace`             | Manual segment-by-segment routing           |
| `route_differential_pair` | Matched-length USB/LVDS/Ethernet pairs      |
| `add_via`                 | Layer transition                            |
| `create_netclass`         | Define width/clearance rules for net groups |

### route_pad_to_pad

The primary routing tool. Looks up both pad positions on the board and lays an
L-shaped trace between them.

```
route_pad_to_pad(board, net_name, ref1, pad1, ref2, pad2, layer?, width?)
```

- Emits one segment when the pads already share an X or Y, two otherwise
- Specify the width in mm from the accepted project netclass or sizing record.
- Routes entirely on `layer` (default `F.Cu`) — it does not add a via. To
  change layer mid-route, place the via yourself with `add_via` and route each
  side separately
- When it cannot resolve a reference that `get_component_list` does show,
  route with `route_trace` between the exact pad coordinates read from
  `get_component_pads`; never estimate a coordinate

### route_trace

One straight segment between two explicit points, for when you want to control
the path yourself.

```
route_trace(board, net_name, layer, x1, y1, x2, y2, width?)
```

- Use when auto-routing creates suboptimal paths
- There is no waypoint list: call it once per segment to build a polyline
- Coordinates are board-space mm

### route_differential_pair

For differential signals (USB, HDMI, Ethernet, LVDS).

```
route_differential_pair(board, net_pos, net_neg, x1, y1, x2, y2, gap?, layer?, width?)
```

- Lays two straight traces parallel to the given line, offset `(gap + width)/2`
  either side, so spacing is constant along the segment
- Not a length-matching router: it adds no serpentine tuning, and equal length
  only follows from the two traces being parallel segments. Skew introduced
  before or after this call is yours to correct
- Common pairs: USB_D+/USB_D-, LVDS_P/LVDS_N

### Netclasses

Define routing rules for groups of nets:

```
create_netclass(board, name, trace_width?, clearance?, via_drill?, via_diameter?)
```

The class is written to the project's `.kicad_pro` file, which is where KiCad
has kept netclasses since v7 — the board file is not modified. While KiCad
holds the board open, its next save rewrites that file: classes and
assignments written by a tool reverted to the default class twice on the
reference project. Create classes with the board closed or in Board Setup,
serialise the writes, and read them back with `get_netclasses` after KiCad's
next save. Give every power and switch-node net an explicit schematic name;
auto-generated names such as `Net-(J2-Pin_1)` break when references change.
A netclass width is the routing default, not a DRC minimum — see
`references/trace-width-table.md` for enforcing it.

Before creating or updating a class, read `get_netclasses` and the applicable
design-rule/trace-sizing references. Derive width, clearance, gap, drill, and
diameter from the selected fabrication contract, stackup, and electrical
calculation. Read the classes back after the write and confirm every special net
resolves through the intended class. Missing inputs make the rule `INCOMPLETE`.

### Pre-defined sizes

Netclass width is the default. The Track/Via dropdowns are a separate palette
in the sibling `.kicad_pro`. Fill them with `set_predefined_sizes` so `W` /
`Shift+W` can step through extra widths without changing netclasses:

The values below show call syntax only; they are not engineering recommendations.
Replace every value with one from the accepted project sizing record, derived
from the current fabrication contract, stackup, and electrical requirements. If
that evidence is unavailable, report the sizing task as `INCOMPLETE` instead of
reusing these illustrative values.

```
set_predefined_sizes(board, track_widths=[0.2, 0.5, 0.8],
    via_dimensions=[{diameter:0.6, drill:0.3}, {diameter:0.8, drill:0.4}])
```

A leading 0 mm / 0,0 via is always kept as “use netclass values”. These sizes
are not DRC limits. KiCad reads the list on next project open.

---

## Copper Pour

Zone tools live in the `pcb_board` toolset.

### add_zone

Creates a copper pour area (polygon fill).

```
add_zone(board, net_name, layer, points, clearance?, min_width?,
         name?, priority?, pad_connection?)
```

- Almost always GND net on both F.Cu and B.Cu
- `points` is the outline polygon; define it slightly inside the board edge
  (0.5mm inset)
- `priority` defaults to 0; the higher priority wins where two pours overlap
- `pad_connection` is `solid` | `thermal` | `none`, defaulting to `thermal`
  as KiCad does
- With KiCad running on this board the zone is created over IPC and refilled
  for you, so it appears at once and is in KiCad's undo stack. Without a live
  KiCad it goes into the file instead, and the result says so (`source: file`)
  and carries a `warning` describing the process-local evidence and cold-start
  limitation. A board observed live earlier in this server session fails with
  `unsafe_file_fallback` instead of writing the file.

### refill_zones

**Must call `refill_zones` after any change that affects copper pour:**
- After adding/moving components
- After routing new traces
- After modifying zone outlines
- After changing design rules

Zones do not auto-update — stale fills cause DRC errors.

### Zone Tips

- GND pour on both layers is standard practice
- Leave spoke thermal reliefs for through-hole pads (easier soldering)
- Use keepout zones to prevent copper in sensitive areas
- Zone clearance typically 0.3-0.5mm from traces
- Set a minimum fill width (0.25 mm on the reference board) so slivers do not
  form, remove islands, and give every remaining island at least two
  connections to the plane
- Stitch the two layers on a grid and at every IC ground pin; the playbook
  lists the site-acceptance rules
- The zone's clearance to NPTH holes comes from Board Setup's hole clearance,
  not from the zone; a local clearance on the NPTH pad fixes a violation there
- Konnect cannot delete a zone or set thermal spoke width and gap; a wrong
  zone needs the KiCad GUI. Never create a test zone on the live board to
  discover a tool's parameters

---

## Layer Reference

| Layer    | Name     | Purpose                              |
|----------|----------|--------------------------------------|
| F.Cu     | Front Copper   | Top copper traces and pads     |
| B.Cu     | Back Copper    | Bottom copper traces and pads  |
| F.SilkS  | Front Silk     | Top silkscreen (text, outlines)|
| B.SilkS  | Back Silk      | Bottom silkscreen              |
| F.Mask   | Front Mask     | Top solder mask openings       |
| B.Mask   | Back Mask      | Bottom solder mask openings    |
| Edge.Cuts| Board Outline  | Physical board boundary        |
| F.Fab    | Front Fab      | Top fabrication drawing        |
| B.Fab    | Back Fab       | Bottom fabrication drawing     |
| F.CrtYd  | Front Courtyard| Top component clearance area   |
| B.CrtYd  | Back Courtyard | Bottom component clearance area|
| In1.Cu   | Inner 1        | Internal copper layer 1        |
| In2.Cu   | Inner 2        | Internal copper layer 2        |

### Layer Usage Guidelines

- Route signals on F.Cu and B.Cu (2-layer) or add inner layers for complex boards
- Board outline MUST be on Edge.Cuts (closed polygon or rectangle)
- Silkscreen for reference designators and polarity marks
- Courtyard defines minimum spacing between components
- Use F.Fab/B.Fab for assembly drawings and component outlines

---

## Design Rule Check

After completing layout, save first — `run_drc` reads the saved board file,
so an unsaved board is judged on stale data (phantom "missing footprint"
items are the usual symptom):

```
save_project()
run_drc()
```

A passing DRC means the layout obeyed the rules it was given, not that the
circuit works. Placement quality, return paths, and rendered inspection are
separate acceptance evidence (`references/routing-gate.md`).

Common DRC errors and fixes:
- **Clearance violation**: move trace or component further apart
- **Unconnected net**: route missing connection
- **Track too close to edge**: move inward from board outline
- **Courtyard overlap**: increase spacing between components
- **Zone fill error**: run `refill_zones`

### Read `owner` before deciding on a board-edge violation

Every violation item carries `owner` and `ownership_status`. Read them before
choosing a fix — `"Circle of J1 on Edge.Cuts"` reads identically whether that
geometry is the board outline or a cutout the footprint carries itself.

- `owner.kind: "board"` — the item is the board's own geometry. Move the
  offending copper inward, or change the outline.
- `owner.kind: "footprint"` — the geometry belongs to that footprint
  (`owner.reference` names it), typically a connector's locking-peg cutout. It
  is still real fabrication geometry and the violation is still real, but the
  pad and the cutout move together, so **repositioning the component cannot fix
  it**. Review the footprint definition or the rule instead.
- `ownership_status` other than `"resolved"` (`"uuid_missing"`,
  `"not_found"`) — ownership is unknown, and `owner` is `null`. Do not assume
  the board owns it; check with `list_board_footprint_graphics` before advising
  a move.

---

## Rules

1. **Never edit .kicad_pcb directly** — all changes go through MCP tools
2. **Always verify placement after moves** — components may snap to unexpected positions
3. **Board outline first** — define the physical boundary before placing anything
4. **Refill zones after changes** — stale zone fills cause phantom DRC errors
5. **Check DRC before finishing** — run `run_drc()` and resolve all errors
6. **Use netclasses for consistency** — define track widths per net type, not per trace
7. **KiCAD normally must be running** — use guarded closed-board paths only when a
   tool explicitly offers them. Treat `unsafe_file_fallback` as a human recovery
   boundary; other PCB edits still require the live IPC connection, except a
   change no tool can make, which goes through the konnect skill's scripted
   board fallback (KiCad closed, a dated copy first, DRC with parity after)
8. **Save frequently** — call `save_project` after major operations
9. **Load toolsets first** — check `get_active_toolsets()` and load what you need
10. **Copper pour last** — add zones only after routing is substantially complete
11. **Constraints and blocks before parts** — write the constraint record and
    the block map before the first placement; ask for missing load-bearing values
12. **Pins face destinations** — read `get_component_pads` before rotating a
    part; check every pad against the outline; close the placement gate before
    routing
13. **Return path before signal** — write where each critical net's current
    returns before routing it; never shorten a trace at the cost of its return
14. **Save before evidence, inspect the render** — DRC and renders read the
    saved file; "DRC passed" alone is not layout acceptance
15. **Scratch first, then transfer** — iterate on a disposable copy outside
    the project where KiCad's Python is available (the konnect One Rule sets
    the limits), or in small undoable IPC batches where only Konnect tools
    are; the live board receives verified placement and copper in a fixed
    order, saved after every batch, through Konnect tools, the user's
    Specctra import, or the scripted board fallback
16. **Never move a routed footprint by shifting track ends** — move it, rip up
    and re-route its connections, with a restorable backup first (a dated copy
    of the saved board, or KiCad's undo for one IPC batch; `snapshot_project`
    writes PDFs only)
17. **Prove the checks could fail** — rule configuration read back after
    KiCad's last save, parity actually requested, netclass widths audited;
    every number in a report carries the command that produced it
18. **Autorouted copper is a draft** — accepted per net on DRC, length, via
    count, and a rendered image, never on "0 unrouted"
