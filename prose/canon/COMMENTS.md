# Comment Anchors

> **Implementation**: `crates/content/src/` (the `lines::anchor` module), `crates/core/src/document/` (the `anchors` module)
> **Related**: [CONTENT.md](CONTENT.md), [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [BINDINGS.md](BINDINGS.md)

## TL;DR

A comment is a thread the host stores, joined to the document by one anchor, `{id, start, end}` in a value's `anchors`, over the text it discusses.
The document carries the anchor and nothing else, so a reply never moves its bytes.
Core makes the anchor a reliable handle: it reads what each anchor is attached to, names every anchor a write drops, and places a dropped anchor again where its text is found exactly once.

## The contract

- **The thread lives outside the document.** No body, author or status enters `Content`. The host keeps the thread, keyed by the anchor's id.
- **The thread's id is the anchor's id.** The client mints it, as it mints every anchor id ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md#anchor-id-identity)). Quillmark mints none.
- **The host mints ids unique per document.** A value holds each anchor id once over its text and every slot, and an `anchor` step placing an id the value holds anywhere is refused: placing and moving are two steps (`{id, start, end}` and `{id, start, end, move: true}`), so a placement never moves another comment's anchor. The model guarantees no more than that, so the contract requires the host's mint to be unique across the document, as a UUID is. A host namespaces its mint, as `c-<uuid>` does, so that the editors of a body and of each table cell or note, minting apart, never mint one id twice. `anchor_context(id)` and the live-id check take the first value holding an id.
- **Every write keeps the anchors it can.** `apply` moves them exactly as its steps move text, and `revise` / `replace` / the writer's `reviseDocument` carry them through a diff ([CONTENT.md](CONTENT.md) § "Writing a value"). A writer removes an anchor with an `anchor` step that names it; a write that deletes an anchor's whole text drops it too, and the receipt names it.
- **A write states what its writer read.** A revise diffs against the value stored now, so a person's edit made between a writer's read and its write reads as text the writer deleted, and the anchors on it drop. A host whose writer reads, thinks and then writes (an LLM) passes the read ([BINDINGS.md](BINDINGS.md#the-write-surface-object-placement-over-one-primitive)):
  - the `base` of `apply`, `revise` and `replace` rebases the write over the change since, so an anchor placed late lands on the text it was read on, and refuses only where the two touch the same text or record (`edit::rebase_conflict`);
  - the writer's `baseHash` refuses the write once the value has changed (`edit::stale_base`);
  - the writer's `baseMarkdown`, the annotated markdown the writer read, merges its edit onto the change card by card, field by field and line by line, so both land and the anchors on the person's text stay; a place both changed keeps the person's version and is named in the receipt's `conflicts`.
- **A kept tag pins.** The annotated read spells each prose anchor as a `qm-anchor` tag, and a revise reads it back: a writer that moves a tag moves its anchor's start there, one that drops a tag leaves the anchor to the diff, and a tag naming no anchor the stored value holds drops with a `parse::dropped_construct` (`qm-anchor[ref]`), never minting one ([markdown-spec.md](../references/markdown-spec.md) §6.4).
- **A write never deletes a thread.** Every write's receipt names the anchors it dropped, in `droppedAnchors`. A dropped anchor is either placed again by `reattach` or reported orphaned, and the thread stays in the host's store either way.

## Reading a selector

`anchor_contexts` answers, for each anchor of a body or content value, in its prose or a cell, its selector: `id`, `path`, `card_id`, `quote`, `prefix` and `suffix`.

- `path` is the `DocPath` of the body or content value holding it.
- `card_id` is the `$id` of the composable card `path` names, and none on the main card ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md#card-id-identity)).
- `quote` is the anchored text as stored: an inline island is its `U+FFFC` slot, a hard break its `U+2028` and a block break its `\n`, in the prose and in a cell alike. A write's `droppedAnchors` and `movedAnchors` spell it the same way. A point's quote is empty.
- `prefix` and `suffix` are up to 32 Unicode scalar values of stored text on each side, cut at the value's edges, or a cell's edges for an anchor in one.

A quote spanning an inline island (a footnote reference) includes its slot character, and is found again only where a slot sits.
The read is pure and cheap enough to run before every markdown write.

## The matching rule

`reattach` runs on anchors `revise` already refused to move. `revise` floors a move at four characters and refuses a match in surviving text, so this rule is at least as strict, or it would undo that refusal. Per value, never guessing:

1. The quote is four scalar values or more, or island slots alone. Any other shorter quote orphans.
2. Every occurrence of the quote in the stored text, the prose and each cell read as its own text, counts where its context agrees: the text before ends with the last eight scalar values of `prefix`, and the text after starts with the first eight of `suffix`.
3. A `prefix` or `suffix` shorter than eight was cut at the value's edge, and agrees whole, wherever the edge now sits: a sentence written ahead of an anchor near the value's start leaves it placeable.
4. A point (an empty quote) counts at each position where the prefix and the suffix both agree.
5. Where the caller passes the write's change for the path, its receipt's `applied`, a match must overlap text that change's top-level splices inserted, the discipline `revise` keeps, and only the value's own text counts, since those splices span it alone. Without one, the context rule alone decides.
6. Exactly one counting place over the prose and every cell: attach there. None or several: orphan. No nearest match, no partial quote, no edit distance.
7. A selector stored before 0.135 spells a hard break as `\n` and a cell's block break as two; where the stored spelling counts nowhere, the rule runs once more over each surface spelled that way.
7. An id already live anywhere in the document, a cell's anchor included, is not re-added. It is answered as reattached with the selector the first value holding it reads.
8. Each id is answered once: of selectors sharing an id, the first is answered and the rest are left out of the receipt.

A range lands exactly over the quote's characters. A point lands at the one position where the prefix meets the suffix, as an empty range.
A write that inserts text at a point leaves the point before it: a point binds to the text on its left, as a caret does.

A selector naming a `card_id` finds its card by that id, wherever the card now sits: the path's card index moves to that card's.
An id no card holds, or a card of another kind than the path names, orphans the selector, since the card it was on is gone.
A selector naming none is path-strict, as is the rest of every path: a path naming no body or content value orphans its selector.

## Drift

Every revise verb is one revise at an address ([CONTENT.md](CONTENT.md) § "Writing a value"), and it can keep an anchor on different words: it relocates a moved quote of four characters or more to the first occurrence overlapping inserted text, and a word-level rewrite can leave both ends of a range over new text. `reattach` never sees those anchors.
A write's `movedAnchors` reports drift by position, a point included: it names each surviving anchor the write moved, with its quote before and after and the first `reason` that holds.

- `position`: an `anchor` step put it other than where the write's text edits carry it, as a writer moving a kept tag does, or in another slot.
- `block`: its start now sits in another block, as after a split or a join, or a paragraph the diff paired otherwise.
- `context`: its quote, or the eight characters either side that `reattach` holds a selector's context to, read otherwise, so a relocated quote with an unchanged text still reports.

A host re-reads the selector of each anchor named there. Nothing moves an anchor on its own.

## Table cells and notes

A table cell and a footnote's note are slots, each a `Content` of its own, and an anchor in one sits in that slot's `anchors`. It moves with the slot wherever its row moves. The contract holds there as in the prose:

- `anchor_contexts` reads it, at the value's path, its context cut at the cell's edges, and the annotated read lists it after the anchors before its island, with the cell's text as its `line`. No tag spells it: a cell holds none.
- The steps reach it: a `nested` step places, moves or removes it, and a placement of an id the value holds anywhere is refused ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md#anchor-id-identity)).
- A revise keeps it where the island continues and the cell at its place keeps its text, and drops it, named in the receipt, where the cell's text changed.
- A splice that deletes the island's slot, or a props step whose grid no longer names the cell, drops it with the cell, named in the receipt's `droppedAnchors` with the `island` and `slot` that held it.
- `reattach` searches every cell with the prose and places it in the one cell its quote counts in.

A comment on a table as a whole anchors the table's slot: a one-character range over its `U+FFFC`, which the receipts list and the steps move with the table.
Its quote is the slot alone, so `reattach` places it on its context like a point, and only where one slot sits between the agreeing sides.

## Text and scalar fields

A `text` field rests as a content object spelling `codec: "plain"` ([SCHEMAS.md](SCHEMAS.md) § "Content fields rest per codec"), so a comment on it is an anchor in its `anchors`, as on a `richtext` field, and the whole contract holds unchanged:

- `anchor_contexts` reads it at the field's path, `reattach` places it by the same rule, and every receipt names it where a write drops it.
- A typed or schema-free field revise and the document revise carry it through the diff of the literal text.
- The annotated read writes the field as its literal text, which holds no tag, and lists the anchor with the text of its line, as it lists a cell's. A writer that moves the text moves the anchor by the diff, never by a tag.
- A `text` field still resting as its authored string holds no anchor; the bound door conforms it to the object, and a stored selector reattaches on it from then on.

A field holding no content (an enum, a number, a date, a boolean, or a container of them) takes a comment on the whole value: a selector with a `path` and `card_id` and an empty `quote`, `prefix` and `suffix`. The document holds nothing for it.

- `reattach` resolves its card by `card_id` as it does any selector's, and answers it reattached, at the path as resolved and with nothing written, while a value sits at that path. It orphans where none does, or where the card is gone.
- A whole-field selector naming a content value is read by the matching rule, as a point with no context.
- Drift for a scalar is the value changing; the host reads the value itself and compares.

## Hosting

A host holding threads persists the `StoredDocument` or keeps the live document and revises it.
It never reopens from markdown: `toMarkdown` drops every anchor, and `equals` counts anchors, so an echo guarded by `equals` never matches once a document holds one.
A host that stores markdown recovers by running `reattach` over every stored selector after the open, under the same rule. Recovery is a search that can orphan, so it is the fallback, not the design.

## Links

- Per-field rule and constants: the `lines::anchor` module of `quillmark-content` (`Content::anchor_sites`, `anchor_contexts`, `locate_site`, `Content::reattach`).
- Document walk and the path writer: `Document::anchor_contexts`, `anchor_context`, `reattach`.
- Receipts: `Receipt.dropped_anchors`, `Revised.dropped_anchors` and `DocumentRevised.dropped_anchors`.
