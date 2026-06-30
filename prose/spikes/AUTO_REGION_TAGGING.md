# Spike: auto-region tagging for content fields (#773)

Status: spike, not for merge. Branch `claude/issue-773-comment-review-c2utbs`.

## Question

#773's comment showed that a Typst region's `field` is the plate-authored
`form-field` name, not the schema path — so `usaf_memo`'s `signature-field("Signature")`
emits `field: "Signature"`, which a consumer can't route to the schema field
`signature_block`. The proposed end-state (A1): content/markdown fields carry
their schema path automatically, with zero plate-author effort, and the page
geometry of *arbitrary* content (not just fixed-size widgets) is recovered after
compilation. This spike asks whether A1 is buildable on Typst 0.15 and what it
costs.

## Result: yes, end to end

Two changes, both small:

1. **Auto-tag at the markdown-eval site.** `lib.typ.template`'s `eval-content`
   already enumerates exactly the content fields (from injected
   `meta.content_fields` / `meta.card_content_fields`) and `eval`s each into
   content. Wrapping that content between two zero-size metadata markers
   (`<__qm_region__>`, `role: start|end`, carrying the schema `field` path) is a
   ~3-line change. The path is the key the loop already holds — `intro`,
   `$cards.<i>.<key>` — so no field-discovery machinery is added. Plate authors
   write nothing.

2. **Recover boxes from the frame tree.** `overlay/region_scan.rs` walks the
   compiled `PagedDocument` frames, composing the group-transform stack exactly
   as `typst_layout::introspect::discover_frame` does. A `start` tag opens a
   field; every drawn leaf (`TextItem::bbox()` / `Shape::bbox()` / image size)
   seen while open unions into that field's box, partitioned by page; an `end`
   tag closes it. Boxes flip to the PDF bottom-left origin the region model uses.

`session.regions()` appends these to the existing form-field regions.

### Measured output (spike test, `spike_content_regions.rs`)

A short `intro` and a `body` long enough to overflow four pages:

```
field=body   page=0 rect=[72.1,  80.2, 524.7, 699.3]  (452w x 619h)
field=intro  page=0 rect=[72.1, 710.0, 272.4, 720.2]  (200w x  10h)
field=body   page=1 rect=[72.1,  71.4, 524.7, 720.2]  (452w x 648h)
field=body   page=2 rect=[72.1,  71.4, 524.7, 720.2]  (452w x 648h)
field=body   page=3 rect=[72.1, 665.5, 524.7, 720.2]  (452w x  54h)
```

`intro` is one line at the top; `body` is one region per page it occupies — full
column on the interior pages, a tail box on the last. Geometry is correct and
keyed on the schema path.

## What the spike proved

- **A1 is real for content fields.** A markdown field auto-tags with no author
  effort. This is the half of A1 that the Typst value model permits: a field
  placed *as content* keeps its tag. A computed scalar (`data.from + ", " + rank`)
  cannot — `str` has no label and there's no operator overloading — so those
  stay untagged. Per the #773 decision, explicit author-facing regions are
  deferred, so untagged is the accepted outcome for scalars for now.

- **Frame introspection beats `measure()`.** It needs no declared size, returns
  true rendered extent, and handles fragmentation natively.

- **Multi-rect per field is required, not optional.** A page-spanning body is N
  boxes, not one. This matches Typst's own PDF link annotations, which emit
  per-fragment rects precisely because a single bbox over a line-broken span
  "would span the entire paragraph, which is undesirable" (`typst_pdf::link`).
  Consequence for the model: `RenderedRegion` must allow a repeated `field` (one
  entry per page-fragment); consumers group by `field`. Form-field widgets stay
  single-entry (a widget *is* one box), so both kinds coexist in one `Vec`.

- **It's non-breaking.** The full workspace (`cargo test --workspace`) stays
  green with every content field wrapped — golden snapshots, the usaf_memo
  signature test, markdown-field tests included. Zero-size metadata is
  layout-neutral, the same property the form-field path already relies on.

### Bug found (and fixed in the spike)

The `active` set must persist across pages. A first cut reset it per page, which
dropped every continuation fragment of a page-spanning field — the walk has to
carry open-field state from the `start` marker (one page) to the `end` marker
(possibly a later page).

## Open questions for the real implementation

1. **Card path form.** The spike emits positional `$cards.<i>.<field>`. The
   0.92→0.93 migration guide writes `$cards.<kind>.<n>.<field>` (kind-grouped).
   The canonical address must be settled *and* must match what the editor uses
   to key fields, or A1's whole premise (region→field is a lookup) fails again.

2. **The signature case from the comment.** `signature_block` is a content field
   and now auto-tags from content. But `usaf_memo`'s plate routes it through
   `signature-field("Signature")` (a widget), which still keys on the widget
   name. So the same logical field can produce *two* differently-keyed regions.
   Reconcile: either bind a schema field on the widget, or have the signature
   helper emit a content tag alongside the widget.

3. **Granularity.** One box per field per page. Per-line "quadpoint" rects
   (finer, what Typst links do for readers that fall back to the bbox) are
   deferred; field-level matches editor addressing and is enough for cross-nav.

4. **Repeated placement.** The start/end markers key by field in a set, so a
   field placed twice toggles ambiguously. Supporting multi-placement needs a
   unique id per occurrence rather than the field string.

## Recommended way forward

Ship A1 for content fields (changes 1 + 2 above), make `RenderedRegion`
multi-rect (repeated `field`), and settle the card-path address as a blocking
prerequisite. Keep form-field widget regions as they are; reconcile the
signature double-key (open question 2) so #773's original case resolves cleanly.

## Spike artifacts (this branch)

- `crates/backends/typst/src/overlay/region_scan.rs` — the frame-walk scanner.
- `crates/backends/typst/src/lib.typ.template` — `qm-region` helper +
  `eval-content` auto-wrap.
- `crates/backends/typst/src/lib.rs` — `regions()` appends content regions.
- `crates/backends/typst/tests/spike_content_regions.rs` — end-to-end probe.
