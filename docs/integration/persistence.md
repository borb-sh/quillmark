# Persistence

A `Document` is held in one Automerge document. Store it as that document's **bytes**: `save` answers them, history included, and `from_bytes` reads them back with every write base they reach, ready to merge and sync. `to_markdown` drops every comment anchor and engine-minted id, and its syntax moves.

What `quill.reader(doc)` hands a consumer to edit is the values form — content
as its codec's text, sparse, carrying neither anchors nor `$quill`. It is a
projection, never a stored row: persist with `save`.

## Round-trip

=== "Python"

    ```python
    row = doc.save()                    # Automerge bytes
    # … store row …
    doc = Document.from_bytes(row)      # the same document, history included
    ```

=== "JavaScript"

    ```javascript
    import { init } from "@quillmark/wasm";

    const { Document } = await init();

    const row = doc.save();                  // Uint8Array
    const doc2 = Document.fromBytes(row);
    ```

The revision is `doc.heads()`; `doc.saveSince(heads)` (`save_since`) answers the changes past it and `doc.merge(bytes)` takes another peer's, so a store can append increments rather than rewrite the row. Two peers converge through `generateSyncMessage` / `receiveSyncMessage` with a `SyncState` per peer.

## The JSON envelope

`to_stored` serializes to a **versioned JSON envelope** frozen per schema version, for a reader that holds no Automerge or a row whose history must not travel. Every blob carries a `schema` tag (`quillmark/document@<version>`): readers dispatch on it and **reject an unknown version** rather than guessing. A save writes `quillmark/document@0.135.0`. A `0.134.0` row loads with its content migrated; save it as bytes to keep it from then on (`Document.fromStored(row).save()`). A document read from JSON holds no history, so no write base read before the load merges ([DOCUMENT_STORAGE.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/DOCUMENT_STORAGE.md#schema-versioning)).

Two documents of different histories hold different ids and bytes for the same content: compare them by what they read, not by their rows.

Full model: [DOCUMENT_STORAGE.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/DOCUMENT_STORAGE.md).
