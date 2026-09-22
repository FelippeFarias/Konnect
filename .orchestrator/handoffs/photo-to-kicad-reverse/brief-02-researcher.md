You are the `researcher` agent of the orc orchestration system. Role: research libraries, APIs, docs and prior art; return cited findings. Every claim carries a source URL or file path. You make NO code changes (tools: read files, run read-only shell commands, web search). Discipline: result first, numbered steps, <=5 items per section, zero filler.

O: Produce a cited research brief (<=1.5 pages) that lets a planner design a "PCB photo -> KiCad replica" workflow integrating ericrihm/retrace into Konnect (this repo: a Rust MCP server for KiCad with skills/agents).

CT:
- Konnect repo root: C:/Users/felip/Documents/FFS-Hardware-Eng/Konnect. Rust workspace; MCP tools live in crates/konnect-core (router/registry.rs lists toolsets; tools shell out to kicad-cli and, in the `integration` toolset, to Freerouting). Skills/agents are markdown under crates/konnect/assets, embedded via crates/konnect/src/manifest.rs and guarded by crates/konnect/tests/asset_references.rs. Bundled agents may only call mcp__konnect__* tools (no Bash), so any external tool must be exposed as a Konnect MCP tool to be usable by them.
- retrace: https://github.com/ericrihm/retrace (MIT, Python >=3.10, v0.3.0). Single-photo pipeline -> AnalysisResult{components[id,label,confidence,bbox px,marking,part_number,value,package], traces[id,points px,width_px,from_component,to_component], board_dimensions px, layer_count_estimate, pattern_matches}. CLI (click): scan --format json|csv|svg -o dir, trace, solve (AC-3), cross-board (15 patterns incl. LDO), identify, learn, export, export-kicad (.net XML, SYNTHETIC pin numbers), export-kicad-pcb (--scale mm/px, placement only, no nets), batch, report-html, compare. Optional extras: [detection] ultralytics/onnxruntime, [ocr] easyocr, [web] gradio.
- Memory to read first: ~/.orc/agents/researcher/MEMORY.md and .orchestrator/memory/researcher.md (project).
- Goal owner intent: user photographs a real board (e.g. an LDO module) from several angles; agents map components, markings, traces, nets, dimensions; then Konnect's KiCad agents rebuild schematic + PCB.

Questions to answer with sources (read retrace source via GitHub raw URLs or `gh api`; use web search for prior art):
1. Exact on-disk output of `retrace scan --format json -o <dir>` (file names, JSON schema incl. bbox units and trace points) and of `solve`, `cross-board --json`, `identify --json`; what `learn` persists and where; whether a `--json` machine mode exists for scan.
2. Windows install path: does `pip install git+...` work on Windows/Python 3.12 without the ML extras; what the OpenCV-only fallback detects (accuracy claims in README/tests); which sample/fixture images ship in the repo or docs (paths) that Konnect tests could reuse; how `scan` behaves with no YOLO/OCR installed.
3. KiCad side: (a) is a legacy `.net` netlist importable into the SCHEMATIC editor in KiCad 10, or only into pcbnew (File -> Import Netlist)? (b) does kicad-cli 10 have any netlist->schematic path? (c) does the KiCad 10 IPC API allow creating tracks/vias/zones (Konnect already has route_trace/add_via/add_zone — confirm in crates/konnect-core) so photo trace polylines can become copper.
4. Prior art for photo/scan -> PCB reconstruction: pcbre (davidcarne), OpenCV-based trace extraction papers/tools, KiCad plugins that import images as reference layers, "PCB reverse engineering to gerber" tools, and how they handle scale calibration (mm/px), top/bottom photo alignment, and inner layers. One line each with URL.
5. Scale and multi-view: known techniques to calibrate mm/px from a photo (ruler, known package size e.g. 0805 = 2.0x1.25 mm, board edge length) and to register top/bottom photos (mirror + fiducials). Cite.

OF: Write .orchestrator/handoffs/photo-to-kicad-reverse/02-researcher.md with this exact frontmatter and sections:
---
change: photo-to-kicad-reverse
task: INTAKE research brief
agent: researcher
verdict: DONE | BLOCKED
failing_layer: n/a
---
## Result   (numbered findings 1-5 mirroring the questions, each with URL/file path)
## Evidence (commands run + key output lines)
## For the next agent (planner): recommended integration shape, hard limits of retrace, risks)
## Deferred findings ((none) or one line each)
Also append role lessons (if any) to ~/.orc/agents/researcher/MEMORY.md (<=60 lines body) and project facts to .orchestrator/memory/researcher.md.

TG: [Read, Bash (read-only: gh api, curl, ls, cat, grep), web search]
TB: [Edit, Write outside the two handoff/memory files, git push, pip install]
BU: research ~150k tokens; report BLOCKED in the handoff instead of grinding past it.
FS: .orchestrator/handoffs/photo-to-kicad-reverse/02-researcher.md, .orchestrator/memory/researcher.md, ~/.orc/agents/researcher/MEMORY.md
SC: (1) handoff file exists with verdict DONE and 5 numbered findings each carrying a URL or path; (2) question 3(a) answered with a KiCad doc citation; (3) at least one reusable fixture image path or a stated absence.
