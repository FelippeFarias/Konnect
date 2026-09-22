---
change: konnect-orchestrator
task: make design.md an implementable, code-verified design (tasks.md consistent), ending in a hardened Pre-mortem
agent: architect
verdict: DONE
---

## Result

READY for implementation. No remaining concern blocks a task (the one open
question, sub-agent access to skill `references/`, is designed around —
see Deferred findings 1).

1. `openspec/changes/konnect-orchestrator/design.md` rewritten against the
   merged tree (`git merge-tree --write-tree ee55a12 1653e79` → `a4351038`,
   clean): D1–D12 + "Implementation order" + a 14-cause `## Pre-mortem`,
   each cause naming its clause and task. Contract fixes vs the planner:
   `flow` is the 23rd toolset (not 21st); records must be supplied **in the
   same `flow_advance` call** (the planner's "or already on disk" was a
   stale-pass bug); rewind/abandon/close transitions added (forward-only
   could not represent the FIX loop and bricked `flow_start` after any
   abandoned job); a gate binds to design hash **and** a package hash over
   the records shown, checked at approval (vs the gate-entry values) and at
   leaving; `flow_status(read)` is how mcp-only agents read records;
   `flow_log` kinds `lesson`/`handoff` give every D2 file exactly one writer;
   D6 evidence check moved inside `flow_advance` (the observer ring holds
   100 calls); autonomous mode specified; JSON front matter (no YAML crate).
2. `tasks.md`: still 32 tasks, re-cut — round 1 = ten `flow` tasks
   (1.1–1.7 sequential in `flow.rs`, 1.8 registry, 1.9 e2e through compiled
   schemas, 1.10 counts); round 2 = two companion skills; round 3 = konnect
   references then router; rounds 4–7 as before plus a manifest-completeness
   test (6.1) and enrollment of flow-calling agents in
   `agents_make_claimed_evidence_executable` (6.3).
3. Round 3 YAGNI verdicts (design D9): `kicad-requirements` **merged** into
   `kicad-architecture`; `kicad-architecture` **kept**; `kicad-sourcing`
   **cut** (method already in `kicad-manufacture`/`kicad-review`/
   `kicad-schematic`; parts-list shape in `architecture-record-schema.md`);
   `kicad-curator` **kept**, SKILL.md only. Deciding fact: Codex installs
   skills only, never agents (`install.rs:239`, `:276`).
4. Specs updated to match: `specs/flow-toolset/spec.md` (12 requirements),
   `specs/agent-roster/spec.md` (two skills, manifest guard, block-list
   frontmatter), `specs/hardware-orchestration/spec.md` (evidence check,
   FIX-layer rewind mapping, Codex rule). `proposal.md` What Changes/Impact
   aligned.
5. Memory: global role file compressed (under the cap) with three lessons
   folded in; project memory state/decisions/gotchas overwritten.

## Evidence

1. `openspec validate --changes konnect-orchestrator --json` → `"valid": true`, `"issues": []`; `orc validate konnect-orchestrator` → `Change 'konnect-orchestrator' is valid`.
2. `grep -c "^- \[ \]" tasks.md` = 32, `grep -c "^Stack: none"` = 32, `grep -c "^Acceptance"` = 32; no Acceptance line has more than three `; `-separated criteria (awk check, empty output).
3. `design_state_hash` cost, same algorithm in a Python probe: `Load-Cell-Digitizer/kicad` 21 files / 2,220,310 bytes → 16.4, 10.5, 9.2 ms; `display-remoto` 53 files / 23,858,760 bytes → 180.1, 69.3, 77.5 ms. Of display-remoto's 53 covered files, 12 are under `.history/` and 27 in two `backup-*` folders; `~display-remoto.kicad_pro.lck` exists beside the project file.
4. Sweep simulation of `doc_tool_counts`' two noun checks over every `.md` in the change directory → 0 hits (this handoff was written to the same rule).
5. Merged-tree facts cited in design's Context table (e.g. `registry.rs:22`/`:120`, `writer.rs:176`/`:397`, `observability.rs:27`, `asset_references.rs:151`/`:175`/`:746`, `doc_tool_counts.rs:165`/`:225`) were read with `git show a4351038:<path>`.

## For the next agent

1. **Implementation order**: design.md "Implementation order" — 1.1→1.7
   (one file, TDD), 1.8 register, 1.9 e2e, 1.10 counts immediately (doc
   tests go red the moment `flow` registers), then skills → konnect
   references → router → agents → existing agents → 6.1–6.4 → 7.x. Asset
   tasks before 6.2 run only the targeted tests their Acceptance names; the
   phantom-name test cannot pass until 6.2.
2. **Most likely done wrong #1 — the same-call record rule.** Do not
   "helpfully" accept a record already on disk; task 1.4's acceptance plants
   one on purpose. Also: only `flow_start` creates `.konnect/flow/`, and a
   refusal writes nothing (validate fully before any side-file write inside
   the `transact_atomic` closure).
3. **Most likely done wrong #2 — the gate keys.** Approval compares against
   the keys stored in the history entry that **entered** the gate
   (`design_hash` + `package_hash`), not "whatever the hash is now"; leaving
   compares against the approval and its `visit`. `package_hash` is derived
   from the job's sequence (records of phases since the previous gate), not
   a per-gate table.
4. **Most likely done wrong #3 — agent frontmatter and flow loading.**
   `skills:`/`tools:` must be block lists (`  - name`); a flow list parses as
   empty and fails `agents_preload_existing_skills`. Every agent that says
   `flow_advance` must also say `load_toolset("flow")` (task 6.3 guards it),
   and every flow step is conditional on the brief naming a `job_id`.
5. Decisions not to re-litigate: planner's "agent records its own phase"
   kept, refined to "the producer records it" (the session records only a
   merged multi-reviewer ledger, a phase it ran itself under Codex, gate
   exits and rewinds); no flow tool writes under the user's home (global
   `MEMORY.md` is promoted by the session's own editor); no cache on
   `design_state_hash` (content key every call; an mtime key is the bug);
   rejected: YAML front matter via a new crate, forward-only transitions,
   a seventh tool, keeping all four companion skills.

## Deferred findings

1. Existing agents limited to `mcp__konnect__*` are told to read skill `references/` (layout agent merged `:27-32`), and `pcb-photo-intake-agent` both instructs reading two references (`:31-34`) and forbids `Read` of anything but the source photos and its own views (Hard Rule 6, `:195-198`) — verify in Claude Code whether a sub-agent without `Read` can open a preloaded skill's references; if not, the existing agents run without their gate details. The new assets do not depend on it (design D9).
2. `design_state_hash` covers editor-history and backup copies (display-remoto: 39 of 53 covered files), so a new backup forces a re-approval; narrowing coverage to the live project tree deserves its own change.
3. After this change `now_rfc3339_utc` (`photo_intake.rs:1159`) serves two modules through a `pub(crate)` widening; move the time helpers to a shared module at the next drift review.
4. `CallRecord` (`observability.rs:63`) carries no arguments and no caller identity, so no call-log check can tell which agent or project a call served; the doc's phase-3 "log-verified handoff" needs that first.
