# Pre-Fabrication Design Checklist

## Schematic Review

### Power
- [ ] Every IC has decoupling cap (100nF minimum, close to VCC/GND pins)
- [ ] Bulk capacitor on each power rail (10µF–100µF at entry point)
- [ ] Power indicator LED (optional but recommended for debug)
- [ ] Reverse polarity protection on external power input
- [ ] Voltage regulator output cap per datasheet recommendation
- [ ] PWR_FLAG on power nets without power-output pins (prevents ERC error)

### Signal Integrity
- [ ] I2C lines have pull-up resistors (4.7k for 100kHz, 2.2k for 400kHz)
- [ ] SPI chip select lines have pull-ups (prevent floating during boot)
- [ ] Reset pins have RC filter (100nF + 10k pull-up)
- [ ] Unused op-amp inputs tied to known state
- [ ] Crystal load caps match crystal specification
- [ ] ADC reference has dedicated decoupling

### Protection
- [ ] ESD protection on external-facing interfaces (USB, Ethernet, GPIO headers)
- [ ] TVS diodes on power inputs (if external power)
- [ ] Current limiting resistors on LEDs
- [ ] Gate resistors on MOSFET drivers (prevent ringing)

### Connectivity
- [ ] No unconnected pins (except NC pins marked with no-connect)
- [ ] No floating inputs on logic ICs
- [ ] All nets have at least 2 connections (no single-pin nets)
- [ ] No shorted nets (distinct nets accidentally merged)

## PCB Review

### Mechanical
- [ ] Board outline is closed (no gaps)
- [ ] Mounting holes placed and correct diameter
- [ ] Connector positions accessible from enclosure
- [ ] Keep-out zones around antennas/RF sections
- [ ] Board dimensions match enclosure

### Placement
- [ ] Constraint record written (size, connectors, currents, voltages, edges, sensitivity, layers, fabricator, assembly)
- [ ] Functional blocks grouped and ordered along the circuit flow
- [ ] Every pad of every part inside the outline and the edge clearance (anchor is not the centre)
- [ ] Parts rotated so pads face their destinations; airwires do not cross bodies
- [ ] Connectors at edges; buttons, LEDs, displays, test points reachable in the enclosure
- [ ] Noise sources (inductors, switching nodes, drivers) away from references, crystals, sensitive inputs
- [ ] Decoupling at the pins it serves with a short loop to rail and return
- [ ] Hot parts away from temperature-sensitive parts; dissipation planned
- [ ] Pads sharing one number (switches, connectors) recorded for bridging

### Return paths
- [ ] A written return path for every critical net (plane, trace, layer changes, stitching vias)
- [ ] No fast or sensitive trace over a slot or cutout in its reference
- [ ] Power currents do not share a return segment with sensitive measurements
- [ ] Ground not split by reflex; blocks separated over one continuous plane
- [ ] Copper fills checked for islands and bottlenecks

### Routing
- [ ] Routed in criticality order (supply loops, decoupling → clocks/RF/pairs → sensitive analog → power → rest)
- [ ] No unrouted nets (ratsnest clear), including bridged same-number pads
- [ ] Power traces and vias sized from the current record, not a fixed table
- [ ] Trace widths match the netclass record
- [ ] No trace crossing a part body or courtyard; traces leave pads toward their destination
- [ ] Differential pairs length-matched (USB, Ethernet)
- [ ] No acute angles on traces (acid traps)
- [ ] Via-in-pad only where needed (adds cost)
- [ ] Ground pour on back (or both sides), refilled after the last change
- [ ] Board saved before DRC and renders; rendered board inspected

### DFM (Design for Manufacturing)
- [ ] All traces/spaces meet fab house minimums
- [ ] All holes meet minimum drill size
- [ ] Annular rings adequate
- [ ] Silkscreen not overlapping pads
- [ ] Component courtyard no overlaps
- [ ] Thermal relief on ground pour connections
- [ ] Fiducial markers (for assembly, 3 minimum)

### Assembly
- [ ] All components have correct footprints
- [ ] Polarity markings visible (caps, diodes, ICs)
- [ ] Reference designators readable
- [ ] Component values on silkscreen (or fab layer)
- [ ] Test points accessible for debug

## Using Konnect for Review

| Check | Tool |
|-------|------|
| Unconnected pins | `find_orphan_items` |
| Shorted nets | `find_shorted_nets` |
| Single-pin nets | `find_single_pin_nets` |
| ERC violations | `run_erc` |
| DRC violations | `get_drc_violations` |
| Decoupling audit | `audit_decoupling(schematic_scope="hierarchy")` for a hierarchy root |
| Connection audit | `audit_connections(schematic_scope="hierarchy")` for a hierarchy root |
| Power rail audit | `audit_power_rails(schematic_scope="hierarchy")` for a hierarchy root |
| BOM health | `check_bom_health(schematic_scope="hierarchy")` for a hierarchy root |
| DFM audit | `audit_manufacturing` |
| Full review | `run_design_review` |
| Pads inside outline | `get_component_pads` against `get_board_extents` |
| Placement quality | `score_placement`, `get_board_2d_view` |
| Trace widths and paths | `query_traces`, `get_netclasses` |
| Layout quality branch | `references/layout-review.md` |
