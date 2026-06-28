# 06 — Docs & canon drift

**Severity:** low **Category:** docs **Status:** Open

A cluster of doc/code mismatches left after the branch (and after the
`458064f` flatten collapse). Each sub-item is small and independently
actionable; grouped because they're all "docs say X, code says Y."

---

## 6a — `pdfform-backend.md` conflates PNG (a format) with canvas (a paint surface)

**Location:** `docs/quills/pdfform-backend.md:187-188`

```
| **SVG**        | Under the `preview` feature only. |
| **PNG / canvas** | Under the `preview` feature only. |
```

`OutputFormat::Png` is **never** in pdfform's `SUPPORTED_FORMATS` — under
`preview` it is `[Pdf, Svg]` (`crates/backends/pdfform/src/lib.rs`). A consumer
calling `render` with `output_format: Some(Png)` gets `FormatNotSupported`.
Canvas is a `paint()` raster surface (the WASM `render_rgba` path), **not** a
`render()` output format. The table conflates the two.

**Fix:** split the row — `SVG` as a real `render()` format under `preview`; a
separate `Canvas (WASM paint)` row noting it is `paint()`, not `render()`, and
not an `OutputFormat`.

---

## 6b — Canon crate inventory omits the two new crates

**Location:** `prose/canon/ARCHITECTURE.md` (Crate Structure section);
`prose/canon/INDEX.md:20-23` (Backends section)

The branch adds two top-level crates — `quillmark-pdf` (the shared stamp spine)
and `backends/quillmark-pdfform` (the second backend) — but:

- ARCHITECTURE.md's crate-structure list still enumerates only core / typst /
  fixtures / fuzz / bindings. (Note: its `RenderSession` entry *does* describe the
  new canvas seam correctly — only the crate list lags.)
- INDEX.md's Backends section still reads "Typst backend internals: see …" with no
  `pdfform` / `quillmark-pdf` entry (though INDEX line 29 does reference pdfform via
  the PREVIEW.md link).

**Fix:** add `quillmark-pdf` and `quillmark-pdfform` subsections to ARCHITECTURE's
crate structure (mirroring the `quillmark-typst` entry), and a `pdfform` backend
entry to INDEX's Backends section linking `docs/quills/pdfform-backend.md` and
naming the `quillmark-pdf` spine.

---

## 6c — PREVIEW.md overlay formula uses an undefined `y_pdf`

**Location:** `prose/canon/PREVIEW.md:148`

```
y_canvas = (pageHeightPt − y_pdf) × renderScale
x_canvas = x_pdf × renderScale
```

`y_pdf` is undefined relative to a region's `rect = [x0, y0, x1, y1]`. A consumer
drawing an overlay box needs the **top** edge, which in bottom-left coordinates is
`rect[3]` (`y1`); using `rect[1]` (`y0`) yields the canvas bottom. As written the
formula is a correct generic scalar transform but is ambiguous/misleading for the
actual rect.

**Fix:** specify the component, e.g.
`y_canvas_top = (pageHeightPt − rect[3]) × renderScale`, or expand to a full
4-component rect-transform example.

---

## Already reconciled (no action)

- The proposal's `§7` "optional flattened-PDF" deliverable contradiction and the
  "value flattening" framing were fixed in `458064f` (flattening is now described
  as internal preview-only machinery; PDF output is always an interactive
  AcroForm).
