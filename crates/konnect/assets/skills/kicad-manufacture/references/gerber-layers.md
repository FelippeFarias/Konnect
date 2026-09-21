# Gerber Layer Mapping

## Common Gerber File Extensions

Filenames and extensions vary with KiCad and exporter options. Accept files by
their plotted layer and content, not by suffix alone.

| KiCAD Layer | Gerber Extension | Purpose |
|-------------|-----------------|---------|
| `F.Cu` | `.gtl` or `-F_Cu.gbr` | Front copper |
| `B.Cu` | `.gbl` or `-B_Cu.gbr` | Back copper |
| `In1.Cu` | `.g2` or `-In1_Cu.gbr` | Inner layer 1 |
| `In2.Cu` | `.g3` or `-In2_Cu.gbr` | Inner layer 2 |
| `F.Mask` | `.gts` or `-F_Mask.gbr` | Front solder mask |
| `B.Mask` | `.gbs` or `-B_Mask.gbr` | Back solder mask |
| `F.SilkS` | `.gto` or `-F_Silkscreen.gbr` | Front silkscreen |
| `B.SilkS` | `.gbo` or `-B_Silkscreen.gbr` | Back silkscreen |
| `F.Paste` | `.gtp` or `-F_Paste.gbr` | Front paste (stencil) |
| `B.Paste` | `.gbp` or `-B_Paste.gbr` | Back paste (stencil) |
| `Edge.Cuts` | `.gm1` or `-Edge_Cuts.gbr` | Board outline |

## Drill Files

| File | Extension | Purpose |
|------|-----------|---------|
| Plated through-holes | `.drl` or `-PTH.drl` | Component holes + vias |
| Non-plated holes | `-NPTH.drl` | Mounting holes, slots |
| Drill map | `.drl.map` | Visual drill reference |

## What `export_gerber` Attempts

With no explicit layer list, `export_gerber` selects enabled copper layers,
front/back mask and silkscreen, and `Edge.Cuts`. Paste is not in that default
selection. With `drill_file` enabled, drill export is a separate best-effort
step. The returned directory listing may contain older entries and does not
prove that the current invocation created a complete, non-empty set.

Choose layers from the saved board and the selected fabricator's current order
contract. Use a fresh output directory and apply the manufacturing skill's
artifact acceptance gate.

## Regenerating with kicad-cli

When outputs are produced with KiCad 10's command line, these details decided
whether the package was current:

| Output | Invocation that worked | Pitfall it avoided |
|---|---|---|
| Gerbers | `kicad-cli pcb export gerbers --no-protel-ext --subtract-soldermask --layers "F.Cu,B.Cu,F.Paste,B.Paste,F.Silkscreen,B.Silkscreen,F.Mask,B.Mask,Edge.Cuts"` | Without `--no-protel-ext` the files came out as `.gtl/.gbl/.gts…` next to an older `.gbr` set, leaving two versions in the folder |
| Drill | `kicad-cli pcb export drill --format excellon --excellon-units mm --excellon-separate-th --generate-map --map-format gerberx2` | The unit flag is `--excellon-units`; `--units` printed the usage text, and piping through `tail` hid the failure while the old drill files stayed |
| Placement | `kicad-cli pcb export pos --format csv --units mm --side both` | Boolean flags take no value; KiCad's Y axis is negated relative to the board view; rename headers to the fabricator's template |
| DRC gate | `kicad-cli pcb drc --format json --severity-all --schematic-parity` | Without `--schematic-parity` the parity list is empty and reads as zero |
| Board-sized view | `kicad-cli pcb export svg --mode-single --page-size-mode 2 --exclude-drawing-sheet` | A page-sized export cut a 400 mm board to A4 |

- Check each command's exit status and its "created file" lines; never judge
  success from a truncated tail of the output.
- Use the same origin choice (absolute or drill/place origin) for plot, drill,
  and placement files. A mismatch shifts holes relative to copper and still
  uploads without an error.
- To prove a delivered package is current, regenerate into a temporary folder
  and diff line by line against it, ignoring comment, creation-date, and
  generator-version lines.
- KiCad's Gerber coordinates are Y-up: a notch at the top edge of the board
  view appears at negative Y when the Edge.Cuts file is decoded.

## Verification Checklist

Before uploading Gerbers:

1. Re-run direct DRC against the saved board and adjudicate every result.
2. Confirm each requested artifact is a regular, non-empty file from the fresh
   invocation.
3. Reconcile every expected copper, mask, silkscreen, paste, and outline layer
   with the accepted manifest.
4. Open Gerbers and drills in a viewer and inspect registration, outline,
   apertures, mask, paste, silkscreen, holes, and slots.
5. Compare the viewer and upload preview with the selected fabricator's current
   order contract.

Any missing, stale, empty, or unexplained artifact makes the package `INCOMPLETE`.
