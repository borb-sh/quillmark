# Quillmark CLI

`quillmark`, the command-line interface to [Quillmark](https://github.com/borb-sh/quillmark): renders Markdown with card-yaml blocks through a quill into PDF, SVG, or PNG.

Maintained by [TTQ](https://tonguetoquill.com).

## Installation

```bash
cargo install quillmark-cli
```

The binary lands at `~/.cargo/bin/quillmark`. To build from a checkout instead:

```bash
cargo install --path crates/bindings/cli
```

## Quick start

```bash
# Render a document; output defaults to document.pdf
quillmark render ./quills/usaf_memo document.md

# Omit the markdown file to render the quill's seeded document
quillmark render ./quills/usaf_memo -o preview.pdf

# Every diagnostic a document draws; --strict fails on warnings too
quillmark check --strict ./quills/usaf_memo document.md

# Pipe the artifact instead of writing a file
quillmark render ./quills/usaf_memo document.md --stdout | evince -
```

## Commands

Every command takes a `<QUILL_PATH>` pointing at a quill directory.

### `quillmark render [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]`

Renders a document. With `MARKDOWN_FILE` omitted, the quill's seeded document is
rendered instead, so a quill previews without any authored input.

- `-o, --output <FILE>` — output path (default: the input filename with the format's extension)
- `-f, --format <FORMAT>` — `pdf` (default), `svg`, or `png`
- `--stdout` — write the artifact to stdout; all chatter moves to stderr
- `--output-data <DATA_FILE>` — also write the compiled JSON data handed to the backend
- `--quiet` — suppress warnings and the output-destination line

Warnings go to stderr: input the page leaves out, such as an undeclared key,
prints in full, and fields the document has yet to answer print as one count.

### `quillmark check [--strict] <QUILL_PATH> <MARKDOWN_FILE>...`

Checks documents against the quill's schema and prints every diagnostic,
unanswered fields included, without compiling the plate. Exits 1 where a
document draws an error; `--strict` exits 1 on any warning too, for CI.

### `quillmark schema <QUILL_PATH>`

Prints the quill's field schema as YAML.

### `quillmark blueprint <QUILL_PATH>`

Prints an annotated Markdown blueprint: a starting document with every declared
field, `!must_fill` where a value is expected.

### `quillmark validate <QUILL_PATH> [-v] [--no-render]`

Loads the quill — `Quill.yaml` parse errors, `example:`/`default:` literals
against their declared types — checks referenced files, and renders the three
canonical documents (the empty document, the blueprint, the seed) through the
plate. `-v` adds advisory warnings such as missing field descriptions;
`--no-render` skips the render. Exits 1 where the configuration is invalid or a
canonical document does not render.

### `quillmark info <QUILL_PATH>`

Prints the quill's identity — name, version, author, backend — and its field,
card and defaults counts.

## Exit codes

`0` on success, `--help`, and `--version`. `2` where argument parsing rejected
the invocation before any command ran — an unknown flag, a missing argument, an
unknown subcommand. `1` where the command ran and refused — an invalid quill, a
missing file, a failed render, a failed `check`, an argument value the command
itself rejects (`-f docx`). Diagnostics go to stderr.

## Links

- [CLI design document](../../../prose/canon/CLI.md)
- [Changelog](https://github.com/borb-sh/quillmark/blob/main/CHANGELOG.md) and [releases](https://github.com/borb-sh/quillmark/releases)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../../LICENSE).
