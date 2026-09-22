# analyst — project memory

## Current state
`photo-to-kicad-reverse` change: wrote `.orchestrator/handoffs/photo-to-kicad-reverse/01-analyst.md` (requirements brief for retrace -> Konnect photo-to-KiCad workflow, verdict DONE). A `02-researcher.md` brief (retrace JSON schemas, Windows install, KiCad netlist-import reality) is expected alongside it — consult it before re-deriving retrace CLI/schema facts.

## Decisions affecting my role
- Konnect's external-tool integration pattern: subprocess wrapper (typed Rust struct from JSON) + MCP tool registration in `crates/konnect-core/src/router/registry.rs`, following `tools/cli.rs` (kicad-cli) and `freerouting_mcp.rs`+`tools/integration.rs` (Freerouting). Any new external CLI (e.g. `retrace`) should follow this same shape — bundled agents have no Bash, only `mcp__konnect__*`.
- Mutation-ownership rule (from `crates/konnect/assets/skills/konnect/SKILL.md`): one agent owns a complete task boundary; don't split vision-intake and KiCad-mutation across a single agent — route through separate agents that hand off a saved/approved artifact.

## Gotchas found here
- `.orchestrator/handoffs/photo-to-kicad-reverse/brief-02-researcher.md` (note the `brief-` prefix) holds the *task prompt* for the researcher agent, not its finished output — the actual result lands at `02-researcher.md` once that agent completes. Don't mistake the prompt file for the deliverable.
