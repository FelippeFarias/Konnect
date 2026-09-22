# Architecture Record Schema — `architecture.md`, `worst-case.md`, `pin-plan.md`

The three records of the `architecture` phase leave it together, in the one
`flow_advance` call that moves the job to `gate:architecture`. With
`constraints.md` they are the package the human approves there, and the
approval binds to their exact bytes: editing any of them afterwards revokes
it. The section headings are in the kicad-architecture skill's "Required
sections"; this file defines what goes under each.

## 1. `architecture.md`

### Blocks

One row per functional block, each traced to the constraint row that needs it:

| Block | Function | Key part | Inputs | Outputs | Rails | Constraint |
|---|---|---|---|---|---|---|

A one-line text diagram above the table is welcome
(`USB-C → ESD → 5 V → LDO 3.3 V → ESP32 module → UART header`); the table is
the record.

### Power tree and budget

One row per rail, from the input to the last load:

| Rail | Source | Converter or regulator | Voltage and tolerance | Consumers (typical / maximum) | Total (typical / maximum) | Dissipation at the worst corner | Derating limit and margin | Record |
|---|---|---|---|---|---|---|---|---|

- The input row carries the drop of every series element (protection diode,
  fuse, PTC) at the maximum current, so each downstream rail starts from the
  effective voltage.
- The Record column names the `worst-case.md` record (`WC-<n>`) that sized
  the rail's converter or regulator. A rail without one is an invented value.

### Interfaces

One row per external connector:

| Connector | Function | Pinout | Board edge and side | Mating part or cable | Protection | Source |
|---|---|---|---|---|---|---|

The method (termination, fail-safe, protection against the transceiver's
absolute maximum, pinout against the equipment it mates with) is in the
kicad-schematic skill's `references/interface-design.md`.

### Parts list

This is the section `kicad-sourcing-agent` fills; its handoff returns rows in
exactly this shape, and the architecture record copies them.

| Ref / block | Function | Manufacturer part number | Package | Distributor code | Stock (quantity, date, source) | Qty per board | Lot ceiling | Datasheet | AVL | Derating | Library |
|---|---|---|---|---|---|---|---|---|---|---|---|

- **Manufacturer part number**: the full orderable suffix. Suffixes change
  pinouts inside one package and split tube from reel stock; the kicad-review
  skill's `references/datasheet-audit.md` §7 treats symbol, Value, part
  number, footprint and datasheet as one part identity.
- **Stock**: quantity, retrieval date and source. For JLCPCB, take it from
  the fabricator's current part page or cart, and record the assembly tag and
  library class there too — the kicad-manufacture skill's
  `references/jlcpcb-rules.md` §2; a downloaded catalogue is discovery, not
  stock.
- **Lot ceiling**: stock ÷ quantity per board, rounded down. Flag any line
  below the order quantity plus attrition from `constraints.md` (the
  kicad-manufacture skill, §2b "Lot ceiling and bins").
- **Datasheet**: the manufacturer's copy of that exact part, validated as a
  real PDF (`datasheet-audit.md` §1); cite the document and revision.
- **AVL**: `on AVL`, `not on AVL`, or `not enforced` when the effective
  `sourcing.avl` list is empty. An empty AVL is never a pass.
- **Derating**: the class limit from `sourcing.derating` against the
  utilization in the matching worst-case record — `ok (62 % of 80 %)`,
  `over`, or `n/a` with the reason.
- **Library**: `found` with the symbol and footprint IDs that
  `search_symbols` and `search_footprints` returned, or `needs library` — the
  library agent makes that part inside the `schematic` phase, before the build
  that places it.

A row with no confirmed source (no stocked part meets the requirement, or the
datasheet could not be obtained) keeps the readiness line BLOCKED.

Example row — the volatile fields are shown as what to write, never as values
to copy:

| Ref / block | Function | Manufacturer part number | Package | Distributor code | Stock (quantity, date, source) | Qty per board | Lot ceiling | Datasheet | AVL | Derating | Library |
|---|---|---|---|---|---|---|---|---|---|---|---|
| C1 / input | VBUS bulk, 10 µF 25 V X5R | GRM21BR61E106KA73L | 0805 | the fabricator's code from its part page | quantity on the part page, retrieval date, "fabricator part page" | 1 | stock ÷ 1 | Murata datasheet, revision cited | not enforced | ok (21 % of 80 % voltage: 5.25 V on 25 V) | found: `Device:C`, Capacitor_SMD:C_0805_2012Metric |

### Open questions

One line per question: the question, the block or value it blocks, who
answers it. An open question that changes a block keeps the readiness line
BLOCKED; one that changes nothing before fabrication may stay, marked so.

### The readiness line

The last non-empty line of the file, trimmed, is exactly `Readiness: PASS`,
or starts with `Readiness: BLOCKED` followed by a reason naming the missing
or invented value. Only blank lines may follow it.

- `Readiness: PASS` — every value a block depends on traces to a datasheet, a
  measurement, a configuration key, or an assumption the user confirmed, and
  every parts-list row has a confirmed source.
- `Readiness: BLOCKED — <value and what would supply it>` — anything else. A
  bare `Readiness: BLOCKED`, `Readiness: PASSED`, or text after `PASS` is
  malformed, and a malformed line is treated like BLOCKED.

`flow_advance` refuses to leave `architecture` unless this line reads
`Readiness: PASS`; `flow_gate` re-checks it before approving the
architecture gate, because the file may have been edited since.

## 2. `worst-case.md`

One record per decisive value: every value whose wrong choice breaks a
requirement at some corner — regulator dissipation, LED and string
resistors, input protection and fuse or PTC ratings, bulk capacitance under
DC bias, bus pull-ups, anything thermal.

- Each record is a table with the seven fields of the kicad-schematic skill's
  `references/design-calculations.md` §1, in that table's order. The method
  for each circuit class is in that file's §2–§6: cite the section, do not
  re-derive the method here.
- Heading `## WC-<n> — <the value decided>`. An ID is never reused, across FIX
  rounds included, because the power tree, the parts list and later ledgers
  cite it.
- A part substitution invalidates every record that used the old part; redo
  them before the readiness line can read PASS again.

```
## WC-1 — U2 LDO dissipation

| Field | Content |
|---|---|
| Requirement | … |
| Formula | … |
| Inputs and sources | … |
| Corners | … |
| Result | … |
| Criterion and margin | … |
| Decision | … |
```

## 3. `pin-plan.md`

One table per programmable part (MCU, module, logic device), every pin
included — an unused pin gets the termination its datasheet requires:

| Pin | Function | Net | Constraint | Source |
|---|---|---|---|---|

- **Pin**: the number and name the part's datasheet gives (a module's pad
  number, not the die's).
- **Function**: the pin's role in this design.
- **Net**: the net name the schematic will use.
- **Constraint**: what limits the choice — strapping level at reset,
  reserved for flash, input only, ADC or touch channel in use, voltage
  tolerance, power-up state.
- **Source**: document, section or table.

Example rows (`## U1 — ESP32-WROOM-32E`):

| Pin | Function | Net | Constraint | Source |
|---|---|---|---|---|
| 3 · EN | Chip enable, RC-delayed | ESP_EN | Must rise after the 3.3 V rail is stable | Module datasheet, pin definitions and power-up timing |
| 25 · IO0 | Boot mode | ESP_BOOT | Strapping: low at reset enters download mode | Module datasheet, strapping pins |
| 14 · IO12 | Spare | — | Strapping: must be low at reset with 3.3 V flash | Module datasheet, strapping pins |
| 34 · RXD0 | UART from the bridge | UART_TX_BRIDGE | Bootloader UART | Module datasheet, pin definitions |
