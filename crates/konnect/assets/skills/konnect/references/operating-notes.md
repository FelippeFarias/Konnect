# Operating Notes — Working Alongside a Live KiCad

These notes come from a three-day production project (a 344-part schematic
and a 362-footprint board) in which most lost time came from file ownership,
editor state, and unverified tool results rather than from engineering.

## 1. Who owns the files right now

- **KiCad open means KiCad owns the files.** Check for lock files
  (`~<name>.kicad_pcb.lck`, `~<name>.kicad_pro.lck`, `~<name>.kicad_sch.lck`)
  before any file-level write, and read-only analysis is the only work to do
  while they exist.
- **KiCad rewrites the project file when it saves the board.** Netclasses,
  netclass assignments, design rules, and predefined sizes written by a tool
  while the board is open are silently discarded at KiCad's next save.
  `set_predefined_sizes` refuses in that state; `set_design_rules` does not
  refuse and is reverted anyway. Write them with the board closed or in Board
  Setup, then read them back after KiCad's next save.
- **Schematic tools write the sheet files on disk.** An open schematic editor
  keeps an older copy and raises a "file changed" prompt, which blocks IPC.
  The user must reload or close it choosing not to save; saving the stale copy
  reverts the edits. `update_pcb_from_schematic` refuses while the hierarchy
  is open.
- **Live IPC edits exist only in KiCad's memory until saved.** Call
  `save_project` after every applied batch and confirm the file changed.
  Placement applied over IPC was lost twice when the editor closed.
- **Checks read the saved file.** `run_drc` on an unsaved board describes the
  old board.
- **One writer per file, one owner per design.** Two write tools on the same
  sheet in parallel can lose an update. A forked or parallel session works
  read-only, or in its own git worktree, and re-reads the disk (timestamps,
  locks, counts) before acting on another session's report.

## 2. IPC states and user handoffs

| Response | Usual cause | What to ask |
|---|---|---|
| Not ready (`AS_NOT_READY`) | A modal dialog; a "file changed on disk" prompt; new footprints still on the cursor after F8; parallel IPC calls | Close the dialog, or click an empty board area to drop the parts |
| Busy (`AS_BUSY`) | An interactive tool is active, or the 3D viewer is open | Click the canvas and press Esc once or twice; close the 3D viewer |
| Unhandled open-documents request | The PCB editor is closed | Open the board in the PCB editor |

- Run `check_kicad_ui` before a batch, call IPC tools sequentially, and never
  fall back to editing a file while KiCad holds it.
- After F8 (Update PCB from Schematic) the new footprints follow the cursor:
  the user clicks to drop them. Esc cancels their insertion.
- Plan every step's mode before a change — live IPC, file with KiCad closed,
  or a GUI action — and batch them. On the reference project the user opened
  and closed KiCad six times in one hour because the modes were not planned,
  and the assistant reversed its own request mid-change.
- Give one precise instruction per handoff: the exact menu path, what to
  tick, and what **not** to click. Ask for editors to be closed once per
  phase, not for every change.

## 3. KiCad GUI recipes for gaps in the tools

When a change is outside the installed tools, say so, record the gap for the
Konnect roadmap, and choose the path. A change to the board alone can go
through the user's native KiCad action below or the scripted board fallback
(§6): prefer the GUI when one native action does the whole change and the
user is at hand; use the fallback for precise or repetitive geometry that no
single native action does, or when the user asked you to proceed without
them. Every other file goes through the GUI.

| Need | KiCad action |
|---|---|
| Keep references upright, set text or silkscreen line width in bulk | Edit → Edit Text & Graphic Properties (filter by layer and item type) |
| Change via drill or track width in bulk | Edit → Edit Track & Via Properties |
| Restore 3D models and fabrication attributes | Tools → Update Footprints from Library, ticking only 3D models and fabrication attributes; untick the text options, which would reset keep-upright and positions. The update also replaces pads and graphics and undoes deviations made only on the board (a larger drill, a maker's land pattern, an antenna keepout) — keep those in a project library first |
| Swap a footprint library ID, or add parts that split a routed net | Tools → Update PCB from Schematic (F8), with "Delete footprints with no symbols" unchecked so board-only holes and fiducials survive; then re-route the affected copper and run DRC with schematic parity |
| Import copper produced on a verified scratch copy | File → Import → Specctra Session, into a board without tracks; audit via drills afterwards |
| Delete or edit a zone, set thermal spokes | Select the zone → Properties, or Delete |
| Minimum connection width, silkscreen clearance, hole clearance, annular ring, rule severities | File → Board Setup → Design Rules / Violation Severity |

Never create a test object in the live design to discover a tool's
parameters; read the tool's schema first. A probe zone could not be removed
through the tools, and the GUI undo left a duplicate pour behind.

## 4. Verify what a tool claims

- Confirm every write in the saved file, the exported netlist, or a fresh
  query, not in the tool's success message. A script once printed "198
  changed" and wrote nothing; a logo import reported 34 polygons and left a
  single off-board sliver.
- The netlist exported from the saved schematic is the connectivity ground
  truth; pin queries return a null net for wire-only nets.
- Large outputs (batch placement echoes, inline renders, unfiltered trace
  queries) can exceed the tool-result limit; render to a file, and summarise
  batch results to status, completed versus requested, and errors.
- Before contradicting the disk from memory, establish provenance: git diff,
  file modification time, lock timestamps, or ask. Memory notes are
  hypotheses with a source; re-derive safety-relevant numbers from the
  primary document.

## 5. Scratch copies and KiCad's own Python

KiCad's bundled Python (`pcbnew`) is useful to **measure** a saved board and
to **simulate** changes on a scratch copy. It changes a project file only
under the scripted board fallback (§6).

- A scratch copy needs its companions with matching basenames: the project
  file and the custom-rules file (or custom rules silently disappear from
  DRC), plus library tables and the project library (or library-configuration
  items appear that the real board does not have).
- One mutation phase per process: object proxies go stale after bulk
  `Add`/`Remove`, and later iteration fails. Collect objects before mutating,
  remove last, refill zones, save, and check the save call exists.
- `board.Save()` also rewrites the project file beside the board, adding
  KiCad's defaults to it (checked on KiCad 10.0.2). Save with
  `pcbnew.SaveBoard(path, board, True)`, whose third argument skips the
  project settings, and stop if it returns False.
- `via.GetWidth()` without a layer argument raises an assertion that opens a
  **modal dialog on the user's KiCad** and hangs the script; call
  `GetWidth(pcbnew.F_Cu)` and disable wx logging in batch scripts. Pad shape,
  size, and mask methods also take a layer.
- Collision tests on rounded-rectangle pads ignore the corner radius;
  polygonise the pad first for exact distances.
- Print enum values instead of assuming them (layer IDs changed in KiCad 10;
  a zone's layer name printed F.Cu for a B.Cu zone — read its layer set).
- Footprint 3D model lists iterate copies: assigning to an element changes
  nothing; rebuild the list.
- `LoadBoard` needs a `.kicad_pcb` file name; copy and rename backups first.
- Validate every checker on a known-good and a known-bad case. "Every
  instance fails identically" is a checker bug, not a board defect.
- On Windows: set UTF-8 output for scripts that print Ω or µ, disable MSYS
  path conversion for arguments that start with "/" (hierarchical net names),
  and build paths with `os.path.join`.

## 6. Scripted board fallback

The konnect One Rule allows one path outside the Konnect tools: changing the
project's `.kicad_pcb` with KiCad's own Python API when no Konnect tool can
make the change. On the reference project nearly every late board fix went
this way. The scripts that went wrong — a save that never happened, a revert
that lost its save call, track ends snapped to the wrong pad of the same net
and shorted — were caught by reading the file back and by DRC, and undone
from a dated backup. Those steps are the procedure.

**Scope**

- The board file only, changed through `pcbnew` in KiCad's bundled
  interpreter. Never a text tool, never another project file.
- Board-only content: tracks, vias, zones and their fill, footprints without
  symbols (holes, fiducials, logos), pad geometry, text, and graphics.
- Not for anything that must match the schematic (parts with symbols,
  footprint IDs, values, fields, nets): change the schematic, then
  `update_pcb_from_schematic` or Update PCB from Schematic (§3). Script
  inserts lost their library nickname and broke parity on the reference
  project.
- Not for project settings (rules, netclasses, severities): Konnect tools
  with the board closed, or Board Setup. Not for libraries.
- Not a way around a tool's refusal: a refusal is a guard; act on its reason.
- Run only by the session that owns the design (§1). Subagents and parallel
  sessions stay read-only; the bundled agents return the gap to their caller.

**Before**

1. Confirm that no tool makes the change (`list_toolboxes`, then the
   candidate tools' schemas) and write down the missing capability.
2. Tell the user what will change, ask them to close KiCad, and confirm that
   no lock file remains beside the board or the project file.
3. Copy the saved board to a dated file that cannot be mistaken for the
   project's (a backups folder, or outside the project) and record its path.
4. Record the baseline: the project file's hash, the counts of footprints,
   tracks, vias, and zones, and `run_drc` with schematic parity.
5. Write the script to a file outside the project folder. When it moves
   copper or changes many objects, run it on a scratch copy first (with its
   companions, §5) and check the copy with DRC.

**Run**

- One mutation phase per process, with the pitfalls in §5 handled; refill
  zones in the same process when copper changed.
- Save with `pcbnew.SaveBoard(path, board, True)` and stop if it returns
  False; `board.Save()` also rewrites the project file.
- One script at a time on the board; KiCad stays closed until the checks
  pass.

**After**

1. The board file's modification time advanced and the project file's hash
   is unchanged.
2. The saved board, loaded in a new process, shows the intended objects
   changed and the counts moved only as planned.
3. `run_drc` (schematic parity included) shows no violation that the
   baseline did not have.
4. Report what changed, the missing capability (a Konnect roadmap item), the
   backup path, and DRC before and after. The user reopens KiCad and looks at
   the change before the next step.

If a check fails, restore the dated copy with KiCad still closed, confirm the
restore by hash, and report the failure.

## 7. Known limitations in Konnect 0.12 (remove an entry when the tool changes)

- `set_design_rules` writes five constraints only and is reverted if KiCad
  saves the board afterwards (§1).
- `export_specctra_dsn` refused boards with rounded-rectangle pads or a
  custom rules file; `apply_specctra_ses` and `plan_specctra_ses_import` need
  Konnect's own export manifest.
- `update_pcb_from_schematic` reports a changed footprint library ID as a
  conflict, refused to split a routed net even after its copper was deleted,
  and once timed out leaving nets without footprints (dry-run again, then
  re-apply).
- `add_power_symbol` numbered power references per sheet and collided with
  existing references while ERC stayed clean; check references across all
  sheets.
- `batch_edit_schematic_components` updates existing fields only.
- `add_zone` cannot set thermal spoke width or gap, and no tool deletes a zone.
- `update_footprints_from_library` refused footprints containing some clauses
  (unlocked properties, pad properties, point children); use the GUI recipe.
- `score_placement` works on bounding boxes: L-shaped courtyards (a module
  with its antenna area) produce false hard failures; confirm each with
  KiCad's DRC before moving parts.
- `copy_routing_pattern` edits the saved board file, so it needs the board
  closed in KiCad.
- `snapshot_project` exports the schematic and board to PDF; it is a visual
  checkpoint, not a restorable backup.
- `run_drc` always requests schematic parity but not GUI exclusions: items
  the user excluded in KiCad are not reported.
- Konnect reads its IPC address (`ipc_address` in its settings) only at
  startup; after changing it, reconnect the MCP server.
- There is no tool for footprint text properties (keep-upright, size,
  position) or for drawing silkscreen lines.
