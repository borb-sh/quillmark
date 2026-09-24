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
- `[MARKDOWN_FILE]`: Path to markdown file with a root card-yaml block (optional, when omitted, the quill's seeded document is rendered, each field populated from its `example:` value, with `default:` used as fallback)

The file must open with a `~~~` block containing a `$quill:` key identifying the quill; the opener's info string is ignored.

**Options:**

- `-o <PATH>` / `--output <PATH>`: Output file path (default: input filename with format extension, e.g. `input.pdf`; `example.<format>` when no markdown file is given)
- `-f <FORMAT>` / `--format <FORMAT>`: Output format: `pdf`, `svg`, `png` (default: the `-o` extension when it names one of these, else `pdf`). A `-f` that disagrees with such an extension is refused (`-f png -o out.pdf`); an `-o` extension naming no format is written as given.
- `--output-data <DATA_FILE>`: Write the compiled, blank-filled JSON data to a file. This is the data before the backend lowers it: a `richtext` value appears as a content object (`{text, lines, marks, islands}`) and a date as its string, where a Typst plate receives content and a `datetime`.
- `--quiet`: Suppress warnings and the output-destination line; errors still print
- `--stdout`: Write the artifact to stdout instead of a file (and ignore `-o`); refused when the render produces more than one page

**Warnings:** `render` prints the parse warnings, then each warning for input the page leaves out — an undeclared key (`validation::unknown_field`), a card no kind claims, a body under `body.enabled: false`, a stranded variant cell, elements past `max:` — then the backend's. Fields the document has yet to answer (`validation::must_fill`) are a draft's normal state, so they print as one line counting them; `quillmark check` lists each. A render of the seeded document counts none: its blanks are the quill's.

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

Check markdown documents against a quill's schema, printing every diagnostic each one draws: parse warnings, then every `validation::*` diagnostic, including the unanswered fields `render` only counts. It does not compile the plate, so a plate failure or a construct the backend declines (`backend::declined_construct`) is `render`'s to report.

```bash
quillmark check [OPTIONS] <QUILL_PATH> <MARKDOWN_FILE>...
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory
- `<MARKDOWN_FILE>...`: One or more documents to check. Each document's diagnostics print under its path, and a document that fails to read or parse does not stop the rest.

**Options:**

- `--strict`: Exit `1` on any warning, not only on an error. An unanswered field, an undeclared key, and a card no kind claims all fail a strict check.

Without `--strict`, `check` exits `1` only on an error: a parse error, a value that is not its field's type, a `$quill` naming another quill.

**Examples:**

```bash
# Every diagnostic for one document
quillmark check ./my-quill input.md

# CI gate: fail on anything unfinished or unread
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

Print a quill's Markdown blueprint: an annotated document showing the quill's fields, constraints, and examples, itself a valid document an author can fill in.

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
three canonical documents: the empty document, the blueprint, and the seed. A
plate that renders its seed and not the empty document fails here, which is the
quill authoring contract every plate is bound by.

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

## Exit Codes

- `0`: success, `--help`, and `--version`
- `1`: the command ran and refused — an invalid quill, a file not found, a parse error, a compilation error, a failed `check` (any warning under `--strict`), or an argument value the command itself rejects (`-f docx`)
- `2`: usage error — an unknown flag, a missing argument, an unknown subcommand; argument parsing rejected the invocation before any command ran
