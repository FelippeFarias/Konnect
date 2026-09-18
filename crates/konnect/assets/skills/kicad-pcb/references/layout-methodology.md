# Expert Layout Methodology

An experienced layout engineer does not start by pulling traces. The work
starts by **understanding the circuit, defining the constraints, and placing
the components along the current and signal paths**. A good placement solves
most routing problems before the first trace is drawn.

A board that looks tidy is not necessarily electrically good. The goal is to
reduce interference, losses, heating, and fabrication difficulty; a clean
appearance is usually a consequence of that, never the objective.

```
understand the circuit → define constraints → plan layers and return paths
→ place by functional block → validate the critical paths
→ route by priority → review → measure the prototype
```

Each phase below names the Konnect evidence that closes it. A phase whose
evidence was not collected is open, and the layout is `INCOMPLETE`.

---

## 1. Before placement: know the constraints

Record these before touching the board. Ask the user for whatever the
schematic and the request do not establish; do not invent a value.

| Information | Why it shapes the layout |
|---|---|
| Board dimensions, holes, enclosure, available height | Define the usable area and the mechanical limits. |
| Connector positions and access to controls | Fix where blocks must sit and how the product is assembled. |
| Continuous current, peaks, and transients per net | Drive trace width, via count, connectors, power distribution, and dissipation. |
| Operating voltages and possible surges | Drive isolation, spacing, and protection. |
| Frequencies and rise/fall times | Decide which connections must be treated as transmission lines. |
| Signal sensitivity | Sets the distance required from noise sources. |
| Layer count and stackup | Decide return references, impedance, and routing freedom. |
| Fabricator capability | Sets widths, spacings, drills, annular rings, and other limits. |
| Assembly and test process | Decides accessibility, test points, and part spacing. |

A digital signal of apparently low frequency still needs high-speed care
when its edges are fast.

Where the constraints live in Konnect:

- Board size, outline, holes: `add_board_outline`, `add_mounting_hole`,
  `get_board_extents`, `get_layer_list`.
- Fabricator limits and project rules: `get_design_rules`, `set_design_rules`,
  `set_layer_constraints`, `load_user_config` (config toolset).
- Net inventory and what each net connects: `export_netlist_summary`,
  `list_schematic_nets`, `get_net_components` (schematic side) and
  `get_nets_list` (board side).

Completion criterion: a written constraint record (in the conversation or the
handoff) covering every row above, with "unknown — asked the user" where the
answer is missing. Placement waits for the answer when the row is load-bearing
(current, voltage, connector position, enclosure).

---

## 2. Place by functional block and by circuit flow

Divide the board into blocks: power entry, protection, conversion,
processing, interfaces, sensors, power stage, and so on.

- **Arrange the blocks along the functional flow.** Connector → protection →
  filter → conditioning → converter → processor.
- **Keep directly interacting parts close**, especially when they share
  pulsed currents or sensitive signals.
- **Keep noise sources away from sensitive circuits.** Inductors, switching
  nodes, and power drivers do not share space with analog references and
  high-impedance inputs.
- **Place by pins, not by bodies.** Two parts can be adjacent and still force
  bad routes if they are badly oriented. Read `get_component_pads` for every
  part and turn it so its pads face their destinations.
- **Reserve routing corridors.** An over-packed board needs detours, vias,
  and bottlenecks it did not need.
- **Never sacrifice electrical paths to align parts for looks.**

### Placement order

1. Outline, holes, connectors, and mechanical constraints.
2. Large parts, heatsinks, and thermally constrained parts.
3. Critical circuits: switching supplies, RF, clocks, converters, fast
   interfaces.
4. Decoupling, terminations, feedback networks, protection.
5. Auxiliary circuits and the least critical parts.

The order is iterative: when a critical connection ends up bad, go back to
placement. Do not "fix" placement mistakes with routing.

### Parts that need especially careful placement

| Part or circuit | Practice |
|---|---|
| Decoupling capacitors | At the pin they serve, with a low-inductance path to the rail and the return. Loop length and area matter more than visual proximity. |
| Bulk / reservoir capacitors | Where they must supply the load transients; they do not replace local decoupling. |
| Crystals and oscillators | Short connections, away from noisy signals, per the IC's guidance. |
| Series termination | Normally next to the driver; confirm the topology the interface requires. |
| ESD and surge protection | At the external entry, before the path spreads across the board, with a short path to the intended discharge reference. |
| Switching supplies | Minimise the pulsed-current loops; keep feedback away from the switching node. |
| Shunt resistors | Kelvin connections for measurement; the sense path never includes the power trace drop. |
| Amplifiers and analog references | Compact sensitive inputs and feedback networks, away from interference sources. |
| Parts that heat | Plan dissipation; do not heat sensors, references, or temperature-sensitive parts. |
| Antennas and RF | Respect the geometry, reference, and keep-out areas of the specific design. |

Completion criterion: the placement gate in
[`placement-gate.md`](placement-gate.md) passes.

---

## 3. Plan the return current before routing the signal

**Every current needs an outgoing path and a return path.** Drawing only the
outgoing trace designs half of the connection.

At high frequency the return current concentrates under the signal trace on
its reference, following the lowest-impedance path.

- Prefer **continuous reference planes**, especially for fast signals.
- Do not run critical signals over slots, cutouts, or discontinuities in
  their reference plane.
- Do not let power currents share a return segment with sensitive
  measurements.
- When a signal changes layer, preserve the return continuity. When both
  references are GND, stitching vias near the transition help.
- When the reference changes between different planes, analyse the return
  path; one GND via alone may not solve it.
- Do not split ground into "analog" and "digital" by reflex. **Physical
  separation of blocks over one continuous plane is usually better than a
  poorly planned split.**
- Do not assume any copper fill is a good ground: it can form islands,
  bottlenecks, and long detours.

**Rule of thumb:** for every critical connection ask "where does its current
come back?" and write the answer down.

Where this lives in Konnect: `add_zone` / `add_copper_pour` for planes,
`refill_zones` after any change, `query_traces` and `get_component_pads` to
trace the actual copper path, `get_board_2d_view` or `export_svg` to look at
the plane for slots and islands. A 2-layer board with a broken B.Cu ground is
an open finding, not a style remark.

---

## 4. Size traces by electrical and thermal criteria

There is no universal width per current. Sizing depends on copper thickness,
layer, available dissipation, and the allowed temperature rise. Read
[`trace-width-table.md`](trace-width-table.md) for the calculation inputs
and the acceptance record; the key criteria are:

| Connection type | Main criteria |
|---|---|
| Power and supply | Current, heating, resistance, voltage drop, length, transients. |
| Fast digital signals | Impedance, continuous reference, reflections, crosstalk, delay. |
| Differential pairs | Differential impedance, consistent geometry, symmetry, skew. |
| Sensitive analog | Coupling, surface leakage, resistance, parasitic capacitance, noise. |
| High voltage | Clearance and creepage, material, environment, applicable safety rules. |
| RF | Impedance, losses, transitions, reference, connection geometry. |

Resistive estimate: `R = ρ · L / (w · t)`, `ΔV = I · R`, `P = I² · R`,
with `L` the length, `w` the width, and `t` the copper thickness. These help
judge losses; they replace neither the thermal analysis nor the impedance
calculation.

Also:

- Size the vias too: a wide trace ending in one small via is a bottleneck.
- Check necks at pads, connectors, and thermal reliefs.
- For high current use copper areas and several vias, and check how the
  current actually splits.
- For controlled impedance use the real stackup agreed with the fabricator.
- Do not enlarge the copper of a switching node by reflex: it raises
  capacitive coupling and radiated noise.

Store the accepted widths in netclasses (`create_netclass`,
`assign_net_to_class`, `get_netclasses`) and the routing palette
(`set_predefined_sizes`) before the first trace.

---

## 5. Route by criticality, not by the easiest connection

A useful sequence, adjusted to the circuit:

1. Critical loops of power supplies and local decoupling paths.
2. RF, clocks, differential pairs, controlled-impedance signals.
3. Sensitive analog inputs, references, feedback.
4. Main supply and power paths.
5. Everything else.
6. Length tuning, only where a timing requirement exists.

While routing:

- Use direct paths, but **never shorten a trace at the cost of destroying its
  return**.
- Reduce needless vias; do not treat "zero vias" as an absolute goal.
- Avoid long parallel runs between signals that can interfere.
- Keep sensitive signals away from nodes with large voltage or current
  swings.
- On adjacent signal layers without a plane between them, different dominant
  directions reduce parallelism, but do not replace a proper stackup.
- Use curves or 45° corners for consistency, without treating every 90°
  corner as an automatic electrical fault.
- Avoid stubs and dead-end branches on fast lines when they can produce
  relevant reflections.
- Length-match only where timing requires it; unnecessary serpentines add
  length and coupling.

Completion criterion: the routing gate in
[`routing-gate.md`](routing-gate.md) passes.

---

## 6. Avoiding the "spaghetti" board

The problem normally starts in placement, not in routing.

- **Look at the airwires before routing.** Many crossings mean badly oriented
  parts or badly distributed blocks. `score_placement` and a rendered view
  (`get_board_2d_view`) are the evidence; a mental picture is not.
- **Rotate and move parts** so pins point at their destinations.
- **Order interfaces and buses** following the signal order.
- **Swap pins only when electrically allowed**, updating the schematic and
  the firmware when needed.
- **Route the critical connections first** before freezing placement.
- **Reserve escape room** around dense packages, including vias and plane
  connections.
- **Do not force a complex board into too few layers** when that compromises
  return, fabrication, and performance.
- **Go back to placement when detours multiply.** Do not keep adding vias.
- Use automated routing as an aid, never as a substitute for constraints and
  technical review.

Layer crossings are normal. The goal is not to eliminate them but to keep
them from causing discontinuities, coupling, and needless complexity.

---

## 7. Configure the rules in the tool before drawing

Use netclasses and specific rules for:

- widths and clearances;
- via types and dimensions;
- differential pairs;
- impedance and allowed layers;
- lengths and delay differences;
- high-voltage isolation;
- distances to edges, holes, and metal regions;
- keep-out areas for copper, traces, and parts.

**A passing DRC means the layout passed the rules it was given — not that the
circuit will work.** Incomplete rules approve a bad board. Read
[`design-rules.md`](design-rules.md) for the rule-provenance workflow.

---

## 8. Review fabrication, assembly, test, and function

| Review | What to check |
|---|---|
| Electrical | Connections, polarities, footprints, pins, supply, and return. |
| Signal integrity | References, discontinuities, terminations, coupling, layer transitions. |
| Power integrity | Voltage drops, decoupling, bottlenecks, transient currents. |
| Thermal | Heat sources, available copper, thermal vias, effect on neighbours. |
| Fabrication | Trace, drill, mask, residual copper, and stackup limits. |
| Assembly | Spacing, solder and rework access, orientation, identification. |
| Test | Reachable supply, GND, programming, and key signals, without harmful stubs on fast lines. |
| Mechanical | Collisions, connectors, screws, enclosure, heights. |
| Final files | Inspection of fabrication files, drills, BOM, and placement. |

For critical designs, add simulation and prototype measurement.

The `kicad-review` skill's `references/layout-review.md` turns this table
into a tool-backed review branch.

---

## Summary

**Understand the circuit → define constraints → plan layers and returns →
place the blocks → validate the critical paths → route by priority → review
→ measure.**

The governing question is never only "does this trace fit here?" but
**"does this path preserve the return current, the signal integrity, the
temperature, and the fabrication?"**
