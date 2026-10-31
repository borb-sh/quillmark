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
- **The host mints ids unique per document.** The ops refuse an anchor id a `Content` already holds, in its prose or a cell, but the model guarantees no more than that, so the contract requires the host's mint to be unique across the document, as a UUID is. `anchor_context(id)` and the live-id check take the first value holding an id.
- **A write carrying anchors goes through a rung that keeps them.** `applyChange` carries them through a splice, and `revise` / `reviseContent` / `reviseDocument` through a diff.
  `overwrite` destroys them and is never the write for a document holding threads ([BINDINGS.md](BINDINGS.md#the-write-surface-object-placement-over-one-primitive)).
- **A write states what its writer read.** A revise diffs against the value stored now, so a person's edit made between a writer's read and its write reads as text the writer deleted, and the anchors on it drop. A host whose writer reads, thinks and then writes (an LLM) passes the read: `baseHash` refuses the write once the value has changed (`edit::stale_base`), and `baseMarkdown`, the annotated markdown the writer read, merges its edit onto the change by line, so both land and the anchors on the person's text stay ([BINDINGS.md](BINDINGS.md#the-write-surface-object-placement-over-one-primitive)).
- **A kept tag pins.** The annotated read spells each prose anchor as a `qm-anchor` tag, and a revise reads it back: a writer that moves a tag moves its anchor's start there, one that drops a tag leaves the anchor to the diff, and a tag naming no anchor the stored value holds drops with a `parse::dropped_construct` (`qm-anchor[ref]`), never minting one ([markdown-spec.md](../references/markdown-spec.md) §6.4).
- **A write never deletes a thread.** Every revise receipt names the anchors it dropped. A dropped anchor is either placed again by `reattach` or reported orphaned, and the thread stays in the host's store either way.

## Reading a selector

`anchor_contexts` answers, for each anchor of a body or content value, in its prose or a cell, its selector: `id`, `path`, `card_id`, `quote`, `prefix` and `suffix`.

- `path` is the `DocPath` of the body or content value holding it.
- `card_id` is the `$id` of the composable card `path` names, and none on the main card ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md#card-id-identity)).
- `quote` is the anchored text as stored: an inline island is its `U+FFFC` slot and a line break its `\n`. A point's quote is empty.
- `prefix` and `suffix` are up to 32 Unicode scalar values of stored text on each side, cut at the value's edges, or a cell's edges for an anchor in one.

A quote spanning an inline island (a footnote reference) includes its slot character, and is found again only where a slot sits.
The read is pure and cheap enough to run before every markdown write.

## The matching rule

`reattach` runs on anchors `revise` already refused to move. `revise` floors a move at four characters and refuses a match in surviving text, so this rule is at least as strict, or it would undo that refusal. Per value, never guessing:

1. The quote is four scalar values or more, or island slots alone. Any other shorter quote orphans.
2. Every occurrence of the quote in the stored text, the prose and each cell read as its own text, counts where its context agrees: the text before ends with the last eight scalar values of `prefix`, and the text after starts with the first eight of `suffix`.
3. A `prefix` or `suffix` shorter than eight was cut at the value's edge, and agrees whole, wherever the edge now sits: a sentence written ahead of an anchor near the value's start leaves it placeable.
4. A point (an empty quote) counts at each position where the prefix and the suffix both agree.
5. Where the caller passes the write's delta for the path, a match must overlap text that write inserted, the discipline `revise` keeps, and only the prose counts, since a delta spans the prose alone. Without one, the context rule alone decides.
6. Exactly one counting place over the prose and every cell: attach there. None or several: orphan. No nearest match, no partial quote, no edit distance.
7. An id already live anywhere in the document, a cell's anchor included, is not re-added. It is answered as reattached with the selector the first value holding it reads.
8. Each id is answered once: of selectors sharing an id, the first is answered and the rest are left out of the receipt.

A range lands exactly over the quote's characters. A point lands at the one position where the prefix meets the suffix, as an empty range.
A write that inserts text at a point leaves the point before it: a point binds to the text on its left, as a caret does.

A selector naming a `card_id` finds its card by that id, wherever the card now sits: the path's card index moves to that card's.
An id no card holds, or a card of another kind than the path names, orphans the selector, since the card it was on is gone.
A selector naming none is path-strict, as is the rest of every path: a path naming no body or content value orphans its selector.

## Drift

`revise` can keep an anchor on different words: it relocates a moved quote to the first occurrence overlapping inserted text, and a word-level rewrite can leave both ends of a range over new text. `reattach` never sees those anchors.
A caller compares each surviving anchor's selector after the write to the one it read before, and a changed quote is the drift it shows. Nothing moves an anchor on its own.

## Table cells and notes

A table cell and a footnote's note are text surfaces of their own, and an anchor in one is a mark in that cell's `marks`. The contract holds there as in the prose:

- `anchor_contexts` reads it, at the value's path, its context cut at the cell's edges, and the annotated read lists it after the anchors before its island, with the cell's text as its `line`. No tag spells it: a cell holds none.
- The ops reach it. `RemoveAnchor` removes it, an anchor `Add` refuses its id, and an island op refuses a cell anchor whose id the value already holds ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md#anchor-id-identity)).
- A revise keeps it where the island continues and the cell at its place keeps its text, and drops it, named in the receipt, where the cell's text changed.
- `reattach` searches every cell with the prose and places it in the one cell its quote counts in.

A stored value can still hold an id twice with a cell taking part, since validation checks the prose alone. It loads, and the load reports `parse::anchor_id_collision` at the value's path ([ERROR.md](ERROR.md)), which the host resolves by removing or re-minting the copies.

A comment on a table as a whole anchors the table's slot: a one-character range over its `U+FFFC`, a prose mark that the receipts list and the rebase rule moves with the table.
Its quote is the slot alone, so `reattach` places it on its context like a point, and only where one slot sits between the agreeing sides.

## Hosting

A host holding threads persists the `StoredDocument` or keeps the live document and revises it.
It never reopens from markdown: `toMarkdown` drops every anchor, and `equals` counts anchors, so an echo guarded by `equals` never matches once a document holds one.
A host that stores markdown recovers by running `reattach` over every stored selector after the open, under the same rule. Recovery is a search that can orphan, so it is the fallback, not the design.

## Links

- Per-field rule and constants: the `anchor` module of `quillmark-content` (`Content::anchor_sites`, `anchor_collisions`, `anchor_contexts`, `locate_site`, `Content::reattach`).
- Document walk and the path writer: `Document::anchor_contexts`, `anchor_context`, `reattach`, `anchor_collisions`.
- Receipts: `Revised.dropped_anchors` and `DocumentRevised.dropped_anchors`.
