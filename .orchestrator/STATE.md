# orc STATE — Konnect

phase: IDLE
active_change: install-scripts
current_task: merge of install-scripts awaits the user's GRANT
in_flight: none
blockers: GRANT: orc state grant-merge install-scripts
next_step: after the GRANT: orc merge install-scripts with the full battery; history FAST line
stacks: none

## sessions
- orc-konnect-session-8c29 | install-scripts | FAST | C:\Users\felip\.orc\worktrees\konnect-3b7e2022\install-scripts | 2026-09-22T12:36:36.955Z

## queue
- photo-to-kicad-reverse slice 2: trace extraction + scale calibration + net inference + PCB placement (>=90% net match on LDO reference board)
- photo-to-kicad-reverse slice 3: top+bottom photos -> F.Cu/B.Cu, angled photos for identification only, routing of confirmed nets, DRC-clean two-layer reference board
- slice 2 planning input (real-photo test 2026-09-18): retrace trace extraction is bare-copper-only (mask-covered traces invisible) and its YOLO default is stock yolov8n.pt (COCO); slice 2 must not depend on retrace nets — plan mask-aware segmentation or manual net entry, and a PCB-trained detector or per-component-type heuristics (LED domes) for identification
- reference-knowledge-base: APPLY openspec/changes/reference-knowledge-base (17 tasks; pilot evidence in .kb-pilot/, 39/39 + 15/15)
- reference-knowledge-base: HOLD — supersedes the APPLY line above; wait for the user's answers on docs/CEREBRO_DO_KONNECT.html, then revise the change into the brain design
- konnect-orchestrator follow-up: let MCP-only bundled agents read their preloaded skills' references/ (pcb-photo-intake-agent already carries Read) — from architect deferred finding 1
- konnect-orchestrator follow-up (Non-Goal 1): konnect-vcs checkpoints before each mutating phase, restore via recovery branch
- konnect-orchestrator follow-up (Non-Goal 2): Codex cross-reviewer for the review phases
- konnect-orchestrator follow-up (Non-Goal 3): curator writes to the brain once CEREBRO phase 3 exists
- konnect-orchestrator follow-up (Non-Goal 4): firmware contract handed to orc (esp32-cpp stack)

## Pending approvals
- ACTION: upgrade codex CLI (npm i -g @openai/codex) so codex-executor roles (researcher/qa/reviewer/design-director) work / WHY: codex exec fails: model gpt-6-astra requires a newer Codex CLI than 0.144.6 / BLAST RADIUS: global npm package on this machine / ROLLBACK: npm i -g @openai/codex@0.144.6
- ACTION: merge orc/photo-to-kicad-reverse (3d77ed5, base ea74faf) into the trunk of FelippeFarias/Konnect and push (GRANT via: orc state grant-merge photo-to-kicad-reverse from a plain shell) / WHY: change photo-to-kicad-reverse verified DONE by qa and reviewer after 2 fix rounds; merging and pushing are Tier 2 / BLAST RADIUS: trunk history of the fork; nothing upstream / ROLLBACK: git revert the merge commit; branch orc/photo-to-kicad-reverse stays
- ACTION: push branch orc/photo-to-kicad-reverse to origin (fork) for review/backup before merge / WHY: work exists only in the local worktree; pushing is Tier 2 / BLAST RADIUS: new remote branch on the fork / ROLLBACK: git push origin --delete orc/photo-to-kicad-reverse
- ACTION: merge orc/board-dossier-reconstruction (df870a1, base 1815948 = head of orc/photo-to-kicad-reverse) into main after photo-to-kicad-reverse; then push (GRANT via: orc state grant-merge board-dossier-reconstruction) / WHY: change verified DONE by qa and reviewer after 1 fix round; stacked on the still-unmerged photo-to-kicad-reverse branch / BLAST RADIUS: trunk history of the fork / ROLLBACK: git revert the merge commit
- ACTION: copy the ESP32 collection originals to ~/.konnect/knowledge/esp32/originais (read-only) as the home of the productized knowledge toolset / WHY: the pilot reads them in place from Downloads, which is fragile; the product needs a stable per-user location outside the git repo (licences, 850 MB); writing outside the project is Tier 2 / BLAST RADIUS: about 850 MB of new files under the user's home; nothing in the repo / ROLLBACK: delete ~/.konnect/knowledge/esp32
- GRANT merge board-dossier-reconstruction trunk:main sha:ee55a1213a73a10026400b6e3b2ad92cdfa22e23 at:2026-09-22T03:06:35.476Z by:felip@ffsdevelopers
- ACTION: merge orc/board-dossier-reconstruction into main / WHY: the main working tree has uncommitted changes orc did not create, and orc never stashes or resets them / BLAST RADIUS: none taken — the merge was not attempted / ROLLBACK: commit or stash by hand, then re-run `orc merge`
- ACTION: merge orc/konnect-orchestrator into main via orc merge konnect-orchestrator (GRANT: orc state grant-merge konnect-orchestrator from a plain shell), then push when the user decides / WHY: change konnect-orchestrator verified DONE by qa and reviewer at verification round 3 after two fix rounds; merging to main is Tier 2 / BLAST RADIUS: trunk history of the FelippeFarias/Konnect fork; nothing upstream / ROLLBACK: git reset --hard bbd5efb on main (or revert the merge); branch orc/konnect-orchestrator stays
- GRANT merge konnect-orchestrator trunk:main sha:bbd5efb76ba1c2089b862caea12982fab0793858 at:2026-09-22T11:28:24.366Z by:felip@ffsdevelopers
- ACTION: install the merged build: rename ~/.konnect/bin/konnect.exe to konnect-0.12.0-2026-09-21.exe.bak, copy target/release/konnect.exe (built from main 9a0f1ce) into ~/.konnect/bin, run konnect init for Claude, restart Claude Code / WHY: the MCP server Claude Code runs is the 2026-09-21 build without the flow toolset, the orchestrator router or the six new agents / BLAST RADIUS: ~/.konnect/bin/konnect.exe and the Konnect skills, agents and hooks under ~/.claude on this machine / ROLLBACK: restore the .bak exe and re-run its konnect init
- ACTION: merge orc/install-scripts into main via orc merge install-scripts (GRANT: orc state grant-merge install-scripts from a plain shell) / WHY: fast lane verified DONE by qa 05 and reviewer 06; DECISION O post-verify fix verified by the orchestrator / BLAST RADIUS: trunk history of the fork (adds scripts/install.ps1 and scripts/install.sh); nothing upstream / ROLLBACK: git reset --hard 9a0f1ce on main (or revert the merge)
- GRANT merge install-scripts trunk:main sha:9a0f1ced35679982449270b6b03815dcc310655d at:2026-09-22T14:55:58.828Z by:felip@ffsdevelopers

## Deferred findings
- pcbre (davidcarne/pcbre) scale-calibration/top-bottom registration algorithm undocumented; source not inspected (source: researcher, change: photo-to-kicad-reverse)
- retrace export --format spdx/cyclonedx and sbom JSON schemas not investigated (source: researcher, change: photo-to-kicad-reverse)
- .gitignore lacks entries for .venv-retrace/ and the .konnect/ runtime data dir the photo_intake tools will write under project_dir (source: orchestrator, change: photo-to-kicad-reverse)
- crates/konnect-core/src/tools/library.rs:1818 portable_uri strips \?\ and breaks UNC paths (pre-existing) (source: reviewer, change: photo-to-kicad-reverse)
- crates/konnect-core/src/tools/config.rs:111 read_config silently substitutes defaults when config.json is unparseable (pre-existing) (source: reviewer, change: photo-to-kicad-reverse)
- CONTRIBUTING.md names four files that move together while doc_tool_counts.rs sweeps the whole repo (source: reviewer, change: photo-to-kicad-reverse)
- crates/konnect/tests/asset_references.rs NOT_TOOLS carries nine names that are real top-level tool parameters (project_dir, lib_id, pin_x, pin_y, footprint_path, new_number, match_all, replace_existing, sheet_instance_path), making the phantom guard a no-op for each (pre-existing) (source: qa, change: photo-to-kicad-reverse)
- asset_references.rs photo-intake NOT_TOOLS comment claims project_dir is absent from the array while it is present elsewhere in the same array (source: qa, change: photo-to-kicad-reverse)
- crates/konnect/tests/asset_references.rs agents_make_claimed_evidence_executable does not check kicad-schematic-build-agent.md's new 'approval_valid' consumer section; add the marker (source: developer, change: photo-to-kicad-reverse)
- photo_intake.rs editing_any_reviewed_field_changes_the_content_hash skips 4 of the 12 projected keys (type, footprint_suggestion, component_id, scale_reference.kind); add a test that iterates the *_CONTENT_KEYS lists against the structs (source: reviewer, change: photo-to-kicad-reverse)
- photo_intake.rs TOOL_OWNED_KEYS lacks saved_path (inert today) (source: reviewer, change: photo-to-kicad-reverse)
- design.md:90 says check_retrace 'always returns a successful result' but it now errors on an unresolvable project_dir; qualify the sentence (source: reviewer, change: photo-to-kicad-reverse)
- crates/konnect/src/install.rs offline reliability-contract test names only two agents while pcb-photo-intake-agent.md also reads the contract; extend the list (source: reviewer, change: photo-to-kicad-reverse)
- no rust stack profile under ~/.orc/stacks/; Rust/Windows technology lessons stay in Konnect L1 memory until one is created (source: memory-curator, change: photo-to-kicad-reverse)
- planner L1 note says orc only wraps new change/init, but orc validate --changes works directly; reconcile (source: memory-curator, change: photo-to-kicad-reverse)
- design.md D1/D6 literal Limits{..no_limits()} does not compile (image::Limits is non_exhaustive); code sets fields individually; align the design text (source: developer, change: board-dossier-reconstruction)
- prepare_board_photo decodes/encodes synchronously inside the async handler; bounded by the pixel cap but blocks the executor; consider spawn_blocking (source: developer, change: board-dossier-reconstruction)
- nothing verifies that dossier.evidence[].view names an existing file under views/ or source_images; reviewer-only rule today (source: developer, change: board-dossier-reconstruction)
- intake agent lacks an in-session feature-count helper (dome/pad template matching); a count_features tool or documented manual method is a later-slice candidate (source: orchestrator, change: board-dossier-reconstruction)
- crates/konnect/tests/schema_parameter_usage.rs all_function_bodies skips any fn whose signature contains ';' (e.g. [T; N] arrays); make the check depth-aware (source: developer, change: board-dossier-reconstruction)
- 6.1 acceptance dossier (map b3d0be59) predates the locations count/kind rule; re-run the reconciliation on the next real-photo acceptance (source: qa, change: board-dossier-reconstruction)
- asset_references.rs guards are file-wide contains; kicad-schematic-build-agent.md's new design-brief gate can be deleted green because its older gate keeps the same markers (source: reviewer, change: board-dossier-reconstruction)
- prepare_board_photo: a stale partial view file from a failed encode permanently blocks its label; the escape (delete the file) is only in the error string, not in the skill (source: reviewer, change: board-dossier-reconstruction)
- release note: prepare_board_photo now refuses to overwrite an existing view (behaviour change for callers that reused labels) (source: reviewer, change: board-dossier-reconstruction)
- bundled agents limited to mcp__konnect__* are told to read skill references/ (layout agent, pcb-photo-intake-agent, which also forbids Read in Hard Rule 6); verify whether a sub-agent without Read can open a preloaded skill's references (source: architect, change: konnect-orchestrator)
- design_state_hash covers editor-history and backup copies (display-remoto: 39 of 53 covered files), so a new backup forces re-approval; narrow coverage to the live project tree in its own change (source: architect, change: konnect-orchestrator)
- now_rfc3339_utc (photo_intake.rs:1159) will serve two modules through a pub(crate) widening; move time helpers to a shared module at the next drift review (source: architect, change: konnect-orchestrator)
- CallRecord (observability.rs:63) carries no arguments and no caller identity, so no call-log check can tell which agent or project a call served; prerequisite for log-verified handoffs (source: architect, change: konnect-orchestrator)
- flow.rs carries a copy of design_hash.rs's private normalize_eol; a shared text-normalization helper would remove it (source: developer, change: konnect-orchestrator)
- flow_start's STATE.md write and its log append are not one unit: if the append fails the job is open and only log_error reports it (source: developer, change: konnect-orchestrator)
- flow_gate writes records/gates/<gate>.md and appends the log before STATE.md is replaced; a failed final replace leaves a gate file saying approve with no approval in STATE.md (source: developer, change: konnect-orchestrator)
- flow_status gate_validity recomputes valid from the hashes only, not the visit, so a hand-edited earlier-visit approval shows valid: true although flow_advance refuses it (source: developer, change: konnect-orchestrator)
- flow STATE.md render_body history table omits a rewind's reason and the evidence_check (front matter and log carry them) (source: developer, change: konnect-orchestrator)
- README/DEV token estimates (~23K listing, ~25K catalogue) not re-derived for the six flow tools (~0.6K more); tool-directory appendix sentence listing cross-cutting categories omits orchestration (source: developer, change: konnect-orchestrator)
- not_tools_allowlist.rs only parses NOT_TOOLS from the photo-intake marker to the end of the array, so a block inserted before that marker is never checked (source: developer, change: konnect-orchestrator)
- README.md's bundled skills/agents count line is not guarded by any test and will go stale on the next manifest change (source: developer, change: konnect-orchestrator)
- flow_status valid ignores the visit; reachable only by a hand edit since a rewind clears approvals at or after its target (source: reviewer, change: konnect-orchestrator)
- doc_tool_counts.rs SKIP matches any directory named archive or .orchestrator anywhere in the repo (name-based, doc_tool_counts.rs:187); only openspec/changes/archive exists today (source: reviewer, change: konnect-orchestrator)
- flow.rs append_file (log, memory, candidates) follows a symlink planted at the destination name; needs local write access, no caller-input path (source: reviewer, change: konnect-orchestrator)
- records/gates/<gate>.md persists across jobs, so a new job's flow_status(read: gates/purchase.md) returns the previous job's decision (the file names its job) (source: reviewer, change: konnect-orchestrator)
- konnect router's single-agent lane and Agent Routing bullets name bundled agents a Codex install does not receive (pre-existing pattern; orchestration.md section 12 covers jobs only) (source: reviewer, change: konnect-orchestrator)
- flow.rs HOSTILE_OBJECTIVE (unit round-trip, ~:3232) lacks a c-cedilla; the e2e covers it (source: qa, change: konnect-orchestrator)
- flow.rs post-commit side writes (gate file, logs) run outside the STATE.md lock because konnect_sexp keeps open_document_lock pub(crate); two concurrent flow_gate calls on one gate may write the gate file in inverted order (STATE.md stays the truth) (source: developer, change: konnect-orchestrator)
- the published descriptions of flow_gate, flow_advance and flow_defer do not mention the warning field; only orchestration.md section 7 explains it (source: developer, change: konnect-orchestrator)
- konnect-sexp transact_atomic can return Err after the rename (read-back failure in the share-mode window, writer.rs:185-190, or a Unix parent-dir fsync failure :137/:422); the caller sees 'Could not update' while STATE.md holds the change and a retried flow_defer duplicates its item — pre-existing race, not reproduced (source: reviewer, change: konnect-orchestrator)
- orchestration.md section 7 says repair only what a warning names, but no tool rewrites records/gates/<gate>.md except flow_gate, which the same bullet forbids repeating; harmless since no asset reads the gate file (source: reviewer, change: konnect-orchestrator)
- manufacture agent Step 4 never mentions include_assembly: false for a bare-board order; export_manufacturing_package without a schematic warns 'No schematic provided — BOM not generated' (manufacturing.rs:363), which keeps INCOMPLETE until a re-export — recoverable, not a deadlock (source: reviewer, change: konnect-orchestrator)
- the SERIOUS-1 closure (warnings-only board exits manufacturing) rests on the agent text; no test runs the manufacture agent on such a board (source: qa, change: konnect-orchestrator)
- the library agent's Step 6 scratch-project registration has never been exercised live with KiCad (source: qa, change: konnect-orchestrator)

## history
Records: `- <date> ARCHIVE <change> <base-sha>..<head-sha>` · `- <date> RETRO <n-changes> <base-sha>..<head-sha>` · `- <date> FAST <slug> <base-sha>..<head-sha>` — verb is the 2nd token, only `ARCHIVE` counts toward the retro cadence, no commit → `nocommit`. The single `initialized` seed line below, written once by `orc init`, is the sole exemption from this shape: a consumer ignores any record whose 2nd field is `initialized`, and it counts toward no cadence.
- 2026-09-18 initialized
- 2026-09-18 ARCHIVE photo-to-kicad-reverse ea74faf..3d77ed5
- 2026-09-18 ARCHIVE board-dossier-reconstruction 1815948..df870a1
- 2026-09-22 MERGE board-dossier-reconstruction+photo-to-kicad-reverse ee55a12..bbd5efb
- 2026-09-22 ARCHIVE konnect-orchestrator bbd5efb..9a0f1ce
- 2026-09-22 MERGE konnect-orchestrator bbd5efb..9a0f1ce
- 2026-09-22 FAST install-scripts 9a0f1ce..6a2f7d2 merged
- 2026-09-22 PUSH main 9d33cf0..6a2f7d2 origin=FelippeFarias/Konnect
