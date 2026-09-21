# ERC/DRC Error Taxonomy

## ERC Error Severity

### CRITICAL (must fix before fabrication)

| Error | Meaning | Fix |
|-------|---------|-----|
| Pin connected to incompatible pin | Power output driving another power output | Verify net assignments, add diode/resistor |
| Unconnected power pin | IC power pin floating | Connect to appropriate power rail |
| Net with no driver | Signal net has only inputs | Add a driver (output pin, label to source) |
| Conflicting net names | Two labels on same wire segment | Remove duplicate, verify intended net |

### WARNING (investigate, may be intentional)

| Error | Meaning | Fix |
|-------|---------|-----|
| Unconnected pin | Pin without connection or no-connect marker | Add `no_connect` if intentional, wire if not |
| Pin not driven | Input pin without a driver on its net | Verify net has an output pin somewhere |
| Bidirectional pin conflict | Multiple bidirectional pins contending | Usually OK for buses, verify if intentional |
| Power pin not driven | Power input without a power flag | Add `PWR_FLAG` symbol to the net |

### INFO (usually benign)

| Error | Meaning |
|-------|---------|
| Duplicate reference | Two components with same refdes (pre-annotation) |
| Missing value | Component without a value field |
| Unresolved text variable | `${...}` variable without a definition |

## DRC Error Severity

### CRITICAL

| Error | Meaning | Fix |
|-------|---------|-----|
| Clearance violation | Copper-to-copper too close | Move trace or reduce width |
| Short circuit | Two different nets touching | Reroute or fix via placement |
| Unconnected items | Ratsnest not fully routed | Complete routing |
| Missing footprint | Component without a footprint | Assign in schematic, re-sync |
| Pad near edge | Copper pad too close to board edge | Move component inward |

### WARNING

| Error | Meaning | Fix |
|-------|---------|-----|
| Silk over pad | Silkscreen overlapping exposed copper | Move silk text |
| Courtyard overlap | Two components physically overlapping | Move component |
| Via near edge | Via too close to board outline | Move via inward |
| Minimum width | Trace narrower than design rule | Increase width or adjust rule |
| Annular ring | Via/pad ring too thin | Increase pad size or reduce drill |

### INFO

| Error | Meaning |
|-------|---------|
| Isolated copper | Copper island not connected to any net |
| Missing courtyard | Footprint without courtyard layer |
| Duplicate footprint | Two footprints with same reference |

## Layout Quality Findings (heuristic until corroborated)

DRC does not report these. They come from pad positions, trace lists,
placement scores, and the rendered board, and they are classified only once
a pad position, a trace, or a DRC item corroborates the picture.

### CRITICAL

| Finding | Meaning | Fix |
|-------|---------|-----|
| Pad outside outline or inside edge clearance | Anchor placed near the edge; the far pad left the board | Move the part inward; re-read `get_component_pads` |
| Same-number pads not bridged | Switch or connector node open on copper | Add a trace between the two pad instances |
| Trace narrower than the current record | Heating or voltage drop in service | Widen per the netclass record |
| Tangential track-to-via joint on a single-path net, measured | The track copper does not reach the plated barrel (measured end-to-centre distance against the via radius); connectivity passes and the joint fails in production; DRC shows only a track-not-centred-on-via warning | Move the track end to the via centre |
| Protection part whose clamp exceeds the protected pin's absolute maximum, or a rail left unclamped after a change | Surge energy reaches the IC | Series resistance between TVS and IC; re-check the port × threat × clamp matrix |
| Part identity mismatch (Value, BOM code, footprint, datasheet describe different parts) | Wrong pinout, rating, or land pattern ordered | Lock the full suffix; verify against the ordered part's own datasheet |

### WARNING

| Finding | Meaning | Fix |
|-------|---------|-----|
| Trace crosses a part body or courtyard | Wrong pad instance chosen, or placement not pin-aware | Reroute from the nearer pad; rotate or move the part |
| Fast or sensitive trace over a plane slot | Return discontinuity | Reroute over continuous reference, or close the slot |
| Decoupling loop long | Rail noise; schematic promise not kept | Move the cap to the pin; shorten the return |
| Noise source beside a sensitive part | Coupling | Move the block; add distance or shielding copper |
| Connector not at an edge; control unreachable | Assembly or use blocked | Move to the edge the enclosure needs |
| IC ground pad depending on one pad, via, or neck | An articulation point in the ground graph: one defect disconnects the IC's ground. Escalate to CRITICAL when the only path runs through another part's pad or a single via under an ESD or interface device, and the geometry is measured | Dedicated vias at the IC ground pins, at least two paths |
| Via inside an SMD pad opening | A tented via still leaves its hole bare inside the pad's mask opening; solder wicks and the joint starves; DRC is silent for a same-net via | Move the via out of the opening, keeping the mask web |
| Silkscreen legend nearer another part or pin than its own | A field technician wires the wrong pin | One legend per pin, centred on it; re-check the nearest-target distance |
| Silkscreen graphics below the fabricator's minimum line width | Polarity marks may not print; text rules do not cover graphics | Raise graphic line widths |
| Strap, boot, or reset net under a metal connector shell | A shell short latches the MCU in the wrong mode | Move the net to the other layer or around the part |
| Long bus on the plane layer slotting the plane under clock lines | Return discontinuity and crosstalk | Reorder lanes, interleave static nets, stitch across, or move the run |

### SUGGESTION

| Finding | Meaning |
|-------|---------|
| Acute angles, needless direction changes, needless vias | Yield and readability |
| No test points on supply, GND, or debug signals | Harder bring-up |
| References unreadable | Assembly and debug |

## Interpreting Results

When `run_erc` or `get_drc_violations` returns results:

1. **Count by severity**: errors first, then warnings
2. **Group by type**: often one root cause creates multiple violations
3. **Fix in order**: 
   - Shorts/clearances first (fabrication blockers)
   - Unconnected items (design incomplete)
   - Warnings (quality issues)
   - Info (cosmetic)
4. **Re-run after fixes**: one fix may resolve multiple violations

Before interpreting any count, confirm the configuration could produce it
(see `verification-traps.md`): a constraint at 0 is off, an ignored severity
hides its class, and parity is empty unless it was requested. A large uniform
class of warnings is a symptom: open one instance and name its root cause
before waiving the class. Classify library-mismatch warnings with a
structural diff (a derived symbol flattened and compared line by line) rather
than by assumption.
