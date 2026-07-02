# Spike: web-app on the unreleased `@quillmark/wasm` 0.93

> Branch pair: `claude/quillmark-wasm-migration-pscq14` here and in
> `tonguetoquill/web-app`. The web-app branch consumes this tree's
> `pkg/` as a `file:` dependency and rebuilds its preview on the
> `Engine.open` → `LiveSession.apply`/`paint`/`regions` surface.

## What the spike did

- Built the three-artifact `pkg/` from this working tree
  (`scripts/build-wasm.sh --ci`) and pointed web-app's
  `@quillmark/wasm` at it.
- Replaced web-app's session-per-render preview with a
  `QuillmarkPreviewController`: one `LiveSession` per (document ×
  quill), `apply(doc)` per edit, repaint of `dirtyPages` only; a
  quill swap re-opens.
- Wired editor ↔ preview cross-navigation over `regions()`: hotspot
  overlays per region (the percent transform in
  `prose/canon/PREVIEW.md` works verbatim), click → expand the owning
  form group and focus the field, editor focus → highlight the region.
  Editor-side addresses use the same `$cards.<kind>.<n>.<field>`
  grammar regions carry.
- Verified against the production quill catalog: a real-wasm vitest
  suite (open / apply / ChangeSet / transactional failure / regions)
  plus a browser drive of the live app (incremental apply repaints,
  both navigation directions).

The surface holds together: apply is transactional as documented,
dirty-page math is correct (edit → `[0]`, no-op edit → `[]`), a
failed apply keeps every read serving the last-good compile, and the
canvas paint/DPR contract needed no changes on the consumer side.

## Friction, by severity

### 1. `plate_file` hard error bricks every published quill

`quill::unknown_key` on `plate_file` under `quill:` makes 0.93 refuse
to LOAD every quill published before it — all four production quills
(`usaf_memo`, `af4141`, `daf1206`, `daf4392`) fail at
`Quill.fromTree` until their `Quill.yaml` moves the key into `typst:`.
Published bundles are content-addressed and immutable, and documents
pin quill refs, so a deployment cannot roll wasm 0.93 without
re-publishing its entire catalog in the same release. The spike
patched the quill sources locally to proceed.

**Ask:** accept `quill.plate_file` for one era (warning diagnostic,
value treated as `typst.plate_file`) so engine and catalog can roll
independently.

### 2. Region coverage on the real catalog is one field

`session.regions()` on the flagship `usaf_memo` template returns
exactly `[tag_line]`. Two independent causes:

- **Content auto-tag does not survive content-transforming packages.**
  The memo package's `render-body` rebuilds body paragraphs through a
  state buffer (AFH 33-337 auto-numbering) and re-emits them; the
  zero-size `qm-region` metadata markers wrapping `$body` are not
  re-emitted, so the body — the field a user most wants to click —
  produces no region. The "no plate-author effort" contract holds only
  for plates that place content verbatim.
- **Scalar fields never tag.** `subject`, `memo_for`,
  `signature_block`, … are strings/arrays-of-strings; a string carries
  no label, so only an explicit `form-field`/`signature-field` binding
  can region them — and no production plate binds any.

Net: the cross-navigation feature works mechanically but is inert on
the shipped catalog. **Asks:** document the marker-preservation
contract for package authors (re-emit non-par children when rebuilding
content); a build/validate-time probe that warns when a quill's
content field yields no region for its seeded document; consider
helper-level scalar tagging (e.g. `tagged(data.subject, "subject")`
wrappers a plate opts into) so scalars can region without full
form-field widgets.

### 3. `Document` lifetime across `engine.open`/`render` is a footgun

The runtime clones the document (`doc.toJson()`) AFTER awaiting the
backend load, so the natural ownership shape

```js
try { return engine.open(quill, doc); } finally { doc.free(); }
```

is a use-after-free — an uncatchable-looking
`null pointer passed to rust` panic on the first render (later
renders hit the already-loaded backend and a narrower race). The docs
do say the caller owns the handle, but nothing says "…until the
promise resolves."

**Ask:** snapshot `doc.toJson()` synchronously (before the first
await) in `runtime.js` `render`/`open`/`LiveSession.apply`. Documents
are small; the copy makes the obvious calling pattern correct.

### 4. `apply` has no warnings channel

`LiveSession.warnings` is attached at `Backend::open` and never
refreshed; `ChangeSet` carries no diagnostics. An edit that introduces
a non-fatal condition mid-session (font fallback, overfull page, a new
`validation::must_fill`) has no path to the preview — web-app's
warnings overlay silently shows open-time state forever. Consumers
must run a separate `quill.validate(doc)` per edit to fake it.

**Ask:** either `ChangeSet.warnings` (per-compile diagnostics) or
define `session.warnings` as "warnings of the current compile" and
refresh it on every committed apply.

### 5. From-source builds claim the previous version

`pkg/package.json` stamps the workspace version — the *last released*
0.92.1 — onto a build containing 0.93 semantics. npm dedupe/peer
checks and humans debugging a spike both read the wrong number.
**Ask:** `build-wasm.sh` stamps a prerelease marker (e.g.
`0.92.1-dev.<sha>`) for non-release builds.

## Notes (no action asked)

- `apply` is synchronous on the main thread. Keystroke-cadence Typst
  compiles block the UI for their duration; web-app yields a macrotask
  first and lives with it. A worker/OffscreenCanvas story stays the
  consumer's problem — fine for now, worth a canon note.
- The editor re-derives the `$cards.<kind>.<n>.` prefix (kind-scoped
  ordinal) from its card list on every focus/reorder; the grammar is
  now implemented three times (Typst prefix builder, pdfform resolver,
  web-app `field-path.ts`). A `document.fieldPath(cardIndex, field)`
  accessor would collapse the drift surface.
- The DAF form quills are Typst recreations, not `pdfform` quills, so
  the pdfform widget-region path has no production consumer yet.
- Toolchain: the build script's pinned `wasm-bindgen-cli 0.2.118`
  no longer matches ambient installs (0.2.122 here); the version check
  fires late (after a full cargo build). Cheap improvement: check the
  CLI version before compiling.

## Web-app branch caveats

- `package.json` depends on `file:../quillmark/pkg`; CI cannot
  `npm ci` until `@quillmark/wasm@0.93.0` publishes. Swap the dep and
  delete this caveat at release.
- `static/quills` on that branch is repacked from locally-migrated
  `Quill.yaml`s (friction #1); the published `@airmark/quiver` package
  still carries the 0.92 layout and must be re-released alongside.
- `live-session.integration.test.ts` pins today's region coverage
  (`tag_line` only, no `$body`) — it starts failing the moment
  friction #2 improves, which is the point.
