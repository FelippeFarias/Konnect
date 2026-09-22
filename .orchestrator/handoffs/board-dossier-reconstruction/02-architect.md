---
change: board-dossier-reconstruction
task: refine design.md (close concerns a-h, verify citations, write the Pre-mortem)
agent: architect
verdict: DONE
failing_layer: n/a
---

## Result

1. `design.md` rewritten in place, keeping the planner's structure: D1-D13 became
   D1-D14 (new D14 = implementation order), every one of the orchestrator's
   concerns (a)-(h) is closed with its own argument, trade-off and a **named
   rejected alternative**, and the `## Pre-mortem` placeholder is replaced by
   **7 numbered causes, each paired with a hardening that names the decision
   changed**. A new `## Open Questions` section carries the three
   non-blocking items with an owner and a slice each.
2. **Two wrong citations corrected, one of them load-bearing.** The planner put
   D6's conditional-inclusion edit in `hashed_fields`/`hashed_elements`
   (`photo_intake.rs:800`/`:820`). It belongs in `review_map_content_hash`
   (`:860`): `hashed_fields`' unconditional `insert(key, …unwrap_or(Null))`
   (`:810-814`) is what makes a component with no `ref` contribute a
   `"ref": null` member, so making *it* skip absent keys would change the
   canonical bytes of every existing map and invalidate every approval in the
   field. D6 now states the three exact edits (`OPTIONAL_CONTENT_KEYS`, a new
   `hashed_fields_with_optional`, and chaining the new list into
   `every_schema_key_is_either_hashed_or_deliberately_not`, `:2751`).
3. **D6 also names a second landmine the planner's draft would have shipped:**
   `PhotoReviewMap`'s new `Option` fields must carry
   `skip_serializing_if = "Option::is_none"`. `handle_save_photo_review_map`
   overlays `to_value(&parsed)` onto the incoming map (`:1553-1556`), so a field
   serialized as `null` lands in the record, `map.get("dossier")` returns
   `Some(Null)`, the conditional insert fires, and **every pre-existing approved
   map is revoked on its next save**. `subcircuit_hints` (`:691-692`) is the
   precedent; task 2.4 case (5) is the test.
4. **D6 drops the per-field key list for `dossier`/`design_brief` and hashes each
   section whole.** The archived projection exists to keep tool-written
   bookkeeping and reviewer annotations out of the gate; neither class exists
   inside these two sections (no tool writes into them; they *are* the reviewed
   content), so a projection buys nothing and costs exactly the predecessor's
   named "fail-open key list" failure — and the "iterate the list against the
   struct" guard the CT asked for cannot exist, because both are open
   `serde_json::Value` objects with no struct to iterate. The guard is moved to
   the section level, where it can be enforced.
5. **D7's allowlist is now exact and finite: six paths, not "a floor".** The
   planner's twelve-entry list was missing at least eight of its own paths.
   `review_map_schema()` declares object nodes with `properties` and every
   array-of-objects as `{"type":"array","description":…}` with no `items`
   subschema (the `subcircuit_hints` precedent, `:1146`), because the guard's
   walker (`router/mod.rs:187-255`) never descends into an array without
   `items`. Element field lists move to the two skill reference docs, where the
   writing agent actually reads them.

## Evidence

1. Verified by reading the cited line in the worktree
   (`konnect-3b7e2022/board-dossier-reconstruction`, base `1815948`): root
   `Cargo.toml:90`, `konnect-core/Cargo.toml:47` (dev-dep), `konnect-render/
   Cargo.toml:12`/`:16`, `photo_intake.rs` `prepare_map_dir:591`,
   `canonical_existing_dir:612`, `canonical_existing_file:623`,
   `CONTENT_KEYS:749`, `UNHASHED_KEYS:758`, `SCALE_REFERENCE_CONTENT_KEYS:786`,
   `hashed_fields:800`, `hashed_elements:820`, `review_map_content_hash:860`,
   `approval_is_valid:890`, `validate_map_id:949`, `existing_map_dir:969`,
   `validate_incoming_map:1033`, `review_map_schema:1070`,
   `subcircuit_hints` schema `:1146`, `tools():1158`,
   `overlay_known_fields:1452`, `TOOL_OWNED_KEYS:1493`,
   `handle_save_photo_review_map:1521`, guard tests `:2751` and `:2773`;
   `router/registry.rs:123` (`tool_count: 5`), `router/mod.rs:185-261`;
   `library.rs:349`/`:392`.
2. `image` 0.25.10 (`Cargo.lock:2349-2351`) read from the vendored crate:
   feature `jpeg = ["dep:zune-core","dep:zune-jpeg"]`;
   `ImageDecoder::orientation()` (`src/io/decoder.rs:53`, defaults to
   `NoTransforms` when there is no EXIF tag);
   `DynamicImage::{from_decoder:243, crop_imm:508, resize_exact:892,
   rotate90:1116, rotate180:1124, rotate270:1135, apply_orientation:1161,
   save:1394}`; `ImageReader::{limits:137, into_decoder:219,
   with_guessed_format:254, open:344}`; `Limits::{max_image_width:35,
   max_alloc:42, no_limits:62}`. `ImageReader::decode()` (`:311`) does **not**
   apply orientation — hence D1's explicit `into_decoder` → `orientation` →
   `from_decoder` → `apply_orientation` sequence.
3. Concern (d) closed against code, not assumption: `asset_references.rs`'s
   `yaml_list` (`:151`) has exactly one caller, with the key `"skills"`
   (`:103`), and `install.rs:276-277` writes each agent file's bytes verbatim.
   **No test parses `tools:`**, so adding `Read` needs no test change.
4. Concern (e) verified field-by-field against
   `crates/konnect/assets/skills/kicad-pcb/references/layout-methodology.md:28-38`
   (the nine-row constraint table) and `:53-57` (the completion criterion and
   the four load-bearing rows). `design_brief.physical_constraints` now has one
   field per row plus `keep_outs[]` and an `unresolved[]` marker.
   `kicad_symbol`/`kicad_footprint` were **kept** (not renamed to
   `lib_id`/`footprint`) because `specs/photo-intake/spec.md:149`/`:159` already
   cite those paths and spec.md is not in this task's write set.
5. `grep -ciE "recommend|presumably|likely|confirm against|TBD" design.md` = 0
   (the broader `…|confirm|…` sweep is also 0). `orc validate --changes
   board-dossier-reconstruction --json` → `"valid": true`, 0 issues.
   `tasks.md`: 31 checkboxes / 31 `Stack: none` / 31 `Acceptance:`, no
   renumbering. Twelve tasks edited: 1.3, 1.5, 2.1, 2.2, 2.3, 2.4, 3.2, 3.3,
   3.4, 3.5, 4.1, 4.2, 4.5, 5.2, 6.1.

## For the next agent

1. **Implementation order is D14, and it is not the task order.** Do 1.1
   (Cargo), then **2.1 + 2.3 + 2.4 before 1.2-1.5** (the hash change is the one
   that can break existing tests — find out first), then 2.2 immediately after
   2.1 (the allowlist guard fails the moment the schema gains an open node, and
   a guard left failing for three tasks gets "fixed" by weakening it), then
   1.2-1.6, then the assets, then 4.6/5.2, then 5.1/5.3/5.4/5.5. 6.1 is the
   orchestrator's, last.
2. **Most likely thing done wrong #1: the `skip_serializing_if` attribute.**
   Declaring `dossier: Option<serde_json::Value>` without
   `#[serde(default, skip_serializing_if = "Option::is_none")]` revokes every
   approved map in the field on its next save, and no test in the existing
   suite fails for it except the one task 2.4 case (5) adds. Copy
   `subcircuit_hints` (`photo_intake.rs:691-692`) exactly.
3. **Most likely thing done wrong #2: putting the conditional insert in
   `hashed_fields`.** It reads like the natural place and it is wrong — see
   Result 2. `hashed_fields` (`:800`) keeps its unconditional insert; the new
   branches live in `review_map_content_hash` (`:860`) and in a **new**
   `hashed_fields_with_optional`. Do not add an `optional_keys` parameter to
   `hashed_fields` itself and pass `&[]` at the other call sites — that is the
   same edit with a wider blast radius.
4. **Most likely thing done wrong #3: enumerating D4/D5 fully in
   `review_map_schema()`.** Every `items: {type: object}` you add is another
   `fixed_records_are_closed_…` allowlist entry and another
   `.expect("every record has an explicit policy")` panic waiting on a node
   with no `additionalProperties`. D7's six paths are the whole set **only if**
   arrays are declared without `items`. If the test names a seventh path, the
   fix is to remove the `items` subschema and document that element's fields in
   `dossier-schema.md`/`design-brief-schema.md`, not to extend the allowlist.
5. Task 5.2's `NOT_TOOLS` list is still derived mechanically from the test's own
   failures (D10 gives the starting set, deliberately not a final one). Two
   traps: `asset_references.rs`'s `snake_words` only collects **two-part**
   snake_case, so single words (`confidence`, `evidence`, `candidates`,
   `hypotheses`, `circuits`, `bom`) are never flagged and must not be added;
   and `image_path`/`project_dir`/`map_id` resolve as real top-level tool
   properties, so adding them would be wrong.

## Deferred findings

1. The planner's deferred finding (partial `library`-toolset loading for
   `pcb-design-reconstruction-agent`) is **closed** in D9: no such mechanism
   exists in the router, loading `library` exposes all 17 tools
   (`registry.rs:111`), and the restriction is a Hard Rule in the agent file —
   the same mechanism every other bundled agent's mutation boundary uses. A
   narrower mechanism would be a router change for every toolset and belongs in
   its own change.
2. `dossier.photo_views_used` names view files but nothing verifies they still
   exist on disk; a reviewer opening an old map cannot tell a deleted view from
   a renamed one. Recorded as Open Question 1 (owner `@architect`, whichever
   slice first ships a review UI).
3. Whether an LLM's own count from `prepare_board_photo` views reaches D12's
   ±10 % without the concept-validation run's ad-hoc HSV blob script is
   unmeasured; if task 6.1 item 3 fails, a seventh `photo_intake` tool
   (`count_blobs`) is the follow-up, not a tolerance change. Open Question 2
   (owner `@pm`).
4. Both approval checkpoints remain text-enforced — `sch_*`/`pcb_*` handlers
   still do not check `approval_valid` server-side. This change adds a second
   gate of the same kind rather than closing the first. Open Question 3
   (owner `@architect`, its own change).
