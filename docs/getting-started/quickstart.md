# Quickstart

=== "Python"

    ## Installation

    ```bash
    uv pip install quillmark
    ```

    ## Basic Usage

    ```python
    from quillmark import Quill, Quillmark, OutputFormat

    engine = Quillmark()                       # backend registry + render dispatcher
    quill = Quill.from_path("path/to/quill")   # portable, declarative config data

    markdown = """~~~
    $quill: my_quill
    $kind: main
    title: Example Document
    ~~~

    # Hello World
    """

    # The bound door: parse and conform against the quill that will render it.
    doc = quill.parse(markdown)
    result = engine.render(quill, doc, OutputFormat.PDF)

    with open("output.pdf", "wb") as f:
        f.write(result.artifacts[0].bytes)
    ```

=== "JavaScript"

    ## Installation

    ```bash
    npm install @quillmark/wasm
    ```

    ## Basic Usage

    ```javascript
    // The single root import is the canonical API: the Engine render
    // dispatcher, plus `init`, which resolves to the internal Typst-less core
    // build's own Quill/Document classes and is the only way to obtain them. An
    // editor that only validates uses Quill/Document and loads no backend:
    // Typst loads lazily on the first render.
    import { init, Engine } from "@quillmark/wasm";

    const { Quill } = await init();

    const enc = new TextEncoder();

    // A Quill is portable, declarative data: no engine needed to load it.
    const quill = Quill.fromTree(new Map([
      ["Quill.yaml", enc.encode("quill:\n  name: my_quill\n  backend: typst\n  version: 1.0.0\n  description: Demo\n\nmain:\n  fields:\n    title: { type: string }\n\ntypst:\n  plate_file: plate.typ\n")],
      ["plate.typ", enc.encode("#import \"@local/quillmark-helper:0.1.0\": data\n#data.at(\"$body\")\n")],
    ]));

    const markdown = `~~~
    $quill: my_quill
    $kind: main
    title: Example Document
    ~~~

    # Hello World`;

    // The bound door: parse and conform against the quill that will render it.
    const doc = quill.parse(markdown);

    // Rendering goes through the Engine. Its methods are async: the first call
    // lazily loads the Typst backend binary; the canonical quill crosses into
    // backend memory internally (no manual fromTree/fromStored needed).
    const engine = new Engine();
    const result = await engine.render(quill, doc, { format: "pdf" });
    const pdfBytes = result.artifacts[0].bytes;
    ```

    `init` is idempotent and concurrency-safe, so destructure it at each entry point (route loader, hydration path, worker) rather than threading one result around: every await after the first resolves from the same memo.

    An annotation needs no await: `Quill` and `Document` are also exported as types (`import type { Quill } from "@quillmark/wasm"`).

    ## Live Preview (Canvas)

    For editor-style previews, paint pages directly into a `<canvas>` instead
    of round-tripping through PNG/SVG. `paint` is WASM-only, both the Typst
    and `acroform` backends support it, and it shares the cached compile with
    the byte-output `render` path.

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

    session.free();                                    // when doc changes
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

    Full design rationale: [PREVIEW.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PREVIEW.md).
