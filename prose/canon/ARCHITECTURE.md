# Quillmark Architecture

> **Implementation**: `crates/` (workspace overview)

## TL;DR

Quillmark converts Markdown with card-yaml blocks into output artifacts (PDF, SVG, PNG, TXT). A `Quill` is portable, engine-free data (parse / validate / schema / seed / blueprint / compile); the `Quillmark` engine is a backend registry + render dispatcher; backends do the heavy compilation.

## Data Flow

1. **Parse** — card-yaml block extraction, bidi stripping, HTML fence normalization
2. **Normalize** — Type coercion, schema defaults, field validation
3. **Compile** — Backend's `open()` receives plate + JSON data and returns a `RenderSession`; `RenderSession::render()` produces artifacts

## Crate Structure

### `quillmark-core`

Foundation types and traits. No backend dependencies; backends depend on this crate.

Key exports: `Backend`, `Artifact`, `OutputFormat`, `RenderOptions`, `RenderSession`, `Document`, `Quill`, `FileTreeNode`, `QuillIgnore`, `RenderError`, `Diagnostic`, `Severity`, `Location`, `RenderResult`, `QuillValue`, `QuillReference`, `Version`, `VersionSelector`. `Quill` is the single quill type — portable, validated data with the pure config-read operations (`validate`, `schema`, `metadata`, `blueprint`, `seed_*`, `compile_data`, `dry_run`); construct it with `Quill::from_tree`.

### `quillmark` (orchestration)

High-level API: `Quillmark` (the engine — a backend registry + render dispatcher) plus the `quill_from_path` loader. Re-exports core's `Quill`. Handles backend resolution at render time and auto-registration. Filesystem walking for `quill_from_path` lives here; core is filesystem-agnostic (in-memory loading is `Quill::from_tree` in core). The engine does not construct quills — it only renders them.

### `backends/quillmark-typst`

Implements `Backend` for PDF, SVG, and PNG. Converts Markdown fields to Typst markup inside `open()`. Resolves fonts and assets. See [PLATE_DATA.md](PLATE_DATA.md).

### `bindings/quillmark-python`

PyO3 bindings published as `quillmark` on PyPI.

> **Status: experimental, second-class binding.** The Python surface lags
> the WASM binding in coverage and in error-shape uniformity. New
> diagnostics / contract work lands in WASM first; Python catches up on a
> best-effort basis. Do not gate releases on Python parity.

### `bindings/quillmark-wasm`

wasm-bindgen bindings published as `@quillmark/wasm`. Builds with `--target bundler` and `--weak-refs` so wasm-bindgen handles are reclaimed by `FinalizationRegistry`; `.free()` remains as the eager teardown hook. Requires Node 22+ / current evergreen browsers.

Ships **multiple artifacts from one crate** behind a single public root export. The root `@quillmark/wasm` is a hand-written **canonical runtime layer** that re-exports the internal Typst-less **core** build's `Document` + `Quill` (load / validate / schema / seed / blueprint) verbatim and adds an `Engine` render dispatcher. Each backend (Typst today) is a **private** build with its own linear memory, lazily loaded on the first render — there is no public `/core` or `/render` subpath. The core build is ~0.66 MB gzip; the Typst backend ~8 MB (Typst dominates), loaded only when something renders. Backend handles never escape the `Engine`: it clones the quill tree + `doc.toJson()` into the backend's memory as serialized data and frees the clones. See [the split proposal](../proposals/wasm-bindings-split.md) (superseded) and [the as-built 0.90 design](../../docs/migrations/0.89-to-0.90.md).

In addition to the byte-output verbs (`engine.render`, `RenderSession.render`), the Typst backend build exposes a Typst-only **canvas preview** path on `RenderSession`: `pageCount`, `pageSize(page)`, `paint(ctx, page, opts?)`, plus `backendId`, `supportsCanvas`, and `warnings`. Capability lives on the engine (`engine.supportedFormats(quill)`, `engine.supportsCanvas(quill)`); the session mirrors it. The painter rasterizes pages directly from the cached `PagedDocument` into a `CanvasRenderingContext2D` or `OffscreenCanvasRenderingContext2D`, sizes the canvas backing store itself, and returns the chosen layout/pixel dimensions. Skips PNG/SVG round-trips. See [PREVIEW.md](PREVIEW.md).

### `bindings/quillmark-dotnet`

C-ABI `cdylib` consumed from C# via P/Invoke, published as `Quillmark` on NuGet — the .NET analogue of the PyO3 module. A flat `qm_*` C ABI over `quillmark` plus a hand-written managed layer (`csharp/`) that reassembles the typed surface, deliberately **symmetrical with the Python binding** method-for-method. Structured data (cards, schema, metadata, diagnostics, field values) crosses as `serde` JSON from the same core types the other bindings use; stateful objects cross as opaque handles; panics are trapped at the boundary (the analogue of PyO3's trapping / the WASM panic hook) and surface as the single `QuillmarkException`. The NuGet package carries the native library per RID under `runtimes/<rid>/native/`.

> **Status: experimental, second-class binding.** Mirrors the Python surface and shares its footing — render-only (no canvas preview), best-effort parity, not a release gate.

### `bindings/quillmark-cli`

Standalone binary. See [CLI.md](CLI.md).

### `quillmark-fixtures`

Test resources under `resources/`. Helper functions for test setup.

### `quillmark-fuzz`

Property-based fuzz tests (proptest): `parse_fuzz` (YAML/Markdown parsing), `convert_fuzz` (Markdown→Typst conversion + escaping), `emit_roundtrip_fuzz` (emit roundtripping), `filter_fuzz` (filter injection safety), `coerce_fuzz` (type coercion).

## Core Interfaces

- **`Quillmark`** — Engine: a backend registry + render dispatcher. Auto-registers `TypstBackend` when the `typst` feature is enabled. Resolves a quill's declared backend at render time (erroring `UnsupportedBackend` on no match) and owns the backend-dependent surface — `render`, `open`, `supported_formats(&quill)`, `supports_canvas(&quill)`. It no longer constructs quills
- **`Quill`** — The single quill type in `quillmark-core`: portable, validated, engine-free data (file bundle + config + metadata, tagged with a declared backend id). Held by value. Exposes the pure config-read operations: `dry_run`, `compile_data`, `backend_id`, plus `validate` (editor-facing: returns every schema diagnostic, including the non-fatal `field_absent` signal render demotes) and the `seed_document` / `seed_main` / `seed_card` starters that emit committed example documents and cards. Construct with `Quill::from_tree` or `quillmark::quill_from_path`
- **`Backend`** — Trait for output formats (`Send + Sync`): `id()`, `supported_formats()`, `supports_canvas()` (default `false`), `open(plate, &Quill, json)`
- **`RenderSession`** — Opaque handle returned by `Backend::open()`; call `render(opts)` to produce artifacts. Exposes `page_count()` and `warnings()` for consumers (e.g. canvas previews) that don't go through `render()`. Backends with richer typed surfaces expose them via a downcast helper that goes through `RenderSession::handle()` + `SessionHandle::as_any` (Typst uses this for canvas preview — see `quillmark_typst::typst_session_of`).
- **`Document`** — Typed in-memory representation of a Quillmark Markdown file (root block, body, cards). Serializes via `serde` to a versioned JSON envelope (`StoredDocument`) for database persistence, decoupled from the evolving Markdown syntax — see [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)
- **`Diagnostic`** — Structured error with severity, code, message, location, hint, source chain
- **`RenderResult`** — Output artifacts + accumulated warnings

## Data Injection

`Backend::open()` receives:
- `plate_content` — raw plate string from `Quill.plate` (empty string for plate-less backends)
- `source` — `&Quill` with static assets/packages, config, metadata
- `json_data` — JSON object after coercion, defaults, normalization

See [PLATE_DATA.md](PLATE_DATA.md) for the Typst helper package.

## Backend Implementation

Implement `id()`, `supported_formats()`, and `open()` of the `Backend` trait; optionally override `supports_canvas()` (defaults to `false`) if the backend can paint to a canvas. Return a `RenderSession` wrapping a `SessionHandle` that handles format-specific rendering.

See `backends/quillmark-typst` for the reference implementation.
