# Live Preview

Two verbs drive rendering. `render(quill, doc, opts)` is the stateless one-shot — bytes for CLI, server, or export. `open(quill, doc)` returns a **`LiveSession`**: a persistent, incremental compiler that owns editor preview. It paints pages into a `<canvas>`, reports what an edit changed, and answers geometry queries for a cursor bridge. Canvas preview is **JavaScript/WASM-only**.

For the minimal paint loop, see the [Quickstart canvas example](../getting-started/quickstart.md#live-preview-canvas). This page covers the editor surface around it.

## The session

```ts
const session = await engine.open(quill, doc);   // compiles once, keeps its world alive
session.pageCount;                                // pages in the current compile
session.warnings;                                 // this compile's non-fatal diagnostics
session.paint(ctx, page, { layoutScale, densityScale });
session.free();                                   // eager teardown
```

`paint` writes a **complete** page raster straight into the canvas backing store — every glyph already visible, no compositing. Give each visible page its own `<canvas>`; the painter owns `canvas.width` / `height` (and the 16384-px backing-store clamp), while the consumer owns `canvas.style.*`, sized from the returned `layoutWidth` / `layoutHeight`. Fold `devicePixelRatio` and in-app zoom into `densityScale`.

## Edits — `apply` and `ChangeSet`

`session.apply(doc)` recompiles in place against new document data and returns a `ChangeSet { pageCount, dirtyPages }`. It is **transactional**: on a compile error the previous compile stays live — reads keep serving the last-good document and its warnings — and the session recovers on the next successful apply. A preview repaints only `dirtyPages ∩ visible`:

```js
function onEdit(editedDoc) {
  const { dirtyPages } = session.apply(editedDoc);
  for (const p of dirtyPages) if (isVisible(p)) repaint(p);
}
```

## Geometry — the cursor bridge

Field geometry is a render-free session query, re-read after each committed `apply`:

- `regions()` → *field → rectangles*, one box per content **segment** (paragraph, heading, whole code fence), keyed on the schema field path.
- `fieldBoxes(field)` → the derived whole-field highlight — one union rect per page.
- `fieldAt(page, x, y)` → *point → field* (click → focus the editor), hit-testing the compiled document so every placement resolves.
- `positionAt(page, x, y)` → *point → content position*: the field plus a cluster-exact USV offset into its content, for placing a caret or mapping a selection.
- `locate(field, pos)` → *content position → caret rect*, the exact inverse of `positionAt`.

Coordinates are PDF points with a **bottom-left origin**. A canvas is top-left in device pixels, so an overlay flips the Y axis: for a page `pageHeightPt` tall painted at `renderScale = layoutScale × densityScale`, a region's top-left canvas corner is `(rect[0] × renderScale, (pageHeightPt − rect[3]) × renderScale)`.

A one-shot `render` can also carry the geometry sidecar on request (`RenderOptions.regions`) for consumers with no live session — static overlays over an exported SVG, PDF post-processing.

## Non-goals

Text selection, find-in-page, and accessibility are not on the canvas by design — keep an SVG/PDF export path if you need them. The painter maps no clicks itself: convert a canvas click to page coordinates (the inverse of the overlay transform) and ask `fieldAt`.

Full design and the DPR/clamp math: [PREVIEW.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PREVIEW.md).
