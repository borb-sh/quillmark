# Quillmark

[![Crates.io](https://img.shields.io/crates/v/quillmark.svg)](https://crates.io/crates/quillmark)
[![PyPI](https://img.shields.io/pypi/v/quillmark.svg?color=3776AB)](https://pypi.org/project/quillmark/)
[![npm](https://img.shields.io/npm/v/@quillmark/wasm.svg?color=CB3837)](https://www.npmjs.com/package/@quillmark/wasm)
[![Documentation](https://docs.rs/quillmark/badge.svg)](https://docs.rs/quillmark)

Quillmark is a schema-driven document framework. You define a document format once, as a **quill**: a schema for its fields and a layout for its pages, either a Typst plate or an existing PDF form. Authors then write plain Markdown, and Quillmark validates each document against its quill and renders it to PDF, SVG, PNG, or, from a Typst plate, Word.

<div class="grid" markdown>

![Page 1 of a rendered memorandum: letterhead, addressees, subject, references, numbered paragraphs, and a signature block](assets/gallery/memo-1.png){ style="border: 1px solid var(--md-default-fg-color--lightest)" }

![Page 2 of the memorandum: an indorsement with an approve or disapprove line and its own signature block](assets/gallery/memo-2.png){ style="border: 1px solid var(--md-default-fg-color--lightest)" }

</div>

??? example "The Markdown behind these pages, rendered through the `usaf_memo` quill"

    ```markdown
    --8<-- "docs/assets/gallery/memo.md"
    ```

<small>Quillmark is not affiliated with or endorsed by the Department of Defense, the Department of the Air Force, or any Military Service.</small>

[Try it in the browser](https://play.quillmark.dev/playground): edit a document and watch the page re-render.

## Status

--8<-- "README.md:status"

## Choose your path

- **Writing documents?** You write Markdown documents in the document format a quill defines.  
  → [Markdown Syntax](authoring/markdown-syntax.md)

- **Building quills?** You create quills, the document formats that control rendering. `npx quillkit@latest init` starts one, and studio shows its form beside the live page.  
  → [Creating Quills](quills/creating-quills.md) · [quillkit & studio](quills/quillkit.md)

- **Integrating into an app?** You use Quillmark via Python or JavaScript.  
  → [Quickstart](getting-started/quickstart.md) · [Integrating Quillmark](integration/index.md)

- **Using the CLI?** You render and validate from the command line.  
  → [CLI Reference](cli/reference.md)

- **Using Rust?** API documentation is on [docs.rs](https://docs.rs/quillmark).

New to the words? The [Glossary](getting-started/glossary.md) defines each one in a line or two.
