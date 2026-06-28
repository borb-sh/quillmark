# 02 — Canvas capability contract not closed by construction

**Severity:** major **Category:** architecture **Status:** Resolved (capability
derived) — shared PDF→canvas extraction deferred

## Resolution

The disagreement class is closed by **deleting** the hand-set flag rather than
deriving it from a parallel impl. `Backend::supports_canvas()` is removed;
capability is derived from the one canvas seam:

- `RenderSession::supports_canvas()` — authoritative, session-level, true
  exactly when the session exposes `page_size_pt` for its pages. It is the same
  seam `paint` dispatches through, so the gate cannot drift from the paint.
- `quillmark_core::formats_support_canvas(formats)` — pre-session hint for the
  GUI affordance (keeps the cheap, no-session query the finding flagged as
  load-bearing), derived from output formats.

This removes the "two separately-defaultable methods must agree" problem at its
root — there is no second declaration to disagree. The `paint` mislabel is moot
under this model: `ensure_canvas` and `paint` read the same seam, so a `None`
from `render_rgba` for a valid page can no longer follow a passing capability
check for the in-tree backends.

**Deferred (not done here):** the larger "automatic canvas for any PDF-producing
backend" extraction (shared `quillmark-pdf-raster` path + `raster_pdf` seam) is
*not* part of this change. It was judged to add more coupling than it removed
(core→hayro dependency, two features that must agree, a second canvas seam with
precedence). The capability contract is now closed without it; the extraction
remains an optional future dedup if a third PDF backend appears. The original
design discussion is retained below for reference.

---

**Original finding (design proposed):**

**Location:**
- `crates/core/src/backend.rs:19-28` (`supports_canvas`, default `false`, with the
  "must agree by construction" doc comment)
- `crates/core/src/session.rs:22,51` (`page_size_pt` / `render_rgba`, default `None`)
- `crates/bindings/wasm/src/engine.rs:240` (capability snapshot), `:1440`
  (`ensure_canvas`), `:1405` (paint maps a `None` raster to `page_oob_error`)
- pdfform's current canvas impls (the thing a shared path would subsume):
  `crates/backends/pdfform/src/lib.rs:214` (`page_size_pt`), `:223` (`render_rgba`
  over `flat_pdf` via hayro), `:251` (`render_svg`)

## Finding

Canvas capability is expressed in two independently-settable places: the static
`Backend::supports_canvas()` (a hand-set `bool`) and the dynamic
`SessionHandle::{page_size_pt, render_rgba}` (default `None`). The `backend.rs`
doc comment claims "the two must agree by construction," but nothing in the type
system enforces it — they are three separately-defaultable methods on two traits.
For the two in-tree backends they agree by discipline; an out-of-tree backend can
set `supports_canvas → true` while leaving `render_rgba` at its default `None`,
reproducing exactly the paint-nothing failure the seam was meant to eliminate.

The failure is worse than "paints nothing": with the capability snapshot true,
`ensure_canvas` passes, and `paint` then treats the only remaining `None` from
`render_rgba` as a bad page index (`engine.rs:1405`, `.ok_or_else(|| page_oob_error(...))`).
So a disagreeing backend surfaces a **misleading "page index out of range"**
error for a page that exists.

This is the one place the implementation under-delivers on its own stated design
goal (the proposal said to fold the flag and the impl "so the gap closes by
construction").

## Why it matters

- The generalized canvas seam is a headline of this branch (it replaced the old
  Typst-only `Any` downcast). Leaving the capability as a parallel hand-set flag
  keeps the disagreement class alive for any third-party backend and mislabels
  the failure when it occurs.
- The static/dynamic split is deliberate and load-bearing: `supports_canvas` is a
  **pre-session** query (so a GUI can decide whether to show a canvas affordance
  before paying to open a session), while `render_rgba` needs an open session.
  Any fix must preserve the cheap pre-session query.

## Proposed design — automatic canvas for PDF-producing backends

Origin: pdfform's three preview methods are already nothing but "rasterize/convert
a PDF via hayro" — none is pdfform-specific except *which* PDF they read
(`flat_pdf`). So generalize that into a shared, feature-gated engine capability
and delete the per-backend copy.

- **Shared PDF→canvas path** (feature-gated, hayro/vello): `pdf → rgba`,
  `pdf → svg`, and page size via `quillmark_pdf::page_media_boxes`. Any backend
  that can hand the engine a raster-ready PDF gets canvas/SVG/size for free.
- **New session seam:** a default-`None` provider, e.g.
  `fn raster_pdf(&self) -> Option<&[u8]> { None }`. A PDF-producing backend
  returns its raster-ready bytes (pdfform returns `&self.flat_pdf`); a non-PDF
  backend returns `None`.
- **Dispatch, single predicate:**
  ```
  if let Some(px) = handle.render_rgba(page, scale) { px }            // native override (Typst)
  else if cfg!(raster) { handle.raster_pdf().map(|pdf| pdf_raster(pdf, page, scale)) }
  else { None }
  ```
- **`supports_canvas` becomes derived** from that same predicate (native override
  present, or raster feature on and a raster PDF available) instead of a hand-set
  bool — so capability and impl cannot disagree. A static pre-session form can be
  derived from `SUPPORTED_FORMATS.contains(Pdf)` + the `cfg`, consistent with the
  dynamic path.

### How it honours both constraints (from discussion)

- **Automatic canvas for PDF-output backends** — provide a raster PDF, get the
  rest for free; no per-backend rasterizer code.
- **No PDF mandate** — a non-PDF backend simply doesn't implement `raster_pdf`,
  reports no canvas, and is never forced to produce PDF.
- **Tiny/headless bundle preserved** — the rasterizer stays behind a feature;
  with it off, `raster_pdf` is inert and `supports_canvas` derives to `false`.
  The form-filling-only build links no rasterizer, as today.

### Two correctness/placement points

1. **Rasterize the *flattened* PDF, not the AcroForm deliverable.** Since PDF
   output is now always AcroForm (commit `458064f`), the deliverable renders
   blank in hayro (`NeedAppearances`). The seam is therefore "backend hands me a
   *raster-ready* PDF" (pdfform's `flat_pdf`, produced by the now-internal
   `flatten.rs`), not "engine grabs the deliverable bytes."
2. **Keep Typst on native `typst-render`.** Typst→pixels directly is higher
   fidelity than Typst→PDF→hayro, and Typst already owns that dep. The generic
   PDF path is the default/fallback; the native `render_rgba` override is the only
   remaining per-backend canvas knob, and it is purely additive (its absence just
   means "fall back to generic," never a disagreement).

### Open decision — where the shared hayro path lives

- **`quillmark-pdf` behind a `raster` feature** — keeps the rasterizer next to the
  PDF byte code it consumes (already has `page_media_boxes`); cost: widens the
  crate's "Typst-free, `pdf-writer`-only leaf infra" charter.
- **A sibling `quillmark-pdf-raster` crate** — keeps the spine pristine; the
  rasterizer is its own opt-in leaf. Slight crate ceremony. *(Reviewer lean.)*

## Cheaper fallback (if we don't do the shared path now)

- **Downgrade the doc** at `backend.rs:22-25` to state the agreement is a
  convention external backends must uphold, not a guarantee; and
- **Fix the `paint` mislabel** (`engine.rs:1405`): a `None` from `render_rgba`
  *after* `ensure_canvas` passed should be its own "backend reported canvas but
  produced no raster" error, not `page_oob_error`. One line; correct regardless
  of which larger path we choose.

## Scope note

The shared-path design is a real refactor (core seam + both backends + a dep
move), larger than the other open items. Sequence it after the cheap cleanups.
