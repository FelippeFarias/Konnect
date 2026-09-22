# researcher — project memory

## Current state
Completed INTAKE research brief for change `photo-to-kicad-reverse` (retrace + Konnect photo-to-KiCad workflow). Handoff: `.orchestrator/handoffs/photo-to-kicad-reverse/02-researcher.md` (verdict DONE). Covers: retrace's exact scan/solve/cross-board/identify/learn output schemas and persistence paths, Windows install viability (plausible, unverified by upstream CI which is ubuntu-only), OpenCV-fallback accuracy caveats, absence of real-photo test fixtures (only synthetic docs/examples/*.png, real iFixit photos are CC BY-NC-SA and not reusable), confirmation that KiCad netlist import is pcbnew-only (never eeschema, no kicad-cli path either), confirmation that Konnect already has `route_trace`/`add_via`/`add_zone`/`set_component_placements` over KiCAD IPC, and prior-art/calibration/registration research (pcbre, lukasloetkolben/pcbRE, Kleber et al. papers).

## Decisions affecting my role
- Prior handoff `01-analyst.md` already set defaults (scale calibration always user-supplied, single-photo-per-run only in v1, hard human-review gate before any KiCad mutation, retrace's synthetic pin numbers never trusted). My research confirmed all of these are technically necessary, not just conservative choices.

## Gotchas found here
- `retrace`'s learning/cross-board/learned-component state all write to `~/.local/share/retrace/*.json` (OS user home dir, no env override) on every run — a per-project Konnect wrapper must scope `HOME`/`USERPROFILE` per subprocess call or accept cross-project state pollution.
- `AnalysisResult.board_dimensions` is never persisted into `analysis.json` — don't expect it there; read the source image's own pixel dimensions instead.
- retrace repo (ericrihm/retrace) is small/low-star (3 stars at research time) and solo-maintained — treat version-specific behavior (CLI flags, JSON schema) as needing a re-check if the planner/dev picks it up much later, since it's evolving fast (v0.3.0, 1905 tests, 33 modules per its own README stats badge).
