---
name: kicad-photo-intake
description: |
  Workflow skill for turning photographs of a physical PCB into a human-reviewed,
  explicitly approved component and net map via Konnect MCP tools. Triggers on:
  "photo of a board", "reverse engineer this PCB", "identify components from a
  photo", "scan this PCB photo", "what parts are on this board", "build a
  schematic from a picture of a board", "board photo intake".
argument-hint: "[board photo to reverse engineer]"
---

# KiCAD Photo Intake Workflow

This skill turns board photographs into an **approved review map** — and nothing
more. It never creates or edits a `.kicad_sch` or a `.kicad_pcb`. The approved
map is the handoff artifact; `kicad-schematic-build-agent` builds the schematic
from it with its own toolsets.

A photo scan is a hypothesis, not a netlist. Everything the scan produces is a
proposal a human reads, corrects, and explicitly approves before any KiCad file
is touched. The gate is enforced by the tools, not by good intentions: only
`approve_photo_review_map` can mark a map approved, and any later edit to the
map's reviewed content revokes that approval until it is approved again.

---

## Prerequisites

Component recognition runs in the optional `retrace` Python package. Konnect
never installs it for you.

- Python 3.10 or newer with `retrace` importable:
  `pip install git+https://github.com/ericrihm/retrace.git`
- Interpreter resolution order: the `python_path` argument, then the
  `photo_intake.retrace_python_path` config key, then the `RETRACE_PYTHON`
  environment variable, then PATH discovery.
- Other config keys: `photo_intake.retrace_extras_expected` (extras you expect
  to be present) and `photo_intake.retrace_timeout_seconds` (default 120).

`check_retrace` reports which interpreter won, the `retrace_version` it found,
which optional extras (detection, ocr) imported, and `candidates_tried` — every
interpreter rejected on the way there. It never returns an error for absence:
absence is a reported field.

Pass the same `project_dir` to `check_retrace` that you will pass to
`scan_pcb_photo`. Both read `photo_intake.retrace_python_path` from that
project's config, and a probe run against a different project can name an
interpreter the scan never uses.

Without the ML extras, retrace falls back to an OpenCV contour pass: coarse
labels, `confidence` around 0.5, and no marking, `value` or `part_number` read
at all. That is a usable starting point for a human review and a terrible source
of truth.

---

## Toolset Loading

```
load_toolset('photo_intake')   # check_retrace, scan_pcb_photo, save_photo_review_map, load_photo_review_map, approve_photo_review_map
```

That is the whole surface for intake. Do NOT load a schematic or PCB toolset in
this workflow — placing symbols and wiring nets is the next agent's job, done
from the approved map.

### References by decision

- Read [`references/review-map-schema.md`](references/review-map-schema.md)
  before building or editing a review map: every field, which fields the server
  owns, and which fields the approval hash covers.

---

## Workflow

Each step ends with evidence. A step without its evidence leaves the intake
`INCOMPLETE`; say so rather than proceeding.

0. **Capability** — `check_retrace(python_path, project_dir)`. If `available`
   is false, stop and report the install command and `candidates_tried`; do not
   scan. If either extra is absent, tell the user the scan will be contour-only
   before spending their time on it.

1. **Scan** — `scan_pcb_photo(image_path, project_dir)`. It returns `map_id`,
   the detected components and traces, `pattern_matches`, the path to the raw
   analysis in `analysis_json_path`, `duration_seconds`, `used_fallback`,
   `fallback_evidence`, and — exactly as `check_retrace` reports them — the
   `python_path` that ran and the `candidates_tried` before it. The raw
   analysis is kept under the project as evidence; name its path in your
   report, and say which interpreter produced it when it is not the one the
   user named.

   `used_fallback: true` means marking and `value` were **never attempted** on
   this board: it is true whenever either extra was missing or either fallback
   warning was printed. Do not present an empty value as "no value printed";
   present it as "not read".

2. **Build the review map** — one entry per detected component, carried over
   from the scan, never invented:
   - `component_id` and `bbox_px` copied verbatim, so every row is traceable
     back to the analysis.
   - `ref` is null until a human assigns it.
   - `confidence` copied unchanged. Anything **below 0.6 is flagged for manual
     identification** — list those rows separately for the user. Never round a
     confidence up, hide a low-confidence row, or "clean up" a coarse label.
   - `value` and `part_number` stay empty when retrace read nothing. An empty
     field is the honest answer; a plausible guess is a fabrication that will
     be soldered.
   - `approved: false` on every component. You never set this.
   - `nets` start from what the scan traced, tagged `traced`; anything you or
     the user reason out is `inferred` or `manual`. Tag it honestly.
   - `scale_reference` is **always user-supplied**. Ask for it (board edge in
     mm, or a known package on the board). Never estimate it from pixels.
   - `subcircuit_hints` may carry the scan's `pattern_matches` verbatim. They
     are **advisory and never evidence**: no component is created, typed,
     valued or approved from a hint, and no net is inferred from one.

3. **Persist** — `save_photo_review_map(project_dir, map)`. It writes
   `review_map.json` under the project and returns `saved_path` and the
   server-owned `approved` flag. Saving never grants approval.

4. **User review** — print `saved_path` and hand the review to the user. They
   may edit the JSON file directly; hand edits survive and are the expected
   workflow. Walk them through the low-confidence rows, the unassigned `ref`
   values, and every `inferred` net. Re-`save_photo_review_map` after any edit
   you make on their behalf, then re-read the file.

5. **Approve** — `approve_photo_review_map(project_dir, map_id)`, and **only**
   on the user's explicit approval of that map. Not because the scan finished,
   not because the map exists, not because the rows look plausible. The call
   records `approved_at` and `content_hash_at_approval`; any later edit to the
   components, nets, source images or scale reference revokes it.

6. **Handoff** — delegate to `kicad-schematic-build-agent` with:
   - the map's `saved_path` and its `map_id`,
   - the component and net counts,
   - the instruction to call `load_photo_review_map(project_dir, map_id)` itself
     and to proceed **only when `approval_valid` is true** — not the map's own
     `approved` field, which stays true on a map that was edited after approval,
   - the instruction to place a real library symbol per approved component,
     matched by `type` and `value` against a real library search, and to wire
     the nets from the map's `ref`-to-`ref` connection entries with the
     `sch_wiring` / `sch_batch` tools.

   retrace also emits a synthetic netlist and synthetic KiCad files with
   arrival-order pin numbering. They are never read, never handed on, and never
   used as a pinout.

---

## The approval gate

| Question | Answer |
|---|---|
| What sets `approved` to true? | `approve_photo_review_map`, and nothing else. |
| Does saving approve? | No. A save writes `approved: false` unless the content is byte-identical to what was approved. |
| Does editing revoke? | Yes. Any change to components, nets, source images or the scale reference revokes it. |
| Which field does a consumer read? | `approval_valid` from `load_photo_review_map`, computed by the server — never the map's own flag. |
| What is outside the hash? | `saved_at` and `subcircuit_hints`. A hint can neither grant nor revoke approval. |

---

## Rules

1. **Never mutate a KiCad file in this workflow** — no `.kicad_sch`, no
   `.kicad_pcb`, no library edit. Intake ends at an approved map.
2. **Never approve on the user's behalf** — approval is an explicit human
   decision about a specific map, restated after every edit.
3. **Never guess a value or a part number** — an empty field that says "not
   read" is correct; a plausible invention is not.
4. **Never hide a low-confidence row** — carry `confidence` through unchanged
   and flag everything below 0.6 for manual identification.
5. **Never estimate the scale** — `scale_reference` comes from the user.
6. **Never treat a hint as evidence** — `subcircuit_hints` and
   `pattern_matches` are reading aids, outside the hash and outside the gate.
7. **Never trust retrace's netlist or its generated KiCad files** — nets come
   from the reviewed map, symbols from real libraries.
8. **Report the evidence path** — `analysis_json_path` and `saved_path` belong
   in every summary, so a reviewer can check the claim against the source.
9. **Check `approval_valid`, not `approved`** — on every consumer, every time.
