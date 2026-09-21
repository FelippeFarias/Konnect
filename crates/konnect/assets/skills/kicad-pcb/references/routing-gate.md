# Routing Gate

Routing turns an accepted placement into copper without undoing what the
placement achieved. Read [`layout-methodology.md`](layout-methodology.md)
sections 3 to 6 for the reasoning; this file is the checklist.

Verdict: `PASS`, `FAIL` (reroute, or go back to placement), or `INCOMPLETE`
(say which evidence is missing).

## A. Before the first trace

| Check | Evidence |
|---|---|
| Netclasses carry the accepted widths, clearances, and via sizes | `get_netclasses` readback after `create_netclass` / `assign_net_to_class`; sizing per `trace-width-table.md` |
| The routing palette matches the accepted sizes | `get_predefined_sizes` after `set_predefined_sizes` |
| A return-path plan exists for every critical net | Written: which plane or trace carries the return, where it crosses layers, where stitching vias go |
| A layer plan exists | Written: the job and dominant direction of each layer, which layer is the reference plane, where long buses run, which edge-sensitive lines must not be neighbours (`routing-playbook.md` §1) |
| The routing order is written by criticality | Supply loops and decoupling → clocks, RF, pairs, controlled impedance → sensitive analog → main power → the rest → tuning |
| Same-number pads are listed | From the placement gate; each pair needs a copper bridge |
| Repeated blocks have one solved topology | One instance routed and verified before replication (`routing-playbook.md` §4) |
| The design rules can fail | The rule configuration was read back from the saved project file after KiCad's last save: no zero-valued constraint, no unjustified ignored severity, custom rules present (`kicad-review` skill, `references/verification-traps.md`) |

## B. Pad selection and path rules

1. **Route from the pad instance nearest the destination.** A footprint whose
   pad number appears at two positions (tactile switch, doubled connector
   pin) has two copper islands. Read both positions from `get_component_pads`
   and start the trace at the one facing the destination.
2. **Bridge same-number pads with copper.** The switch body joins them
   mechanically, not the board. Without a trace between them DRC reports an
   unconnected item and the ratsnest stays open.
3. **A trace never crosses the body or the courtyard of the part it leaves,
   nor of any other part**, unless the footprint was designed for traces to
   pass under it and the clearance rules allow it. A diagonal from the far
   pad across the switch to reach the resistor is a placement or pad-choice
   error, not a routing style.
4. **Do not pass between two pads of another footprint** unless the clearance
   rule proves it fits and the path is intended.
5. **Use the width from the netclass.** Passing an ad-hoc width to
   `route_trace` or `route_pad_to_pad` is acceptable only when the netclass
   record names that width for that net.
6. **Corners at 45° or curved; no acute angles** (acid traps) and no needless
   direction changes. A 90° corner is a consistency issue, not an electrical
   fault by itself.
7. **Vias sized from the palette**, and as many as the current needs; place
   stitching vias where a signal changes layer over a ground reference.
8. **Keep the return path.** A shorter trace that leaves its reference plane
   or crosses a plane slot is worse than a longer one that keeps it.
9. **Sensitive signals keep their distance** from switching nodes and power
   traces; no long parallel runs between aggressor and victim.
10. **Power stage loops stay small** (input capacitor, switch, diode/inductor,
    output capacitor), and the feedback trace stays away from the switching
    node.
11. **End every track at the via centre.** A track ending on the edge of a
    via ring is a tangential joint: connectivity passes, the joint fails.
12. **No strap, boot, reset, or enable net under a metal connector shell** on
    the component side.
13. **Power nets keep their class width.** A neck below the class needs a
    recorded current-capacity check; otherwise reroute on the other layer or
    around the part.
14. **Two vias or more, for redundancy,** wherever a supply of more than
    about 0.5 A changes layer; a single via is a single point of failure for
    the rail even when it can carry the current.
15. **Clock and latch lines are not neighbours** over long runs; interleave a
    static net or leave room for guard copper.
16. **Prefer a layer change to a detour** when two traces must cross in a
    dense two-layer region, as long as rule 8 holds for fast or sensitive
    nets; when detours multiply in one region, go back to placement or pin
    assignment.

## C. Tool practice

- `route_pad_to_pad` lays an L-shaped path between two named pads on one
  layer. When it cannot resolve a reference that `get_component_list` does
  show, route with `route_trace` between the exact pad coordinates read from
  `get_component_pads`. Never estimate a coordinate.
- `route_trace` is one straight segment; build a polyline with several calls
  and keep every intermediate point on the same net.
- `add_via` places the layer transition; route each side separately.
- `route_differential_pair` keeps a constant gap along one segment; it does
  not tune length.
- `query_traces` lists what exists on a net; `delete_trace` removes a wrong
  segment by its uuid before replacing it. Do not leave a wrong trace and add
  a second one.
- Copper pours come **after** routing: `add_zone`, then `refill_zones` after
  every later change.
- Konnect cannot delete a zone or set thermal spoke parameters. Never probe a
  tool by creating test objects on the live board: a three-point probe zone
  could not be removed through the tools, and a GUI undo left a duplicate
  pour behind. Read the tool's schema first.
- An autorouter result is a draft (`routing-playbook.md` §6): lock accepted
  copper, keep GND out of it, turn fan-out off, and accept each net only on
  DRC, length, via count, and a rendered image.
- In a dense corner, work from DRC's unconnected list and a fast clearance
  check, block by block (`routing-playbook.md` §5).

## D. After routing

| Check | Evidence |
|---|---|
| The board is saved before any check | `save_project`; `run_drc` and `get_drc_violations` read the saved file, so phantom "missing footprint" items on an unsaved board are stale data, not findings |
| Zero unrouted items, zero DRC errors | `run_drc` summary on the saved board; every warning adjudicated (footprint-library and field mismatches are metadata unless the geometry differs) |
| Every same-number pad pair is bridged | `run_drc` shows no unconnected item for those pads; `query_traces` on that net lists the bridge |
| Every critical net follows its return-path plan | `query_traces` per net compared with the plan; rendered view of the reference plane |
| Trace widths match the netclass record | `query_traces` `width` values against `get_netclasses` |
| Zones are filled and free of islands | `refill_zones`, then rendered view; DRC isolated-copper items reviewed |
| Schematic parity was actually checked | `run_drc` reports parity (it is `null` when KiCad could not load the schematic); with kicad-cli, `--schematic-parity --severity-all`. Without the flag the parity list is empty and reads as zero |
| Every net meets its netclass width | Per-net minimum width against `get_netclasses` from `query_traces`; DRC enforces only the board-wide minimum unless custom rules exist |
| The ground is robust, not only connected | Every IC ground pad has its own path to the plane (two or more for ESD arrays, transceivers, bus buffers); no pad, via, or neck is an articulation point; every filled island has at least two connections |
| Track-to-via joints are centred | Track-not-centred-on-via items reviewed one by one, never waived as cosmetic |
| Vias are what they should be after any import | Diameter and drill of every via after a Specctra session import (the importer applies the netclass drill); no via inside an SMD pad opening |
| Each block closed its own connections | Unconnected items filtered by net for every routed block, not once at the end; deferred nets (GND to the pours) named |
| The rendered board was actually inspected | `get_board_2d_view` output looked at against the list below; each copper layer also rendered separately so a full pour does not hide the traces |

## What a rendered routed board must show

- No trace crossing a part body or courtyard.
- No trace leaving a pad on the side away from its destination.
- Short, direct paths between adjacent stages; corners at 45° or curved.
- Power traces visibly wider than signal traces where the record says so.
- The ground plane continuous under the signals that need it, no slots
  under fast or sensitive traces.
- Every same-number pad pair joined by copper.
- Nothing within the edge clearance of the outline.
- Buses as parallel lanes in pin order; repeated blocks with identical
  copper; no diagonal crossing a block.
- No long run of the plane layer that splits the pour under clock lines.

If any item is not visible, fix it before reporting. "DRC passed" alone is
not routing acceptance.
