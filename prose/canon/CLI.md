# CLI

> **Package**: `quillmark-cli` → binary `quillmark`
> **Implementation**: `crates/bindings/cli/`

## TL;DR

`quillmark-cli` is a `clap` surface over the engine holding no logic of its own:
`render` turns a quill + markdown into PDF/SVG/PNG, `check` reads documents
against a quill's schema, `validate` reads a quill's configuration and compiles
its plate, `schema`/`blueprint`/`info` introspect a quill without rendering
it, and `workspace` exports what Typst's own tooling needs to compile a plate.
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
  It compiles nothing, so a plate failure is `render`'s to find; a construct
  the backend declines draws `validation::declined_construct`. A document that fails to read or parse draws that one
  error (`cli::unreadable_document` for a path that is missing or not UTF-8)
  and does not stop the rest. An
  `Error` exits `1`; `--strict` exits `1` on a `Warning` too, for a CI gate over
  a repository of documents.
- **`render` prints what the page leaves out.** Its warnings are the parse
  carrier, then every `Quill::validate` warning, each naming unclaimed input
  ([SCHEMAS.md](SCHEMAS.md#what-blocks-a-render)) less
  `validation::declined_construct`, then the compile's
  ([ERROR.md](ERROR.md#warning-flow)). The engine's one-shot render carries
  the last two; the CLI adds the parse carrier. A render that fails returns no
  result, so the CLI prints the parse carrier and every `Quill::validate`
  warning itself, `validation::declined_construct` among them, ahead of the
  error. A failed compile carries the backend's load warnings after its errors
  ([ERROR.md](ERROR.md#warning-flow)), and the CLI prints them third.
- **`validate` compiles the plate.** It renders the three canonical documents —
  the empty document, the blueprint, the seeded document — through the quill's
  backend at the backend's first declared format, and reports each failure as a
  `cli::canonical_document_failed` error naming which of the three it is,
  followed by the backend's own diagnostics. This is where the quill authoring
  contract reaches a quill outside the fixtures quiver
  ([BLUEPRINT.md](BLUEPRINT.md) § "Guarantees"); a schema-only pass is
  `--no-render`. A backend that does not resolve is `cli::backend_unresolved`,
  and a configuration the read already refused is not compiled: each document
  would fail for the reason already named.
- **`validate` holds the example to no diagnostic.** A quill with an
  `example.md` at its root has it read through `Quill::example_document`,
  checked by `Quill::validate`, and, unless `--no-render`, rendered after the
  canonical documents. Any diagnostic on it fails the quill, a warning
  included: one `cli::example_not_clean` error names the file and the count,
  and each diagnostic follows at `Error` severity. A render warning a canonical
  document raised too is the plate's, and does not count against the example.
  A render that fails adds `cli::example_render_failed`. `--verbose` names
  whether the quill has an example, so a misspelled one reads as none.
- **The render date is the local date.** `render`, `validate` and `workspace` supply it
  to the engine, which reads no clock; `--today YYYY-MM-DD` on `render` or
  `workspace` pins it for a reproducible render. The local offset unreadable,
  the date is UTC's.
- **`workspace` hands the plate to Typst's tooling.** It writes
  `quillmark_typst::workspace`'s files under `-o` and prints the `typst watch`
  command over them: the quill as `--root`, the generated helper and vendored
  packages as `--package-path`, the backend's fonts as `--font-path` with
  system and embedded fonts ignored, the render date as
  `--creation-timestamp`, and the PDF written under `-o`. The timestamp is
  noon UTC, since Typst reads a fixed timestamp's UTC date for
  `datetime.today()`. The helper is one document's data, so the plate
  recompiles live and the document does not. An `-o` inside the quill is
  refused, since the quill would load it as its own files. A rerun replaces the
  `packages/` and `fonts/` an earlier export wrote, and refuses an `-o` holding
  either without the helper package.
- **The export does not vouch for a render.** `workspace` prints the warnings
  a render raises before it compiles: the parse carrier, every
  `Quill::validate` warning, and the backend's load warnings `Workspace`
  carries. Typst serves the plate any file under the root and fetches a
  package the quill does not vendor, both of which a render refuses; the
  [CLI reference](../../docs/cli/reference.md#workspace) tabulates the two.
- **Seeded fallback.** `render` or `workspace` with no `MARKDOWN_FILE` reads the quill's
  seeded document: one card per kind carrying its kind's `seed:`, every
  other field at its `default:`/blank, so a quill renders with no input file. `render`'s output
  defaults to `example.{format}`.
- **Parsing is not relaxed for the CLI.** A `MARKDOWN_FILE` needs a root `~~~`
  block (the opener's info string is ignored) carrying a `$quill` line,
  exactly as every other surface requires.
- **Both backends by default.** The binary inherits `quillmark`'s default
  features, `typst` and `acroform`.
- **Every artifact reaches disk.** `svg` and `png` render one artifact per page,
  and a multi-page render writes `out-1.svg`, `out-2.svg`, …. `--stdout` carries
  one artifact and refuses such a render.
- **`-o` and `-f` agree.** An `-o` extension naming a format supplies an
  omitted `-f`, and one naming another format refuses the render before any
  file is written.
- **Two failure codes.** `clap` rejects an unparseable invocation — unknown
  flag, missing argument, unknown subcommand — with `2`, before any command
  runs. A command that ran and refused exits `1`. Success, `--help`, and
  `--version` exit `0`.
