# Live Preview

For editor-style previews, paint pages directly into a `<canvas>` instead of
round-tripping through PNG/SVG. `paint` is WASM-only, both the Typst and
`acroform` backends support it, and it shares the cached compile with the
byte-output `render` path. The code below continues from the `quill`, `doc` and
`engine` of the JavaScript [Quickstart](../getting-started/quickstart.md).

```javascript
const session = await engine.open(quill, doc);     // compile once (async)

// Surface session-level diagnostics from compile time.
for (const w of session.warnings) console.warn(w.message);

function renderPage(canvas, page, userZoom = 1) {
  canvas.style.width = "100%";                     // the box sets display size
  if (canvas.clientWidth === 0) return;            // no layout box yet
  const cssPxPerPt = canvas.clientWidth / session.pageSize(page).widthPt;
  const scale = cssPxPerPt * (window.devicePixelRatio || 1) * userZoom;
  session.paint(canvas.getContext("2d"), page, scale);
}

for (let p = 0; p < session.pageCount; p++) renderPage(canvases[p], p);

session.free();                                    // when the editor closes
```

Key contract points:

- `scale` is backing-store pixels per point: the CSS px per point the
  page is shown at, times `devicePixelRatio` and any in-app zoom.
- A canvas with no layout box (`display: none`, detached) has
  `clientWidth` 0, and `paint` throws `backend::invalid_raster_scale` on
  a 0 scale: skip it, and paint once a `ResizeObserver` reports a width.
- The painter owns `canvas.width` / `canvas.height` and rewrites them on
  every call (so each `paint` is a full repaint: no `clearRect` needed),
  reducing `scale` where it must so neither exceeds 16384 px. The consumer
  owns `canvas.style.*`.
- `pageCount` and `pageSize(page)` reflect the session's current compile:
  stable between edits, but re-read them after a committed `update(doc)`,
  which recompiles in place and can change the page count
  (`ChangeSet.pageCount`).
- In an edit loop, repaint `dirtyPages ∩ visible` rather than every page:

  ```javascript
  function onEdit(editedDoc) {
    const { pageCount, dirtyPages } = session.update(editedDoc);
    for (const p of dirtyPages) if (isVisible(p)) renderPage(canvases[p], p);
  }
  ```

  `update` is transactional: if it throws, the canvas still shows the
  last-good compile, so keep it up and surface the diagnostics.

The rules behind this design: [Quillmark invariants § Preview](https://github.com/borb-sh/quillmark/blob/main/prose/INVARIANTS.md#preview).
