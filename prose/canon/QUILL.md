# Quill Resource File Structure and API

> **Implementation**: `crates/core/src/quill/` (the `Quill` type and its
> operations), `crates/quillmark/src/` (filesystem loading)

## TL;DR

A `Quill` is a loaded template bundle: file tree plus parsed `Quill.yaml`
config: tagged with its declared backend id but holding no backend and
needing no engine. It carries the pure config-read operations (`validate`,
`schema`, `blueprint`, `seed_*`, `compile_data`, `dry_run`); rendering is the
engine's job.

## The `Quill` type

One type models a loaded quill: **`Quill`** (in `quillmark-core`), declarative
data. It is the authored input (file bundle, parsed config, metadata) tagged
with its *declared* backend id, and it carries the pure config-read
operations (`validate`, `schema`, `metadata`, `blueprint`, `seed_*`,
`compile_data`, `dry_run`). It holds **no backend** and needs **no engine** to
construct or use; rendering is the engine's job (see
[ARCHITECTURE.md](ARCHITECTURE.md)). A `Quill` is `Send + Sync` and portable
across engines: any engine with a backend matching its `backend_id()` can
render it.

Bindings expose `Quill` directly.

A **quiver** is a collection of quills. The bundled fixtures under
`crates/fixtures/resources/quills/` are one quiver; the
[quill authoring contract](BLUEPRINT.md#guarantees) is verified across the
whole quiver by `every_quill_in_quiver_renders`
(`crates/quillmark/tests/quiver_test.rs`).

## Internal File Structure

```rust
pub enum FileTreeNode {
    File { contents: Vec<u8> },
    Directory { files: HashMap<String, FileTreeNode> },
}

pub struct Quill {
    pub(crate) metadata: HashMap<String, QuillValue>,
    pub(crate) config: QuillConfig,
    pub(crate) files: FileTreeNode,
}
```

`metadata` is populated from `Quill.yaml` fields plus computed entries: `backend`, `description`, `version`, `author`, and any `<backend>_*` keys (e.g. `typst_*`) from the top-level section named after `quill.backend`.

## In-memory Tree Contract (`Quill::from_tree`)

In-memory construction is `Quill::from_tree(tree)`: a pure constructor in
`quillmark-core` with no backend and no engine. Filesystem loading
(`quillmark::quill_from_path`) lives in `quillmark` rather than in core, so core
stays filesystem-agnostic. Input is a `FileTreeNode` directory tree with UTF-8
and binary file contents represented as bytes.

For JS/WASM consumers this is exposed as the static `Quill.fromTree(...)`
accepting a `Map<string, Uint8Array>` (path→bytes). Plain objects
(`Record<string, Uint8Array>`) are also accepted and walked via `Object.entries`
at the boundary.

Validation rules:
1. Root MUST be a directory node
2. `Quill.yaml` MUST exist and be valid YAML
3. File paths use `/` separators and are resolved relative to root

Core reads no backend-specific assets at load time. A backend resolves its own
inputs from the file bundle when it opens a session (the Typst backend reads its
`typst.plate_file`; the pdfform backend reads `form.pdf` / `form.json`), so a
missing or malformed template surfaces as a render-time error, not a load error.

## `Quill.yaml` Rules

One required top-level section, `quill` (bundle metadata); optional `main`
(document fields), `card_kinds` (card kind definitions), and a backend-named
section (`typst`). Top-level keys naming a backend are reserved, and the engine
reads exactly one: the section named by `quill.backend`. A section named for any
other backend is `quill::unknown_section`, the same refusal a typo gets. Every
key, type, and UI property is documented in the
[Quill.yaml reference](../../docs/quills/quill-yaml-reference.md); what follows
is what the engine enforces.

Field names must be `snake_case` (match `[a-z][a-z0-9_]*`). Capitalized or `$`-prefixed keys are rejected at config parse time with `quill::invalid_field_name`: document-level metadata sits on dedicated `$`-prefixed keys in the plate JSON (`$quill`, `$body`, `$cards`, `$kind`), and user fields stay lowercase so they cannot shadow it. Standalone `object` fields require a `properties` map. Every `array` field requires an `items:` element schema: use `items: { type: string }` (or `integer`, `richtext`, …) for a list of scalars, and `items: { type: object, properties: … }` for a list of objects.

Metadata resolution:
- `name`, `description`, `backend`, `version`, `author` are direct struct fields on `QuillConfig`. `description` (required, non-empty in the `quill:` section) describes the quill itself; it is independent of `QuillConfig.main.description`, which is the optional schema description authored under `main:` like any other card kind.
- `metadata` on `Quill` stores `backend`, `description`, `version`, `author`, and
  `typst_*` keys from the `typst:` section (so a declared `typst.plate_file`
  surfaces as `typst_plate_file`). This identity `metadata` is pure config: the
  backend's `supportedFormats` is a resolved-backend capability read from the
  engine, not part of it. The `quill:` section accepts only `name`, `backend`,
  `description`, `version`, `author`, and `ui`; unknown keys produce a
  `quill::unknown_key` error rather than landing in `metadata`. A backend's own
  settings (e.g. the Typst plate) live under the backend-named section, never in
  `quill:`.
- `quill.ui` (a `UiCardSchema`, same shape as `card_kinds.<name>.ui`) is a fallback for `main.ui`: the `main` card uses `main.ui` when present, otherwise `quill.ui`.

## Strict Parsing

`Quill.yaml` is parsed strictly: every problem the parser can detect is collected and reported in one pass as a `Vec<Diagnostic>`, rather than failing on the first error or silently dropping unsupported shapes. Specifically:

- Unknown keys in the `quill:` section error with `quill::unknown_key` (typos like `platefile` are not silently captured).
- Unknown top-level sections error with `quill::unknown_section` (typos like `card_kind:` are not silently ignored). Root-level `fields:` gets a targeted hint pointing to `main.fields:`.
- `main:` and each `card_kinds.<name>:` entry parse under one card-schema shape, which accepts `description`, `fields`, `ui`, and `body` only: a section that is not a mapping, an unknown key (`feilds:`), or a `fields:` that is not a mapping errors with `quill::invalid_card_schema` rather than loading as a card with no fields. A `card_kinds:` that is not a mapping errors with `quill::invalid_card_kinds`.
- Field schemas that fail to parse (e.g. a bare `title:`, missing `type:`) error with `quill::field_parse_error` and an actionable hint where applicable, rather than being dropped from the schema.
- `object` fields without a `properties` map error with `quill::object_missing_properties`; an empty `properties` map errors with `quill::object_empty_properties`.
- Malformed `quill.ui` / `main.ui` / `card_kinds.<name>.ui` blocks error with `quill::invalid_ui` rather than being silently discarded.
- Malformed `main.body` / `card_kinds.<name>.body` blocks error with `quill::invalid_body`.
- A `body.example` set together with `body.enabled: false` warns with `quill::body_example_unused` (the example has no effect).

Errors flow through `RenderError` (a non-empty `Vec<Diagnostic>`) and surface to bindings as a structured array (`err.diagnostics` in WASM, `.diagnostics` attribute in Python).

## File Ignore Rules

When loading from disk, `quillmark::quill_from_path` respects a `.quillignore` file at the bundle root. If absent, default patterns apply: `.git/`, `.gitignore`, `.quillignore`, `target/`, `node_modules/`.

A line is one of three rules. `dir/` drops that directory and its subtree, anchored at the bundle root. A line free of `*`, `?` and `[` is a literal name, matching the whole path or the basename at any depth. Anything else is a glob (`glob::Pattern`), matched against both the whole path and the basename, so a slash-free pattern applies at any depth and a slashed one anchors at the root; `*` stops at `/`. A glob also matches the line it was written as, since `[` is both a character-class opener and an ordinary character in a filename. A glob that fails to compile falls back to a literal name.

## API

Construction:
- `Quill::from_tree(tree)` (`quillmark-core`): pure in-memory constructor;
  returns `Result<Quill, Vec<Diagnostic>>`. Exposed to JS as `Quill.fromTree`.
- `quillmark::quill_from_path(path)`: load from a filesystem directory (fs walk
  lives in `quillmark`, not core); returns `Result<Quill, RenderError>`.

The two differ in error shape: the pure constructor hands back the raw
`Vec<Diagnostic>` and bindings map it at the call site, while the loader wraps
it in `RenderError`. Either way the `Quill` carries no backend: rendering goes
through the `Quillmark` engine (`engine.render` / `engine.open`).

`FileTreeNode` exposes the file and directory reads over the bundle. Paths use
forward slashes, the root is `""`, absolute paths and `..` traversal are
rejected, and every file reads back as `&[u8]`.
