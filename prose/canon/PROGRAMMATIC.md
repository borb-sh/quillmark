# Programmatic Document Construction

> **Implementation**: `crates/core/src/document/` (the `edit` module), mirrored by every surface in `crates/bindings/`

## TL;DR

A `Document` is built and mutated in memory (no Markdown text involved)
through validated constructors and mutators: `Document::new` (blank canvas),
`Card::new`, `store_field` / `store_fields`, `push_card`. Every mutator enforces
the same field-name, depth, and kind invariants the Markdown parser does, so a
constructed document cannot be invalid — with no bypass, since `Payload`'s own
mutation half is crate-internal and `card.payload()` is a read view. This is the
authoring surface for
programs (database row → rendered PDF); Markdown serves human authoring and
the blueprint serves LLM/MCP consumers.

## Three authoring surfaces, one model

| Surface | Consumer | Entry |
|---|---|---|
| card-yaml Markdown | humans | `Quill::parse` (the bound door; `Document::parse` is the quill-free transport one) |
| annotated blueprint | LLMs / MCP | `blueprint()` → fill → `Quill::parse` |
| structured mutators | programs | `Document::new` → `store_fields` / `push_card` |

All three produce the same `Document`; render, validation, storage, and
emission do not distinguish how it was built. `to_markdown()` emits canonical
Markdown from any of them, so a programmatically built document round-trips
the same emitter parsed ones do.

## Blank canvas vs seeded starter

`Document::new(quill_ref)` is the blank canvas: a main card with no user
fields, an empty body, and no composable cards. Absent fields resolve at
render time (schema `default`, else the field's blank; see
[SCHEMAS.md](SCHEMAS.md)), so nothing the program did not set reaches the
output.

`Quill::seed_document()` is the illustration-first starter: `example` values
committed, one card per declared kind: the structured twin of the blueprint
(see [BLUEPRINT.md](BLUEPRINT.md)). Hand it to a human or an editor as
something to edit; start from the blank canvas when the data is authoritative
and example values would pollute it.

## The flow

Python shown; Rust and WASM mirror it method-for-method:

```python
doc = Document("invoice")                       # blank canvas
w = quill.writer(doc)                            # schema-bound: coerce + check at the write
w.set_all({"customer": row.name, "total": row.total})
for item in row.items:
    w.add_card("line_item", {"desc": item.desc, "qty": item.qty})
result = engine.render(quill, doc, OutputFormat.PDF)
```

Values convert in place at each boundary (Python objects, JS values, Rust
scalars via `Into<QuillValue>`); no surface asks the caller to serialize
YAML or Markdown.

The whole document is one call in each direction. `quill.reader(doc).values()`
reads it in the values form and `quill.writer(doc).set_values(values)` writes
that shape back, so a program holding a whole form or an API payload issues one
verb rather than folding it over `set_all` and `add_card` itself
([SCHEMAS.md](SCHEMAS.md) § "The values form"). It is the typed lane:
`set_values` refuses an undeclared name exactly as `set_all` does, and reports
every refused cell at once under its own `DocPath`. Where `set_all` merges,
`set_values` replaces each axis it names and leaves the rest alone.

## Validation: batched, atomic, at the boundary

Structural invariants (field-name grammar, value depth, card kind) are
enforced per mutator call. `store_fields` validates its whole batch before
applying any of it: on violation nothing is applied and the single error
carries one diagnostic per offending field with `path` set to the field name:
externally sourced names (database columns, form keys) surface every violation
in one pass. Schema validation (types, enums, constraints) is a separate pass:
deferred to `Quill::validate` / render for the opaque store, or pulled forward
to the write by typed commit (below).

## Two write disciplines: opaque store vs typed commit

Document mutation is a data primitive that never requires a Quill. `store_field`
/ `store_fields` (the opaque **store**) hold only a `$quill` *reference* and
enforce the structural invariants above. They store the value verbatim,
deferring coercion to render.

Typed commit is a schema-bound layer over that primitive: `Quill::writer(&mut doc)`
binds the resolved schema, and its `set` / `set_all` resolve each field's `type`,
coerce to the canonical form (`"3"` → `3`, a markdown string → a richtext
content), and fail at the write on a mismatch: the default whenever a Quill is
in hand. A name the schema does not declare fails with `EditError::UnknownField`
rather than falling to the opaque store: on the typed path an undeclared name is
a typo, not a fallback, so it is refused at the write rather than surfacing later
at validation. `set_all` is all-or-nothing and reports every undeclared name
(and every conform failure) in one pass, so a whole-form batch surfaces every
typo at once.

**Conform is the third stratum, and it is the typed commit run by the schema
instead of by a caller.** `Quill::conform(&mut doc)` walks every declared
content field through the same strict write `set` commits through, so a
document that arrived through the opaque primitive (a parse, a stored row, a
`store_field`) lands where the typed writer would have put it, whichever lane
built it ([SCHEMAS.md](SCHEMAS.md) § "Content fields rest per codec").
`Quill::parse(md)` is parse-then-conform and the documented ingestion path.
Where the writer refuses a value, conform leaves it authored and reports a
`conform::*` warning: an ingestion pass must open a document it can repair, not
reject it.

The primitive stays load-bearing: it is what lets a `Document` be constructed
and `from_stored`'d with no bundle (standalone data), what quill-agnostic
storage/migration infra writes through, what a store-now-validate-later editor
uses to hold not-yet-conforming input, and the way to store a value opaquely on
purpose. Reach for the opaque `store_*` for those; reach for the writer by
default. `Quill::writer(&mut doc)` is the documented front door in every
surface: `quill.writer(doc)` in WASM and Python alike (the schema-bound
`DocumentWriter` / `Writer` with `set` / `set_all` / `revise_body` / `revise_field` /
`add_card` / `card(i)`); the quill owns the schema, so it is the factory. The
`_commitField` / `_commitFields` / `_reviseField` verbs (addressed by `Addr`) are
the stable ABI underneath it, and `storeField` / `storeFields` remain the
quill-free opaque store. See [BINDINGS.md](BINDINGS.md) for the write surface, the
`store` / `set` / `overwrite·revise·apply` vocabulary rule (a ladder by anchor
fate, governing user-field writes), and the core-vs-bindings parity table.

## Addressing cards for re-render

Card mutators address by index, and the engine offers no durable card handle: a `remove_card` / `add_card` moves every index after it. For patch-and-re-render automation (a source row changed, re-render the document), carry your own key in the card's `$ext` under a namespace you own, and resolve the index when patching:

```python
ext = doc.cards[index]["ext"] or {}                                # at build time
doc.store_ext({**ext, "myapp": {"row_id": row_id}}, card=index)
idx = next(i for i, c in enumerate(doc.cards)                      # at patch time
           if (c["ext"] or {}).get("myapp", {}).get("row_id") == row_id)
quill.writer(doc).card(idx).set_all({"qty": new_qty})
```

`$ext` round-trips through Markdown and the storage DTO and never reaches a backend ([CARDS.md](CARDS.md) § Out-of-band Metadata). It rides the values form too, so a key stamped here survives a `values()` → edit → `set_values` cycle: the shape carries `$ext` precisely because this pattern depends on it. **The engine guarantees nothing about what a consumer puts there**: no uniqueness, no collision check, no repair on a hand-edited file. A key duplicated across two cards resolves to whichever the scan hits first. Namespacing (`$ext.myapp`) is what keeps two tools on one card from colliding, and it is a convention, not an enforced rule.

Patching one card at a time is for a document that is no longer a pure projection of its source data: where data → document is a pure function, rebuild instead. Reach for this only when a rebuild would destroy accumulated hand edits.

## Links

- [SCHEMAS.md](SCHEMAS.md): schema model and the blank-filled render projection
- [BLUEPRINT.md](BLUEPRINT.md): the LLM/MCP authoring surface
- [CARDS.md](CARDS.md): `$seed` overlays for editor-spawned cards
- [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md): persisting built documents
- [BINDINGS.md](BINDINGS.md): the language surfaces that mirror this API
