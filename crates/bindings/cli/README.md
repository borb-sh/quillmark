# Quillmark CLI

`quillmark`, the command-line interface to [Quillmark](https://github.com/borb-sh/quillmark): renders Markdown with card-yaml blocks through a quill into PDF, SVG, or PNG.

Maintained by [borb-sh](https://github.com/borb-sh).

## Installation

Prebuilt binaries are on each [GitHub Release](https://github.com/borb-sh/quillmark/releases), and `cargo install quillmark-cli --locked` builds from source. The [CLI reference](https://quillmark.dev/cli/reference/#installation) has the other routes and what `quillmark --version` prints.

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

Each command and its options are in the [CLI reference](https://quillmark.dev/cli/reference/):

- [`render`](https://quillmark.dev/cli/reference/#render): a document through a quill into PDF, SVG or PNG
- [`check`](https://quillmark.dev/cli/reference/#check): a document's diagnostics, optionally as a CI gate
- [`schema`](https://quillmark.dev/cli/reference/#schema): the quill's field schema, or the JSON Schema an LLM writes against
- [`validate`](https://quillmark.dev/cli/reference/#validate): the quill itself, loaded and rendered
- [`info`](https://quillmark.dev/cli/reference/#info): the quill's identity and counts
- [`workspace`](https://quillmark.dev/cli/reference/#workspace): the files Typst's tooling needs to compile a plate
- [`pack`](https://quillmark.dev/cli/reference/#pack): the quill as one `.quill` bundle

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
