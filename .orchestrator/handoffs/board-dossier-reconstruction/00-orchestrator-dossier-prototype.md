---
change: board-dossier-reconstruction
task: concept validation — manual board dossier from the real photos
agent: orchestrator
verdict: DONE
failing_layer: n/a
---

## Result

Board A (WhatsApp Image 2026-09-15 at 14.32.51/52, crops `boardA_top.png` 460x485 px, `boardA_bottom.png`).

1. Identity (observed silkscreen, zoom x3): "SEMAFARO 1.3 24V 03/2020" bottom edge; "24V" beside a 2-pin screw terminal with polarity marks. Product class: 24 V LED traffic-light lamp module. Confidence 0.95.
2. Inventory (visual, LLM reading zoomed views + blob counting): ~107 (board A) / ~122 (board B) 5 mm clear-lens LEDs in a staggered grid, silkscreen "LEDnn"; 19 axial through-hole resistors on board A in groups 5 (top-left), 5 (top-right), 3 (near terminal), 3 (bottom-right), 3 (right edge) — colour bands not readable at this resolution; 1 x 2-pin 5.08 mm screw terminal (green) bottom-left; 4 mounting holes at the corners (plated ring); 1 small unplated hole near top-right. No ICs, no SMD parts.
3. Physical: square outline ~ 410 x 445 px between hole centres in the crop; real size unknown — scale reference needed (ask user: hole pitch or board edge in mm; typical 100 x 100 mm). Terminal on the bottom edge, holes ~5 mm from corners.
4. Electrical inference (labelled INFERENCE): 24 V DC in; red/green/yellow high-brightness 5 mm LEDs Vf 2.0-2.2 V (red) to 3.0-3.4 V (green); 19 resistors and ~114 LEDs -> hypothesis A: 19 strings x 6 LEDs + 1 series R (6 x 2.1 V = 12.6 V, 11.4 V across R, 20 mA -> 570-620 ohm, 0.23 W -> 1/2 W axial). Hypothesis B: strings of 8-10 with 2-3 fewer strings. The bottom photo shows serpentine mask-covered traces chaining LED pads -> series strings confirmed qualitatively; string length must be confirmed by counting one chain on the bottom view or by continuity.
5. Design brief seed: recreate as N strings of 6 x 5 mm LEDs + 1 x 620 ohm 1/2 W, 2-pin 5.08 mm terminal, 4 x M3 holes, 100 x 100 mm (to confirm), single-sided routing feasible (original is 2-layer with bottom traces only visible).

## Evidence

- Zoom crops: scratchpad/photo-test/zoom/*.png (read by the orchestrator); retrace scan (`scan_pcb_photo`): 71 coarse boxes, 0 markings, 0 linked traces — hints only.
- Blob count script (HSV low-sat/high-val + connected components, area 120-900 px): 107 / 122; Hough circles undercounted (63/54).
- Resistor groups counted on zoom crops A_top_topleft_resistors (5), A_top_topright_resistors_hole (5), bottom-left terminal area (3), bottom-right (3), right edge (3).

## For the next agent

- The methodology works with LLM vision + trivial CV helpers; retrace alone does not. The dossier must carry evidence pointers (crop path + region) and a confidence per claim, and separate OBSERVED from INFERRED.
- A `prepare_board_photo` crop/zoom tool is the missing primitive; a blob/circle helper is optional (the agent can estimate counts from views, but a count helper improves the +-10 % metric).
- Scale is the one thing the photo cannot give: the design brief must ask for hole pitch / edge length or default to a stated assumption.

## Deferred findings
(none)
