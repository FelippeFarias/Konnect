# Verification Traps — When "Clean" Is Not Clean

A clean result proves only that the checks which actually ran found nothing.
Every trap below produced a false "clean", "verified", or "ready" on a real
production board (2 layers, 400 × 105 mm, 362 footprints, JLCPCB assembly).
The board was declared ready five times before an independent review with
re-measured evidence closed it. Rule each trap out before reporting a clean
ERC, DRC, parity, connectivity, or fabrication result, and say how.

## 1. Evidence rules

- **A check passes only if it ran in a mode that could fail.** Give every
  number its exact command or tool call and flags. A check that lacked its
  enabling flag, or whose result key was missing, is `BLOCKED`, not zero.
- **Positive control.** When a check reports zero, show that it can fire:
  a known violation still appears, or the check reports what it inspected.
- **Close a fix by re-measuring the defect location.** Report claimed versus
  measured ("VBUS minimum width 0.15 mm → 0.40 mm over all segments"). On the
  reference board three "fixed" claims were written from the plan and were
  false: a neck was still 0.15 mm, zero vias had been doubled, and a net
  claimed moved to B.Cu was 63.5 % on F.Cu.
- **Refute with the same rigour as a finding.** Measure the whole population
  the finding is about and report "n of N checked". A refutation from a small
  sample of capacitors missed that 28 of 36 had no voltage rating.
- **Tool success is not evidence.** Confirm every write in the saved file,
  the exported netlist, or a fresh query. A script once printed "198
  changed" and wrote nothing.
- **Recent edits are the least reviewed.** After a fix batch, the new
  defects on the reference board were in that day's own edits: vias dropped
  inside SMD pads, an unclamped rail after moving a TVS, script-inserted
  footprints without their library link.

## 2. Checks that were never enabled

| Trap | Why it hides defects | Rule it out |
|---|---|---|
| A Board Setup constraint left at **0** | In KiCad 0 disables the check; it is not "no limit". Minimum connection width and silkscreen clearance at 0 hid a 0.15 mm copper neck and 18 silkscreen overlaps | Read the project file: every fabrication-relevant minimum is non-zero and equals the order contract. `set_design_rules` writes only clearance, trace width, via drill, via size, and hole-to-hole; copper-to-edge, hole clearance, annular ring, connection width, silkscreen clearance, and text size need Board Setup or custom rules, then a read-back |
| A rule severity set to **ignore** or downgraded | The whole class disappears or hides among warnings | List the project's rule severities; justify each `ignore`. Re-enable footprint-type mismatch, missing courtyard, and track-not-centred-on-via after scripted edits |
| A custom rule looser than Board Setup | A `.kicad_dru` rule overrides Board Setup even when looser: an edge rule of 0.3 mm overrode 0.5 mm and the pour sat 0.30 mm from a V-cut edge | Compare every custom rule with Board Setup and the contract |
| A noisy custom rule deleted | Deleting it removed the only silkscreen check (Board Setup was 0) and hid references printed off the board | Narrow a noisy rule with layer or type conditions; never delete it to get a clean report |
| Text-only silkscreen rules | A text-thickness rule does not cover silkscreen **graphic** lines. 1,229 lines, including 792 LED polarity marks, sat at 0.12 mm, below the fabricator's 0.15 mm | Audit graphic line widths separately; library footprints often ship 0.12 mm silk |
| Netclass widths assumed enforced | A netclass width is the routing default, not a DRC minimum. +5 V ran 76.7 % of its length at 0.25 mm in a 0.4 mm class | Add a custom rule per class (`A.NetClass == 'Supply'` → track width min) or audit the minimum width of every net against its class |
| Schematic parity not requested | `kicad-cli pcb drc` compares with the schematic only with `--schematic-parity`. Without it the list is empty and reads as "0"; the real count was 24 | Prefer `run_drc`, which includes parity and reports it unchecked when KiCad cannot load the schematic; with kicad-cli always pass `--schematic-parity --severity-all` |
| Items excluded in the GUI, and one error per track | Violations the user marked as excluded in KiCad are not reported unless exclusions are requested (`--severity-all` does; `run_drc` does not), and kicad-cli reports only the first error of each track unless `--all-track-errors` is passed | Request exclusions and all track errors for a release check; review every exclusion |
| Silkscreen-to-pad clearance trusted to DRC | KiCad 10's silkscreen checks missed a 0.076 mm silk-to-pad gap on 198 LEDs with a 0.15 mm clearance rule, and only fired beyond about 0.12 mm of overlap | Measure silkscreen-to-pad and silkscreen-to-mask-opening distances geometrically before release |

## 3. Evidence taken from the wrong file

| Trap | Rule it out |
|---|---|
| DRC or render on an unsaved board | Save first; every check reads the saved file |
| DRC on a copy without its rule files | kicad-cli loads `<name>.kicad_pro` and `<name>.kicad_dru` by basename. A copy saved without the `.kicad_dru` silently drops every custom rule. Copy both, and confirm a known custom-rule hit still appears |
| DRC on a copy outside the project folder | `${KIPRJMOD}` does not resolve, so library-configuration items appear that the real board does not have. Copy the library tables and project library too, or treat those items as artifacts of the copy |
| Fabrication files older than the board | After any change regenerate every layer and drill file into a fresh folder. An `Edge.Cuts` change also changes zone fills, so copper and mask layers change too |
| A schematic edited on disk while the schematic editor holds an older copy | The editor's next save overwrites the edits. The user closes it without saving, or reloads, before any sync or further edit |
| Memory or a previous report instead of the disk | Memory notes are hypotheses with provenance. Before contradicting the disk, check provenance (git diff, file mtime, lock files). A notch the user had just drawn, and drawn wrongly, was once declared "pre-existing and ideal" from memory |

## 4. Metrics that lie

| Metric | What it misses | Better evidence |
|---|---|---|
| "0 unconnected items" | A track that ends on the edge of a via ring. On the reference board an RS-485 receive track ended exactly at the via radius: overlap 0.125 mm, contact lens 0.244 mm, copper 0.025 mm short of the barrel — the only path of that signal. Minimum-connection checks stay silent while the lens is wider than the minimum | Treat track-not-centred-on-via above a few micrometres as an error; move the end to the via centre |
| "GND is one connected group" | Robustness. The ground of the buffer driving every display clock reached the plane only through a tactile-switch pad; the ESD array and the RS-485 transceiver each hung on one via | Articulation points (cut vertices) of the GND copper graph: tracks, vias, pads, and every filled-zone island as nodes. No IC ground pad may depend on a single node |
| Distance to the edge by courtyard | Overestimates V-cut and depaneling risk; the solder joint is what cracks | Pad-to-edge distance. A capacitor "0.855 mm from the edge" by courtyard had its joints 4.15 mm away |
| Geometry from footprint origins | For many THT parts the origin is pad 1, so rotation moves it across the body. A reviewer "found" a 2.54 mm misalignment in every digit that did not exist | Body centroid: pad midpoint, F.Fab outline, or courtyard centre |
| Paste coverage of the numbered pad | Module and QFN footprints put paste in unnumbered paste-only pads | Union of every paste shape over the pad |
| Straight-line distance to a decoupling capacitor | A capacitor 5.3 mm away was 16.4 mm of copper with 2 vias | Copper path length and via count (graph shortest path) |
| Clearance against footprint bounding boxes | A new resistor's pad landed on a via (and 0.02 mm from a track) that the placement script's own output had listed | Real pad geometry against every track, via, and pad nearby, then DRC on the window |
| A worst-case metric over mixed categories | "Minimum annular ring 0.0 mm at H1" (an NPTH) masked 0.15 mm PTH rings | Report each category separately — vias, PTH component holes, NPTH, slots — against its own limit |
| A local clearance checker | Fast, but blind to holes, mask, edge, courtyard, and silkscreen | Use it inside the loop; the full DRC is the gate |
| Transfer checked by totals | 1,581 tracks matched, yet 8 vias came in with the wrong drill | Full signature diff: tracks and vias with width, layer, diameter, and drill; print the count of differences, never a truncated list |
| The Specctra session importer | KiCad assigns the **netclass** via drill, not the padstack drill: 0.6/0.3 vias on a class with a 0.4 mm drill arrived as 0.6/0.4 and broke the annular ring | Audit every via after any SES import |
| An own parser returning zero or "everything unconnected" | Regexes over KiCad 10's multi-line S-expressions silently match nothing | Look at the raw file before concluding; parse S-expressions with a tokenizer |

## 5. Judgement traps

- **Severity follows a documented limit.** A finding that compares a file
  fact with a documented number (absolute maximum, the maker's recommended
  land pattern, a fabricator rule, an app-note "shall") at a defined worst
  case survives verification. Estimates with assumed parameters (θJA on this
  copper, nH per mm, crosstalk, enclosure temperature) were downgraded in
  every case on the reference board; state their assumptions and keep them
  below the documented-limit findings.
- **A proposed fix must be validated before it is recommended.** In 26 of 35
  verified findings the analyst's own fix failed or created a new defect: a
  pull-up fighting a push-pull output, a smaller resistor still over the LED
  limit, a silkscreen change that put ink on 198 pads. Apply the fix to a
  scratch copy and re-run DRC and the calculation.
- **Do not stack independent worst cases** into one headline number, and
  separate uniform effects (supply tolerance moves every LED together) from
  differential ones (Vf spread between neighbours, which the eye sees).
- **Quantify before fixing.** Ten fixes once went to power copper for a
  1.2 % brightness gradient while the LED Vf bin, worth 64 %, stayed open.
- **A uniform class of warnings is a symptom.** 199 library-mismatch warnings
  were waived as "metadata"; the cause was footprints that had lost their 3D
  models and THT/SMD attributes, which would have dropped 198 THT LEDs from
  the pick-and-place file. Open one instance and name the root cause before
  waiving a class.
- **A "not verifiable" item that depends on the user becomes a question now.**
  The enclosure turned out to be outdoors in the sun, which reversed three
  closed decisions and exposed a module rated for 65 °C ambient.

## 6. Readiness levels

Report the one that the evidence supports, never a higher one:

| Level | Meaning |
|---|---|
| Files match the board | The fabrication package was regenerated from the saved board and cross-checked (layers, drills, BOM ↔ CPL ↔ board both ways) |
| Ready for a pilot run | No open fabrication-blocking finding; the remaining questions can only be answered by a prototype (a supplier bin, a power-up transient, a mechanical fit) and are listed |
| Ready for production | The pilot was measured, the questions are closed, and the verification record shows no open fix-before-fab item |

A design review's "ready for fab" verdict means no open blocking finding; it
maps to "ready for a pilot run" at most while questions only hardware can
answer remain. "Files match the board" belongs to the manufacturing workflow,
which regenerates and cross-checks the package.

## 7. Recovery discipline

- Make a restorable backup before touching routed geometry: a dated copy of
  the saved board file, or KiCad's undo for a single IPC batch.
  `snapshot_project` exports PDFs and cannot restore a board. Do not rely on
  an inverse script: a generated revert once dropped its own save call and
  silently did nothing.
- Never move a routed footprint by shifting track ends. Move it, rip up and
  re-route its connections, then run DRC.
- Snap a track end to a pad only when that pad is the only one of its net in
  the region. On a double-row USB-C connector two pads of one net sit 1.0 mm
  apart, and "nearest pad" snapping created shorts.
- Recover a damaged region by restoring its geometric window (tracks and vias
  inside a bounding box) from the backup, not by reverting the whole board.
