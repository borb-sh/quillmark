# Persistence

A `Document`'s in-memory layout tracks the evolving Quillmark model and is not a stable interface. To store a document (a database row, a cache, a message payload) without persisting Markdown (whose syntax also evolves), serialize to a **versioned JSON envelope** with `to_stored`. That is the one form frozen per schema version; `to_markdown` drops every comment anchor and engine-minted id, and its syntax moves.

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

Every blob carries a `schema` tag (`quillmark/document@<version>`). Readers dispatch on it, accept every still-supported past version by migrating forward on read, and **reject an unknown version** rather than guessing. A save writes `quillmark/document@0.135.0`, which stores each body and richtext field as the content model: blocks, containers, anchors and island slots carrying ids. A row under an older tag loads with its content migrated, and gets the same ids on every load.

## Moving a document to another quill

A stored document names one quill in `$quill`. To open it with another quill, or with a later version of the same one, retarget the reference and conform: `set_quill_ref`, then `quill.conform(doc)`. Quillmark rewrites only what the new quill reads as another type, and keeps everything else as written.

`Quill.compare` says what that move does before any document makes it. It reads the two schemas and no document, so one answer covers every document stored against the first quill:

=== "JavaScript"

    ```javascript
    const delta = Quill.compare({ from: resume, to: resumeModern });

    if (delta.carries) {
      doc.setQuillRef("resume_modern@0.1");
      resumeModern.conform(doc);
    }
    ```

=== "Rust"

    ```rust
    let delta = Quill::compare(&resume, &resume_modern);

    if delta.carries() {
        doc.set_quill_ref("resume_modern@0.1".parse()?);
        resume_modern.conform(&mut doc)?;
    }
    ```

Each entry names a slot by its card kind and its path under the card (`skills`, `contact.email`, `skills.0.name`, or `$body`), and gives it one class:

| Class | The two quills | A value the document stored there |
|---|---|---|
| `same` | declare the slot alike | reads as it did |
| `widened` | the new quill admits more: an enum member added, a wider `shape`, `integer` to `number` | reads as it did |
| `added` | only the new quill declares it | none yet, so the slot renders its default or blank |
| `narrowed` | the new quill admits less | stays as written, and fails `validate` if it is a value the new quill refuses |
| `retyped` | the new quill reads another type | is rewritten by `conform`, with loss from text to `richtext` |
| `unclaimed` | only the old quill declares it | stays in the document, prints nowhere, and warns `validation::unknown_field` |

`carries` is true when every entry is `same`, `widened` or `added`. Moving such a document back returns it byte for byte.

An entry also carries `default_changed`, set where both quills declare the slot with different `default:`s. No stored value changes, and a document that left the slot unanswered prints the new default. A label, a description, `ui`, `max` and field order are not compared: no stored document reads them.

An entry classed `retyped`, `unclaimed` or `added` stands for everything beneath it, and so does the entry for a card kind only one quill declares, which has no `path`. A matrix is one slot, classed by its members and columns together.

Python does not expose the comparison.

## Transient anchors

An anchor whose id opens with `~` lives only in memory. The prefix is `TRANSIENT_ANCHOR_PREFIX`, exported from `@quillmark/wasm` and from `quillmark_content::model`. Such an anchor moves with its text through every write, rides `getStored`, `clone` and every other read of the live document, and is named in a receipt's `droppedAnchors` when an edit deletes its text. `to_stored`, the annotated markdown read and every hash leave it out, so placing or removing one never moves a hash or stales a writer's `base`, and a reload drops it. Equality still sees it.

That fits what a host keeps for one session, such as a suggestion or a highlight, beside its own record under the anchor's id. A rewrite of the anchored text keeps the anchor on whatever text the diff kept, rather than dropping it. A host whose record depends on the exact text stores the quote it read and compares it with `anchorContext(id).quote` before acting.

## Byte-stability

Equal documents serialize to byte-equal JSON, and re-serializing under any later release carrying the same `schema` tag reproduces those bytes — so a stored document can be content-hashed for cache keys or template-divergence detection. Only a `schema`-version bump may change the layout.

A row still carrying an older schema tag migrates forward on read; that migrated form's bytes become stable once you **rewrite the row under its current tag** (read-repair).

The rules behind this: [Quillmark invariants § Storage](https://github.com/borb-sh/quillmark/blob/main/prose/INVARIANTS.md#storage).
