# Programmatic Construction

Build a `Document` in memory (no Markdown text) through validated, schema-bound mutators. Every write enforces the same field-name, depth, and kind rules the Markdown parser does, so a constructed document cannot be invalid. This is the surface for programs: a database row becomes a rendered PDF without assembling YAML or Markdown by hand.

Markdown authoring, the [blueprint](../quills/blueprint.md) (for LLMs), and these mutators all produce the same `Document`; render, validation, and storage do not distinguish how it was built, and `to_markdown()` round-trips any of them.

## Blank canvas vs seeded starter

- **`Document(quill_ref)`** is the blank canvas: a main card carrying only `$quill`, an empty body, no cards. Absent fields resolve at render time (schema `default:`, else the field's blank), so nothing you did not set reaches the output. Start here when the data is authoritative.
- **`quill.seed_document()`** is the illustration-first starter: `example:` values committed, one card per declared kind. Hand it to a human or an editor to fill in; see [Blueprint & Seeding](../quills/blueprint.md).

## The typed writer

`quill.writer(doc)` is the schema-bound front door. It resolves each field's declared type, coerces the value (`"3"` → `3`, a Markdown string → richtext content), and fails at the write on a mismatch. A name the schema does not declare is a typo (`edit::unknown_field`), not a silent fallback. `set_all` and `add_card` are atomic: on any bad field nothing is applied, and the raised error carries one diagnostic per offending field, so a whole-form batch surfaces every problem at once.

=== "Python"

    ```python
    from quillmark import Document, Quill, Quillmark, OutputFormat

    engine = Quillmark()
    quill = Quill.from_path("invoice-quill")

    doc = Document("invoice")                       # blank canvas
    w = quill.writer(doc)
    w.set_all({"customer": row.name, "total": row.total})
    for item in row.items:
        w.add_card("line_item", {"desc": item.desc, "qty": item.qty})

    result = engine.render(quill, doc, OutputFormat.PDF)
    ```

=== "JavaScript"

    ```javascript
    const doc = new Document("invoice");            // blank canvas
    const w = quill.writer(doc);
    w.setAll({ customer: row.name, total: row.total });
    for (const item of row.items) {
      w.addCard("line_item", { desc: item.desc, qty: item.qty });
    }
    const result = await engine.render(quill, doc, { format: "pdf" });
    ```

`add_card` fuses build + typed-commit + insert in one atomic call: `at` inserts at an index, and absent it appends. Values convert in place at each boundary (Python objects, JS values); no surface asks you to serialize YAML.

## Reading fields back

`quill.reader(doc)` is the read twin: `reader.get(name)` returns each field in the values form (every content leaf as its codec's text, a `richtext` leaf as Markdown, everything else as stored) with schema authority, so an undeclared name raises `edit::unknown_field` rather than reading back nothing. A read never coerces: `qty: "3"` reads `"3"` until a write canonicalizes it; `reader.resolve()` (JavaScript) is the coerced, blank-filled render view.

`reader.get_content(name)` (`getContent` in JavaScript) is the same read at the other end of the codec, returning the field's content corpus rather than its projection. Reach for it when you hold a content editor: a content field rests as a corpus when the typed writer committed it and as the authored string when a Markdown parse produced it, and this read decodes both through the codec the declared type names. A type that is not a content leaf raises `edit::field_not_content`: an `integer` has no corpus, and an `array<richtext>` carries content without having one.

## Addressing cards for re-render

Card mutators address by index, and the engine offers no durable card handle: a `remove_card` / `add_card` moves every index after it. For patch-and-re-render automation (a source row changed, re-render the document), carry your own key in the card's `$ext` under a namespace you own, and resolve the index when patching:

```python
ext = doc.cards[index]["ext"] or {}                                # at build time
doc.store_ext({**ext, "myapp": {"row_id": row_id}}, card=index)
idx = next(i for i, c in enumerate(doc.cards)                      # at patch time
           if (c["ext"] or {}).get("myapp", {}).get("row_id") == row_id)
quill.writer(doc).card(idx).set_all({"qty": new_qty})
```

`$ext` round-trips through Markdown and the storage DTO and never reaches a backend. The engine guarantees nothing about its contents: no uniqueness, no collision check, no repair. A key duplicated across two cards resolves to whichever the scan hits first.

Patching one card at a time is for a document that is no longer a pure projection of its source data: where data → document is a pure function, rebuild instead.

## Scope note

The typed writer is the recommended path in both bindings. JavaScript additionally exposes a quill-free **opaque store** (`storeField` / `storeFields`, coercion deferred to render) and an anchor-preserving content lane for editors; those are WASM-only by audience: storage/migration tooling holding no quill, and live editors preserving caret identity. Python's field I/O is the typed writer and reader exclusively.

Full model: [PROGRAMMATIC.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PROGRAMMATIC.md).
