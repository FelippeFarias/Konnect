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
| No courtyard overlaps | `run_drc` courtyard items; `score_placement` hard failures |
| Tall or hot parts respect enclosure height and thermal keep-outs | Constraint record; `check_clearance` between the part and its neighbours where a datasheet gives a distance |

## C. Placement follows the circuit, not the grid

| Check | Evidence |
|---|---|
| Functional blocks are grouped and ordered along the signal/power flow | Rendered view (`get_board_2d_view` or `export_svg`), explained block by block |
| Connectors sit at the board edge on the side the enclosure and cables need | Rendered view; constraint record |
| User-facing parts (buttons, LEDs, displays, test points) are reachable | Rendered view; constraint record |
| Noise sources (inductors, switching nodes, drivers, relays) are away from sensitive inputs, references, and crystals | Rendered view with the net names from `get_component_pads` |
| Decoupling capacitors sit at the pins they serve with a short loop to the rail and the return | `get_component_pads` distances; `score_placement` (note: `interface_filter_caps` names caps that filter a connector, which is correct placement) |
| Heat sources do not sit against temperature-sensitive parts | Rendered view; constraint record |

## D. Pins face their destinations

| Check | Evidence |
|---|---|
| Each part is rotated so its pads point at the pads they connect to | `get_component_pads` for both ends of every airwire: the connecting pads are on facing sides, not across the body |
| Airwire crossings are minimal | `score_placement` before and after each change; a rendered view with the ratsnest when available |
| Footprints with several pads sharing one pad number (tactile switches, redundant connector pins) are identified | `get_component_pads`: two entries with the same `number`; record them for the routing gate, which must bridge them |
| Routing corridors exist between blocks for the traces that will need them | Rendered view; no block is packed against another without a corridor |

## E. Record and save

| Check | Evidence |
|---|---|
| Placement is saved | `save_project`; a DRC run reads the saved file, so an unsaved board is judged on stale data |
| The rendered board was actually inspected | `get_board_2d_view` output looked at; report what was seen, not that the command succeeded |
| Placement rationale is written | One line per part or block: why it is where it is, and which airwire it serves |

## What a rendered placement must show

- Every part fully inside the outline, with visible margin to the edge.
- Blocks recognisable at a glance; the current path readable left to right or
  top to bottom.
- Connectors and controls on the edges the product needs.
- No airwire that has to cross a part body to reach its pad.
- No two parts so close that the silkscreen or the courtyard collides.

If any of these is not visible, fix placement now. Routing cannot repair it.
