# Proposal: `pdfform` backend, `quillmark-pdf` stamping layer, shared preview

> **Status**: Proposal — not yet implemented. Tracking: [#744](https://github.com/quillmark-org/quillmark/issues/744).
> Canon (`prose/canon/`) is not updated until this lands; this doc lives in
> `prose/proposals/` by design.

## TL;DR

Add a dedicated `pdfform` backend for filling government PDF forms, and an
orthogonal shared stamping layer (`quillmark-pdf`) that both the Typst backend
and `pdfform` use to write styled AcroForm widgets into a PDF. Forms arrive
**stripped and rebuilt from a `form.json` spec** rather than filled in place,
which collapses the design to a single "stamp from spec" operation. Preview
generalizes the existing Typst canvas path into a backend-agnostic
raster-preview capability.

## Three pieces

- **`quillmark-pdf` (shared, orthogonal):** a pure function
  `(pdf_bytes, [field_spec]) → pdf_bytes_with_widgets` via `pdf-writer`
  incremental update. Knows AcroForm dicts + styling; knows nothing about Typst
  or field maps. This is essentially today's Typst `overlay::inject`, generalized
  to all field types and lifted into its own crate.
- **Typst backend:** typesets, uses introspection to locate field placements,
  hands the spec list to `quillmark-pdf`. Keeps signature fields; gains general
  form fields.
- **`pdfform` backend:** stripped background PDF + `form.json` → spec list →
  `quillmark-pdf`. `supported_formats = [pdf]` plus raster/SVG/canvas preview.

## Strip-and-rebuild (the key simplification)

The qualification ("quillifying") layer — a separate upstream tool, never the
engine — emits two assets into the quill:

- a **stripped background PDF**: the normalized gov form with its `/AcroForm`,
  widgets, and page `/Annots` **removed** — pure pages + content streams.
- **`form.json`**: the complete field spec to reconstruct the form.

`pdfform` writes the AcroForm **fresh** from `form.json`. It never reads or
reconciles a foreign AcroForm. This deletes the hardest runtime work (resolving
an existing `/AcroForm`, walking the `/T` field tree, reading on-states,
upserting into existing field dicts) and makes both backends do the *same*
single Create operation.

`form.json` must be a **complete, accessible field spec** — not just bounding
boxes: name, page, rect, type, flags (`/Ff`), `/MaxLen`, choice `/Opt`,
checkbox on-state/export values, defaults, tooltip (`/TU`), tab order. Required
to reconstruct equivalent 508-accessible fields.

`form.json` is simultaneously the **field map**, the **regions sidecar** (see
below), and the **reconstruction spec** — one source of truth, page-relative
geometry, inspectable/diffable in the quill.

## The shared spine

Both backends reduce to one producer shape; a shared post-stage stamps + reports:

```
Backend::render(...) → { artifact: base_pdf_bytes, format, field_specs }
                         ↓  (orchestrator applies quillmark-pdf)
quillmark-pdf.stamp(base, field_specs) → { artifact', regions }
```

- **Typst:** `base = typst-pdf bytes`, `field_specs = introspection placements`.
- **`pdfform`:** `base = stripped gov PDF`, `field_specs = form.json`.

They differ only in where geometry comes from. We do **not** route gov forms
through Typst (Typst 0.15 can't embed a PDF page; the background must be carried
through untouched) nor make `pdfform` a Typst "mode."

## Regions sidecar (phased)

The render path returns the artifact **plus** field geometry for the editor's
interactivity overlay (click-to-field, scroll-sync, highlight). Interactivity is
GUI-owned; the engine only reports.

- **Phase 1 (now):** fields only. `RenderedRegion { name, page, rect, kind }`,
  `RegionKind::Field { type, value }`. `kind` is a discriminated enum from day
  one so later phases are additive.
- **Phase 2 (roadmap):** generalize the `<__qm_sig__>` label mechanism to a
  `region(name)[…]` plate function → a named-region `kind`.
- **Dropped:** no markdown↔preview source map. The MD→Typst converter never
  needs to become span-aware.

## Render targets + shared raster-preview capability

The Typst backend already has a WASM canvas painter (`TypstSession::render_rgba`
+ the `paint(ctx)` verb) — see canon `PREVIEW.md`. It is the *right* preview
architecture; the only thing `pdfform` changes is that it stops being
Typst-only.

Render targets per backend:

| Target | Typst | `pdfform` |
|---|---|---|
| PDF (deliverable) | typst-pdf + `quillmark-pdf` stamp | stripped bg + `quillmark-pdf` stamp |
| SVG | `typst-svg` (frames) | `hayro-svg` (vector, PDF→SVG) |
| PNG / pixmap | `typst-render` | `hayro` raster |
| Canvas (paint) | `render_rgba` → `ImageData` | `hayro` pixmap → `ImageData` |

Consequences:

- **Keep the canvas path; generalize it.** `paint(ctx)` is a background-pixel
  delivery optimization (skips PNG encode/decode) and centralizes the
  DPR/backing-store/16384-px-clamp math — it is not an interactivity leak
  (click/cursor mapping stays a documented non-goal, GUI-owned).
- **De-Typst-special-case the pixel capability.** Canon `PREVIEW.md` routes
  canvas through an `Any` downcast (`typst_session_of`) explicitly *because*
  "canvas is Typst-only." `pdfform` invalidates that premise. Promote
  "render page → RGBA pixmap" to a small shared capability (a `RasterPreview`
  trait or optional handle method) that both backends implement; `paint`
  dispatches generically.
- **Render the background, not the stamped PDF.** Under Technique A
  (`NeedAppearances`, no baked `/AP`), a renderer shows blank fields. Both
  preview paths raster the field-free background; field **values** are
  composited from `form.json`/regions by the GUI (or a small server-side
  text-compositing step for a flat artifact).
- **Canvas stays out of `OutputFormat`.** Still a side-effecting paint, not a
  serializable byte stream — the canon rationale holds with two backends.
- **Same engine outputs, different GUI modes.** Both emit *pixmap + regions*;
  the GUI branches on `backend_id` for presentation (generative editing vs form
  projection) and caching cadence (Typst re-rasters on edit; `pdfform`'s
  background is static, re-raster only on zoom). Consistency at the boundary,
  divergence in UX — by design.

## Dependencies & where robustness lives

- **`quillmark-pdf` (runtime, wasm):** `pdf-writer` (stamp) + a pure-Rust reader.
  Strong candidate: **`hayro-syntax`** (reads xref-/object-streams natively),
  pending confirmation it exposes enough for an incremental-update *append*;
  otherwise a hardened minimal scanner. **No `lopdf`, no `liteparse` at runtime.**
- **`pdfform` preview:** **`hayro`** (PNG/pixmap) + **`hayro-svg`** (vector SVG),
  pure Rust → wasm-friendly, same ecosystem as the Typst crates.
- **Qualification layer (upstream, once, verified):** the heavy PDF work —
  normalize, **decrypt (`qpdf --decrypt` mandatory; hayro can't read encrypted)**,
  strip AcroForm, extract → `form.json`. `lopdf`/pdfium/`liteparse` live here,
  never in the engine.

## Relationship to canon

This proposal **generalizes** `prose/canon/PREVIEW.md` (today: "WASM-only Typst
canvas preview path"). When implemented, `PREVIEW.md` is revised to describe the
shared raster-preview capability across backends, and the `Any`-downcast
rationale is replaced by the shared-capability description. Until then, canon
correctly states the Typst-only reality.

## Decisions to ratify

1. `form.json` is a complete, accessible field spec, not just geometry.
2. The qualification **verifier** owns stripped-background fidelity ("rects land
   on printed boxes"; box chrome that lived only in widget `/AP` is restored via
   `form.json` `/MK` styling on rebuild).
3. **Byte-preservation is formally replaced by visual-fidelity + clean
   reconstruction** (strip and rebuild; background page content preserved
   structurally, never rasterized).

## Deferred

Fixed AcroForm capacity / overflow → Typst-typeset continuation pages
**composed** (page-append) with the stamped form. `/DA` font must be in
`/AcroForm /DR`. Encrypted inputs handled entirely upstream.
