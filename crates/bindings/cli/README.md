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
- `-f, --format <FORMAT>` — `pdf`, `svg`, or `png`; defaults to the `-o` extension when it names one, else `pdf`, and refuses one that disagrees with it
- `--stdout` — write the artifact to stdout; all chatter moves to stderr
- `--output-data <DATA_FILE>` — also write the compiled JSON data handed to the backend
- `--today <YYYY-MM-DD>` — the render date: what a `today` date and a plate's `datetime.today()` render as (default: the local date)
- `--quiet` — suppress warnings and the output-destination line

Warnings go to stderr, including one for each undeclared key or other input the
page leaves out. A render that fails still prints the parse and validation
warnings ahead of its error.

### `quillmark check [--strict] <QUILL_PATH> <MARKDOWN_FILE>...`

Checks documents against the quill's schema and prints every diagnostic,
without compiling the plate. Exits 1 where a
document draws an error; `--strict` exits 1 on any warning too, for CI.

### `quillmark schema <QUILL_PATH>`

Prints the quill's field schema as YAML.

### `quillmark blueprint <QUILL_PATH>`

Prints an annotated Markdown blueprint: a starting document with every declared
field, each holding its `default:` or left empty (`title: # string`) where a
value is expected, and each `example:` on a `# e.g.` line above its field.

### `quillmark validate <QUILL_PATH> [-v] [--no-render]`

Loads the quill — `Quill.yaml` parse errors, `example:`/`default:` literals
against their declared types — checks referenced files, and renders the three
canonical documents (the empty document, the blueprint, the seed) through the
plate. `-v` prints each warning, such as a missing field description, where
a plain run only counts them; `--no-render` skips the render. Exits 1 where the configuration is invalid or a
canonical document does not render.

### `quillmark info <QUILL_PATH>`

Prints the quill's identity — name, version, author, backend — and its field,
card and defaults counts.

### `quillmark workspace [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]`

Writes what Typst's own tooling needs to compile a Typst quill's plate against
one document (the generated helper package, the vendored packages, the fonts),
then prints the `typst watch` command that compiles it. The helper holds that
document's data: rerun `workspace` after the document changes.

- `-o, --output <DIR>` — workspace directory (default: `quillmark-workspace`)
- `--today <YYYY-MM-DD>` — what a `today` date renders as (default: the local date); a plate's `datetime.today()` is Typst's to supply
- `--quiet` — suppress warnings and the command line

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
