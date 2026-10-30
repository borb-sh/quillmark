# Quillmark Canon Index

## Core

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Crate structure and system overview
- **[ERROR.md](ERROR.md)** - Structured diagnostics and cross-language serialization

## Components

- **[../references/markdown-spec.md](../references/markdown-spec.md)** - Quillmark Markdown specification (superset of CommonMark)
- **[../references/quill-bundle.md](../references/quill-bundle.md)** - The `.quill` bundle: one quill's file tree as one ZIP archive
- **[DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)** - Versioned JSON serialization of `Document` for database persistence
- **[COMMENTS.md](COMMENTS.md)** - Comment anchors: the thread outside the document, the selector, and the exact re-attachment rule
- **[PARITY.md](PARITY.md)** - What each surface does with each content construct, the invariants they keep, and the corpus that pins them
- **[QUILL.md](QUILL.md)** - Quill resource file structure and the declarative `Quill` data type
- **[VERSIONING.md](VERSIONING.md)** - Quill version format and `$quill` reference syntax (selector parsed, not runtime-resolved)
- **[SCHEMAS.md](SCHEMAS.md)** - `QuillConfig` schema model, native validation, what blocks a render, and emission overview
- **[AUTHORING.md](AUTHORING.md)** - The JSON authoring document and its schema, for LLM/MCP authoring
- **[PROGRAMMATIC.md](PROGRAMMATIC.md)** - Building documents in memory (blank canvas, batched mutators) for automation
- **[CARDS.md](CARDS.md)** - Composable cards delivered on the `$cards` plate-JSON array

## Backends

A backend's core-side seam is canon; its quill-authoring surface is a `docs/`
page and its internals are rustdoc.

- **[CONVERT.md](CONVERT.md)** - How the Typst backend lowers a `Content` value to Typst markup
- **[PLATE_DATA.md](PLATE_DATA.md)** - Plate data injection: the Typst backend's data seam
- The `acroform` seam is [ARCHITECTURE.md](ARCHITECTURE.md) (`Backend::open`, the two-asset model) plus [PREVIEW.md](PREVIEW.md) (canvas paint, `regions()`)
- Outbound, authoring a Typst quill: [docs/quills/typst-backend.md](../../docs/quills/typst-backend.md)
- Outbound, authoring an `acroform` quill: [docs/quills/acroform-backend.md](../../docs/quills/acroform-backend.md) (`form.pdf` + `Form.yaml`, on the `quillmark-pdf` stamp spine)
- Outbound, Typst backend internals: `crates/backends/typst/` rustdoc

## Bindings

- **[BINDINGS.md](BINDINGS.md)** - Language surfaces (Python, WASM, CLI) over the one core library
- **[CLI.md](CLI.md)** - Command-line interface
- **[PREVIEW.md](PREVIEW.md)** - WASM live preview: LiveSession (update/ChangeSet) + multi-backend canvas paint (Typst, acroform)
