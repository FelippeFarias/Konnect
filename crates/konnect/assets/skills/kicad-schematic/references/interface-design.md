# Interfaces, Logic Levels, and Power-Up States

Wired interfaces and power-up behaviour produced the defects that were most
expensive to find on the reference project, because ERC, DRC, and a netlist
check cannot see them. Size every item here with a written record (see
`design-calculations.md`) from the exact datasheets.

## 1. RS-485 node

- **Fail-safe bias** must keep the idle differential above the receiver
  threshold (±200 mV for a standard receiver) with the real bus:
  `V_AB = V_CC · R_T,eff / (R_pu + R_pd + R_T,eff)`, where `R_T,eff` is every
  termination in parallel (60 Ω for 120 Ω at both ends). A 680 Ω pair at
  3.3 V gave 268 mV with one termination and 139 mV with two.
- Include resistor tolerance and the minimum supply: 390 Ω at 3.3 V gave
  236 mV nominal but 200.7 mV worst case with 1 % parts, and failed with 5 %.
  Cross-check with a vendor formula (for example TI SLLA272 eq. 2,
  `R_B = V_bus,min / (V_AB · (1/375 + 4/Z0))`) after reproducing that
  document's own worked example.
- **Driver load** stays at or above 54 Ω (the V_OD test condition) with the
  bias network in parallel: 55.1–58.9 Ω with two terminations — a 2 % margin
  that a third terminator installed in the field breaks.
- **Bias-resistor power at the common-mode extremes** (−7 V / +12 V): a 390 Ω
  0805 dissipates about 0.37 W at +12 V against a 0.125 W rating, and the
  bias current counts against the unit-load budget. Lowering the bias
  resistance to fix fail-safe creates this defect.
- **Is a second termination needed at all?** Compare the bit time with the
  line's round trip: at 9600 bps over 50 m reflections settle in 1.4 % of the
  bit. Shipping the termination jumper open and terminating only the far end
  doubled both margins at zero cost. Document the default.
- **Protection**: TVS at the connector; compare its clamping voltage at the
  relevant surge current with the transceiver's absolute maximum. An SM712
  clamps at about 19 V at 1 A against ±15 V on the bus pins, so 10 Ω surge
  resistors go between the TVS and the IC (order: connector → PTC → TVS →
  R → IC). Re-check V_OD with the added resistance (2.30 V at the connector
  with one termination). Series PTCs are acceptable when their R1max is well
  below the 10–20 Ω that application notes allow.
- **Enables and receiver output**: tie DE and /RE with a pull-down so the
  node listens at boot, and pull RO up, because RO floats while the node
  transmits. Use non-strapping MCU pins.
- **Cable**: 120 Ω terminations match twisted pair only; specify it for the
  installation. A resistor in series with the cable ground limits
  ground-loop current by letting the offset appear as common-mode voltage —
  it raises bias-resistor stress, it does not relieve it.

## 2. RS-232 receive-only port

One MAX3232-class receiver, unused driver inputs tied to a defined level,
a bidirectional TVS and a PTC on the line. Check the TVS stand-off against
the standard's worst-case line voltage.

## 3. USB-C device (sink) with an MCU's native USB

- One 5.1 kΩ Rd resistor per CC pin, each to GND — never one shared
  resistor — and no Rp.
- D+/D− straight through the ESD array (a flow-through part joins pins 1–6
  and 3–4; verify on the rendered pin figure, not a text mirror).
- VBUS on every VBUS pin, 1–10 µF of bulk, and the ESD array's own 100 nF
  right beside it: a 4.7 µF 0805 self-resonates at 2–3 MHz and is inductive
  across the ESD spectrum. Reserve its place before routing.
- Shield to GND through 1 MΩ ∥ 4.7 nF (check the capacitor's voltage rating
  for ESD). No 22 pF on D±: full-speed USB has no capacitance budget for it.
- An MCU full-speed PHY (12 Mbps) needs no length matching; a few
  millimetres of skew are irrelevant.

## 4. Logic levels and power domains

- Use the receiving part's own thresholds, never generic 0.3/0.7·V_CC.
  TPIC6B595 inputs need V_IH ≥ 0.85·V_CC (4.25 V at 5 V) and V_IL ≤ 0.15·V_CC
  (0.75 V). A 3.3 V MCU cannot drive them.
- A ratiometric threshold is only met by a buffer on the **same rail**. A
  TTL-threshold family (AHCT, V_IH = 2.0 V) accepts 3.3 V inputs; an AHC
  (V_IH = 0.7·V_CC) does not. Compute the guaranteed margins from datasheet
  minimum and maximum values (AHCT V_OH ≥ 4.40 V against 3.825 V: +575 mV).
- **Build a power-domain matrix**: which rails are alive in each supply mode
  (main only, USB only, both, and the sequencing windows). For every pin that
  crosses domains check input clamp diodes to V_CC, tolerance with V_CC = 0,
  and I_off. AHCT inputs have no clamp to V_CC; HCT inputs do. A stock-driven
  swap to HCT injected 72.7 mA per input into the dead 5 V rail during USB
  programming.
- Do not load daisy-chain data outputs: a shift register's SER OUT meets
  V_OH only at microamp loads.

## 5. Long multi-drop lines on a large board

- Series-terminate at the source, within a few millimetres of the driver,
  with `R_s ≈ Z0 − R_out`, and check the far-end overshoot
  (≈ 2·V·Z0/(Z0 + R_s + R_drv)) against the receivers' absolute maximum. On
  427–448 mm clock lines with seven loads, 33 Ω gave 6.3–6.9 V against a 7 V
  maximum; 68 Ω fixed it and softened the edges.
- With source termination, loads near the source see a half-amplitude step
  until the reflection returns; check it against V_IH/V_IL and keep the clock
  rate modest.
- Estimate the node capacitance (trace, vias, inputs) against the driver's
  load rating and compute the RC edge: 68 Ω × 65 pF gave a ceiling near
  10 MHz, against a need of 1 MHz (56 bits per frame at 17.8 kHz refresh).
- Route the clock so it reaches the last device of the data chain first;
  clock and data flowing the same way is the unfavourable direction for hold
  time.

## 6. Power-up and default states

- Every enable, output-enable, reset, and strap line needs a defined level
  from the first microsecond, from a source that is powered at that moment.
  Walk the real ramp order (who powers whom, soft-start, regulator start-up).
- A pull-up on a rail that rises **later** than the logic it controls is not a
  default. On the reference board +5 V rose before +3.3 V, the /OE pull-up to
  3.3 V was unpowered, the 5 V buffer drove the drivers' /G low, and the LED
  drivers enabled with random latch contents.
- A resistor cannot override an actively driven output: 10 kΩ against a
  push-pull buffer through 68 Ω holds the node at 5 × 68/10068 ≈ 34 mV. Put
  the buffer's own OE on a pull-up to its V_CC (Hi-Z during power-up), or use
  a supervisor or transistor gate.
- A clear pin tied permanently inactive removes the hardware clear the vendor
  recommends; give it a pull-up plus an MCU pin, or record the firmware
  sequence.
- MCU pins glitch at reset differently. The ESP32-S3 datasheet has a table of
  pins with power-up glitches (a 60 µs low pulse on GPIO17 enabled every LED
  driver on the reference board). Read it by pad name as well as GPIO number:
  it lists GPIO15/16 under their 32 kHz crystal names, so a verifier once
  wrongly called them glitch-free; GPIO21 and GPIO38–48 are not in it. Put
  output enables on pins the table does not list.
- Size the glitch window from the real sequence before deciding between fix
  and accept: an LDO fed from the 5 V rail tracked a 4 ms soft-start, which
  limited the exposure to about 2 ms of weak flicker — accepted and documented.

## 7. MCU pin planning

- Keep interchangeable assignments provisional until the floorplan exists:
  GPIO functions routed through a pin matrix, shift-register outputs mapped
  to segments by firmware, and the position of a series part inside a string.
  List them in the layout handoff as swappable; the layout will match them to
  geometry.
- ESP32-S3-WROOM-1 rules used: GPIO35–37 unused on octal-PSRAM variants;
  strapping pins GPIO0, 3, 45, 46 treated deliberately; native USB on
  GPIO19/20; EN with 10 kΩ / 1 µF at the module; a test point on the UART
  pins for field debugging.
- Keep one GPIO map note on the sheet and update it in the same change as
  every swap; a stale note is a review finding.

## 8. Field connectors that replace or mate with legacy equipment

- Take pin numbering from the reference's cable tables or a meter on a real
  unit. A drawing's physical sequence is not its numbering: a retrofit would
  have put a scale's RS-232 TX on the board's 12 V output.
- Prefer a pinout where the worst miswiring means "no data", not "damage".
  Drop power outputs the requirements do not ask for; every extra pin adds
  protection, thermal, and BOM obligations.
- One legend per pin on the silkscreen, beside the pin.

## 9. The hardware-to-firmware contract

Write down every behaviour the hardware relies on and ship it with the
design (README or fabrication notes), as requirements, not suggestions:
clear the shift registers and latch before enabling outputs; initialise
GPIO levels at the start of the application; PWM derating when the thermal
budget requires it; software debounce; jumper defaults; module variant
settings (PSRAM mode, flash size).
