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
- `[MARKDOWN_FILE]`: Path to markdown file with a root card-yaml block (optional — when omitted, the quill's seeded document is rendered, each field populated from its `example:` value, with `default:` used as fallback)

The file must open with a `~~~` block containing a `$quill:` key identifying the quill. The `~~~card-yaml` opener is also accepted.

**Options:**

- `-o <PATH>` / `--output <PATH>`: Output file path (default: input filename with format extension, e.g. `input.pdf`; `example.<format>` when no markdown file is given)
- `-f <FORMAT>` / `--format <FORMAT>`: Output format: `pdf`, `svg`, `png`, `txt` (default: `pdf`)
- `--output-data <DATA_FILE>`: Write compiled JSON data to a file
- `-v` / `--verbose`: Show detailed processing information
- `--quiet`: Suppress all non-error output
- `--stdout`: Write output to stdout instead of file

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
quillmark schema [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `-o <FILE>` / `--output <FILE>`: Output file (default: stdout)

**Examples:**

```bash
# Print schema to stdout
quillmark schema ./my-quill

# Save schema to file
quillmark schema ./my-quill -o schema.yaml
```

### blueprint

Print a quill's Markdown blueprint — an annotated document showing the quill's fields, constraints, and examples, itself a valid document an author can fill in.

```bash
quillmark blueprint [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `-o <FILE>` / `--output <FILE>`: Output file (default: stdout)

**Examples:**

```bash
# Print blueprint to stdout
quillmark blueprint ./my-quill

# Save blueprint to file
quillmark blueprint ./my-quill -o blueprint.md
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

Display metadata and information about a quill.

```bash
quillmark info [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `--json`: Output as machine-readable JSON instead of human-readable format

**Examples:**

```bash
# Display quill info
quillmark info ./my-quill

# Output as JSON
quillmark info ./my-quill --json
```

## Exit Codes

- `0` — success
- `1` — error (invalid arguments, file not found, parse error, compilation error, etc.)
