# Layout Quality Review

ERC and DRC prove that a board obeys the rules it was given. They do not
prove that the placement follows the circuit, that the return currents have
a path, or that the board can be assembled and tested. This branch collects
that evidence. It runs after the formal checks, on the **saved** board, and
its findings are classified with the same severity scale as the rest of the
review.

## Toolsets and evidence

```
load_toolset('pcb_components')   # get_component_pads, get_component_list, get_board_2d_view
load_toolset('pcb_routing')      # query_traces, get_nets_list, get_netclasses
load_toolset('pcb_board')        # get_board_extents, get_layer_list
load_toolset('verification')     # run_drc, get_design_rules, check_clearance
load_toolset('placement')        # score_placement
```

Save first (`save_project`): DRC and the renders read the saved file.

## Review table

| Review | What to check | Evidence |
|---|---|---|
| Electrical | Every pad inside the outline; nets complete; polarities and pin 1 match the schematic | `get_component_pads` per part against `get_board_extents`; `run_drc` unrouted count; rendered view |
| Signal integrity | Continuous reference under fast or sensitive traces; no plane slots crossed; layer changes with a return via; terminations where the interface needs them | `query_traces` per critical net; rendered view of the plane; requirements record |
| Power integrity | Width and via count against the current record; decoupling loops short; no bottlenecks at pads or reliefs | `query_traces` widths against `get_netclasses`; `get_component_pads` distances; sizing record |
| Thermal | Hot parts have copper and vias; sensitive parts are not beside them | Rendered view; constraint record; datasheet |
| Fabrication | Trace, clearance, drill, ring, mask, and edge limits met | `run_drc` against `get_design_rules` that encode the fabricator contract |
| Assembly | Courtyards clear; references readable; polarity marks present; rework access | `run_drc` courtyard and silk items; rendered view |
| Test | Supply, GND, programming, and key signals reachable without stubs on fast lines | Rendered view; net list |
| Mechanical | Holes, connectors, and heights match the enclosure | Constraint record; `get_board_extents`; `check_clearance` |
| Placement quality | Blocks grouped along the flow; pins face destinations; airwires do not cross bodies | `score_placement`; rendered view; `get_component_pads` |
| Ground robustness | No articulation point at an IC ground; every island connected at least twice; stitching gaps bounded | Graph of GND copper (tracks, vias, pads, filled islands); Tarjan articulation points |
| Decoupling effectiveness | Copper path cap → pin and via count, not straight-line distance; severity from the current that pin really carries | Shortest path over the net's tracks; `query_traces` |
| Return detours | Path through ground copper from a switching regulator to sensitive grounds compared with the straight line | Breadth-first search through both layers and vias |
| Joints | Track ends centred on vias; no via in an SMD pad opening | DRC warnings reviewed one by one; pad-opening geometry |
| Silkscreen meaning | Each legend nearest its own part or pin; rotated text rendered and read | Word positions against pads; rendered image |
| Edge and depaneling | Pad-to-edge distances where the board is V-cut or broken off after assembly | `get_component_pads` against `get_board_extents` |
| Hot pads | Copper connected to each part dissipating more than about 0.3 W, against the area the datasheet's θJA assumes | Copper area by radius; datasheet |

## Layout findings and severity

| Finding | Severity | Why |
|---|---|---|
| A pad or copper item outside the outline or inside the edge clearance | CRITICAL | The fabricator cuts it or the ring breaks |
| Unrouted item, including an unbridged same-number pad pair | CRITICAL | The board is incomplete; the switch or connector node is open |
| A trace narrower than the current record requires | CRITICAL | Heating or voltage drop in service |
| A fast or sensitive trace over a slot in its reference plane | WARNING, CRITICAL when the interface has a stated integrity requirement | Return discontinuity, emissions, crosstalk |
| No continuous ground reference on a board with switching or fast signals | WARNING | Return paths undefined |
| A decoupling capacitor far from its pin or with a long loop | WARNING | Rail noise; the schematic promise is not kept on copper |
| A trace crossing a part body or courtyard, or leaving a pad on the far side of the part | WARNING | Assembly and rework risk, and a sign the placement was not pin-aware |
| Connector not at an edge, or a user control that cannot be reached in the enclosure | WARNING | Product cannot be assembled or used as intended |
| Noise source placed against a sensitive input, reference, or crystal | WARNING | Coupling the datasheet warns about |
| Acute trace angles, needless direction changes, needless vias | SUGGESTION | Fabrication yield and readability |
| No test points on supply, GND, or debug signals | SUGGESTION | Harder bring-up |
| Reference designators unreadable or under parts | SUGGESTION | Assembly and debug |
| DRC or render collected from an unsaved board | INCOMPLETE | The evidence describes a stale file |
| A single-path track-to-via joint where the track copper does not reach the barrel (measured) | CRITICAL | Connectivity passes; the joint opens in production |
| An IC ground pad whose only path to the plane is one pad, via, or neck (articulation point) | WARNING, CRITICAL when that path is another part's pad or a single via under an ESD or interface device | One defect disconnects the IC's ground |
| A net narrower than its netclass without a recorded capacity check | CRITICAL when it carries supply current; WARNING otherwise | DRC does not enforce netclass widths |
| A via inside an SMD pad opening | WARNING | Solder wicks down the bare hole and starves the joint |
| A silkscreen legend nearer another pin or part than its own | WARNING | Field wiring errors |
| A clean result from a disabled or never-requested check | INCOMPLETE | The check could not fail (`verification-traps.md`) |

A visual finding is heuristic until a pad position, trace list, or DRC item
corroborates it; report the corroboration, not only the picture.

Some of these measurements need geometry that Konnect does not expose yet
(articulation points, copper path length, joint contact, word positions).
When the tools available cannot collect one, mark that check blocked and
name the method, rather than reporting it as passed.

## Report additions

Add to the review report:

```
### Layout quality
- Placement: [blocks, flow, pin orientation — evidence]
- Return paths: [per critical net — evidence]
- Widths and vias: [against the record — evidence]
- Assembly/test/mechanical: [evidence]
- Rendered inspection: [what was seen]
```
