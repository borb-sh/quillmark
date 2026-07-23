# Integrating Quillmark

Quillmark embeds in an app through two published bindings — **Python** (`quillmark` on PyPI) and **JavaScript/WASM** (`@quillmark/wasm` on npm) — over one core engine. Rust consumers use the crates directly ([docs.rs](https://docs.rs/quillmark)).

Three objects carry every integration:

- **Engine** — a backend registry and render dispatcher. It holds no quills; every render routes through it, and it never constructs a quill. `Quillmark()` in Python, `new Engine()` in JavaScript.
- **Quill** — portable, declarative config data (schema, plate, assets). Loaded once, rendered by any engine whose backend matches; it never renders itself.
- **Document** — the typed model of one Quillmark Markdown file (root block, body, cards). Built from Markdown, from a blank canvas, or from stored JSON.

Field I/O is schema-bound: `quill.writer(doc)` writes each field by its declared type and `quill.reader(doc)` reads it back. `Document` itself is quill-free data — parse, persist, structure.

## API surface

| Task | Python | JavaScript |
|---|---|---|
| Engine | `Quillmark()` | `new Engine()` |
| Load a quill | `Quill.from_path(dir)` | `Quill.fromTree(map)` |
| Parse a document | `Document.from_markdown(md)` | `Document.fromMarkdown(md)` |
| Blank document | `Document(quill_ref)` | `new Document(quillRef)` |
| Typed write | `quill.writer(doc).set_all({…})` | `quill.writer(doc).setAll({…})` |
| Typed read | `quill.reader(doc).get(name)` | `quill.reader(doc).get(addr)` |
| Add a card | `quill.writer(doc).add_card(kind, {…})` | `quill.writer(doc).addCard(kind, {…})` |
| Render | `engine.render(quill, doc, OutputFormat.PDF)` | `await engine.render(quill, doc, {format:'pdf'})` |
| Live preview | — | `await engine.open(quill, doc)` → `LiveSession` |
| Persist | `doc.to_json()` / `Document.from_json(s)` | `doc.toJson()` / `Document.fromJson(s)` |
| Schema (for UIs) | `quill.schema` | `quill.schema` |
| Blueprint | `quill.blueprint` | `quill.blueprint` |
| Seed a starter | `quill.seed_document()` | `quill.seedDocument()` |

Both bindings share the core model, diagnostics, and storage format; they differ in language idiom, packaging, and extras (canvas preview is JavaScript-only).

## Where to go

- **[Programmatic Construction](programmatic.md)** — build and mutate a `Document` in memory (database row → PDF).
- **[Error Handling](error-handling.md)** — the diagnostic model every binding raises.
- **[Live Preview](live-preview.md)** — the `LiveSession` editor API: canvas paint, dirty-page repaint, and the click/caret bridge.
- **[Persistence](persistence.md)** — store a `Document` as versioned JSON.
- **[Blueprint & Seeding](../quills/blueprint.md)** — the LLM/MCP authoring surface and its filled-out twin.
