# Persistence

A `Document` is held in one Automerge document. Store it as the **versioned JSON envelope**: `to_stored` answers the row, and `from_stored` reads it back into a new document. `to_markdown` drops every comment anchor and engine-minted id, and its syntax moves.

What `quill.reader(doc)` hands a consumer to edit is the values form — content
as its codec's text, sparse, carrying neither anchors nor `$quill`. It is a
projection, never a stored row: persist with `to_stored`.

## Round-trip

=== "Python"

    ```python
    row = doc.to_stored()               # the JSON envelope
    # … store row …
    doc = Document.from_stored(row)     # the same document, a new session
    ```

=== "JavaScript"

    ```javascript
    import { init } from "@quillmark/wasm";

    const { Document } = await init();

    const row = doc.toStored();
    const doc2 = Document.fromStored(row);
    ```

Every blob carries a `schema` tag (`quillmark/document@<version>`): readers dispatch on it and **reject an unknown version** rather than guessing. A save writes `quillmark/document@0.135.0`, and a `0.134.0` row loads with its content migrated.

## A live session

The Automerge bytes carry a live editing session between peers, not a row. `doc.save()` answers the whole document with its history, and `Document.load(bytes)` reads it back as a peer of the same session; `doc.heads()` is the revision, `doc.saveSince(heads)` (`save_since`) the changes past it, and `doc.merge(bytes)` takes another peer's. Two peers converge through `generateSyncMessage` / `receiveSyncMessage` with a `SyncState` per peer.

A write passes the heads its writer read, and lands there, merged with every change since ([CONTENT.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/CONTENT.md#live-state-and-merge)). A session lasts as long as its peers share one history: heads read anywhere in it resolve on every peer loaded from its bytes. A reload from the row starts a new session, in which no heads read before it resolve (`edit::stale_base`), and peers rejoin from the row, which every process loads to one origin.

Two documents of different histories hold different ids and bytes for the same content: compare them by what they read, not by their rows.

Full model: [DOCUMENT_STORAGE.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/DOCUMENT_STORAGE.md).
