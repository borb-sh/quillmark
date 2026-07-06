# PR-F spike findings — regions + navigation (#829)

A de-risking spike on `claude/phase-2-richtext-pr-f-spike` (off PR-E), probing
the two genuinely uncertain mechanisms in [phase-2.md](phase-2.md)'s "Regions +
navigation" section and risk-register risks 2 and 7. Not an implementation:
every claim below maps to a test in
`crates/backends/typst/src/overlay/span_scan.rs`'s `#[cfg(test)] mod tests`,
none wired into `scan`/`field_at`/`RenderedRegion`. Run with
`cargo test -p quillmark-typst --lib overlay::span_scan`.

**Bottom line: go, with one correction to the plan text and one correction to
Spike B's (phase-0.md) optimism about `glyph.span.1`.** The run machine needs a
third classifier outcome and one matching arm — "unmodified" is imprecise but
the fix is exactly as small as the plan implies. `glyph.span.1` is exact for
markup text but coarser than a single Typst call for `#raw` string literals —
worse than "cluster-exact, never sub-char" implied.

## Unknown 1 — run-machine transparency

**Not truly unmodified. The classifier needs a third outcome; the machine
needs one new arm — but it's exactly as small as the plan implies, once scoped
correctly.**

### The fix

`Classifier` gains `resolve_range` (`span_scan.rs:149`), extracted verbatim
from `classify`'s existing unpack so both share it — `classify`'s behavior is
untouched (all 45 existing crate tests plus the 14 `content_regions.rs`
integration tests still pass unmodified). The prototype
`classify_two_tier` (`span_scan.rs:696`) layers segment lookup on top: having
found the owning window by block-range containment (today's `classify`), it
additionally searches that window's `segments` for one whose `gen` range
contains the resolved range, returning `Option<(window, Option<segment>)>`.

The run machine (`run_two_tier`, `span_scan.rs:1002`, a flattened stand-in for
`scan`'s loop at `span_scan.rs:295-324`) keeps every existing state
(`Run::NotSeen`/`Suspended`/`Done`), the single global cursor, and the page-`+1`
continuation tolerance verbatim. It gains exactly one arm:

```rust
TierHit::FieldOnly(w) => match current {
    Some((c, _)) if c.0 == w => {}  // transparent: same field, no segment
    _ => { /* suspend current, exactly like foreign ink */ }
},
```

`field_only_ink_is_transparent_but_foreign_ink_is_not` (`span_scan.rs:1058`)
proves the crux directly: the identical hit sequence with `FieldOnly` ink
between two hits of one segment leaves the run unbroken (`vec![0, 0]`), while
the same sequence with genuinely-untracked `Foreign` ink breaks it (`vec![0]`,
no same-page resume) — exactly today's rule, unweakened.

### The scoping the plan text omits

Transparency is **not** a global "skip `(window, None)` unconditionally" rule.
It must be scoped to *the same window as whatever segment is currently
accruing*. `field_only_ink_still_suspends_a_different_fields_current_run`
(`span_scan.rs:1106`) proves why: if field B's segment is the current run and
field A's own field-only ink passes by, it must still suspend B — otherwise an
interleaved second placement of B, separated only by A's structural ink, would
merge across the gap into one lying box, which is precisely the failure mode
`content_regions.rs::field_placed_twice_surfaces_first_region_but_field_at_resolves_every_placement`
guards against at the whole-field granularity today. A same-window `FieldOnly`
hit is a genuine no-op; a different-window one is indistinguishable from
foreign ink. The plan's phrasing ("neither suspend the current segment run")
reads as if it means the literal global `current`, which is only safe in the
common case (the field-only ink usually appears while its own field's segment
is running); get the scoping wrong and a second real placement of a *different*
field silently disappears into one wrong box.

### The two-tier construction — real classification, plus an empirical surprise

`two_tier_classification_resolves_each_segment_independently`
(`span_scan.rs:746`) compiles a real two-item list (`- Item ONE\n- Item TWO`)
and confirms the half that matters most: each item's own ink resolves to its
own segment (`Some((win, Some(0)))` / `Some((win, Some(1)))`), independently.

The other half — real `(window, None)` ink from list markers or a block-quote
wrapper — **does not materialize**. Traced by hand (temporarily swapping the
test's markdown and inspecting `Classifier::resolve_range` per glyph): Typst's
synthesized list-bullet ink carries a **detached span**
(`DiagSpanKind::Detached`, printed as `Span(1)`) — it resolves to no window at
all, landing in the plain "untracked ink" bucket alongside page chrome, which
today's *unmodified* `None` arm already handles correctly. A `#quote(block:
true)[...]` wrapper draws no extra ink at all by default (no border, no
indicator glyph) — nothing to classify either way. So for the two container
types `emit.rs` currently supports, "(rare, usually inkless)" undersells it:
observed, it's not merely rare, it's **absent** — the plan's own risk-mitigation
language turns out to be conservative in the right direction, not optimistic.

This does not make the fix moot — `(window, None)` is still a real,
independently-necessary classifier outcome, proven with
`classify_two_tier_resolves_field_only_ink_between_segments`
(`span_scan.rs:846`): a real compile of `"before **BOLD** after"` gives three
genuinely distinct resolved node ranges in one paragraph (a bold run is its own
content child); re-windowing them by hand — segment 0 = the "before" nodes,
segment 1 = the "after" nodes, the bold run deliberately excluded from both —
proves `classify_two_tier` correctly reports `(window, None)` for the excluded
run. **Recommendation for PR-F:** implement the three-way classifier and the
transparent arm regardless of today's empirical rarity — a future container
type (nested list markers with real spans, a different Typst version's marker
implementation, a custom show-rule quill) could reintroduce real field-only ink
at any time, and the mechanism is cheap once built. Do not skip it on the
"it never fires today" observation alone.

## Unknown 2 — `glyph.span.1` char precision

**Exact for markup text. Coarser than a single Typst call for `#raw` string
literals — a real correctness gap, not just reduced precision.** Probed in
`glyph_span_1_precision_findings` (`span_scan.rs:1205`), compiling `"This is
**bold** difficult fickle text.\n\n\`\`\`\nfn add(a, b) {\n    return a + b;\n}\n\`\`\`"`
and inspecting every glyph's `span.0` (resolved node range), `span.1`
(intra-node offset), and `glyph.range()` (its own slice of the `TextItem`'s
text) against `emit.rs`'s recorded `SegmentMap`.

### Plain and mark-wrapped markup text — exact, byte-granular

Every node Typst reports (`"This is"`, the space after it, `"bold"` inside
`#strong[...]`, the space before `"difficult fickle text."`, and that
remainder) is **tight**: it nests inside exactly one recorded run, and
`node.start + span.1` lands on the correct generated byte for every glyph
tested — cross-checked against the literal bytes at that offset, not just
range containment. Typst's own node granularity is finer than ours (it splits
plain text and its neighboring space into separate nodes; `emit.rs` records
one run for the whole plain-text stretch) but every sub-node still nests
cleanly inside the owning run, so per-run inversion (the plan's "invert that
run's recomputed escape scan") works exactly as Spike B (phase-0.md) found.

### `#raw(block: true, "…")` — the node is coarser than our own runs

For the three-line code fence, **every line's glyphs resolve to the identical
node range** — the whole `#raw(...)` call expression, confirmed against
`typst-library`'s `raw.rs` (`preprocess`/`highlight`: every line's `Content` is
`.spanned(line_span)` where `line_span` is the *same* value, `self.span()`, for
every line) and against our own recorded runs directly: the call's node range
(56 bytes) does not fit inside *any* one of the three per-line runs `emit.rs`
recorded (14, 18, and 1 bytes) — it is wider than every one of them. Two
consequences, both pinned by the test:

1. **`span.0` alone cannot tell which physical line a hit belongs to.** `span.1`
   *does* reset to 0 at each line boundary (confirmed: offsets go `0..13` for
   line 1, reset to `0..16` for line 2, reset to `0` for line 3) — so within one
   line, precision is real — but nothing in `(span.0, span.1)` distinguishes
   "line 2, offset 4" from "line 1, offset 4". Per-run inversion cannot even
   attempt containment (no run contains the node), so it isn't a precision
   loss on the right line — it's a **structural failure to pick a line at
   all**.
2. **The degrade is coarser than the plan's stated fallback.** "cluster
   resolution falls back to node-start" implies you still land in the right
   token, one notch coarser. Here `node.start` is the byte position of `raw(`
   in the generated source — inside none of the three lines' own text at all.
   The only fallback that stays correct is the **segment's** start (the whole
   code fence's corpus range), which does still classify correctly (the node
   nests inside the segment's `gen` range even though it nests inside no run) —
   i.e. the safe degrade for a multi-line raw block is segment-level, not
   run/line-level.

**Recommendation for PR-F's `position_at`:** when the resolved node's range
does not fit inside any run of the classified segment, do not attempt
`node.start + span.1` — report the segment's corpus start (or, if useful, the
corpus midpoint) rather than a value computed from the wrong base, which would
silently misplace the caret inside the wrong line. A follow-up worth a
deliberate decision, not built here: per-line precision inside `#raw` blocks is
recoverable by correlating each new `FrameItem::Text` boundary (a raw line
starts a fresh `TextItem`, confirmed — `glyph.range()` resets to 0 exactly at
each line) with `SegmentMap::runs`'s ordinal position, in document order. That
is a materially heavier mechanism (frame-structural correlation, not span
inversion) and is out of scope for the paragraph-level #829 delivery; flag it
as future work if code-fence caret precision is ever required.

### List/enum numbering ink

Already covered above (Unknown 1): synthesized marker ink carries a
**detached** span. `position_at` gets nothing to invert there — not degraded
precision, zero attribution. A click on a bullet or auto-number resolves to no
field at all (same as clicking page chrome) — worth documenting explicitly as
an accepted gap in PR-F's navigation contract, since it is a real, common
construct (every list), unlike the rarer container-ink case above.

### Shaping/hyphenation clusters — inconclusive, not disproven

`"difficult fickle"` targets the `fi`/`ff`/`ffi` clusters most Latin fonts
ligate. No clustering was observed under Typst's default font and settings in
this probe: every glyph's `glyph.range()` stayed one character wide. This does
not prove ligatures never collapse multiple characters into one glyph —
`Glyph::range`'s own doc states the range "may be more than one due to...
ligatures" — only that this specific font/config didn't trigger one. Treat the
mechanism as real but unexercised: `position_at` should still defensively floor
a hit inside a wider-than-one-char `glyph.range()` to that cluster's first
character (consistent with how `span.1` is computed once per glyph, at the
shaped cluster's start) rather than assume one glyph is always one character.

## Other landmines

- **Spike B (phase-0.md) needs a correction, not just an extension.** It
  reported char precision as "one mechanical step: add `glyph.span.1`... no new
  Typst capability" and "cluster-exact, never sub-char" for the markup-only
  case it tested. That holds for markup text. It does not hold for `#raw`
  string literals, which Spike B's own scope note didn't exercise (no code
  fence in its probe). PR-F's handover should cite this doc, not just phase-0,
  for the `#raw` case.
- **Detached-span ink is invisible to *both* tiers, not just the new one.**
  Any construct relying on Typst-synthesized decoration (list markers, and by
  extension probably footnote markers, outline/heading numbering if enabled,
  auto-generated captions) gets zero attribution today already — PR-F doesn't
  worsen this, but `position_at`'s design should assume "click resolves to
  nothing" is a normal, frequent outcome near list ink, not an edge case.
- **The synthetic-vs-real test split matters for future maintenance.** The
  mechanism test (`classify_two_tier_resolves_field_only_ink_between_segments`)
  had to build its `(window, None)` case by hand-editing real compiled spans,
  because no current container produces one naturally. If `emit.rs` grows a
  container whose wrapper *does* carry real ink (a future table border, a
  custom callout box), add a same-shape real-compile test alongside the
  existing `two_tier_classification_resolves_each_segment_independently` rather
  than trusting the synthetic one alone to catch a regression in real usage.

## Go/no-go

**Go.** Both mechanisms check out well enough to proceed, with two amendments
to carry into the real PR-F:

1. The classifier needs a genuine third outcome and the run machine one new,
   correctly-scoped (same-window-only) transparent arm — write it as designed
   here, not as a cosmetic no-op layered onto the existing `Some(i)` path with
   a sentinel index (which would let a phantom "segment" accrue its own state).
2. `position_at` must special-case "resolved node wider than every run in the
   classified segment" (the `#raw` multi-line case) as a segment-level, not
   run-level, degrade — the plan's "node-start fallback" phrasing needs
   sharpening to "segment-start fallback" for this case specifically, since a
   literal node-start computation can point at bytes outside every line's own
   text.

Neither finding changes the shape of `RenderedRegion.span`, `locate`, or the
session/WASM surface — the corrections are internal to `position_at`'s
implementation and the classifier's construction, not the API contract PR-F
commits to.
