# Quillmark WASM

WebAssembly bindings for Quillmark.

Maintained by [TTQ](https://tonguetoquill.com).

## Overview

Quillmark in browsers and Node, over explicit in-memory trees
(`Map<string, Uint8Array>` / `Record<string, Uint8Array>`).

The package has one import surface: `@quillmark/wasm`, whose `init` resolves to
`Quill` and `Document`, plus an `Engine` that renders them.

`Quill` and `Document` are the internal Typst-less core build's own classes,
handed out verbatim by `init`, so editor/validation code (`Quill.fromTree`,
`Document.fromMarkdown`) loads only that small core binary: no backend is
loaded until you render. The `Engine` hides everything else: each backend
(`typst`, `pdfform`) is a separate, private WASM binary with its own linear
memory, lazily loaded on the first render. The Engine clones a `Quill` /
`Document` into the backend's memory as data and frees the clones: you never
hold a backend object or cross a memory boundary yourself.

## Build

```bash
bash scripts/build-wasm.sh
```

The script builds three variants: the core (no backend), the Typst backend
(default features), and the Typst-free pdfform backend (`pdfform` feature):
each with `--target web` and `--weak-refs` enabled (see
[Initialization](#initialization) and [Lifecycle](#lifecycle)). It then asserts
none of them carries a `.wasm` ESM import or a top-level await.

## Test

```bash
bash scripts/build-wasm.sh
cd crates/bindings/wasm
npm install
npm test
```

## Usage

```ts
import { init, Engine } from "@quillmark/wasm";

const { Quill, Document } = await init(); // see Initialization

const quill = Quill.fromTree(tree);   // no engine needed: build + validate
const engine = new Engine();          // loads a backend lazily on first render

const markdown = `~~~
$quill: my_quill
$kind: main
title: My Document
~~~

# Hello`;

const parsed = Document.fromMarkdown(markdown);
const result = await engine.render(quill, parsed, { format: "pdf" });
```

## Initialization

`init` resolves to the core surface: `Quill`, `Document`, and the free
functions. Everything after the await is the synchronous surface the rest of
this README describes.

```js
import { init, Engine } from "@quillmark/wasm";
const { Quill, Document } = await init();
```

The same line works everywhere: the binary streams from a URL in a browser and
is read off disk under Node, chosen by the package's `#quillmark-env` subpath
import rather than a runtime environment check. No bundler plugin is required:
the builds are `--target web`, so nothing in the package graph imports a `.wasm`
module or carries a top-level await, and a static `import` of this package is
safe anywhere, SSR included.

`init` is idempotent and concurrency-safe: every non-conflicting call returns
the same promise, so several entry points may each `await init()` for one
instantiation. Destructure at **every** entry point (route loader, hydration
path, worker) rather than threading one result around. A failed init clears the
memo, so a retry works. Each realm initializes its own copy, a Worker included.

**Backends need nothing.** `Engine` instantiates a backend inside its lazy load,
on the first render against it.

**Overriding the source.** `init(source)` accepts bytes, a `Response`, a
`WebAssembly.Module`, or a URL, for hosts that route assets themselves or embed
the binary. Pass it on the first call; a later call passing a *different* source
rejects with `runtime::init_conflict` rather than silently ignoring it. Passing
the same value again is fine, so several entry points may each
`await init(BYTES)` against one constant.

**Both failures reject.** `runtime::init_conflict` and `runtime::init_failed`
alike ride the returned promise, so one `catch` around `await init(...)` covers
the gate. The core surface has no static export, so a call site that skips the
await has no name to call.

**Vite's dev server** pre-bundles dependencies, which moves the package away
from its binary. Exclude it:

```js
// vite.config.js
export default { optimizeDeps: { exclude: ["@quillmark/wasm"] } };
```

A load failure surfaces as `runtime::init_failed`, whose hint names that line.

## API

### `new Engine(options?)`
Create the render dispatcher. Routes each quill to its backend by
`quill.backendId`, lazily loads that backend binary, and renders: cloning the
quill/document into the backend's memory and freeing the clones internally.
`render`, `open`, `supportedFormats`, and `supportsCanvas` are **async** (the
first call may load a backend). Pass `{ backends }` to register or override
backend descriptors. Each entry is a descriptor
(`{ [backendId]: { load, formats, canvas } }`) where `load` is the lazy thunk
returning the backend module and `formats`/`canvas` are the **required** static
capability manifest. A malformed descriptor throws at `new Engine(...)`, naming
the backend id.

**Capability probes are always free.** `supportedFormats` and `supportsCanvas`
depend only on `quill.backendId`, and answer from the descriptor's required
`formats`/`canvas` manifest: never loading the multi-MB backend binary and
never cloning the quill. Use them as non-failing pre-render probes.

### The two doors: `Document.fromMarkdown` vs `quill.parse` / `quill.conform`

`Document.fromMarkdown` is the quill-free **transport door** (migrations, `$ext`
stamping, a quill that will not load, opening a document to fix its `$quill`).
It needs a root `~~~` block carrying a `$quill` line, and a content field rests
as authored.

`quill.parse` is the **bound door**, and the primary ingestion path. It is
`Document.fromMarkdown` followed by `conform`: the returned document's declared
content fields rest at one form per codec (a `richtext` field as the canonical
content object, a `plaintext` field as its literal string), so `getStored`
answers "content object or string?" by the field's declared codec rather than by how the
document was built. Parse warnings and the `conform::*` warnings both ride
`doc.warnings`.

`quill.conform(doc)` is the same walk in place on a document that arrived any
other way (`fromStored`, a stored row), returning the `conform::*` `Diagnostic[]`
(`[]` when everything rested). It is idempotent and a byte no-op on an
already-canonical document, YAML comments included, so calling it on every load
is safe. A `!must_fill` marker anywhere in a field's value skips that field; a
value the strict write refuses stays as authored under a warning. Both throw
when the document declares a `$quill` this quill does not answer to, before any
mutation.

```ts
const doc = quill.parse(markdown);          // rests canonical
const stale = Document.fromStored(row);
const diags = quill.conform(stale);         // converges in place
```

### Storage compatibility across versions

Persist `doc.toStored()`, not `doc.toMarkdown()`: the DTO wire format is frozen
per `schema` version, whereas Markdown syntax evolves, and `toMarkdown` output
is normalised rather than byte-equal to the source. `Document.tryFromStored`
discriminates the two formats without exceptions as control flow:

```ts
const doc = Document.tryFromStored(content) ?? Document.fromMarkdown(content);
```

The `schema` value (`quillmark/document@0.112.0`) is the **model version**,
not the running crate version. It is a hand-set constant, bumped only when
the `Document` model itself changes, so every `0.112.x` patch release reads
and writes that same value.

- **Upgrading is safe.** A newer build always reads documents written by an
  older one. Each schema version's wire format is frozen and never changes;
  when the model does change, the new build ships a migration that converts
  old payloads on `fromStored`. A document you commit as your canonical
  on-disk format keeps loading across crate upgrades: there is no need to
  pin old wasm to read old data.
- **Downgrading is not.** `fromStored` rejects an *unknown* (i.e. newer)
  `schema` version rather than guessing at a format it predates. Don't feed
  documents written by a newer build back into an older one.

To detect a version mismatch before parsing, use the static accessors:

```ts
const v = Document.storageVersionOf(blob); // undefined | string
if (v && v !== Document.currentStorageVersion()) {
  // payload is from a build with a different model version
}
```

`storageVersionOf` does not validate the payload: it only reads the
`schema` field, returning `undefined` for non-JSON, non-objects, or
payloads that don't carry one. Use it to distinguish "wrong version" from
"corrupt" when `fromStored` throws.

In short: persist the `toStored` string, upgrade freely, never downgrade. The
full design (including how migrations are added) is in
`prose/canon/DOCUMENT_STORAGE.md`.

### Cards, seeds, and addresses

To render a form editor, read field definitions from `quill.schema` (walk
`fields` in key order: declaration order is display order) and the authored
values from the `Document` payload: there is no separate form-view projection.
`quill.validate(doc)` scores it without invoking the backend.

`quill.seedDocument()` returns a starter document with each field's `example:`
committed; `quill.seedMain()` and `quill.seedCard(kind)` seed one card. All
return the read `Card` shape of `doc.main` / `doc.cards`, which `doc.insertCard`
accepts directly:

```ts
doc.insertCard(quill.seedCard("note"));                 // seed → append
doc.insertCard(Document.makeCard("note", { x: 1 }));    // build from a flat map
doc.insertCard({ kind: "note", body: "Plain **markdown**." });  // bare inline
doc.insertCard({ kind: "note" }, 0);                    // insert at index 0
```

Reads and writes are two aligned shapes. A read `Card` always has `body:
Content` (canonical content, never a raw string): no narrowing, no guessing
whether the body was normalized. The write shape `CardInput` widens `body` to
`Content | string` (a markdown string imports to the content) and makes every
field but `kind` optional. Every `Card` is a valid `CardInput`, so `insertCard`
still takes exactly what `cards` / `removeCard` / `seedCard` return.
Build a fresh card from a flat field map with
`Document.makeCard(kind, fields?, body?)`.

**One address for the whole surface.** Reads and writes navigate by an `Addr`:
`{ card?, field? }`, absent `card` = main, absent `field` = body, and a bare
string is shorthand for `{ field }`. So `doc.storeField("qty", 3)` targets the
main card's `qty`, `doc.storeField({ card: 2, field: "qty" }, 3)` a composable
card's. Reads are total over the field axis (`getStored` → `undefined`, `isFill` → `false` for
an absent field; only an out-of-range card throws); field writes throw on a body
address. `getStored` is the verbatim transport read, distinct from the interpreted
`quill.reader(doc).get`; `bodyMarkdown` is the body markdown read (a `CardAddr`; a field's
markdown is read through `quill.reader(doc).get(field)`). A content field's stored
form follows how the document was built (a canonical content object when the
typed writer committed it, the authored string when a markdown parse produced
it), so for the `Content` either way read `quill.reader(doc).getContent(addr)`, which
decodes through the codec the field's declared type names. Card-scoped verbs take a
`CardAddr` (`{ card? }`) first: `doc.getExt({ card: 2 })`, and the batch below.

Batch mutation: `doc.storeFields({}, {...})` / `doc.storeFields({ card: index }, {...})`
apply a whole object atomically: on any invalid field nothing is applied and
the thrown error carries one diagnostic per offending field (`path` = field
name). The address is first (never shape-overloaded, since `card` is a legal
field name), and parses strictly: a stray key throws rather than silently
reading as `{}`. The main card is `{}`, or **`MAIN_CARD_ADDR`** (from
`@quillmark/wasm/runtime`), a frozen alias that spells the intent:
`doc.storeFields(MAIN_CARD_ADDR, {...})`.

### Typed writes: `commit*` is the default, `store*` is the quill-free primitive

A `Document` holds only a `$quill` *reference*, not the resolved schema, so typed
writes go through the schema-bound writer while the quill-free opaque store sits
on `Document` itself (**store** = verbatim, **set** = typed):

- **`quill.writer(doc)`: the typed door whenever a quill is in hand.** Bind the
  schema once and issue bare `set` / `setAll` / `reviseBody` / `reviseField` /
  `addCard` / `card(i)`. Each resolves the field's schema `type`, coerces the
  value to its canonical form (`"3"` → `3`, a markdown string → a richtext
  content), and **fails now** on a mismatch instead of at render. A name the schema
  does not declare throws `UnknownField` rather than falling to the opaque store:
  on the typed path an undeclared name is a typo, not a fallback. The batch form
  (`setAll`) is all-or-nothing: an undeclared name aborts the whole write and its
  per-field diagnostics name every offending field, so a whole-form submit
  surfaces every typo `storeFields` would silently absorb. (The raw wasm class
  carries the quill-taking `_commitField` / `_commitFields` / `_addCard` /
  `_reviseField` ABI the writer delegates to, hidden from the `.d.ts`.)

- **`store*`: the deliberate quill-free primitive.** `doc.storeField(addr, value)`
  / `doc.storeFields(cardAddr, {...})` (and `storeFill`) validate only the field
  name/depth/kind and store the value verbatim, no quill required. Reach for it
  on purpose when you *want* the opaque store: quill-agnostic storage/migration
  infra that has no bundle and must write regardless of a drifted schema;
  store-now-validate-later editors holding in-progress input that `commit`
  would reject; or verbatim passthrough of fields the schema doesn't own. It is
  the lower layer, not a lighter `commit`: a typo'd field name stores silently
  and only surfaces at `quill.validate` / render.

Per-keystroke cost is the same either way (both mutate the in-memory `Document`
in place; no seam is crossed), so steering to the writer buys the type check for
free.

#### `DocumentWriter` / `CardWriter`: bind the quill once

`quill.writer(doc)` binds the quill's schema to the document once, so a form
editor or MCP writer that holds both issues bare verbs (the writer forwards to
the per-call `_commit*` ABI):

```ts
const ed = quill.writer(doc);                       // Rust `quill.writer(doc)` twin; new DocumentWriter(quill, doc) also works
ed.set("subject", "Q3 results");                    // strict-committed to the schema type
ed.setAll({ qty: "3", subject: "Q3" });             // all-or-nothing batch
ed.reviseField("subject", "Q3 **results**");        // typed AND anchor-preserving; returns a Delta
ed.set("titel", "x");                               // throws UnknownField: a typo, not a fallback
ed.card(2).set("body", "**note**");                 // composable card, resolved by its $kind
ed.setValues({ fields: { subject: "Q3" } });        // the values form: a present axis replaces, an absent one is untouched
ed.card(2).setValues({ fields: { body: "**note**" } });
```

`DocumentWriter` / `CardWriter` are pure JS holding references to your existing
`quill` and `doc`: no WASM handle of their own, nothing to `free()`. `card(i)`
is lazy: it never throws; an out-of-range index throws `IndexOutOfRange` at the
write.

#### `DocumentReader` / `CardReader`: the read twin

`quill.reader(doc)` carries the writer's ephemerality and schema authority:

```ts
const v = quill.reader(doc);
v.get("subject");                                   // the values form: every content leaf as its codec's text, else as stored
v.getContent("subject");                            // the same read as a `Content`, whichever lane stored it
v.bodyMarkdown();                                   // the main body markdown (quill-free)
v.card(0).get("body");                              // a card field, resolved by its $kind
v.values();                                         // the whole document in the values form; ed.setValues(v.values()) is a no-op
v.card(0).values();                                 // one card in it
v.resolve();                                        // the render view: blank-filled, coerced, each field tagged with its rung
```

`get` projects and `getContent` returns the `Content`; both decode through the codec
the field's **declared type** names, which is why they bind the quill and the
verbatim `doc.getStored` does not. An undeclared name throws `UnknownField`, a
type that is not a content leaf throws `FieldNotContent`, and an undecodable
value throws `FieldDecode`; an absent field reads back `undefined` and a
present-null `null`. A read never coerces a scalar (`qty: "3"` reads `"3"`);
`resolve()` is the coerced view. `values()` is total where `get` throws.

### `engine.render(quill, parsed, opts?)` vs. `engine.open(quill, parsed)`

Use **`engine.render`** for one-shot exports (PDF/SVG/PNG): compiles, emits
artifacts, done. Use **`LiveSession`** (returned by `engine.open`) for
reactive previews: the session is a persistent compiler. `paint` / `render` /
`regions` / `fieldAt` read its current compile without recompiling, and `apply(doc)`
recompiles in place on each edit, returning a `ChangeSet` whose `dirtyPages`
tells you which pages to repaint (`dirty ∩ visible`). Apply is transactional:
on throw, every read keeps serving the last-good compile. Don't open a session
per export, and don't re-open per edit: `apply` instead.

A document that compiles to zero pages still produces a valid session
(`pageCount === 0`); `paint(ctx, 0)` and `pageSize(0)` then throw. Branch on
`pageCount === 0` to render a "no pages to preview" UI rather than relying on
the throw.

Their `warnings` differ in reach. `engine.render` returns one list for the
whole pipeline: `doc.warnings` (parse, `conform::*`,
`plate::unsupported_construct`) ahead of the compile's own. A session outlives
the document it opened from, so `session.render` and `session.warnings` carry
the compile half alone — read `doc.warnings` beside them.

### Canvas Preview

`session.paint(ctx, page, opts?)` rasterizes a page directly into a
`CanvasRenderingContext2D` (main thread) or
`OffscreenCanvasRenderingContext2D` (Worker), skipping PNG/SVG byte
round-trips.

The painter owns `canvas.width` / `canvas.height`: it sizes the backing
store itself. Consumers own `canvas.style.*` (or the layout system that
sets them) and read `layoutWidth` / `layoutHeight` from the returned
`PaintResult`.

```ts
const result = session.paint(canvas.getContext("2d"), 0, {
  layoutScale: 1,                            // layout px per pt (page geometry unit)
  densityScale: window.devicePixelRatio,     // backing-store density
});

canvas.style.width  = `${result.layoutWidth}px`;
canvas.style.height = `${result.layoutHeight}px`;
```

- `layoutScale` sets the display-box size (`layoutWidth = widthPt * layoutScale`);
  fold `devicePixelRatio`, in-app zoom, and `visualViewport.scale` into
  `densityScale`. Their product is the rasterization scale, clamped at 16384 px
  per side (`result.clamped`, `result.effectiveDensityScale`).
- `paint` writes the whole backing store with `putImageData`, which ignores the
  2D context transform, `globalAlpha`, and clip. Give each visible page its own
  `<canvas>`: no compositing, sub-rect, or transform reaches through `paint`.
- `paint` is always a full repaint, and there is no per-page raster cache. Keep
  a page's canvas alive while it stays near the viewport: an idle canvas retains
  its pixels for free, whereas pooling one canvas across pages re-renders on
  every scroll.
- `pageCount` and `pageSize(page)` are stable for the session's lifetime: cache
  them.
- In a Worker, pass an `OffscreenCanvasRenderingContext2D`; the layout
  dimensions are informational there. Loading the WASM module inside the Worker
  is the host's responsibility.
- Backend support is gated by `supportsCanvas`. Probe upfront with
  `engine.supportsCanvas(quill)`; the throw on `paint` / `pageSize` remains the
  enforcement contract and names the resolved `backendId`.

### Schema model

A field carries two independent axes, and no `required` one.

**Value** — what the cell holds. With a `default:`, `quill.blueprint` renders
that value under a type-only `# <type>` annotation and the render path uses it
when the document omits the field. Without one, an `example` takes the cell as
a suggested value, and an absent field blank-fills.

**Obligation** — whether a human must author the field, read off `default:`'s
absence: a defaulted field asks nobody, a defaultless one asks. An obliged
field carries the `!must_fill` marker in `quill.blueprint`, and
`quill.validate(doc)` emits the non-fatal `validation::must_fill` warning while
the document leaves it unauthored — from either of two triggers, named by the
diagnostic's `trigger` arg: `marker` for a marker the document still carries,
`unauthored` for a cell the schema obliges and the document never filled.
Authoring the field's blank discharges the obligation; clearing the key does
not.

Neither axis gates render. Partial documents are accepted, and
`engine.render(quill, doc)` throws only for malformed input.

### Errors

Every method that can fail throws a **`QuillmarkError`**: a JS `Error` with
`.diagnostics` attached. The type and a guard are exported from the root:

```ts
import { isQuillmarkError, type QuillmarkError } from "@quillmark/wasm";

try {
  const result = await engine.render(quill, doc);
} catch (e) {
  if (isQuillmarkError(e)) {
    for (const d of e.diagnostics) console.error(d.severity, d.message);
  } else {
    throw e; // not a quillmark failure: programming error, re-throw
  }
}
```

**Delivery follows the function, not the failure.** A synchronous method throws;
a promise-returning one rejects. The promise-returning surface is `init` and the
four `Engine` verbs (`render`, `open`, `supportedFormats`, `supportsCanvas`), so
a programming error reached through one of them (a foreign handle, an
unregistered backend) rejects like any other failure. Nothing here both returns
a promise and throws, so a `.catch` on a promise-returning call is a whole
guard.

`QuillmarkError` is a **structural interface, not a class**: the WASM layer
throws a real `Error` and attaches the property, so there is no constructor to
`instanceof` against. Narrow with `isQuillmarkError`, which also works on errors
from any build or WASM instance in the page.

`diagnostics` is always non-empty: length 1 for most failures, length N for
backend compilation errors, and `message` is derived from it. The same shape
applies to every throw site:

- `Document.fromMarkdown`: parse errors (missing root `$quill` metadata, YAML
  errors, `parse::input_too_large` for inputs > 10 MiB).
- `Document` mutators (`storeField`, the writer's `set`, etc.): mutator
  failures carry a namespaced `edit::*` `code` on `diagnostics[0]`
  (`edit::invalid_field_name`, `edit::unknown_field`, `edit::index_out_of_range`,
  `edit::field_coercion_failed`, …). Route on `diagnostics[0].code`, never on message
  text.
- `engine.render` / `session.render`: backend compilation failures and
  validation errors.
- `engine.render(quill, parsed)` against a quill whose *name* differs
  (`quill::name_mismatch`) or whose *version* falls outside the document's
  selector (`quill::version_mismatch`): a throw, never a warning.
- Any method taking a `Quill` or `Document`: a handle from a *second* copy of
  `@quillmark/wasm` is refused with `runtime::foreign_handle`, hinting `npm ls
  @quillmark/wasm`. Two copies are two WASM memories and two `Quill`/`Document`
  classes; dedupe to one. A value that is not a handle at all keeps its own
  `runtime::not_a_document` / `runtime::not_a_quill`.

### Lifecycle

Handles begin at [`init`](#initialization), which instantiates the core build;
`Engine` instantiates a backend on the first render against it.

The wasm bindings are built with `--weak-refs`, so dropped `Document`,
`Quill`, and `LiveSession` handles are reclaimed by `FinalizationRegistry`
without manual `.free()` discipline. `.free()` is still emitted as an eager
teardown hook for callers that want deterministic release.

`engine.render` and `engine.open` read the `quill` and `doc` handles
synchronously, before their first await, so freeing a handle as soon as the
call returns: `try { return engine.render(quill, doc); } finally
{ doc.free(); }`: is safe even on the first render, while the backend
binary is still loading.

The package floor is Node 24+ (`engines: { node: ">=24" }`) and current
evergreen browsers; `--weak-refs` itself only needs Node 14.6+. The `using`
sugar ([explicit resource management][erm]) is on that floor and optional;
an explicit `try` / `finally` is the equivalent, and the form that also runs
in a browser that hasn't shipped it:

```ts
const session = await engine.open(quill, doc);
try {
  for (let p = 0; p < session.pageCount; p++) {
    session.paint(ctx, p);
  }
} finally {
  session.free();
}
```

[erm]: https://github.com/tc39/proposal-explicit-resource-management

## Changelog

See the [changelog](https://github.com/borb-sh/quillmark/blob/main/CHANGELOG.md)
and the [GitHub Releases](https://github.com/borb-sh/quillmark/releases) page for
release notes and version history.

## License

Apache-2.0
