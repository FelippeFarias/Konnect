# Routing Playbook — Clean, Harmonic, Reviewable Copper

`layout-methodology.md` says what to decide; this playbook says how the
decisions were executed on a real 2-layer board: 400 × 105 mm, 362
footprints, 198 THT LEDs, seven daisy-chained drivers, an MCU module with
USB-C, and two field interfaces. The user had demanded clean, "harmonic"
routing and approved the result after reviewing each copper layer. Measured
on the final board file: 1,600 segments and 745 vias (including stitching),
86 % of the copper length orthogonal and 9 % at 45°, no diagonals across
blocks, and identical routing in every repeated block. Two autorouter
attempts on the same board were rejected. Read this file before routing any
board with repeated blocks, long buses, a module, or a dense connector
corner.

## 1. Plan the layers and the return before the first trace

Write a layer plan and keep to it:

| Layer | Job on the reference board |
|---|---|
| F.Cu | LED strings and their resistors, the 12 V buses and verticals, short links inside a block, USB escapes |
| B.Cu | The logic bus as parallel lanes, MCU-to-interface feeders, short jumps under crossings, GND fill |
| Both | GND pours after routing, stitched together |

- In a dense region give each layer one dominant direction (long horizontals
  on B.Cu, descents on F.Cu) and resolve crossings by a layer change at a via
  or a THT pad rather than by a detour around the other trace — while fast or
  sensitive nets keep their reference plane (`layout-methodology.md` §3).
- Decide which layer is the reference plane and how long a cut in it may be.
  Several long tracks side by side on the "plane" layer merge their clearances
  into one slot: four 400 mm bus lanes on B.Cu formed a 13.4 mm void that
  crossed every driver package, and the clock and latch lines ran 271 mm in
  parallel over it with no return beneath. Keep the plane layer's long runs
  few, interleave static nets (supply, enable) between edge-sensitive ones,
  and leave room for guard copper only where
  `gap ≥ 2 × zone clearance + minimum fill width` (0.95 mm with 0.35/0.25).
- On a THT-populated board every plated pad blocks **both** layers; plan bus
  corridors through the gaps between pads (§3).

## 2. Make the fan-out planar during placement

Most crossings are decided by pin order, before any copper exists.

- **Match interchangeable outputs to geometry.** List the loads' positions
  around the driver in angular order and assign channels so the pin order
  along each pad row equals that order. For a seven-segment digit driven by a
  TPIC6B595 rotated 270°: top row pins 7, 6, 5, 4 → segments g, f, a, b;
  bottom row pins 14–17 → e, d, c, dp. Every drain then fans in without a
  crossing. Swapping identical channels is a placement decision; record the
  resulting bit map for firmware on the schematic.
- **Pin-swap rules** when a pin matrix or free choice exists:
  1. move a function to a free pin on the module face that faces its
     destination — never a strapping, reserved, or dedicated-peripheral pin;
  2. order a bundle's pins to match the destination IC's pin order (an
     RS-485 TX/DE swap made four signals run parallel with zero crossings);
  3. put each passive of a pair on the side of the pin it serves (the CC1
     resistor on the CC1 side of a USB-C connector, CC2 on the other);
  4. move passives out of a bundle's arrival path.
  Simulate the swap on a scratch copy and route it first. Then edit the
  schematic — labels **and** no-connect flags — rewrite the pin-map note, run
  ERC, and sync; the sync dry-run must list exactly the predicted pads.
- **Put the series part where it lets passives form a bank.** A string
  resistor can sit at either end of a series string with the same current.
  On the cathode side, every digit's resistors formed one aligned bank beside
  the driver, each resistor over its own drain pin, while the anodes went
  straight to the supply bus. Resistors scattered next to each load were
  shorter but looked chaotic, and the user rejected them.
- **Rotate THT parts along the chain**: each LED's cathode faces the next
  LED's anode, so every link is a straight stub on the chain axis. For
  `LED_D5.0mm` the origin is pad 1 (cathode) and the body centre sits 1.27 mm
  along the anode direction; compute the anchor from the intended body centre
  and verify one part with `get_component_pads` before a batch.
- Take array pitch from the courtyard (6.5 mm for 5 mm LEDs) and from label
  fit: a bank of 1206 resistors with 1 mm references between them needed a
  3.8 mm pitch; 2.6 mm gave 113 label collisions.
- Count airwire crossings per block before and after each rotation or swap. A
  block with crossings is not ready to route.

## 3. Buses as lanes

- Route a bus as parallel lanes at one pitch (1.0 mm for 0.25 mm tracks
  here), in the same order as the pins at both ends.
- **Planarity rule for taps and feeders**: order feeders so the depth of each
  bend grows steadily across the bundle; a feeder may cross only lanes that
  continue past it. A feeder crossing a lane that ends before it is a
  crossing you created.
- **Nested-L fan-in to a pad row**: the trace to the farthest pin takes the
  outermost lane, lanes about 1 mm apart, each entering its pad
  perpendicularly.
- **Windows through THT rows**: a lane crosses a row of through-hole pads only
  through the gap between two pads, entered and left with 45° ramps, sized
  from the real pad geometry and clearance. Alternate the lanes of cascaded
  signals (SER_IN and SER_OUT) so their vias never coincide.
- **Tap a driver under its body**: a via at the pin's x, then a short stub on
  the component side.
- Keep lane width and pitch constant; jog only where a window or pad forces
  it, with 45° chamfers of one fixed length (0.9 mm here).

## 4. Repeated blocks: solve once, stamp N times

- Solve the topology of one block on paper, write it as data in coordinates
  relative to the block origin (net role, layer, width, points), and stamp it
  N times by offset — with `copy_routing_pattern` (it edits the saved board
  file, so the board must be closed in KiCad; pass a net map for each copy)
  or a generator on a scratch copy. Special-case the ends of a chain (the
  first block has no incoming bus; the last one turns toward the
  controller).
- A parametric bug multiplies by N: an inverted top/bottom condition turned
  18 crossings into 100. Verify the first instance with DRC and a rendered
  crop, then re-verify every copy.
- Allow a bounded number of short layer jumps per block (two here: one
  string dips under a row exit, one anode feed under another) and keep them
  identical across blocks. Where a string must cross another net on a THT
  board, move one LED-to-LED link to B.Cu: through-hole pads exist on both
  layers, so the jump costs no via.

## 5. Dense corners: MCU, USB-C, connectors

"Write every path and fix what DRC reports" produced about 150 coupled
violations here and was thrown away. The method that worked:

1. Freeze everything already routed and verified.
2. Dump the region's exact geometry: real pad extents and existing copper.
3. Take the exact missing links from DRC's unconnected items (JSON), filtered
   by net, not from memory or a "nets without copper" list.
4. Use a fast windowed clearance check as the inner loop; keep the full DRC
   as the gate, because the fast check cannot see holes, mask, edges,
   courtyards, or silkscreen.
5. Remove crossings with engineering changes first (§2 pin swaps, passives
   moved to the side of their pads or out of arrival paths).
6. Declare one layer rule for the region.
7. Route block by block (top area, USB, interfaces, splices), each gated on
   zero collisions **and** zero unconnected items for its non-deferred nets.

Specific patterns:

- **USB-C receptacle**: data escapes stay on the component side at the
  fabricator-safe width (0.2 mm here); anything that must cross them passes
  underneath or behind the connector. Join duplicated pins (A/B VBUS, the
  second D− pin) behind the connector with short jumpers.
- **No strap, boot, or reset net under a metal connector shell** on the
  component side: only solder mask separates it from the shell, and a short
  there latches the MCU in download mode.
- **Module fan-out**: a short stub from each pin to a via column just outside
  the pads, then lanes at the pin pitch on the other layer. Consecutive pins
  with the same destination, in the destination's order, give a planar
  bundle.
- **Interface columns**: terminal → PTC → TVS → series resistor → transceiver,
  with the transceiver's bus pins facing the connector, the termination
  between the pair without crossings, the TVS within a few millimetres of the
  connector with 2–3 GND vias beside its ground pin, and A/B routed as a
  close, equidistant pair.
- **Power entry** on the edge facing the power supply, with the high-current
  trunk and its return running around the board edge so it never crosses the
  MCU-to-transceiver or transceiver-to-connector paths.

A trace narrower than its netclass is acceptable only as a recorded exception
with a current-capacity number (a 0.15 mm VBUS branch between a connector pad
and its alignment hole carried 0.5 A against a 0.6 A capacity, while the main
path was widened to 0.4 mm).

## 6. Autorouter policy

The autorouter is a draft generator, never the result. The first full-board
run reached "0 unrouted" and was presented as good; it had 148 track-width
errors from neck-downs, 250 vias, and diagonals across blocks.

- Route the structured nets yourself first and lock them; KiCad's Specctra
  export writes locked tracks as fixed wiring.
- Keep GND out of the autorouter; convert fixed GND copper to keepouts, and
  make antenna and mechanical keepouts explicit.
- Disable fan-out on THT/SOIC boards (it added 266 escape vias here).
- Stop when the unrouted count stalls for two passes, and trust KiCad's
  counts, not the router's (its "92 unrouted" was 16 in KiCad).
- Remove zero-length stubs and duplicates of existing copper from the result.
- Accept a net only when: DRC shows 0 errors under the real rules, its length
  is close to the shortest possible path, it has no more vias than its layer
  changes require (signals 1–2), it crosses no other block diagonally, and
  a rendered image agrees. A length ratio alone misled: most nets were ≤ 1.3
  while the image was spaghetti; the via counts (6 on a clock, 18 on a
  supply) showed it.
- When many nets in one region fail, the cause is placement or pinout: fix
  those, not the routes.

## 7. Ground pours and stitching

- Pour GND on both layers after signal routing, inset from the edge by the
  copper-to-edge rule, and cut every antenna keepout out of both layers.
- Stitch on a grid (6–10 mm here). Accept a site only if it is clear of
  other-net copper by more than the clearance rule (0.45 mm, so the pour flows
  around it), 0.6 mm from other GND vias, clear of mounting holes and the edge,
  and outside THT bodies and antenna zones.
- Give every IC ground pad its own vias to the plane — at least two
  independent paths for ESD arrays, transceivers, and bus buffers — and place
  the stitching via at the IC ground pin, not beside the capacitors. Never
  rely on a neighbouring part's pad as the path.
- Every filled island needs at least two connections; one via makes a
  resonant patch that loses its ground silently if the via fails.
- Starved thermal reliefs: make room for the spokes first; a solid connection
  is the fallback (on a two-terminal passive it adds tombstoning risk).
- Prove the result by graph, not by picture: GND forms one group, and no pad,
  via, or neck is an articulation point for an IC ground.

## 8. Widths, vias, and joints

- Widths come from the sizing record and the netclass
  (`trace-width-table.md`). Reference board: LED strings 0.3 mm, drains
  0.2 mm, logic 0.25 mm, 5 V/3.3 V/VBUS 0.4 mm, local 12 V 0.5 mm, 12 V buses
  1.0–2.0 mm where voltage drop along 400 mm governed.
- For redundancy, use at least two vias wherever a supply of more than about
  0.5 A changes layer: one via can carry it, but it is then a single point of
  failure for the whole rail.
- Size power vias so that any drill the tools may apply still leaves the
  required ring (0.8/0.4 mm on power classes here), because the Specctra
  session importer applies the netclass drill.
- End every track at the via centre; a track ending on the edge of a via
  ring is a tangential joint that connectivity checks accept.

## 9. Scratch copies, previews, and transfer

- Develop on a disposable copy outside the project folder, changed only with
  KiCad's own Python API (the konnect skill's One Rule covers what may touch
  the project): keep placement as data, rebuild the copy from scratch on
  every iteration, and run DRC with the project's rule files copied beside it
  (matching basenames; confirm a known custom-rule hit still appears).
- Where only Konnect tools are available, the equivalent is small, undoable
  IPC batches on the live board, each checked and saved before the next.
- Show the user evidence they can judge before transferring: each copper
  layer rendered separately (a full-board pour otherwise looks "all blue"),
  zoomed crops of dense areas, and the DRC and connectivity numbers.
- Transfer in a written order, verifying each step before the next:
  1. schematic changes, ERC, sync dry-run listing exactly the predicted pads,
     apply;
  2. placement delta in one batch, then `save_project`;
  3. routing into a board with no tracks (`apply_specctra_ses` when its
     preconditions hold, otherwise the user runs KiCad's File → Import →
     Specctra Session, or the verified generator is re-run on the closed
     board under the konnect skill's scripted board fallback), verified by a
     full signature — tracks and vias with width, diameter, and drill —
     before saving;
  4. pours and refill;
  5. DRC with parity on the saved board, and the ground graph.
- Save after every live batch and read the result back from the file: unsaved
  IPC placement was lost twice when the editor closed.
- Never move a routed footprint by shifting track ends; move it, rip up and
  re-route its connections, and back up first.
