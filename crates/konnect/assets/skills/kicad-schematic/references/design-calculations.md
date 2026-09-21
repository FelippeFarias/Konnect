# Design Calculations — Worst-Case Sizing Records

A value is not accepted because it works at "typical". It is accepted when a
written record shows it holds at **both corners** of every input that varies
— supply, part tolerance and bin, temperature, number of devices on a bus —
and states the margin used. On the reference project (a 12 V outdoor LED
display with 198 LEDs) the string resistor went through four values
(180 → 150 → 200 → 300 Ω), and three blocking power defects surfaced a day
after the schematic was called "verified", all because values had been
chosen at nominal. ERC and a netlist check prove connectivity; only these
records prove the values.

## 1. The record

| Field | Content |
|---|---|
| Requirement | What must hold (brightness, no nuisance trip, fail-safe level) |
| Formula | The equation used |
| Inputs and sources | Datasheet document, page and figure; distributor field; measurement |
| Corners | Which inputs vary, their min and max, which corner is worst for which criterion |
| Result | Values at each corner |
| Criterion and margin | Limit, derating policy, margin left |
| Decision | Value chosen, and what would invalidate it (another bin, enclosure, bus topology) |

Rules that apply to every record:

- Read the effective configuration before choosing parts
  (`get_effective_config`): derating classes (resistor power, LED current,
  capacitor voltage), default passive sizes, naming. Applying it after the
  build forced part changes.
- Open the manufacturer datasheet of every IC and every power, protection, or
  interface passive before fixing its value. Values chosen from habit (a
  33 Ω series resistor, a 680 Ω bias pair, an AMS1117 behind a diode) were
  all overturned later.
- When a datasheet omits a value the design depends on (a typical forward
  voltage), close the design at the unfavourable end of the range the
  supplier will ship. Read decisive curves numerically, not by eye.
- **A part substitution resets every derived number.** Rebuild a parameter
  sheet from the new part's datasheet (forward-voltage range and bins,
  dynamic resistance, maximum current and derating slope, temperature range,
  packaging) before quoting anything. After an LED change the old part's
  typical Vf, derating slope, and "reel" packaging kept being quoted; the new
  part had no typical Vf, a 25 % steeper derating, and shipped in bags.
- Check that a part meeting a requirement exists and is stocked before
  writing the requirement into the design. A self-derived "Vf ≤ 2.1 V" was
  written into 198 symbols; no stocked LED met it.

## 2. LED strings on a fixed rail with a current sink

Topology: `rail → n LEDs → R → sink` (an open-drain driver output).

```
I = (V_rail,eff − n·Vf − I·R_sink) / R
V_rail,eff = V_supply − V_series-protection(I_total) − V_distribution
```

- Model a DMOS sink by its on-resistance (TPIC6B595: 4.2 Ω typical, 5.7 Ω
  maximum, about 9.5 Ω hot → 0.08–0.19 V at 20 mA), not a fixed 0.3–1 V.
- Use the effective rail. A reverse-polarity Schottky (≈0.45 V at 1–2 A) and
  a PTC took a 12 V input to about 11.4 V.
- **Low-Vf corner, high supply → maximum current** must stay at or below the
  LED's forward-current limit **derated at the maximum internal ambient**.
  **High-Vf corner, low supply → minimum current** must still meet the
  brightness requirement.
- Headroom decides sensitivity: ΔI/I ≈ ΔV/(V_rail − n·Vf − V_sink). Four red
  LEDs on 12 V leave about 3.4 V across R, so the Vf bin alone swung current
  from 22.6 to 13.9 mA (−39 %). With a 1.8–2.2 V bin and a 20 mA limit, the
  resistor had to be ≥ 225 Ω for one corner and ≤ 135 Ω for the other: **no
  single value works**. When the window is empty, change the system, not the
  resistor: fewer LEDs per string, a regulated LED rail, a Vf-ranked single
  lot, or a constant-current sink with the same serial interface
  (TLC5916/TLC5926 class). Present these alternatives at architecture time.
- Colour limits the series count: n·Vf(max) must fit under the effective
  rail with headroom. Four in series on 12 V only works for low-Vf colours.
  Write that constraint on the sheet.
- Derating example from a digitised datasheet curve: 20 mA flat to 25 °C,
  then −0.265 mA/°C (16.0 mA at 40 °C, 10.7 mA at 60 °C). With 300 Ω:
  Vf 1.8 V → 15.8 mA (allowed up to 41 °C); Vf 2.2 V → 10.5 mA. Answering
  "can we use 220 Ω for more light?": 17.8 mA nominal, a limit of 33 °C
  ambient, and 21.4 mA on a low-Vf reel — above the absolute maximum at
  25 °C — for +36 % light. Cooling the enclosure buys more usable light: each
  10 °C removed frees 2.65 mA.
- Pulsed operation allowed by the datasheet (for example 100 mA peak at 1/10
  duty) makes PWM dimming the thermal lever; when the thermal budget needs
  it, record it as a firmware requirement.
- Strings of different length on one rail (a 1-LED decimal point next to
  4-LED segments) change brightness ratio with Vf, and the spread of that
  ratio does not depend on the resistor values. Size the long strings first,
  then choose the short string's resistor by minimax over the whole Vf range.
- Indicator LEDs far below their test current: solve I = (V − Vf(I))/R
  together with the diode curve (Vf falls by about n·Vt·ln(I/I_test)); a
  fixed Vf from the test current badly underestimates the current.
- Compare LEDs by intensity inside the viewing cone the product needs, not by
  on-axis millicandela alone: a 2000 mcd 30° part over a 400 mcd 45° part is
  about 2.2× the light in its cone, not 5×, and loses off-axis readability.
- Resistor power at the maximum-current corner against the derating policy.
  A single-LED string drops most of the rail in its resistor and is usually
  the hottest part of its sheet.
- Driver: per-output current, package dissipation with every output on
  (Σ I²·R_DS(on),max + I_CC·V_CC), and V_DS(max) against the rail. Calculate
  before adding copper: the drivers here dissipated 20 mW each, 3 % of their
  budget.

## 3. Supply budget and input protection

- **Budget** every consumer at its worst corner: all loads on at the
  high-current corner, logic referred through regulator efficiency, and every
  auxiliary output at its fuse rating. The first budget here omitted the
  regulator input and an auxiliary 12 V output and said 1.1 A; the real worst
  case was 2.10 A.
- **Input chain order**: connector → fuse or PTC → TVS → series
  reverse-polarity diode (or ideal-diode MOSFET) → bulk capacitor. With the
  TVS after the diode, a reverse surge falls entirely on the diode.
- **Protection matrix**: for every port and every threat (surge, reverse
  polarity, ESD, miswiring), name what clamps and what limits, before and
  after any change. A connector pin that exports a rail is a surge entry
  point too. Moving the input TVS ahead of the diode left the rail with no
  clamp at all, while one connector exported +12 V on a field cable.
- **PTC hold current** comes from the maker's rerating table at the hottest
  local temperature, with margin over the worst load: a 3 A 2920 part holds
  about 2.0 A at 60 °C (below a 2.10 A load, so it trips in summer); holding
  4.5 A at 60 °C takes a 6 A part. The trip current derates too, so check
  what the supply and wiring must survive. Keep hot parts away from the PTC:
  a diode dissipating 0.8 W 8 mm away makes it trip earlier. Oversizing only
  raises the energy a fault can dump.
- **PTC voltage**: V_max must exceed the largest voltage that can appear
  across the part once it has tripped — the maximum source voltage with the
  downstream side shorted, or a source overvoltage minus the downstream
  clamp. Requiring V_max above the downstream TVS clamp voltage is a
  conservative shortcut. Common 1206 PPTCs are 6 V parts; specify V_max in the
  BOM.
- **PTC resistance in signal lines**: use R1max (post-trip, post-reflow), not
  the initial resistance, in every series budget.
- **Series Schottky**: Vf at the worst current (it lowers the rail for every
  other calculation) and its dissipation against the copper the datasheet's
  θJA assumes. An SS34 at 0.8–1.0 W on 17–24 mm² of copper was estimated at
  a 139 °C junction at 60 °C ambient (θJA scaled for the small copper area);
  an ideal-diode P-MOSFET would dissipate about 0.05 W and return 0.45 V of
  LED headroom.
- **TVS**: stand-off above the maximum operating voltage; clamping voltage at
  the relevant surge current below the absolute maximum of what it protects —
  or add series resistance between the TVS and the IC (see
  `interface-design.md`).
- **Entry capacitance**: every entry net — connector, diode-OR node,
  regulator input, USB VBUS — gets local bulk plus a high-frequency
  capacitor. A long board gets bulk distributed along the rail; a USB device
  needs 1–10 µF on VBUS.
- Add a part only to correct a measured defect, and prefer reusing a part
  number already on the BOM (a second TVS reused the first one's code, so the
  BOM gained no line). Spend the cents where a part's only job is to survive
  a fault.

## 4. Regulators

- **Switching regulators**: redo the datasheet equations with the real input
  and tolerances — ripple, peak current against the inductor's worst-column
  saturation (above the IC's maximum current limit), effective input and
  output capacitance, minimum on-time, dissipation from the efficiency curve
  of this exact part. The full checklist is in the `kicad-review` skill's
  `references/datasheet-audit.md`.
- **LDO headroom along the whole chain**: V_in(min) − V_diode(at load) −
  V_dropout(max, at peak current) ≥ V_out + margin, on every supply path.
  An AMS1117 fed from 5 V through a Schottky gives 5.0 − 0.45 − 1.3 = 3.25 V
  at the Wi-Fi transmit peak — out of regulation; from USB it gives 2.70 V, a
  brown-out. Use the guaranteed dropout point; label any interpolation as
  derived.
- A reused symbol does not make the part right: the symbol may stay
  AMS1117-compatible while the Value and BOM name the part actually ordered,
  with its full suffix (SOT-223 regulators exist in pin-swapped variants).
- **Tab copper**: compare the copper connected to a regulator or diode tab
  with the area its θJA assumes (17.9 mm² against 645 mm² led to an estimated
  θJA ≈ 150 °C/W and a 157 °C junction).

## 5. Capacitors

- MLCC capacitance collapses with DC bias. Examples from a maker's curves:
  22 µF 25 V X5R 1206 keeps 13.6 µF at 5 V (−38 %) and 5.7 µF at 12 V
  (−74 %); 10 µF 25 V X5R 1206 keeps 3.5 µF at 12 V. A regulator asking for
  "10 µF input" means effective capacitance. When a small maker publishes no
  curve, use a major maker's part of the same size, voltage, and dielectric
  as an optimistic bound, and say so.
- Rate by the voltage across the part: a bootstrap capacitor sees
  V(BST) − V(SW), not V_in plus the gate drive.
- Put the rating in the Value (`22uF 25V`): it is the column the assembler
  reads, and BOM grouping by Value and footprint must not merge parts of
  different ratings. One BOM line once held 10 V parts on 5 V and unrated
  parts on the 12 V rail.
- Consolidate: one part per value and case at the highest needed rating (all
  22 µF 1206 at 25 V, all 100 nF 0805 at 50 V). This removed eight BOM lines
  and the wrong-voltage risk at once.
- Electrolytic life roughly doubles per 10 °C below the rating:
  L ≈ L0·2^((T_rated − T_internal)/10). A 2000 h / 105 °C part gives about
  11 years at 49 °C but 2.7 years at 69 °C. Keep one bulk electrolytic on an
  all-ceramic input as the hot-plug damper (check 2·√(L/C) against the
  loop resistance), and record its height for the enclosure.

## 6. Thermal and environment

- Capture the environment first: indoor or outdoor, maximum ambient, solar
  exposure, enclosure material, colour, and sealing. It decides every
  derating.
- Internal rise of a sealed enclosure from the board's own dissipation:
  `ΔT ≈ P_total · θ_enclosure`. With 9.26 W and an assumed 3 °C/W for a
  sealed plastic box, the estimate was +28 °C.
- Outdoors, add solar gain `Q ≈ α · G · A_face` with G ≈ 1000 W/m²: a
  450 × 150 mm face absorbs about 61 W when dark (α ≈ 0.9) and about 20 W when
  light (α ≈ 0.3). The same project estimated that colour was worth about
  20 °C — which implies a far lower θ (about 0.5 °C/W) than the 3 °C/W used
  for internal dissipation. Two inconsistent models circulated and both
  looked authoritative.
- So: state the model, its θ, and its source for every thermal number; do not
  mix models in one estimate; treat the result as a bracket, not a value; and
  make every decision that rests on it (LED derating, capacitor life, module
  grade) conditional on a thermocouple measurement of the first unit in its
  enclosure.
- Check every temperature grade against the internal ambient. The
  ESP32-S3-WROOM-1 variants with octal PSRAM are rated to 65 °C ambient;
  others reach 85 °C or 105 °C. Treat a module variant swap as a firmware
  change too (PSRAM mode, flash size, partition table).
