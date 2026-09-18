# Board dossier — schema reference

The `dossier` section of a photo review map: what the photographs say this
board is, with the pixels behind every claim. It is an **optional** top-level
key of the map stored at
`<project_dir>/.konnect/photo_intake/<map_id>/review_map.json`, beside
`components`, `nets` and `scale_reference`.

Read this before writing or editing a `dossier`, and before consuming one.

Two things the JSON Schema cannot enforce, and this file therefore does:

- **Array elements have field lists too.** Every array below is declared in the
  tool schema as a plain `{"type": "array"}` with a prose description and no
  `items` subschema, so the server does not check element shape at all. The
  per-element tables here are the contract.
- **A section is an object, or it is absent.** `save_photo_review_map` rejects
  `"dossier": null` with "Omit the key entirely to remove the section". There
  is no such thing as a nulled-out dossier.

Every level is open (`additionalProperties: true`), for the reason the rest of
the map is open: a human edits this file by hand between calls, and an
annotation no tool reads must survive a save.

---

## Evidence pointers — the shape used everywhere `evidence` appears

An evidence entry is an **object**, never a sentence, so a reviewer can open
it:

```json
{ "view": "boardA_zoom3.png", "rect_px": [30, 430, 180, 24], "note": "silkscreen identity line" }
```

| Field | Required | Meaning |
|---|---|---|
| `view` | yes | Either the bare file name of a view inside the map's `views/` directory (a `prepare_board_photo` output) or a string **equal to one of the map's `source_images` entries**. Nothing else is a valid `view`. |
| `rect_px` | yes | `[x, y, w, h]` in that view's own coordinate space: for a `views/` file, the space `prepare_board_photo` reported as `output_size_px`; for a `source_images` entry, the oriented source space it reported as `source_size_px`. |
| `note` | no | Prose, for what the rectangle is meant to show. |

Nothing checks that the file exists or that the rectangle is inside it. That
makes writing a pointer nobody can follow the easiest mistake in this
document, and the one a reviewer catches last. Point at a view you actually
produced, with a rectangle you actually cropped or measured.

**Every claim-bearing object carries `basis` and `confidence`:** `basis` is the
string `observed` or `inferred`, `confidence` a number in `0..1`, unrounded.
An LLM survey mixes both kinds of statement in one document, and the pair is
what lets a reviewer tell them apart without reading tone.

---

## Top-level fields

| Field | Type | Meaning |
|---|---|---|
| `identity` | object | What this board is, as one claim. |
| `physical` | object | Sizes, holes, connectors, and the scale situation. |
| `component_survey` | array | One entry per visual class. |
| `silkscreen_markings` | array | One entry per legible printed string. |
| `topology_claims` | array | One entry per open question about the circuit. |
| `retrace_correlation` | array | Ties survey observations to `scan_pcb_photo` component ids. |
| `photo_views_used` | array of string | The view files this survey was read from. |
| `design_brief_seed` | object | A preliminary sketch of the implied design direction. |
| `open_questions` | array of string | What the photos could not settle. |

---

## `identity`

| Field | Meaning |
|---|---|
| `summary` | One sentence naming the product class, quoting the silkscreen identity where there is one. |
| `basis` | `observed` when the summary rests on printed text; `inferred` when it rests on the parts. |
| `confidence` | 0..1. |
| `evidence` | Evidence pointers. For an identity read off silkscreen, `evidence[0]` names the view the string was read from. |

## `physical`

| Field | Meaning |
|---|---|
| `board_size_px` | `[w, h]` in the oriented source space — always available, always written. |
| `board_size_mm` | `[w, h]` in millimeters, **`null` until a scale reference resolves one**. Never estimated, never taken from a "typical" board size. |
| `scale_status` | Why `board_size_mm` is `null`, in the words the user needs: what measurement would resolve it. |
| `mounting_holes` | Array — see below. |
| `connectors` | Array — see below. |
| `layers_visible` | Which layers these photographs actually show, e.g. "top silkscreen + bottom copper only; inner layers out of scope". |

### `physical.mounting_holes[]`

| Field | Meaning |
|---|---|
| `position_px` | `[x, y]` of the hole centre in the named view's space. |
| `role` | What kind of hole. **The string must literally contain `plated` or `unplated`** — `plated_corner`, `unplated`, `plated_standoff` — because plating is the one property a reader needs to filter on, and "corner hole" does not say it. When you cannot tell from the photo, say so in `notes` and use the word you can defend. |
| `count` | How many holes this entry stands for, when several share a role and a description. |
| `evidence` | Evidence pointers. |

### `physical.connectors[]`

| Field | Meaning |
|---|---|
| `type` | What it is, as seen: "2-pin screw terminal, 5.08mm pitch". |
| `location_px` | `[x, y]` in the named view's space. |
| `edge` | Which board edge it sits on. **The string must start with `top`, `bottom`, `left` or `right`** — `bottom`, `bottom-left`, `right edge near the holes` — so the side is machine-readable and any refinement follows it. A connector that is not on an edge is `interior`. |
| `basis` / `confidence` | As everywhere. |
| `evidence` | Evidence pointers. |

## `component_survey[]`

One entry per **visual class** — what the part looks like, not what it does.

| Field | Meaning |
|---|---|
| `visual_class` | Short token for the class: `led_5mm_clear`, `axial_resistor_tht`, a 2-position screw terminal, an 0805 SMD passive, an 8-pin DIP IC. |
| `count` | The number you stand behind. |
| `count_method` | How that number was reached: `manual_count_by_region`, `blob_count`, `hough_circles`, or your own named method. **A count with no method is not a count.** |
| `count_confidence` | 0..1, unrounded. |
| `count_alternatives` | Array of `{method, count, note}` — a second method's number and why you did not use it. Present whenever two methods disagreed. |
| `locations` | Array — see below. The `observation` entries' counts **sum to** `count`; a total no location accounts for is not traceable. |
| `retrace_component_ids` | Array of `scan_pcb_photo` component ids correlated to this class, or `[]`. |
| `notes` | What was not legible at this resolution, e.g. resistor colour bands. |

`count_alternatives` exists to **hold** disagreement, not to resolve it. Two
methods giving different numbers is the only signal a reviewer has that a count
is soft; recording only the trusted number throws it away.

### `component_survey[].locations[]`

| Field | Required | Meaning |
|---|---|---|
| `region` | yes | Where on the board, in words: "top-left", "near terminal", "right edge". |
| `view` | yes | The view file this group was counted in, named exactly as an evidence pointer names one. |
| `rect_px` | yes | `[x, y, w, h]` bounding the group inside that view, so the region is a rectangle a reviewer can open, not a description. |
| `count` | yes | **Integer.** How many parts of this class are in that rectangle. Never prose, never absent, never "a row of 5" written in `region`. |
| `kind` | yes | `observation` or `cross_check`. |

**The reconciliation rule:** the class's `count` **equals the sum of `count`
over the entries whose `kind` is `observation`**. That is the whole point of
the list — a reviewer, or a script, adds the groups up and gets the total, or
finds the discrepancy.

- `observation` entries **partition** the class: disjoint regions, each part
  counted once. Overlapping regions break the sum silently, which is worse than
  an obviously wrong total.
- `cross_check` entries are re-counts of ground an `observation` entry already
  covers — a second method over the same region, a zoomed re-count, a count
  from the other side of the board. They are **excluded from the sum** and
  exist so a re-count is recorded rather than discarded.

A dry run that wrote the per-group numbers into prose ("5 parts in a row") and
mixed re-counts into the same list produced the right total and an unverifiable
document: nothing could reconcile the groups to the class count. Put the number
in `count`, and mark every re-count `cross_check`.

## `silkscreen_markings[]`

| Field | Meaning |
|---|---|
| `text` | The string **verbatim**, including spacing and revision. Never repaired, never completed into the word you expect. |
| `location_px` | `[x, y]` of the string in the named view's space. |
| `basis` | `observed` — this array is for text read off the board. A number reached by arithmetic belongs in `topology_claims`. |
| `confidence` | 0..1; low when the string was partly legible. |
| `evidence` | Evidence pointers to the view and rectangle it was read from. |

## `topology_claims[]`

| Field | Meaning |
|---|---|
| `claim_id` | A short stable id, e.g. `string-length`. Other sections reference it (`design_brief_seed.depends_on_open_questions`). |
| `question` | The open question in plain words. |
| `basis` | `inferred` for a claim the copper implies; `observed` for one the photos settle outright. |
| `evidence` | Pointers to what the photos **do** establish — often less than the question asks. |
| `hypotheses` | Array — see below. **Two or more entries whenever the evidence does not resolve to one answer.** |
| `resolution_path` | How the question could be closed with more evidence: a trace to follow, a measurement to take, a continuity test. `null` when nothing would close it. |

### `topology_claims[].hypotheses[]`

| Field | Meaning |
|---|---|
| `label` | `A`, `B`, … — how the rest of the document refers to this candidate. |
| `description` | The candidate answer in one sentence. |
| `confidence` | 0..1, this hypothesis's own. Confidences across a claim are not averaged and need not sum to 1. |
| `calculation` | The arithmetic written out with units, not just its result — e.g. `6 x Vf 2.1 V = 12.6 V; 24 V - 12.6 V = 11.4 V across R at 20 mA -> ~570 ohm -> E24 620 ohm; P = I^2 R = 0.25 W`. **Required whenever the hypothesis is numeric**; `null` only when it is not. |
| `assumptions` | Array of strings: every value **you** supplied that the board did not — a forward voltage, a target current, a regulated supply, a tolerance. |

A hypothesis is never promoted to an observation because it was the only one
you could calculate, and a competing hypothesis is never deleted because its
confidence was lower.

## `retrace_correlation[]`

| Field | Meaning |
|---|---|
| `component_id` | The `scan_pcb_photo` component id, copied verbatim. |
| `bbox_px` | `[x, y, w, h]`, the scan's own box, copied verbatim. |
| `visual_class` | The survey class that box lands on. |
| `overlap_confidence` | 0..1 — how much of that box you believe covers that part. |

A correlation makes the scan searchable from the dossier. It is **not
evidence**: a scan box never creates a survey entry, never changes a `count`,
and a `pattern_matches` hint never establishes a topology claim.

## `photo_views_used[]`

Bare file names of the views the survey was read from. Nothing verifies they
still exist on disk, so a renamed view silently orphans every pointer that
names it — rename views before you write claims, not after.

## `design_brief_seed`

| Field | Meaning |
|---|---|
| `summary` | The implied design direction in a sentence or two: topology idea, rough BOM shape, physical spec. |
| `depends_on_open_questions` | Array of `claim_id` values this sketch rests on. |

A sketch, deliberately distinct from the full `design_brief` object that
`kicad-design-reconstruction` later writes onto the same map. The seed is
replaced, never edited into the brief.

## `open_questions[]`

Strings — the list the human is being asked to close. The missing scale, the
unreadable markings, every value a hypothesis had to assume. A gap belongs
here **even when a hypothesis already guesses at it**: the hypothesis is the
guess, the open question is the admission.

---

## The approval hash

`dossier` joins the map's content hash **the moment it is present**, and
contributes nothing when absent — which is what makes a map saved before this
section existed hash exactly as it always did.

The consequences:

- Adding a dossier to an approved map **revokes that approval**. Any edit to
  any field above does the same. That is the second review checkpoint working
  through the same mechanism as the first.
- A map that never gains a dossier is unaffected in every respect.
- A consumer reads `approval_valid` from `load_photo_review_map`, never the
  map's own `approved` field, which stays `true` after a post-approval edit.
