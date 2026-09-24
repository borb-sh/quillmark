# Quillmark

[![Crates.io](https://img.shields.io/crates/v/quillmark.svg)](https://crates.io/crates/quillmark)
[![PyPI](https://img.shields.io/pypi/v/quillmark.svg?color=3776AB)](https://pypi.org/project/quillmark/)
[![npm](https://img.shields.io/npm/v/@quillmark/wasm.svg?color=CB3837)](https://www.npmjs.com/package/@quillmark/wasm)
[![CI](https://github.com/borb-sh/quillmark/workflows/CI/badge.svg)](https://github.com/borb-sh/quillmark/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-lightgray.svg)](LICENSE)
 
Quills define document presentation and schema. Use a quill to generate a formatted document from markdown or code.

Maintained by [borb-sh](https://github.com/borb-sh).

**UNDER DEVELOPMENT**: APIs may change.

## Documentation

- **[User Guide](https://borb-sh.github.io/quillmark/)** - Tutorials, concepts, and bindings
- **[Rust API Reference](https://docs.rs/quillmark)** - Rust crate docs
- **[Changelog](CHANGELOG.md)** - Release notes and version history ([GitHub Releases](https://github.com/borb-sh/quillmark/releases))

## Installation

```bash
cargo add quillmark
```

## Quick Start (Rust)

```rust
use quillmark::{quill_from_path, OutputFormat, Quillmark, RenderOptions};

// A `Quill` is declarative data: no engine needed to load it.
let quill = quill_from_path("path/to/quill")?;
let engine = Quillmark::new();

let markdown = r#"~~~
$quill: my_quill
$kind: main
title: Example
~~~

# Hello World
"#;

// The bound door: parse and conform against the quill that will render it.
let doc = quill.parse(markdown)?.document;
let result = engine.render(
    &quill,
    &doc,
    Some("2026-03-14".parse()?),
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

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
