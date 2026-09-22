---
change: board-dossier-reconstruction
task: 6.1 orchestrator checklist on the real reference photos
agent: orchestrator
verdict: DONE
failing_layer: n/a
---

## Result

Run: real binary (worktree b2eda51 release, 6-tool `photo_intake`) driven over MCP stdio; `scan_pcb_photo` on the top-side photo (10 fallback boxes, none on board A), 12 `prepare_board_photo` views; comprehension by a Claude subagent with Read (playing pcb-photo-intake-agent) producing `dossier` JSON; `save_photo_review_map` -> `approval_valid:false` -> `approve` -> `approval_valid:true` (hash 06696d88…). Map: scratchpad/photo-test/checklist-project/.konnect/photo_intake/b3d0be59-4257-41e7-92d7-590875c09cd1/review_map.json.

Checklist (design D12):
1. identity — PASS: summary quotes "SEMAFORO L3 24V 03/2020" (agent read 'O'/'L3' where the prototype read 'A'/'1.3' — both recorded as marginal), evidence[0].view exists, rect_px inside bounds.
2. "24V" observed — PASS (three silkscreen occurrences, basis observed, 0.9).
3. LED count — PASS: 119 (template matching + eye verification of every detection), alternatives 118/120; board B independently 119. Prototype's blob count 107/122 was the coarse reference; the agent's method is stronger — 119 recorded as the new reference.
4. Axial resistors 19 with locations summing to 19 — PASS on substance (19; groups 5/5/4/3 + 2 isolated, methods disagreeing on 3-vs-4 recorded as OQ-8 with alternatives 17/18/20), FAIL on machine reconciliation: locations carry counts in prose only and mix cross-check entries. Routed to round B docs: locations[] gain `count` + `kind`.
5. Mounting holes — PASS: 4 plated corner holes with position_px (+2 unplated field holes recorded separately with role).
6. Connector — PASS: one 2-pole screw terminal, bottom edge, left end.
7. Topology hypothesis with calculation — PASS: TC-1 H1 17 strings x 7 LEDs, red 2.0 V, V_R 10 V, 500 ohm @ 20 mA, 0.20 W, 340 mA total; H2/H4 kept; colour/string-length coupling stated.
8. Scale unresolved — PASS: board_size_mm null, scale_status UNRESOLVED with what resolves it, open question present, no mm_per_px.

Score: 7/8 PASS, item 4 substantively PASS with a reconciliation gap fixed at the doc/skill layer.

## Evidence
- stage1: run_61_stage1.py output (tools list of 6; map_id b3d0be59…; 12 views with output sizes).
- stage3: run_61_stage3.py output (save ok; approval_valid false -> approve -> true; scorer output) plus manual inspection of locations/holes JSON.

## For the next agent
- The dry run shows the methodology works with LLM vision + `prepare_board_photo`; retrace contributed nothing on this board (0 boxes on board A). The skill must say so plainly: hints, not evidence.
- The subagent wrote 16 open questions and refused to estimate board size — the honesty rules hold.
- Two schema-precision rules added for round B (locations count/kind; hole role literal; connector edge literal).

## Deferred findings
- The intake agent needs an in-session way to count domes (template/NCC) — today it improvised with Python; a `count_features` helper tool or a documented view-based manual method is a slice-3 candidate.
