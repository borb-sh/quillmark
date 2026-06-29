# CLI

> **Package**: `quillmark-cli` → binary `quillmark`
> **Implementation**: `crates/bindings/cli/src/`

## Commands

### `render`

```
quillmark render [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]
```

`QUILL_PATH` provides the local quill bundle used for rendering. `MARKDOWN_FILE` requires a root bare `~~~` block (`~~~card-yaml` is also accepted) with a `$quill` system-metadata line because parsing enforces it.

When `MARKDOWN_FILE` is omitted, the quill's seeded document is rendered instead (each field's `example:` with `default:`/zero interpolated), so the quill renders with no input file. Output defaults to `example.{format}`.

Options:
- `-o, --output <FILE>` — output file path (default: derived from input filename)
- `-f, --format <FORMAT>` — `pdf`, `svg`, `png`, or `txt` (default: `pdf`)
- `--stdout` — write output to stdout
- `--output-data <DATA_FILE>` — write compiled JSON data to file
- `-v, --verbose` — detailed processing output
- `--quiet` — suppress non-error output

### `schema`

```
quillmark schema <QUILL_PATH> [-o <FILE>]
```

Outputs the Quill's public schema contract as YAML to stdout or file.

### `blueprint`

```
quillmark blueprint <QUILL_PATH> [-o <FILE>]
```

Outputs the Quill's annotated Markdown blueprint (see [BLUEPRINT.md](BLUEPRINT.md)) to stdout or file.

### `validate`

```
quillmark validate [OPTIONS] <QUILL_PATH>
```

Validates quill configuration.

Options:
- `-v, --verbose` — show all validation details including warnings

### `info`

```
quillmark info <QUILL_PATH> [--json]
```

Displays quill metadata: name, version, author, description, backend, field/card counts, defaults count, has-plate status, and any non-standard metadata keys. `--json` emits the same fields plus `has_plate` and a `metadata` object for non-standard keys. Standard keys (`backend`, `version`, `author`, `description`) are excluded from the metadata section.

## Project Structure

```
crates/bindings/cli/src/
├── main.rs
├── commands/
│   ├── mod.rs
│   ├── info.rs
│   ├── render.rs
│   ├── schema.rs
│   ├── blueprint.rs
│   └── validate.rs
├── output.rs
└── errors.rs
```

## Dependencies

- `clap` — argument parsing
- `quillmark` — the engine
- `quillmark-typst` — Typst backend
- `quillmark-core` — types
- `serde_json` — JSON output
