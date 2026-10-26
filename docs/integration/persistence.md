# Persistence

A `Document`'s in-memory layout tracks the evolving Quillmark model and is not a stable interface. To store a document (a database row, a cache, a message payload) without persisting Markdown (whose syntax also evolves), serialize to a **versioned JSON envelope** with `to_stored`. That is the one form frozen per schema version; `to_markdown` round-trips but its syntax moves.

What `quill.reader(doc)` hands a consumer to edit is the values form — content
as its codec's text, sparse, carrying neither anchors nor `$quill`. It is a
projection, never a stored row: persist with `to_stored`.

## Round-trip

=== "Python"

    ```python
    blob = doc.to_stored()              # versioned JSON string
    # … store blob …
    doc = Document.from_stored(blob)    # exact reconstruction
    ```

=== "JavaScript"

    ```javascript
    import { init } from "@quillmark/wasm";

    const { Document } = await init();

    const blob = doc.toStored();
    const doc2 = Document.fromStored(blob);   // Document.storageVersionOf(blob) first to test without throwing
    ```

Every blob carries a `schema` tag (`quillmark/document@<version>`). Readers dispatch on it, accept every still-supported past version by migrating forward on read, and **reject an unknown version** rather than guessing. A save writes the oldest tag that holds the document: `quillmark/document@0.131.0` for one holding a font mark, an element named `table`, a merged table cell or a table `head` 0.130 cannot draw, `quillmark/document@0.129.0` for any other.

## Byte-stability

Equal documents serialize to byte-equal JSON, and re-serializing under any later release carrying the same `schema` tag reproduces those bytes — so a stored document can be content-hashed for cache keys or template-divergence detection. Only a `schema`-version bump may change the layout.

A row still carrying an older schema tag migrates forward on read; that migrated form's bytes become stable once you **rewrite the row under its current tag** (read-repair).

Full model: [DOCUMENT_STORAGE.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/DOCUMENT_STORAGE.md).
