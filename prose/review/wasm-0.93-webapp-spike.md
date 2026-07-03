# Spike: web-app on the unreleased `@quillmark/wasm` 0.93

> Branch pair: `claude/quillmark-wasm-migration-pscq14` here and in
> `tonguetoquill/web-app`. The web-app branch consumes this tree's
> `pkg/` as a `file:` dependency and rebuilds its preview on the
> `Engine.open` → `LiveSession.apply`/`paint`/`regions` surface.

## What the spike did

- Built the three-artifact `pkg/` from this working tree
  (`scripts/build-wasm.sh --ci`) and pointed web-app's
  `@quillmark/wasm` at it.
- Replaced web-app's session-per-render preview with a
  `QuillmarkPreviewController`: one `LiveSession` per (document ×
  quill), `apply(doc)` per edit, repaint of `dirtyPages` only; a
  quill swap re-opens.
- Wired editor ↔ preview cross-navigation over `regions()`: hotspot
  overlays per region (the percent transform in
  `prose/canon/PREVIEW.md` works verbatim), click → expand the owning
  form group and focus the field, editor focus → highlight the region.
  Editor-side addresses use the same `$cards.<kind>.<n>.<field>`
  grammar regions carry.
- Verified against the production quill catalog: a real-wasm vitest
  suite (open / apply / ChangeSet / transactional failure / regions)
  plus a browser drive of the live app (incremental apply repaints,
  both navigation directions).
- Filed findings as #782; rebased onto the resolutions
  (#783/#784/#785/#788) once landed and re-verified against the
  production catalog a second time (this pass).

The surface holds together: apply is transactional as documented,
dirty-page math is correct (edit → `[0]`, no-op edit → `[]`), a
failed apply keeps every read serving the last-good compile, and the
canvas paint/DPR contract needed no changes on the consumer side.

## Resolved (#782)

### `Document` lifetime across `engine.open`/`render` was a footgun — fixed (#785)

`runtime.js` now snapshots `doc.toJson()` (and `quill.toTree()` on a
clone-cache miss) synchronously, before the first await. The natural
`try { return engine.open(quill, doc); } finally { doc.free(); }`
shape is safe again; web-app's integration test comment claiming
otherwise is corrected.

### `apply` had no warnings channel — fixed (#784/#790)

`LiveSession.warnings` now reflects the *current* compile (Typst
compile warnings — font fallback, overfull pages — threaded through
same as errors), refreshed on every committed `apply` and held at
last-good on a failed one. `validation::must_fill` deliberately does
**not** ride this channel — it stays a `quill.validate(doc)` concern,
per the #782 rescope. Web-app's `QuillmarkPreviewController` now
re-reads `session.warnings` in `#readCompileState()` (previously only
read at open), so an edit that introduces or clears a compile warning
updates the preview's warnings overlay without a re-open.

### From-source builds claimed the previous version — fixed (#785)

`build-wasm.sh` stamps `<next-patch>-dev.<short-sha>` on dev builds
(verified: this rebuild produced `0.92.2-dev.ee01902`, not `0.92.1`);
`release.yml` passes `--release-stamp` and asserts the stamp matches
the tag before publish. The `wasm-bindgen-cli` version check also now
runs before the cargo build, not after.

### Region coverage on the real catalog — the escape hatch landed and is in production use (#788)

The generated helper now exports `tagged(field)[body]`: it brackets
whatever a plate places at a site, validated against the schema
address table at compile time (a typo'd path is a compile error), and
survives a package's internal content-rebuild (the original gap —
`render-body`'s AFH-numbering rebuild dropped verbatim auto-tag
markers) because the wrap sits *outside* the package call. Each card
dict carries its `$path` prefix so plates compose card addresses
without hand-rolling the kind+ordinal grammar.

The fixture `usaf_memo` plate was tagged upstream (`$body`,
`signature_block`, indorsement card addresses via `$path`); this pass
ported the same three edits into web-app's `@airmark/quiver` copy of
`usaf_memo` (a plate that had already diverged from the fixture —
CUI fields, a different date-default — so the port is hand-applied,
not a file copy). Confirmed live: `session.regions()` on the shipped
USAF template now returns `$body`, `signature_block`, and `tag_line`
(previously `tag_line` alone), and a synthetic multi-page +
indorsement document confirms the full contract — one `$body`
fragment per page it spans, `$cards.indorsement.0.$body` and
`$cards.indorsement.0.signature_block` addressed by kind-scoped
ordinal, not loop index.

**Residual scope, not a regression:** `af4141` and `daf4392` disable
`main.body` entirely (form-fill quills, nothing to tag). `daf1206`
bypasses the standard content-eval pipeline — its plate stuffs field
values into a generated form template's parameter dict rather than
placing them as Typst content — so neither auto-tag nor `tagged()`
reaches it without a deeper plate rework; out of scope for this pass.

**Consumer breaking change, handled:** `field` is no longer unique —
`regions()` returns one entry per (placement, page fragment); a
page-spanning `$body` now yields one fragment per page. Web-app's
region-hotspot render loop keyed its `{#each}` block on `region.field`
alone, which breaks the moment two regions share a field on one page;
fixed to key on `field + index` and grouped consumers (the
integration test, the hotspot-per-page map) accordingly.

### `plate_file` — scope decision, not a fix (#782 discussion)

Rejected as an alias: pre-1.0, hard cutovers stand (same policy as
`!fill` → `!must_fill`), and an alias would only defer the load
failure by one release since documents pin quill refs. The catalog
republishes alongside the 0.93 release; web-app's branch carries a
hand-migrated local copy of `@airmark/quiver`'s `Quill.yaml`s in the
interim (unchanged from the original spike).

## New, minor: canonical `runtime.d.ts` drifted from the per-placement region contract

`runtime.d.ts`'s hand-written `FieldRegion` doc comment still reads
"click a rendered field → focus `field`… or highlight the rect for the
focused field" with no mention of grouping, and its `RenderOptions`
interface has no `regions?: boolean` — the generated Typst backend's
`RenderOptions` gained one (opt-in region population on one-shot
renders). `runtime.types.test-d.ts`'s mutual-assignability check
doesn't catch either: an *optional* field missing from one side is
still structurally assignable both directions, so the drift guard is
silent on it. Low severity — no consumer code is affected, since
web-app reads regions off `LiveSession` (which delegates to the
already-correct generated backend), never off the canonical
`RenderOptions` type. Worth a doc pass whenever `runtime.d.ts` is next
touched.

## Notes (no action asked, unchanged from the original spike)

- `apply` is synchronous on the main thread. Keystroke-cadence Typst
  compiles block the UI for their duration; web-app yields a macrotask
  first and lives with it. A worker/OffscreenCanvas story stays the
  consumer's problem — fine for now, worth a canon note.
- The editor re-derives the `$cards.<kind>.<n>.` prefix (kind-scoped
  ordinal) from its card list on every focus/reorder; the grammar is
  now implemented four times (Typst prefix builder, the new `tagged`
  helper's `$path` injection, pdfform resolver, web-app
  `field-path.ts`). A `document.fieldPath(cardIndex, field)` accessor
  would collapse the drift surface.
- The DAF form quills are Typst recreations, not `pdfform` quills, so
  the pdfform widget-region path has no production consumer yet.

## Web-app branch caveats

- `package.json` depends on `file:../quillmark/pkg`; CI cannot
  `npm ci` until `@quillmark/wasm@0.93.0` publishes. Swap the dep and
  delete this caveat at release.
- `static/quills` on that branch is repacked from a locally-migrated
  and locally-tagged `usaf_memo` (`plate_file` moved under `typst:`,
  `$body`/`signature_block`/indorsement addresses wrapped in
  `tagged()`). The published `@airmark/quiver` package still carries
  neither change and must be re-released alongside 0.93 — at that
  point this local patch should be dropped in favor of the real
  upstream content package.
- `live-session.integration.test.ts` now pins the *resolved* coverage
  (`$body`, `signature_block`, `tag_line` on the shipped template) plus
  the per-placement/group-by contract on a synthetic multi-page +
  indorsement document. It starts failing if a future catalog or
  wasm-side change silently drops tagging or reintroduces a
  unique-`field` assumption — which is the point.
