# Orchestrating a Pre-Fabrication Review

A single pass by the engineer who made the design does not verify it. On the
reference project, the board passed ERC and DRC and was declared ready five
times; each independent round then found real defects, and the round that
re-measured the previous round's fixes found the worst ones. This reference
is the review structure that finally closed it. Use it for a full or
pre-fabrication review, and use its regression phase after every batch of
fixes. When subagents are available, run the dimensions in parallel; a
single reviewer follows the same phases sequentially.

## Phase 1 — Freeze and inventory

1. Save. Review a frozen copy of the project (schematics, board, project
   file, custom rules, library tables and project libraries), never the live
   files while KiCad or another agent is editing them.
2. Export the netlist from the saved schematic, and DRC/ERC results with every
   severity and schematic parity. Audit the rule configuration first
   (`verification-traps.md` §2); on the copy, switch on disabled checks.
3. Record the baseline: every accepted warning with its justification.
4. Build the part-identity table: symbol, Value, manufacturer part number and
   suffix, distributor code, footprint, archived datasheet (validated as a
   real PDF of that exact part).
5. Record the inputs the design depends on, from files or the user: the order
   recipe (fabricator, assembly tier, panel and rails, V-cut or routed edges,
   mask colour), the current budget, the operating environment (ambient
   maximum, enclosure, sun), and firmware assumptions.
6. List what changed since the last review.

## Phase 2 — Deterministic checks

Run the checks that need no judgement and record their numbers: single-pin
nets, multiple drivers on one net, power and ground on every IC pin,
decoupling copper path, vias in SMD pad openings, track-to-via joints,
silkscreen over pads and graphic line widths, pad-to-edge distances,
courtyard overlaps, THT drill against the purchased part's recommended hole,
GND articulation points and single-connection islands, per-net minimum width
against the netclass, connector face against the board edge, BOM grouping
(Value + footprint) with the ratings and nets of each group, and the
circuit's function rebuilt from the copper — each driver channel followed
pad → resistor → loads → rail and compared with the firmware map (it showed
all six digits wired identically, so one firmware table serves them all).

## Phase 3 — Dimension reviewers

Give each reviewer one dimension and explicit exclusions so depth is not
lost to overlap:

| Dimension | Core checks (with a pre-set severity trigger each) |
|---|---|
| Power | Input protection order and clamps against every rail capacitor; regulator against its datasheet (`datasheet-audit.md`); ripple against saturation; effective MLCC capacitance; LDO dropout along the whole diode chain at the peak load; power-up sequencing; junction temperatures with the real copper |
| MCU / USB | USB-C sink resistors (one per CC pin, no Rp), D+/D− pairing, VBUS pins and local ESD capacitor, strapping and reserved pins, reset RC at the module, boot glitches on enable lines |
| Interfaces | Receiver fail-safe with each termination option, driver load, protection clamp against the transceiver's absolute maximum, connector pinout against the reference and the installed cable |
| Loads and drivers | Logic thresholds from the receiving part's datasheet, cascade order and power-up states, load current at every corner, per-package dissipation with all outputs on, matching between strings of different length |
| PCB current and thermal | Width and via count against current, vias above 0.5 A, voltage drop to the farthest load, hot-pad copper against the datasheet's θJA area, netclass compliance |
| PCB ground and signal integrity | Articulation points, islands, slots cut by buses, reference fraction under fast signals, hot loops, stitching gaps, parallel clock lines |
| Fabrication | Fabricator minima, joints near V-cut lines, polarity marks, rotation-risk list for the placement file, fiducials, holes, attributes, rails |
| BOM and procurement | Code, stock and package against the measured land pattern; truthful Value with ratings; lot ceiling (stock ÷ quantity per board); binned parts (a Vf rank and a 1,000-piece bag for 990 needed parts risks mixed ranks on one board) |
| Regression | Diff against the last backup; confirm each change and its neighbourhood; sweep the whole board for other instances of each fixed defect class; DRC, ERC, and value parity against the baseline |

Prompt rules that produced checkable findings:

- Build the brief from the files, and tell reviewers to correct it. Wrong
  facts in a brief (package, part count) propagate as givens.
- State premises as "believed; verify". A wrong model in a delegation prompt
  (a fixed 0.3–1.0 V drop for a DMOS sink with 4 Ω on-resistance) produced a
  part requirement no stocked part met.
- Point past the rule checkers: "ERC and DRC are clean; find the functional
  error that passes both."
- Every claim needs a number measured from the file or a literal datasheet
  quote with its section; otherwise "not verifiable, missing X".
- One line per block checked correct, with the method.
- The complete report goes in the final message.
- List the tools available (PDF renderer, KiCad's Python, the netlist
  exporter) and the known accepted exceptions not to re-report unless the
  earlier analysis was wrong. Two reviewers once concluded no PDF renderer
  existed and skipped every curve.

## Phase 4 — Merge

- Assign IDs centrally (block prefix plus sequence) and cluster findings by
  reference, net, and mechanism. Parallel reviewers rediscover one root
  cause under several IDs and reuse IDs for different defects; eight of 35
  verifications on the reference project re-verified duplicates and came
  back with conflicting fixes.
- Run a contradiction check: every "checked OK" claim that touches a
  documented limit or an external interface is compared with every other
  reviewer's findings. An OK list once contained holes another reviewer had
  rated blocking, a connector twice marked "matches the manual" that did
  not, and a current above an absolute maximum.
- Persist the ledger to a file: findings of every severity, OK claims with
  their numbers, and unverifiable items with the decision each would change.
  A review that returned only counts lost 38 low-severity findings and 250 OK
  claims; the next session re-did checks already done and stated false
  coverage gaps.

## Phase 5 — Verification

- Verify every cluster; do not gate verification on the reviewer's own
  severity (it was wrong in both directions). A low-severity module
  temperature note became a BOM change five hours later.
- The verifier re-measures independently and returns a verdict (CONFIRMED,
  OVERSTATED, UNDERSTATED, REFUTED, UNVERIFIABLE), a corrected severity, and
  an action (FIX_BEFORE_FAB, ORDER_NOTE, FIRMWARE_REQ, DOC_ONLY, NONE). Only
  FIX_BEFORE_FAB gates fabrication. A single "partial" verdict that means both
  "overstated" and "do nothing" turns real-but-harmless items into work.
- The verifier validates the proposed fix on a scratch copy (DRC and the
  calculation) and returns coordinates that pass. Proposed fixes failed far
  more often than findings.
- Symmetric cost framing: a false finding costs a needless re-spin; a true
  finding discarded costs boards that do not work.
- Timebox each verifier and checkpoint partial notes.

## Phase 6 — Coverage critic

Start it in parallel with verification and give it the full ledger, not a
truncated list. Ask what nobody checked:

- single-pin nets, power and ground on every IC, floating enable, reset, or
  chip-select lines, two drivers on a net, missing decoupling, polarity,
  mirrored footprints;
- firmware requirements the hardware cannot meet (a register with no hardware
  clear);
- **connector pinout against the cable that will actually be plugged in**,
  from two independent sources — a drawing's physical sequence is not pin
  numbering. The critic found that a retrofit would put the scale's RS-232
  TX on the board's 12 V output, overturning two reviewers' "matches";
- back-door power pins that bypass a protected rail;
- mechanical support of force-bearing connectors;
- procurement (bin, lot, pack size against quantity plus attrition) and the
  environment (enclosure temperature against module and capacitor ratings);
- if nothing new is found, the three largest residual risks.

## Phase 7 — Decision packet

- FIX_BEFORE_FAB items, split into those with no trade-off (apply them) and
  real trade-offs for the user (with cost and effect in numbers).
- Firmware requirements the hardware relies on.
- Order notes: only deviations from the standard process, each with its
  evidence. A "hand-solder this connector" note relayed from a verifier's
  process remark was rejected by the user; the fabricator assembles that part
  routinely.
- Questions for the user or supplier: every unverifiable item whose answer
  changes a decision.
- The updated list of accepted findings, including rationales a verifier
  corrected.

## Phase 8 — Apply and regress

Back up, apply, and re-route rather than move-and-snap. Re-run phases 1–2, a
regression reviewer scoped to the diff, and a same-class sweep. Loop until no
FIX_BEFORE_FAB item is open. Only then state a readiness level
(`verification-traps.md` §6) and attach the ledger.

## Coordinator protocol

- Re-verify every finding that would change the design or the user's
  decision: the numbers and the reasoning. Say who verified what, and quote
  verdicts verbatim; never paraphrase a verdict to justify a retreat.
- Resolve conflicting reviewers with the stronger method (a nodal solver over
  a lumped estimate, the datasheet over memory) and state the criterion
  (for example "absolute maximum at the worst corner, derated at the maximum
  internal ambient").
- Keep a ledger of refuted findings and give it to later rounds; the same
  wrong fix (a pull-up against a push-pull output) was proposed three times.
- When merging recommendations, keep every qualifier. "Add a TVS at the
  connector and keep the existing one" became "move the existing one", which
  left the 12 V rail unclamped.
- Record declined recommendations with their reasons, so they can be
  revisited.
- Decide obvious engineering questions with data (pull a part back from a
  V-cut, drop tooling holes the fabricator adds itself). Ask the user only
  about product intent, cost, and appearance.
- Choose the fix that is right for the product, not the one least risky for
  the reviewer: remove an unused connector pin cleanly instead of giving it
  an invented role, and do not downgrade a confirmed finding because an edit
  attempt failed.
