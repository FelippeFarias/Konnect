# Trace, Via, and Impedance Sizing

This reference defines the sizing process, not universal dimensions. Store the
accepted results in project netclasses and predefined sizes so routing tools use
the same values that were reviewed.

## Current-carrying traces

For each current-carrying net, capture:

- continuous and transient current;
- copper thickness and plating assumptions;
- external or internal layer;
- ambient and temperature-rise budget;
- trace length and voltage-drop budget;
- available routing width and thermal environment; and
- the selected fabricator's current minimums and stackup.

Use an accepted current-capacity method or calculator with those inputs. Record
the method, inputs, result, and chosen margin. Changing copper weight, layer,
temperature, length, or allowed drop requires a new calculation; a scale factor
is not sufficient acceptance evidence.

Completion criterion: the selected width satisfies both thermal and voltage-drop
limits and is no narrower than the current fabrication contract.

### Worked method: a long supply bus with distributed loads

On a 400 mm board feeding 54 LED strings (about 1.1 A in total), heating
alone asked for 0.34 mm and voltage drop asked for 1.5 mm. Compute both and
keep the wider result.

1. **Heating** — IPC-2221 for an external layer:
   `I = 0.048 · ΔT^0.44 · A^0.725` (I in A, ΔT in °C, A in mil²; use 0.024
   for an internal layer). 1.1 A at ΔT = 10 °C gives A ≈ 18.6 mil², i.e.
   0.34 mm of 1 oz (35 µm) copper.
2. **Resistance** — `R = ρ · L / (w · t)`, ρ = 1.724 × 10⁻⁸ Ω·m at 20 °C
   (+0.393 %/°C). 1.5 mm × 35 µm × 0.35 m gives R ≈ 0.115 Ω.
3. **Drop** — loads tapped evenly along the bus see about `ΔV ≈ I · R / 2`:
   1.1 × 0.115 / 2 ≈ 0.06 V.
4. **Effect on the load** — translate the drop into what the circuit feels.
   For resistor-ballasted LED strings, `ΔI = ΔV / R_string` ≈ 0.06 / 180 Ω ≈
   0.33 mA (1.7 %, invisible). The 0.34 mm "thermal" width would drop about
   0.28 V (about 8 %, a visible brightness gradient).
5. **Series segments** — every segment in series with the bus must be at
   least as wide as the bus it feeds. A 1.0 mm, 85 mm segment carrying the
   whole board's 1.93 A caused 81 mV, 56 % of all copper drop on that rail.
6. **Proportion** — compare the copper error with the other error terms
   before widening anything. On the reference board copper caused 1.2 % of
   brightness spread while the LED forward-voltage bin caused 64 %.

When the drop matters, solve the copper network instead of multiplying
length by current: split tracks at junctions, use about 0.49 mΩ per square
for 1 oz copper and about 1 mΩ per via, and iterate voltage-dependent loads
to convergence. Lumped estimates on the reference board overstated the drop
four to five times.

### Enforcing the widths

- A netclass width is the routing default, not a DRC minimum; KiCad enforces
  only the board-wide minimum track width. Enforce each class with a custom
  rule — for example `(rule "Supply width" (condition "A.NetClass == 'Supply'")
  (constraint track_width (min 0.4mm)))` — or audit the minimum width of every
  net against its class before release.
- An accepted exception needs a capacity number and a reason: when a
  connector's alignment hole forced a 0.15 mm branch, IPC-2221 gave 0.60 A at
  ΔT = 10 °C against a 0.5 A load, and the warning was recorded as accepted.
  Do not force a width that creates a worse clearance violation.

## Ordinary signals

For an ordinary, non-impedance-controlled signal, choose a width and clearance
that the selected process can fabricate reliably and the available geometry can
route. Keep one project netclass as the source of truth. A prose default is only
a candidate until it is written to the project and passes DRC.

## Controlled impedance

Obtain the actual stackup before choosing geometry. An external microstrip and
an internal stripline have different fields; an internal conductor is not a
microstrip. A differential pair additionally depends on spacing, reference
planes, copper thickness, dielectric properties, solder mask, and the
fabricator's impedance-control process.

Use a field solver or the fabricator's stackup calculator. Record:

- target single-ended or differential impedance and tolerance;
- layer and reference plane(s);
- dielectric thickness and material assumptions;
- copper thickness, finished trace width, and etch assumptions;
- pair spacing and solder-mask treatment; and
- solver/tool version and result.

Apply the solved width and gap to the project netclass. Re-solve whenever the
stackup or fabricator changes, and verify the ordered impedance service matches
the calculation.

## Vias

Choose via drill and finished diameter from the selected fabricator's current
capability, required annular ring, board thickness/aspect ratio, current, and
reliability target. Power and thermal paths may require parallel vias; justify
their count with electrical/thermal evidence rather than a fixed lookup table.

Use `set_predefined_sizes` to record accepted via choices and
`get_predefined_sizes` to verify the stored palette before routing.

Practical rules from a production board:

- A single 0.3 mm via barrel with typical plating has about the copper
  cross-section of a 0.67 mm, 1 oz trace, so capacity is rarely the limit.
  Redundancy is: a single via is a single point of failure for the rail it
  carries. The reference project used at least two vias wherever a supply of
  more than about 0.5 A changed layer, and counted real duplicates per
  transition point when auditing (vias within about 2 mm of each other).
- Size power-class vias so that the ring stays adequate whichever drill a tool
  applies: KiCad's Specctra session importer assigns the netclass drill, not
  the padstack drill (0.6/0.3 vias arrived as 0.6/0.4 with a 0.1 mm ring).
  0.8/0.4 mm vias on power classes kept a 0.2 mm ring either way.

## Acceptance record

For every non-default netclass, preserve the sizing purpose, governing inputs,
calculation or current contract, selected values, and DRC result. A required
input that is unavailable makes the sizing decision `INCOMPLETE`.
