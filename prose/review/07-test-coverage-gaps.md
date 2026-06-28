# 07 — Test coverage gaps

**Severity:** low–medium **Category:** coverage **Status:** Open

Consolidated test/edge-case gaps surfaced across the review slices. None is a
known live bug; each is an assertion that *would* have caught a class of
regression and currently does not. Listed roughly by value.

## Spine (`quillmark-pdf`)

- **No non-zero-generation base test.** Directly tied to item #01 — the case that
  silently corrupts output (overwriting a gen-≠-0 catalog/page) has no test. A
  test stamping such a base would currently fail or emit a broken PDF.
- **No xref-stream / encrypted-PDF rejection test.** `assert_traditional_xref` and
  the `/Encrypt` check are core to the "hard-error on out-of-contract input"
  promise, but no test asserts the clean error (unlike the `/Size` and
  missing-page cases, which do).
- **No non-zero `/MediaBox` origin integration test.** `page_media_boxes` returns
  the full origin-bearing rect (a headline deviation), but `tests/stamp.rs` only
  uses `[0 0 612 792]`; nothing asserts a non-zero `x0/y0` flows through.
- **No multi-subsection xref test.** The run-coalescing loop that emits multiple
  `<first> <count>` xref subsections is never exercised with a gap in allocated
  object ids (e.g. overwrite id 1 plus new ids 7,8).
- **No inline-`/Annots` merge test.** `rewrite_page_with_annots`'s array-splice
  branch (base page already has inline `/Annots [...]`) is never hit; the
  indirect-`/Annots` hard-error branch is also untested.
- **`pdf_text_string` UTF-16BE branch untested.** The ASCII-escape path has a unit
  test; the non-ASCII UTF-16BE-hex path (called out in the design as how multiline
  `/V` serializes) does not.

## Regions sidecar

- **`RenderResult.regions` is never asserted non-empty in any integration test.**
  The sidecar is "the only path to values in non-interactive output," yet no test
  renders a fixture and checks `regions` content (name / page / rect / value).
  Especially worth adding for the pdfform `sample_form` path.

## Structural PDF validity

- **Widgets validated only by a tolerant `lopdf` reparse** (which, per the test's
  own comment, "silently tolerates" malformed dicts) plus a single
  duplicate-`/Subtype` byte check on the **signature** widget. The new
  text/checkbox/choice widgets get no byte-level duplicate-key check, and no
  stricter validator (qpdf / MuPDF / pdfium) runs over multi-type output. A
  qpdf/MuPDF lint would harden the regression fence across all four field types.
- **`usaf_memo` multi-signature plate not exercised end-to-end** in `sig_field.rs`
  (the real regression target uses several `Ind_<i>_Signature` widgets on a page;
  the new tests cover one field per page).

## Preview / canvas (`pdfform`, `preview` feature)

- **Flatten byte-level coverage** — being restored at the `flatten()` unit level
  as the finalization of the flatten collapse (no longer a public-path test).
  Once landed, mark this sub-item closed.
- **Multiline / auto-size layout untested.** Flatten tests assert presence of
  `BT`/`Tj`/`re W n`/WinAnsi bytes but not the multiline line-advance
  (`0 -line_h Td`) or the `0 Tf` auto-size clamp; a regression collapsing all
  lines onto one baseline would pass.
- **Canvas "complete raster" heuristic is too weak.** `canvas.test.js:284` asserts
  `inkPixels > 0`, which the background's own borders/labels satisfy even if no
  field value is painted — the single most important pdfform-canvas invariant.
  Sample a known field coordinate (from `FieldRegion.rect` → device space) and
  assert non-white, or floor `inkPixels` by expected glyph coverage.

## Build matrix / bindings

- **Headless `pdfform`-only build (no preview) untested.** The entire motivation
  for the feature split — a Typst-free, raster-free form-filling bundle — has no
  CI step. A break gated on `#[cfg(feature = "pdfform")]` without
  `pdfform-preview` would go undetected. Add a `cargo check`/`test` matrix entry
  (and ideally a wasm size-budget check for the pdfform artifact, analogous to the
  existing `core` budget guard).
- **Python `regions` accessibility unverified.** `PyRenderResult` wraps core's
  `RenderResult`, but no `@property` exposing `regions` was observed; unclear
  whether Python callers can read regions at all. Confirm and either expose or
  document as pending.

## Coercion / resolver (`pdfform`)

- **`is_truthy` string variants** (`"yes"`/`"on"`/`"1"`/`"checked"`, non-zero
  number) are unit-untested — only the JSON-bool path is exercised.
- **`coerce_text` with mixed/non-string array elements** (`[null, "a", {}]`) and
  the all-null-array→`None` path are not directly asserted.
- **Numeric-`$kind` card addressing ambiguity** (`lookup_card` tries
  `parse::<usize>()` first, so a card kind that is a numeric string can't be
  addressed by kind) — untested; document the "kinds must be non-numeric"
  expectation or disambiguate.
