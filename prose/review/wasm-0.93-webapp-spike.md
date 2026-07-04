# Spike: web-app on the unreleased `@quillmark/wasm` 0.93

> Branch pair: `claude/quillmark-wasm-migration-pscq14` here and in
> `tonguetoquill/web-app`. The web-app branch consumes this tree's
> `pkg/` as a `file:` dependency and rebuilds its preview on the
> `Engine.open` → `LiveSession.apply`/`paint`/`regions`/`fieldAt`
> surface.

## What the spike did

- Built the three-artifact `pkg/` from this working tree
  (`scripts/build-wasm.sh --ci`) and pointed web-app's
  `@quillmark/wasm` at it.
- Replaced web-app's session-per-render preview with a
  `QuillmarkPreviewController`: one `LiveSession` per (document ×
  quill), `apply(doc)` per edit, repaint of `dirtyPages` only; a
  quill swap re-opens.
- Wired editor ↔ preview cross-navigation: preview → editor clicks
  resolve through `LiveSession.fieldAt(page, x, y)` (a document
  hit-test), not by rendering per-region hotspot buttons; editor →
  preview focus highlights the focused field's `regions()` geometry
  (decorative only, not a click target). Editor-side addresses use the
  same `$cards.<kind>.<n>.<field>` grammar regions carry.
- Verified against the production quill catalog: a real-wasm vitest
  suite (open / apply / ChangeSet / transactional failure / regions /
  `fieldAt`) plus a browser drive of the live app.
- Five passes so far: filed findings as #782 (resolved by
  #783/#784/#785/#788); pulled onto the span-based region rework (#795,
  superseding #788) and a helper-codegen rewrite (#800), filing the two
  findings below as #801; pulled onto #801's fix (#813) plus an
  unrelated correctness fix (#814); pulled onto two further backlog-
  cleanup commits (#815, #816) with no findings — see below, the pass
  this report now describes.

The surface holds together: `apply` is transactional as documented, a
failed apply keeps every read serving the last-good compile, the
canvas paint/DPR contract needed no consumer-side changes, `fieldAt`
resolves clicks correctly everywhere content is drawn (including where
`regions()` under-enumerates), and a no-op apply now correctly dirties
nothing — the last open finding from this spike is closed.

## Resolved this pass: both #801 findings landed upstream (#813)

### `fieldAt` delegation — fixed on `main`, this branch's local patch superseded

The missing `runtime.js` delegation (found last pass, patched locally
on this branch as a stopgap) is now fixed identically on `main`
(`346c864`). Rebasing produced the expected conflict — both sides added
the same three-line forward with different doc-comment wording — kept
upstream's wording and dropped the local patch's own copy. No smoke
test asserting a live call (not just a type-check) was added, per the
original ask; still worth doing, but not blocking.

### Dirty-page tracking — fixed on `main` with a two-layer, root-cause repair (`346c864`)

Confirmed by direct inspection of the landed diff, not just changelog
text. Two independent layers, matching the mechanism this report
guessed at:

1. **`page_hashes` (`crates/backends/typst/src/lib.rs`) now explicitly
   excludes Typst `Span` from every hashed `FrameItem`** — glyphs keep
   font/size/paint/geometry, `Shape`/`Image` keep their visible fields,
   none keep the trailing source-location `Span` their derived `Hash`
   impl would otherwise fold in. The fix's own doc comment states the
   contract plainly: *"two compiles whose pages rasterize
   pixel-for-pixel identically must hash identically... only
   render-affecting data... may enter the hash."* A direct regression
   test (`page_hashes_ignore_span_shift_when_ink_is_identical`)
   pins it: two quills differing only by an unused extra schema field
   (shifting every content block's byte position, hence every glyph's
   span, with zero rendered-pixel difference) must hash identically.
2. **Codegen (`crates/backends/typst/src/helper.rs`) now emits every
   dict in canonical sorted-key order**, closing the actual trigger
   this report traced to: `serde_json`'s `preserve_order` let an
   editor's mutate path hand `apply` the same content in a different
   field order than `open` saw, shifting the generated `lib.typ`'s
   byte layout (hence every span below the shift) with no rendered
   change. Canonical ordering makes the generated source a pure
   function of values, independent of caller insertion order — a
   reorder-only apply is now a `Source::replace` no-op, pinned by
   `reordered_input_emits_byte_identical_source`.

Both are load-bearing: sorted-key emission prevents the common
reorder-only trigger from shifting layout at all, and span-exclusion
makes hashing correct even when a layout shift does happen for some
other reason. The backend's own end-to-end regression test
(`reapply_with_reordered_fields_same_content_is_clean`,
`crates/backends/typst/tests/live_apply.rs`) names this exact scenario
as *"the web-app's #801 `[0]`, reproduced at the backend"* and asserts
the conjunction holds.

**Verified against web-app's own integration test**: the "no-op apply"
case, which last pass pinned the buggy `[0]` with an explanatory
comment, now asserts the correct `dirtyPages: []` and passes.

## Unrelated fix landed in the same pull: silent data loss in typed-dict/table resolution (#814)

Not something this spike found — noting it because it landed in the
same `main` pull and is a genuine correctness fix worth knowing about.
`resolve_value` (`crates/core/src/quill/compose.rs`), which projects a
present value against its field schema at render time, previously
**rebuilt** a typed dictionary or typed-table row from only its
*declared* properties — any key the schema didn't name was silently
dropped from what reached the plate. Fixed to preserve undeclared keys
verbatim (the schema is a floor, not an allowlist), with regression
tests for both the plain typed-dict and typed-table-row cases. Also in
this pull: a blueprint fix so an Unendorsed markdown field's `example:`
surfaces as a `# e.g.` hint instead of vanishing, and doc-comment
corrections for error messages/codes stale since the earlier error-
system rework (no behavioral change).

**No web-app action**: confirmed none of the four production quills
(`usaf_memo`, `af4141`, `daf1206`, `daf4392`) declare a `type: object`
field, so this bug had no surface to affect in the current catalog.

## This pass: two backlog-cleanup commits reviewed line-by-line, no findings (#815, #816)

Read every changed file in both commits rather than trusting the
titles — the last three passes each turned up something the changelog
didn't mention, so "cleanup, no consumer impact" got the same scrutiny
as a breaking change would.

**#816** ("Ground #809/#810"): `content_field_names`/`date_field_names`/
`array_field_names` (`crates/backends/typst/src/lib.rs`) now share one
`field_names_where(properties, predicate)` filter — confirmed
byte-identical map-order iteration to the three separate loops it
replaced, so `regions()`'s enumeration order (which the region tests
and this branch's `fieldAt`/widget-ordering assertions depend on) is
unaffected. `quillmark-pdf`'s `splice_dict_value` deduplicates a
pointer-arithmetic dict-rewrite that already existed, verbatim, at two
call sites (`stamp.rs`, `writer.rs`) — not a new pattern, just one copy
instead of two; both existing call sites already upheld the "value
must be a subslice `find_dict_value` returned from this exact buffer"
contract the function's doc comment states. `PdfUpdate::resolve_pages`
now memoizes its per-page generation/rotation checks — sound, because
those checks are pure validity predicates over a fixed byte buffer
within one call; skipping a repeat check for an already-validated page
cannot let an invalid page through. A new doc comment on
`COMEMO_EVICT_MAX_AGE` (`crates/backends/typst/src/compile.rs`) honestly
notes the eviction clock is process-global, not per-`QuillWorld` — cache
reuse degrades under concurrent-session use (a multi-tenant server, not
web-app's one-session-per-tab preview), never a wrong render.

**#815** ("Tidy deferred backlog"): the count-based error-summary rule
(`"<N> error(s): <first message>"` / bare message / `"render error"`)
is now one function, `RenderError::summary_message`, with `Display`,
the WASM binding, and both Python error paths delegating to it instead
of re-deriving it. Traced the WASM path specifically since it's what
web-app's `diagnostic-utils.ts` consumes: `WasmError::message()` still
short-circuits its own `[] → "Unknown error"` case before ever calling
`summary_message` (whose own empty-case default, `"render error"`, is
therefore unreachable from WASM) — the one- and many-diagnostic cases
delegate and produce byte-identical strings to before. Zero
consumer-visible change. A test file (`eval_error_hint.rs`) dropped an
assertion for a retired synthetic-hint feature; confirmed the claimed
replacement coverage (`error_mapping::unresolvable_span_without_typst_hint_carries_no_hint`)
actually exists rather than taking the commit message's word for it.
New `pdfform-backend.md` documentation of a real (pre-existing, not
new) limitation — the flattened SVG/PNG/canvas preview renders
non-WinAnsi characters as `?` and clips multi-line overflow, while the
stamped PDF stays correct and complete — worth remembering if web-app
ever adopts a genuine `pdfform` quill (today's DAF-form quills are
Typst recreations, not pdfform, so this has no current surface either).

**Outcome**: no web-app source changes this pass. Rebuilt `pkg/`
(`0.92.2-dev.5d19050`), reran the full integration suite unchanged
(4/4 pass, same assertions as last pass), and the rest of web-app's
verification (svelte-check, lint, 594 server + 22 browser tests,
production build) is clean without touching any consumer code — the
first pull in this branch's history where "rebuild and reverify" was
the entire task.

## Superseded from the previous pass: `tagged()` is gone, replaced by span tracking (#795)

The `tagged()` escape hatch from #788 (which the previous pass ported
into web-app's `usaf_memo` plate) is **removed** — calling it is now a
compile error. Region tracking for content fields is span-based:
each content field's markup is codegen'd as its own markup-block
binding (`helper-codegen-v2`, #800), so every glyph carries a span
inside that field's byte window regardless of what package reprocesses
it afterward — including the exact case `tagged()` existed to patch
(the memo package's AFH-numbering rebuild). Direct scalar references
(`data.subject`) are now tracked per reference site too, with no
wrapper needed.

**Reverted on this branch**: unwrapped both `tagged()` calls in
web-app's `usaf_memo` plate back to plain placement, matching the
fixture's own revert (`2a9d516`). Confirmed live: `session.regions()`
coverage is unchanged from the tagged version (`$body`,
`signature_block`, `tag_line` on the shipped template;
`$cards.indorsement.0.$body` / `.signature_block` on a card) — the
span mechanism reaches everything the marker mechanism did, with no
plate-author effort at all now.

**New consumer-visible nuance**: `regions()` reports only a value's
*first maximal run* of ink — a run ends at any foreign-ink interruption
on the same page (the "twice-placed" ambiguity span data can't resolve
any other way), so a body broken up by per-paragraph auto-numbering
(every paragraph in `usaf_memo`) reports exactly one region, on its
first page, not one per page it actually spans. Verified: a 12-paragraph,
8-page body yields a single `$body` region on page 0. `fieldAt`,
which hit-tests the compiled document directly rather than reading the
sidecar, still resolves correctly on every later page. Web-app's
highlight-on-focus overlay is downstream of `regions()` and inherits
this: focusing a long body highlights only its first page's worth of
ink, not the full extent. Click resolution is unaffected since it goes
through `fieldAt`, never the region list — see the `Preview.svelte`
rewrite below.

**Consumer breaking change, handled:** with hit-testing now the
documented click path, web-app's per-region hotspot-button overlay
(one absolutely-positioned clickable `<button>` per enumerated region)
is gone. `Preview.svelte` now has one click surface per page (the
existing whole-canvas mask) that converts the click point to PDF pt
and calls `fieldAt`; `regions()` is read only to draw a non-interactive
highlight box for the editor's currently-focused field.

## Resolved from earlier passes (recap)

- **`Document` lifetime footgun** (#785) — fixed; the natural
  `try { return engine.open(...) } finally { doc.free() }` shape is
  safe.
- **`apply` warnings channel** (#784/#790) — fixed; `session.warnings`
  reflects the current compile, refreshed per committed apply.
- **From-source version stamping** (#785) — fixed; this rebuild
  produced `0.92.2-dev.dcea9f7`, not `0.92.1`.
- **Canonical `runtime.d.ts` drift** (flagged two passes ago, `b1b5438`) —
  fixed; `RenderOptions.regions` and the per-placement `regions()` doc
  are synced, and a `typecheck` step is now wired into CI so this class
  of drift fails the build going forward.
- **`fieldAt` delegation gap and dirty-page span-hashing bug** (#801,
  fixed as of this pass, `#813`/`346c864`) — see above.
- **`plate_file`** — still a deliberate scope decision, not a fix
  (pre-1.0 hard cutover policy); web-app's branch still carries a
  hand-migrated local copy of `@airmark/quiver`'s `Quill.yaml`s.

## Notes (no action asked)

- `apply` is synchronous on the main thread; a worker/OffscreenCanvas
  story stays the consumer's problem.
- The `$cards.<kind>.<n>.` address grammar is now implemented four
  times (Typst prefix builder, the removed `tagged` helper's `$path`
  injection — still used by `form-field`/`signature-field` — pdfform
  resolver, web-app `field-path.ts`). A `document.fieldPath(cardIndex,
  field)` accessor would collapse the drift surface.
- `fieldAt` returns a field path only, no geometry — a "highlight
  under cursor on hover" feature (as opposed to click-to-navigate) has
  no API to build on without either a rect back from `fieldAt` or a
  separate placement-rect-at-point query. Not requested; noting the
  gap since the click-anywhere model invites the question.
- The DAF form quills are Typst recreations, not `pdfform` quills, so
  the pdfform widget-region path has no production consumer yet.
- `daf1206`'s plate bypasses the content-eval pipeline entirely (stuffs
  field values into a generated form template's parameter dict, not
  Typst content), so neither auto-tag nor span-tracking reaches it
  without a deeper plate rework. Out of scope for this pass, unchanged
  from the previous one.

## Web-app branch caveats

- `package.json` depends on `file:../quillmark/pkg`; CI cannot
  `npm ci` until `@quillmark/wasm@0.93.0` publishes.
- `static/quills` is repacked from a locally-migrated `usaf_memo`
  (`plate_file` moved under `typst:`, `tagged()` calls added then
  reverted per the supersession above) living in
  `node_modules/@airmark/quiver` — untracked by git, and NOT
  regenerated by any install/build hook (`pack:quills` is a standalone
  script), so it only goes stale on an explicit re-pack without
  reapplying the patch. The published `@airmark/quiver` package still
  carries neither the `plate_file` move nor benefits from span
  tracking (it never needed `tagged()` to begin with) and should
  replace this local patch once it re-releases alongside 0.93.
- `live-session.integration.test.ts` pins: current region coverage
  (`$body`/`signature_block`/`tag_line`, span-tracked, no `tagged()`);
  `fieldAt` resolving both at region centers and past what `regions()`
  enumerates; kind-scoped card addressing; and — since #801's fix
  landed — the CORRECT no-op-apply behavior (`dirtyPages: []`), no
  longer a pin on the regression itself.
