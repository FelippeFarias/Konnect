---
change: konnect-orchestrator
task: verification round 3 of 3 — confirm reviewer 16's findings closed, attack fix round 2 (1c05a85..e2228f9), ship/no-ship on the whole change (bbd5efb..e2228f9)
agent: reviewer
verdict: DONE
---

## Result

**DONE: ship.** Every round-2 finding is closed. I found no new SERIOUS finding. Three new MINOR findings are all text-only and none blocks shipping. A fourth gap I found at `e2228f9` (the §4 READY bullet had no guard) is already closed by QA's `9e848d3`, which landed during this review; I checked that by mutation.

### Round-2 finding → closed?

| Round-2 finding (16-reviewer.md) | Closed? | Evidence |
|---|---|---|
| 1 SERIOUS: a warnings-only board could never exit `manufacturing` | **Closed** | Walked the new Verdict (`kicad-manufacture-agent.md:176-192`) and "Ending the run" (`:201-223`) against the tool code. **ecc83-pp shape** (0 DRC errors, 17 warnings): each warning is adjudicated, and "An adjudicated DRC or preflight warning is not an open item" (`:187`). The preflight passes DRC *errors* only (`manufacturing.rs:1020`), so DRC warnings never become preflight issues. The run ends `INCOMPLETE` naming only the three purchase-gate checks, and that is an exit. **An export `warnings` entry** keeps `INCOMPLETE` with no escape: the clause at `:183-186` plus the non-exit sentence at `:216-218` ("an artifact warning"). **A missing artifact** is also not an exit (same two places). **An unwaived DRC error** gives `NOT READY` (`:181`). **No new deadlock:** export `warnings` come only from failures, a missing schematic, or a population mismatch (`manufacturing.rs:220-369`); `PREVIEW_REQUIRED` is not a warning, so a clean JLCPCB export exits. **Mutation:** I added "or a DRC check passed with a warning" to the INCOMPLETE clause, and `manufacture_verdict_permits_exit_with_adjudicated_drc_warnings` failed by name ("the INCOMPLETE clause names `DRC`…"), 21/22 green. That test carries the load; the older guard stayed green. design.md F is back to "every **artifact** check" (`design.md:619-625`). |
| 2 MINOR: nobody owns READY after `gate:purchase` | **Closed** | The owner is stated consistently in four places: `orchestration.md:141-146` (§4), the agent's "Ending the run" (`:210-214`), its frontmatter (`:3`), and design.md's J note (`:648-659`). No agent text tells anyone to upload (grep `upload` in `assets/agents/`: 3 hits, all "waits" or "never uploads"). **Live `r3_p1`:** fab_only job → `gate:purchase` → 3 evidence logs → approve (`warning=None`) → `flow_log(evidence)` "READY …" accepted at the gate → purchase is still `status=current valid=True` (the log sits outside the design and package hashes; `design_hash.rs:63` skips `.konnect`) → `closed` ok → purchase `passed`. The new_board path to `learn` after the READY log is also ok. |
| 3 MINOR: `valid` described as recomputed for every approval | **Closed** | Live `tools/list`: the `flow_status` description carries "only for the gate the job stands at now". Live `STATE.md` body: "Validity is recomputed by `flow_status` only for the gate the job stands at now; …". `orchestration.md:148-150` matches §7 (`:226-231`). `tool-directory.md:505` was fixed by M. The code agrees (`gate_validity`, `flow.rs:1225-1253`). No other `.md` in the repo says otherwise (grep). The flow-toolset spec agrees (`spec.md:40-41`, `:69-74`). |
| 4 MINOR: D11's cost of a false stale | **Closed** | `design.md:812-815`: "a re-run of the producing agent plus a re-approval". |
| 5 MINOR: post-commit writes run outside the lock | **Closed by DECISION L** (accepted trade-off) | design.md D2 note (`:309-323`). No code change, as intended. My round-2 probe (0 divergences in 600 runs) still stands. |

### Attack areas

1. **Verdict vs skill: covered.** See row 1 above, plus new MINOR 1 below: the narrowing went one step too far for DRC that *did not run*.
2. **READY after the purchase gate: covered.** See row 2. The session declares READY with `flow_log` evidence naming the approval, the user uploads, and no agent does. §4, the agent and design.md agree.
3. **`valid` / `status: passed` / `warning` texts: covered.** See row 3. `warning` is always present on success, because `transact_then_record` sets it unconditionally (`flow.rs:1899`). Live: `warning=None` on approve and on close. The `flow_gate`, `flow_advance` and `flow_defer` descriptions carry "never repeat the call", and the published `flow_advance` text says "a validation refusal writes nothing". The one residue is in `tool-directory.md` (MINOR 3).
4. **Final sweep of the whole change: covered.**
   - Rounds 3 and 2 changed no production code other than `flow.rs` description strings. Every code hunk since `bbd5efb` was reviewed in round 1 or round 2 (`84f7e0f..62d2045` and `94d2944..1c05a85` are test-only).
   - Path confinement was covered in round 1 (P4) and is unchanged.
   - The three user gates were re-probed live on the fresh build. Guided: an empty or whitespace `user_words` on architecture is refused (`invalid_argument`). Autonomous: architecture and placement accept empty words (recorded as session), and purchase with empty words is refused.
   - Schema callability: `call_examples_name_real_parameters` and `schema_parameter_usage` pass.
   - Catalogue counts match the tree: 238/245/23, README 12 skills + 11 agents = `ls` 12/11.
   - No asset tells an agent to upload, change the design, or call `flow_gate`.

### New findings (all MINOR, none blocking)

1. **MINOR — spec (design.md D9 Fix-round-1 note `:619-633`) → implementation (agent Verdict `:176-188`) → test (the new guard). The Verdict's classes have no place for DRC that did not run completely. "Every preflight issue is adjudicated" also covers error-severity issues the skill says always block.**
   - **Reproduction (`r3_p2`, live).** I ran `validate_for_manufacturing` on KiCad's `ecc83-pp` without a same-stem schematic, which is the fab_only case where this agent's DRC is the job's only DRC. Result: `verdict: NOT READY`, `drc.schematic_parity: null`, and exactly one issue, severity `error`: "DRC schematic parity was not checked …".
   - **Read literally, this exits.** The agent may accept that issue "with its reason recorded". Then `NOT READY` no longer applies ("an unadjudicated preflight issue"). `INCOMPLETE` is artifact-only, and the guard forbids "DRC"/"preflight" in that clause. So the run ends `INCOMPLETE` with only the three purchase checks open, which is an exit.
   - **What the skill says.** DRC evidence "must be available and complete for a `READY` verdict", and "A null or incomplete `drc`, a `NOT READY` verdict, or an unadjudicated issue blocks release" (`SKILL.md:93-97`).
   - **The round-2 text caught this case** ("a check your tools can run did not run"). This round narrowed all four `INCOMPLETE` conditions to artifact checks. DECISION I asked only for the *warning* condition to be narrowed.
   - **Why this is MINOR and not SERIOUS.** The literal path is contradicted in three places:
     - Step 2 (`:104-105`): "confirm parity was checked, not `null`".
     - "Ending the run" (`:216-218`) counts "a check that did not run or failed" as an open item with no artifact qualifier; QA's `9e848d3` now pins that sentence.
     - The skill, which says otherwise, is preloaded.
     `gate:purchase` is still a human gate. Exposure is limited to the `fab_only` lane, because routing and prefab_review establish DRC with parity in every other lane.
   - **Fix: one sentence after the INCOMPLETE clause.** It cannot go inside that clause, because the new guard rejects "DRC" there. Suggested text: "A DRC that did not run or ran without schematic parity (a null or incomplete `drc`), and a preflight `NOT READY` verdict, are never adjudicated away: the run does not exit." Optionally, the §4 purchase "Show" row (`orchestration.md:116`) could name `manufacturing.md`'s Design-evidence waivers and adjudications. That gate is the only human review of what this agent accepted.
2. **MINOR — spec (doc). design.md contradicts amended DECISION I.** `design.md:626-628` says "a DRC **error** always blocks" and then quotes the skill's waiver clause. The log's amended DECISION I and the agent (`:178`, `:181`, "resolved or waived", "an unwaived DRC error") allow a deliberate, reviewable waiver. **Scenario:** an archived design that states a stricter rule than the shipped agent follows misleads the next change that relies on it. **Fix:** "a DRC error blocks unless resolved or covered by a deliberate, reviewable waiver".
3. **MINOR — implementation (doc). DECISION K is still missing from `tool-directory.md`.** The `flow_advance` row (`:507`) still ends "A refusal writes nothing.", while the published description now says "a validation refusal writes nothing". The `flow_gate` row (`:508`) and the `flow_defer` row (`:510`) do not mention `warning` / "never repeat the call". Only humans read this table; sessions get the rule from §7 and the tool descriptions. **Fix:** reword the three rows.

### Ship rationale

The tool core is sound, and it has been attacked three times. Round 1 tested locking, atomicity, path confinement, hash binding and schemas. Round 2 re-ran every reproduction on a fresh build, and I repeated the key ones again this round:
- A gate cannot be kept without its producer.
- A rewind cannot re-enter a gate.
- `STATE.md` is the one commit point.
- `valid` is current-only.
- The three user gates hold in both modes, with purchase always needing the user's words.

This round changed only prose and description strings, and each change matches the code (checked live against `tools/list` and a rendered `STATE.md`). The round-2 liveness defect is gone: a 0-error board with adjudicated DRC warnings reaches `gate:purchase`, while an export warning, a missing artifact or an unwaived DRC error still does not. A guard also fails by name if the warning rule is widened again. The three remaining findings are wording. The one that touches safety (MINOR 1) is contradicted by two other sentences in the same agent and by the preloaded skill, and a human purchase gate follows it. None of them justifies stopping the change at the cap. They fit in a small follow-up edit, or the next change that touches the manufacture agent.

## Evidence

- **Build.** Fresh `konnect.exe` from `git archive e2228f9`, extracted to `%TEMP%\k21src`, `CARGO_TARGET_DIR=%TEMP%\k16t` (outside the repo), sha256 prefix `ecf13a6c3b778585`. Copied into the scratchpad kit `probe3/`, driven over MCP stdio with `KONNECT_STATE_DIR`/`APPDATA` in temp.
- **Tests on that export (e2228f9).**
  - `cargo test -p konnect-core --lib flow::`: 61 passed.
  - `--test flow_contract_e2e`: 11 passed. `--test flow_fix_round_one_e2e`: 4 passed. `--test flow_gate_e2e`: 4 passed.
  - `cargo test -p konnect --test asset_references`: 22 passed. `doc_tool_counts`: 6, `not_tools_allowlist`: 3, `schema_parameter_usage`: 5, all passed.
  - With QA's `9e848d3` test files overlaid: `asset_references` 24 passed, `flow_fix_round_two_texts` 2 passed.
- **r3_p1 (`probe3/r3_p1_ready_after_purchase.py`).**
  - `approve purchase: ok warning=None`; `flow_log READY evidence at gate:purchase: ok`; `after READY log: purchase status= current valid= True`; `leave gate:purchase -> closed: ok warning=None`; after close, purchase is `passed`.
  - `new_board leave gate:purchase -> learn after READY log: ok`.
  - Gates: `autonomous purchase empty words: refused kind=invalid_argument`; `guided architecture empty words` and `whitespace words` both refused. Autonomous architecture and placement with empty words: ok (session).
  - Descriptions: `flow_status` valid-clause=True; `flow_gate`, `flow_advance` and `flow_defer` have warning=True and never-repeat=True; no description still says "a refusal writes nothing".
- **r3_p2 (`probe3/r3_p2_preflight_parity.py`).** `verdict: NOT READY`; `drc: {"design_rule_violations": 17, "errors": 0, "schematic_parity": null, "unconnected_items": 0}`; one issue, `error | DRC schematic parity was not checked: …`.
- **Mutations** (on the temp export, each file restored and compared byte-for-byte with `cmp`).
  - (a) INCOMPLETE re-widened with a DRC-warning clause: the new test FAILED, naming `DRC`.
  - (b) §4 READY-owner bullet deleted: at `e2228f9`, 22/22 still green (the gap). With `9e848d3`'s guards, `orchestration_reference_gives_ready_an_owner_after_the_purchase_gate` FAILED and listed all 4 lost markers.
- **Marker counts** in the agent (whitespace-flattened): "An adjudicated DRC or preflight warning is not an open item" 1, "the orchestrating session declares that" 1, "every DRC warning and every preflight issue is adjudicated" 1, "left an artifact missing, or passed with a warning" 1. Each is load-bearing.
- **Code and text read.**
  - The full `1c05a85..e2228f9` diff.
  - The whole manufacture agent and `kicad-manufacture/SKILL.md`.
  - The whole of `orchestration.md`.
  - design.md: D2 Fix-round-2 notes, D9 F/I/J notes, and D11 (`:812`).
  - tasks.md §9; the log's DECISIONs I (amended) to M; developer 18.
  - `manufacturing.rs` export warnings (`:198-441`) and the preflight verdict (`:1016-1074`).
  - `gate_validity`, `transact_then_record`, `package_records`.

## For the next agent

- **Orchestrator:** the verdict is DONE, so the change can go on to archive or merge. MINORs 1–3 are text edits:
  - MINOR 1: one sentence in the agent's Verdict bullet, outside the INCOMPLETE clause.
  - MINOR 2: one phrase in design.md.
  - MINOR 3: three `tool-directory.md` rows.
  Either land them as a follow-up commit before archive (none needs a verify round of its own; `asset_references` covers the agent), or record them as deferred.
- **This verdict covers `e2228f9`.** QA's `9e848d3` (test-only: `flow_fix_round_two_texts.rs` and 2 new `asset_references` guards) landed during this review. I ran it (24 + 2 passed) and used it in mutation (b), but I did not review it line by line.

## Deferred findings

1. **A bare-board export without a schematic warns.** `export_manufacturing_package` defaults `include_assembly: true`. Without `schematic` it warns "No schematic provided — BOM not generated" (`manufacturing.rs:363`), and under the artifact rule that warning keeps `INCOMPLETE`. The agent's Step 4 (`:118-120`) never mentions `include_assembly: false` for a bare-board order. It recovers by re-exporting (with the schematic, or with assembly off) into a fresh directory, and the warning text says what to pass, so this is not a deadlock. Low priority.
2. **Carried unchanged from round 2:**
   - Deferred 1: `transact_atomic` can return `Err` after the rename (konnect-sexp, pre-existing).
   - Deferred 2: no tool rewrites `records/gates/<gate>.md` to repair a missed gate file (cosmetic).
   - Deferred 3: round-1 deferred items 1, 3, 4 and 5.
