# Integrating Quillmark

Quillmark embeds in an app through two published bindings (**Python** (`quillmark` on PyPI) and **JavaScript/WASM** (`@quillmark/wasm` on npm)) over one Rust library. Rust consumers use the crates directly ([docs.rs](https://docs.rs/quillmark)).

Three objects carry every integration:

- **Engine**: a backend registry and render dispatcher. It holds no quills; every render routes through it, and it never constructs a quill. `Engine::new()` in Rust, `Engine()` in Python, `new Engine()` in JavaScript.
- **Quill**: a loaded quill. A quill is a document format: a schema for a document's fields, a layout for its pages, and the assets the layout uses. Loaded once as portable, declarative data, it renders through any `Engine` that registers its backend, and never renders itself.
- **Document**: the typed model of one Quillmark Markdown file (root block, body, cards). Built from Markdown, from a blank canvas, or from stored JSON.

Field I/O is schema-bound: `quill.writer(doc)` writes each field by its declared type and `quill.reader(doc)` reads it back. `Document` itself is quill-free data: parse, persist, structure.

Both bindings share the core model, diagnostics, and storage format; they differ in language idiom (`snake_case` vs. camelCase), packaging, and extras (canvas preview is JavaScript-only). Exact method signatures live with each binding (`import quillmark` and the `@quillmark/wasm` `.d.ts`) rather than in a table here that would drift as the pre-1.0 surface moves; the task pages below show the calls in context.

## Where to go

- **[Programmatic Construction](programmatic.md)**: build and mutate a `Document` in memory (database row → PDF).
- **[Error Handling](error-handling.md)**: the diagnostic model every binding raises.
- **[Persistence](persistence.md)**: store a `Document` as versioned JSON.
- **[Operations](operations.md)**: what render does not bound, concurrency, and what a long-lived process accumulates.
- **[LLM Authoring](llm-authoring.md)**: the JSON document a model writes, its schema, and the import and export.

!!! note "Advanced: live preview"
    Canvas / `LiveSession` editor integration (incremental `update`, `paint`, and the click/caret geometry bridge) is WASM-only and single-consumer. Its design contract lives in [PREVIEW.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PREVIEW.md); [Live Preview](live-preview.md) has the minimal paint loop.
