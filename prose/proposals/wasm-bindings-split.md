# WASM Bindings Split: Core + Render

> **Motivation**: a web content editor that only loads quill schemas and
> validates documents currently downloads and instantiates the full
> rendering binding — ~8.7 MB gzipped — of which Typst is ~96%. Splitting
> the binding into a small **core** (load / validate / schema / seed /
> blueprint) and a heavy **render** (Typst-backed preview / export)
> decouples editor and serverless cold start from rendering.
> Pre-1.0; breaking changes acceptable.

## TL;DR

Publish **two** WASM artifacts from one crate, gated by a `render` cargo
feature, shipped as one npm package with two entry points
(`@quillmark/wasm/core`, `@quillmark/wasm/render`). Render is a strict
superset of core.

The split is enabled by one architectural change: **`Quill` no longer
holds a backend.** It becomes engine-free, portable validated data. The
`Quillmark` engine shrinks to a backend registry + render dispatcher and
exists only in the render build. The boundary falls on *which types
exist*, not on feature-gated methods inside a shared type.

## Measurement (spike result)

Both built with the shipping `wasm-release` profile (opt-level=z, fat
LTO, 1 codegen-unit, stripped), gzipped at -9:

| Artifact | Raw | Gzip |
|---|---:|---:|
| Core (parse · load · schema · validate · seed · blueprint) | 0.9 MB | **0.34 MB** |
| Monolith (everything, incl. Typst) | 25.1 MB | **8.72 MB** |

Typst is ~8 MB of the 8.7 MB gzip (~96%). Core is ~26× smaller. The
~0.34 MB core floor is YAML/JSON/Unicode-table code, not Typst — there is
little more to squeeze, but 8.7 MB → 0.34 MB is the win.

## Design

### `Quill` is engine-free, portable data

Today `Quill = Arc<QuillSource> + Arc<dyn Backend>`, and the backend is
baked in at load by the engine that created it. The backend is only used
by `render` / `open` / `supported_formats`.

After this change `Quill` holds only `Arc<QuillSource>` plus the
*declared* `backend_id` (= `config.backend`, a string). Every remaining
method — `validate`, `schema`, `metadata`, `blueprint`, `seed_*`,
`compile_data`, `backend_id` — is a pure `source.config()` read.

Consequences:

- **A `Quill` requires no engine to construct or to use.** Construction
  moves onto `Quill`:
  - `Quill::from_tree(FileTreeNode) -> Result<Quill, …>` (pure;
    core-reachable)
  - `Quill::from_path(path) -> Result<Quill, …>` (filesystem walk stays
    in `quillmark`; core remains fs-agnostic)
- **A `Quill` is portable across engines.** It is `Send + Sync` data
  tagged with intent ("I want backend `typst`"); any engine with a
  matching backend can render it. Same id may map to different backend
  impls per engine — expected.
- **The backend-existence check moves from load to render time.** Loading
  never needs a backend (which is what lets a Typst-less core build load
  and validate). `engine.open` / `render` returns `UnsupportedBackend`
  when no registered backend matches `quill.backend_id()`.

### The engine is render-only

`Quillmark` becomes a backend registry + render dispatcher. **Locked
surface** — keep both verbs:

```
Quillmark::new() / register_backend(...)
engine.open(&quill, &doc) -> RenderSession
engine.render(&quill, &doc, opts) -> RenderResult   // one-shot convenience
engine.supported_formats(&quill)
```

`RenderSession` is unchanged (it already carries `render` / `paint` /
`pageSize` / warnings).

**Remove the factory.** `engine.quill(tree)` / `engine.quill_from_path`
are deleted; `Quill::from_tree` / `Quill::from_path` are the only
constructors, in both builds. The engine no longer loads quills — it only
renders them.

### Straggler homes

- `backend_id` — stays on `Quill` (it is `config.backend`).
- `supported_formats` — needs the backend → `engine.supported_formats(&quill)`.
- `supportsCanvas` — stays on `Quill` as the existing `backend_id == "typst"`
  probe, so an editor can ask "is preview possible?" without an engine or
  Typst.

### Resulting binding surfaces

- **Core build** = `Document` + `Quill`. **No `Quillmark` engine.**
  Construct via `Quill.fromTree(...)`; then validate / schema / seed /
  blueprint / `compile_data`.
- **Render build** = core + `Quillmark` (engine) + `RenderSession` +
  Typst. Same `Quill.fromTree(...)` constructor; rendering through the
  engine.

### Cross-module handoff is data, not handles

Two WASM modules have separate linear memories; a `Quill` / `Document`
handle from core is unusable by render. This is fine because both models
are serializable: a quill is a `Map<string, Uint8Array>` tree, and a
`Document` round-trips through `toJson` / `fromJson`. Intended flow: the
editor boots **core** (~0.34 MB) for schema/validation/seeding; on
preview/export it lazy-loads **render** (superset) and re-feeds the tree +
`doc.toJson()`. The cross-module case is just the extreme of `Quill`
being portable data.

## Plan

### Phase 1 — Engine-free `Quill` (orchestration / core)
- Drop `Arc<dyn Backend>` from `Quill`; hold only `Arc<QuillSource>`.
- Add `Quill::from_tree` (pure) and `Quill::from_path` (fs walk in
  `quillmark`); delete `engine.quill` / `engine.quill_from_path`.
- Move `open` / `render` / `supported_formats` onto `Quillmark`; resolve
  backend at render time, error `UnsupportedBackend` on no match.
- Move `seed_*` to take `&QuillSource` (already only reads `config()`).
- `compile_data` / `validate` stay pure, hung off `Quill`.

### Phase 2 — Feature-gate the binding
- `default = ["render"]`; `core` = no Typst.
- Gate behind `#[cfg(feature = "render")]`: the `Quillmark` engine,
  `RenderSession`, canvas paint, and all `quillmark_typst::` /
  backend imports. `quillmark` dep uses `default-features = false` in
  the core build.
- Keep the shared TS (`QuillSchema` / `Card` / metadata
  `typescript_custom_section`) in one module compiled into both.

### Phase 3 — Build, package, CI
- `build-wasm.sh`: build twice (core / render) → `pkg/core/`, `pkg/render/`.
- `package.template.json`: two `exports` entry points; render lazy-loadable.
- CI/release: build + test both; size-budget check on core to keep Typst
  from creeping back in.

### Phase 4 — Tests & docs
- Core JS tests: load → schema → validate → seed → blueprint; assert no
  render API present.
- Render tests: existing suite, unchanged.
- Update `ARCHITECTURE.md`, `QUILL.md` (the `QuillSource` vs `Quill`
  split: `Quill` no longer holds a backend; engine no longer constructs
  it), and `PREVIEW.md`. Document the data-not-handles handoff.

## Out of scope
- Python / CLI changes (best-effort follow-up).
- Collapsing `Quill` into `QuillSource` (possible once `Quill` is
  backend-free, but churns core's public API and the canon split for
  little extra payoff — defer).
- Sharing one linear memory across modules; new render features.
