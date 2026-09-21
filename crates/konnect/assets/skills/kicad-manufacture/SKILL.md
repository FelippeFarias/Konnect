---
name: kicad-manufacture
description: |
  Manufacturing and fabrication workflow for KiCAD projects via MCP tools. Triggers on: "send to fab",
  "order boards", "gerbers", "JLCPCB", "manufacturing", "export for production", "pick and place",
  "assembly files", "generate fabrication outputs", "BOM for fab", "production files", "fab house".
argument-hint: "[fab house or export task]"
---

# KiCAD Manufacturing & Fabrication Workflow

Prepare manufacturing outputs through Konnect MCP tools. Treat each tool result
as evidence with a named scope: a successful request is not by itself proof of a
complete, current, upload-ready package. A required check or artifact that cannot
be established makes the manufacturing verdict `INCOMPLETE`.

## Toolset loading

Load the required toolsets:

```
load_toolset('pcb_export')       # Gerber, drill, BOM, position, 3D, and direct DRC evidence
load_toolset('manufacturing')    # Package export, manufacturing preflight, and rough cost estimate
```

Load additional toolsets only for the branch that needs them:

```
load_toolset('sch_analysis')     # schematic inventory and footprint assignment
load_toolset('integration')      # local JLCPCB catalogue and alternatives
```

Call `get_active_toolsets()` before loading more.

### References by manufacturing branch

- Read [`references/gerber-layers.md`](references/gerber-layers.md) when
  selecting plot layers or accepting a generated artifact inventory.
- Read [`references/jlcpcb-rules.md`](references/jlcpcb-rules.md) only when
  JLCPCB is the selected fabricator or assembler. It defines how to capture the
  current order contract without caching volatile limits, prices, categories,
  or field names in this skill.

## 1. Capture the order contract

Before checking or exporting, record the selected fabricator, service tier,
stackup, copper weight, finish, assembly sides, stencil requirement, quantity,
and any controlled-impedance service. Record the source and retrieval date for
every vendor-controlled requirement. Project rules and output acceptance are
judged against that record.

Completion criterion: every applicable fabrication and assembly constraint has
one current authority, and no decision rests on an undated table in this skill.

The order configuration changes the design rules, so fix it before the DFM
review and encode its consequences in the board rules:

- **Panel and rails** — rails attached by V-cut need more copper-to-edge
  clearance than a routed edge, and solder joints near a break line crack
  when the rails are removed after assembly (measure pad-to-edge, not
  courtyard-to-edge). V-cut needs a continuous straight line; a notch on a
  railed edge needs routing and tabs there — confirm with the fabricator.
- **Who adds fiducials and tooling holes** — when the fabricator adds rails,
  it may put its own on them, and board-level tooling holes become wasted
  area.
- **Assembly tier** — its reflow profile against every part's maximum (a
  module rated 250 °C peak ruled out a hotter tier), single versus double
  side, panel limits.
- **Board size** — long boards may not be panelizable for assembly, may need
  a minimum thickness, and deserve a bow-and-twist requirement in the order
  remarks.
- **Mask colour** — the minimum solder-mask bridge can depend on it.

Before the order, list every edge's occupants (edge connectors, module
bodies, antenna keepouts) from coordinates, and copy verified remedies into
the order remarks verbatim, not as a paraphrase.

## 2. Establish direct design evidence

Run direct KiCad DRC against the saved target board and resolve every error or
record a deliberate, reviewable waiver. Then run:

```
validate_for_manufacturing(board, fab_house?)
```

The current handler checks only:

- presence of at least one `Edge.Cuts` item;
- presence of footprints;
- the configured minimum trace width against its built-in fab profile;
- the coarse case where several nets exist but no routed tracks exist; and
- direct `kicad-cli` DRC evidence, which must be available and complete for a
  `READY` verdict.

Read `verdict`, `issues`, and `drc` together. A null or incomplete `drc`, a
`NOT READY` verdict, or an unadjudicated issue blocks release.

This preflight does not prove outline closure, copper on every pad, drill-size
acceptance, silkscreen clearance, stackup compatibility, or assembly readiness.
Establish those separately with direct DRC, the selected fabricator's current
contract, Gerber/drill inspection, BOM/CPL review, and the order preview.

Completion criterion: direct DRC is complete, every reported issue is resolved
or waived, and every check outside the handler's stated scope has named evidence.

Direct DRC is only as strong as its configuration. Before accepting it, rule
out the traps in the `kicad-review` skill's `references/verification-traps.md`:
constraints left at 0 (disabled), ignored severities, custom rules missing
from a checked copy, schematic parity never requested, netclass widths not
enforced, and silkscreen graphic line widths that no text rule checks.

## 2b. BOM integrity

A BOM that exports cleanly can still order the wrong parts. Check:

- **The Value names the ordered part, with the ratings that decide the
  purchase** (`22uF 25V`). The assembler reads Value as its comment column;
  custom fields are dropped by a standard export; a label such as "RESET" or
  a stale part name (MAX3485 on an SP3485 line) makes the BOM lie.
- **One rating per group.** Parts are grouped by Value and footprint. List
  the nets and required ratings of each group's members: one line once held
  10 V capacitors on 5 V and unrated capacitors on a 12 V rail, and a planned
  footprint clean-up would have merged them for good. Declare ratings first,
  then unify identities.
- **A distributor code and the full manufacturer suffix on every line**:
  same-package variants can have different pinouts (AP2114H versus
  AP2114HA), and tube and reel suffixes can differ in lifecycle and stock.
- **A substitution is a design change.** Re-verify the substitute against its
  own datasheet: pinout, ratings, input structure in every power mode,
  mechanical actuation, land pattern. Stock-driven swaps on the reference
  project broke USB-only power (HCT inputs clamp to V_CC), nearly fitted
  side-actuated switches, and put a clone TVS with half the surge rating on
  the field port.
- **Protection parts are chosen by the specific code's verified surge
  rating**, never by stock or price alone.
- **Lot ceiling and bins.** For every line, stock ÷ quantity per board caps
  the run. For binned parts (LED forward voltage, brightness), check the
  pack size against the quantity plus attrition — 990 LEDs from 1,000-piece
  bags risks two ranks on one board — and get the supplier's rank
  confirmation or supply the parts.
- **Parts added during a fix inherit every convention**: rating in the Value,
  distributor code, library nickname, description.
- Consolidating values into fewer distinct lines lowers assembly setup cost
  and removes wrong-part risk at the same time; check the selected service's
  current price rules for how lines are charged.

## 3. Export into a fresh destination

Prefer a new, empty output directory for each invocation. This makes stale files
structurally unable to impersonate output from the current invocation.

For a package attempt:

```
export_manufacturing_package(board, output_dir, fab_house?, schematic?, jlcpcb_cpl_corrections_path?)
```

Pass `schematic` when assembly output requires a BOM. The tool attempts Gerber,
drill, position, and BOM exports according to the request; individual failures
can still leave a partial directory.

For `fab_house="jlcpcb"`, use millimetres (the default) and provide the BOM
fields, labels, and grouping required by the current order contract. The tool
emits `BOM-<project>.csv` plus `CPL-<project>.csv`; the CPL uses JLCPCB's
`Designator,Mid X,Mid Y,Layer,Rotation` schema, and KiCad is instructed to
enumerate grouped BOM references instead of compressing them into ranges. DNP
parts are excluded from both native exports; any remaining population mismatch
caused by board/schematic exclusion flags makes the package incomplete. This
conversion applies Konnect's independently verified built-in CPL correction
policy. Pass a checked-in project policy through
`jlcpcb_cpl_corrections_path` when an unmatched footprint or one exact
designator needs a correction. The precedence and JSON format are documented in
`docs/JLCPCB_CPL_CORRECTIONS.md`.

Inspect **placement_orientation.applied_corrections** and
**placement_orientation.unmatched_footprints**. **complete: true** proves the
package is structurally complete; it does **not** prove physical placement
orientation. Require **placement_orientation.status == "PREVIEW_REQUIRED"** to
be discharged by inspecting every component in JLCPCB Component Placements
before an order is approved. Never describe the CPL as physically validated
from the automated result alone.

### Artifact acceptance gate

1. Inspect `warnings` and `files_generated`. Any warning or missing requested
   artifact type keeps the result `INCOMPLETE`.
2. Reconcile the requested copper, mask, silkscreen, paste, and `Edge.Cuts`
   layers against the actual files in the fresh output directory.
3. Confirm every required artifact is a regular, non-empty file produced by the
   current invocation. A directory entry, reported path, or zero exit status is
   not enough.
4. Confirm the required plated and non-plated drill outputs for the actual board
   hole inventory. Absence is acceptable only when the design proves that output
   is inapplicable.
5. Open the Gerbers and drills in a viewer. Inspect layer registration, outline,
   apertures, holes/slots, mask, paste, and silkscreen.
6. For assembly, inspect BOM contents, DNP handling, designator coverage, CPL
   side/units/origin/rotation, and the fabricator's export preview.

The `files` field is derived from regular, non-empty artifacts verified at the
export boundary; `files_generated` describes each successful export. That
evidence does not establish vendor acceptance, correct component rotations, or
that unrelated stale files elsewhere in a reused directory are safe to upload.
Preserve the viewer and order-preview checks.

Completion criterion: an accepted manifest accounts for every required output,
every accepted path is fresh and non-empty, and visual/order previews agree with
the saved design.

Regeneration triggers:

- Any `Edge.Cuts` change: refill zones, re-run DRC with parity, and regenerate
  **every** layer, drill, map, and job file — zone fills change the copper and
  mask layers too. Confirm the new outline in the Edge.Cuts Gerber itself.
- A BOM-only change regenerates the BOM and the placement file (its value
  column changes); Gerbers stay valid only if no copper, mask, or outline
  changed.
- After regenerating, cross-check BOM designators against the placement file
  in both directions, check for duplicates, and list the parts deliberately
  excluded (holes, fiducials).

## 4. Use manual exports when control is required

Use the individual tools when a package needs explicit layer, BOM, side, unit,
or filename choices:

```
export_gerber(board, output_dir, layers?, drill_file?)
export_bom(schematic, output, format?, fields?, group_by?, labels?, exclude_dnp?)
export_position_file(board, output, format?, side?, units?)
```

Apply the same fresh-destination and artifact acceptance gate. A manual sequence
does not lower the evidence requirement.

## 5. Treat cost output as a heuristic

```
estimate_cost(board, quantity?, layers?, fab_house?)
```

`estimate_cost` is an indicative heuristic built from fixed assumptions and
rough average component costs. Use it only for coarse comparisons. It is not a
vendor quote and does not know the selected finish, service, complete BOM,
shipping, taxes, coupons, or current pricing. Budget and purchasing decisions
require a current quote from the selected fabricator.

## 6. Record manufacturing acceptance

The final report must include:

- saved design revision or hash;
- direct DRC status and any explicit waivers;
- `validate_for_manufacturing` verdict, issues, and DRC coverage;
- selected fabricator/order contract with source and retrieval date;
- accepted artifact manifest with file type, path, and non-empty evidence;
- Gerber/drill viewer result;
- BOM/CPL and order-preview result when assembly is in scope;
- 3D/enclosure inspection status when mechanically relevant;
- the requirements the hardware places on firmware and the product
  (register clear before enable, thermal derating, jumper defaults,
  enclosure colour or venting), written into the release notes;
- the questions only a prototype, the user, or the supplier can answer; and
- final `READY`, `NOT READY`, or `INCOMPLETE` verdict.

Only `READY` permits upload. Preserve the accepted manifest rather than telling
the user to upload every entry found in a reused directory.

`READY` means the package matches the board and the fabricator can build it.
It does not mean the product is proven. When open questions can only be
answered by hardware — a supplier bin, a power-up transient, a mechanical fit
— recommend a pilot run from the same package and list what to measure on it,
and offer a staged order (bare boards first, assembly after the one
irreversible risk is resolved). Never let "files are ready" stand for "ready
for production".
