# CLI

> **Package**: `quillmark-cli` → binary `quillmark`
> **Implementation**: `crates/bindings/cli/`

## TL;DR

`quillmark-cli` is a `clap` surface over the engine holding no logic of its own:
`render` turns a quill + markdown into PDF/SVG/PNG, `check` reads documents
against a quill's schema, `validate` reads a quill's configuration and compiles
its plate, and `schema`/`blueprint`/`info` introspect a quill without rendering
it.
Commands, options, and examples are the
[CLI reference](../../docs/cli/reference.md); this page is the contract behind
them.

## Contract

- **`render` and `validate` need the engine.** Each constructs `Quillmark` to
  resolve the quill's backend; `check` / `schema` / `blueprint` / `info` load the
  quill with `quillmark::quill_from_path` and read the pure config-read
  operations a `Quill` already carries ([QUILL.md](QUILL.md)). `validate
  --no-render` is the config-read-only validate.
- **`check` is the document's verdict, `validate` the quill's.** `check` parses
  each `MARKDOWN_FILE` through the bound door (`Quill::parse`) and prints the
  parse warnings and every `Quill::validate` diagnostic, under the file's path.
  It compiles nothing, so a plate failure is `render`'s to find. A document that
  fails to parse draws its one parse error and does not stop the rest. An
  `Error` exits `1`; `--strict` exits `1` on a `Warning` too, the strict consumer
  [SCHEMAS.md](SCHEMAS.md#native-validation) names, for a CI gate over a
  repository of documents.
- **`render` prints what the page leaves out.** Its warnings are the parse
  carrier, then the `Quill::validate` warnings of the unclaimed class, then the
  compile's ([ERROR.md](ERROR.md#warning-flow)). The incomplete class,
  `validation::must_fill`, is a draft's normal state and condenses to one count
  line, naming the `check` that lists them where there is a file to check. The
  class is the code's ([SCHEMAS.md](SCHEMAS.md#what-blocks-a-render)), so the
  selection is one filter on `validation::must_fill`, not a list the CLI keeps.
- **`validate` compiles the plate.** It renders the three canonical documents —
  the empty document, the blueprint, the seed — through the quill's backend at
  the backend's first declared format, and reports each failure as a
  `cli::canonical_document_failed` error naming which of the three it is,
  followed by the backend's own diagnostics. This is where the quill authoring
  contract reaches a quill outside the fixtures quiver
  ([BLUEPRINT.md](BLUEPRINT.md) § "Guarantees"); a schema-only pass is
  `--no-render`. A backend that does not resolve is `cli::backend_unresolved`,
  and a configuration the read already refused is not compiled: each document
  would fail for the reason already named.
- **Seeded fallback.** `render` with no `MARKDOWN_FILE` renders the quill's
  seeded document: each field's `example:`, with `default:`/blank interpolated,
  so a quill renders with no input file. Output defaults to
  `example.{format}`.
- **Parsing is not relaxed for the CLI.** A `MARKDOWN_FILE` needs a root `~~~`
  block (the opener's info string is ignored) carrying a `$quill` line,
  exactly as every other surface requires.
- **Both backends by default.** The binary inherits `quillmark`'s default
  features, `typst` and `acroform`.
- **Every artifact reaches disk.** `svg` and `png` render one artifact per page,
  and a multi-page render writes `out-1.svg`, `out-2.svg`, …. `--stdout` carries
  one artifact and refuses such a render.
- **Two failure codes.** `clap` rejects an unparseable invocation — unknown
  flag, missing argument, unknown subcommand — with `2`, before any command
  runs. A command that ran and refused exits `1`. Success, `--help`, and
  `--version` exit `0`.
