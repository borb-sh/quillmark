# Live Preview (WASM)

> **Implementation**: `crates/core/src/`, `crates/backends/typst/src/`, `crates/backends/pdfform/src/`, `crates/bindings/wasm/src/`

## TL;DR

The preview surface is two verbs: `render(quill, doc, opts)`, stateless
one-shot bytes for CLI / server / export, and `open(quill, doc)` →
**`LiveSession`**, a persistent, incremental compiler that owns preview. Reads
(`render`, `paint`, `pageSize`, `regions`, `fieldAt`, `positionAt`, `locate`)
serve the session's current compile; `update(doc)` recompiles in place and
returns a `ChangeSet` naming the dirty pages. `paint` writes a rasterized page directly into a
`CanvasRenderingContext2d`; each paint is a **complete** raster: every piece
of page content already visible, so the consumer never composites. It is
multi-backend: any backend whose session can rasterize a page (Typst, pdfform)
paints through one generic painter.

## Why

Every byte-output format costs per page what canvas costs once: iframed SVG
spends a browser document per page, inline SVG a DOM node per glyph, PNG a zlib
encode and decode plus N decoded bitmaps. The painter writes rasterizer pixels
straight into the backing store, so a consumer keeps memory bounded to the
visible viewport.

The edit loop is the second half of the argument. Re-opening a session per
keystroke rebuilds the whole compilation world (fonts, packages, assets) and
repaints every visible page whether or not it changed. A `LiveSession` keeps
the world alive across edits and reports what an edit visibly changed, so the
per-keystroke cost is *incremental recompile + repaint of `dirty ∩ visible`*.

## The seam

`core` carries a backend-neutral session seam, `SessionHandle`
(`crates/core/src/session.rs`); the WASM painter dispatches through it
generically, never downcasting to a backend session type. Compiling and
counting pages is all the trait requires — update, canvas, geometry and
warnings each carry a default, and each method states what a backend leaving
its default costs a consumer. A backend answers the ones it can.

A backend opts into canvas by overriding the two seam methods; there is
no separate capability flag. Capability is **derived** from the seam:
`LiveSession::supports_canvas()` is true exactly when the session exposes
`page_size_pt` for its pages, so `paint`/`pageSize` succeed precisely when the
session reports canvas: the gate cannot drift from the implementation because
there is nothing to keep in sync. There is no pre-session estimate. A binding
consumer opens the session and handles the throw `paint`/`pageSize` already
owe a zero-page compile; a painterless backend, which the workspace ships
none of, would answer through the same throw.

## Live edits: `update` and `ChangeSet`

`update(doc)` recompiles the session against a new document, checking its
`$quill` and compiling it through the config the session was opened against.
**Transactional**: on `Err` the previous compile stays live, every read keeps
serving the last-good document and its `warnings`, and the session recovers on
the next successful update. On `Ok` reads serve the new compile: `warnings`
included, and the returned `ChangeSet { page_count, dirty_pages }` names the
pages whose rendered content changed (including added pages; removed pages are
implied by `page_count`). A preview repaints `dirty ∩ visible` and nothing
else: that repaint bound, not compile speed, is the throughput lever.

Per-backend, update is an implementation choice, not a flag:

- **Typst** recompiles incrementally. The session persists its `QuillWorld`
  (fonts, packages, assets parsed once at `open`); an edit swaps the helper
  package's `lib.typ` via `Source::replace` (incremental reparse) and
  recompiles: `comemo` reuses every memoized eval/layout result the edit did
  not reach. Dirty pages come from per-page fingerprints of *visible* frame
  content; introspection `Tag` items are excluded because a page-spanning
  element's tag carries a hash of content on other pages and would dirty
  page 0 on an end-of-document edit.
- **pdfform** recompiles fully: its compile is a re-resolve + re-flatten,
  cheap by construction. Dirty pages are those carrying a field whose resolved
  spec changed.

**Cache eviction.** Typst's `comemo` cache is process-global and grows
unboundedly without eviction; an editing loop compiles once per keystroke. The
Typst backend evicts entries older than 10 compiles after *every* compile
(`compile.rs`, matching typst-cli's watch policy): the one-shot path leaks
otherwise too, so eviction is unconditional, not a session feature.

**One session type.** Immutability is an invariant, not a type: reads between
edits see a stable document because update swaps the compile only on success,
and the preview consumer (ours: preview is WASM-only by non-goal) executes
serially. There is no separate
frozen snapshot type and no change-generation counter: with a single owned
consumer there is no cross-edit reader to protect. If a long-lived read-only
viewer ever needs to shed the retained world, a `freeze()` that drops it and
keeps the pageable document is a *mode* to add, not a second type.

`update` is the only edit verb: a whole-document recompile, and it is named apart
from the content lane's `applyChange`, which splices ops into a document rather
than recompiling one. Anchoring a caret
or selection across edits is the **editor's** job: its own transaction mapping
(a ProseMirror / CodeMirror `StepMap`) carries positions through local edits, so
the session holds no change log, no revision stamp, and no per-field delta
path: `FieldRegion` / `ContentHit` carry no `revision`. Geometry
(`regions`, `positionAt`, `locate`) is read against the current compile and
re-read after each committed `update`. `positionAt` (point → content position) and
`locate` (content position → caret rect) are exact inverses over that compile:
that pair *is* the bidirectional preview↔editor cursor bridge, and it needs no
forward-mapping because the editor owns the live position it feeds in.

### Complete-raster contract

`render_rgba` returning `Some` guarantees a **complete** page raster: all
content is visible in the returned pixels and the caller paints them with no
compositing of its own. Backends satisfy it differently:

- **Typst** rasterizes its laid-out page natively (`typst-render` →
  `tiny_skia::Pixmap` → unpremultiply → RGBA8).
- **pdfform** pre-flattens the bound field values into the page content
  streams at session-open (and again at each `update`), then rasterizes that
  flat PDF via hayro, so field values appear in the raster on their own, with
  no regions-compositing by the caller.

`Ok(None)` is the out-of-range page and the painterless backend; the `Err` is a
`scale` no page can be rasterized at. Neither rasterizer bounds the buffer it
sizes from `scale × page size`, so a scale that is not finite and positive, or
that puts the page past `MAX_RASTER_PIXELS` (16384², the area of the per-side
clamp below), is refused under `backend::invalid_raster_scale` before either is
asked. `RenderOptions.ppi` meets the same ceiling on the byte-artifact path.

### Painter owns the canvas

`paint` writes the whole backing store with `put_image_data`, which bypasses
the 2D context transform, `globalAlpha`, and clip. The painter therefore owns
the entire canvas: **give each visible page its own `<canvas>` element.** You
cannot paint two pages into one canvas, paint into a sub-rect, or push a page
through a context transform: the raster is complete precisely so you never
need to composite, and the write ignores context state so you could not if you
tried.

Because every `paint` re-rasterizes from scratch (no per-page raster cache on
the session: a deliberate omission, § Decisions), keep a page's canvas alive
while it stays near the viewport rather than pooling one canvas across pages: an
idle canvas retains its pixels for free, whereas reusing a canvas on scroll
re-runs a full render. This keeps memory bounded to *visible + margin* without
paying re-rasterization on every scroll reversal.

Field geometry is primarily a **session-level query**, `LiveSession::regions()`
(see the region type in `crates/core/src/region.rs`): the interactive-preview
path holds a session and reads geometry off the current compile with no
render: re-read it after each committed `update`. A one-shot byte render
carries the same sidecar only on request (`RenderOptions::regions` → `RenderResult::regions`),
for consumers without a live session: static overlays over an exported SVG,
PDF post-processing, CI coverage probes. The sidecar always describes the
whole document: page indices are document-space even under a `pages` subset
render. Each region carries per-field geometry keyed on the **quill schema
field path** (the address the editor uses) plus, for content ink, the
**content span** it covers (§ Segments and the striped union). Navigation is
four queries, two coarse and two fine:

- `regions()` answers *field → rectangles* (scroll to / highlight a field),
  one box per content **segment** it draws. `fieldBoxes(field)` derives the
  whole-field highlight from it: one union rect per page over the field's
  `span`-bearing segments, so consumers do not reimplement the union.
- `fieldAt(page, x, y, tolPt?)` answers *point → field* (click → focus in the
  editor), hit-testing the compiled document directly so **every** placement
  resolves, not just the ones `regions()` surfaces.
- `positionAt(page, x, y, tolPt?)` answers *point → content position*: the field
  *and* a USV offset into its `Content`, cluster-exact, for placing a caret
  or mapping a selection into the content model.
- `locate(field, pos)` answers *content position → caret rect*: the reverse of
  `positionAt`, the box to draw a caret at.

Four producers: **content fields** (a `richtext` or `plaintext` value —
a body, a card's field, a scalar or an `[]` element) are tracked by the
spans their glyphs carry: the
backend evaluates each value at its own generated call site and records the
site's byte window, so the rendered ink resolves back to its field through
*any* placement context, including a package that rebuilds the content (a
`show`-rule pass that buffers and re-emits paragraphs): the origin rides the
glyph, not a marker a rebuild could drop. **Direct scalar references**: each
`data.<field>` / `data.at("field")` expression in the plate is its own
tracked site; a scalar shown in header and footer surfaces both sites, and a
reference wrapped in an expression (`#upper(data.subject)`) attributes the
whole expression's ink to the field when it is the only reference inside it.
A read that steps into a declared container (`data.classification.poc`,
`data.address.at("city")`, `data.refs.at(0).org`) tracks on the **cell**, so the
region names what the plate read rather than the container holding it; a key the
container does not declare is no address, and the read falls back to the
container. The steps are the address grammar's
([PLATE_DATA.md](PLATE_DATA.md#schema-addresses)), so a scanned region is a
name a `field-region` could claim, and each step is its own address: a
whole-row read (`data.refs.at(0)`) names the row.
A read through a `let` alias tracks where the chain it names would, so naming a
container or a row before stepping into it costs no address. The alias holds only where
the plate binds the name exactly once to one whole chain: a name a second binder
could rebind would attribute another value's ink to the field. Only the
occurrences that *read* the name track: an identifier spelling it as a named
argument (`#text(size: 12pt)` under `#let size = data.size`), a dict key, or
another value's field reads nothing off the field, and schema field names
collide freely with the parameter names of callees the plate never defines.
Not tracked: expressions mixing several fields (`data.from + ", " + rank` has
no single owner), a value laundered past what the alias pass follows (a
function parameter, a destructured binding), and card scalars read from the
per-card loop variable (one shared expression site carries no per-instance
identity: bind a widget or wrap a claim for those).
**Marker claims** cover the ink a plate *composes* rather than reads off a
field: a banner keyed on `data.classification`, a package-built address block, a
computed table. The helper's `field-region(field, body)` brackets `body` with
two invisible `metadata` markers, and the frame walk claims for `field` every
piece of ink between them that resolves to no window. It is a **fallback**
claim, not an override: content blocks, scalar sites, and nested
`field-region`s inside keep their own field, so wrapping only ever adds a
region. Each *call* claims separately,
so a wrapper invoked once per card with a `$path`-composed address is one
region per card. Ink attributable to no source position at all (list bullets,
underline rules) stays unclaimed here as everywhere. The marker stack persists
across pages so a claim can span a page break, which leaves a claim whose close
marker never reaches a frame bounded by nothing: those are found before the
scan, suppressed in both the region and point queries, and reported as a
`typst::unclosed_field_region` warning naming the field.
**Form-field widgets** carry the path explicitly — pdfform from the form
mapping, a Typst `form-field` from its `field:` argument, both against the one
address grammar ([PLATE_DATA.md](PLATE_DATA.md#schema-addresses)) — and surface
a region only when they bind one: a widget with no schema field is a backend
artifact, not a routable field.

### Segments and the striped union

A content field is not one box. The backend records a per-**segment** source
map (a segment is one paragraph, heading, or whole code fence: the content's
`continues`-joined line run), and `regions()` returns one region per
`(segment, page)`, each carrying `span: [start, end)`: the USV range of the
field's `Content` that box covers. A scalar reference site, a marker claim, and
a widget carry no `span` (`undefined`): geometry with no content address.

The whole-field highlight is **derived, not emitted**: per `(field, page)`,
union the `span`-bearing segment rects. The union is *striped*: it leaves
inter-paragraph whitespace uncovered, which a single field-level box would
paint over. Emitting a field-level union from the *backend*
would reintroduce the lie the disjointness invariant exists to prevent, so the
union stays out of `regions()`. But the derivation itself is subtle (which
rects carry spans, first-placement-only, widget-vs-content), so a **convenience
owns it** rather than every consumer: `fieldBoxes(field)` (on `LiveSession`,
core `field_boxes(regions, field)` for the one-shot sidecar) folds the
span-filter + per-page union, leaving `regions()` the low-level disjoint truth.
It is content-only: a field placed solely as a scalar reference or a bound
widget carries no `span` and yields nothing, its box being a single `regions()`
rect. Equivalent to the union a consumer would write by hand:

```ts
const boxes = session.fieldBoxes(field);        // one union rect per page

// …which is exactly this, per page, now owned by the helper:
const box = regions()
  .filter(r => r.field === field && r.page === page && r.span)
  .reduce(unionRect, undefined);
```

Each `(segment, page)` key surfaces its **first placement**: one region per
page it touches, so highlighting covers continuation pages (page marginals
between one page's body and the next's do not end a placement; a same-page
interruption does), not every placement: span data cannot distinguish
package chrome interrupting one placement from a second placement of the same
value, and a spanning union would claim the ink between them. A marker claim is
exempt from that rule and accrues its whole extent, because `field-region`
delimits that extent explicitly: an interruption inside a claim is never a
second placement. A field's own
ink *between* its segments (brackets, container-open syntax: usually inkless)
is transparent: it neither accrues a box nor breaks a run. `field` is still
not unique: segment fragments, page fragments, several scalar sites, or
content plus a bound widget each surface independently; consumers group by
`field`. Later placements stay reachable through `fieldAt` / `positionAt`,
where a concrete point identifies one drawn item unambiguously. A blank field
draws nothing and surfaces no region. Geometry only, never a value, and never
needed to complete the picture.

`positionAt` reads the same map the other way: the hit glyph's resolved node
range plus `glyph.span.1` gives an exact generated byte, which inverts through
the owning run's escape scan to a cluster-exact content offset. It is
**cluster-exact, not sub-character**: a hit inside a char that escaped to
several bytes floors to that cluster's first char, and degrades to the
containing segment's start on origin-less ink: a list marker or numbering
(detached-span decoration, attributable to no field: like clicking page
chrome, it resolves to nothing) and, inside a multi-line code fence, every
line sharing one resolved node wider than any per-line run, so per-line
precision collapses to the fence's content start (segment-level correctness
kept). Which of the two happened rides the hit as `granularity`
(`'cluster'` when an owning run resolved the offset, `'segment'` when it floored
to the segment start), so a caret UI trusts a `cluster` offset for the caret
and treats a `segment` one as a segment selection rather than guessing from the
value. `locate` forward-maps a content offset to a generated byte and returns
the covering glyph's box.

**A hit target is the ink, and `tolPt` is what a pointer misses it by.** A
glyph's box is the run's ink height by that glyph's advance, so a text column
answers over a fraction of the area it occupies: horizontally the boxes of one
line abut, but the leading between two lines is inside the paragraph and on no
glyph, and past the end of a short line there is nothing at all. An 11pt
paragraph is live over roughly two thirds of its own height at default leading,
and under half of it double-spaced. Both point queries therefore take a
tolerance: the nearest ink within `tolPt` answers, `0` being exact containment.

**The caller owns the number, because the imprecision is the pointer's.**
Slack is a screen quantity — what a finger or a hand-held mouse
misses by — so a tolerance fixed in points shrinks under the cursor as the page
is drawn smaller, which is where the target is already hardest to hit. A
consumer converts its own slack at the scale it drew the page and passes the
result; the engine holds no default because it cannot see that scale.

**Nearest, not a grown box.** The nearest placement within `tolPt` answers,
which keeps the tolerance a pure widening: containment is distance zero, so no
point that resolves exactly ever changes answer as `tolPt` rises. On a tie the
later-painted wins. Widgets and content ink rank in one comparison, a widget
taking a tie: a widget draws no spanned ink of its own, so ink beneath one must
not swallow a click that lands on it.

The slack is a radius in both axes. A click stays on ink some placement drew:
the empty measure beside a short line is as far from that line as a point the
same distance above it, and a point off the text block resolves to nothing.

## TypeScript surface

Capability and rendering live on the **engine** (it holds the resolved
backend); `Quill` is declarative data. Canvas is in the backend builds only.

The declarations are `crates/bindings/wasm/runtime/runtime.d.ts`, which carries
the per-member contract and which `npm run typecheck` holds to the runtime
beside it.

### DPR / clamp math

The painter owns `canvas.width` / `canvas.height` and sizes the backing store
on every call; consumers own `canvas.style.*` and read `layoutWidth` /
`layoutHeight` from the result. The effective rasterization scale is:

```
renderScale = layoutScale × densityScale
```

Fold `window.devicePixelRatio`, in-app zoom, and `visualViewport.scale` into
`densityScale`. Past **`MAX_BACKING_DIMENSION` (16384 px per side)**: the
floor that works across browsers: the painter clamps `densityScale`
proportionally and reports the outcome on the result (`clamped`,
`effectiveDensityScale`), so a consumer never reconstructs the clamp from the
dimensions. A clamped page renders soft at the same `canvas.style` size.

Each `paint` resets the backing store (writing `canvas.width` clears it), so
paint is always a full repaint: consumers never call `clearRect`.

### Regions overlay transform

One origin serves the whole canvas surface: `pageSize`, `regions`, the point
queries, and the raster all measure from the **page's lower-left corner as
drawn**, so `(0, 0)` is the raster's first pixel and `pageSize × renderScale` is
its extent. A Typst page starts there already. A pdfform background's page need
not. The file's own numbers are in PDF user space, and the page a viewer shows
is the **canvas box**, `/CropBox` ∩ `/MediaBox`, which `pdfcrop` leaves
translated away from `(0, 0)`. The backend reports that box's extent as the page
size and subtracts its corner from every region, matching what hayro rasterizes.
The widget `/Rect`s in the PDF the same session renders stay in user space:
canvas geometry is box-relative, the deliverable is not.

A consumer drawing overlays from `regions` must flip the Y axis: region
`rect = [x0, y0, x1, y1]` is in PDF points with a **bottom-left** origin, a
canvas is **top-left** in device pixels. For a page `pageHeightPt` tall (from
`pageSize`) painted at `renderScale`, the box's top-left canvas corner is the
PDF rect's *upper* edge (`y1 = rect[3]`), not its lower edge (`y0 = rect[1]`):

```
x_canvas_left = rect[0] × renderScale
y_canvas_top  = (pageHeightPt − rect[3]) × renderScale
width_canvas  = (rect[2] − rect[0]) × renderScale
height_canvas = (rect[3] − rect[1]) × renderScale
```

That form is the one for painting an overlay *into* a raster. An HTML/CSS
overlay on a `width:100%` canvas is better off in percentages of the page:
`left% = rect[0] / pageWidthPt × 100`, `top% = (pageHeightPt − rect[3]) /
pageHeightPt × 100`, and the extents likewise, because they track the
displayed size across DPI and pane-resize with no `renderScale` to thread.

## Feature / build mapping

Canvas ships per-backend:

| Build                                     | Backend  | Canvas | Notes                                                    |
| ----------------------------------------- | -------- | ------ | -------------------------------------------------------- |
| `pkg/core/` (no features)                 | —        | no     | `Document` + `Quill` only; no engine, no Typst           |
| `pkg/backends/typst/` (`typst`)           | typst    | yes    | native page raster                                       |
| `pkg/backends/pdfform/` (`pdfform`)       | pdfform  | yes    | pre-flatten + hayro raster; `web-sys` canvas painter     |

Canvas paint is independent of the output formats a backend emits: pdfform
emits PDF alone and paints, because it always links its hayro raster seam.
The wasm `pdfform` feature pulls in `web-sys` unconditionally, so the pdfform
build also ships the generic canvas *painter* (`page_size` / `paint`,
dispatching through the core `SessionHandle` seam): there is no painterless
pdfform variant. `build-wasm.sh` builds the three artifacts (core, typst,
pdfform) sequentially; `runtime/runtime.js` maps each backend id to its build
with a `{ formats }` manifest, drift-guarded by `runtime.test.js`.

## Non-goals

- Native (CLI / Python) exposure. Capability is WASM-only.
- Text selection, find-in-page, accessibility. Canvas has none of these by
  design: if you need them, keep an SVG/PDF export path alongside.
- Click handling in the painter. The painter is a dumb blit; it maps no
  clicks itself. Click→field lives on the **session** (`fieldAt`, hit-testing
  the compiled document): a consumer converts the canvas click to PDF-pt
  page coordinates (the inverse of the [regions overlay
  transform](#regions-overlay-transform)) and asks the session, keeping the
  painter free of interaction state.

## Decisions and rationale

- **Two verbs, one session type.** `render` is the stateless one-shot;
  `open` → `LiveSession` owns preview. The frozen single-compile snapshot is
  not a separate type: its immutability survives as the swap-on-commit
  invariant of a transactional `update`, and its "hold last-good while
  computing next" behavior falls out of the same invariant with no
  special-casing. Third-party preview controllers are out of scope, so no
  defensive snapshot type and no change-generation counter guards a consumer
  we do not ship.
- **One generic painter over the `SessionHandle` seam, not a per-backend
  downcast.** `paint` calls `page_size_pt` / `render_rgba` on the opaque
  session; every canvas backend implements the same two methods. Adding a
  canvas backend is overriding the two seam methods (`page_size_pt` /
  `render_rgba`): capability is then derived from the seam, with no separate
  flag to flip and no binding to touch.
- **No pre-session canvas probe.** The only capability answer is the session's
  own `supports_canvas()` and the throw `paint`/`pageSize` owe when it is
  false. A pre-session probe answers for the backend rather than for the
  compile, so a consumer gating its canvas UI on one still has to handle the
  throw a zero-page document raises; keyed on output formats it also answers
  for the wrong thing, since `render_rgba` is a seam a backend can override
  while emitting PDF alone. Every backend the workspace ships paints, so the
  probe had one answer everywhere; a painterless backend would need a declared
  flag, and there is none to declare it for.
- **`update` reports dirty pages, not new handles.** Page identity is the index;
  a `ChangeSet` is data. Nothing borrowed from a previous compile outlives an
  edit because reads resolve against the current compile at call time.
- **A marker claim is a fallback, and ink still belongs to exactly one field.**
  Wrapping content that already resolves to a field could plausibly make the
  region name both. It does not: `fieldAt` and `positionAt` return one field, so
  a two-field region has no answer to give, and `fieldBoxes` would double-count
  it. Innermost wins instead, which is the ordinary scoping intuition and makes
  wrapping purely additive: a `field-region` takes the ink nothing more specific
  took, and never moves a region off the field that generated it. Retargeting
  ink that is already tracked is therefore not expressible, and deliberately so:
  the wrapper exists for ink with *no* attribution.
- **Complete raster, never compose-from-regions.** Both backends hand back a
  finished page (Typst natively, pdfform by pre-flattening values into content
  streams before rasterizing). Regions are an overlay sidecar, not a
  compositing input: the painter stays a dumb blit.
- **No session raster cache: re-rasterize per `paint`.** Caching the last
  raster per `(page, renderScale)` and blitting on scroll-back would skip
  re-rasterizing unchanged pages (`ChangeSet` already names dirty pages to
  invalidate), but it stays unbuilt: the surface ships ahead of its first
  consumer, the megabyte-scale per-page buffers reintroduce the unbounded
  memory the viewport-bounded design set out to avoid, and any `renderScale`
  change (DPR / zoom) rotates the key and voids the cache. Consumer-side canvas
  liveness (keep the visible page's canvas alive rather than pooling) covers the
  common scroll case without that trade-off; a real consumer's profile is what
  should justify the cache and its eviction policy, not speculation.
- **Method on `LiveSession`, not a sub-handle.** Even with click resolution
  shipped (`fieldAt`), it shares no state with `paint` beyond the compile the
  whole session already owns: a `Preview` sub-handle grouping them is
  ceremony.
- **Not an `OutputFormat`.** Canvas is a side-effecting paint into a JS object,
  not a serializable byte stream. Forcing it into the enum would leak
  `wasm_bindgen` into `core` or make `Artifact` dishonest.
- **Coalesce at the session, not the format.** One compile feeds bytes
  (`render`), pixels (`paint`), and metadata (`pageSize`, `warnings`).
- **`layoutScale` and `densityScale` separated, both optional.** A single
  scalar conflated layout (how big on screen) with sharpness (how many backing
  pixels). The split mirrors how editor consumers think: `layoutScale` is a
  layout decision, `densityScale` a sharpness decision folding `devicePixelRatio`
  + zoom + `visualViewport.scale`. Both default to 1 because the painter cannot
  know the consumer's DPR (SSR, tests, off-screen).
- **Painter owns `canvas.width`/`height`; consumer owns `canvas.style.*`.**
  Folding backing-store math into the painter eliminates a class of "blurry on
  retina" bugs and lets the 16384-px clamp live in one place.
- **Unpremultiplied RGBA on the wire.** Rasterizers produce premultiplied
  alpha; `ImageData` expects non-premultiplied. The backend unpremultiplies
  before handing back the buffer. One allocation per repaint; fine for edit
  cadence.
- **`warnings` accessor on `LiveSession`.** The current compile's non-fatal
  diagnostics (e.g. Typst font fallback): set at open, refreshed by each
  committed `update`, swapped transactionally with the compile. Without the
  accessor they are invisible to canvas consumers (only surfaced via
  `render()`'s `RenderResult`).
- **`regions()` render-free on the session; opt-in on one-shot renders.** The
  invariants are that geometry never composites (the raster is complete
  without it) and that the edit loop reads it without producing bytes: a
  paint-only consumer must never run a throwaway byte render to harvest the
  sidecar. Session exclusivity was never the invariant: there is exactly one
  producer (the frame scan over the current compile), so `RenderOptions::regions`
  attaches the same entries to `RenderResult` for consumers with no session in
  hand (static SVG overlays, PDF post-processing, CI coverage probes, and the
  native bindings, which expose no session surface at all). Off by default:
  exports pay no introspection cost, and best-effort geometry stays a request,
  not a promise attached to every artifact.

## Lifecycle and consumer flow

`open` → `paint` per visible page → `update` on edit →
repaint `dirtyPages ∩ visible`. The [quickstart's canvas
section](../../docs/getting-started/quickstart.md#live-preview-canvas) is the
worked loop.
