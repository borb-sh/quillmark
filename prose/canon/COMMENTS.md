# Comment Anchors

> **Implementation**: `crates/content/src/` (the `anchor` module), `crates/core/src/document/` (the `anchors` module)
> **Related**: [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [BINDINGS.md](BINDINGS.md)

## TL;DR

A comment is a thread the host stores, joined to the document by one `anchor{id}` mark over the text it discusses.
The document carries the anchor and nothing else, so a reply never moves its bytes.
Core makes the anchor a reliable handle: it reads what each anchor is attached to, names every anchor a write drops, and places a dropped anchor again where its text is found exactly once.

## The contract

- **The thread lives outside the document.** No body, author or status enters `Content`. The host keeps the thread, keyed by the anchor's id.
- **The thread's id is the anchor's id.** The client mints it, as it mints every anchor id ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md#anchor-id-identity)). Quillmark mints none.
- **The host mints ids unique per document.** The content model guarantees anchor-id uniqueness per `Content` only, so the contract requires the host's mint to be unique across the document, as a UUID is. `anchor_context(id)` and the live-id check take the first value holding an id.
- **A write carrying anchors goes through a rung that keeps them.** `applyChange` carries them through a splice, and `revise` / `reviseContent` / `reviseDocument` through a diff.
  `overwrite` destroys them and is never the write for a document holding threads ([BINDINGS.md](BINDINGS.md#the-write-surface-object-placement-over-one-primitive)).
- **A write never deletes a thread.** Every revise receipt names the anchors it dropped. A dropped anchor is either placed again by `reattach` or reported orphaned, and the thread stays in the host's store either way.

## Reading a selector

`anchor_contexts` answers, for each prose anchor of a body or content value, its selector: `id`, `path`, `quote`, `prefix` and `suffix`.

- `path` is the `DocPath` of the body or content value holding it.
- `quote` is the anchored text as stored: an inline island is its `U+FFFC` slot and a line break its `\n`. A point's quote is empty.
- `prefix` and `suffix` are up to 32 Unicode scalar values of stored text on each side, cut at the value's edges.

A quote spanning an inline island (a footnote reference) includes its slot character, and is found again only where a slot sits.
The read is pure and cheap enough to run before every markdown write.

## The matching rule

`reattach` runs on anchors `revise` already refused to move. `revise` floors a move at four characters and refuses a match in surviving text, so this rule is at least as strict, or it would undo that refusal. Per value, never guessing:

1. The quote is four scalar values or more. A shorter quote orphans.
2. Every occurrence of the quote in the stored text counts where its context agrees: the text before ends with the last eight scalar values of `prefix`, and the text after starts with the first eight of `suffix`.
3. A `prefix` or `suffix` shorter than eight was cut at the value's edge, so it agrees only as the whole text on that side, edge included.
4. A point (an empty quote) counts at each position where the prefix and the suffix both agree.
5. Where the caller passes the write's delta for the path, a match must overlap text that write inserted, the discipline `revise` keeps. Without one, the context rule alone decides.
6. Exactly one counting place: attach there. None or several: orphan. No nearest match, no partial quote, no edit distance.
7. An id already live anywhere in the document, a table cell's anchor included, is not re-added. It is answered as reattached with the selector the first value holding it reads, or with the selector as given where only a table cell holds it.
8. Each id is answered once: of selectors sharing an id, the first is answered and the rest are left out of the receipt.

A range lands exactly over the quote's characters. A point lands at the one position where the prefix meets the suffix, as an empty range.

Matching is path-strict. A path naming no body or content value orphans its selector, and core remaps no card index. `DocumentRevised.from` is the alignment a caller remaps a dropped selector's card index through before calling `reattach`.

## Drift

`revise` can keep an anchor on different words: it relocates a moved quote to the first occurrence overlapping inserted text, and a word-level rewrite can leave both ends of a range over new text. `reattach` never sees those anchors.
A caller compares each surviving anchor's selector after the write to the one it read before, and a changed quote is the drift it shows. Nothing moves an anchor on its own.

## Table cells

An anchor in a table cell sits outside the op surface and the id-uniqueness check ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md#anchor-id-identity)), so cells are outside the comment contract: `anchor_contexts` does not read one, and no receipt names one a write drops.
A comment on a table anchors the table's slot: a one-character range over its `U+FFFC`, a prose mark that the receipts list and the rebase rule moves with the table.
Its quote is the slot alone, under the four-character floor, so once a write drops it `reattach` orphans it.
Bringing cell anchors into the op surface and the uniqueness check is the step that makes a cell comment honest.

## Hosting

A host holding threads persists the `StoredDocument` or keeps the live document and revises it.
It never reopens from markdown: `toMarkdown` drops every anchor, and `equals` counts anchors, so an echo guarded by `equals` never matches once a document holds one.
A host that stores markdown recovers by running `reattach` over every stored selector after the open, under the same rule. Recovery is a search that can orphan, so it is the fallback, not the design.

## Links

- Per-field rule and constants: the `anchor` module of `quillmark-content` (`anchor_contexts`, `locate`, `Content::reattach`).
- Document walk and the path writer: `Document::anchor_contexts`, `anchor_context`, `reattach`.
- Receipts: `Revised.dropped_anchors` and `DocumentRevised.dropped_anchors`.
