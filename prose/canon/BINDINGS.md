# Bindings

> **Implementation**: `crates/bindings/`

## TL;DR

Quillmark exposes one core engine through several language surfaces: Python (PyO3), WebAssembly (wasm-bindgen), and a CLI binary. Every surface drives the same `quillmark` core: the same `Document`/`Quill`/`Card` model, the same `serde` diagnostics, and the same capability principle. Surfaces differ only in language idiom, packaging, and which extras they expose (canvas preview is WASM-only).

## Shared model

- **Capability principle.** A `Quill` is portable, declarative config data. Its format capability (`supportedFormats`) and rendering are resolved by the `Quillmark` engine *against* a quill at render time: never by the quill itself. So `quill.metadata` is a pure, infallible config snapshot, while `render` / `supportedFormats` can fail for an unregistered backend.
- **One model, serialized across every boundary.** The `Document`/`Card` model and `Diagnostic`s cross each language boundary as the same core `serde` shapes (`CardWire`, the versioned storage DTO, `Diagnostic`), so a card or an error reads identically no matter which surface emits it. See [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [CARDS.md](CARDS.md), [ERROR.md](ERROR.md).
- **Uniform errors.** Each binding raises a single error type that always carries a non-empty diagnostic list (`QuillmarkError.diagnostics` / thrown `Error.diagnostics`).

The WASM binding is the reference surface; Python mirrors it within its scope (Tier 1 + storage + render; see the scope note below). New contract work lands in WASM first.

## The write surface: object placement over one primitive

Placement decides where a mutation verb lives, in one sentence:

> **If a verb needs a schema, it lives on the writer. `Document` is quill-free data.**

`quill.writer(doc)` (mirroring core's `quill.writer(&mut doc)`) is the one schema-bound door: bare `set` / `set_all` / `reviseBody` / `reviseField` / `addCard` / `card(i)`, names and markdown in, diagnostics out. It resolves each field's type from the bound quill, so a name the schema does not declare is a typo (`UnknownField`), not a fallback.

`Document` holds everything quill-free: the opaque `store*` primitive (verbatim, coercion deferred to render) and the addressed content lane: `overwrite` / `revise` / `applyChange` plus the `importMarkdown` / `exportMarkdown` / `rebase` / `mapPos` / `mapMarks` codec: which navigate by `Addr` and return `Delta` receipts but never consult a schema. **Transport** reads (`getStored` / `isFill` / `getExt`) return the stored value verbatim, need no schema, and sit on `Document` too.

The **interpreting** reads (reading a field by its type) are schema-shaped questions ("this field's richtext, as markdown"), so they gain a schema-bound home: `quill.reader(doc)`, the read twin of `quill.writer(doc)` (mirroring core's `quill.reader(&doc)`).

`reader.get(addr)` reads each field in the values form: every content leaf in its type tree to its codec's text (a `richtext` leaf to markdown, a `plaintext` leaf to its literal text, marks verbatim), everything else as stored, a present-null as `null`: with schema authority, so a name the schema does not declare throws `UnknownField` instead of reading back `undefined`, and a content leaf holding an undecodable value throws `FieldDecode`. `reader.values()` is the same read over the whole document, total where `get` throws.

`reader.getContent` is the same read at the other end of the codec, returning the **`Content`** rather than the projection: it makes a content field's storage form an implementation detail instead of a caller's branch, and it still spans the transport door's as-authored rest.

**The bound door is the primary ingestion path.** `quill.parse(md)` is parse-then-conform, and `quill.conform(doc)` is the same walk on a document that arrived any other way; both live on the quill because both need the schema. A document that came through either rests at its canonical form, one per codec ([SCHEMAS.md](SCHEMAS.md) § "Content fields rest per codec"): a `richtext` field as the `Content` object, a `plaintext` field as its literal string, so `getStored` answers "content object or string?" by the field's declared codec rather than by how the document was built.

The schema-free `Document.fromMarkdown` remains the transport/repair door (migrations, `$ext` stamping, a quill that will not load, opening a document to fix its `$quill`); the resting form of what it returns is unspecified, and the next bound load converges it.

Across the content fields the walk covers, the exception states are named, never silent: no quill means as-authored rest, the wrong quill throws before any mutation, a content-field value the strict write refuses stays authored under a `conform::*` warning, and a `!must_fill` marker anywhere in a field's value skips that field, the marker being the state.

A field whose type tree bears no content leaf is outside the walk, so conform is silent about it by construction: a scalar's shorthands are the typed write's to canonicalize, and a stored value its grammar refuses (a wall clock in a `date`) is `quill.validate`'s to report. A clean conform says the content fields are at rest, not that every field would commit.

Decoding needs the schema and not the payload: a `richtext` string is markdown and a `plaintext` string is literal text, so the same stored bytes decode two ways and only the declared type says which. That is why the `Content` read binds the quill instead of sitting beside `getStored`: a quill-free version would guess a codec, and would guess markdown.

**A `Content`-typed read is also a write input, so the seam spells every `Container.instance`.** Storage omits a zero, since a row written before the field existed then re-encodes byte for byte ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Container identity").

A read inheriting that omission cannot be typed with the field required. That left `overwrite(addr, importMarkdown(md))` and `CardInput.body` unable to report a container path missing the discriminator its writer owes.

Which form a lane carries follows its declared type, not its direction:

| Form | Lanes |
|---|---|
| Seam: every `instance` spelled | `reader.getContent{,At}`, `getStored` on a body, `importMarkdown`, `rebase`, and the `Card` wire behind `document.main` / `cards` / `card(i)` / `removeCard` / `makeCard` / `seedMain` / `seedCard` |
| Storage: a zero omitted | `getStored` on a field, `payloadItems` — the stored bytes verbatim, which is why both are typed `unknown` |

The two forms decode identically.

Required is not correct. A checker reports a container that omits the field; it cannot report a `0` stamped on every run, which is the write that welds them. `assignInstances` is the rule, not the type.

A field's markdown lives here: `Document.bodyMarkdown` is **body-only** (it takes a `CardAddr`; a present `field` throws), and the quill-free **body** projection stays on `Document` (a body's type is a format fact, not a schema fact, so `reader.bodyMarkdown` mirrors it rather than gating it on the schema). One name for one projection, on every surface: core's `Card::body_markdown`, `doc.bodyMarkdown`, `reader.bodyMarkdown`. The placement rule generalizes: *a verb that needs a schema lives on the writer (writes) or the view (reads); `Document` is quill-free data.*

The rule scales from a field to the document without a new receiver: `reader.values()` / `writer.setValues(v)` read and write the whole document in the values form, `reader.card(i).values()` / `writer.card(i).setValues(v)` one card, and `reader.resolve()` is the render view beside `values()` ([SCHEMAS.md](SCHEMAS.md) § "The values form"). `quill.parse` / `quill.validate` / `quill.conform` stay on the quill as its ingestion and verdict verbs: a constructor, a diagnostic list, and `parse`'s in-place twin, none of them a read or write of a value.

`reviseField` is the writer verb that is both typed *and* anchor-preserving: it rebases surviving anchors like the content `revise`, then conforms the diffed result to the field schema like `set`. Because it needs the schema, it lives on the writer: wrapping core's `Card::revise_field_checked` primitive, which `Document` does not expose.

**The writer is the only typed door, core included.** The typed primitives (`Card::commit_field`, `Card::revise_field_checked`) are `#[doc(hidden)]`, unpromised on the same terms as the other hidden items ([COMPATIBILITY.md](COMPATIBILITY.md)). A resolved `FieldSchema` argument is the only thing that tells them from their opaque and schema-blind neighbours, and disambiguating by argument is a third mechanism beside the receiver and the verb: one the rule below does not name and a caller cannot see.

**The verb carries the lane.** One vocabulary rule, stated once here:

| Verb | Lane | What it does |
|---|---|---|
| **store** | verbatim | the quill-free opaque write; coercion deferred to render |
| **set** | typed | the writer's strict commit at the write. `set` writes one field, `set_all` merges a batch, `set_values` replaces the values form per present axis, at card or document scope |
| **overwrite** · **revise** · **apply** | content | identity-aware, and a ladder by **anchor fate** |

The content lane's three verbs are rungs, and what sorts them is the fate of the identity anchors already on the value:

- **overwrite** *destroys* them. Value semantics: store exactly this content, no import, no diff. A `toMarkdown → overwrite` round-trip cannot resurrect an anchor, which is why the cold-import path is spelled `overwrite(addr, importMarkdown(md))` at the call site, where the loss is visible.
- **revise** *rebases* them. Import the markdown, diff against the current value, carry surviving anchors onto the new text, hand back the `Delta`.
- **apply** *preserves* them. Splice ops into the content in place; anchors and island ids ride the splice because the text around them is what moved.

**The rebase rule.** Every channel that moves text (`delta`, `islandOps`, `lineOps`) rebases the marks already in the field by one assoc rule, and `revise` carries surviving anchors by the same biases:

| mark edge | assoc | an insertion at that exact position |
|---|---|---|
| a range's `start` | `after` | grows text outside the span |
| a range's `end` | `before` | grows text outside the span |
| a zero-width mark | `before` | leaves the mark put |

The last row is the only position where the two assocs differ, and where an anchor most often sits.

`markOps` are written in the coordinates that rebase produces. Left as prose, the rule is a prediction every editor bridge reimplements to decide which ops to emit; `mapMarks(content, bundle)` runs it instead, and a bridge diffs its intended marks against what it returns. Both readings walk one channel list (`Content::apply_text_channels`) and normalize, so the store and the prediction cannot answer a position differently, nor hold a different set of marks where a move left two same-kind runs touching.

**The rule governs user-field writes.** System metadata (`store_ext` / `store_seed_*`), structural operations (`insertCard` / `moveCard` / `setCardKind`), and the `remove_*` family sit outside it: `remove_*` has no lane because one verb serves every write path, and a structural op moves cards rather than writing into one. So `store_field` / `store_fields` / `store_fill` are the opaque store, `set` / `set_all` the typed writer, and a name never needs per-verb disambiguation against its neighbor (the opaque batch `store_fields` and the typed batch `set_all` are not near-homographs).

**Why the verb carries it when a receiver could.** Where a receiver exists it names the lane too (`doc.storeField` vs `writer.set`), so the verb looks redundant. It is not, on a design choice rather than a law: the reference surface (WASM `Document`) **flattens all three lanes onto one class**, and the parity table below requires a binding verb be identical to its core twin. A verb leaning on its receiver would need a different spelling on the flattened surface, and the table would have to admit a fourth difference class to hold it. Change the flattening and this rule loses its load.

The body verbs follow from the same rule rather than from a separate one. `writer.reviseBody` is the content lane's `revise` reached through the writer: a body carries no field schema, so there is nothing for a typed verb to type, and the receipt it returns is the content lane's, not a typed lane's. The typed writer holds it because that is where a caller already is, not because the write is typed.

**The read side mirrors it.** The verbatim read is `getStored`: the read echo of `store`, and the interpreted read is `reader.get`, the reader/writer twin of `set`. So the transport and schema-plane reads carry the lane in the verb the same way the writes do, rather than collapsing both onto one `get` where only the receiver (`doc` vs `reader`) tells them apart. (Core needs no such split: its verbatim read is the map-idiomatic `payload().get`, already lexically distinct from `reader.get`; the collision is a WASM/pyo3 artifact of fusing the read onto the one `Document` handle, so only the bindings rename it.)

**`equals` is the change gate.** A consumer driving a live preview gates `apply` on structural equality against a retained clone: `if (doc.equals(last)) return; last = doc.clone()`. It covers the document the consumer did not mutate itself: one swapped in from storage, or written through a writer held elsewhere. `toStored` is byte-deterministic within a schema version, for a consumer that prefers a hashed gate.

**No revision counter.** Neither `Document` nor the session carries one ([PREVIEW.md](PREVIEW.md)). Equality answers whether this is the content last compiled. A counter answers only whether something was written, so it re-applies on a load that is content-identical to the live compile. Core cannot back one regardless: `main_mut` / `cards_mut` hand out raw `&mut`, so no bump site sees every write.

**Writers and card cursors are ephemeral: bind, write, discard.** They hold an address (the quill + document, or an index), never a cache; every call reads through the document, so a `removeCard` / `addCard` between binding a cursor and writing through it silently retargets it. A caller whose cards move re-resolves the index at write time ([PROGRAMMATIC.md](PROGRAMMATIC.md)).

**The hand-written runtime is the real API; the wasm class is its ABI.** The quill-taking `_commitField` / `_commitFields` / `_addCard` / `_reviseField` methods are the stable ABI under the writer's `set` / `set_all` / `addCard` / `reviseField`: underscored and dropped from the `.d.ts`, not from the binary. The visible `Document` class then carries zero quill-taking methods, so the split is structural, not asserted.

### Parity table

Every binding verb is *identical* to its core twin or names its one forced difference: **FFI** (a wasm-bindgen / pyo3 constraint), **idiom** (a language ergonomic), or **scope** (a lane a binding omits by intent: Python is Tier 1 + storage + render), nothing else admitted. Drift is a reviewable diff to this table.

| Concept | Core | Bindings | Class |
|---|---|---|---|
| Typed writer front door | `quill.writer(&mut doc)` | `quill.writer(doc)` | **idiom**: core holds `&mut Document` under the checker; the bindings re-borrow per call (pyo3/wasm objects carry no lifetime), so the guarantee becomes the ephemerality convention |
| Typed reader front door | `quill.reader(&doc)` | `quill.reader(doc)` | **idiom**: the read twin; same re-borrow/ephemerality as the writer |
| Interpreted read | `reader.get(name)?` → `Option<QuillValue>` in the values form (every content leaf its codec's text, else as stored); `reader.card(i)?.get(..)?` | `reader.get(addr)` / `reader.card(i).get(name)` (JS), `reader.get(name)` / `reader.card(i).get(name)` (py) | **idiom** / **FFI**: one `get` reads by declared type; absent → `undefined`/`None`, present-null → `null`/`None` (Python cannot tell the two apart here: **idiom**), unknown name → `UnknownField`, undecodable content → `FieldDecode`; a field's markdown reads here, not on the body-only `bodyMarkdown`. Body read (`reader.bodyMarkdown` / absent-field addr) stays quill-free |
| Interpreted `Content` read | `reader.get_content(name)?` → `Option<Content>`; `reader.card(i)?.get_content(..)?` | `reader.getContent(addr)` / `reader.card(i).getContent(name)` (JS), `reader.get_content(name)` / `reader.card(i).get_content(name)` (py) | **idiom** / **FFI**: the `Content` twin of the projecting `get`, decoding through the codec the declared type names, so both storage forms (committed content object, parsed string) read alike; a declared type that is not a content leaf is `FieldNotContent` (an `array<richtext>` carries content and still has no one `Content`). JS/Python return canonical Content-JSON; the absent-field addr reads the body's `Content` (JS) |
| Nested `Content` read | `reader.get_content_at(name, path)?`; `reader.card(i)?.get_content_at(..)?` | `reader.getContentAt(addr, path)` / `reader.card(i).getContentAt(name, path)` (JS), `reader.get_content_at(name, path)` / `reader.card(i).get_content_at(name, path)` (py) | **idiom** / **FFI**: the same read one axis in, `path` a `PathStep[]` / `Sequence[str \| int]` walked through the field schema's `items` / `properties` / `variants` to the leaf whose declared type names the codec; the empty path *is* `get_content`. A path naming nothing stored reads absent rather than throwing, the field axis's totality applied to the axis a repeater mutates; a bad *card* index still throws, being guarded by a count the caller holds. The path is the read's own argument rather than an `Addr` member, since `storeField` / `isFill` / `applyChange` take an `Addr` and cannot answer an element axis |
| Values read | `reader.values()` → `DocumentValues`; `reader.card(i)?.values()` → `CardValues` | `reader.values()` / `reader.card(i).values()` → a JS object (JS) / `dict` (py) | **identical**: the values form, every axis present — the fields the document carries with content leaves as their codec's text and everything else as stored, bodies as markdown, `$ext` (`null` when none), `kind` (`null` when kindless). Sparse and total; never raises. A projection, never a storage format ([SCHEMAS.md](SCHEMAS.md) § "The values form") |
| Values write | `writer.set_values(&values)?` / `writer.card(i)?.set_values(&values)?` → `Vec<(DocPath, EditError)>` on refusal | `writer.setValues(values)` / `writer.card(i).setValues(values)` (JS), `writer.set_values(values)` / `writer.card(i).set_values(values)` (py) | **identical**: an absent axis untouched, a present one replaced; all-or-nothing, one diagnostic per refused cell carrying its own `DocPath` (`main.qty`, `cards.line_item[0].desc`). A cell equal to its projection is not written, so writing back an unedited `values()` is a byte no-op. JS drops an `undefined` member before the boundary, which would fold it to `null` (**idiom**). A values object the binding cannot read as the shape is the surface's argument error: a code-less `QuillmarkError` (JS) / `ValueError` (py), as every other malformed argument is (**idiom**) |
| Resolved view | `reader.resolve()` → `Resolved` (over the `Quill::resolve` producer) | `reader.resolve()` (JS) | **scope**: the render view beside `values()`; WASM-only until a Python consumer names a call site |
| Scalar / batch write | `set` / `set_all` | `set` / `setAll` (JS), `set` / `set_all` (py) | identical |
| Body revise | `writer.revise_body(md)?` → `Delta` | `writer.reviseBody(md)` (JS), `writer.revise_body(md)` (py) | **scope**: the content lane's `revise` reached through the writer, since a body has no field schema to type against. JS returns the `Delta`; Python discards it, as on `revise_field` |
| Typed content field revise | `TypedWriter::revise_field(name, md)?` / `CardWriter::revise_field(..)?` | `writer.reviseField(name, md)` / `writer.card(i).reviseField(..)` (JS); `writer.revise_field(name, md)` / `writer.card(i).revise_field(..)` (py) | **idiom**: typed *and* anchor-preserving; both wrap `Card::revise_field_checked`. JS returns the `Delta`; Python discards it (the position-mapping receipt is an editor concern, WASM-only) |
| Card creation | `add_card(kind, fields, body?, at?)` | `addCard(kind, fields?, body?, at?)` | identical: fused make + typed-commit + insert, transactional (`at` appends when absent, else inserts at the index: one atomic positioned insert, not `addCard` + `moveCard`) |
| Card insertion | `push_card(card)` / `insert_card(i, card)` | `insertCard(card, at?)` | **idiom**: the binding folds core's append + positional-insert verbs into one; absent `at` appends |
| Card removal (writer) | `writer.remove_card(i)` | `writer.removeCard(i)` | identical |
| Card cursor | `writer.card(i)?` (eager check) | `writer.card(i)` (lazy check) | **FFI**: no borrow to validate against; the index is checked at the write |
| Cursor kind | `writer.card(i)?.kind()` | `writer.card(i).kind` | identical: the JS getter reads through `doc.card(i)` |
| Reads (value / body markdown / fill / `$ext`) | `body_markdown(..)` / `payload().get(..)` / `payload().is_fill(..)` / `card.ext()` (borrow chain; index for a card) | `doc.getStored(addr?)` / `doc.bodyMarkdown(cardAddr?)` / `doc.isFill(addr)` / `doc.getExt(cardAddr?)` / `doc.getExtNamespace(cardAddr, ns)` (JS) | **idiom** / **FFI** / **scope**: WASM fuses the transport reads onto the one `Addr` (a bare string ⇒ `{field}` for `getStored`/`isFill`) and names the verbatim field read `getStored`, not `get`, so it never collides with the interpreted `reader.get` (core's `payload().get` has no such neighbor); *total over the field axis* (absent field → `undefined`, `isFill` → `false`; only an out-of-range card throws); `bodyMarkdown` is the **body** read (a `CardAddr`; a present `field` throws). Python has no quill-free field read: interpreted field reads go through `quill.reader(doc).get`, and `$ext` / body content read off the `main` / `cards` dict snapshots |
| Reads (whole card / seed) | `card(i)` / `main().seed()` | `doc.card(i)` / `doc.seedOverlay(kind)` (JS), `doc.card(i)` / `doc.seed_overlay(kind)` (py) | **idiom**: both bindings fuse each into one named verb on `Document`, quill-free structure reads that spare the caller the whole-`cards` / whole-`main` projection; `card(i)` throws out of range, the seed read is total (no such kind → `undefined`/`None`) |
| Path mint (address → anchor) | `DocPath::main()` / `DocPath::card(kind, i)` extended by `field` / `body` | `doc.pathFor(addr)` / `doc.cardPath(i)` (JS) | **scope**: the `Addr` rendered as the `DocPath` string `Diagnostic.path` carries and the geometry queries take, so a consumer never restates the kind lookup, the `Addr` defaults or the range guard; quill-free (the stored `$kind` verbatim) and *total on the index axis*, unlike the `Addr` reads: a path is an anchor, not a read, so an out-of-range card mints the `cards[<i>]` root the error itself anchors at. WASM-only: Python has no `Addr` and exports no path parser or serializer, so there is no gap to close |
| Content revise (content lane) | `Card::revise_field(name, md)?` (schema-blind, borrow chain) | `doc.revise({card, field}, md)` (addr literal, JS) | **FFI** / **scope**: same model, flattened navigation, schema-blind, `Delta` in hand; WASM-only. Python's anchor-preserving write is the typed `writer.revise_field` |
| Content overwrite / splice (content lane) | `Card::overwrite_body(rt)` / `Card::overwrite_field(name, rt)?` / `Card::apply_body_change(b)?` / `Card::apply_field_change(name, b)?` | `doc.overwrite(addr, rt)` / `doc.applyChange(addr, bundle)` (JS) | **FFI** / **scope**: WASM fuses each body/field pair onto one `Addr` verb; WASM-only. The other two rungs of the anchor-fate ladder above |
| Live session edit | `LiveSession::update(&doc)` → `ChangeSet` | `session.update(doc)` | identical: a whole-document recompile, distinct from the content lane's `applyChange` splice |
| Opaque store | `store_field` / `store_fields` / `store_fill` | `storeField` / `storeFields` / `storeFill` (JS, `Addr`) | **scope**: the quill-free verbatim write, WASM-only; Python has no opaque field store (the typed writer is the only field write). A field write without a loadable quill operates on the storage DTO directly |
| Parse + warnings | `Document::parse(md) -> Parsed { document, warnings }` | `Document.fromMarkdown(md)` → `doc.warnings` getter | **FFI**, the wrapper fuses `Parsed` + `Document` into one session object: `fromMarkdown` returns the document directly and stashes the parse `warnings` on it (`doc.warnings`). That getter is a deliberate asymmetry with core, where warnings live only on `Parsed`: it is session state, so `equals` and the storage DTO exclude it and `loadStored`/`fromStored` clear it (a reloaded document carries no parse warnings) |
| Bound parse (the primary ingestion path) | `quill.parse(md) -> Parsed` | `quill.parse(md)` (JS), `quill.parse(md)` (py) | **FFI**: same `Parsed`-fusing wrapper as `fromMarkdown`, so parse warnings *and* the `conform::*` warnings ride the one `doc.warnings` carrier. A `$quill` naming a different quill throws (core: the `BoundParseError` mismatch half) rather than conforming under the wrong schema |
| Conform (read-repair) | `quill.conform(&mut doc) -> Vec<Diagnostic>` | `quill.conform(doc)` (JS), `quill.conform(doc)` (py) | identical: mutates in place and returns the `conform::*` diagnostics; Python returns the same diagnostic-dict list `validate` does |
| Engine backend roster | `engine.registered_backends()` | `engine.registered_backends()` (py) | **scope**: Python only. WASM settles the same question at build time: `build-wasm.sh` emits a Typst-free `core` variant alongside the default one, so which backends exist is a property of the artifact a consumer imported, not a runtime read |

The single **idiom** row on the front door is the honest cost: the typed writer is the one shape pyo3 carries worst, so its "identical" is qualified, not claimed.

## Python: `bindings/quillmark-python`

PyO3 bindings published as `quillmark` on PyPI. A `snake_case` surface over the shared model; one-shot `engine.render` (no canvas).

> **Scope: Tier 1 + storage + render.** Field I/O flows through `quill.writer(doc)` / `quill.reader(doc)` exclusively, the whole document through `reader.values()` / `writer.set_values(values)`; `Document` is quill-free data and structure (parse, the storage DTO, `insert_card` / `remove_card` / `move_card` / `make_card`, `remove_field`, the `card` / `seed_overlay` reads, `$ext` / `$seed`).
>
> The opaque store (`store_field` / `store_fields` / `store_fill`) and the anchor-preserving content lane (`overwrite` / `revise` / `apply_change` + the `import_markdown` / `export_markdown` / `rebase` / `map_pos` / `map_marks` codec) are **WASM-only by scope, not by lag**: their audience, storage/migration tooling holding no quill and live editors preserving anchor identity, is not a Python audience. A field write without a loadable quill operates on the storage DTO directly.

Every Python verb is identical to its core/WASM twin or names its one difference in the parity table above: `card=None` selectors fold the composable-card `$ext` / `remove_field` twins onto one axis, and `revise_field` discards the `Delta` (an editor receipt). No half-mirrored lane remains to drift.

**The surface is typed.** pyo3 docstrings are runtime-only, so the wheel ships a `py.typed` marker beside a hand-written `_quillmark.pyi`: without them every class resolves to `Any`. The stub is signatures only, the prose staying in `src/`, and nothing gates the two together: unlike WASM's `runtime.d.ts`, which CI typechecks against the generated backend declarations, a Python verb can be added or resignatured without the stub following. `python -m mypy.stubtest --ignore-disjoint-bases quillmark` is the check to run by hand when it changes.

## WebAssembly: `bindings/quillmark-wasm`

wasm-bindgen bindings published as `@quillmark/wasm`. Builds with `--target web` and `--weak-refs` so wasm-bindgen handles are reclaimed by `FinalizationRegistry`; `.free()` remains as the eager teardown hook. Requires Node 24+ / current evergreen browsers.

**The runtime owns instantiation.** `--target web` emits no `.wasm` ESM import and no top-level await, so nothing in the package graph forces a bundler plugin and a static import of `@quillmark/wasm` is safe anywhere, SSR included. The cost is that instantiation becomes explicit: `const { Quill, Document } = await init()` before the sync surface is touched.

- **Core only.** `Engine` instantiates each backend itself, inside the lazy load, so a consumer initializes core and nothing else.
- **Memoized on the promise**, so several entry points may each await it for one instantiation: the pattern is a destructure at the top of every entry point, not one trusted startup site.
- **Both failure codes reject**, `runtime::init_conflict` included, keeping the delivery rule in [ERROR.md](ERROR.md) § "Bindings Error Delegation" true at the one export that could break it.
- **One contract everywhere.** The binary streams from a URL in a browser and is read off disk under Node, resolved through the `#quillmark-env` subpath import rather than a runtime `typeof process` branch.

The `--target bundler` alternative emits `import * as wasm from "./wasm_bg.wasm"`, which no browser and no bundler resolves natively; the plugin that fixes it rewrites the import to a top-level await, and because the runtime statically imports core, that await lands on every consumer's static module graph. `build-wasm.sh` asserts the built artifacts carry neither form.

**The gate is the only door, so a pre-init call is unwritable.** `init` resolves to the core surface — the core build's values, which are exactly what its instance stands behind — and the runtime layer exports none of them statically, so a handle cannot be obtained without having awaited. With one entry in the `exports` map there is no subpath around it either. This is what makes the precondition structural instead of a convention a consumer has to know. A floating `init()` is an ESLint rule rather than a `tsc` diagnostic, so a signature is no guard, and a load order that reaches an initialized runtime by luck passes in dev and in the test that covers it.

Everything needing no instance stays a static export, unawaited: `MAIN_CARD_ADDR`, the open-set guards, `assignInstances`, `isQuillmarkError`. So do `Engine`, `LiveSession` and the four writer/reader classes, which their **arguments** gate rather than the door — every `Engine` verb takes a `Quill` first, and the writer/reader constructors take both handles, so a caller who has not awaited cannot produce an argument to call them with. Holding them out of the gate keeps them tree-shakable, so the editor path drops the dispatcher it never calls.

The two constructors taking no handle reach no wasm: `new Engine()` validates a descriptor map, and a `LiveSession` forwards to the backend session `engine.open` is the sole source of. None of the six carries a static method, the one member shape an argument cannot gate. `gate.test.js` is the executable guard, driving the whole static surface before `init`.

Ships **multiple artifacts from one crate** behind a single public root export. The root `@quillmark/wasm` is a hand-written **canonical runtime layer** that hands out the internal Typst-less **core** build's `Document` + `Quill` (load / validate / schema / seed / blueprint) verbatim and adds an `Engine` render dispatcher.

Each backend (Typst and pdfform) is a **private** build with its own linear memory, lazily loaded on the first render: there is no public `/core` or `/render` subpath. The core build is ~0.66 MB gzip; the Typst backend ~8 MB (Typst dominates), loaded only when something renders.

Backend handles never escape the `Engine`: it clones the quill tree + `doc.toStored()` into the backend's memory as serialized data and frees the clones.

The storage DTO carries no warnings, so the clone the backend renders knows nothing of the load's. `Engine.render` snapshots `doc.warnings` beside the DTO and splices it into `RenderResult.warnings` ahead of the compile's own ([ERROR.md](ERROR.md) § "Warning flow"): the runtime layer, not the backend build, is the surface that merges the two halves. `LiveSession.render` carries the compile half alone, a session outliving the document it opened from.

**Exactly one copy of the package per process.** Two copies are two core builds (two linear memories, two `Quill`/`Document` classes), and no topology legitimately loads a multi-megabyte binary twice and needs handles to cross between the copies. Every seam taking a core handle checks it and throws a `QuillmarkError` naming the duplicate install, including the ones that could cross as data (`Engine`, `LiveSession.update`). Errors are the exception: `isQuillmarkError` stays structural, an error being data rather than a handle.

Beyond the byte-output verbs (`engine.render`, `LiveSession.render`), the canvas-capable backend builds (Typst, and pdfform under its preview seam) expose a **live preview** path on `LiveSession` (`update`, `pageCount`, `pageSize`, `paint`, …). See [PREVIEW.md](PREVIEW.md).

## CLI: `bindings/quillmark-cli`

Standalone `quillmark` binary. See [CLI.md](CLI.md).

## Links

- [PROGRAMMATIC.md](PROGRAMMATIC.md): building documents in memory through each surface's mutators
- [CLI.md](CLI.md): command-line surface
- [PREVIEW.md](PREVIEW.md): WASM multi-backend canvas preview (Typst, pdfform)
- [ERROR.md](ERROR.md): the diagnostic model that crosses every boundary
- Per-binding API detail: the respective `crates/bindings/*/` rustdoc and READMEs
