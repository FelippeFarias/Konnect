---
name: kicad-review
description: |
  Design review and validation workflow for KiCAD projects via MCP tools. Triggers on: "review my design",
  "check for errors", "audit", "DRC", "ERC", "find problems", "design review", "is this ready",
  "validate", "check my schematic", "check my PCB", "what's wrong", "run checks", "pre-fab review".
argument-hint: "[what to review]"
---

# KiCAD Design Review & Validation Workflow

This skill guides Claude through systematic design review of a KiCAD project using MCP tools.
ALL checks are performed through MCP tools — never parse .kicad_sch or .kicad_pcb files directly.

---

## Toolset Loading

Load the required toolsets for design review:

```
load_toolset('sch_analysis')     # find_orphan_items, find_shorted_nets, find_single_pin_nets
load_toolset('verification')     # run_drc, check_clearance, get_design_rules
load_toolset('sch_export')       # run_erc
load_toolset('pcb_export')       # get_drc_violations
load_toolset('manufacturing')    # validate_for_manufacturing
load_toolset('design_review')    # audit_decoupling, audit_connections, audit_power_rails, etc.
```

Optional (for deeper analysis):

```
load_toolset('sch_analysis')     # get net info, trace connections, inspect components
load_toolset('pcb_routing')      # query_traces, get_nets_list
```

For the layout-quality branch of a PCB review:

```
load_toolset('pcb_components')   # get_component_pads, get_component_list, get_board_2d_view
load_toolset('pcb_board')        # get_board_extents, get_layer_list
load_toolset('placement')        # score_placement
```

Always call `get_active_toolsets()` first to see what is already loaded.

## Evidence hierarchy

Judge findings in this order:

1. Exact design requirements and manufacturer datasheets.
2. Direct KiCad ERC/DRC and saved or exported connectivity.
3. Direct Konnect net, short, pad, trace, via, unrouted, and inventory evidence.
4. Aggregate review and manufacturing summaries.
5. Heuristic orphan, single-pin, decoupling, protection, and best-practice findings.

A weaker finding may ask a question; it does not override stronger contradictory
evidence. Any required check that did not run, returned impossible coverage, or
remains inconsistent with stronger evidence makes the verdict `INCOMPLETE`.

Rules that kept this hierarchy honest on a production board declared ready
five times before it really was:

- A check passes only if it ran in a mode that could fail. Give every number
  the command or tool call and flags that produced it; a missing flag or a
  missing result key is blocked evidence, not zero.
- Severity follows a documented limit. A file fact that violates a datasheet
  absolute maximum, the maker's recommended land pattern, a fabricator rule,
  or an app-note requirement at a defined worst case can be critical; an
  estimate built on assumed parameters (θJA on this copper, nH per mm,
  enclosure temperature) states its assumptions and ranks below it.
- A fix is closed by re-measuring the defect location, and a finding is
  refuted only by measuring its whole population ("n of N checked").
- A proposed fix is part of the finding and is validated (DRC and the
  calculation on a scratch copy) before it is recommended.

DRC violation items carry `owner` and `ownership_status`. That is direct
evidence, not a heuristic: `owner.kind: "footprint"` means the offending
geometry belongs to `owner.reference`'s own artwork, so the remedy is a
footprint or rule change rather than a placement change — the finding itself
stands either way. When `ownership_status` is not `"resolved"`, ownership is
unknown and `owner` is `null`; corroborate with
`list_board_footprint_graphics` rather than assuming the board owns it.

## References by review branch

- Read [`references/design-checklist.md`](references/design-checklist.md) for a
  comprehensive or pre-fabrication review. Mark an item only from evidence
  collected in this run.
- Read [`references/error-taxonomy.md`](references/error-taxonomy.md) when
  classifying a finding or assigning the final verdict. Direct ERC, DRC, and
  connectivity evidence outrank heuristic classifications.
- Read [`references/layout-review.md`](references/layout-review.md) when the
  project has a PCB. DRC proves rule compliance; this branch checks that the
  placement follows the circuit, that return currents have a path, that
  widths match the current record, and that the board can be assembled and
  tested. Save before running it: DRC and renders read the saved file.
- Read [`references/verification-traps.md`](references/verification-traps.md)
  before reporting any clean ERC, DRC, parity, connectivity, or
  fabrication result, and before stating a readiness level. It lists the
  checks that are silently disabled, the metrics that lie, and the three
  readiness levels.
- Read [`references/datasheet-audit.md`](references/datasheet-audit.md) for
  every IC block in a full or pre-fabrication review, and after any part
  substitution: compare against the manufacturer's application circuit,
  equations, and layout section, and check that symbol, BOM part, footprint,
  and datasheet describe the same part.
- Read [`references/review-orchestration.md`](references/review-orchestration.md)
  when running a full or pre-fabrication review, with or without subagents,
  and after every batch of fixes (its regression phase): freeze and
  inventory, dimension reviewers, a findings ledger, independent
  verification, a coverage critic, and the decision packet.

---

## Quick Checks (Escalating Severity)

Run these first — they are fast and catch the most critical issues.

### Level 1: Structural Integrity

```
find_orphan_items()
```

Finds floating wires, labels, and symbols not connected to anything. Treat the
result as a heuristic candidate list and corroborate it with direct connectivity
or ERC before calling an item a defect.

### Level 2: Critical Net Issues

```
find_shorted_nets()
```

Detects nets that are connected together but should not be. A shorted net means:
- Two different net labels on the same wire
- Power rails bridged unintentionally
- Signal nets merged by accident

A confirmed unintended short is critical. Resolve disagreement with requirements
or direct ERC/connectivity evidence before assigning severity.

### Level 3: Suspicious Connections

```
find_single_pin_nets()
```

A one-pin net is a heuristic review candidate:
- Incomplete wiring (forgot to connect the other end)
- Orphan net labels (typo in name, so it does not match)
- Leftover stubs from deleted components

---

## Formal Checks

### ERC — Electrical Rules Check

```
run_erc()
```

Checks schematic-level rules:
- Pin type conflicts (output driving output, unconnected inputs)
- Power pin connections
- Missing no-connect flags
- Duplicate reference designators
- Missing net connections

Review each violation. Some can be waived (e.g., intentional unconnected pins marked with no-connect flag).

### DRC — Design Rules Check

```
get_drc_violations()
```

Checks PCB-level rules:
- Clearance violations (copper-to-copper, copper-to-edge)
- Minimum trace width violations
- Minimum drill size violations
- Unrouted connections (incomplete routing)
- Zone fill issues
- Courtyard overlaps

**Every DRC error must be resolved or explicitly justified before manufacturing.**

A passing DRC means the layout obeyed the rules it was given — not that the
circuit will work. Incomplete rules approve a bad board. Check
`get_design_rules` against the fabricator's contract before trusting a clean
result, then run the layout-quality branch.

`get_design_rules` returns five values only. Read the project's full rule
configuration too: a constraint left at 0 is a disabled check (minimum
connection width and silkscreen clearance at 0 hid a 0.15 mm neck and 18
silkscreen overlaps), ignored severities hide whole classes, and a custom
rule overrides Board Setup even when it is looser. `run_drc` includes
schematic parity (but not items excluded in the GUI); when collecting DRC
with kicad-cli directly, pass `--schematic-parity --severity-all` — without
the parity flag the list comes back empty and reads as zero. The full list of
traps is in `references/verification-traps.md`.

---

## Layout Quality (PCB)

Run after ERC and DRC, on the saved board, following
[`references/layout-review.md`](references/layout-review.md):

1. `get_component_pads` for every part against `get_board_extents`: every pad
   inside the outline and the edge clearance; pads that share a number are
   bridged by copper.
2. `score_placement` and `get_board_2d_view`: blocks grouped along the
   circuit flow, connectors at edges, controls reachable, noise sources away
   from sensitive parts, pins facing their destinations, no trace across a
   part body.
3. `query_traces` per critical net: width against `get_netclasses` and the
   current record; the path against the return-path plan; no plane slot
   crossed.
4. Assembly, test, thermal, and mechanical rows of the review table against
   the constraint record.

A visual finding is heuristic until a pad position, trace list, or DRC item
corroborates it. Report the corroboration with the finding.

---

## Design Audits

These go beyond rule checking — they evaluate design quality and best practices.

The standalone schematic audits and `check_bom_health` default to the supplied
file only. When the supplied file is a hierarchy root, pass
`schematic_scope: "hierarchy"` to cover every reachable sheet instance. Read
`status`, `coverage`, and `diagnostics` before interpreting a hierarchy result;
missing or cyclic child references make the result incomplete. Reused child
files have one result per KiCad sheet instance, identified by the
`sheet_instance_path` response field.

### Decoupling Audit

```
audit_decoupling(schematic, schematic_scope="hierarchy")
```

Checks:
- Every IC power pin has a bypass capacitor
- Capacitor is placed close to the pin (PCB proximity)
- Appropriate capacitor values (100nF ceramic minimum)
- Bulk capacitance present for high-current ICs

### Connection Audit

```
audit_connections(schematic, schematic_scope="hierarchy")
```

Checks:
- All expected connections are made
- No nets with unexpected fan-out
- Signal integrity basics (termination on long traces)
- Pull-up/pull-down resistors where required (I2C, reset pins, enable pins)

### Power Rail Audit

```
audit_power_rails(schematic, schematic_scope="hierarchy")
```

Checks:
- All power rails have proper source (regulator, connector, etc.)
- Current capacity matches expected load
- Voltage levels are consistent (no 3.3V device on 5V rail)
- Power sequencing considered for multi-rail designs
- Power flags present (avoids ERC false positives)

### Manufacturing Audit

```
audit_manufacturing()
```

Checks:
- All footprints are fab-house compatible
- Pad sizes meet minimum requirements
- Silkscreen readability
- Test point accessibility
- Fiducial marks present (for SMT assembly)
- Mechanical clearances around mounting holes

---

## Full Review Shortcut

```
run_design_review()
```

Runs the aggregate design audits and produces a consolidated report. Use it to
organize findings, not as a substitute for the direct ERC, DRC, and connectivity
checks above.

Read `status`, `coverage`, and `diagnostics` before interpreting the findings.
If `status` is `partial` or `failed`, the verdict is `INCOMPLETE — review could
not evaluate the full design`. Report that verdict verbatim, explain the
diagnostics and unevaluated coverage, and do not describe the design as ready,
passing, clean, or looking good. Findings gathered before the coverage gap are
still valid and should still be reported.

Collect direct `run_erc`, `get_drc_violations`, short, and connectivity evidence
separately. Then compare the aggregate findings with that stronger evidence and
report any disagreement.

---

## Severity Classification

### CRITICAL — Must fix before manufacturing

| Finding                            | Why Critical                                    |
|------------------------------------|-------------------------------------------------|
| Shorted nets                       | Short circuit on the board, may damage components |
| Missing ground connection          | Circuit will not function                       |
| Reversed polarity on power IC      | Immediate destruction on power-up               |
| Unrouted nets                      | Missing connections on fabricated board          |
| DRC clearance violation            | May cause electrical short on fab board          |
| Power pin unconnected              | IC will not operate                             |
| Wrong voltage on IC power pin      | Exceeds absolute maximum, destroys part         |
| Pad or copper outside the outline / inside edge clearance | Cut by the fabricator or broken ring |
| Same-number pads not bridged (switch, connector) | Node open on the board; DRC unconnected item |
| Trace narrower than the current record requires | Heating or voltage drop in service |

### WARNING — Should fix, design risk

| Finding                            | Why a Warning                                   |
|------------------------------------|-------------------------------------------------|
| Missing decoupling capacitor       | Noise susceptibility, possible oscillation      |
| No test points on key signals      | Cannot debug in production                      |
| No ESD protection on connectors    | Vulnerable to ESD damage in the field           |
| Single-point-of-failure nets       | No redundancy for critical signals              |
| Pull-up/pull-down missing          | Floating input, unpredictable behavior          |
| Tight clearances (near DRC limit)  | Higher fab defect rate                          |
| Fast or sensitive trace over a reference-plane slot | Return discontinuity, emissions, crosstalk |
| Decoupling cap far from its pin / long loop | Rail noise; the schematic promise is not kept on copper |
| Trace crossing a part body, or leaving a pad on the far side | Assembly risk; placement was not pin-aware |
| Connector not at an edge / control unreachable in the enclosure | Product cannot be assembled or used |
| Noise source beside a sensitive input, reference, or crystal | Coupling the datasheet warns about |

### SUGGESTION — Improvement opportunities

| Finding                            | Why a Suggestion                                |
|------------------------------------|-------------------------------------------------|
| Consolidate passive values         | Fewer unique BOM lines, lower assembly cost     |
| Add net labels to unnamed nets     | Improves schematic readability                  |
| Missing silkscreen designators     | Harder to assemble and debug manually           |
| Components could be closer         | Shorter traces, better signal integrity         |
| Consider bulk capacitor addition   | Better transient response on power rails        |
| Add board revision marking         | Traceability for manufacturing runs             |

---

## Reporting Format

Present findings grouped by severity with actionable fix suggestions:

```
## Design Review Results

### CRITICAL (X issues) — Must fix

1. **[Finding title]**
   - Location: [component reference or net name]
   - Issue: [what is wrong]
   - Fix: [specific action to take using MCP tools]

### WARNING (X issues) — Should fix

1. **[Finding title]**
   - Location: [component reference or net name]
   - Issue: [what is wrong]
   - Fix: [specific action to take]

### SUGGESTION (X items) — Optional improvements

1. **[Finding title]**
   - Detail: [what could be better]
   - Action: [suggested improvement]

### Summary
- Critical: X (must resolve)
- Warnings: X (recommended)
- Suggestions: X (optional)
- Verdict: [LOOKS GOOD / NEEDS ATTENTION / NOT READY / INCOMPLETE]
- Readiness level: [files match the board / ready for a pilot run / ready for production / none]
- Coverage status: [complete / partial / failed]
- Coverage diagnostics: [none, or each unevaluated sheet/object/audit]
- Checked and correct: [one line per block, with the method]
- Open questions: [items only the user, the supplier, or a prototype can answer]
```

Each finding also carries its action: fix before fabrication, order note,
firmware requirement, documentation only, or none. Only "fix before
fabrication" blocks the order; a real but harmless item is reported, not
turned into work.

---

## Review Workflow

### Quick Review (5-minute check)

1. `find_shorted_nets()` — catch fatal issues
2. `run_erc()` — schematic rule check
3. `get_drc_violations()` — PCB rule check
4. Report findings

### Full Review (comprehensive)

1. Load all review toolsets
2. Save, then run direct short/connectivity checks, `run_erc`, and `get_drc_violations`
3. `run_design_review()` — aggregate audit suite
4. Check `status`, `coverage`, and `diagnostics`; never approve an incomplete review
5. Layout quality (PCB): the branch above, per `references/layout-review.md`
6. Reconcile aggregate or heuristic findings with stronger direct evidence
7. Classify all gathered findings by severity
8. Present report with fix suggestions, including the `Layout quality` block
9. Offer to fix CRITICAL issues immediately

### Pre-Manufacturing Review

1. Full review (above), structured per `references/review-orchestration.md`
2. Datasheet conformance for every IC block (`references/datasheet-audit.md`)
3. Run `validate_for_manufacturing()`, then inspect `verdict`, `issues`, and
   `drc` against the handler's limited contract; it does not replace outline,
   drill, silkscreen, artifact, BOM/CPL, or order-preview acceptance
4. Verify BOM completeness and integrity: every Value names the ordered part
   with its ratings, one rating per Value + footprint group, a distributor
   code and full manufacturer suffix on every line
5. Check part availability (if targeting specific fab house), including the
   lot ceiling (stock ÷ quantity per board) and binned parts
6. Turn every "not verifiable" item that depends on the user (enclosure,
   ambient, installed cables) into a question now
7. Final verdict and readiness level (`references/verification-traps.md` §6)

After any fix batch, run the regression phase of
`references/review-orchestration.md` before offering fabrication files; the
fixer's own confirmation is not verification.

---

## Rules

1. **Never skip quick checks** — find_shorted_nets catches the worst bugs fast
2. **Classify every finding** — severity helps the user prioritize
3. **Provide specific fixes** — name the MCP tool and parameters to resolve each issue
4. **Run DRC after fixes** — verify that corrections did not introduce new violations
5. **Do not approve a design with CRITICAL issues** — even if the user says "it's fine"
6. **Load toolsets first** — check `get_active_toolsets()` and load what you need
7. **Save before reviewing** — ensures checks run against current state
8. **Offer to fix** — after reporting, offer to use MCP tools to resolve issues
9. **Re-run after fixes** — always verify fixes resolved the issue and created no new ones
10. **Document waivers** — if user explicitly waives a warning, note it in the report
11. **Never soften `INCOMPLETE`** — partial or failed coverage is not a passing review
12. **Never claim "ready" from your own fixes** — an independent re-verification
    with re-collected evidence closes a fix batch; report the readiness level
    the evidence supports and no higher
13. **Waive a class of warnings only after opening one instance** — 199
    library-mismatch warnings waived as "metadata" were footprints that had
    lost their 3D models and THT/SMD attributes
14. **Re-verify subagent findings before relaying them** — the numbers and the
    reasoning; say who verified what and quote verdicts verbatim
