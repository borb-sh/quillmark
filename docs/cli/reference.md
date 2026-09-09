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

The file must open with a `~~~` block containing a `$quill:` key identifying the quill. The `~~~card-yaml` opener is also accepted.

**Options:**

- `-o <PATH>` / `--output <PATH>`: Output file path (default: input filename with format extension, e.g. `input.pdf`; `example.<format>` when no markdown file is given)
- `-f <FORMAT>` / `--format <FORMAT>`: Output format: `pdf`, `svg`, `png` (default: `pdf`)
- `--output-data <DATA_FILE>`: Write compiled JSON data to a file
- `--quiet`: Suppress warnings and the output-destination line; errors still print
- `--stdout`: Write the artifact to stdout instead of a file (and ignore `-o`); refused when the render produces more than one page

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

Validate quill configuration and structure.

```bash
quillmark validate [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `-v` / `--verbose`: Show verbose output with all validation details

**Examples:**

```bash
# Validate quill structure
quillmark validate ./my-quill

# Verbose validation
quillmark validate ./my-quill -v
```

### info

Display a quill's identity and schema counts.

```bash
quillmark info <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Fields shown:** name, description (when nonempty), version, author, backend,
field count, and the card and defaults counts when nonzero.

**Examples:**

```bash
# Display quill info
quillmark info ./my-quill
```

## Exit Codes

- `0`: success, `--help`, and `--version`
- `1`: the command ran and refused — an invalid quill, a file not found, a parse error, a compilation error, or an argument value the command itself rejects (`-f docx`)
- `2`: usage error — an unknown flag, a missing argument, an unknown subcommand; argument parsing rejected the invocation before any command ran
