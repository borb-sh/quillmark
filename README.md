# Quillmark

[![Crates.io](https://img.shields.io/crates/v/quillmark.svg)](https://crates.io/crates/quillmark)
[![PyPI](https://img.shields.io/pypi/v/quillmark.svg?color=3776AB)](https://pypi.org/project/quillmark/)
[![npm](https://img.shields.io/npm/v/@quillmark/wasm.svg?color=CB3837)](https://www.npmjs.com/package/@quillmark/wasm)
[![CI](https://github.com/borb-sh/quillmark/workflows/CI/badge.svg)](https://github.com/borb-sh/quillmark/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-lightgray.svg)](LICENSE)

Quillmark is a schema-driven document framework. You define a document format once, as a **quill**: a schema for its fields and a layout for its pages, either a Typst plate or an existing PDF form. Authors then write plain Markdown, and Quillmark validates each document against its quill and renders it to PDF, SVG, or PNG.

<p>
  <img src="https://raw.githubusercontent.com/borb-sh/quillmark/main/docs/assets/gallery/memo-1.png" width="49%" alt="Page 1 of a rendered memorandum: letterhead, addressees, subject, references, numbered paragraphs, and a signature block">
  <img src="https://raw.githubusercontent.com/borb-sh/quillmark/main/docs/assets/gallery/memo-2.png" width="49%" alt="Page 2 of the memorandum: an indorsement with an approve or disapprove line and its own signature block">
</p>

Both pages come from one Markdown file, rendered through the [`usaf_memo`](https://github.com/borb-sh/quillmark/tree/main/crates/fixtures/resources/quills/usaf_memo/0.2.0) quill. Its main card, abridged ([full file](https://github.com/borb-sh/quillmark/blob/main/docs/assets/gallery/memo.md)):

```markdown
~~~
$quill: usaf_memo
letterhead_title: NORTHWIND REGIONAL AUTHORITY
letterhead_seal: ""
memo_for:
  - ALL DIVISION CHIEFS
subject: Transition to the Records Portal (Suspense 15 Mar 26)
signature_block:
  - JANE Q. DOE
  - Chief, Records Management
date: 2026-02-02
~~~

The Records Portal replaces the shared drive for official records on 1 April 2026.
```

- **One implementation, four surfaces**: a Rust crate, a Python package, an npm package (WebAssembly, with a live canvas preview), and a CLI.
- **Two backends**: Typst typesets new pages; `acroform` fills the fields of an existing PDF form.
- **Written by people or by LLMs**: the schema emits a JSON Schema for the document an LLM writes as JSON, and validation reports diagnostics with machine-readable codes at document paths.

[Try it in the browser](https://borb-sh.github.io/quillmark-js/playground): edit a document and watch the page re-render.

To write a quill of your own, start a quiver with [quillkit](https://github.com/borb-sh/quillmark-js/tree/main/packages/quillkit) and open it in studio, a form beside the live page. [Creating Quills](https://quillmark.dev/quills/creating-quills/) walks through it:

```bash
npx quillkit@latest init my-quiver
cd my-quiver && npm install && npx quillkit studio
```

[TongueToQuill](https://tonguetoquill.com) produces Air and Space Force correspondence with Quillmark. Quillmark is not affiliated with or endorsed by the Department of Defense, the Department of the Air Force, or any Military Service.

Maintained by [borb-sh](https://github.com/borb-sh).

## Status

Quillmark is pre-1.0, and its API changes between minor releases. Each breaking release ships a [migration guide](https://quillmark.dev/migrations/). A document saved as [versioned JSON](https://quillmark.dev/integration/persistence/) carries a version tag, and later releases migrate supported past versions forward on read.

## Documentation

- **[User Guide](https://quillmark.dev/)** - Tutorials, concepts, and bindings
- **[Rust API Reference](https://docs.rs/quillmark)** - Rust crate docs
- **[Changelog](CHANGELOG.md)** - Release notes and version history ([GitHub Releases](https://github.com/borb-sh/quillmark/releases))

## Installation

```bash
cargo add quillmark
```

## Quick Start (Rust)

```rust
use quillmark::{quill_from_path, CalendarDate, Engine, OutputFormat, RenderOptions};

// A `Quill` is declarative data: no engine needed to load it.
let quill = quill_from_path("path/to/quill")?;
let engine = Engine::new();

let markdown = r#"~~~
$quill: my_quill
$kind: main
title: Example
~~~

# Hello World
"#;

// Parse the Markdown and conform it to the quill's schema.
let doc = quill.parse(markdown)?.document;
// A render reads no clock: the host passes its local date.
let today: CalendarDate = "2026-03-14".parse()?;
let result = engine.render(
    &quill,
    &doc,
    today,
    &RenderOptions::default().with_output_format(OutputFormat::Pdf),
)?;

let pdf_bytes = &result.artifacts[0].bytes;
```

## Examples

```bash
cargo run --example usaf_memo
cargo run --example taro
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
