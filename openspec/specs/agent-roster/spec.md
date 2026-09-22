# agent-roster Specification

## Purpose
TBD - created by archiving change konnect-orchestrator. Update Purpose after archive.

## Requirements

### Requirement: six new bundled agents with the standard contract
The system SHALL ship six new bundled Claude agents —
`kicad-requirements-agent`, `kicad-architecture-agent`,
`kicad-sourcing-agent`, `kicad-library-agent`, `kicad-manufacture-agent`,
`kicad-curator-agent` — each with frontmatter naming `description` (stating
its triggers), `model: sonnet`, an explicit `maxTurns`, `tools:` limited to
`mcp__konnect__*`, and `skills:` as a block list naming only bundled skills
that exist in this change or already existed before it. An agent SHALL call
a `flow_*` tool only when its brief names a flow job.

#### Scenario: every new agent preloads real bundled skills
- **WHEN** `agents_preload_existing_skills` runs against the six new agent
  files
- **THEN** every skill each one's frontmatter names resolves to a real
  bundled skill directory with a `SKILL.md`

#### Scenario: every new agent declares an explicit turn budget
- **WHEN** any of the six new agent files is read
- **THEN** its frontmatter states a `maxTurns` value, not the tool default

#### Scenario: an agent that records a phase loads the flow toolset
- **WHEN** `agents_make_claimed_evidence_executable` runs
- **THEN** every agent that prescribes `flow_advance` also prescribes
  `load_toolset("flow")`

### Requirement: two new companion skills
The system SHALL ship two new bundled skills: `kicad-architecture`, with a
`SKILL.md` and the references `constraint-record-schema.md` and
`architecture-record-schema.md` describing the records its agents write,
and `kicad-curator`, a `SKILL.md` holding the lesson-triage rule and the
candidate format. `kicad-sourcing-agent`, `kicad-library-agent` and
`kicad-manufacture-agent` SHALL draw their method from existing skills
(`kicad-manufacture`, `kicad-review`, `kicad-library`) plus
`kicad-architecture`'s parts-list section, without a skill of their own.

#### Scenario: a new skill's reference is reachable from its SKILL.md
- **WHEN** `every_reference_is_reachable_from_its_parent_skill` runs
  against `kicad-architecture`
- **THEN** each reference file under its `references/` directory is named in
  its `SKILL.md`

#### Scenario: the record rules an agent needs are preloaded
- **WHEN** a new agent that writes a record is started with its preloaded
  skills
- **THEN** the required sections of that record are stated in a preloaded
  `SKILL.md` body or in the agent file, not only in a reference file

### Requirement: every bundled asset is installed
The system SHALL register every bundled skill, skill reference and agent
file in the install manifest, and a test SHALL fail when an asset file
exists without a manifest entry.

#### Scenario: a forgotten manifest entry fails the build's tests
- **WHEN** an agent file exists under `crates/konnect/assets/agents/`
  without a matching `AGENTS` entry
- **THEN** the manifest completeness test fails naming the file

### Requirement: every bundled agent is routed from the top-level skill
The system SHALL name every bundled agent's file stem in the `konnect`
skill's text, including the six agents this change adds and the two
photo agents (`pcb-photo-intake-agent`, `pcb-design-reconstruction-agent`)
present on the branch point.

#### Scenario: no bundled agent is left unrouted
- **WHEN** `top_level_skill_routes_every_bundled_agent` runs after this
  change's edits to `konnect/SKILL.md` land
- **THEN** every agent file under `crates/konnect/assets/agents/` — the
  eleven that exist once this change and the merged photo agents are both
  present — has its file stem named somewhere in `konnect/SKILL.md`'s text

### Requirement: existing agents gain a flow-record responsibility, unchanged otherwise
The system SHALL extend `kicad-schematic-build-agent`,
`kicad-pcb-layout-agent`, and `kicad-design-review-agent` with documented,
job-only steps that read their input records through `flow_status` and call
`flow_advance` with the records each produces (`schematic-evidence.md`;
`placement.md`/`routing.md`; `ledger-schematic.md`/`ledger-prefab.md` in
single-reviewer mode), without changing any of their existing triggers,
anti-triggers, tools, model, or `maxTurns`.

#### Scenario: schematic build states its flow-record step
- **WHEN** `kicad-schematic-build-agent.md` is read after this change
- **THEN** it names calling `flow_advance` with `schematic-evidence.md` in
  a step after Step 8 that applies only when the brief names a job, and its
  frontmatter (`model`, `tools`, `maxTurns`, `skills`) is unchanged from
  before this change

#### Scenario: the layout agent still stops after placement
- **WHEN** `kicad-pcb-layout-agent.md` is read after this change
- **THEN** its placement step records `placement.md` with `flow_advance`
  and returns without routing in the same run
