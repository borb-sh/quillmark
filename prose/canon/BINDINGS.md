# Bindings

> **Implementation**: `crates/bindings/`

## TL;DR

Quillmark exposes one core library through several language surfaces: Python (PyO3), WebAssembly (wasm-bindgen), and a CLI binary. Every surface drives the same `quillmark` core: the same `Document`/`Quill`/`Card` model, the same `serde` diagnostics, and the same capability principle. Surfaces differ only in language idiom, packaging, and which extras they expose (canvas preview is WASM-only).

## Shared model

- **Capability principle.** A `Quill` is portable, declarative config data. Its format capability (`supportedFormats`) and rendering are resolved by the `Engine` *against* a quill at render time: never by the quill itself. So `quill.metadata` is a pure, infallible config snapshot — the identity keys `name`, `version`, `backend`, `author`, `description`, then `license` and `homepage` when the quill declares them, in that order on both surfaces — while `render` / `supportedFormats` can fail for an unregistered backend.
- **One model, serialized across every boundary.** The `Document`/`Card` model and `Diagnostic`s cross each language boundary as the same core `serde` shapes (`CardWire`, the versioned storage DTO, `Diagnostic`), so a card or an error reads identically no matter which surface emits it. See [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [CARDS.md](CARDS.md), [ERROR.md](ERROR.md).
- **Uniform errors.** Each binding raises a single error type that always carries a non-empty diagnostic list (`QuillmarkError.diagnostics` / thrown `Error.diagnostics`).

The WASM binding is the reference surface; Python mirrors it within its scope (Tier 1 + storage + render; see the scope note below). New contract work lands in WASM first.

## The write surface: object placement over one primitive

Placement decides where a mutation verb lives, in one sentence:

> **If a verb needs a schema, it lives on the writer. `Document` is quill-free data.**

`quill.writer(doc)` (mirroring core's `quill.writer(&mut doc)`) is the one schema-bound input path: bare `set` / `set_all` / `revise` / `reviseDocument` / `addCard` / `removeCard` / `card(i)`, names and markdown in, diagnostics out. It resolves each field's type from the bound quill, so a name the schema does not declare is a typo (`UnknownField`), not a fallback.

`Document` holds everything quill-free: the opaque `store*` primitive (verbatim, coercion deferred to render)
and the addressed content write: `read` / `hash(addr)` / `toMarkdown(addr?, { annotated })` and the write primitive `apply` with its producers `revise` / `replace` ([CONTENT.md](CONTENT.md) § "Writing a value"), all navigating by `Addr`, answering receipts and never consulting a schema. The content-model functions `importMarkdown` / `exportMarkdown` / `normalize` / `invert` / `diff` sit beside it, free functions over a `Content`.
**Transport** reads (`getStored` / `getExt`) return the stored value verbatim, need no schema, and sit on `Document` too.

The `isInline` / `isPlain` predicates sit beside the content-model functions: `Content::is_inline` / `is_plain`, the one-line and `text` constraints, over any `Content`. A constraint the schema declares reports through `validation::not_single_line` / `not_plain`; one an editor imposes on its own, such as a one-paragraph surface over an `array<richtext>` element, has no diagnostic, so the editor asks the predicate before the leaf commits.

The **interpreting** reads (reading a field by its type) are schema-shaped questions ("this field's richtext, as markdown"), so they gain a schema-bound home: `quill.reader(doc)`, the read twin of `quill.writer(doc)` (mirroring core's `quill.reader(&doc)`).

`reader.get(addr)` reads each field in the values form: every content leaf in its type tree to its codec's text (a `richtext` leaf to markdown, a `text` leaf to its literal text, marks verbatim), everything else as stored, a present-null as `null`: with schema authority, so a name the schema does not declare throws `UnknownField` instead of reading back `undefined`, and a content leaf holding an undecodable value throws `FieldDecode`.

`reader.getContent` is the same read at the other end of the codec, returning the **`Content`** rather than the projection: it makes a content field's storage form an implementation detail instead of a caller's branch, and it still spans what the transport input path holds as authored.

**The bound input path is the primary one.** `quill.parse(md)` is parse-then-conform, and `quill.conform(doc)` is the same walk on a document that arrived any other way; both live on the quill because both need the schema. A document that came through either is in its stored form, one per codec ([SCHEMAS.md](SCHEMAS.md) § "Content fields are stored per codec"): every content field as the `Content` object, a `text` field's spelling `codec: "plain"`, so `getStored` answers by the field's declared codec rather than by how the document was built.

The schema-free `Document.parse` remains the transport input path, for repair (migrations, `$ext` stamping, a quill that will not load, opening a document to fix its `$quill`); the stored form of what it returns is unspecified, and the next bound load converges it.

Across the content fields the walk covers, the exception states are named, never silent: no quill means the document stays as authored, the wrong quill throws before any mutation, and a content-field value the strict write refuses stays authored under a `conform::*` warning.

A field whose type tree bears no content leaf is outside the walk, so conform is silent about it by construction: a scalar's shorthands are the typed write's to canonicalize, and a stored value its grammar refuses (a wall clock in a `date`) is `quill.validate`'s to report. A clean conform says the content fields are in their stored form, not that every field would commit.

Decoding a string needs the schema and not the payload: a `richtext` string is markdown and a `text` string is literal text, so the same stored bytes decode two ways and only the declared type says which. A content object in its stored form names its codec, and a quill-free write or read honours it. That is why the `Content` read binds the quill instead of sitting beside `getStored`: a quill-free version would guess a codec, and would guess markdown.

**A `Content`-typed read is also a write input, so it answers in the one canonical form** ([CONTENT.md](CONTENT.md) § "Validity"), the only form a `Content` input takes: every verb taking one refuses a value `normalize` would rewrite, as Rust's `Content::from_value` and `Card::replace` do, but `normalize`, which answers that form. Every write path carries it: `doc.read`, `reader.getContent{,At}`, `getStored` on a body, `importMarkdown`, `normalize`, and the `Card` wire behind `document.main` / `cards` / `card(i)` / `removeCard` / `seedCard`. `getStored` on a field and `payloadItems` carry the same form, echoed unread, which is why both are typed `unknown`.

A field's markdown lives here, and the quill-free markdown export stays on `Document`: `doc.toMarkdown(addr?, { annotated })` answers the whole document without an address and one body or content value with one, the codec the value names deciding its spelling. Markdown has one import and one export: `Document.parse` / `importMarkdown` in, `toMarkdown` / `exportMarkdown` out, each export taking `annotated` to say whether anchors and islands survive. The placement rule generalizes: *a verb that needs a schema lives on the writer (writes) or the view (reads); `Document` is quill-free data.*

The rule scales from a field to a card without a new receiver: `reader.card(i)` / `writer.card(i)` answer the same verbs one card in, and `reader.resolve()` is the render view over the whole document, beside the authored read `get` answers ([SCHEMAS.md](SCHEMAS.md) § "The values form"). `quill.parse` / `quill.validate` / `quill.conform` stay on the quill as its ingestion and verdict verbs: a constructor, a diagnostic list, and `parse`'s in-place twin, none of them a read or write of a value.

`revise` is the writer verb that is both typed *and* anchor-preserving: the content `revise` at the address, merged with the read's loss, with its quote relocation ([CONTENT.md](CONTENT.md) § "Writing a value").

- At a field, the result then commits under the field schema like `set`. Because that needs the schema, it lives on the writer, wrapping core's `Card::revise_field_checked` primitive, which `Document` does not expose.
- It takes the address `get` and `hashAt` take, so the body is the address with no field, not a verb of its own. The body's codec is `richtext` by format, and it has no schema to conform to.
- `reviseDocument` is the same revise at every value of the document.

**The writer is the only typed input path, core included.** The typed primitives (`Card::commit_field`, `Card::revise_field_checked`) are `#[doc(hidden)]`. A resolved `FieldSchema` argument is the only thing that tells them from their opaque and schema-blind neighbours, and disambiguating by argument is a third mechanism beside the receiver and the verb: one the rule below does not name and a caller cannot see.

**The verb carries the write path.** One vocabulary rule, stated once here:

| Verb | Write path | What it does |
|---|---|---|
| **store** | verbatim | the quill-free opaque write; coercion deferred to render |
| **set** | typed | the writer's strict commit at the write. `set` writes one field and `set_all` merges a batch, at card or document scope |
| **apply** · **revise** · **replace** | content | one primitive and its producers, each answering a `Receipt` |

The content write goes through one primitive. `apply(addr, change, { base, actor })` runs a `Change`'s steps, all or none, and the producers build the change for a writer with no steps of its own ([CONTENT.md](CONTENT.md) § "Writing a value"):

- **apply** is what an editor writes: each transaction lowered to a `Change`.
- **revise** is the one content revise, over the markdown of `toMarkdown(addr, { annotated: true })`. Its receipt carries a `parse::dropped_construct` warning per construct the import dropped ([ERROR.md](ERROR.md) § "Warning flow"). An anchor it drops stays the host's to `reattach` ([COMMENTS.md](COMMENTS.md)).
- **replace** writes a whole value, in canonical form. The cold-import path is `replace(addr, importMarkdown(md).content)`, the import's `warnings` beside the `.content` it takes. `normalize(c)` gives the canonical form of `c`, holding no document, and throws where `c` is invalid once normalized.

Every content write on `Document` takes `{ base, actor }` ([CONTENT.md](CONTENT.md) § Ids): `actor` scopes each id the write mints, and a live client passes one per session. The writer's revise verbs take a base alone, so the ids they mint, a card `reviseDocument` inserts included, are positional.

Every receipt is `{ applied, hash, droppedAnchors, movedAnchors, warnings, rebased, minted }`: `invert(applied, before)` undoes the write, and `hash` is the next write's `base`. A dropped anchor is its selector, which `reattach` takes as given, and an anchor a removed island or slot held carries that `island` and `slot`. `warnings` holds a revise's `parse::dropped_construct` warnings, and `rebased` is true where the write merged with a change since its base. `minted` maps each placeholder the change gave a record it put to the id the write minted for it.

The bindings carry the write and the two helpers a writer outside the editor needs, `invert` and `diff`. WASM carries the editor's too ([CONTENT.md](CONTENT.md) § Changes):

- the structural edits as `Document` verbs, `doc.splitBlock(addr, at, opts)`, `joinBlocks(addr, id, opts)`, `deleteRange(addr, start, end, opts)`, `indent(addr, first, last?, opts)`, `outdent(addr, first, last?, opts)` and `wrap(addr, first, last, kind, opts)`, each writing as `apply` does and answering its `Receipt`, offsets in UTF-16 into the value `opts.base` names;
- `doc.blockAt(addr, offset)`, the block holding an offset: `{ id, index, path }`;
- `diffRange(before, after, { from, to })`, the exact splice over a range the editor knows, beside `diff`;
- `inheritedMarks`, the formatting typed text carries, and `mapPos`, which maps a selection across a change.

Every input path that takes a value or a change bounds it before any work: a `Change` of more than `MAX_CHANGE_STEPS` (16 384) steps throws `edit::too_many_steps`, a `Content` or `RawContent` whose text and its slots' texts pass 10 MiB throws `edit::input_too_large`, as markdown past 10 MiB does at `revise` and `importMarkdown`, and a value past its mark or anchor bound throws `edit::too_many_marks` or `edit::too_many_anchors` ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies").

WASM and Python also expose merge `repair`, which reads a state concurrent edits merged into (`RawContent`) as the valid value every peer holding it reads, with each mend it made and the change from the merged text to the value's, which `mapPos` maps a position through ([CONTENT.md](CONTENT.md) § "Merge repair"). A stale write reaches the merge through `apply`, `revise` and `replace` with a base.

**The document crosses as bytes.** A document's Automerge document ([CONTENT.md](CONTENT.md) § "Live state and merge") is reached through verbs on `Document`, bytes a `Uint8Array` in JS and `bytes` in Python:

- `doc.save()` answers the whole document's bytes, its history included, and `Document.fromBytes(bytes)` (Python `Document.from_bytes`) reads them back.
- `doc.heads()` answers the document's revision, the hex hashes of its Automerge heads; `doc.saveSince(heads)` (`save_since`) the changes past them; `doc.merge(bytes)` takes another peer's changes.
- `new SyncState()` (Python `SyncState()`, or `Document.syncState()` in either) makes an opaque `SyncState`, what one peer is known to hold; `state.encode()` and `SyncState.decode(bytes)` keep it across a restart.
- `doc.generateSyncMessage(state)` answers the next message's bytes, `undefined` (`None`) once the peer holds everything, and `doc.receiveSyncMessage(state, message)` takes a peer's.

Bytes or a message that would leave the document outside its schema throw `edit::bad_live_bytes` with its `reason`, and change nothing. Peers that sync start from one document, one read from another's `save`, and each writes under its own `actor` or none: a write with no actor writes under its document's own random Automerge actor, while two peers writing under one actor mint one Automerge sequence number twice.

**A content write names its card by `$id`.** `Addr.card` (Python's `card=`) is a composable card's `$id` or its index on every verb, an id no card holds throwing `edit::unknown_card_id`. The content writes (`apply`, `revise`, `replace`) take the id alone and throw on an index: a card moved or inserted between a writer's read and its write shifts an index onto another card, and an equal value there passes the base. The rest of the surface keeps the index, which it reads at the call.

A base there guards the value and not the card: the card writer's `revise` takes an index and a base, and an index shifted onto a card holding an equal value passes it, so a writer that must hold to the card it read writes by `$id` through `apply`, `revise` or `replace`.

The writer's `reviseDocument` is the markdown rewrite over a whole document. An incoming composable card spelling a stored card's `$id` revises it, when both are of one `$kind`; of several spelling one id, the most similar keeps it ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § Card-id identity). Where the incoming cards spell any stored id, a stored card holding an id none spells was removed and pairs with nothing.
The rest align by a diff over their `$kind` sequences: a run of one kind paired by text similarity, an unchanged card paired with its twin wherever it moved, and the cards no text pairs paired in order by kind within each gap the text pairs leave.
A list field's items align the same way, by text, so an item inserted ahead of an anchored one leaves the anchor on its own item.
Each aligned body and content field revises; an anchor on an unaligned card, or one a diff drops, drops, and the `DocumentRevised` receipt names each one with its card's `cardId`.
The receipt's `from` is the alignment itself: for each revised composable card, the stored index it revises or none for an inserted card. It names no card operations, since which cards of a reorder count as moved is the consumer's choice.

### Content offsets

Storage is scalars, and every host speaks its own: storage and core count Unicode scalar values, and each binding speaks its host's units at every surface that takes or answers an offset, converting inside (`units` module of `quillmark-content`). WASM speaks UTF-16 code units, JavaScript string indices; Python speaks code points, which are storage's own, so it converts nothing.

- A `Content` converts on its own text, and a slot on its own.
- A `Change` converts against the value it applies to, each step on the text the steps before it left, so a step's offsets read as the writer saw them. An offset inside a surrogate pair refuses the change (`edit::content_apply`).
- `repair` reads a `RawContent` as a merged state, so it refuses no offset: one inside a surrogate pair moves to the character's start and one past the text to its end, and the mend is reported.
- `invert` converts the change on `before`, and answers in the units of the value it left; `diff` answers in the units of `before`.
- A content write's change converts on the value its `base` names, which a stale base makes an earlier value than the one it writes over; its receipt's `applied` converts on the value it wrote over.
- `merge` and `receiveSyncMessage` answer nothing: a host reads the document again, and the bytes and messages hold no offset a host reads.
- The render seam ([PREVIEW.md](PREVIEW.md)) converts on the text of the content value the compile read at each field: a region's `span`, `positionAt`'s `pos`, and `locate`'s `pos`, which answers nothing inside a surrogate pair. Core's `LiveSession::*_in` and `Engine::render_in` take the units.
- A writer revise's `Receipt.applied` converts on the value it revised (`Receipt::in_units`), and `reattach` reads each change it takes on the value's text now, the text that write left.
- A diagnostic's `location` is a line and column in a source file, not a content offset, and crosses as the parser counts it.

A content value inside a payload, a richtext field's value or one nested in a list or object field, converts wherever it crosses: any object holding a `text` string and a `blocks` array is read as content.

**The rule governs user-field writes.** System metadata (`store_ext` / `store_seed_*`), structural operations (`moveCard` / `removeCard`), and the `remove_*` family sit outside it: `remove_*` names no write path because one verb serves every write path, and a structural op moves cards rather than writing into one. Placing a card writes its fields, so `insertCard` is a store. So `unchecked.store_field` and `unchecked.insert_card` are the opaque store, `set` / `set_all` / `add_card` the typed writer, and a name never needs per-verb disambiguation against its neighbor.

**The opaque store is a receiver in every surface.** `doc.unchecked()` in core and `doc.unchecked` in both bindings hold the writes no schema checks, so a call site names the write path it takes (`doc.unchecked.storeField` vs `writer.set`), and the receiver is the same on every surface, so the parity table needs no difference class for it. An unchecked write still holds every invariant but the schema: names, the bounds, card ids, and each content value decoded as a body write decodes one and stored in its canonical form. Every write on `Document` validates; only the schema check is the writer's.

The body follows from the same rule rather than from a separate one. `writer.revise` at the body is the content `revise` reached through the writer: a body carries no field schema, so there is nothing to type, and the receipt it returns is the content write's. The body has no verb of its own on either side, writer or reader: it is an address.

**The read side mirrors it.** The verbatim read is `getStored`: the read echo of `store`, and the interpreted read is `reader.get`, the reader/writer twin of `set`. So the transport and schema-plane reads carry the write path in the verb the same way the writes do, rather than collapsing both onto one `get` where only the receiver (`doc` vs `reader`) tells them apart. (Core needs no such split: its verbatim read is the map-idiomatic `payload().get`, already lexically distinct from `reader.get`; the collision is a WASM/pyo3 artifact of fusing the read onto the one `Document` handle, so only the bindings rename it.)

**`equals` is the change gate.** A consumer driving a live preview gates `update` on structural equality against a retained clone: `if (doc.equals(last)) return; last = doc.clone()`. It covers the document the consumer did not mutate itself: one swapped in from storage, or written through a writer held elsewhere. `toStored` is byte-deterministic within a schema version, for a consumer that prefers a hashed gate.

**No revision counter.** Neither `Document` nor the session carries one ([PREVIEW.md](PREVIEW.md)). Equality answers whether this is the content last compiled. A counter answers only whether something was written, so it re-applies on a load that is content-identical to the live compile. `main_mut` / `card_mut` hand out a `CardMut`, which is a place a bump could sit — the decision turns on what the counter would answer, not on where it could be stamped.

**A write precondition is a hash, not a counter.** The preview gate asks whether this is the content last compiled; a write precondition asks whether the value a writer read is still the value stored. A write lands on the value stored now, so without one an edit made between a writer's read and its write reads as text the writer deleted, and is reverted without a signal.

**Pass the base.** The content `apply`, `revise` and `replace` take `base`, the hash `doc.hash(addr)` answered when the writer read. A value that now hashes otherwise merges the write with the change since: the write lands as one Automerge change at the heads that hash was left at, concurrent with every change since ([CONTENT.md](CONTENT.md) § "Live state and merge"). The receipt says `rebased`.

- Nothing refuses: text both changed keeps both edits, and a record both set reads as the later write.
- A hash the document's history does not reach, as every base read before the document was loaded from its stored row, refuses with `edit::stale_base`, and the writer reads again.

The content writes' options take the hash as `base` or as `baseHash`, one meaning under either name: a stale hash merges, on the content writes and on the typed writer's value revises below alike.

The writer's revise verbs take an optional base, the hash or the canonical value the writer read. The host holds what its writer read, so it holds the value itself as readily as its hash:

- `baseHash` / `base_hash` (`Base::Hash`): the hash of the value the writer read, SHA-256 over `quillmark/hash@1` and the value's JSON with every object's keys sorted, so a store that reorders keys keeps every base valid.
- On `revise`, a value that now hashes otherwise merges the write with the change since, as the content `revise` does under `base`, refusing with `edit::stale_base` where the document's history does not reach the base. A document read from its bytes keeps its history; one loaded from its stored row reaches no base before its load ([CONTENT.md](CONTENT.md) § "Live state and merge"). On `reviseDocument` a stale `baseHash` refuses with `edit::stale_base`; `baseStored` is the document's merge. A hash covers anchors, so a comment added since the read is a change the write merges with.
- The read pairs with the write path. The writer's `hash` / `hashAt` (`hash_at`) hash through the schema: a `richtext` or `text` field as its content however it is stored, ids and anchors included, the document as the quill conforms it, so a conform between the read and the write stales nothing. The quill-free `doc.hash(addr)` (Python `doc.hash_at`) hashes a body or richtext field, the `base` a content write names; `doc.hashAt` is the same read.
- `baseContent` / `base_content` (`Base::Content`) on `revise`: the canonical `Content` the writer read, as `doc.read` (`Card::read`) returns it, found by its hash among the heads the document noted, and merged as a `baseHash` is.
- `baseStored` / `base_stored` (`DocumentBase::Stored`) on `reviseDocument`: the `save()` bytes the writer read, or a `toStored()` row. Under bytes of this document's history, the writer's revise of the document they hold lands as one Automerge change at their heads, merged with every change since: a card both sides changed merges field by field, a field both set reads as the later write, and text both edited keeps both edits. A row, or bytes of another history, holds only where it reads as the document now, and refuses with `edit::stale_base` otherwise.

A stale base carries `current`, the hash now. The hash is content-addressed, so it answers what equality answers for the preview gate: a content-identical reload keeps every base valid, where a counter would invalidate it.

**Writers and card cursors are ephemeral: bind, write, discard.** They hold an address (the quill + document, or an index), never a cache; every call reads through the document, so a `removeCard` / `addCard` between binding a cursor and writing through it silently retargets it. A caller whose cards move re-resolves the index at write time ([PROGRAMMATIC.md](PROGRAMMATIC.md)).

**The hand-written runtime is the real API; the wasm class is its ABI.** The quill-taking `_commitField` / `_commitFields` / `_addCard` / `_revise` / `_reviseDocument` / `_hash` / `_hashAt` methods are the stable ABI under the writer's `set` / `set_all` / `addCard` / `revise` / `reviseDocument` / `hash` / `hashAt`, and `_uncheckedStoreField` / `_uncheckedInsertCard` under `doc.unchecked`: underscored and dropped from the `.d.ts`, not from the binary. The visible `Document` class then carries zero quill-taking methods, so the split is structural, not asserted.

### Parity table

Every binding verb is *identical* to its core twin or names its one forced difference: **FFI** (a wasm-bindgen / pyo3 constraint), **idiom** (a language ergonomic), or **scope** (a surface a binding omits by intent: Python is Tier 1 + storage + render), nothing else admitted.

**Identical to their core twins**, so the table below omits them, each binding spelling them in its own case:

- `set` / `set_all`.
- `add_card`: make, typed commit and insert fused into one transactional call. `at` appends when absent, else inserts at the index, and `actor` scopes the `$id` it mints as `unchecked.insert_card`'s does. It answers that id beside its warnings as `CardAdded`, a dict of `id` and `warnings` in Python.
- `remove_card`.
- `conform`: in place, returning the `conform::*` diagnostics; Python's are the dicts `validate` answers with.

Two more are identical where they exist, both WASM-only: the `writer.card(i).kind` cursor read, and `session.update`, a whole-document recompile distinct from the content `apply`. The table holds one row per forced difference.

| Concept | Core | Bindings | Class |
|---|---|---|---|
| Typed writer entry | `quill.writer(&mut doc)` | `quill.writer(doc)` | **idiom**: core holds `&mut Document` under the checker; the bindings re-borrow per call (pyo3/wasm objects carry no lifetime), so the guarantee becomes the ephemerality convention |
| Typed reader entry | `quill.reader(&doc)` | `quill.reader(doc)` | **idiom**: the read twin; same re-borrow/ephemerality as the writer |
| Interpreted read | `reader.get(target)?` → `Option<QuillValue>`; `reader.card(i)?.get(..)?` | `reader.get(addr)` / `reader.card(i).get(name)` (JS), `reader.get(field=, card=i)` (py) | **idiom** / **FFI**: the values form above, reached through an `Addr` or a `card=` selector; no field reads the body's markdown. An absent field reads `undefined` / `None` and a present-null reads `null` / `None`, so Python cannot tell the two apart |
| Interpreted `Content` read | `reader.get_content(target)?` → `Option<Content>`; `reader.card(i)?.get_content(..)?` | `reader.getContent(addr)` / `reader.card(i).getContent(name)` (JS), `reader.get_content(field=, card=i)` (py) | **idiom** / **FFI**: a declared type that is not a content leaf is `FieldNotContent` (an `array<richtext>` carries content and still has no one `Content`). Both bindings return canonical Content-JSON; no field reads the body's `Content` |
| Nested `Content` read | `reader.get_content_at(name, path)?`; `reader.card(i)?.get_content_at(..)?` | `reader.getContentAt(addr, path)` / `reader.card(i).getContentAt(name, path)` (JS), `reader.get_content_at(name, path, card=i)` (py) | **idiom** / **FFI**: the same read one axis in, `path` a `PathStep[]` / `Sequence[str \| int]` walked through the field schema's `items` / `properties` / `variants` to the leaf whose declared type names the codec; the empty path *is* `get_content`. A path naming nothing stored reads absent, the field axis's totality applied to the axis a repeater mutates; a bad card index still throws. The path is the read's own argument rather than an `Addr` member, since `storeField` / `getStored` / `apply` take an `Addr` and cannot answer an element axis |
| Resolved view | `reader.resolve()` → `Resolved` (over the `Quill::resolve` producer) | `reader.resolve()` (JS) | **scope**: the render view beside the authored `get`; WASM-only until a Python consumer names a call site |
| Revise | `TypedWriter::revise(target, text, base)?` / `CardWriter::revise(..)?` → `Receipt`, `target` a `Target` or a field name | `writer.revise(addr, text, opts?)` / `writer.card(i).revise(addr, ..)` (JS); `writer.revise(text, field=, card=, base_hash=, base_content=)` (py) | **idiom**: typed *and* anchor-preserving at a field, wrapping `Card::revise_field_checked`; at the body (no field) the content `revise`. The receipt is the content write's `Receipt`, `{ applied, hash, droppedAnchors, movedAnchors, warnings, rebased }`, its warnings anchored at the address by the binding; a `text` field's `applied` is one splice over its literal text. Python returns it as a dict with snake_case keys, as `Document.revise` does, its warnings the `Diagnostic` objects `set` returns |
| Typed whole-document revise | `TypedWriter::revise_document(md)?` → `DocumentRevised { dropped_anchors, warnings, from }`, over the quill-free `Document::revise(md)?`; `revise_document_against(base, md)?` | `writer.reviseDocument(md, opts?)` (JS), `writer.revise_document(md, base_hash=, base_stored=)` (py) | **idiom**: `Document::revise`, then conform, so an input path holding the quill keeps parse-then-conform; no binding exposes the quill-free revise. A `$quill` this quill does not answer to throws before any mutation, as `quill.parse` does. JS returns `{ droppedAnchors, warnings, from }`, each dropped anchor the selector `reattach` takes, as on a value write's receipt, each `path` a `DocPath` string and `from` a `(number \| null)[]`; Python returns the receipt as a dict, `{ dropped_anchors, warnings, from }`, as on `revise_body` |
| Anchor contexts | `Document::anchor_contexts()` / `anchor_context(id)` → `AnchorSelector { id, path, card_id, quote, prefix, suffix }` | `doc.anchorContexts()` / `doc.anchorContext(id)` (JS), `doc.anchor_contexts()` / `doc.anchor_context(id)` (py) | **FFI**: the selector crosses flat, `{ id, path, quote, prefix, suffix }`, its `path` a `DocPath` string, built as a `DocumentAnchor` is; an absent id reads `undefined` / `None`. The quill-free read a comment layer stores beside each thread ([COMMENTS.md](COMMENTS.md)) |
| Re-attachment | `Document::reattach(selectors, &changes)` → `Reattached { reattached, orphaned }` | `doc.reattach(selectors, changes?)` (JS), `doc.reattach(selectors, changes=None)` (py) | **idiom**: `changes` is a `Record<path, Change>` / `dict[str, Change]`, each a receipt's `applied`, keyed by `DocPath` string, absent for none; the receipt is `{ reattached, orphaned }` of flat selectors. A path that does not parse throws, where one naming no body or content value orphans its selector, unless it is a whole-field selector (empty `quote`, `prefix` and `suffix`) naming a field value holding no content, which is reattached ([COMMENTS.md](COMMENTS.md) § "Text and scalar fields") |
| Annotated read | `Document::to_markdown_annotated()` → `AnnotatedMarkdown { markdown, anchors }` | `doc.toMarkdown(undefined, { annotated: true })` (JS), `doc.to_markdown(annotated=True)` (py) | **idiom**: the one export with its `annotated` option. Annotated, both return `{ markdown, anchors }`, each anchor `{ id, path, cardId?, line }` (`card_id` in Python), each `path` a `DocPath` string. The read is two-way: written back through `writer.reviseDocument`, a kept anchor tag pins its anchor, a dropped one leaves the anchor to the diff, and an island token keeps its island as stored ([markdown-spec.md](../references/markdown-spec.md) §6.4); every revise verb reads them the same way. It is two-way for anchors and islands, not for the whole value ([CONTENT.md](CONTENT.md) § "What markdown keeps"): every revise verb is the one content `revise`, which keeps what markdown does not spell by merging it back in ([CONTENT.md](CONTENT.md) § "Writing a value") |
| Scoped markdown read | `Card::annotated(target)?` → `Annotated { markdown, anchors }`; `body_markdown()` | `doc.toMarkdown(addr, { annotated })` (JS), `doc.to_markdown_at(field=, card=, annotated=)` (py) | **idiom**: the export at an address, the read a revise at that address reads back. Annotated, JS answers `AnnotatedMarkdown`, each anchor's `path` the address, and Python `{ markdown, anchors }`, each anchor `{ id, line }`; Python's name is the `hash` / `hash_at` pair's |
| Card insertion | `doc.unchecked().insert_card(at, card, actor)` → the card's `$id` | `doc.unchecked.insertCard(card, at?, { actor })` (JS), `doc.unchecked.insert_card(card, at=, actor=)` (py) | **idiom**: absent `at` appends; a card spelling no `$id` mints one, under `actor` where one is given, and the id it answers is the one a collab client carries to its peers ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Card-id identity") |
| Card cursor | `writer.card(i)?` (eager check) | `writer.card(i)` (lazy check, JS) | **FFI** / **scope**: no borrow to validate against, so the index is checked at the write; WASM-only, where the writer holds the document by reference — Python addresses a card with the `card=` selector each verb takes |
| Reads (value / `$ext`) | `payload().get(..)` / `card.ext()` (borrow chain; index for a card) | `doc.getStored(addr?)` / `doc.getExt(cardAddr?)` (JS) | **idiom** / **FFI** / **scope**: WASM fuses the transport reads onto the one `Addr` (a bare string ⇒ `{field}` for `getStored`), and they are *total over the field axis*: an absent field reads `undefined`, and only an out-of-range card throws. Python has no quill-free field read: interpreted field reads go through `quill.reader(doc).get`, and `$ext` / body content read off the `main` / `cards` dict snapshots |
| Reads (whole card / seed) | `card(i)` / `main().seed()` | `doc.card(i)` / `doc.seedOverlay(kind)` (JS), `doc.card(i)` / `doc.seed_overlay(kind)` (py) | **idiom**: both bindings fuse each into one named verb on `Document`, quill-free structure reads that spare the caller the whole-`cards` / whole-`main` projection; `card(i)` throws out of range, the seed read is total (no such kind → `undefined`/`None`) |
| Path mint (address → anchor) | `DocPath::main()` / `DocPath::card(kind, i)` extended by `field` / `body` | `doc.pathFor(addr)` / `doc.cardPath(i)` (JS) | **scope**: the `Addr` rendered as the `DocPath` string `Diagnostic.path` carries and the geometry queries take, so a consumer never restates the kind lookup, the `Addr` defaults or the range guard; quill-free (the stored `$kind` verbatim) and *total on the index axis*, unlike the `Addr` reads: a path is an anchor, not a read, so an out-of-range card mints the `cards[<i>]` root the error itself anchors at. WASM-only: Python has no `Addr` and exports no path parser or serializer, so there is no gap to close |
| Content write | `Card::apply(target, change, opts)` / `Card::revise(target, md, opts)` / `Card::replace(target, content, opts)`, one `Card::write(target, edit, opts)` → `Receipt`, `opts` a `WriteOptions { base, actor }` or a bare base | `doc.apply(addr, change, { base, actor })` / `doc.revise(addr, md, { base, actor })` / `doc.replace(addr, content, { base, actor })` (JS); `doc.apply(change, field=, card=, base=, actor=)` / `doc.revise(..)` / `doc.replace(..)` (py) | **FFI** / **idiom**: WASM fuses core's `Target` and the card into one `Addr`, its `card` the card's `$id`; Python folds them into `field=` / `card=` selectors. Offsets cross in the host's units (§ "Content offsets"), and the receipt crosses with each dropped anchor as a flat selector, a slot's anchor naming its `island` and `slot` |
| Content read | `Card::read(target)` / `hash_of(target)` | `doc.read(addr)` / `doc.hash(addr)` (JS); `doc.read(field=, card=)` / `doc.hash_at(..)` (py) | **FFI** / **idiom**: the same `Addr` and selector folding; an absent field reads as the empty content |
| Document bytes and sync | `Document::to_bytes()` / `from_bytes(bytes)?` / `heads()` / `save_since(heads)` / `merge(bytes)?`; `generate_sync_message(&mut State)` → `Option<Message>`; `receive_sync_message(&mut State, Message)?` | `doc.save()` / `Document.fromBytes(bytes)` / `doc.heads()` / `doc.saveSince(heads)` / `doc.merge(bytes)` / `Document.syncState()` / `doc.generateSyncMessage(state)` / `doc.receiveSyncMessage(state, message)` (JS); `doc.save()` / `Document.from_bytes(data)` / `doc.heads()` / `doc.save_since(heads)` / `doc.merge(data)` / `Document.sync_state()` / `doc.generate_sync_message(state)` / `doc.receive_sync_message(state, message)` (py) | **FFI**: bytes cross as a `Uint8Array` / `bytes`, heads as hex strings, and a sync message as its encoded bytes, `undefined` / `None` for none; Automerge's `sync::State` (core's `SyncState`) is the opaque `SyncState` handle, with a constructor and `encode` / `decode` |
| Structural edits | `Card::split_block` / `join_blocks` / `delete_range` / `indent` / `outdent` / `wrap`, each a `Card::write` of `Edit::Structural` → `Receipt` | `doc.splitBlock(addr, at, opts)` / `doc.joinBlocks(addr, id, opts)` / `doc.deleteRange(addr, start, end, opts)` / `doc.indent(addr, first, last?, opts)` / `doc.outdent(..)` / `doc.wrap(addr, first, last, kind, opts)` (JS) | **scope**: WASM-only until Python gains them; `addr` folds core's `Target` as the content write does, and the new block's id is the one the write mints, named in `minted` ([CONTENT.md](CONTENT.md) § Changes) |
| Block at an offset | `Card::block_at(target, at)` → `Located { id, index, path }` | `doc.blockAt(addr, offset)` (JS) | **scope**: WASM-only until Python gains it |
| Inherited marks | `structural::inherited_marks(content, at)` | `inheritedMarks(content, at)` (JS) | **scope**: WASM-only; the formatting typed text carries |
| Range lowering | `diff::diff_range(before, after, from, to)` | `diffRange(before, after, { from, to })` (JS) | **scope**: WASM-only until Python gains it; the editor's exact splice over ProseMirror's changed range |
| Position mapping | `mapping::Mapping::from_change` + `map_point` | `mapPos(change, pos, assoc?)` (JS) | **scope**: WASM-only; maps a selection across a change |
| Merge repair | `repair::repair(raw)` | `repair(raw)` (both) | the collab projection's read of a merged state, answering `{content, repairs, change}` ([CONTENT.md](CONTENT.md) § "Merge repair") |
| Change helpers | `change::invert`, `diff::diff` | `invert` / `diff` (JS, py) | **idiom**: module functions holding no document, offsets in the host's units |
| Content-model functions | `markdown::from_markdown`, `to_text` / `to_text_annotated`, `Content::normalize` / `is_inline` / `is_plain` | `importMarkdown` / `exportMarkdown(c, { annotated })` / `normalize` / `isInline` / `isPlain` (JS); `import_markdown` / `export_markdown(c, annotated=)` / `normalize` / `is_inline` / `is_plain` (py) | **idiom**: module functions holding no document; an annotated export answers `{ markdown, anchors }`, each anchor `{ id, line }` |
| Opaque store | `doc.unchecked().store_field(card, name, value)` | `doc.unchecked.storeField(addr, value)` (JS, `Addr`), `doc.unchecked.store_field(name, value, card=)` (py) | **idiom**: the quill-free write, coercion deferred to render, each content value stored in its canonical form; WASM folds the card and field into one `Addr` |
| Parse + warnings | `Document::parse(md) -> Parsed { document, warnings }` | `Document.parse(md)` → `doc.warnings` getter (both) | **FFI**, the wrapper fuses `Parsed` + `Document` into one session object: `parse` returns the document directly and stashes the parse `warnings` on it (`doc.warnings`), each body's `parse::dropped_construct` among them; a field's markdown string reports none here, since no schema says which field is `richtext`, and reports from `quill.parse`'s conform or `quill.validate(doc)`. That getter is a deliberate asymmetry with core, where warnings live only on `Parsed`: it is session state, so `equals` and the storage DTO exclude it, `loadStored`/`fromStored` leave it empty, and the writer's `reviseDocument` clears it |
| Bound parse (the primary ingestion path) | `quill.parse(md) -> Parsed` | `quill.parse(md)` (JS), `quill.parse(md)` (py) | **FFI**: same `Parsed`-fusing wrapper as `Document.parse`, so parse warnings *and* the conform's warnings, a `richtext` field's `parse::dropped_construct` among them, ride the one `doc.warnings` carrier. A `$quill` naming a different quill throws (core: the `BoundParseError` mismatch half) rather than conforming under the wrong schema |
| Authoring import | `quill.from_authoring(&value) -> Parsed` | `quill.fromAuthoring(value)` (JS), `quill.from_authoring(value)` (py) | **FFI**: the bound parse's `Parsed`-fusing wrapper, over the [authoring document](AUTHORING.md) rather than Markdown. A shape error names its authoring pointer in `args.pointer` |
| Render date | `today: CalendarDate` on `Engine::open` / `Engine::render` / `Quill::resolve` / `reader.resolve` | an optional trailing `today` on `engine.open` / `engine.render` / `reader.resolve` (JS, `YYYY-MM-DD`), `engine.render(.., today=)` (py, a `datetime.date`); omitted, the host's local date | **idiom**: a binding is the host, so it supplies the local date core never reads ([SCHEMAS.md](SCHEMAS.md#the-render-date-today)) |
| Font library | `Engine::with_fonts(files)?`, `engine.font_faces(&quill)` | `new Engine({ fonts })` / `engine.fontFaces(quill)` (JS), `Engine(fonts=)` / `engine.font_faces(quill)` (py) | **idiom**: JS reads the files when the engine first loads a Typst backend, so a refused file rejects that first verb rather than throwing at `new Engine`, and `fontFaces` returns a promise as every `Engine` verb does. A face's bytes cross as a `Uint8Array` (JS) or `bytes` (py) |
| Engine backend roster | `engine.registered_backends()` | `engine.registered_backends()` (py) | **scope**: Python only. WASM answers the same question at build time: `build-wasm.sh` emits a Typst-free `core` variant alongside the default one, so which backends exist is a property of the artifact a consumer imported, not a runtime read |

The single **idiom** row on the typed entry is the honest cost: the typed writer is the one shape pyo3 carries worst, so its "identical" is qualified, not claimed.

## Python: `crates/bindings/python`

PyO3 bindings published as `quillmark` on PyPI. A `snake_case` surface over the shared model; one-shot `engine.render` (no canvas).

> **Scope: Tier 1 + storage + render.** Field I/O flows through `quill.writer(doc)` / `quill.reader(doc)` exclusively; `Document` is quill-free data and structure (parse, the storage DTO, `unchecked.insert_card` / `unchecked.store_field`, `remove_card` / `move_card`, `remove_field`, the `card` / `seed_overlay` reads, `$ext` / `$seed`, and the comment-anchor `anchor_contexts` / `reattach`).
>
> The content write primitive and reads (`read` / `to_markdown_at` / `apply` / `revise` / `replace`) and every change helper of the parity table's row are on Python's `Document` and module, so a server writes a value with the same receipts an editor gets. So are the content-model functions, `import_markdown` / `export_markdown` / `normalize` / `is_inline` / `is_plain`, and the document's bytes and sync, `save` / `from_bytes` / `heads` / `save_since` / `merge` / `sync_state` / `generate_sync_message` / `receive_sync_message`, so a server is a sync peer of the editors it serves.
>
> Anchor identity is not editor-only. A server writing a document that holds comment threads keeps them through the writer's revise verbs, which return the whole receipt, and `anchor_contexts` / `reattach` on `Document` ([COMMENTS.md](COMMENTS.md)).

Every Python verb is identical to its core/WASM twin or names its one difference in the parity table above:
`card=None` selectors fold every composable-card twin onto one axis — the `Writer` / `Reader` verbs, `$ext`, `remove_field` — and `revise` / `revise_document` return their whole receipt as a dict, its warnings the `Diagnostic` objects every writer verb returns. No half-mirrored surface remains to drift.

**The surface is typed.** pyo3 docstrings are runtime-only, so the wheel ships a `py.typed` marker beside a hand-written `_quillmark.pyi`: without them every class resolves to `Any`. The stub is signatures only, the prose staying in `src/`, and nothing gates the two together: unlike WASM's `runtime.d.ts`, which CI typechecks against the generated backend declarations, a Python verb can be added or resignatured without the stub following. `python -m mypy.stubtest --ignore-disjoint-bases quillmark` is the check to run by hand when it changes.

## WebAssembly: `crates/bindings/wasm`

wasm-bindgen bindings published as `@quillmark/wasm`. Builds with `--target web` and `--weak-refs` so wasm-bindgen handles are reclaimed by `FinalizationRegistry`; `.free()` remains as the eager teardown hook. Requires Node 22+ / current evergreen browsers.

**The runtime owns instantiation.** `--target web` emits no `.wasm` ESM import and no top-level await, so nothing in the package graph forces a bundler plugin and a static import of `@quillmark/wasm` is safe anywhere, SSR included. The cost is that instantiation becomes explicit: `const { Quill, Document } = await init()` before the sync surface is touched.

- **Core only.** `Engine` instantiates each backend itself, inside the lazy load, so a consumer initializes core and nothing else.
- **Memoized on the promise**, so several entry points may each await it for one instantiation: the pattern is a destructure at the top of every entry point, not one trusted startup site.
- **Both failure codes reject**, `runtime::init_conflict` included, keeping the delivery rule in [ERROR.md](ERROR.md) § "Bindings Error Delegation" true at the one export that could break it.
- **One contract everywhere.** The binary streams from a URL in a browser and is read off disk under Node, resolved through the `#quillmark-env` subpath import rather than a runtime `typeof process` branch.

The `--target bundler` alternative emits `import * as wasm from "./wasm_bg.wasm"`, which no browser and no bundler resolves natively; the plugin that fixes it rewrites the import to a top-level await, and because the runtime statically imports core, that await lands on every consumer's static module graph. `build-wasm.sh` asserts the built artifacts carry neither form.

**The gate is the only way in, so a pre-init call is unwritable.** `init` resolves to the core surface — the core build's values, which are exactly what its instance stands behind — and the runtime layer exports none of them statically, so a handle cannot be obtained without having awaited. With one entry in the `exports` map there is no subpath around it either. This is what makes the precondition structural instead of a convention a consumer has to know. A floating `init()` is an ESLint rule rather than a `tsc` diagnostic, so a signature is no guard, and a load order that reaches an initialized runtime by luck passes in dev and in the test that covers it.

Everything needing no instance stays a static export, unawaited: `MAIN_CARD_ADDR`, `VARIANT_DISCRIMINANT_KEY`, `isQuillmarkError`. So do `Engine`, `LiveSession` and the three writer/reader classes, which their **arguments** gate rather than the entry — every `Engine` verb takes a `Quill` first, and the writer/reader constructors take both handles, so a caller who has not awaited cannot produce an argument to call them with. Holding them out of the gate keeps them tree-shakable, so the editor path drops the dispatcher it never calls.

The two constructors taking no handle reach no wasm: `new Engine()` validates a descriptor map and a font list, and a `LiveSession` forwards to the backend session `engine.open` is the sole source of. None of the six carries a static method, the one member shape an argument cannot gate. `gate.test.js` is the executable guard, driving the whole static surface before `init`.

Ships **multiple artifacts from one crate** behind a single public root export. The root `@quillmark/wasm` is a hand-written **canonical runtime layer** that hands out the internal Typst-less **core** build's `Document` + `Quill` (load / validate / schema / seed / authoring) verbatim and adds an `Engine` render dispatcher.

The **render** build, carrying both backends (Typst and acroform), is a **private** build with its own linear memory, lazily loaded on the first render or `engine.load(quill)`: there is no public `/core` or `/render` subpath. Core is small and eager; the render build is megabytes (Typst dominates) and loads only when something asks for it. `load` exists so a host can start that fetch without a document: `open` snapshots the document before it awaits the load, so a session opened mid-load compiles a stale one. `scripts/build-wasm.sh` enforces core's gzip ceiling, failing the build when Typst leaks back into the no-features build.

Backend handles never escape the `Engine`: it clones the quill tree + `doc.toStored()` into the backend's memory as serialized data. The document clone is freed after each call; the quill clone is cached in a per-`Engine` `WeakMap` keyed on the canonical `Quill`, so it is dropped with that `Quill`.

The storage DTO carries no warnings, so the clone the backend renders knows nothing of the load's, and `Engine.render` returns the backend render's own: `quill.validate`'s warnings but `backend::declined_construct`, then the compile's, which carry that decline ([ERROR.md](ERROR.md) § "Warning flow"). The load's stay on `doc.warnings`. `LiveSession.render` carries the compile's alone.

**An options object refuses a key it does not read.** Its reader looks up the names it declares and never visits the rest (`serde_wasm_bindgen` ignores `deny_unknown_fields`), so a misspelled option would read as absent and the call go ahead on its default: `engine.render(quill, doc, { today })` would render the local date. `RenderOptions`, on both render verbs, and `EngineOptions` throw on such a key instead, as an `Addr` and a `CardInput` do, through one key walk per layer (`reject_unknown_keys` in the binding, `rejectUnknownKeys` in the runtime).

The reader also reaches a non-enumerable key and one up the prototype chain, so the walk reads every own name, a key holding `undefined` included. It takes only a plain object, one whose prototype is `null` or has a `null` prototype itself, as `Object.prototype` of any realm does: a `Map`, a class instance and `Object.create({ … })` throw. That prototype may be a null-prototype object of defaults, so the walk reads its own names too, but those `Object.prototype` holds. The refusal carries no code: it is a call site to fix, not a condition to route on.

A payload item and a `$ext` or `$seed` value are read whole rather than by declared name, so they see own enumerable keys alone: a non-enumerable or inherited key there is neither read nor refused.

**Exactly one copy of the package per process.** Two copies are two core builds (two linear memories, two `Quill`/`Document` classes), and no topology legitimately loads a multi-megabyte binary twice and needs handles to cross between the copies. Every seam taking a core handle checks it and throws a `QuillmarkError` naming the duplicate install, including the ones that could cross as data (`Engine`, `LiveSession.update`). Errors are the exception: `isQuillmarkError` stays structural, an error being data rather than a handle.

Beyond the byte-output verbs (`engine.render`, `LiveSession.render`), the render build exposes a **live preview** path on `LiveSession` for both backends (`update`, `pageCount`, `pageSize`, `paint`, …), canvas paint being required of the session seam rather than declared. See [PREVIEW.md](PREVIEW.md).

## CLI: `crates/bindings/cli`

Standalone `quillmark` binary. See [CLI.md](CLI.md).

## Links

- [PROGRAMMATIC.md](PROGRAMMATIC.md): building documents in memory through each surface's mutators
- [CLI.md](CLI.md): command-line surface
- [PREVIEW.md](PREVIEW.md): WASM multi-backend canvas preview (Typst, acroform)
- [ERROR.md](ERROR.md): the diagnostic model that crosses every boundary
- Per-binding API detail: the respective `crates/bindings/*/` rustdoc and READMEs
