# Constraint Record Schema — `constraints.md`

The constraint record is what every later phase reads instead of the
conversation: the architecture sizes against it, the layout takes its
connector sides and outline from it, the manufacture phase takes the service
and quantity from it. It is written once, in the `requirements` phase, and
leaves that phase in the same `flow_advance` call that moves the job to
`architecture`. The section headings are in the kicad-architecture skill's
"`constraints.md` — required sections"; this file defines the rows.

## 1. Row format

The Constraints section is one table, one row per field of §2:

| Field | Value | Source | Evidence |
|---|---|---|---|

Every value names exactly one source:

| Source | Means | Evidence to cite |
|---|---|---|
| `user` | The user stated it, or confirmed an assumption put to them | The user's words, quoted, and the question round they answered |
| `datasheet` | A manufacturer document fixes it: a part datasheet, the manual of the equipment the board connects to, an enclosure or mating-connector drawing | Document, revision, page, table or figure |
| `config` | The effective configuration fixes it | The `get_effective_config` key, such as `sourcing.derating.capacitor.voltage` |
| `decision` | An obvious engineering call, made and logged | The `flow_log` decision entry: its message, why and rollback |

A value without a source is not a value. If it decides nothing, it goes to
Open questions; if it decides a block, a part, the power budget or the
outline, it is a question for the single BLOCKED round.

An assumption becomes `user` only when the user confirmed it. An unconfirmed
assumption that is the obvious call is a `decision` and is logged; one that
touches product intent, cost or appearance is a question.

## 2. Fields

| Field | Record | Usual source | Decides |
|---|---|---|---|
| Environment | Indoor or outdoor; ambient minimum and maximum inside the enclosure; sun; humidity or condensation; vibration | `user`, `decision` | Every derating class in use, temperature grades |
| Power | Each input: source and connector, nominal voltage, range including transients and surge, reverse-polarity and hot-plug exposure, battery chemistry if any | `user`, `datasheet` of the supplying equipment | Input protection, converter and regulator choice |
| Currents | Per load and per rail, typical and peak; what each source can deliver | `datasheet` of the loads and the source, `decision` | Power budget, copper, connector ratings |
| Interfaces | Each external signal interface: standard, speed, cable length, the equipment at the far end, direction | `user`, `datasheet` of the far-end equipment | Transceivers, protection, pin plan |
| Connectors and sides | Each connector: part or family, board edge and side, orientation, mating cable or part, keying | `user`, `datasheet` of the mating part | Placement, mechanical support |
| Size | Outline limits, component height per side, mounting holes and their pitch | `user`, `datasheet` of the enclosure | Placement density, layer count |
| Enclosure | Material, sealing, thermal path, cut-outs | `user` | Thermal derating, connector placement |
| Fab/service | Fabricator, service tier, layer count, thickness, finish, assembly sides and tier | `config` (the fabrication constraints), `user` | Design rules, part library class; for JLCPCB, the kicad-manufacture skill's `references/jlcpcb-rules.md` |
| Quantity | Boards per order, prototype or production, attrition allowance | `user` | Lot ceiling, cost, review depth |
| Cost | Target and ceiling per board (parts, fabrication, assembly), currency | `user` | Part choices, every cost trade-off |

Add a row when the request carries a constraint outside these fields
(certification, a regulatory limit, a firmware platform the user already
chose); never drop one of the ten. A field that does not apply reads "n/a"
with a `decision` source saying why.

## 3. The exit test

The phase is done when no field that decides the design is missing. A field
decides the design when a different value would change a block, a part, the
power budget or the outline. Every missing deciding field becomes one
question in the BLOCKED round, stated with the options, their numbers, and a
recommended option. A field that decides nothing (mask colour with no
appearance requirement) becomes a logged `decision`.

In a revision job, the previous `constraints.md` is the baseline: the Scope
section names it, and each changed row keeps its old value in the Evidence
column (`was 12 V; changed by the user on …`).

## 4. Example

```
# Constraints — USB-serial converter with ESP32

## Scope

A USB-C powered USB-serial converter built around an ESP32 module.
Lane: new_board. Baseline: none.

## Constraints

| Field | Value | Source | Evidence |
|---|---|---|---|
| Environment | Indoor, 0–40 °C ambient inside the enclosure | user | "it lives in a box on the bench" (question round 1) |
| Power | USB-C sink, 5 V ±5 %, no other input | user | question round 1 |
| Currents | 500 mA total budget from the host | decision | log: "USB 2.0 default budget; no PD negotiation" |
| Interfaces | USB 2.0 full speed to the host; 3.3 V UART to the target, 115200 baud, 30 cm cable | user | question round 1 |
| Connectors and sides | USB-C on the short edge; 1×6 2.54 mm header on the opposite edge | user | question round 1 |
| Size | 50 × 25 mm maximum; 8 mm component height on top | datasheet | enclosure drawing rev B, page 2 |
| Enclosure | ABS, unsealed, no forced airflow | user | question round 1 |
| Fab/service | JLCPCB, 2 layers, 1.6 mm, HASL, top-side economic assembly | config | fabrication constraints; assembly tier by the user |
| Quantity | 10 boards, prototype | user | question round 1 |
| Cost | Target 8 USD per assembled board, ceiling 12 USD | user | question round 1 |

## Decisions

- Default USB power budget of 500 mA, no PD controller (logged).

## Open questions

none
```
