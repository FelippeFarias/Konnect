# developer — project memory

## Current state

- `photo-to-kicad-reverse` and `board-dossier-reconstruction` merged into `main` and archived (`bbd5efb`); their worktrees are history, not work in progress.
- `konnect-orchestrator`: 50/50 tasks done (round 1 tasks 1.1-7.x; fix round 1 tasks 8.1-8.9; fix round 2 tasks 9.1-9.7) across 3 verify rounds; final commit `8b17a76` on `orc/konnect-orchestrator` (base `bbd5efb`, worktree `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\konnect-orchestrator`); archived 2026-09-22 at `9a0f1ce`. Merge to `main` pending the user's GRANT. Commit-by-commit history (handoffs `03`-`22-developer.md`) dropped now the change is archived; the shapes/decisions worth keeping live in "## Decisions affecting my role" below.
- Final counts: 23 toolsets / 238 registered / 245 with meta / 11 categories; 12 skills / 11 agents.
- `install-scripts`: `scripts/install.ps1` + `scripts/install.sh` on `orc/install-scripts` (base `9a0f1ce`,
  worktree `C:\Users\felip\.orc\worktrees\konnect-3b7e2022\install-scripts`): `897c4a0` initial, `334a8ac`
  the fast lane's single repair round (qa R2 + reviewer SERIOUS/MINORs, handoff `04-developer.md`; a second
  FIX promotes the lane to PROPOSE). Never run for real here; README/DEV docs queued separately.
  `6a2f7d2` = DECISION O post-verify fix (handoff `07-developer.md`): ps1 rollback is ONE guarded
  `if ((Test-Path B) -and (Test-Path T) -and -not (Test-Path P)) {…} else {'rollback stopped: …'}`
  (target-absent variant: `B -and -not T`; fresh install: `T -and -not P`), `Get-RollbackLines`
  returns nothing under `-DryRun`, `Format-PsLiteral` doubles U+0027/2018/2019/201A/201B. Probe
  harness: session scratchpad `t7.ps1` (reuses `rv2\bin\old.exe`/`new.exe`, cleans its own `d7\`).

## Decisions affecting my role

- Build env, every cargo call: `PROTOC=C:/Users/felip/tools/protoc/bin/protoc.exe` and the VS 2022
  BuildTools CMake `bin` on PATH. `RETRACE_PYTHON` is UNSET at user, machine and Bash-process scope
  (checked 2026-09-22) — the venv is `<main checkout>/.venv-retrace` (retrace 0.3.0); worktrees have none.
- Installer tests: never `init` (writes the real `~/.claude`; konnect resolves home via the OS, not
  `$HOME`). HARD RULE (after qa's 2026-09-22 incident): EVERY invocation carries `-DryRun`, or all of
  `-Target <temp> -SkipInit -NoRetrace` (via a wrapper that appends them); snapshot the real exe sha256 +
  RETRACE_PYTHON before/after. Recipe (fake repo, rustc fake binary, cmdlet/PATH shims): `04-developer.md`.
  `~/.claude/settings.json` mtime moves on its own (Claude Code model/effort edits) — prove init did not
  run by `~/.claude/agents|skills` mtimes, not by settings.json.
- The content hash now has FOUR lists, not two: `CONTENT_KEYS` (always covered),
  `OPTIONAL_CONTENT_KEYS` = `dossier`/`design_brief` (covered only when the map carries them, cloned
  whole — no field projection, because nothing tool-written lives inside them), `UNHASHED_KEYS`, and
  `SCALE_REFERENCE_OPTIONAL_CONTENT_KEYS` = `mm_per_px`/`evidence` via `hashed_fields_with_optional`.
  `hashed_fields`' unconditional insert is untouched and must stay that way: every stored digest
  depends on an absent `ref` contributing `"ref": null`.
- `every_schema_key_is_either_hashed_or_deliberately_not` now reads `review_map_schema()`, not the
  fixture map — the fixture deliberately carries none of the new sections so its pinned digest
  `18afc7b9…` still proves a pre-change map hashes unchanged.
- `validate_path_token(raw, field)` is the one token rule; `validate_map_id` delegates to it so the
  `map_id` error text is byte-identical and `prepare_board_photo`'s `label` gets its own field name.
- `prepare_board_photo` renders the view in memory BEFORE creating `views/`, so a rejected call
  leaves no directory either — three of the four bound tests assert exactly that ordering.
- `tool!` takes exactly four arguments (`tools/mod.rs:425`); `BoardAccess::None` is `#[default]`.
  Handler shape: `async fn handle_x(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult>`.
- The content hash is a PROJECTION, not a selection: `CONTENT_KEYS` picks the four top-level keys
  and `hashed_fields`/`hashed_elements` then reduce each component, net and the scale reference to
  `COMPONENT_CONTENT_KEYS` / `NET_CONTENT_KEYS` / `SCALE_REFERENCE_CONTENT_KEYS`, with `""` → null
  applied to `value`/`footprint_suggestion`. Adding a D8 field means adding it to one of those lists
  or it is unhashed and unguarded.
- `TOOL_OWNED_KEYS` (`approval_valid`, `approved`, `approved_at`, `content_hash_at_approval`,
  `saved_at`) are removed from the incoming `map` before the overlay. Ignored, never rejected: a map
  that came from `load` legitimately carries four of them.
- The gate's shape is unchanged: only `approve_photo_review_map` writes `approved: true`; `save`
  re-grants it only when the content hash still matches; `load` reports `approval_valid`
  server-side. Assets say "read `approval_valid`, never `approved`" — now on BOTH sides.
- The review map is an OPEN record by design: `map`, `scale_reference`, `components[]` and
  `nets[]` all declare `additionalProperties: true`, and each is listed with its reason in
  `router/mod.rs`'s `fixed_records_are_closed_and_only_reviewed_maps_are_extensible`.
- Registry and docs move together, and "docs" is repo-wide: `doc_tool_counts` sweeps every
  `.md`/`.json`, so `docs/TROUBLESHOOTING.md`, `packaging/metadata.json` and `plugin/plugin.json`
  bump alongside README/DEV/tool-directory.

- `flow.rs` shape (konnect-orchestrator): phases are `String`s in `JobState`, checked against
  `CANONICAL_PHASES` by `validate_phases` and re-validated on every `parse_state`; `Lane`/`Mode`/
  `HistoryKind`/`GateDecision`/`ApprovedBy` are serde enums with an `as_str` twin pinned by
  `the_vocabularies_agree_with_each_other`. `PHASE_RECORDS` is the single D4 table
  (`required_records`, `record_phase`, `package_records` all read it).
- Every mutating flow handler: pure arg checks → `resolve_project_dir` → `spawn_blocking` →
  `transact_atomic(STATE.md, …)` whose closure returns `(current, Err(refusal))` on any refusal
  (so nothing is written) and writes side files only after every check passed. `flow_start` alone
  uses `write_new_atomic` first and falls through to the locked path on `AlreadyExists`.
- The gate-entry history entry carries `package_hash` + `package_files` (per-record sha256, `null` =
  absent); `current_visit` = last history index whose `to` is the current phase; the leave-gate
  D11 check (`check_gate_exit`) already lives in `flow_advance` (1.4), so 1.6 only adds `flow_gate`.
- 1b shapes: every mutating tool but `flow_start` runs `transact_state(project, |flow_dir,
  state_path, current| …)` + `load_job` (parse → `conflict`, foreign job → `stale_target`);
  `record_gate_keys` fills package keys on ANY entry whose `to` is a gate (forward or rewind);
  `commit_transition` is the shared log/push/render tail. A rewind clears approvals of gates at
  OR after the target. `flow_gate` compares against `history[current_visit]`, never now-vs-now;
  readiness refusal field = `decision`, phase mismatch (closed included) = `gate_name`.
  `flow_log` applicability is the `LOG_PARAMETERS` table (`role` is an optional author tag on
  decision/evidence); blank optional strings = absent; handoff NN = max existing + 1.
  `evidence_check` is stored only when `evidence_calls` is non-empty (deduped, rewinds too).
- Fix round 1 shapes: `validate_phases` checks `GATED_PHASES` both ways; `move_back` refuses
  `is_gate(to_phase)` FIRST (field `to_phase`) and no longer calls `record_gate_keys` (only a
  forward entry into a gate carries keys now); `gate_validity` emits `status` and `valid` only
  for the gate equal to `state.phase`. `transact_state` wraps a generic `transact<T>`;
  `flow_gate`/`flow_advance` use `transact_then_record`: `apply_gate`/`commit_transition` return
  a `Committed { response, gate_file, log }`, the side files are written after
  `transact_atomic` returns `Ok`, failures land in `warning` (always present, `null` on success).
  `flow_defer` joined them in 8.9 (`Committed { gate_file: None, .. }`); `flow_start` alone keeps
  its own `log_error` field. `flow_log` still appends inside the closure (its content IS the log).
- Manufacture agent verdicts (8.7, narrowed by 9.3/DECISION I): the "any warning keeps INCOMPLETE"
  rule covers ARTIFACT checks only (the export's `warnings` array); DRC errors are resolved or
  waived, DRC warnings and preflight issues adjudicated in Design evidence, and an adjudicated one
  is not an open item. The agent never marks the three purchase-gate checks done, so it ends
  `INCOMPLETE` naming them and exits to `gate:purchase`; the SESSION declares READY with
  `flow_log(kind: evidence)` after `flow_gate` approves purchase (orchestration.md §4).
- `flow_advance` never writes a gate file (`commit_transition` → `gate_file: None`); only
  `flow_gate` does. Per-tool texts must not copy §7's generic "(gate file or log entry)".
- `konnect_sexp::open_document_lock` is `pub(crate)`: konnect-core cannot hold the `STATE.md` lock
  past `transact_atomic`, so post-commit side writes run unlocked (ordering race noted in 13).

## Gotchas found here

- `review_map_schema()` outgrew `json!`'s macro recursion limit the moment `dossier`/`design_brief`
  went inline. `dossier_schema()`/`design_brief_schema()` are separate functions for that reason —
  do NOT inline them back, and do NOT raise `recursion_limit` on the crate.
- `image` 0.25.10 specifics: `Limits` is `#[non_exhaustive]` (no `..Limits::no_limits()` literal —
  bind it `mut` and assign `max_alloc`); `reader.limits(l)` returns `()` so it is not chainable;
  `.dimensions()`/`.orientation()` need `use image::ImageDecoder as _` on the opaque
  `into_decoder()` type; `ImageReader::decode()` does NOT apply EXIF orientation, only
  `from_decoder` + `apply_orientation` does.
- EXIF fixtures without a camera: splice a PNG `eXIf` chunk (raw little-endian TIFF, tag 0x0112,
  type SHORT, count 1 — value 6 = `Rotate90`) in after IHDR; the `png` crate hands it to
  `Orientation::from_exif_chunk` verbatim. Same 8-line hand-rolled CRC-32 also patches IHDR's
  width/height to fake a 65535x65535 header for the megapixel-cap test — `decoder.dimensions()`
  reads the header, so the bogus pixel data is never touched.
- WhatsApp reference photos (`C:\Users\felip\Downloads\WhatsApp Unknown 2026-09-18 at 12.00.58\`)
  are 900x1600 baseline JPEG with EXIF stripped: `exif_orientation` comes back `"NoTransforms"`.
  Verified end to end through `prepare_board_photo` (crop 400x400 at scale 2.0 → an 800x800 PNG).
- `ToolDef::new` → `close_input_schema` (`tools/mod.rs:134`) inserts `additionalProperties: false`
  into EVERY object subschema that declares none, recursing through `properties`/`items`/etc. An
  explicit `true` survives (`entry().or_insert()`). A bare `{"type":"object"}` argument therefore
  ships a tool that accepts `{}` and nothing else, and `mcp/handler.rs:366` validates before it
  dispatches — while every unit test that calls `handle_*` directly stays green.
- `std::fs::canonicalize` returns `\\?\C:\…` OR `\\?\UNC\srv\share\…` on Windows. Strip the first
  outright; the second must become `\\srv\share\…` or it turns into a relative path
  (`subprocess_arg`; `portable_uri`, `library.rs:1818`, still has the bug).
- The workspace has NO date crate, so RFC 3339 is hand-rolled (`rfc3339_utc`).
- `serde_json` has `preserve_order` OFF, so `Map` is a `BTreeMap` and `to_vec` emits sorted keys at
  every depth. Every stored content hash depends on that.
- `config::deep_merge` treats a `null` overlay as "no opinion" and keeps the base. That is the
  opposite of what a save needs (`null` IS the value being written), hence the separate
  `overlay_known_fields` in `photo_intake.rs`.
- Never spawn a blocking test child through `cmd /C` or `sh -c`: the grandchild keeps the inherited
  stdout pipe open. Spawn `ping`/`sleep` directly.
- retrace 0.3.0 on this machine: `.venv-retrace/Scripts/python.exe`, base install only (neither
  `ultralytics` nor `easyocr`), contour scan of an 800x600 synthetic board about 0.6 s.
- `agents_make_claimed_evidence_executable` now requires `load_toolset("photo_intake")` and the
  `approval_valid` marker in `kicad-schematic-build-agent.md`; a marker list addition passes on the
  first run whether or not it bites, so mutate the asset (rename the marker) and read the failure.
- `snake_words` (`asset_references.rs`) flags every snake word with **2 OR MORE** parts, not just
  two: `parts.filter(non-empty).count() >= 2`. `design_brief_seed`, `max_component_height_mm` and
  `depends_on_open_questions` are all collected. The existing NOT_TOOLS entries
  (`content_hash_at_approval`, `exclude_from_pos_files`) prove it. Any handoff claiming otherwise
  is wrong — run the guard and take its list.
- `schema_parameter_usage.rs`'s `all_function_bodies` skips any `fn` whose signature text contains
  `;`, to ignore trait declarations — so a signature carrying `[u32; 4]` is dropped from the
  transitive index and its `args.get("…")` literals are invisible. That is why
  `prepare_board_photo.crop` reads as an ignored parameter. Fix: a `type` alias in the source, or
  a depth-aware `;` check in the test.
- `asset_references.rs` guards, in the order they bite: `snake_words` flags EVERY 2+-part snake word
  (backticked or bare) unless it is a registered tool, a TOOLSET name, a top-level schema property,
  or in `NOT_TOOLS` — a dotted result field like `drc.schematic_parity` trips it too (task 9.7:
  describe the field in words instead); `signature_examples` schema-checks any `tool(a, b)` whose args are all bare
  lowercase identifiers; a `load_toolset('x')  # a, b` trailing comment may name only that toolset's
  tools; `agents_make_claimed_evidence_executable` and
  `skills_define_the_same_evidence_boundary_as_their_agents` are HARD-CODED per-file marker lists —
  new prose in an existing agent gets no guard unless you add a marker there.
- Python heredocs: this repo mixes line endings (`photo_intake.rs`, the `.md` assets are LF;
  `config.rs`, `asset_references.rs` are CRLF). A `patch()` helper that converts `\n` → `\r\n` when
  the file already has CRLF is the only reliable way to string-match; `sed -i` silently rewrites
  CRLF files as LF.
- The `schema_parameter_usage` red above is fixed (commit `8be8d66`): option (a) from the prior
  entry — `type CropRect = [u32; 4];` in `photo_intake.rs`, used in `render_view`'s `crop` param
  and `parse_crop`'s return type (bodies keep the `[u32; 4]` literal). The guard's `;`-inside-`{`
  skip is still live for any *other* future array-typed signature in that file or elsewhere in
  `konnect-core/src/tools/`; option (b) (depth-aware `;` check in the test) was not taken — it
  would touch the test file, which is out of `@dev`'s write set for this kind of fix anyway.
- Adding a hard-coded marker case to `agents_make_claimed_evidence_executable` or
  `skills_define_the_same_evidence_boundary_as_their_agents`: `get_drc_violations` lives in
  `pcb_export`, while `run_drc` lives in `verification` (`tool-directory.md` sections and its
  line 534 say so). Grep `registry.rs`, the tool's own doc comment or the tool directory for the
  real toolset before writing a `required_toolsets` entry; never infer it from a name.
- Writing skill/agent prose: any `tool(a, b)` of bare identifiers is schema-checked, so a
  shorthand like `flow_status(read)` fails `call_examples_name_real_parameters`. Write the full
  required shape and keep values in prose, or use `name: value` (a colon is skipped). Backticked
  `Lib:Symbol` tokens are checked against installed KiCad. `references/reliability-contract.md`
  DOES exist at install: `manifest.rs:39-40` embeds `docs/RELIABILITY_CONTRACT.md` under that
  name (`install.rs:735-748` tests it) — citing it is valid. `signature_examples` scans ONE line:
  a `flow_advance(…)` wrapped across two lines is silently unchecked, so keep each on one line.
- Bundled agents (`mcp__konnect__*` only) cannot open a datasheet, a web page (live JLCPCB stock)
  or a Gerber viewer. The new agents say so and route those checks to the session or the
  purchase gate instead of claiming them (sourcing: `located, not validated`; manufacture:
  `## Checks at the purchase gate`; library: pinout pages must come in the brief).
- `prepare_board_photo` refuses to overwrite an existing view file (round-1 fix, `f982824`): the
  check is `view_path.exists()` right before `rendered.image.save(&view_path)`, so a rejected call
  never touches a file already on disk. Proving that needs re-reading the bytes after the refused
  call and asserting equality with the bytes read before — a bare `result.is_error` assertion does
  not rule out a partial/truncated overwrite.
- `design_state_hash` already skips every `*.lck`, so `~demo.kicad_pro.lck` is never a covered file;
  `flow_status.lock_files` derives `~<name>.lck` beside each covered file instead.
- `konnect_sexp::transact_atomic` reads the file BEFORE the closure, so it cannot create one: the
  first `STATE.md` needs `write_new_atomic`, whose `persist_noclobber` failure arrives as
  `SexpError::Io` with kind `AlreadyExists`.
- The worktree checks files out CRLF (`core.autocrlf=true`, index LF); new files written LF stay LF
  until git touches them. `cargo fmt` (no `rustfmt.toml`) rewrites any file it actually reformats
  as LF-only and leaves untouched files CRLF — re-detect EOLs before any scripted patch after fmt.
- `flow_gate_e2e.rs` dispatches through `ToolDef.handler`, bypassing `mcp/handler.rs`, so the
  observer ring is empty there: an e2e passing `evidence_calls` would see them all "missing".
- In Git Bash, `git show <rev>:<path>` prints the SMUDGED (CRLF) text, so `grep -c $'\r'` on it
  says "all CRLF" for an LF blob — read the truth with `git ls-files --eol <path>`. A cumulative
  `git diff --stat` over several commits that insert blocks mid-file reports hundreds of phantom
  deletions (Myers alignment): check per-commit stats or `--diff-algorithm=histogram` first.
- `not_tools_allowlist.rs` parses the "photo-intake block" from its marker comment to the
  NOT_TOOLS array's closing `    ];`, so ANY block appended at the array's end is checked too
  (no tool, no top-level parameter) — the flow block relies on that. A block inserted BEFORE the
  photo-intake marker gets no such check, and the failure text still says "photo-intake".
- `manifest_ships_every_asset` (install.rs tests) walks `assets/` and compares keys AND bytes
  with `SKILLS`/`AGENTS`; the only non-asset entry is `GENERATED_REFERENCES`
  (`konnect/reliability-contract.md` ← `docs/RELIABILITY_CONTRACT.md`). A new generated
  reference must be declared there, and any non-`.md` file under `assets/` fails it as stray.
- `agents_make_claimed_evidence_executable` mutation: the schematic agent loads `flow` TWICE, so
  a mutation removing one occurrence stays green — replace all occurrences.
- `serde_json::Value`'s `v["key"]` returns `Null` for a MISSING key, so `assert_eq!(v["warning"],
  Value::Null)` cannot prove a field is present-and-null (I wrote exactly that in 8.9's test and
  caught it): use `v.get("warning").is_some_and(Value::is_null)`. Candidate for a rust stack file.
- `schema_parameter_usage` iterates only REGISTERED tools (`registry::ALL_TOOLSETS`), so an
  unregistered `flow.rs` is invisible to it until 1.8; its transitive literal walk follows helper
  calls (`log_request` → `opt_text`/`opt_token`, `parse_records`).
