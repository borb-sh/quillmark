# Seeding & the Empty Document

A quill's schema yields two ready-made documents: a **seed** (a starter document to edit) and the **empty document** (the type-minimal input every plate must render). Both come from `Quill.yaml` alone: no one hand-writes them. A model authors through the [authoring document](../integration/llm-authoring.md) instead.

## Seeding: the starter document

Seeding materializes a real `Document`: the main card plus one card per composable kind, each carrying its kind's [`seed`](quill-yaml-reference.md#card-level-seed) and every other field left absent so the render floor fills `default:`, else the field's blank. Hand it to an editor as a "new document" starter, or render it directly.

## Accessors

| | Seed | Empty | Example |
|---|---|---|---|
| Python | `quill.seed_document()` | `quill.empty_document()` | `quill.example_document()` |
| JavaScript | `quill.seedDocument()` | `quill.emptyDocument()` | `quill.exampleDocument()` |
| Rust | `Quill::seed_document()` | `Quill::empty_document()` | `Quill::example_document()` |
| CLI | `quillmark render <quill>` (no input file) | `quillmark validate <quill>` renders it and the seed | `quillmark validate <quill>` |

## The empty-document contract

**A plate MUST render an empty document** (just `$quill` / `$kind: main`, no fields) without error.

Under blank-filled render every absent field becomes its blank, so the empty document is the type-minimal valid input. A plate that renders it degrades gracefully on every valid shape. Two rules follow:

- **No template asserts a declared field is *non-empty*.** The schema guarantees presence, not non-emptiness.
- **A template branching on an `enum` covers `values ∪ blank` exhaustively.** The blank is valid present input for every enum, so an `else` fallback renders a variant nobody chose.

Bundled quills are checked against this by fixture tests; `quillmark validate <quill>` runs the same check over any quill, rendering the empty document and the seeded document through its backend.

Full model: [QUILL.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/QUILL.md) § "The plate contract"; the seeding cascade is in [SCHEMAS.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/SCHEMAS.md) § "Document seeding".
