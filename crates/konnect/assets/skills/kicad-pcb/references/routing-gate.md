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
| The routing order is written by criticality | Supply loops and decoupling → clocks, RF, pairs, controlled impedance → sensitive analog → main power → the rest → tuning |
| Same-number pads are listed | From the placement gate; each pair needs a copper bridge |

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

## D. After routing

| Check | Evidence |
|---|---|
| The board is saved before any check | `save_project`; `run_drc` and `get_drc_violations` read the saved file, so phantom "missing footprint" items on an unsaved board are stale data, not findings |
| Zero unrouted items, zero DRC errors | `run_drc` summary on the saved board; every warning adjudicated (footprint-library and field mismatches are metadata unless the geometry differs) |
| Every same-number pad pair is bridged | `run_drc` shows no unconnected item for those pads; `query_traces` on that net lists the bridge |
| Every critical net follows its return-path plan | `query_traces` per net compared with the plan; rendered view of the reference plane |
| Trace widths match the netclass record | `query_traces` `width` values against `get_netclasses` |
| Zones are filled and free of islands | `refill_zones`, then rendered view; DRC isolated-copper items reviewed |
| The rendered board was actually inspected | `get_board_2d_view` output looked at against the list below |

## What a rendered routed board must show

- No trace crossing a part body or courtyard.
- No trace leaving a pad on the side away from its destination.
- Short, direct paths between adjacent stages; corners at 45° or curved.
- Power traces visibly wider than signal traces where the record says so.
- The ground plane continuous under the signals that need it, no slots
  under fast or sensitive traces.
- Every same-number pad pair joined by copper.
- Nothing within the edge clearance of the outline.

If any item is not visible, fix it before reporting. "DRC passed" alone is
not routing acceptance.
