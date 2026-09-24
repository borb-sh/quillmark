# Blueprint & Seeding

A quill's schema yields two ready-made documents: a **blueprint** (an annotated form to fill) and a **seed** (a starter document to edit). Both come from `Quill.yaml` alone: no one hand-writes them.

## Blueprint: the authoring surface

`blueprint()` emits an annotated Markdown document, the same shape an author writes: each cell holds its `default:` or is left empty, with examples and type hints as comments. It is the authoring surface for LLM and MCP consumers: answer the empty cells and the structure, `$` metadata, and body markers come for free. The emitted document is itself valid: it parses, round-trips, and renders.

```
~~~
$quill: cmu_letter@0.1.0 # keep verbatim
$kind: main
# The recipient's name and full mailing address.
# e.g. [Mr. John Doe, 123 Main St]
recipient: # array<string>
# The department name for the letterhead.
# e.g. Department of Electrical and Computer Engineering
department: "" # string
~~~

Write main body here.
```

Two annotation slots, disjoint by purpose: **leading `# …` lines** carry prose (a description, an `# e.g.` example) plus an `array`'s `# up to <N>` cap; the **inline `# …`** at the end of a value line carries structure, the field's `# <type>[<format>]`. A closed vocabulary shows its whole roster there — `# matrix<flight_cc | dodin_ops>` — so a reader can tick a member without looking the schema up, and cannot invent one. A matrix with columns adds an `# e.g.` line spelling one held member, `{flight_cc: {held: true, detail: …}}`, which names every column; the member it picks is only an illustration.

One thing in the own-line slot is not an annotation. An `enum` declaring `variants:` shows the cells of the world its discriminant names live, and every other world's cells commented out under a `# when <MEMBER>:` header — the same cells, with a `# ` in front, at the column they would sit at. Choose that member and delete the `# `.

The reader's one rule: an empty cell (`title: # string`) awaits a value; a concrete value is the field's `default:`, shippable as-is. An `example:` never takes a cell: it always rides a `# e.g.` line above the field, as a one-line flow collection for an array or object, and is the schema's illustration rather than real data. An empty cell renders at the field's blank, and nothing warns about it.

## Seeding: the starter document

Seeding materializes a real `Document` rather than an annotated string: the main card plus one card per composable kind, each body taken from `body.example`, and every field left absent so the render floor fills `default:`, else the field's blank. No `example:` is committed. Hand it to an editor as a "new document" starter, or render it directly.

| Projection | Intent | Output |
|---|---|---|
| `blueprint` | "give me the form to fill" | annotated Markdown string |
| seeding | "give me a starter document" | committed `Document` |

## Accessors

| | Blueprint | Seed | Empty |
|---|---|---|---|
| Python | `quill.blueprint` | `quill.seed_document()` | `quill.empty_document()` |
| JavaScript | `quill.blueprint` | `quill.seedDocument()` | `quill.emptyDocument()` |
| Rust | `QuillConfig::blueprint()` | `Quill::seed_document()` | `Quill::empty_document()` |
| CLI | `quillmark blueprint <quill>` | `quillmark render <quill>` (no input file) | `quillmark validate <quill>` renders all three |

## The empty-document contract

`blueprint()` guarantees the emitted document renders, but that also depends on the quill's `plate.typ`. The quill authoring contract: **a plate MUST render an empty document** (just `$quill` / `$kind: main`, no fields) without error.

Under blank-filled render every absent field becomes its blank, so the empty document is the type-minimal valid input. A plate that renders it degrades gracefully on every valid shape. Two rules follow:

- **No template asserts a declared field is *non-empty*.** The schema guarantees presence, not non-emptiness.
- **A template branching on an `enum` covers `values ∪ blank` exhaustively.** The blank is valid present input for every enum, so an `else` fallback renders a variant nobody chose.

Bundled quills are checked against this by fixture tests; `quillmark validate <quill>` runs the same check over any quill, rendering the empty document, the blueprint and the seed through its backend.

Full model: [BLUEPRINT.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/BLUEPRINT.md); the seeding cascade is in [SCHEMAS.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/SCHEMAS.md) § "Document seeding".
