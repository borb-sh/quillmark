# The Content Model

> **Implementation**: `crates/content/`
> **Related**: [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) (how a value is stored), [COMMENTS.md](COMMENTS.md) (what anchors serve), [BINDINGS.md](BINDINGS.md) (the write surface)

## TL;DR

One rich-text value per card body or `richtext` field: flat text, explicit blocks and containers, range marks, comment anchors, and islands that hold rich text of their own. Every structural part has an id and nothing derived is stored. Every edit is a `Change` that applies, inverts and composes, and a value two concurrent edits merged into an invalid state repairs deterministically.

## The value

A short field as it is stored. A `\n` ends a block, `U+2028` is a hard break inside a block, and `U+FFFC` is the slot an island occupies.

```json
{
  "text": "Quarterly memo\nRevenue grew 12%.\nShip v2\nDraft spec\n￼",
  "blocks": [
    { "id": "b0", "kind": "heading", "attrs": { "level": 1 } },
    { "id": "b1", "kind": "para" },
    { "id": "b2", "kind": "para", "path": ["l0", "i0"] },
    { "id": "b3", "kind": "para", "path": ["l0", "i1"] },
    { "id": "b4", "kind": "para" }
  ],
  "containers": [
    { "id": "l0", "kind": "list", "attrs": { "ordered": false, "start": 1 } },
    { "id": "i0", "kind": "item" },
    { "id": "i1", "kind": "item", "attrs": { "checked": true } }
  ],
  "marks": [{ "start": 28, "end": 31, "type": "strong" }],
  "anchors": [{ "id": "c-7f3a", "start": 23, "end": 27 }],
  "islands": [{ "id": "t0", "type": "table", "props": { "columns": ["k0"], "rows": [{ "id": "r0", "cells": { "k0": "s0" } }] }, "slots": { "s0": { "text": "Q", "blocks": [{ "id": "b0", "kind": "para" }] } } }]
}
```

- **Blocks** are one record per `\n`-separated segment, in order. A record names its kind, its attributes and its container path, outermost first.
- **Containers** are lists, list items and quotes. Nesting lives only in the blocks' paths, so a container can never dangle from a missing parent or sit in a cycle. A list holds `ordered` and `start` and an item holds `checked`; an item's number is computed when read.
- **Marks** are formatting over `[start, end)`. Same-kind marks union. `link` and `font_family` are keyed: a character holds at most one url and one family.
- **Anchors** are comment handles with a host-minted id, kept apart from marks. A point is an anchor with `start == end`.
- **Islands** are tables, images, footnotes and equations, one per slot in slot order. A table's cells and a footnote's note are slots, each a nested `Content` of paragraphs. Rows, columns and slots carry ids, so an anchor in a cell stays in its slot wherever the row moves.

Offsets are Unicode scalar values in storage. Each binding speaks its host's units at its boundary and converts inside ([BINDINGS.md](BINDINGS.md) § "Content offsets").

| Axis | Members |
|---|---|
| Block `kind` | `para`, `heading` (`level` 1 to 6), `code` (optional `lang`), `rule` |
| Container `kind` | `list` (`ordered`, `start`), `item` (optional `checked`), `quote` |
| Mark `type` | `strong`, `emph`, `underline`, `strike`, `code`, `link` (`url`), `font_family` (`name`) |
| Island `type` | `table`, `image` (`url`, `alt`), `footnote` (slot `note`), `math` (`tex`, optional `display`) |

Each axis is a closed set: a name outside it is `ParseError::UnknownName` at every decoder, so a value holding one does not open ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies").

## Validity

`Content::validate` is what every stored value holds to, and what every step keeps. It refuses:

- a `\r`, a bidi control, or a line separator other than `U+2028` in the text;
- a block count other than the segment count, or an island count other than the slot count;
- an id that is empty, longer than 128 scalar values, or holds whitespace, a control or a slot, and an id held twice: blocks, containers and islands each in their scope, anchors once over the value and every slot;
- a path naming a missing container, a path whose kinds do not nest (an item only right after a list, a list only right before an item), or one nesting past `MAX_NESTING_DEPTH` (12);
- a container whose blocks are not one contiguous run, or that sits under two different parent paths;
- a heading holding a hard break, a rule holding text, a code block holding an island, and a block-only island (a table) sharing its block with anything else;
- a mark or anchor past the text, an empty mark, and a slot holding anything but paragraphs in no container.

`Content::normalize` gives the one canonical form: marks cut at each `\n` and `U+2028`, unioned and sorted; anchors sorted; containers in the order paths first name them, an unused one dropped; island props key-sorted and settled. A value's wire form (the `wire` module) writes every object's keys in ascending order and omits a key whose value is empty, so equal values serialize to equal bytes.

## Ids

Blocks, containers, islands, slots, table rows and table columns carry structural ids. Import, migration and a revise mint them deterministically, `{prefix}{n}` past the highest number a value already holds under that prefix (`b`, `l`, `i`, `q`, `isl-`, `r`, `k`, `s`), so the same markdown always imports to the same bytes. A live client mints its own. Anchor ids are the host's: no content determines what a comment refers to, so Quillmark mints none ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "The id handles").

## Changes

A `Change` is a list of steps. Each step reads the value the step before it left, and `apply` runs all of them or none.

| Step | Shape | What it does |
|---|---|---|
| `splice` | `{at, delete, insert, marks?, blocks?, islands?}` | Replaces text. `delete` is the removed text itself, and the step fails if the value holds anything else there. `marks` is the formatting the inserted text carries; omitted, it carries none. Each `\n` in `insert` brings the record of the segment it ends, and each `U+FFFC` its island. The removed records, marks and islands travel with the step as `removed`, for inversion. |
| `setBlock` | `{id, kind?, attrs?, path?}` | Changes a block's kind, attributes or path. Indenting and outdenting are path changes. |
| `container` | `{id, kind, attrs?}` or `{id, remove: true}` | Adds a container or sets an existing one's attributes, or removes one no path names. |
| `mark` | `{start, end, type, attrs?, add}` | Adds or removes formatting over a range. |
| `anchor` | `{id, start, end}` or `{id, remove: true}` | Places, moves or removes a comment anchor. |
| `island` | `{id, props}` or `{at, island}` | Replaces an island's props, or inserts an island with its slot. Deleting the slot in a splice removes the island. |
| `nested` | `{island, slot, change}` | Applies a change inside one slot: a table cell or a footnote. |

A split inserts a `\n` whose record names the left half; the right half keeps the original id, because the original `\n` still ends it. A join deletes a `\n` and its record, and the joined block keeps the right segment's record. Splits and joins therefore never rewrite a record a concurrent `setBlock` could be aimed at. No `\n` carries the last block's record, so no splice reaches its id: `replace` keeps the stored one there.

`apply` never grows a mark: a splice states the marks its inserted text carries, so every inverse is exact. The editor defaults are for producers: `strong`, `emph`, `underline` and `strike` grow at their end, `code` and `link` at neither edge, `font_family` at both. Anchors never grow: a range keeps its edges, and a point stays left of text typed at it.

Text deleted under a range anchor shrinks it to what survives. An anchor whose text is deleted entirely is removed and named in the receipt's `droppedAnchors`; a surviving anchor whose text a write rewrote is named in `movedAnchors`.

## The laws

Each law is a property test over generated values and generated changes (`tests/change_laws.rs`, `tests/repair_laws.rs`).

1. `apply(apply(d, c), invert(c, d)) == d`. Every change undoes exactly, deleted islands, anchors and block records included.
2. `apply(d, compose(a, b)) == apply(apply(d, a), b)`. Compose concatenates the steps and merges adjacent splices that touch.
3. `mapPos(c, p)` agrees with `apply`: a position in untouched text lands on the same character afterwards, a position inside a nested slot included.
4. `parse(serialize(d)) == d`, and equal values serialize to equal bytes.
5. Every `Change` yields a valid value or fails and changes nothing.
6. `repair` is total and deterministic: any merged state repairs to a valid value, the same state always to the same value, and a valid value to itself with nothing reported.

## Writing a value

One write primitive and two producers over it, on a card's body or richtext field (`crates/core/src/document/write.rs`):

| Write | Builds the change from | Anchors |
|---|---|---|
| `apply(target, change, base)` | the caller's steps, an editor's transaction lowered | move exactly as the steps move text |
| `revise(target, markdown, base)` | a writer's markdown: the `qm-*` tags of `annotated(target)` it keeps, then a word-aligned diff | kept by the tags kept, otherwise by the diff; drift reported |
| `replace(target, content, base)` | a whole value, through the same diff | each anchor the value does not name carried where the diff keeps its text; the rest reported |

`Document::apply_all` runs several writes, a base each, all or none. Each write answers a `Receipt`: `applied`, the change in its normalized form, which `invert(applied, before)` undoes; `hash`, the value's new hash and the next write's base; `droppedAnchors`; `movedAnchors`; and a revise's import warnings.

**Pass the base.** `base` is the hash the writer read (`hash(addr)`), and a value that no longer hashes to it refuses the write with `edit::stale_base`, carrying `current`. Without one, a write lands over whatever the value now holds, so an edit made between the writer's read and its write reads as text the writer deleted. A stale writer reads again and rewrites, which for a script is one more `revise`.

`diff(before, after)` is the producer for a writer with no steps of its own. It diffs the text by line, pairs rewritten lines by word similarity, and diffs a paired line by word, so a rewritten word changes as a whole and a range over it drops rather than riding its scattered letters. Block records, containers, marks, anchors and islands follow as their own steps.

## Merge repair

A single step cannot break the model, but two valid concurrent edits can merge into a state no step allows. `repair(raw) -> (Content, Repair[])` makes any such state valid (`src/repair.rs`). A peer that sees a repair writes it back into the shared state, so peers converge on the repaired value instead of each repairing on read.

| Merged state | How it arises | Repair |
|---|---|---|
| A path names a missing container | One peer removes a list while another adds an item to it | The id leaves the path |
| One container under two paths | One peer indents a two-block item while another adds a block to it | Every block of the container takes the path prefix of its first block |
| One container over two runs | A block lands between items of one list, in no container | The later run gets a fresh container, its id derived from the original id and the run's first block id |
| A slot names no island record, or a record no slot names | Concurrent deletion and edit of a table | The slot's character is removed; the orphan record is removed |
| Two slots name one island | A concurrent copy of a slot | The first keeps it; the second holds a copy under a derived id |
| Text beside a block island | Two peers type on either side of a table's slot | The text splits into blocks of its own, each with a fresh id; the island keeps the record |
| A range anchor at zero width | Its text was deleted while another peer moved it | Dropped and reported |

The raw value is shaped as `Content` with two additions only repair reads. Its `islands` may be an object keyed by island id, with `slot_ids` naming the island each `U+FFFC` carries in slot order, `null` where it carries none; an `islands` array pairs with the slots in order. And an anchor placed as a range carries `range: true`, since a range collapsed to zero width cannot otherwise be told from a point.

Beyond the table, repair is total over any value shaped as content: it removes forbidden characters, mints a missing block record and drops an extra one, re-mints a bad or duplicate id, mends a table grid a concurrent row and column edit broke (a missing cell gets an empty slot), clips marks and anchors to the text, and turns a rule holding text or a code block holding an island into a paragraph. A fresh id is the ids involved joined by `-`, with a numeric suffix where that id is taken, so two peers repairing one state at once write the same bytes.

## The line form

The renderers and the markdown codecs read the line form under the `lines` module, one line per block with marks and islands inline. `migrate::to_lines` lowers a value to it and `migrate::from_lines` lifts a v1 value, minting ids deterministically; a markdown import runs through the line form and lifts ([CONVERT.md](CONVERT.md), [PARITY.md](PARITY.md)).

| v1 | The model |
|---|---|
| `lines[]` with a container path | `blocks[]` with a path of container ids; the records become `containers[]` |
| `continues: true` lines | One block, its lines joined by hard breaks |
| A code block's `lang` on every line | Once, on the block |
| `ordinal`, `instance` | Gone: numbers are computed, and runs are told apart by container id |
| `anchor` marks | `anchors[]` |
| Table cells as `{text, marks}` in `props` | Slots with ids; a cell's anchors move into its slot |
| A footnote's note in `props` | The `note` slot |
