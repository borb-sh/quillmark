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
- Three passes so far: filed findings as #782 (resolved by
  #783/#784/#785/#788); pulled again onto the span-based region rework
  (#795, superseding #788) and a helper-codegen rewrite (#800), which
  is the pass this report now describes.

The surface holds together: `apply` is transactional as documented, a
failed apply keeps every read serving the last-good compile, the
canvas paint/DPR contract needed no consumer-side changes, and
`fieldAt` — once its own gap below was fixed — resolves clicks
correctly everywhere content is drawn, including where `regions()`
under-enumerates.

## New this pass

### `fieldAt` was typed but not implemented — found and fixed here

`dbcd553` added `fieldAt(page, x, y)` to the generated Typst/pdfform
backends and to the canonical `runtime.d.ts` type declaration, but
never added a delegation method to the canonical `LiveSession` wrapper
class in the hand-written `runtime.js`. Every other method on that
class (`regions`, `pageSize`, `paint`, `render`, …) forwards to
`#inner`; `fieldAt` simply had no such forward. Result: `@quillmark/wasm`
type-checks `session.fieldAt(...)` cleanly (the `.d.ts` says it exists)
but throws `session.fieldAt is not a function` at runtime — the method
is unreachable through the package's actual public surface.

The type-level drift guard (`runtime.types.test-d.ts`) does not catch
this class of bug — it checks structural type compatibility between
the canonical and generated interfaces, not whether the hand-written
JS implementation actually has a matching method. Nothing in the test
suite calls `fieldAt` on the canonical `LiveSession` instance.

**Fixed directly on this branch** (`crates/bindings/wasm/runtime/runtime.js`):
added the missing three-line delegation, matching every sibling
method's style. Trivial and unambiguous, so fixed rather than just
reported. **Ask:** land the same fix on `main`, and add a smoke test
that calls every canonical `LiveSession` method (not just type-checks
it) against a live session — the gap this bug fell through.

### Dirty-page tracking no longer converges to empty on a no-op apply, for any quill with content fields

**Severity: high — undermines the incremental-repaint value proposition
`apply`/`ChangeSet` exists for.**

Reapplying byte-identical markdown to an already-open `usaf_memo`
session reports `dirtyPages: [0]` — every time, including a *second*
consecutive no-op apply of the same content (not a one-time settling
artifact). Isolated the cause with a differential test:

- `usaf_memo` (has a `$body` markdown content field): reapplying
  identical content → `[0]`, every time.
- `af4141` (a form-fill quill with `main.body.enabled: false`, no
  markdown content fields at all): reapplying identical content →
  `[]`, correctly.

The difference tracks content fields specifically, which points at
`page_hashes` (`crates/backends/typst/src/lib.rs`): it hashes every
non-`Tag` `FrameItem` via that item's own `Hash` impl, and a `Text`
item's hash transitively includes each glyph's Typst `Span` — data the
span-based region-tracking rework (#795) now depends on for content
fields. If `Span` identity is not guaranteed stable across two separate
compiles of byte-identical source (plausible if spans are allocated
from a per-parse arena rather than derived purely from content), a
content-bearing page's hash differs between compiles even when nothing
about the rendered pixels changed, so `dirty_pages` reports it dirty
unconditionally.

**Consequence for consumers:** any quill with a markdown content field
loses the `dirty ∩ visible` optimization for that field's page(s) —
expect a same-page repaint on *every* keystroke, not just ones that
touch that page. For a single-page memo this is a full-page repaint
per keystroke (functionally correct, no longer incremental); for a
multi-page document only the page(s) actually carrying tracked content
are affected. Web-app's `QuillmarkPreviewController` still works
correctly (repainting an unnecessarily-dirtied page is wasted work,
not a correctness bug) but the perf story is weaker than `apply`
advertises for the common case.

**Not fixed here** — this is a Typst-`Span`-semantics question deeper
than a delegation bug, and needs proper investigation (does `Span`
have a stable, content-derived identity across separate `Source`
instances, or does page-hashing need a different fingerprint for
content-field frame items — e.g., hash the item's *rendered output*
sans span, or hash span *ranges relative to the field's own window*
rather than raw `Span` values) rather than a guess from this pass.
**Ask:** reproduce against a minimal quill (one markdown field, two
identical `apply` calls, assert `dirtyPages` converges to `[]`) and
trace whether `Span` truly varies between compiles of identical
source, or whether the bug is elsewhere in the hash walk.

`live-session.integration.test.ts`'s "no-op apply" case now pins the
observed `[0]` behavior with a comment explaining why, rather than
asserting the correct-but-false `[]` — so this regression is visible
and testable, not silently normalized.

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

## Resolved from the previous pass (recap)

- **`Document` lifetime footgun** (#785) — fixed; the natural
  `try { return engine.open(...) } finally { doc.free() }` shape is
  safe.
- **`apply` warnings channel** (#784/#790) — fixed; `session.warnings`
  reflects the current compile, refreshed per committed apply.
- **From-source version stamping** (#785) — fixed; this rebuild
  produced `0.92.2-dev.d2d2d46`, not `0.92.1`.
- **Canonical `runtime.d.ts` drift** (flagged last pass, `b1b5438`) —
  fixed; `RenderOptions.regions` and the per-placement `regions()` doc
  are synced, and a `typecheck` step is now wired into CI so this class
  of drift fails the build going forward.
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
  enumerates; kind-scoped card addressing; and the dirty-page
  regression's actual (`[0]`, not `[]`) behavior on a no-op apply, with
  a comment pointing back here. Several of these assertions are
  designed to start failing the moment the underlying issue is fixed
  upstream — that's the point for the regression pin; the coverage/
  `fieldAt` pins guard against silent regressions in the other
  direction.
