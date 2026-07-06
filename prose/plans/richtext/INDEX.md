# Richtext rework — integration HQ

Working plan for the content-model rework tracked in
[#831](https://github.com/quillmark-org/quillmark/issues/831). This branch
(`integration/richtext`) is the long-range integration point; phases land here
behind their spike gates, not on `main` piecemeal.

## Status

Phases 0–2 are landed. The only remaining work is **PR-G**: `richtext(inline)`
enforcement, load-time schema-example import + cache, seed-commits-corpus, and
retiring the `markdown` type alias as a hard load-time error (today it is a
silent alias). PR-G's spec lives in [phase-2.md](phase-2.md) — this doc does
not duplicate it.

For how the system works *today* — the `RichText` corpus, the seam, storage,
schema surface, navigation — see `prose/canon/` (ARCHITECTURE, CONVERT,
PLATE_DATA, SCHEMAS, PREVIEW, DOCUMENT_STORAGE). This HQ records only
direction and sequencing, not implemented behavior.

## Objective

Replace markdown-string content fields with a canonical corpus value —
`RichText`: one text sequence per field carrying line attributes, anchored
marks, and embedded islands — and demote markdown to a projection (import /
export codecs). A web form with rich prose fields is the primary authoring
surface; the LLM/MCP whole-document markdown flow and human-authored markdown
documents stay co-equal writers; a Notion-class block canvas is a non-goal.

The full model spec — `RichText` shape, lines / marks / islands, codecs,
storage, schema — lives in the body of
[#831](https://github.com/quillmark-org/quillmark/issues/831). This HQ is the
canonical direction: what is decided, and how the work is sequenced; it does
not restate the model.

## Decided

- **Model is a corpus, not a block tree** (settled #831): one `RichText` per
  richtext field over a USV coordinate space — `text` + `lines` + `marks` +
  `islands`, every edit a splice. Superseded the #830 block tree.
- **Seam is structured RichText-JSON ("Option A")**, never a markdown string,
  across `Backend::open(source, json_data)`. Landed in phase 2 (PR-E). A typed
  `Document` seam ("Option C") stays available as a later, non-urgent backend
  refactor — not ruled out, just not needed yet.
- **Type name is `richtext`** at every author-facing surface and in code
  (`RichText`); `markdown` is today a deprecated alias, becoming a load error
  in PR-G. Current surface: SCHEMAS.md.

## Phase map

- **[Phase 0 — spikes](phase-0.md).** Landed, no red flag. De-risked mark
  semantics, source-map inversion, and seam/determinism before phase 1 froze
  anything.
- **[Phase 1 — type + codecs, engine-off](phase-1.md).** Landed. `RichText` +
  canonical serialization + markdown⇄corpus codecs, in `crates/richtext`,
  engine untouched.
- **[Phase 2 — engine consumes RichText](phase-2.md) (delivered
  [#829](https://github.com/quillmark-org/quillmark/issues/829)).** Landed
  through PR-F: seam flip to corpus JSON, typst emitter + segment maps,
  storage cutover, regions + navigation (`locate`/`position_at`). **PR-G is
  open** — see phase-2.md.
- **Phase 3 — edit surface.** Per-field delta (`retain`/`insert`/`delete` text
  splices, CodeMirror `ChangeSet` semantics, not attributed Quill-Delta) +
  monotonic revision + bounded change log; form-editor binding on phase-0's
  frozen mark semantics. Opens the residual Spike-A gate: bind one real rich
  editor and confirm none forces an edge-expand / adjacent-merge semantic back
  into the model (if one does, it enters as editor config, not a
  serialization change).
- **Phase 4 — islands + collab.** First real island type (tables, with
  per-creation id minting rather than import's sequential ids), then a
  text-CRDT sync binding if wanted; core stays CRDT-free.

Sequencing invariant: nothing a later phase needs is frozen before the
phase-0 spike that validates it, and no phase discards another's output.

## Related

- #831 (this rework), #830 (block-tree predecessor, superseded), #829
  (regions, delivered by phase 2)
- `prose/canon/DOCUMENT_STORAGE.md`, `QUILL_VALUE.md`, `PREVIEW.md`,
  `CONVERT.md`, `PLATE_DATA.md`, `SCHEMAS.md`
