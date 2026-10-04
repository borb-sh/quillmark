# CLI Reference

Command-line interface for Quillmark rendering.

## Installation

```bash
cargo install quillmark-cli
```

## Commands

### render

Render a markdown document to the specified output format.

```bash
quillmark render [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory
- `[MARKDOWN_FILE]`: Path to markdown file with a root card-yaml block (optional, when omitted, the quill's seeded document is rendered: one card per kind with empty bodies, every field at its `default:` or blank)

The file must open with a `~~~` block containing a `$quill:` key identifying the quill; the opener's info string is ignored.

**Options:**

- `-o <PATH>` / `--output <PATH>`: Output file path (default: input filename with format extension, e.g. `input.pdf`; `example.<format>` when no markdown file is given)
- `-f <FORMAT>` / `--format <FORMAT>`: Output format: `pdf`, `svg`, `png` (default: the `-o` extension when it names one of these, else `pdf`). A `-f` that disagrees with such an extension is refused (`-f png -o out.pdf`); an `-o` extension naming no format is written as given.
- `--output-data <DATA_FILE>`: Write the compiled, blank-filled JSON data to a file. This is the data before the backend lowers it: a `richtext` value appears as a content object (`{text, lines, marks, islands}`) and a date as its string, where a Typst plate receives content and a `datetime`.
- `--today <YYYY-MM-DD>`: The render date: what a `today` date field and a plate's `datetime.today()` render as (default: the local date)
- `--quiet`: Suppress warnings and the output-destination line; errors still print
- `--stdout`: Write the artifact to stdout instead of a file (and ignore `-o`); refused when the render produces more than one page

**Warnings:** `render` prints the parse warnings, then each warning for input the page leaves out — an undeclared key (`validation::unknown_field`), a card no kind claims, a body under `body.enabled: false`, a stranded variant cell, elements past `max:` — then the backend's. A render that fails prints, ahead of its error, the first two and then the backend's warnings from loading the quill, which a failed compile still carries. Those explain an error naming only a missing file: a package skipped for its manifest warns `typst::package_manifest`, and an import of it fails as `typst::file_not_found`.

**Streams:** under `--stdout` the artifact owns stdout, and warnings and errors go to stderr, so `quillmark render ./my-quill input.md --stdout > out.pdf` writes a valid PDF. Without `--stdout`, the one stdout line is `Output written to: <path>`, which `--quiet` suppresses.

**Pages:** `svg` and `png` render one artifact per page. A multi-page document writes one numbered file per page — `out.svg` becomes `out-1.svg`, `out-2.svg`, … — so no unnumbered file claims to be the whole document. `--stdout` carries one artifact and refuses a multi-page render.

**Examples:**

```bash
# Render to PDF
quillmark render ./invoice-quill input.md -o output.pdf

# Render to SVG
quillmark render ./my-quill input.md -f svg -o output.svg

# Emit compiled data for inspection
quillmark render ./my-quill input.md --output-data data.json

# Output to stdout
quillmark render ./my-quill input.md --stdout > output.pdf

# Render the quill's seeded document
quillmark render ./my-quill
```

### check

Check markdown documents against a quill's schema, printing every diagnostic each one draws: parse warnings, then every `validation::*` diagnostic. It does not compile the plate, so a plate failure or a construct the backend declines (`backend::declined_construct`) is `render`'s to report.

```bash
quillmark check [OPTIONS] <QUILL_PATH> <MARKDOWN_FILE>...
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory
- `<MARKDOWN_FILE>...`: One or more documents to check. Each document's diagnostics print under its path. A document that is missing or unreadable draws `cli::unreadable_document`, one that fails to parse draws its parse error, and neither stops the rest.

**Options:**

- `--strict`: Exit `1` on any warning, not only on an error. An undeclared key and a card no kind claims both fail a strict check.

Without `--strict`, `check` exits `1` only on an error: a parse error, a value that is not its field's type, a `$quill` naming another quill.

**Examples:**

```bash
# Every diagnostic for one document
quillmark check ./my-quill input.md

# CI gate: fail on anything unread (an undeclared key, a card no kind claims)
quillmark check --strict ./my-quill documents/*.md
```

### schema

Output the quill's field schema as YAML, including main-card and card-kind field definitions with UI hints.

```bash
quillmark schema <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Examples:**

```bash
# Print schema to stdout
quillmark schema ./my-quill

# Save schema to file
quillmark schema ./my-quill > schema.yaml
```

### blueprint

Print a quill's Markdown blueprint: an annotated document showing the quill's fields, their descriptions, and their constraints, itself a valid document an author can fill in.

```bash
quillmark blueprint <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Examples:**

```bash
# Print blueprint to stdout
quillmark blueprint ./my-quill

# Save blueprint to file
quillmark blueprint ./my-quill > blueprint.md
```

### validate

Validate quill configuration and structure, and compile the plate against the
three canonical documents: the empty document, the blueprint, and the seeded
document. A plate that renders the seeded document and not the empty one fails
here, which is the quill authoring contract every plate is bound by.

A quill with an `example.md` at its root has that example document read,
validated, and rendered too. Any diagnostic on it fails the quill, a warning
included (`cli::example_not_clean`): the example's values are the author's own,
so a warning there is a mistake to fix. `--no-render` still reads and validates
it. `--verbose` names the example it found, or says there is none, so a
misspelled `examples.md` shows.

```bash
quillmark validate [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `-v` / `--verbose`: Show each check as it runs, and print warnings as well as errors. Among them, `cli::missing_description` flags every field and card kind without a `description:`, the help text editors and the blueprint show.
- `--no-render`: Skip the render check: only read the configuration

**Examples:**

```bash
# Validate quill structure and compile the plate
quillmark validate ./my-quill

# Verbose validation
quillmark validate ./my-quill -v

# Configuration only, no compile
quillmark validate ./my-quill --no-render
```

### info

Display a quill's identity and schema counts.

```bash
quillmark info <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Examples:**

```bash
# Display quill info
quillmark info ./my-quill
```

### workspace

Write the files Typst's own tooling needs to compile a Typst quill's plate: the
`@local/quillmark-helper` package generated from one document, every vendored
package at the path Typst's package resolution reads, and the fonts the backend
renders in. It then prints the `typst watch` command that compiles the plate
from the quill directory, so an edit to the plate or a module shows on the next
save. The helper holds that one document's data: rerun `workspace` after the
document changes.

Before the command, `workspace` prints the parse warnings, each warning for
input the page leaves out, and the warnings a render raises loading the quill
(`typst::package_manifest`, `typst::package_entrypoint_missing`,
`typst::path_skipped`, `typst::unknown_key`). It compiles nothing, so the
plate's own warnings and errors are the printed command's to report.

```bash
quillmark workspace [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]
```

**Arguments:**

- `<QUILL_PATH>`: Path to a quill whose `backend` is `typst`; any other backend refuses with `typst::wrong_backend`
- `[MARKDOWN_FILE]`: Path to markdown file with a root card-yaml block (optional, when omitted, the quill's seeded document)

**Options:**

- `-o <DIR>` / `--output <DIR>`: Workspace directory (default: `quillmark-workspace`). A directory inside the quill is refused: the quill would load it as its own files
- `--today <YYYY-MM-DD>`: The render date (default: the local date). A `today` date field renders as it, and the printed command passes it as `--creation-timestamp`, noon UTC of that date, so the plate's `datetime.today()` returns it too.
- `--quiet`: Suppress warnings and the command line; errors still print

**Failures:** besides `typst::wrong_backend`, the command refuses each plate a render refuses, before writing anything: `typst::plate_missing` when `typst.plate_file` names no file in the quill, `typst::plate_path_invalid` when it names one at a path Typst cannot load, and `typst::invalid_utf8` when the plate is not UTF-8. A quill declaring no `typst.plate_file`, which a render compiles as an empty plate, has no plate to export and fails as `typst::plate_missing`.

**Examples:**

```bash
quillmark workspace ./my-quill input.md -o ws --today 2026-03-14
# Workspace written to: ws
# typst watch --root ./my-quill --package-path ws/packages --font-path ws/fonts --ignore-system-fonts --ignore-embedded-fonts --creation-timestamp 1773489600 ./my-quill/plate.typ ws/plate.pdf
```

Tinymist takes the same flags through its `tinymist.typstExtraArgs` setting,
which gives an editor completion on `data.` fields and a live preview.

**What the printed command reads that a render does not:** Typst serves the
plate any file under `--root` and any package it can reach, where a render
loads only part of the quill. A plate that compiles under the command can
still fail to render, as `typst::file_not_found`:

| The plate reaches | A render | The printed command |
|---|---|---|
| A `.typ` module outside `packages/` | Loads it | Loads it |
| A file under `assets/`, for `image`, `read`, `json` and the like | Loads it | Loads it |
| Any other file in the quill: `read("data.csv")` beside the plate, or a vendored package's `/packages/<dir>/lib.typ` by its path | Refuses it | Loads it |
| A file the quill's load skips: a symlink, or anything under the quill's own `.git/`, `target/` or `node_modules/` | Refuses it | Loads it |
| A package the quill does not vendor | Refuses it: Quillmark never downloads a package | Loads it from Typst's package cache, downloading an `@preview` package the cache lacks |

Render the quill with `quillmark render` before trusting a plate that
compiles here.

## Exit Codes

- `0`: success, `--help`, and `--version`
- `1`: the command ran and refused — an invalid quill, a file not found, a parse error, a compilation error, a failed `check` (any warning under `--strict`), or an argument value the command itself rejects (`-f docx`)
- `2`: usage error — an unknown flag, a missing argument, an unknown subcommand; argument parsing rejected the invocation before any command ran
