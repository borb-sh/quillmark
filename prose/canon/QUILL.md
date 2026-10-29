# Quill Resource File Structure and API

> **Implementation**: `crates/core/src/quill/` (the `Quill` type and its
> operations), `crates/quillmark/src/` (filesystem loading)

## TL;DR

A `Quill` is a loaded quill, the document format as data: its file tree plus
its parsed `Quill.yaml` config, tagged with its declared backend id but holding
no backend and needing no engine. It carries the pure config-read operations (`validate`,
`schema`, `seed_*`, `example_document`, `to_authoring`/`from_authoring`, `compile_data`,
`dry_run`); rendering is the engine's job.

## The `Quill` type

One type models a loaded quill: **`Quill`** (in `quillmark-core`), declarative
data. It is the authored input (file bundle, parsed config) tagged
with its *declared* backend id, and it carries the pure config-read
operations (`validate`, `schema`, `seed_*`, `example_document`, `to_authoring`/`from_authoring`,
`compile_data`, `dry_run`). It holds **no backend** and needs **no engine** to
construct or use; rendering is the engine's job (see
[ARCHITECTURE.md](ARCHITECTURE.md)). A `Quill` is `Send + Sync` and portable
across engines: any engine with a backend matching its `backend_id()` can
render it.

Bindings expose `Quill` directly.

A **quiver** is a collection of quills. The bundled fixtures under
`crates/fixtures/resources/quills/` are one quiver; the
[plate contract](#the-plate-contract) is verified across the
whole quiver by the sweeps in `crates/quillmark/tests/quiver_test.rs`, and over
one quill anywhere by `quillmark validate` ([CLI.md](CLI.md)).

## Internal File Structure

```rust
pub enum FileTreeNode {
    File { contents: Vec<u8> },
    Directory { files: HashMap<String, FileTreeNode> },
}

pub struct Quill {
    pub(crate) config: QuillConfig,
    pub(crate) files: FileTreeNode,
    pub(crate) warnings: Vec<Diagnostic>,
}
```

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
2. `Quill.yaml` MUST exist and be valid YAML, read as a card-yaml payload is:
   only `true` and `false` are booleans
   ([markdown-spec.md](../references/markdown-spec.md) §3.4)
3. File paths use `/` separators and are resolved relative to root

Core reads no backend-specific assets at load time. A backend resolves its own
inputs from the file bundle when it opens a session (the Typst backend reads
`plate.typ` at the quill root, or the file `typst.plate_file` names; the
acroform backend reads `form.pdf` / `Form.yaml`), so a missing or malformed
template surfaces as a render-time error, not a load error.

## The `.quill` bundle

A quill that travels as bytes is a `.quill` bundle: its file tree as one ZIP archive, `Quill.yaml` at the archive root. The format, its budget and its refusals are specified in [quill-bundle.md](../references/quill-bundle.md). A bundle is the tree and nothing else, so a quill that shares nothing with another quill is one file wherever it goes, and two runtimes holding the same bundle hold the same quill.

The codec lives in `quillmark-core`, beside `from_tree`, so the WASM core build, the Python wheel and the CLI read and write it through one implementation. Its deflate and CRC-32 crates were already in the render build through Typst; the core artifact is what pays for them.

## `Quill.yaml` Rules

One required top-level section, `quill` (bundle metadata); optional `main`
(document fields), `card_kinds` (card kind definitions), and a backend-named
section (`typst`). Top-level keys naming a backend are reserved, and Quillmark
reads exactly one: the section named by `quill.backend`. A section named for any
other backend is `quill::unknown_section`, the same refusal a typo gets. Every
key, type, and UI property is documented in the
[Quill.yaml reference](../../docs/quills/quill-yaml-reference.md); what follows
is what Quillmark enforces.

Field names must be `snake_case` (match `[a-z][a-z0-9_]*`). Capitalized or `$`-prefixed keys are rejected at config parse time with `quill::invalid_field_name`: document-level metadata sits on dedicated `$`-prefixed keys in the plate JSON (`$quill`, `$body`, `$cards`, `$kind`), and user fields stay lowercase so they cannot shadow it.

A field may not be named `body`: it is the body terminal of every card's `DocPath` (`main.body`, `cards.<kind>[i].body`), so the name is refused with `quill::reserved_field_name`. Standalone `object` fields require a `properties` map. Every `array` field requires an `items:` element schema: use `items: { type: text }` (or `integer`, `richtext`, …) for a list of scalars, and `items: { type: object, properties: … }` for a list of objects.

Identity resolution:
- `name`, `description`, `backend`, `version`, `author`, `license`, `homepage` are direct struct fields on `QuillConfig`. `description` (required, non-empty in the `quill:` section) describes the quill itself and is a Typst PDF's title when neither the render's `title` nor the plate's `set document(title: ..)` names one; it is independent of `QuillConfig.main.description`, which is the optional schema description authored under `main:` like any other card kind.
- `license` and `homepage` are optional package metadata, `None` when absent,
  and omitting them is never a diagnostic. Neither reaches the plate or the
  authoring schema; both surface on `quill.metadata` and `quillmark info`.
- `license` is one line holding an SPDX license expression, stored trimmed:
  each id on the SPDX license list the `spdx` crate ships, spelled in the
  list's case, and `AND`, `OR`, `WITH` uppercase. Any other value is
  `quill::invalid_license`, `NOASSERTION` and an empty `LicenseRef-` included.
- A deprecated id loads with `quill::deprecated_license`. The list rides a
  caret dependency, so a deprecation arriving in a patch release cannot refuse
  on one build a quill another loads.
- `homepage` is an absolute `http` or `https` URL naming a host and no user
  name, its port, IPv6 brackets and host code points held to what a WHATWG URL
  parser accepts; a Unicode host is not run through IDNA. Any other value is
  `quill::invalid_homepage`.
- The `quill:` section accepts only `name`, `backend`, `description`, `version`,
  `author`, `license`, `homepage`, `ui`, and `requires`; an unknown key is a
  `quill::unknown_key` error. A backend's own settings (e.g. the Typst plate)
  live under the backend-named section, never in `quill:`, and reach a backend
  as `QuillConfig::backend_config`.
- `quill.requires` (`">=MAJOR.MINOR[.PATCH]"`) is a floor on Quillmark's
  crate version. It is read before anything else in the file, and a floor above
  the running Quillmark is the load's only error, `quill::requires_unmet`:
  strict parsing would otherwise report a newer Quillmark's keys as typos. The
  floor is checked and not stored on `QuillConfig`. Quillmark's version is its
  release with any prerelease suffix dropped, so a release candidate meets its
  own release's floor, and a build of `main` carries the last released version
  until the release bump.
- `quill.ui` (a `UiCardSchema`, same shape as `card_kinds.<name>.ui`) is a fallback for `main.ui`: the `main` card uses `main.ui` when present, otherwise `quill.ui`.

## The example document

The quill's example is derived from its `Quill.yaml`: the `example:` its slots
declare and the `body.example` its kinds declare, committed over the seeded
document by `Quill::example_document`
([SCHEMAS.md](SCHEMAS.md) § "The example document"). It is a filled-in page for
judging the quill. Its values are made up and nobody keeps them; starter content
someone keeps is a template. A root `example.md` is an ordinary file.

- **Checked at load, judged by `quillmark validate`.** Each example is checked
  where it is declared, as a `default:` is, so a quill whose examples do not
  fit its schema fails to load. `validate` fails the quill on any diagnostic
  the composed document carries, a warning included ([CLI.md](CLI.md)).
- **Pinned as the seed is.** Its `$quill` is `name@MAJOR.MINOR`, the seeded
  document's. `None` means the quill declares no example the document would
  commit.

A consumer takes the example from the `Quill` it holds.

## The plate contract

A quill's plate MUST render the **empty document** (`$quill` and `$kind: main`,
no fields) to a successful output. Under blank-filled render every absent field
is filled with its blank ([SCHEMAS.md](SCHEMAS.md#blank-filled-render)), so the
empty document is by construction the type-minimal valid input. A plate that
renders it degrades gracefully on every type-valid input shape.

The contract requires:

- A plate treats blanks (`""`, `0`, `false`, `[]`, an empty body) as valid
  *present* input: read via `qm.data.field`, `card.at("field", default: …)`, or
  guarded with `if "field" in qm.data`.
- **A plate branching on an `enum` covers `values ∪ blank` exhaustively.** The
  blank is valid present input for every enum, so an `else` fallback silently
  renders a member nobody chose. Match the blank and decide what it means.
  `qm.data.at(key, default: X)` is **not** such a guard: every declared key is
  present, so its `default:` is dead code and the blank flows through.
- **A plate reads a variant field only inside the branch that selects its
  world.** A variant-bearing enum arrives as `{value: …}` carrying exactly the
  live world's fields ([SCHEMAS.md](SCHEMAS.md#enum-variants)).
- No plate asserts that a defaultless field is *non-empty*. The schema
  guarantees presence, not non-emptiness.
- "Renders" means "compiles without error", not "produces meaningful output".

The fixture sweeps in `quiver_test.rs` render each bundled quill's empty and
seeded documents. A quill outside that directory reaches the contract through
`quillmark validate`, which renders both and reports each failure as a
`cli::canonical_document_failed` error naming the document ([CLI.md](CLI.md)).

## Strict Parsing

`Quill.yaml` is parsed strictly: every problem the parser can detect is collected and reported in one pass as a `Vec<Diagnostic>`, rather than failing on the first error or silently dropping unsupported shapes. The exception is `quill::requires_unmet`, which is reported alone. Specifically:

- Unknown keys in the `quill:` section error with `quill::unknown_key` (typos like `platefile` are not silently captured).
- Unknown top-level sections error with `quill::unknown_section` (typos like `card_kind:` are not silently ignored). Root-level `fields:` gets a targeted hint pointing to `main.fields:`.
- The backend-named section is a mapping of the keys its backend reads, the table `quillmark_core::backend::config_keys` holds per backend id. Any other key errors with `quill::unknown_backend_key`, its hint listing the valid keys, and a scalar or sequence section with `quill::invalid_backend_section`; an empty section reads as no keys. A backend id the table does not name loads its section unchecked: the render refuses the id itself.
- `main:` and each `card_kinds.<name>:` entry parse under one card-schema shape, which accepts `title`, `description`, `fields`, `ui`, `body`, and `seed` only (`seed` on a card kind alone, [SCHEMAS.md](SCHEMAS.md#kind-seeds-and-document-overlays)): an unknown key (`feilds:`, or `sections:`, whose hint names the top-level `object` per [SCHEMAS.md](SCHEMAS.md#grouping)), or a `fields:` that is not a mapping errors with `quill::invalid_card_schema` rather than loading as a card with no fields. A `card_kinds:` that is not a mapping errors with `quill::invalid_card_kinds`.
- Field schemas that fail to parse (e.g. a missing `type:`, or `ui.title`, whose hint names the top-level `title:`) error with `quill::field_parse_error` and an actionable hint where applicable, rather than being dropped from the schema.
- A `title` is a literal: a `{field}` token in one errors with `quill::title_template`, and a `title` on an array's `items` with `quill::title_on_items`.
- `object` fields without a `properties` map error with `quill::object_missing_properties`; an empty `properties` map errors with `quill::object_empty_properties`.
- Malformed `quill.ui` / `main.ui` / `card_kinds.<name>.ui` blocks error with `quill::invalid_ui` rather than being silently discarded; one spelling `title` is hinted to the card's own `title:`.
- A `ui.layout: table` column that is not a leaf errors with `quill::table_column_not_leaf`, naming the column: declaring the key contracts the shape the control needs, leaving an editor only the capability decline ([SCHEMAS.md](SCHEMAS.md#schema-emission)).
- Malformed `main.body` / `card_kinds.<name>.body` blocks, `enabled` being the one key, error with `quill::invalid_body`.
- A card declaring more than `MAX_FIELD_COUNT` (1000) fields errors with `quill::too_many_fields`: seeding builds one card-yaml block per card schema, so the block's cap is the schema's to meet. Counted per card over declared fields alone — nested `properties`, array `items`, and `variants:` cells ride inside the field declaring them.

Errors flow through `RenderError` (a non-empty `Vec<Diagnostic>`) and surface to bindings as a structured array (`err.diagnostics` in WASM, `.diagnostics` attribute in Python).

## File Ignore Rules

`QuillIgnore` is the fixed set a filesystem walk skips; a bundle does not configure it. `.git/`, `target/` and `node_modules/` drop with their subtrees, anchored at the bundle root, so a vendored package keeps its own. `.gitignore` drops wherever it sits. Everything else the walk meets enters the tree.

## API

Construction:
- `Quill::from_tree(tree)` (`quillmark-core`): pure in-memory constructor;
  returns `Result<Quill, Vec<Diagnostic>>`. Exposed to JS as `Quill.fromTree`.
- `Quill::from_bundle(bytes)` (`quillmark-core`): the same, over a `.quill`
  bundle; `quill.to_bundle()` writes one. Exposed to JS as `Quill.fromBundle` /
  `quill.toBundle()`, to Python as `Quill.from_bundle` / `quill.to_bundle()`.
- `quillmark::quill_from_path(path)`: load from a filesystem directory (fs walk
  lives in `quillmark`, not core), or from a file named `*.quill` as a bundle;
  returns `Result<Quill, RenderError>`.

The two differ in error shape: the pure constructor hands back the raw
`Vec<Diagnostic>` and bindings map it at the call site, while the loader wraps
it in `RenderError`. Either way the `Quill` carries no backend: rendering goes
through the `Engine` (`engine.render` / `engine.open`).

Advisory diagnostics ride the loaded quill rather than the constructor's return
value, so both doors keep them and a host reads them when it likes:
`Quill::warnings()`, `quill.warnings` in WASM and Python.

`FileTreeNode` exposes the file and directory reads over the bundle. Paths use
forward slashes, the root is `""`, absolute paths and `..` traversal are
rejected, and every file reads back as `&[u8]`.
