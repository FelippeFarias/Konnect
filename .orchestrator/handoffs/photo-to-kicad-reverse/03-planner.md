---
change: photo-to-kicad-reverse
task: Create OpenSpec change artifacts for Slice 0 + Slice 1
agent: planner
verdict: DONE
failing_layer: n/a
---

## Result

Created the OpenSpec change `photo-to-kicad-reverse` (Slice 0 + Slice 1 only; Slices 2-3 explicitly deferred) with all four artifacts, validated via `openspec validate --changes photo-to-kicad-reverse --json` (`valid: true`, 0 issues):

1. `openspec/changes/photo-to-kicad-reverse/proposal.md` — why/what/capabilities/impact, user stories 1-4, non-goals, Slice 0/1 success metrics verbatim from the analyst brief.
2. `openspec/changes/photo-to-kicad-reverse/specs/photo-intake/spec.md` — new capability `photo-intake`, 5 requirements (retrace probe, typed project-scoped scan, persisted review map, hard approval gate, schematic-build handoff), each with 3-4 scenarios.
3. `openspec/changes/photo-to-kicad-reverse/design.md` — module layout (`tools/photo_intake.rs`), the 5-tool `photo_intake` toolset with input/output schemas, config keys, error contract, fixture strategy, review-map JSON schema, new skill/agent, asset-guard implications, all 6 analyst open questions resolved, and a `## Pre-mortem` placeholder for @architect.
4. `openspec/changes/photo-to-kicad-reverse/tasks.md` — 26 tasks across 7 groups (subprocess+config+probe; scan tool+parsing; review-map+approval; toolset registration; skill/agent/manifest wiring; docs; final fmt/clippy/test gate), worktree/branch noted in the header.

## Evidence

- `openspec status --change photo-to-kicad-reverse --json` → all 4 artifacts `"status": "done"`.
- `openspec validate --changes photo-to-kicad-reverse --json` → `"valid": true`, 0 errors/warnings/info.
- `grep -c "^Stack: none" tasks.md` = 26; `grep -c "^- \[ \]" tasks.md` = 26; `grep -c "^Acceptance:" tasks.md` = 26 — one-to-one.
- Design grepped against real repo patterns before writing: `crates/konnect-core/src/router/registry.rs` (`ALL_TOOLSETS`/`build_tools_for`), `crates/konnect-core/src/tools/cli.rs` (kicad-cli subprocess pattern, `cli_failure_diagnostics`), `crates/konnect-core/src/freerouting_mcp.rs` + `crates/konnect-core/src/tools/integration.rs` (`check_freerouting` graceful-absence shape, `tool!` macro, `ToolDef`), `crates/konnect-core/src/tools/config.rs` (`default_user_config()` is a plain JSON document, not a fixed struct — used for the new `photo_intake` config keys), `crates/konnect/tests/asset_references.rs` (all 6 mechanical checks it enforces, addressed one-for-one in design D10), `crates/konnect/assets/agents/kicad-pcb-layout-agent.md` (frontmatter shape), `crates/konnect/src/manifest.rs` (`include_str!` embedding pattern).

## For the next agent

- Implementation runs in worktree `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\photo-to-kicad-reverse` (branch `orc/photo-to-kicad-reverse`, base `ea74faf`) — stated in `tasks.md`'s header.
- Design D5's content-hash approval-state machine is the load-bearing safety mechanism (spec "hard human-review gate") — implement it exactly as specified (`save_photo_review_map` always resets `approved: false` unless content hash matches `content_hash_at_approval`); do not simplify it to a client-supplied flag.
- Design D7's fixture strategy avoids any network fetch or CC BY-NC-SA-licensed image: synthesize a test PNG in-test and use a hand-written `analysis.json` fixture for parsing tests; only the `#[ignore]`-gated integration test (task 2.4) needs a real `retrace` install.
- `kicad-schematic-build-agent.md` is explicitly **not modified** by this change (design D6 step 4) — it already satisfies `asset_references.rs`'s evidence-boundary test; only the new `pcb-photo-intake-agent` and `kicad-photo-intake` skill are additions.
- Design's "Open Questions" section flags one deferred question for Slice 2 planning: whether `scale_reference` should be validated against matched-footprint real dimensions at approval time.

## Deferred findings

(none)
