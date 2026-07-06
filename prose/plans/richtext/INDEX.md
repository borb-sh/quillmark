# Richtext rework — integration HQ

Working plan for the content-model rework tracked in
[#831](https://github.com/quillmark-org/quillmark/issues/831). This branch
(`integration/richtext`) is the long-range integration point; phases land here
behind their spike gates, not on `main` piecemeal.

## Objective

Replace markdown-string content fields with a canonical corpus value —
`RichText`: one text sequence per field carrying line attributes, anchored
marks, and embedded islands — and demote markdown to a projection (import /
export codecs). Delivers #829's paragraph-level regions as the step-2
degenerate case. Product frame: a web form with rich prose fields is the
primary authoring surface; the LLM/MCP whole-document markdown flow and
human-authored markdown documents stay co-equal writers; a Notion-class block canvas is a non-goal.

The full model spec — `RichText` shape, lines / marks / islands, codecs,
storage, schema — lives in the body of
[#831](https://github.com/quillmark-org/quillmark/issues/831). This HQ is the
canonical direction: it records what is **decided**, the naming, and how the
work is **sequenced**; it does not restate the model. The earlier working docs
on `claude/issue-830-context-uuia5t` (proposal + #830 reviews) are superseded —
their live conclusions are distilled here.

## Locked decisions

### Model — corpus, not block tree

Settled in #831. One `RichText` per richtext field over a single coordinate
space (Unicode scalar values): `text` + `lines` + `marks` + `islands`, every
edit a splice. Supersedes the #830 block tree; the Peritext/Automerge research
#830 cited points at the corpus shape, not the tree.

### Seam — Option A: structured RichText-JSON across the data seam

The core→backend seam (`Backend::open(source, json_data)`, `core/src/backend.rs`)
stays a **data** contract; content crosses it as **structured RichText-JSON**,
not a markdown string.

Grounding for the call:

- **Codegen is not a universal seam.** `typst` is a source backend (lowers JSON
  → Typst dict literal + `#let` content blocks); `pdfform` is a data backend
  (`pdfform/src/lib.rs`, `resolve_field_specs` stamps `json_data` values into an
  AcroForm — no codegen, none possible). A codegen seam would force `pdfform` to
  un-lower source back to values.
- **Three tiers, named.** The seam is the *data model*; codegen is `typst`'s
  private lowering of it (and the only place the source map can be produced);
  JSON is the *serialization* of the model. Content-as-markdown-string was a
  lossy encoding *inside* the data seam, not a codegen seam.
- **Why A over a typed-`Document` seam (Option C).** Keep JSON as the
  language-agnostic cross-backend contract: bindings already serialize it,
  `PLATE_DATA.md` publishes it, and both backends lower from it uniformly
  (`typst` → source + map; `pdfform` → plaintext via `RichText.text` minus
  island slots). C (promote the seam to the typed model, demote JSON to storage
  serialization) is cleaner conceptually but rewrites the `Backend` trait and
  `pdfform`'s field resolution. Because both A and C carry richtext
  *faithfully*, A↔C is a later backend refactor that changes no session API —
  so A is the low-regret starting point.
- **`pdfform` cost is zero today.** No pdfform fixture binds a content field
  (`sample_form` sets `body.enabled: false`; all fields are scalar). Option A
  ships with a trivial `RichText → .text` lowering and no fixture churn.

The durable contract is **"richtext crosses the seam faithfully"** (never as a
lossy markdown string). The refactorable detail is the encoding (JSON vs typed).

**Confirmed by [phase 0](phase-0.md) (Spike C):** canonical
RichText-JSON serializes byte-deterministically, the seam JSON and the storage
serialization are the *same* canonical bytes (one encoding, not two to keep
aligned), and both backends lower trivially — `typst` via the shipped
`mark_to_typst`, `pdfform` via `RichText.text` minus island slots, with zero
`sample_form` fixture churn.

### Naming — `richtext`

The type is `richtext` at every author-facing surface (`type: richtext`,
`array<richtext>`, `richtext(inline)`, blueprint `# richtext<markdown>`) and in
code (`RichText`); **corpus** stays internal shape vocabulary. The blueprint
format slot carries the surface encoding (`# richtext<markdown>`) — the type
names the role, the refinement names the syntax. Follows the `datetime`
one-word precedent. Rejected, each with the criterion it failed: `markdown`
(names one projection's encoding, not the role), `content` (saturated;
permanent `typst::foundations::Content` collision), `prose` (undersells
lists/tables/islands), `corpus` (implementation shape; outsider prior is a
*collection* of documents), bare `rich` (still live as an ordinary adjective).

## Technical takeaways carried into the phases

Risk register. Each item states the risk in one line; a resolved item collapses
to its status and points at the phase doc that owns the detail. The [phase
0](phase-0.md) items reported with no red flag; the source of truth for their
results is `phase-0.md` (and the assertions in `crates/richtext-spikes/tests/`),
not this list.

- **Freeze ordering.** Phase-1 serialization fixes the mark set + overlap / edge
  / identity semantics, exactly where ProseMirror / Lexical / Quill disagree —
  so the editor spike must *precede* the freeze (hence phase 0). The open mark
  set absorbs new mark *types*, not changed semantics of known ones.
  → **Resolved (phase 0·A):** editor-independent by construction — the model
  encodes only the mark set + three normalization rules and delegates
  edge-expand / adjacent-merge to the editor. **Frozen in phase 1** and verified
  byte-deterministic across producers (import vs a hand-built editor corpus emit
  identical bytes) and feature configs (whole-tree key sort); a live binding is
  the residual gate ([phase-1.md](phase-1.md), [phase-0.md](phase-0.md#residual-gate)).
- **Seam encoding, not map production.** `locate` / `position_at` / regions key
  on `(field, corpus range)` however `typst` builds the map — but the map is a
  codegen artifact, so content must cross the seam *faithfully* (not as a
  re-parsed string) from phase 2 on. Map production itself is deferrable.
  → Encoding confirmed under **Seam — Option A** above (phase 0·C); the map and
  the `(field, corpus range)` key **landed in phase 2** (#829). A `revision`
  third key component **defers to Phase 3** with the change log — it earns its
  keep only when a stale position is *mapped* forward, not merely detected
  (`RenderedRegion.span` is additive-optional so it appends without a break).
- **Navigation is cluster-exact, not character-exact.** Point↔corpus resolves at
  the shaping cluster, inverting the source map through `escape_markup` and the
  glyph's intra-node offset (`glyph.span.1`). Origin-less ink (list markers,
  numbering, decorations) is nav-ignored. → **Resolved (phase 0·B):**
  cluster-exact, invertible by recomputation; `escape_markup` char-local except
  the `//`→`\/\/` coupling. **Built in phase 2** (#829): the two-tier segment
  run machine (per-`(field, segment)` regions), `position_at`/`locate`, and
  `RenderedRegion.span` landed. One correction to phase-0's Spike B: a
  multi-line `#raw` block's lines share one node wider than any per-line run, so
  `position_at` degrades to the code segment's start, not a per-line offset —
  see [phase-2 § PR-F](phase-2.md) and `pr-f-spike-findings.md`.
- **Determinism has an honest boundary.** Legacy bodies **can** hold tables and
  images — the prior `mark_to_typst` rendered both — which import as islands
  with **sequential** ids (`isl-N`); import is a pure function, so the migration
  stays deterministic (real per-creation minting is Phase 4). Text stays
  deterministic. → **Resolved (phase 0·C, corrected in phase 2):** sequential
  import ids are the migration's only island source; the `preserve_order`
  `props`-key-order leak is **closed in phase 1** (`model::sorted_value`
  recursively sorts before serialization). Mint nondeterminism does not appear
  until phase 4 (per-creation island ids).
- **The move-annotation weak spot lands on the flagship writer.** The stale-text
  writer (MCP `update_document`, a saved document) rebases via cold-parse + corpus
  diff; a reorder is delete+insert, so annotations on moved text drop unless a
  move detector confines it. → **Implemented in phase 1** (`delta::diff_import`):
  a verbatim block move re-homes the anchor, restricted to *inserted* text
  (overlap + length floor) so it cannot capture unrelated surviving text; residual
  drop (moved *and* rewritten) accepted and stated. See [phase-1.md](phase-1.md).
- **USV coordinates are a standing cross-binding tax.** JS editors are UTF-16,
  Rust UTF-8; every delta crossing WASM/Python converts at its boundary, and the
  property suite owns surrogate-pair / astral-plane correctness. → Validated in
  phase 0 (mid-surrogate rounds down to its owning char).

## Phase map

Detail per phase in its own doc as it opens. Rough shape:

- **[Phase 0 — spikes](phase-0.md)** — de-risk the freezes before any
  schema lands. Editor binding (gates mark semantics), source-map/navigation
  inversion (gates the phase-2 emit design), seam + determinism prototype (gates
  Option A). Nothing here ships to `main`. **Reported — no red flag** (see
  [phase-0.md](phase-0.md); runnable probe `crates/richtext-spikes/` on branch
  `claude/issue-831-phase-0-tbjjm1`, not on `main`/`integration`). Phase 1 opened
  on the Spike-A contract, with a live-editor binding tracked as its residual
  gate.
- **[Phase 1 — type + codecs, engine-off](phase-1.md)** — **landed.** `RichText`
  + canonical serialization (frozen on the [Spike-A contract](phase-0.md): mark
  set + three normalization rules) + markdown⇄corpus import/export codecs +
  property suite. New crate `crates/richtext`, engine untouched. Built from the
  written contract, then hardened against an independent review cross-checked
  with the Phase-0 spike code — the freeze is byte-deterministic across producers
  and feature configs. Decisions, review outcome, and the Phase-2 handover in
  [phase-1.md](phase-1.md).
- **[Phase 2 — engine consumes RichText](phase-2.md) (delivers #829) — in progress.**
  **PR-A**, **PR-B**, **PR-C** (storage cutover to `quillmark/document@0.93.0`),
  and **PR-D** (typst emitter + segment maps) are **landed** (PR #836 →
  `integration/richtext`), plus a landed **Option A** structured-table-cell step
  (parity 116/116, table cells no longer a raw markdown slice). **PR-E** (seam
  flip) is handed off — self-contained, decisions locked — in
  [phase-2.md § PR-E handover](phase-2.md#pr-e-handover--seam-flip-option-a-live).
  The one PR-B deviation still owed by a later PR (lossy example import,
  deferred to PR-G) is in
  [phase-2.md § PR-B landing log](phase-2.md#pr-b-landing-log--pr-c-handover).
  The markdown parse moves to ingest: `crates/richtext` stays a **separate leaf
  crate** (`quillmark-richtext`) that `core` now **depends on** (arrow inverted
  from phase 1), so `import` runs once at ingest, not per render. The seam carries
  canonical RichText-JSON (Option A, [confirmed](phase-0.md)), `typst` lowers the
  corpus to markup with a per-segment source map, `pdfform` lowers via `.text`,
  storage cuts over (new `StoredDocument` version, fallible cold-import migration),
  and regions re-key on `(field, corpus range)` — `revision` **defers to Phase 3**
  with the change-log. Delivers #829's paragraph regions as the segment-map
  degenerate case. Supersedes phase-1 handover items 1 (re-home) and 4
  (`MarkdownFixer` unify). Decisions, sub-PRs (A–G), sequencing, risks, and the
  canon rework are in [phase-2.md](phase-2.md); the design inverts the
  "markdown-engine-free core" invariant into "one parse site, in
  `quillmark-richtext::import`". The arrow-inversion groundwork (relocate
  `normalize_markdown`, `core` → `quillmark-richtext`, `publish`) is **landed**.
- **Phase 3 — edit surface.** Per-field delta — `retain`/`insert`/`delete` text
  splices (CodeMirror `ChangeSet`, **not** attributed Quill-Delta; marks/lines are
  separate op channels) — plus monotonic revision + bounded change log with
  position mapping; form-editor binding built on the phase-0 spike's frozen
  semantics. Opens the **residual Spike-A gate**: bind one real rich editor and
  confirm no editor forces an edge-expand / adjacent-merge semantic back into the
  model (if one does, it enters as editor config, not a serialization change).
- **Phase 4 — islands + collab.** First real island type (tables), then a
  text-CRDT sync binding when wanted; core stays CRDT-free.

Sequencing invariant: nothing a later phase needs is frozen before the phase-0
spike that validates it, and no phase discards another's output.

## Related

- #831 (this rework), #830 (block-tree predecessor, superseded), #829 (regions,
  delivered by phase 2)
- `prose/canon/DOCUMENT_STORAGE.md`, `QUILL_VALUE.md`, `PREVIEW.md`,
  `CONVERT.md`, `PLATE_DATA.md`
- `crates/core/src/backend.rs`, `crates/core/src/region.rs`,
  `crates/backends/typst/src/overlay/span_scan.rs`,
  `crates/backends/typst/src/helper.rs`
