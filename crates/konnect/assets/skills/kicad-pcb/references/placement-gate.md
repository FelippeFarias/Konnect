# Placement Gate

Routing does not start until every check below has evidence. Read
[`layout-methodology.md`](layout-methodology.md) for the reasoning; this file
is the checklist and the tool that answers each item.

Verdict: `PASS` (all items answered, no failure), `FAIL` (go back to
placement — do not route around a placement mistake), or `INCOMPLETE` (an
item could not be evaluated; say which).

## A. Inputs are on the board

| Check | Evidence |
|---|---|
| The board carries the saved schematic's footprints and pad nets | `update_pcb_from_schematic` dry run reviewed, then applied with its exact `expected_plan_revision`; `get_component_list` shows every reference |
| The constraint record exists (size, connectors, currents, voltages, edges, sensitivity, layers, fabricator, assembly) | Written in the conversation or the handoff; unknown load-bearing rows were asked, not assumed |
| Outline, mounting holes, and mechanical keep-outs are in place first | `add_board_outline` / `set_board_size`, `add_mounting_hole`; `get_board_extents` matches the requested size |
| Project rules encode the fabricator's limits | `get_design_rules` readback; `set_design_rules` / `set_layer_constraints` when missing |

## B. Every part is physically inside the board

The placement coordinate is the footprint **anchor**, and the anchor is often
pad 1 rather than the body centre (axial resistors, pin headers, many
connectors). A part placed "at x = 40 mm" on a 50 mm board can have its
second pad at 50.16 mm — outside the outline.

| Check | Evidence |
|---|---|
| All pads of every footprint lie inside the outline with the edge clearance from the rules | `get_component_pads` for each reference: compare every pad `x`/`y` (plus half its size) against the outline coordinates; then `run_drc` on the saved board shows no copper-to-edge items |
| No courtyard overlaps | `run_drc` courtyard items (real polygons). `score_placement` hard failures decide its own verdict but are computed on bounding boxes — an L-shaped module courtyard (with its antenna area) produced false off-board and overlap failures — so confirm each one with DRC before moving parts |
| Tall or hot parts respect enclosure height and thermal keep-outs | Constraint record; `check_clearance` between the part and its neighbours where a datasheet gives a distance |
| Solder joints are far enough from V-cut or break-off edges when the board is depaneled after assembly | Pad-to-edge distance from `get_component_pads`, not courtyard-to-edge (a capacitor "0.855 mm" from the edge by courtyard had its joints 4.15 mm away); ceramic capacitors near a break line have their axis parallel to it |
| Edge connectors sit where the mating plug seats | Connector face against `Edge.Cuts`, measured from the footprint's PCB-edge marker: flush or slightly proud. A USB-C face 0.55 mm inside the edge lets the plug overmold hit the board before the latch engages |
| Mechanical support is planned | Unsupported spans between mounting holes bounded on long boards, and a hole near every screw-terminal block, where screwdriver torque lands (a 241 mm span and a terminal 46 mm from any hole were review findings) |

## C. Placement follows the circuit, not the grid

| Check | Evidence |
|---|---|
| Functional blocks are grouped and ordered along the signal/power flow | Rendered view (`get_board_2d_view` or `export_svg`), explained block by block |
| Connectors sit at the board edge on the side the enclosure and cables need | Rendered view; constraint record |
| User-facing parts (buttons, LEDs, displays, test points) are reachable | Rendered view; constraint record |
| Noise sources (inductors, switching nodes, drivers, relays) are away from sensitive inputs, references, and crystals | Rendered view with the net names from `get_component_pads` |
| Decoupling capacitors sit at the pins they serve with a short loop to the rail and the return | `get_component_pads` distances; `score_placement` (note: `interface_filter_caps` names caps that filter a connector, which is correct placement) |
| Heat sources do not sit against temperature-sensitive parts | Rendered view; constraint record; a PTC fuse kept away from a hot series diode |
| Blocks follow function and use, not wire length | Service controls (reset, boot) beside the programming connector and apart from operator controls; indicators grouped in one labelled block; repeated passives in aligned banks |
| Room is reserved for datasheet-mandated local parts | The ESD array's local 100 nF on VBUS, a TVS within a few millimetres of its connector, a regulator's input capacitor at its pins — placed before routing fills the corner |
| Antenna keepouts are real rule areas | Copper-free on every layer under a module antenna, as a keepout rule area rather than a gap left in the pour; a board notch or overhang under the antenna where the panel plan allows it |

## D. Pins face their destinations

| Check | Evidence |
|---|---|
| Each part is rotated so its pads point at the pads they connect to | `get_component_pads` for both ends of every airwire: the connecting pads are on facing sides, not across the body |
| Airwire crossings are minimal | `score_placement` before and after each change; a rendered view with the ratsnest when available |
| Footprints with several pads sharing one pad number (tactile switches, redundant connector pins) are identified | `get_component_pads`: two entries with the same `number`; record them for the routing gate, which must bridge them |
| Routing corridors exist between blocks for the traces that will need them | Rendered view; no block is packed against another without a corridor |
| Interchangeable outputs follow the geometry of their loads | Driver pin order along each pad row equals the angular order of its loads; the pin-swap pass (`routing-playbook.md` §2) is done and the firmware map written; airwire crossings counted per block |
| Rotation is confirmed from pad coordinates | After placing, read `get_component_pads` and confirm which side pad 1 landed on; assuming it from the rotation value crossed two supply nets without any DRC error. IPC rejects non-90° rotation of footprints that contain rounded-rectangle graphics |
| Geometry is measured on bodies, not origins | For THT parts whose origin is pad 1, measure alignment and spacing from pad midpoints or the fabrication outline |

## D2. Silkscreen and assembly data are part of placement

A placement is not ready for review while its own DRC shows silkscreen
violations. On the reference board it was offered with 80 silkscreen
overlaps and 121 silkscreen-over-copper items and was rejected at once.

| Check | Evidence |
|---|---|
| No silkscreen overlaps and no silkscreen over exposed copper | `run_drc` silkscreen items on the saved board, with silkscreen clearance enabled (a 0 in Board Setup disables it) |
| References legible and reading the same way | No reference collides with another reference or a pad with keep-upright applied; bank pitch derived from label size (a 1206 bank with 1 mm references needed 3.8 mm) |
| Legends sit on the parts they name | Each legend is nearer its own part than any other, one legend per connector pin, "+"/"−" on the connector; a rotated label naming two stacked parts once put each word beside the wrong switch |
| Silkscreen meets the fabricator minimum | Text height and stroke, and graphic line width too (library footprints ship 0.12 mm lines) |
| Every footprint has its 3D model and fabrication attribute | THT or SMD set on every part except holes and fiducials, and a model that resolves. Footprints created by automation had lost both, which would have dropped 198 THT LEDs from the placement file; restore with `update_footprints_from_library` or KiCad's Update Footprints from Library with only the model and attribute options ticked. Either one replaces pads and graphics from the library and undoes deviations made only on the board, so move intentional deviations into a project library first |

## E. Record and save

| Check | Evidence |
|---|---|
| Placement is saved | `save_project` after every live batch, then read back from the file; unsaved IPC placement was lost twice when the editor closed. A DRC run reads the saved file, so an unsaved board is judged on stale data |
| The rendered board was actually inspected | `get_board_2d_view` output looked at; report what was seen, not that the command succeeded |
| Placement rationale is written | One line per part or block: why it is where it is, and which airwire it serves |
| The user's review gate is respected | When the user asked to approve placement, stop at "ready for review" and route nothing until approval; adopt any manual edit the user made (diff the live board against the placement source before each apply) |

## What a rendered placement must show

- Every part fully inside the outline, with visible margin to the edge.
- Blocks recognisable at a glance; the current path readable left to right or
  top to bottom.
- Connectors and controls on the edges the product needs.
- No airwire that has to cross a part body to reach its pad.
- No two parts so close that the silkscreen or the courtyard collides.

If any of these is not visible, fix placement now. Routing cannot repair it.
