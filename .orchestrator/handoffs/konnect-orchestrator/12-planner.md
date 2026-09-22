---
change: konnect-orchestrator
task: revise proposal for fix round 1 (reviewer 11's SERIOUS finding + minors 1-5; orchestrator DECISIONs A-F)
agent: planner
verdict: DONE
---

## Result

1. **`specs/flow-toolset/spec.md`** — extended 4 existing requirement bodies
   and added 4 new scenario headings (no existing heading renamed or
   reworded), one pair per decision:
   - **A**: `flow_start opens exactly one job per project` body gains the
     converse gate/phase rule; new scenario `a gate without its producing
     phase is rejected`.
   - **B**: `flow_advance rewinds, abandons and closes` body gains "a
     rewind target SHALL NOT be a gate phase"; new scenario `a rewind
     cannot target a gate`.
   - **C**: `STATE.md round-trips and serializes concurrent writers` body
     gains the commit-point/warning-field rule; new scenario `a side-file
     failure after STATE.md commits is a warning, not a lost decision`.
   - **D**: `flow_status reads state and reality` body reworded (gate
     approvals: recomputed `valid` only for the current gate, `passed`
     otherwise); new scenario `a gate already left reports passed, not a
     recomputed validity`.
2. **`design.md`** — 8 "Fix round 1" notes (`grep -n "Fix round 1"
   design.md`), one per decision, each naming DECISION A-F explicitly, in
   D3 (A, B), D2 (C), D1 + D11 + D8 (D, three touch points), D9 (E, F).
   Each note names the exact function to change (`validate_phases`,
   `move_back`, `apply_gate`/`commit_transition`, `gate_validity`), the
   exact reviewer reproduction it closes, and — for C — the precise
   before/after-commit split and the `log_error`-pattern precedent it
   copies (`flow.rs:1519-1532`).
3. **`tasks.md`** — new `## 8. Fix round 1 (reviewer 11)` section, 8
   unchecked tasks (8.1-8.8), all 34 prior tasks untouched and still
   `[x]`. Grouped by file set per the brief: 8.1-8.4 sequential on
   `flow.rs` (one per decision A-D), 8.5-8.7 parallel on assets
   (orchestration.md for A+D; kicad-library-agent.md for E;
   kicad-manufacture-agent.md + orchestration.md for F), 8.8 final
   regression. Reviewer minor 6 explicitly called out as already closed
   by QA, no task.

## Evidence

1. `openspec validate --changes konnect-orchestrator --json` (repo root):
   `"valid": true`, 0 issues. Same result with `--strict`.
2. `grep -c "^- \[.\]" tasks.md` = 42, `grep -c "^Stack: none" tasks.md` =
   42, `grep -c "^Acceptance" tasks.md` = 42 (all three equal, per SC2).
   `grep -c "^- \[x\]" tasks.md` = 34 (all prior tasks still checked).
   `grep -c "^- \[ \]" tasks.md` = 8 (new section only).
3. `grep -n "Fix round 1" design.md` shows 8 notes covering A, B, C, D
   (x3: D1's table, D11, D8), E, F — every decision named.
4. SC3 (A, B, C each have a reproduction test that fails at `62d2045` and
   passes after the fix):
   - **A** — task 8.1: extends `foundation_tests::the_validator_names_
     the_entry_it_rejects` with 2 cases built directly from reviewer 11's
     Scenario A (`[routing, prefab_review, gate:purchase]`) and Scenario B
     (`[requirements, gate:architecture, schematic]`) shapes; I traced
     `validate_phases` (`flow.rs:416-459`) and confirmed today's single
     `GATED_PHASES` loop (`:450-457`) only checks phase⇒gate, so both new
     cases currently return `Ok`, i.e. the new assertions fail at
     `62d2045`. Also adds an e2e test in `flow_gate_e2e.rs` reproducing it
     through the published `flow_start` schema (not just the internal fn).
   - **B** — task 8.2: new `rewind_tests::a_rewind_cannot_target_a_gate`.
     I traced `move_back` (`:2078-2139`) and confirmed it never calls
     `is_gate(&request.to_phase)` — a rewind to `gate:placement` from
     `routing` is accepted today and calls `record_gate_keys` on the gate
     token, exactly reviewer minor 3's repro. The task's assertion (refused
     with `invalid_argument`) fails today, passes once `move_back` gains
     the check named in the task body.
   - **C** — task 8.3: two new tests exploiting a portable, deterministic
     fault-injection (pre-create the target subdirectory as a plain file,
     so `ensure_flow_subdir`'s directory creation fails cross-platform —
     no OS-specific locking trick needed, unlike reviewer's Windows
     `FILE_SHARE_DELETE` probe). I traced `apply_gate` (`:2319-2412`,
     comment at `:2347` "side files first, STATE.md last") and
     `commit_transition` (`:2177-2237`, `append_log` at `:2235` before the
     function returns) and confirmed both write their side file before the
     caller (`transact_state`) ever attempts the `STATE.md` rename. Today,
     forcing the side write to fail makes the *whole call* fail as an
     error with `STATE.md` untouched — the test's new expectation
     (success + `warning`, `STATE.md` updated) fails at `62d2045` and
     passes once the write order flips per the task body.

## For the next agent

1. **Decision C is the one real refactor.** `apply_gate` and
   `commit_transition`/`move_back` currently do I/O (write the gate file /
   append the log) and return `Result<(String, Value), CallToolResult>`
   directly to `transact_state`'s closure. To make `STATE.md` the sole
   commit point, they need to return the *data* to write (gate file text +
   path; log line) instead of writing it, and a new thin wrapper around
   `transact_state` for `flow_gate`/`flow_advance` needs to perform that
   write only after `transact_atomic` returns `Ok`, catching a failure
   into a `warning` field (mirror `flow_start`'s existing `log_error`
   field, `flow.rs:1519-1532` — same idiom, new field name because two
   different side effects can fail here, not one).
2. **No QA assertion needs to change.** I read every one of QA's 5 new
   guards' pinned substrings/sections in `asset_references.rs`
   (`orchestration_reference_keeps_its_protocol_rules`:1400,
   `orchestrator_agents_keep_their_contract`:1597,
   `record_sections_are_stated_where_their_writer_loads_them`:1855) and
   confirmed every fix-round asset edit (orchestration.md §1/§2/§7,
   kicad-manufacture-agent.md's Verdict/Ending-the-run,
   kicad-library-agent.md Step 6) is additive to a scope those guards
   read a heading or substring from, never removing or rewording a pinned
   phrase. `flow_contract_e2e.rs` (QA's) needs no edit either — decisions
   A-D only add new refusals/fields, none of which QA's existing e2e
   calls exercise in a way that would flip outcome.
3. **Decision F keeps `kicad-manufacture/SKILL.md` byte-identical** — only
   the *agent* file and `orchestration.md` change; the skill's vocabulary
   ("Only READY permits upload", "Any warning keeps INCOMPLETE") is the
   target the agent is brought back into line with, not something to
   relax further.
4. **Test names in tasks 8.1-8.4 are prescriptive, not decorative** — I
   picked them to match the existing module convention
   (`foundation_tests`, `rewind_tests`, `gate_tests`, `advance_tests`,
   `status_tests`) exactly as seen in `flow.rs`'s current test modules;
   reusing a different name will still satisfy the acceptance's behavioral
   claims but breaks the "name exact test names" contract the brief asked
   for — flag it in the dev handoff if a different name is used.
5. **Rejected approach for C's test**: I considered reproducing reviewer's
   exact Windows `FILE_SHARE_DELETE` OS-level probe in Rust, but that is
   platform-specific and flaky in CI; the file-blocks-a-directory trick
   is portable (Windows and Unix both fail `create_dir`/`create_dir_all`
   the same way when a plain file already occupies that path) and
   exercises the identical code path (`ensure_flow_subdir`).

## Deferred findings

(none) — everything reviewer 11 raised (SERIOUS + minors 1-5) now has a
task; minor 6 was already closed by QA per the orchestrator's note. QA's
own 4 deferred findings (10-qa.md) and reviewer's 5 deferred findings
(11-reviewer.md) are unrelated to this fix round and remain owned as
recorded there (dev/test-hygiene backlog, not blocking this change).
