# Concepts

Quillmark is the whole framework: the Markdown format and its card-yaml blocks, the quill schema model, the engine that renders, the Typst and acroform backends, and the Rust, Python, JavaScript and CLI surfaces over them.

## Terms

The engine's own terms are below. The words a quill author meets (card, seed, authoring document, blank, ink, region, quiver and the rest) are in the [Glossary](glossary.md).

| Term | Means |
|---|---|
| Quillmark | The framework. Also a release, as in "Quillmark 0.127". |
| engine | The runtime object that renders: a backend registry and render dispatcher, `Engine` in Rust, Python and JavaScript alike. |
| backend | Typst or acroform: turns a quill plus document data into PDF, SVG or PNG. |
| parser | serde-saphyr for YAML, pulldown-cmark for Markdown. |
| core | The `quillmark-core` crate: parsing, schema validation, and the diagnostic printer. The *core build* is the Typst-free WASM build. |
| quill | A document format: a schema for a document's fields, a layout for its pages, and the assets the layout uses. `Quill` is its class. |
| plate | The Typst template a Typst quill lays its pages out with. An acroform quill has none. |

## How Quillmark separates concerns

Quillmark separates content, schema, and presentation:

- **A quill's schema drives the data**: it validates the card-yaml metadata and applies coercion, defaults, and scaffolding. The schema is the contract every render consumes.
- **A quill's layout controls presentation**: a Typst plate, or for acroform a PDF form, lays out and typesets the output artifact.
- **Markdown provides content**: authors write plain Markdown with card-yaml blocks; Quillmark renders it through the quill into a fully typeset document.

## Core Components

### Quills

A **quill** is a document format: a schema for a document's fields, a layout for its pages, and the assets the layout uses. On disk it is a directory holding:

- **Metadata** (`Quill.yaml`): name, backend, and field schemas
- **Layout**: a Typst plate, or for acroform, `form.pdf` and `Form.yaml`
- **Assets**: fonts, images, and other resources the layout uses

### card-yaml Blocks

Quillmark documents use **card-yaml blocks** to provide structured metadata. A
card-yaml block is delimited by bare `~~~` / `~~~` fences and may begin
with a run of `$`-prefixed system metadata lines followed by a YAML payload.
(The opener's info string is ignored, so `~~~card-yaml` and `~~~yaml` are
accepted too; the canonical opener is a bare `~~~`. To write a literal fenced
*code* block in prose, use a backtick fence: no tilde form escapes, as both
`~~~~` and `~~~rust` still open a card.)

```markdown
~~~
$quill: my_format
$kind: main
title: My Document
author: John Doe
date: 2025-01-15
~~~

# Content starts here
```

The plate reads these fields, and the quill's schema validates them. See [card-yaml Blocks](../authoring/card-yaml.md) for the full syntax.

### Backends

A backend compiles a quill's backend-specific inputs plus injected JSON data into the final artifact. Quillmark ships two backends:

| Backend | Reads | Produces |
|---|---|---|
| Typst | a plate | PDF, SVG, and PNG; fields declared `type: richtext` are lowered to Typst markup during compilation |
| acroform | a stripped PDF background and field spec, instead of a plate | an existing PDF form with its fields filled directly |

### Required `$quill` Metadata

Each document names its quill in the root block's `$quill` system metadata line. If missing, parsing fails. Quill names must be `snake_case` (`[a-z][a-z0-9_]*`); hyphens are not allowed.

## The Rendering Pipeline

Quillmark follows a three-stage pipeline:

1. **Parse** - Read the card-yaml blocks and the Markdown under them, and check each value against the quill's schema, converting it to the field's type and filling in defaults. A field the document leaves out reaches the backend as its blank, so a half-written document still renders; the blank is never saved into the document.
2. **Compile** - The backend takes the quill's layout and the document's data and produces the pages (PDF, SVG or PNG).
3. **Output** - The render returns the pages, each as bytes.

```
Markdown + YAML → Parse/Normalize → Compile (Backend) → Artifacts
```

## Next Steps

- [Create your first quill](../quills/creating-quills.md)
- [Learn quill versioning](../quills/versioning.md)
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
- [Explore the Typst backend](../quills/typst-backend.md)
