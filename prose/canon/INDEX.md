# Quillmark Canon Index

## Core

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Crate structure and system overview
- **[ERROR.md](ERROR.md)** - Structured diagnostics and cross-language serialization

## Components

- **[../references/markdown-spec.md](../references/markdown-spec.md)** - Quillmark Markdown specification (superset of CommonMark)
- **[DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)** - Versioned JSON serialization of `Document` for database persistence
- **[QUILL.md](QUILL.md)** - Quill resource file structure and the declarative `Quill` data type
- **[VERSIONING.md](VERSIONING.md)** - Quill version format and `$quill` reference syntax (selector parsed, not runtime-resolved)
- **[SCHEMAS.md](SCHEMAS.md)** - `QuillConfig` schema model, native validation, and emission overview
- **[BLUEPRINT.md](BLUEPRINT.md)** - Annotated Markdown blueprint for LLM/MCP authoring
- **[PROGRAMMATIC.md](PROGRAMMATIC.md)** - Building documents in memory (blank canvas, batched mutators) for automation
- **[CARDS.md](CARDS.md)** - Composable cards delivered on the `$cards` plate-JSON array

## Backends

A backend's engine-side seam is canon; its quill-authoring surface is a `docs/`
page and its internals are rustdoc.

- **[CONVERT.md](CONVERT.md)** - How the Typst backend lowers a `Content` value to Typst markup
- **[PLATE_DATA.md](PLATE_DATA.md)** - Plate data injection: the Typst backend's data seam
- The `pdfform` seam is [ARCHITECTURE.md](ARCHITECTURE.md) (`Backend::open`, the two-asset model) plus [PREVIEW.md](PREVIEW.md) (canvas paint, `regions()`)
- Outbound, authoring a Typst quill: [docs/quills/typst-backend.md](../../docs/quills/typst-backend.md)
- Outbound, authoring a `pdfform` quill: [docs/quills/pdfform-backend.md](../../docs/quills/pdfform-backend.md) (`form.pdf` + `form.json`, Technique A stamping, on the `quillmark-pdf` stamp spine)
- Outbound, Typst backend internals: `crates/backends/typst/` rustdoc

## Bindings

- **[BINDINGS.md](BINDINGS.md)** - Language surfaces (Python, WASM, CLI) over the one core engine
- **[CLI.md](CLI.md)** - Command-line interface
- **[PREVIEW.md](PREVIEW.md)** - WASM live preview: LiveSession (update/ChangeSet) + multi-backend canvas paint (Typst, pdfform)
