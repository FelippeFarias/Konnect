# Datasheet Conformance Audit

Checking a design "item by item" finds typos. Comparing each block against
what its manufacturer requires finds what is **missing**, what is **extra**,
and what is **out of range** — the defects that pass ERC and DRC. On the
reference project this method found an input capacitor delivering 3.5 of the
required 10 µF, a power-up enable defect, a clone TVS rated at half the
surge of the part the design assumed, and a TVS clamp above the protected
pin's absolute maximum. Run it for every IC block before a pre-fabrication
verdict, and again after a part substitution.

## 1. Inputs

- The exact manufacturer part number and suffix from the BOM, and **that
  part's** datasheet. A sibling, another vendor's equivalent, or a
  distributor summary is a different document.
- Validate every downloaded file: it starts with `%PDF` and has a plausible
  size. Distributor links often return an HTML or JavaScript page saved as
  `.pdf` (9–10 kB, starting `<!DOC`); an audit once cited a 108-byte page as
  "the datasheet". Fetch the manufacturer's copy instead.
- The block's netlist (exported from the saved schematic), the BOM line, the
  footprint, and the board file for layout measurements.

## 2. Compare against the manufacturer, not against a checklist

For each block, read:

1. the **typical application circuit**;
2. the **recommended-component table** and selection equations;
3. absolute maximum and recommended operating conditions;
4. the **layout guidelines** section;
5. derating and characteristic curves that the design depends on.

Then list:

- what the manufacturer requires and the design lacks — treat it as blocking
  until disproved;
- what the design has in excess without a documented reason;
- what falls outside the equations or operating ranges;
- what the manufacturer recommends, even as optional, and whether it matters
  here.

## 3. Redo the equations with real values

Use the real input rail (after series diodes and fuses), tolerances (for
example L −20 %, switching frequency ±6 %, supply ±5 %, resistor tolerance),
effective values (MLCC capacitance after DC bias), and the worst column of
each table. Record each result against its limit and margin. Example set for
a buck converter:

| Check | Formula / source |
|---|---|
| Inductor ripple | ΔIL = VOUT·(VIN − VOUT)/(VIN·L·fSW), nominal and worst case |
| Peak current and saturation | IL,pk = IOUT + ΔIL/2; Isat from the worst column and its definition (for example −30 % L) must exceed the IC's **maximum** current limit so the inductor cannot saturate into a short |
| Input capacitor | Effective C at VIN from the maker's DC-bias curve; RMS current IOUT·√(D(1 − D)) against the ripple rating |
| Output capacitor | Effective C at VOUT inside the datasheet range; transient equation at the real load |
| Minimum on-time | tON = D/fSW at maximum VIN and maximum fSW, against tON,min |
| Dissipation | PD = POUT·(1/η − 1) − P(DCR), with η from the curve for this exact part and frequency; TJ = TA + PD·θJA, against the derating curve |

A table value is a starting point, not a requirement: a 6.8 µH inductor was
correct where the table listed 4.7 µH, because the datasheet's own ripple and
saturation rules held and the table was tuned for another input voltage.

## 4. Measure the layout against the layout section

Measure, do not eyeball: input hot-loop return length, GND vias at every
capacitor and at the IC ground pin, the zone connection style on the IC
ground pad (a thermal relief can leave only 41 % of the perimeter connected),
feedback routing (never under the switch node or inductor, never slicing the
plane there), bootstrap loop area, output-capacitor return to the IC ground,
and the copper area the datasheet's θJA assumes. Quantify each effect in
millivolts or degrees to decide between fix-now and next revision.

## 5. Output format

Four parts, always:

1. **Table** — manufacturer requires X / design has Y / verdict, with the
   section or figure for X.
2. **Equations** — redone with the design's numbers and tolerances.
3. **Findings** — by severity, each with its evidence and a validated fix.
4. **Checked and correct** — one line per item, with the method. This makes
   coverage visible and stops later reviews from re-doing the same work.

Every claim carries a measured number or a literal quote with its section.
When a value cannot be extracted, say what is missing instead of estimating
from memory.

## 6. When the datasheet is silent or wrong

- **No application section** (for example an RS-485 transceiver whose figures
  are all test circuits): declare it as a finding about the method, then fall
  back in this order — the pin-compatible part the vendor names, a modern
  part's layout section with explicit distances, standard-level application
  notes, the protection device's application section, cross-vendor
  application notes for quantities. Label each substitution.
- **A missing parameter**: take it from a named sibling part or an older
  revision of the same datasheet (a θJA table dropped from a newer revision),
  label it as a reference value, and turn the residual risk into a bench
  test.
- **Tables can be wrong**: one shift-register datasheet lists SRCK as pin 15,
  duplicating DRAIN5; the package drawing and logic symbol say 13. Build
  pinouts from the rendered package drawing, cross-check the table, the
  figure, and the logic symbol, and look for duplicate pin numbers.
- **Family text can mislead**: a fixed-output regulator's pin table said FB
  goes to "the resistive divider"; the application section for the fixed part
  ties FB straight to VOUT.
- **Column labels and internal contradictions**: design to the guaranteed
  parametric limit when a truth table and a description disagree.
- **Distributor attributes** may show typical values as if they were limits;
  a distributor's Vf range is often the union of all bins.
- **Curves** that decide a value are digitised, not eyeballed: render the page
  at high resolution, calibrate two ticks per axis, trace, fit. After an LED
  change, a derating slope of −0.200 mA/°C carried over from the previous
  part kept being used; the new part's digitised curve was −0.265 mA/°C, 25 %
  steeper, which removed the margin the old number seemed to give.

## 7. Part identity is one object

The schematic symbol and Value, the BOM manufacturer part number and
distributor code, the footprint land pattern, and the archived datasheet
must describe the same physical part. Check each of these, because every one
slipped on the reference project:

- **Suffix variants with different pinouts in the same package**: AP2114H
  (1 = GND, 2 = VOUT, 3 = VIN) versus AP2114HA (1 = VIN, 2 = GND, 3 = VOUT),
  both SOT-223. Lock the full suffix in the BOM.
- **Clones** carrying the same headline voltages with less surge capability:
  a 300 W "SM712" clone was ordered while the schematic Description quoted
  600 W. Parts whose ratings enter a calculation are selected by the specific
  distributor code's own datasheet, never by stock.
- **Family swaps**: 74AHCT125 inputs have no clamp to VCC and tolerate 5.5 V
  unpowered; 74HCT125 inputs clamp to VCC. The swap, made for stock, injected
  72.7 mA per input into a dead rail in USB-only power mode.
- **Land patterns**: a brand-name footprint used for a clone terminal block
  had 1.30 mm holes where the purchased part's drawing asks for 1.50 mm; an
  inductor substitute needed 2.8 × 5.7 mm pads, not the original 2.35 × 5.10.
- **Mechanical variants**: the highest-stock tactile switches were
  side-actuated.
- **Lifecycle and packaging**: the tube suffix of a driver was obsolete while
  the tape-and-reel suffix was active and stocked.
- **Value text**: the Value must name the ordered part (a board said MAX3485
  for an SP3485), with ratings in it.

## 8. Severity and verification

- Rate a finding blocking or high only when a file fact numerically violates
  a documented limit at a defined worst case. Model estimates with assumed
  parameters stay below that unless the margin is negative across every
  plausible assumption.
- Before relaying a subagent's datasheet finding, re-verify the numbers **and
  the reasoning** that change the decision. On the reference project the
  coordinator overturned four agent conclusions: a power-up glitch window
  re-derived from the real rail sequence (about 2 ms, cosmetic), a
  "3× brightness" claim that stacked two independent worst cases, a
  ground-separation resistor claimed to relieve bias-resistor stress (it
  raises it), and a "grounded NC pin" alarm (no internal connection).
- A proposed fix is part of the finding: validate it with the same equations
  before recommending it.
