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

### Worst-case values (records per `kicad-schematic` `references/design-calculations.md`)
- [ ] Current budget per rail at the worst corner, including auxiliary outputs and regulator inputs
- [ ] LED and indicator currents at both corners (Vf min/max, supply min/max) against the forward-current limit derated at the maximum internal ambient
- [ ] PTC hold current at the hottest local temperature with margin; PTC V_max above the downstream clamp; R1max used in every series budget
- [ ] Input chain order connector → fuse/PTC → TVS → series diode → bulk; a protection matrix per port, counting every connector pin that exports a rail
- [ ] LDO headroom along the whole diode chain at the peak load, from the guaranteed dropout
- [ ] MLCC effective capacitance at the working voltage where a datasheet asks for a value; ratings in the Value field
- [ ] Bulk and high-frequency capacitance on every entry net, VBUS included
- [ ] Series-diode and regulator junction temperatures with the copper really connected
- [ ] Internal enclosure temperature estimated (dissipation, sun) and every temperature grade checked against it

### Interfaces and power-up (per `kicad-schematic` `references/interface-design.md`)
- [ ] Logic thresholds taken from the receiving part's datasheet; buffers on the same rail as ratiometric inputs
- [ ] Power-domain matrix: no input clamp back-feeding an unpowered rail in any supply mode
- [ ] Every enable, output-enable, reset, and strap line defined from power-up by a source already powered; no pull-up expected to beat a push-pull output
- [ ] RS-485 fail-safe above the threshold with each termination option, driver load ≥ 54 Ω, bias-resistor power at the common-mode extremes
- [ ] TVS clamping voltage below the protected pin's absolute maximum, or series resistance between them
- [ ] Series termination on long multi-drop lines sized from Z0; far-end overshoot below the receivers' absolute maximum
- [ ] Connector pinout checked against the cable that will be plugged in, from two independent sources; no back-door power pin into a protected rail
- [ ] Firmware requirements the hardware relies on written down (register clear before enable, GPIO initial levels, thermal PWM)
- [ ] Sheet notes and the GPIO map match the circuit

## PCB Review

### Mechanical
- [ ] Board outline is closed (no gaps) and does not self-intersect (notches drawn by breaking the edge)
- [ ] Mounting holes placed and correct diameter; unsupported spans bounded; a hole near every screw terminal
- [ ] Connector positions accessible from enclosure; edge-connector faces flush with or slightly proud of the edge
- [ ] Keep-out zones around antennas/RF sections as rule areas on every layer; notch or overhang where the panel plan allows
- [ ] Board dimensions match enclosure
- [ ] Solder joints far enough from V-cut or break-off edges, measured pad-to-edge

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
- [ ] Interchangeable outputs assigned in the geometric order of their loads; pin swaps reflected in the schematic and firmware map
- [ ] Silkscreen clean at placement: no overlaps, no ink on copper, references legible and upright, legends beside the parts they name
- [ ] Every footprint has a THT or SMD attribute and a resolving 3D model

### Return paths
- [ ] A written return path for every critical net (plane, trace, layer changes, stitching vias)
- [ ] No fast or sensitive trace over a slot or cutout in its reference
- [ ] Power currents do not share a return segment with sensitive measurements
- [ ] Ground not split by reflex; blocks separated over one continuous plane
- [ ] Copper fills checked for islands and bottlenecks
- [ ] No GND articulation point at an IC ground pad; ESD arrays, transceivers, and bus buffers have at least two ground paths; every island has at least two connections
- [ ] Long bus runs on the plane layer do not slot the plane under clock lines; clock and latch lines not neighbours

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
- [ ] Every net at or above its netclass width (DRC does not enforce it); accepted exceptions carry a capacity number
- [ ] Track ends centred on vias (no tangential joints); no via inside an SMD pad opening
- [ ] At least two vias, for redundancy, wherever a supply of more than about 0.5 A changes layer
- [ ] No strap, boot, or reset net under a metal connector shell
- [ ] Autorouted nets accepted on length, via count, and image, not on "0 unrouted"
- [ ] Via diameters and drills audited after any Specctra session import

### DFM (Design for Manufacturing)
- [ ] All traces/spaces meet fab house minimums
- [ ] All holes meet minimum drill size
- [ ] Annular rings adequate
- [ ] Silkscreen not overlapping pads
- [ ] Component courtyard no overlaps
- [ ] Thermal relief on ground pour connections
- [ ] Fiducial markers (for assembly, 3 minimum), unless the order configuration puts them on rails the fabricator adds
- [ ] Silkscreen graphic line width at or above the fabricator minimum, not only text
- [ ] Annular rings judged per category: vias, PTH component holes, and NPTH have different limits
- [ ] THT holes from the purchased part's drawing, not the footprint's namesake
- [ ] Paste coverage counts paste-only apertures under thermal pads
- [ ] Solder-mask bridges on fine pitch checked against the minimum for the ordered mask colour

### Assembly
- [ ] All components have correct footprints
- [ ] Polarity markings visible (caps, diodes, ICs)
- [ ] Reference designators readable
- [ ] Component values on silkscreen (or fab layer)
- [ ] Test points accessible for debug
- [ ] Rotation-risk list for the placement file, checked in the fabricator's preview

### Verification configuration
- [ ] No fabrication-relevant constraint left at 0 (a 0 disables the check)
- [ ] Every ignored or downgraded rule severity justified
- [ ] Custom rules present and not looser than Board Setup; copies checked with their rule files
- [ ] Schematic parity actually checked

### BOM and order
- [ ] Every Value names the ordered part, with its ratings; one rating per Value + footprint group
- [ ] Distributor code and full manufacturer suffix on every line (same-package pinout variants exist)
- [ ] Every substitution re-verified against its own datasheet: pinout, ratings, input structure, land pattern, actuation
- [ ] Protection parts chosen by the specific code's verified surge rating, not by stock
- [ ] Lot ceiling per line (stock ÷ quantity per board); binned parts ordered from one confirmed rank (a lot is not a rank)
- [ ] Order configuration recorded: assembly tier, rails and V-cut, who adds fiducials and tooling holes, reflow profile against part maxima
- [ ] Datasheet provenance for every pinout and threshold (primary document, library, or memory)

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
| DRC with parity | `run_drc` (parity field); kicad-cli with `--schematic-parity --severity-all` |
| Datasheet conformance | `references/datasheet-audit.md` |
| False-clean traps | `references/verification-traps.md` |
