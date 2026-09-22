## Why

Konnect can already build, wire, lay out, and review a KiCad design tool by
tool, but nothing conducts a whole board from a one-line request to a
fabrication package: the session picks agents ad hoc, nobody enforces that
architecture is approved before schematic work starts, nothing ties a
placement approval to the exact design it was shown, and nothing survives a
new session restarting mid-board except whatever the human remembers to
repeat. `docs/ORQUESTRADOR_DO_KONNECT.html` (2026-09-21, decisions answered
same day) analyzed `orc`'s own orchestrator — root router, contracted agent
roster, phased flow with gates, single-writer state, capped verification,
layered memory — and found that the concepts translate to hardware even
though the code cannot (no worktrees/merge queue: one KiCad project has one
owner; no "never ask": architecture, placement, and purchase are the user's
calls; OpenSpec's own artifacts do not fit a board — a board revision needs a
constraint record and a worst-case ledger, not a spec delta).

This change builds phases "1 · Mestre e estado" and "2 · Elenco" of the
doc's proposed order: a `flow` MCP toolset inside `konnect.exe` that is the
only writer of per-project orchestration state, an orchestrator router added
to the existing `konnect` skill (entry point `/konnect <request>`), and all
six new bundled agents the roster calls for (requirements, architecture,
sourcing, library, manufacture, curator) alongside the three that already
exist (schematic, layout, review) and the two merged-but-unreleased photo
agents (`pcb-photo-intake-agent`, `pcb-design-reconstruction-agent`). Phase
"3 · Garantias" beyond the hash-bound approvals already folded in here (the
one piece phase 3 needs that phase 1 also needs), phase "4 · Aprender"'s
brain link, and "5 · Depois" stay future changes — see Non-Goals.

The seven decisions in the doc's "Decisões" section were answered by the user
on 2026-09-21 and are binding inputs to this change, not open questions:
entry point `/konnect <request>`; guided mode by default with an optional
per-job autonomous mode that still stops at the purchase gate; the engine is
a new Rust `flow` toolset bound to the existing, currently-unused
`design_state_hash`; state lives in `<project>/.konnect/flow/`; role memory
starts at `~/.konnect/agents/<role>/MEMORY.md` until the brain exists; all
six new agents ship in this change; and the change itself is built as an
OpenSpec change run through `orc`.

## What Changes

- **Add the `flow` MCP toolset** (`crates/konnect-core/src/tools/flow.rs`,
  registered in `router/registry.rs`): `flow_status`, `flow_start`,
  `flow_advance`, `flow_gate`, `flow_log`, `flow_defer`. State under
  `<project>/.konnect/flow/` (`STATE.md`, `records/`, `handoffs/<job>/`,
  `log/<date>-<job>.md`, `memory/<role>.md`) is written only through these
  six tools — no agent or session ever hand-writes a file under
  `.konnect/flow/`, and agents read their input records through
  `flow_status`. Approvals recorded by `flow_gate` are bound to
  `design_state_hash` (`crates/konnect-core/src/design_hash.rs`) and to a
  hash of the records the user was shown; `flow_gate` refuses to approve a
  design or package that changed after it was produced, and `flow_advance`
  refuses to leave a phase without its records (supplied in the same call)
  or to leave a gate whose approval no longer matches. Rewinds (the FIX
  loop), abandoning a job and an optional autonomous mode that still stops
  at the purchase gate are part of the contract.
- **Add an orchestrator router to the `konnect` skill** and a new reference
  file, `references/orchestration.md` (the 10-phase flow, the phase → record
  table, the three fixed gates, the lane table including "call one agent
  alone", the three-round verification cap, the readiness gate before the
  schematic phase, and guidance — prose, not code — to cross-check a
  handoff's claimed tool calls against `get_recent_calls` before accepting a
  `DONE` verdict). Bounded edits keep their existing direct lane, unchanged.
  New brief and handoff reference templates (`references/brief-template.md`,
  `references/handoff-template.md`) mirror `orc`'s O/CT/OF/TG/TB/BU/FS/SC
  brief and DONE/FIX/BLOCKED handoff, `failing_layer` renamed to Konnect's
  own three layers (requirement, architecture, implementation).
- **Add six new bundled agents**: `kicad-requirements-agent`,
  `kicad-architecture-agent`, `kicad-sourcing-agent`, `kicad-library-agent`,
  `kicad-manufacture-agent`, `kicad-curator-agent` — each with the orc-style
  contract (triggers, anti-triggers, `tools: [mcp__konnect__*]`, `model:
  sonnet`, an explicit `maxTurns`, and `skills:` naming only bundled skills).
  Two new companion skills carry the methods no existing skill has:
  `kicad-architecture` (requirements and architecture records, the
  readiness line, the parts-list procedure) and `kicad-curator` (lesson
  triage). `kicad-sourcing-agent`, `kicad-library-agent` and
  `kicad-manufacture-agent` preload existing skills (`kicad-manufacture`,
  `kicad-review`, `kicad-library`) unchanged — see design D9 for why the
  planned `kicad-requirements` and `kicad-sourcing` skills were merged and
  cut.
- **Route every agent from the top-level `konnect` skill**, including the
  two merged-but-unreleased photo agents this change assumes are already on
  `main` by implementation time (`pcb-photo-intake-agent`,
  `pcb-design-reconstruction-agent`, from `orc/board-dossier-reconstruction`)
  — required by the existing `top_level_skill_routes_every_bundled_agent`
  test, which fails for any bundled agent the router does not name.
- **Wire the new assets**: `crates/konnect/src/manifest.rs` (`include_str!`
  for the 2 new skills + their references + 6 new agents + the 3 new
  konnect reference files) with a new test that fails when an asset has no
  manifest entry; `crates/konnect/tests/asset_references.rs` (flagged
  non-tool names in `NOT_TOOLS`; the flow-calling agents enrolled in
  `agents_make_claimed_evidence_executable`); and the documents
  `crates/konnect/tests/doc_tool_counts.rs` sweeps (README.md, DEV.md,
  tool-directory.md, docs/TROUBLESHOOTING.md, packaging/metadata.json,
  plugin/plugin.json) for the `flow` toolset's six tools and one new
  toolset slot, counted from the merged base; historical records
  (`.orchestrator/`, archived changes) leave that sweep instead of being
  rewritten.

## Capabilities

### New Capabilities
- `flow-toolset`: the six-tool MCP toolset, its `.konnect/flow/` state
  layout, and the hash-bound gate/refusal contract.
- `hardware-orchestration`: the `konnect` skill's orchestrator router, the
  orchestration reference, the brief/handoff templates, the lane table, the
  verification cap, and the readiness gate.
- `agent-roster`: the six new bundled agents' contract shape and their
  routing, plus routing the two merged photo agents.

### Modified Capabilities
(none — `photo-intake` is untouched by this change; it is only routed by
name from the `konnect` skill, which this change also updates for its own
new agents)

## Impact

- **Code:** `crates/konnect-core/src/tools/flow.rs` (new),
  `crates/konnect-core/src/tools/mod.rs` (`pub mod flow;`),
  `crates/konnect-core/src/tools/photo_intake.rs` (one visibility change:
  `now_rfc3339_utc` becomes `pub(crate)`),
  `crates/konnect-core/src/router/registry.rs` (new `ALL_TOOLSETS` entry +
  `build_tools_for` arm), `crates/konnect-core/src/router/mod.rs` (an
  ordered-membership test); `crates/konnect-core/src/design_hash.rs` and
  `crates/konnect-sexp/src/writer.rs` are consumed, not modified.
- **Assets:** `crates/konnect/assets/skills/konnect/SKILL.md` +
  `references/orchestration.md` + `references/brief-template.md` +
  `references/handoff-template.md` (new); `crates/konnect/assets/skills/
  kicad-architecture/**`, `kicad-curator/**` (new);
  `crates/konnect/assets/agents/kicad-requirements-agent.md`,
  `kicad-architecture-agent.md`, `kicad-sourcing-agent.md`,
  `kicad-library-agent.md`, `kicad-manufacture-agent.md`,
  `kicad-curator-agent.md` (new); the schematic, layout and review agents
  gain job-only flow steps (frontmatter unchanged).
- **Manifest/install:** `crates/konnect/src/manifest.rs` (all `include_str!`
  registrations); `crates/konnect/src/install.rs` gains only a test (the
  manifest completeness guard) — it already installs whatever `manifest.rs`
  lists.
- **Docs:** README.md, DEV.md, tool-directory.md, docs/TROUBLESHOOTING.md,
  packaging/metadata.json, plugin/plugin.json (tool/toolset counts).
- **Tests:** `crates/konnect/tests/asset_references.rs`,
  `crates/konnect/tests/doc_tool_counts.rs` (`SKIP` gains the historical
  record folders), unit tests in `crates/konnect-core/src/tools/flow.rs`,
  and `crates/konnect-core/tests/flow_gate_e2e.rs` driving the approval
  chain through the published schemas.
- **Non-Goals (this change — see design.md for the full list with
  rationale):** `konnect-vcs` checkpoints (the crate is an unwired
  scaffold); a Codex cross-reviewer; brain integration (the curator writes
  lesson candidates locally only — no read or write to a brain that does not
  exist yet); a firmware-contract hand-off to `orc`.
