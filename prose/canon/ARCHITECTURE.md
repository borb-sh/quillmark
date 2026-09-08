# Quillmark Architecture

> **Implementation**: `crates/` (workspace overview)

## TL;DR

Quillmark is a schema-driven document engine: it turns Markdown with card-yaml
blocks into a fully typeset document (PDF, SVG, PNG). A `Quill` is declarative
data: no backend, engine, or filesystem needed to construct or read it. Its
schema drives validation and scaffolding (parse / validate / schema / seed /
blueprint / compile). The `Quillmark` engine is the thin-but-mandatory core
every render routes through: a backend registry + render dispatcher. Backends
do the heavy compilation.

## Data Flow

1. **Parse**: card-yaml block extraction, bidi stripping, line-separator spacing, HTML fence normalization
2. **Normalize**: Type coercion, schema defaults, field validation
3. **Compile**: Backend's `open()` receives the quill + JSON data and returns a `LiveSession`; `LiveSession::render()` produces artifacts

## Crate Structure

### `quillmark-core`

Foundation types and traits: the render contract (`Backend` / `LiveSession`),
the `Document` and `Quill` model, and the `Diagnostic` currency; see the
crate's rustdoc for the full surface, and [Core Interfaces](#core-interfaces)
below for the ones carrying the contract. It depends on `quillmark-content` (the leaf rich-text
primitive one layer below it) and on no backend; backends depend on it.

### `quillmark-content`

The leaf rich-text primitive `quillmark-core` depends on: the `Content` content
model (one USV text with line attributes, anchored marks, embedded islands), its
canonical byte-deterministic serialization (the frozen wire form storage, the
render seam, and the binding seam carry, the last spelling a zero
`Container.instance` the other two omit), the markdown⇄content import/export
codecs, and edit deltas.
The workspace's only markdown parser (`pulldown-cmark`) lives here, in
`quillmark-content::import`, run once at ingest. **Invariant:** the markdown
engine appears exactly once in the workspace; no render path parses markdown:
backends lower the content.

### `quillmark` (orchestration)

The `Quillmark` engine plus the `quill_from_path` loader; re-exports core's
`Quill`. Filesystem walking lives here, so core stays filesystem-agnostic:
in-memory loading is `Quill::from_tree` in core.

### `backends/quillmark-typst`

Implements `Backend` for PDF, SVG, and PNG. Lowers each content field's `Content` value to Typst markup at codegen (`emit::emit_content`), recording a per-segment source map. Resolves fonts and assets. See [CONVERT.md](CONVERT.md) and [PLATE_DATA.md](PLATE_DATA.md).

### `backends/quillmark-pdfform`

The second backend: fills an existing AcroForm PDF rather than typesetting from
scratch. It resolves card values against the quill's `form.json` spec and stamps
them onto the base `form.pdf` as real interactive fields (Technique A:
`NeedAppearances`, no baked appearance streams).

The PDF deliverable is always an interactive AcroForm, the one output format
this backend emits. It paints a WASM canvas raster by pre-flattening values into
the page content streams and rasterizing that with hayro. Field geometry is a
session-level query (`LiveSession::regions()`): per-field geometry keyed on the
schema field path, no bound value. Quill-authoring surface:
[docs/quills/pdfform-backend.md](../../docs/quills/pdfform-backend.md); preview
seam: [PREVIEW.md](PREVIEW.md).

### `quillmark-pdf`

The shared PDF stamp spine: Typst-free, `pdf-writer`-only leaf infrastructure
consumed by `quillmark-pdfform`. A minimal byte-level reader plus a single
incremental-update appender that splices a fresh `/AcroForm` (and `/Info`
`/Producer` stamp) onto a base PDF. Deliberately small: it hard-errors on
out-of-contract input rather than parsing the full format; `reader`'s module
docs carry that input contract.

### `bindings/*`

Language surfaces over the one core engine: `quillmark-python` (PyO3, PyPI),
`quillmark-wasm` (wasm-bindgen, npm), and `quillmark-cli` (the `quillmark`
binary). See [BINDINGS.md](BINDINGS.md).

### `quillmark-fixtures`

Test resources under `resources/`. Helper functions for test setup.

### `quillmark-fuzz`

Property-based fuzz tests (proptest) over the boundaries that take arbitrary
input. Per-target coverage:
[crates/fuzz/README.md](../../crates/fuzz/README.md).

## Core Interfaces

- **`Quillmark`**, Engine: a backend registry + render dispatcher. Auto-registers one backend per enabled feature (`TypstBackend` under `typst`, `PdfformBackend` under `pdfform`; both are default). Resolves a quill's declared backend at render time (erroring `engine::backend_not_found` on no match) and owns the backend-dependent surface: `render`, `open`, `supported_formats(&quill)`. It does not construct quills.
- **`Quill`**, The single quill type in `quillmark-core`: declarative data (file bundle + config + metadata, tagged with a declared backend id), held by value and carrying the pure config-read operations (`validate`, `schema`, `blueprint`, `seed_*`, `compile_data`, `dry_run`). Construct with `Quill::from_tree` or `quillmark::quill_from_path`; see [QUILL.md](QUILL.md)
- **`Backend`**, Trait for output formats (`Send + Sync`): `id()`, `supported_formats()`, `open(&Quill, json)`. There is no universal template input: a backend reads whatever static inputs it needs (a Typst plate, a `form.pdf`) from the quill's own files. No canvas-capability method: capability is derived from the session seam, as `LiveSession::supports_canvas()`
- **`LiveSession`**, Opaque live session returned by `Backend::open()`: a persistent compiler whose reads serve its current compile and whose `update(&Document)` recompiles in place, transactionally, returning a `ChangeSet` of dirty pages. Born bound to the `QuillConfig` it was opened against, so the edit verb checks the `$quill` pairing and compiles through the same door as the first compile (`QuillConfig::compile_checked`) rather than trusting a caller to have done both. The canvas seam lives on `SessionHandle` (`page_size_pt`/`render_rgba`), so a canvas backend overrides two methods and the WASM painter dispatches generically; see [PREVIEW.md](PREVIEW.md)
- **`Document`**: Typed in-memory representation of a Quillmark Markdown file (root block, body, cards). Serializes via `serde` to a versioned JSON envelope (`StoredDocument`) for database persistence, decoupled from the evolving Markdown syntax; see [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)
- **`Diagnostic`**: Structured error with severity, code, message, location, hint, source chain
- **`RenderResult`**: Output artifacts + accumulated warnings

## Data Injection

`Backend::open()` receives:
- `source`: `&Quill` with static assets/packages, config, metadata. A backend reads its own inputs from here: the Typst backend reads the template named by `typst.plate_file` from `source.files()`; pdfform reads `form.pdf` / `form.json`
- `json_data`: JSON object after coercion, defaults, normalization

See [PLATE_DATA.md](PLATE_DATA.md) for the Typst helper package.

## Backend Implementation

Backends are an in-workspace seam, not an extension point. `Quillmark::new`
registers one per enabled cargo feature and nothing else registers one:
`register_backend` is private, and the `LiveSession` `Backend::open` returns is
built only from a `#[doc(hidden)]` `SessionHandle`.

Two `pub` seams exist for the workspace rather than for a crates.io consumer:

| Seam | Who it is for |
|---|---|
| `Backend` + `SessionHandle` | The workspace's own backends; an implementation outside it has no way to reach the registry. |
| `quillmark_typst::emit` | `quillmark-fuzz`, which drives the escapers directly. |

A quill declares one backend and renders through that one. Rendering a schema
two ways is therefore two quills, with nothing keeping their field definitions
in agreement.

Implement the `Backend` trait and return a `LiveSession` wrapping a
`SessionHandle` that does the format-specific rendering; to paint to a canvas,
override that handle's `page_size_pt` / `render_rgba`. See
`backends/quillmark-typst` for the reference implementation.
