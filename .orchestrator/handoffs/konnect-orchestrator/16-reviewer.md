---
change: konnect-orchestrator
task: re-review after fix round 1 (62d2045..94d2944, 8 commits) — confirm reviewer 11's findings closed, attack the fix diff
agent: reviewer
verdict: FIX
failing_layer: spec
---

## Result

**FIX: one new SERIOUS finding (spec layer) and four new MINOR findings.** Every round-1 finding is closed. Both SERIOUS reproductions were re-run against a freshly built binary and are refused at `flow_start`. I found no other path (rewind, abandon/close, lane subset, autonomous mode, a new job over old records) that reaches a gate without its producing phase or approves a closed job's record. The new commit ordering holds under a failed `STATE.md` write and under two-process contention.

The new SERIOUS finding does not reopen the safety hole. It is a liveness defect created by DECISION F's wording: the manufacture agent can no longer leave `manufacturing` on a board whose DRC shows only warnings, which is common.

### Round-1 findings

| Round-1 finding (11-reviewer.md) | Closed? | Evidence |
|---|---|---|
| 1 SERIOUS — a gate is kept without its producing phase | **Closed** | `validate_phases` converse loop (`flow.rs:461-468`). `r2_p1` on a fresh build: Scenario A `[routing, prefab_review, gate:purchase]` and Scenario B `[requirements, gate:architecture, schematic]` return `invalid_argument`, field `phases`, in guided and in autonomous mode. So do 9 other gate-without-producer shapes, including the photo lane minus `manufacturing` and all three gates without their producers. `STATE.md` is byte-unchanged. The original `p1_gate_without_phase.py` now dies at `flow_start` (`KeyError: 'job_id'`). All five documented lanes are still accepted. No other entry path exists: `r2_p3` shows every rewind to a gate refused; a new job starts with `gate_approvals: {}`; entering `gate:purchase` without records, while an old `manufacturing.md` is on disk, is refused (`field=records`). |
| 2 (minor 1) — side files are written before the `STATE.md` commit | **Closed** | `transact_then_record` / `record_after_commit` (`flow.rs:1889`, `:1908`). `r2_p2`: a reject while `STATE.md` is held returns an error; `gates/purchase.md` still reads `approve`; the log has no `gate purchase reject`; STATE keeps the approval (`current`, `valid: true`). The retry succeeds with `warning: null`: the gate file reads `reject`, the log holds exactly 1 reject entry, and leaving the gate is `stale_target`. A rewind and a defer while STATE is held both error, with nothing in the log and nothing in STATE. |
| 3 (minor 2) — `valid` on passed gates | **Closed in code**; text residue → new MINOR 3 | `gate_validity` (`flow.rs:1224`). `r2_p3`: at `schematic` after a save, architecture reports `passed` with no `valid`; at `gate:placement` placement reports `current`, `valid: true`; at `learn` and after close, every approval reports `passed`. |
| 4 (minor 3) — rewind into a gate | **Closed** | `flow.rs:2185`. `r2_p3`, both modes: rewinds `prefab_review → gate:placement` and `→ gate:architecture` (with or without a reason) and `learn → gate:purchase` are refused with `invalid_argument`, field `to_phase`; STATE bytes are unchanged. The replacement loop works: rewind to `placement` → `cleared=['placement']`, re-enter with a new `placement.md`, re-approve, leave. |
| 5 (minor 4) — library scratch project cannot resolve the new library | **Closed (traced, not run live)** | Step 6's parameters match the schemas (`library.rs:183-190`, `:313-320`; `project` accepts a `.kicad_pro` or its directory). `lib_table_target` writes an absolute URI for a library outside the scratch dir (`library.rs:1952-1975`). `resolve_footprint_path` reads the scratch dir's `fp-lib-table` (`:1635-1640`). |
| 6 (minor 5) — manufacture verdict drift | **Closed for both named scenarios**, but over-corrected → new SERIOUS 1 and MINOR 2 | A job-less run now reads `INCOMPLETE` naming the three checks (`kicad-manufacture-agent.md:181-184`). An export warning is now `INCOMPLETE`. |
| 7 (minor 6) — hard-coded flow guard | **Closed** (by QA, before this range) | `every_agent_that_names_a_flow_tool_loads_flow` iterates `assets/agents/` (`asset_references.rs:1812-1852`, `enrolled >= 9`). `architecture_confirmation_rule_stays_guarded` pins `located, not validated` per section. |

### New findings

1. **SERIOUS — spec (design.md DECISION F text) → implementation (manufacture agent). "Passed with a warning" is wider than the skill, so a warnings-only board cannot leave `manufacturing`.**
   - **The text.**
     - `kicad-manufacture-agent.md:176-181`: `READY` needs "every check your tools can run passed with no warning". `INCOMPLETE` covers a check that "passed with a warning".
     - `:205-207`: an `INCOMPLETE` "with any other open item (a warning, …) is not an exit: do not advance". No waiver path for a warning exists anywhere in the agent.
   - **What the skill it quotes actually says.**
     - "Any warning … keeps the result `INCOMPLETE`" is step 1 of the artifact acceptance gate. It is about the export's `warnings` array (`kicad-manufacture/SKILL.md:186`).
     - For DRC the skill requires "resolve every error or record a deliberate, reviewable waiver" (`:80-81`). DRC warnings are not blocking.
     - For the preflight, only "an unadjudicated issue blocks release" (`:97`). `validate_for_manufacturing` returns `NEEDS REVIEW` for any non-error issue (`manufacturing.rs:1068-1074`).
   - **Where the widening came from.** The orchestrator's DECISION F (log) says "READY only when every **artifact** check passes with no warning". `design.md:585-588` turned that into "every check the agent's tools can run passed with no warning", and the agent implements design.md faithfully. Before this round, `INCOMPLETE` did not include warnings at all.
   - **Reproduction.** `kicad-cli 10.0.2 pcb drc --severity-all` on KiCad's own `ecc83-pp` demo (repo fixture `crates/konnect-sexp/tests/fixtures/ecc83-pp.kicad_pcb`) gives **0 errors, 17 warnings**: 2× `silk_edge_clearance` "Silkscreen clipped by board edge", plus 15× `lib_footprint_issues`, which depend on the environment.
     - Read literally, `run_drc` "passed with a warning", so the verdict is `INCOMPLETE` and not an exit.
     - The agent returns `FIX` to the producing phase (routing cannot fix a library-configuration warning) or `BLOCKED`.
     - §5's cap then stops the job after three rounds.
   - The rule applied here is the agent's own literal text. I did not run the LLM agent.
   - **Fix.**
     - design.md F: say "every artifact check" (as the log does). Scope the warning clause to the export's `warnings` array (artifact acceptance gate step 1).
     - Agent Verdict bullet: state that DRC warnings and preflight issues follow skill §2 — errors resolved or waived reviewably, issues adjudicated in the record. A waived or adjudicated item is not an open item.
     - The QA guard now pending in the worktree (uncommitted; `asset_references.rs:2068` in the working tree) pins "left an artifact missing, or passed with a warning". Change it in the same edit.

2. **MINOR — spec. After the purchase gate, `READY` has no owner.**
   - The agent never emits `READY`, with or without a job (`:181-184`; hard rule 4). `manufacturing.md` is hash-bound when the job enters the gate, so the record stays `INCOMPLETE` for good.
   - `SKILL.md:267` says "Only `READY` permits upload". `orchestration.md` §4 (`:129-137`) has the session run the three checks and record them with `flow_log`, but it never says that this makes the package `READY` or permits the upload.
   - DECISION F / design.md says "READY is still what the skill requires before anything is actually sent", but no text assigns it to anyone.
   - **Scenario.** The user approves the purchase. A session holding the kicad-manufacture skill finds only `INCOMPLETE` on record, and either withholds "you can upload" or says it against the record.
   - **Fix.** One sentence in §4's purchase bullet: once each listed check is recorded as passed, the package is `READY` in the skill's sense and the approval is asked on that basis.
   - The agent's frontmatter still advertises "ending in READY, NOT READY or INCOMPLETE" (`:3`).

3. **MINOR — implementation. DECISION D left three statements that `valid` is recomputed for every approval.**
   - The `flow_status` tool description (`flow.rs:3105-3106`) says "gate approvals with a recomputed `valid`". That is the text every MCP client and every MCP-only agent gets.
   - `orchestration.md:142-143` (§4) says "`flow_status` reports every approval's validity against the current files". It contradicts the §7 sentence this round added to the same file (`:221-224`); task 8.5's "append, do not reword" constraint kept it.
   - The body of `STATE.md` says "Validity is recomputed by `flow_status`" (`flow.rs:664`).
   - **Scenario.** A client or agent without §7 looks for `valid` on a passed gate and finds nothing.
   - **Fix.** Reword all three.

4. **MINOR — spec. D11's cost of a false stale is out of date.**
   - `design.md:739-740` says a false "stale" "costs a re-approval".
   - Since DECISION B, recovery means rewinding to the producing phase. D4 then requires the producer to re-supply its records in the leaving call, and the session never advances on an agent's behalf. The real cost is now a producer run plus a re-approval.
   - DECISION B's "costs no lane" is still right. This sentence is not.

5. **MINOR — implementation. The post-commit side writes run after the `STATE.md` lock is released.**
   - Two opposing `flow_gate` calls, or two `flow_defer` calls, can land their gate file or log entry in reverse commit order (developer 13 #2 / 14 #1). This change caused it; before, those writes happened under the lock.
   - Traced, not observed: `r2_p5` with two server processes gave **0/200** gate-file or log mismatches over approve-vs-reject rounds, and log order equal to STATE order over 400 concurrent defers.
   - An inversion needs one caller descheduled for longer than another caller's whole transaction, fsync included. `STATE.md` stays the truth, and no asset reads `records/gates/*.md`.
   - I recommend accepting this by a logged DECISION rather than exporting a lock guard from konnect-sexp.

### Adjudication of developer 14's two questions

- **(a) The manufacture agent never reaching READY.** Inside the agent, the text is coherent and honest: it says outright that the agent never reaches `READY` and why, and the exit exception is exact and matches the §2 row. What is not coherent is the scope of "warning" (SERIOUS 1) and the missing owner of `READY` after the gate (MINOR 2). The job-less scenario from round 1 is closed.
- **(b) The replaced table row.** Accepted. The new `manufacturing` row is the old row with the exception inserted before the closing pipe, a pure insertion. The guard still keys on `| \`manufacturing\` |` and `manufacturing.md`.

## Evidence

**Build.** Fresh `konnect.exe` built from the worktree at `94d2944` (`flow.rs` clean) into `%TEMP%\k16t`, outside the repo; sha256 prefix `691450c12559ef01`. It was copied into the scratchpad probe kit. Everything below is Python MCP stdio against that copy, with `KONNECT_STATE_DIR`/`APPDATA` in temp. The scripts are in the session scratchpad `probe/` as `r2_p1`, `r2_p2`, `r2_p3` and `r2_p5`.

- **r2_p1.** 11/11 bad shapes `REFUSED … kind=invalid_argument field=phases`; `STATE.md unchanged by the refusals: True`; 5/5 documented lanes `ACCEPTED`.
- **r2_p2.**
  - Held reject → `ERROR: Could not update …STATE.md (IO error: Access is denied. (os error 5))`, then `gates/purchase.md first line: # Gate \`purchase\` — approve`, `log has 'gate purchase reject': False`, `STATE still holds approval: True current valid: True`.
  - Retry → `ok warning: None`, then `— reject`, `'gate purchase reject' entries in log: 1`, `leave gate:purchase after the reject -> refused (stale_target)`.
  - Held rewind → `ERROR`, with no rewind in the log and the phase still `gate:purchase`.
  - Held defer → `ERROR | in STATE: False | in log: False`.
- **r2_p3.** Quoted in the table above (both modes).
- **r2_p5.** `(a) 200 rounds: gate-file mismatches=0 log mismatches=0 warnings=0 errors=0`; `(b) STATE items=400 log items=400 … positions where log order != STATE order: 0`.
- **DRC.** `ecc83-pp {'warning': 17}`, `pic_programmer {'error': 8, 'warning': 9}`, `RoyalBlue54L-NFC-Antenna {'error': 2, 'warning': 6}` (kicad-cli 10.0.2, copies in `%TEMP%\r2drc`).

**Tests** (`CARGO_TARGET_DIR=%TEMP%\k16t`).
- `cargo test -p konnect-core --lib flow::`: 61 passed.
- `--test flow_gate_e2e --test flow_contract_e2e`: 4 + 11 passed.
- `cargo test -p konnect --test asset_references --test schema_parameter_usage --test not_tools_allowlist --test doc_tool_counts`: 21 / 5 / 3 / 6 passed. That run used the working tree, which contains QA's uncommitted guards.
- On a clean `git archive 94d2944` export: `asset_references` **18 passed**.

**Code read.**
- `transact_atomic` (`konnect-sexp/src/writer.rs:176-194`).
- Every hunk of the `flow.rs` diff.
- `package_records` (`:876`): each gate's producer is adjacent in the canonical order, so with the converse rule every package record is supplied in this job (D4).
- `design.md` Fix round 1 notes, flow-toolset `spec.md:80-110` and `:285-315` (the converse scenario and the warning scenario are present), tasks §8, and log DECISIONs A–H.

## For the next agent

- **SERIOUS 1 goes to the architect first** (design.md F, `:585-588`), then to the developer (`kicad-manufacture-agent.md:176-184` and `:205-207`, plus QA's pending guard marker). MINORs 2–4 are one sentence each (`orchestration.md` §4 ×2, `flow.rs:3105`, `flow.rs:664`, `design.md:739`). MINOR 5 needs only a logged decision.
- **Worktree activity.** While this review ran, another agent worked in the worktree: an uncommitted `crates/konnect/tests/asset_references.rs` (+144), an untracked `crates/konnect-core/tests/flow_fix_round_one_e2e.rs`, and a live mutation in `kicad-library-agent.md` Step 6 ("Then place the symbol at once."). None of it is reviewed here; this verdict covers `62d2045..94d2944` only.
- **Build path.** Probe builds need a short `CARGO_TARGET_DIR`: nng-sys's MSBuild step fails on scratchpad-length paths (MAX_PATH).

## Deferred findings

1. **Error after commit (pre-existing; konnect-sexp untouched).** `transact_atomic` can return `Err` **after** the rename. This happens if the read-back fails, for example when a share-mode-0 opener lands in the window (`writer.rs:185-190`), or on Unix if the parent-directory fsync fails (`:137`, `:422`). The caller then gets "Could not update …" while STATE holds the change, and no side file is written. A retried `flow_defer` would duplicate its item. That breaks the spec's "never as an error that could invite a retry" (`flow-toolset/spec.md:291-297`) inside that window. Not reproduced; it is a race.
2. **Unrepairable gate file.** §7's "repair only what the warning names" (`orchestration.md:231-233`) has no tool that rewrites `records/gates/<gate>.md` except `flow_gate`, which the same bullet forbids repeating. It is harmless, because no asset reads the gate file. Caused by this change; cosmetic.
3. **Round-1 deferred items 1, 3, 4 and 5 still stand unchanged.** Item 1 (`valid` ignores the visit) can now be reached only by a hand edit, since DECISION B stops a gate from being re-entered with an old approval.
