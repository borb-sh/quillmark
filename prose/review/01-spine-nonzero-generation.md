# 01 — Spine re-emits overwritten objects at generation 0

**Severity:** major **Category:** correctness **Status:** Open

**Location:**
- `crates/quillmark-pdf/src/writer.rs:16` (`dict_object`)
- `crates/quillmark-pdf/src/stamp.rs:162` (trailer `/Root … 0 R`)
- `crates/quillmark-pdf/src/reader.rs:178` (`find_object_bytes`, matches any generation)
- test proving the reader accepts non-zero generations: `crates/quillmark-pdf/src/reader.rs:701` (`find_object_matches_nonzero_generation`)

## Finding

The incremental-update appender always re-emits an overwritten base object at
**generation 0** and references it as gen 0 (`dict_object`, and the trailer's
`/Root … 0 R`). But the reader deliberately matches an object header at **any**
generation — there is even a passing test (`find_object_matches_nonzero_generation`)
asserting the base may carry gen-2 objects.

So a base PDF whose catalog / page / `/Info` object lives at a non-zero
generation is **in-contract for the reader** (it parses fine) yet produces a
**corrupt incremental update**: the new xref subsection lists the object at gen
`00000` and `/Root` points to gen 0, while the prior xref still resolves the same
object number at its true generation. A reader that honours generations sees an
inconsistent document.

This is a malformed-but-readable input that slips through the contract: the
reader's permissiveness (any generation) and the writer's assumption (everything
is gen 0) disagree.

## Why it matters

- It is a **silent corruption** path, not a clean rejection — the spine's whole
  stated stance is "reject out-of-contract input cleanly" (xref streams,
  encryption, indirect `/Annots`, near-`u32::MAX` `/Size` all hard-error). This
  case is the one gap in that posture.
- **Reachability is genuinely low.** The qualification layer emits gen-0
  traditional-xref PDFs, and the hand-authored `sample_form` fixture is gen 0.
  So no current producer hits this. It is a latent-robustness gap that would
  surface only with a hand-supplied or third-party background.

## Options

1. **Hard-error (recommended).** When any object being overwritten
   (catalog / page / `/Info`) has generation ≠ 0, return a `PdfError`. One small
   check, consistent with the existing `assert_traditional_xref` / `/Encrypt`
   rejections. Closes the gap by construction; adds no permanent complexity.
2. **Carry the real generation** through `dict_object`, the xref entry, and the
   `/Root`/ref serialization. Correct in full generality but adds generation
   plumbing the contract was specifically designed to avoid.

Recommendation: **(1)** — it matches the design's "normalize upstream, keep the
runtime reader light, reject the rest" philosophy and is cheaper than (2).

## Notes

- Adjacent minor robustness item in the same module: `find_object_bytes` locates
  the `endobj` terminator by raw byte search, which could truncate on an object
  whose body legitimately contains the bytes `endobj` (e.g. inside a stream or
  string). Safe for the dict-only objects this crate overwrites, but
  `page_media_boxes` runs the scanner over arbitrary in-contract bases. Consider
  matching `endobj` only at a token boundary. Track with this item or fold into
  #07 coverage.
