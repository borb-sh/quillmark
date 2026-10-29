# Quillmark CLI

`quillmark`, the command-line interface to [Quillmark](https://github.com/borb-sh/quillmark): renders Markdown with card-yaml blocks through a quill into PDF, SVG, or PNG.

Maintained by [borb-sh](https://github.com/borb-sh).

## Installation

Each [GitHub Release](https://github.com/borb-sh/quillmark/releases) carries
prebuilt binaries for Linux (x86_64, aarch64), macOS (Apple silicon, Intel) and
Windows (x86_64). With
[cargo-binstall](https://github.com/cargo-bins/cargo-binstall), the same
binaries install by crate name:

```bash
cargo binstall quillmark-cli
```

From source, which needs a Rust toolchain:

```bash
cargo install quillmark-cli --locked
```

The binary lands at `~/.cargo/bin/quillmark`. To build from a checkout instead:

```bash
cargo install --path crates/bindings/cli
```

`quillmark --version` prints the CLI's version and, in parentheses, the Typst
release plates compile against.

## Quick start

```bash
# The JSON Schema an LLM writes a document against
quillmark schema --authoring my_quill > authoring.schema.json

# Render a document; output defaults to document.pdf
quillmark render my_quill document.md

# Omit the markdown file to render the quill's seeded document
quillmark render my_quill -o preview.pdf

# Every diagnostic a document draws; --strict fails on warnings too
quillmark check --strict my_quill document.md

# Pipe the artifact instead of writing a file
quillmark render my_quill document.md --stdout | lpr
```

`my_quill` is a quill directory, the one holding `Quill.yaml`. The
[Quickstart](https://quillmark.dev/getting-started/quickstart/) builds one.

## Commands

Every command takes a `<QUILL_PATH>` pointing at a quill directory, or at a
`.quill` bundle of one; `workspace` takes the directory alone.

### `quillmark render [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]`

Renders a document. With `MARKDOWN_FILE` omitted, the quill's seeded document is
rendered instead, so a quill previews without any authored input.

- `-o, --output <FILE>` — output path (default: the input filename with the format's extension)
- `-f, --format <FORMAT>` — `pdf`, `svg`, or `png`; defaults to the `-o` extension when it names one, else `pdf`, and refuses one that disagrees with it
- `--ppi <PPI>` — pixels per inch for `png` (default: 144); a page past 16384 px a side at it is refused, naming the ppi that fits; `pdf` and `svg` have no pixel ceiling
- `--title <TITLE>` — the `pdf` document title (default: the plate's own, else the quill's `description`); `png` and `svg` ignore it
- `--stdout` — write the artifact to stdout; all chatter moves to stderr
- `--output-data <DATA_FILE>` — also write the compiled JSON data handed to the backend
- `--today <YYYY-MM-DD>` — the render date: what a `today` date and a plate's `datetime.today()` render as (default: the local date)
- `--font <FONT_FILE>` — a font file for the engine's font library, repeatable: a `.ttf`, `.otf`, `.ttc` or `.otc`
- `--quiet` — suppress warnings and the output-destination line

Warnings go to stderr, including one for each undeclared key or other input the
page leaves out. A render that fails still prints the parse and validation
warnings ahead of its error, and after them the backend's warnings from loading
the quill, such as a package skipped for its manifest.

### `quillmark check [--strict] [--regions] <QUILL_PATH> <MARKDOWN_FILE>...`

Checks documents against the quill's schema and prints every diagnostic.
Without `--regions` it does not compile the plate. Exits 1 where a document
draws an error; `--strict` exits 1 on any warning too, for CI. `--regions`
renders each document that drew no error and lists the filled fields no click
on the page reaches, and those only an enclosing field's region reaches; the
list never moves the exit code.

### `quillmark schema <QUILL_PATH> [--authoring]`

Prints the quill's field schema as YAML. `--authoring` prints the JSON Schema
of the quill's authoring document instead: the JSON an LLM writes, rich text as
Markdown strings.

### `quillmark validate <QUILL_PATH> [-v] [--no-render]`

Loads the quill — `Quill.yaml` parse errors, `default:` literals against
their declared types — checks referenced files, and renders the two
canonical documents (the empty document and the seeded document)
through the plate. `-v` prints each warning, such as a missing field description, where
a plain run only counts them; `--no-render` skips the render. Exits 1 where the configuration is invalid or a
canonical document does not render.

### `quillmark info <QUILL_PATH>`

Prints the quill's identity from its `quill:` section, and its field, card and
defaults counts.

### `quillmark workspace [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]`

Writes what Typst's own tooling needs to compile a Typst quill's plate against
one document (the generated helper package, the vendored packages, the fonts),
then prints the `typst watch` command that compiles it. The helper holds that
document's data: rerun `workspace` after the document changes. Typst reads files
and packages a render refuses, and the installed `typst` can be another release
than the embedded one, so a plate compiling there can still fail to render; the
[CLI reference](../../../docs/cli/reference.md#workspace) lists the differences.

- `-o, --output <DIR>` — workspace directory (default: `quillmark-workspace`)
- `--today <YYYY-MM-DD>` — the render date: what a `today` date and, through the printed command's `--creation-timestamp`, a plate's `datetime.today()` render as (default: the local date)
- `--font <FONT_FILE>` — a font file for the engine's font library, repeatable, as on `render`; written into the workspace's `fonts/`
- `--quiet` — suppress warnings and the command line

### `quillmark pack [-o FILE] <QUILL_PATH>`

Writes the quill as one `.quill` bundle, its file tree as a ZIP archive with
`Quill.yaml` at the root: the file `Quill.fromBundle` in `@quillmark/wasm` and
`Quill.from_bundle` in Python read. The default output is
`<name>@<version>.quill` in the working directory; one inside the quill is
refused.

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
