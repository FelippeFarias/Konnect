---
change: board-dossier-reconstruction
task: "fix: CropRect alias + task 7.3 gate"
agent: developer
verdict: DONE
failing_layer: none
---

## Result

1. Fixed the one remaining red test. Added `type CropRect = [u32; 4];` in
   `crates/konnect-core/src/tools/photo_intake.rs` and used it in
   `render_view`'s `crop: Option<CropRect>` parameter and `parse_crop`'s
   `Result<Option<CropRect>, String>` return type (function bodies unchanged,
   still spell `[u32; 4]` internally). `cargo test -p konnect --test
   schema_parameter_usage` → `5 passed; 0 failed`.
2. Root cause confirmed exactly as diagnosed in `04-developer.md`: the guard's
   `all_function_bodies` drops any `fn` whose signature text contains a bare
   `;`, meant to skip trait-method declarations, but a `[u32; 4]` array type
   in a signature also contains `;` and tripped the same skip — so neither
   `render_view` nor `parse_crop` entered the transitive index and
   `prepare_board_photo.crop` looked unread.
3. Ran the full task 7.3 gate, all four commands exit 0: `cargo test -p
   konnect-core` (1384 lib + 4 conformance + 12 integration + 5 e2e, 16
   ignored, 0 failed), `cargo test -p konnect` (every binary green, 135 tests
   passed across all suites including `schema_parameter_usage`), `cargo fmt
   --check` (clean), `cargo clippy --all-targets` (0 warnings).
4. Committed `8be8d66` on `orc/board-dossier-reconstruction`:
   `fix(photo-intake): name the crop rectangle type so the parameter-usage
   guard indexes its readers`. Diff is exactly `crates/konnect-core/src/tools/
   photo_intake.rs`, +5/-2, nothing else touched. Worktree clean, nothing
   pushed.
5. Updated `.orchestrator/memory/developer.md` to record the fix and note
   that the guard's `;`-inside-signature skip remains a live trap for any
   future array-typed signature elsewhere in `konnect-core/src/tools/`.

## Evidence

1. `cargo test -p konnect --test schema_parameter_usage` → `running 5 tests …
   test every_declared_parameter_is_read_by_its_registered_handler ... ok …
   test result: ok. 5 passed; 0 failed; 0 ignored`.
2. `cargo test -p konnect-core` tail → `test result: ok. 1384 passed; 0
   failed; 16 ignored` (lib), plus conformance (4/0), integration (12/0),
   photo_intake_gate_e2e (5/0, 1 ignored — requires `RETRACE_PYTHON`).
3. `cargo test -p konnect` → 15 test binaries, every `test result: ok. N
   passed; 0 failed` line (23, 57, 12, 6, 0/3-ignored, 0/5-ignored, 3, 2, 18,
   2, 3, 5, 0, 4, 0 across the suites, doc-tests included).
4. `cargo fmt --check` exit 0 (no output). `cargo clippy --all-targets` exit
   0, `Finished` with no warning lines.
5. `git status --short` empty after commit; `git diff --stat HEAD~1` shows
   only `crates/konnect-core/src/tools/photo_intake.rs | 7 +++++--, 1 file
   changed, 5 insertions(+), 2 deletions(-)`.

## For the next agent

1. Round is clean: `schema_parameter_usage`, `cargo test -p konnect-core`,
   `cargo test -p konnect`, `cargo fmt --check`, `cargo clippy --all-targets`
   all exit 0 as of `8be8d66`. Nothing outstanding from task 7.3.
2. If a future change adds another array-typed parameter or return type
   (`[T; N]`) to a function signature under `konnect-core/src/tools/`, it
   will silently fall out of `schema_parameter_usage`'s transitive index the
   same way — the guard's `;`-inside-`{` skip is unconditional, not
   depth-aware. Either alias the array type (as done here) or fix the guard's
   depth tracking (option (b) in `04-developer.md`'s finding, still not
   taken; it touches the test file).
3. The 6 deferred findings and the task-6.1 walkthrough from `04-developer.md`
   are unaffected by this fix and still stand as the next round's queue.

## Deferred findings

1. None new from this task — see `04-developer.md`'s Deferred findings
   section, still open (D9 phase numbering, `references/` directory gap,
   cross-skill reference pointers unchecked, `agents_make_claimed_evidence_
   executable` marker-list gap, duplicated nine-row mapping table).
