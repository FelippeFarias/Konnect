# JLCPCB Order-Contract Verification

Use this reference only for a JLCPCB fabrication or assembly branch. JLCPCB
controls its capabilities, prices, part categories, templates, and portal
validation. The current order contract is authoritative; this file deliberately
does not cache those volatile values.

## 1. Pin the selected service

Record the exact selected service before configuring the board:

- fabrication versus fabrication plus assembly;
- economic/standard or other service tier shown by the current portal;
- layer count, stackup, copper weight, thickness, finish, colour, and quantity;
- assembly side(s), stencil choice, panelization, and special processes; and
- controlled-impedance, via, slot, castellated, edge-plating, or other options.

For every copied limit or template requirement, record the source and retrieval date.
A generic capability page does not override the selected service's order-page
constraints.

Completion criterion: the order record names one selected service and contains
the current limits for every feature the design uses.

## 2. Verify parts against the live assembly branch

Use `search_jlcpcb_parts` and `suggest_jlcpcb_alternatives` to produce candidates,
then verify each candidate in the current assembly order:

- exact manufacturer part number and package;
- current category, availability, and quantity;
- feeder/setup or special-handling effects; and
- lifecycle or substitution constraints.

The downloaded catalogue is useful discovery evidence, not a live stock or price
guarantee. Record the catalogue date and recheck the order before payment.
Third-party mirrors of the catalogue lagged the fabricator's own part pages
by large margins on one project (a 1,792-piece "stock" was really 235); take
stock, library class, and price from the fabricator's current part page or
cart at order time.

For every line also verify:

- the part's **assembly tag** — through-hole parts handled by the manual or
  wave line are billed per joint, and a mis-tagged part is a large cost
  surprise; count THT joints per board;
- the **orderable suffix** the fabricator actually stocks (tape-and-reel
  versus tube can differ in stock and lifecycle);
- the **distributor datasheet link** — it can return an HTML page instead of
  a PDF; fetch the manufacturer's copy when the file does not start with
  `%PDF`.

## 3. Bind BOM and CPL to current templates

Generate BOM and position data through Konnect, then compare the exported headers,
units, side names, origin, delimiter, and designator grouping with the templates
offered by the selected service. Map the project supplier field deliberately;
do not assume one historical column spelling remains mandatory.

In the export preview, account for every placed designator and every intentional
DNP. Inspect pin 1, polarity, side, rotation, and footprint/package agreement for
each orientation-sensitive part. The preview, datasheet, footprint, and physical
pin-map evidence must agree.

Completion criterion: the portal accepts both files and the export preview has
no unexplained missing, extra, rotated, mirrored, or substituted component.

## 4. Apply current fabrication constraints

Copy trace/space, annular-ring, drill, slot, copper-to-edge, mask, silkscreen,
stackup, and impedance constraints from the selected service into project rules
and netclasses. Use the stricter applicable value when the component datasheet,
electrical calculation, or enclosure imposes a stronger requirement.

Re-run KiCad DRC after applying the contract and after every routing or placement
change. Inspect the Gerber and drill outputs in a viewer; a rule table alone does
not prove the exported geometry.

Contract items that KiCad's DRC does not check by default and that mattered on
a JLCPCB assembly order:

- silkscreen **graphic** line width (library footprints ship lines below the
  usual minimum; text rules do not cover graphics);
- annular ring of PTH component holes versus vias — separate limits, and
  thermal vias drawn as footprint pads count as component holes;
- the solder-mask bridge minimum for the ordered mask colour on fine-pitch
  parts;
- copper-to-edge on V-cut edges, which is larger than on routed edges;
- THT hole size against the purchased part's recommended hole.

## 4b. Mechanics observed on a real order — re-verify before relying on them

These observations from a September 2026 order drove decisions; they are
recorded as mechanisms with their date, not as current limits or prices.

- **The assembly tier changes the cost lever.** The Standard PCBA price page
  then charged a feeder-loading fee for every distinct part, basic and
  extended alike; the Economic tier charged it for extended parts only. Under
  Standard the lever is fewer distinct lines, not library class. Carry the
  tier as an explicit variable in every cost statement; an assistant stated
  the Economic rule for a Standard order and had to retract it.
- **Edge rails added by the fabricator** (offered for a single-board Standard
  PCBA order, with a choice of sides) carried the fabricator's own fiducials
  and tooling holes, so board-level tooling holes were wasted area; three own
  fiducials were still cheap insurance along a 400 mm board. The rails were
  joined by V-cut, which raised the copper-to-edge rule on those edges and
  required solder joints to stay clear of the break line.
- **Panel limits applied to panels.** A 400 mm single board could be
  assembled, but not panelized for assembly.
- **The Economic reflow profile peaked above the ESP32 module's rated
  maximum**, so the module forced the Standard tier.
- **Order-form options were more complete than the help pages**: rails on
  four sides and "added by the fabricator" were found in the order form after
  a help page suggested otherwise. Check the live order form before
  concluding an option does not exist.
- **Lot risk is not rank risk.** The pre-purchase terms guaranteed neither a
  single lot nor a lot choice, and a lot is not a forward-voltage rank. For
  binned parts, get the rank in writing or supply the parts.

## 5. Accept the package

Create a fresh export destination and apply the manufacturing skill's artifact
acceptance gate. Upload only the accepted manifest. In the order portal:

- confirm every copper, mask, silkscreen, paste, and outline layer;
- confirm plated/non-plated holes, slots, cutouts, dimensions, and layer count;
- review warnings and the rendered board preview;
- verify assembly mapping and rotations when assembly is selected; and
- save the final quote and order configuration as the purchasing record.

Any unexplained difference between the saved design, artifact manifest, and
export preview makes the result `INCOMPLETE`.
