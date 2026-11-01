# The Content Model

> **Implementation**: `crates/content/`
> **Related**: [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) (how a value is stored), [COMMENTS.md](COMMENTS.md) (what anchors serve), [BINDINGS.md](BINDINGS.md) (the write surface)

## TL;DR

One rich-text value per card body or `richtext` field: flat text, explicit blocks and containers, range marks, comment anchors, and islands that hold rich text of their own. Every structural part has an id and nothing derived is stored. Every edit is a `Change` that applies, inverts and composes, and a value two concurrent edits merged into an invalid state repairs deterministically.

## The value

A short field as it is stored. A `\n` opens a block, `U+2028` is a hard break inside a block, and `U+FFFC` is the slot an island occupies.

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

- **Blocks** are one record per `\n`-separated segment, in order. A record names its kind, its attributes and its container path, outermost first. Each `\n` carries the record of the block it opens, and `blocks[0]`, the first block's, rides no character.
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

Each axis is a closed set, and a name outside it is `ParseError::UnknownName` at every decoder, so a value holding one does not open. Block kinds, mark types and island types also hold the reserved `x-` names: `x-` and at least one more character, an id as a whole. One decodes into the axis's `Ext` member, `attrs` or `props` held as written, so a value a newer build wrote opens and re-encodes to the same bytes ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies"). Container kinds hold none, since a reader cannot check a path through a nesting rule it does not know.

- Validation of an `x-` member is structural only: the name, the ranges, and its bag's JSON depth. An `x-` island holds any slots, each of paragraphs.
- The line form lowers an `x-` block to a paragraph, drops an `x-` mark, and keeps an `x-` island with its slots as cells. The renderers draw the island as nothing, and markdown writes it as nothing, reporting the loss; the annotated read spells it as its token, so a revise keeps it.
- A revise keeps a kept block's `x-` kind where the markdown reads back as a paragraph, and an `x-` mark over the text the diff keeps.
- A revise or replace whose change removes an `x-` island refuses with `edit::drops_ext_island`, since its writer may never have seen it; a change through `apply` removes one.
- Steps set and remove `x-` names like any other.

## Validity

`Content::validate` is what every stored value holds to, and what every change leaves. It refuses:

- a `\r`, a bidi control, or a line separator other than `U+2028` in the text;
- a block count other than the segment count, or an island count other than the slot count;
- an id that is empty, longer than 128 scalar values, or holds whitespace, a control or a slot, and an id held twice: blocks, containers and islands each in their scope, anchors once over the value and every slot;
- a path naming a missing container, a path whose kinds do not nest (an item only right after a list, a list only right before an item), or one nesting past `MAX_NESTING_DEPTH` (12);
- a container whose blocks are not one contiguous run, or that sits under two different parent paths;
- a heading holding a hard break, a rule holding text, a code block holding an island, and a block-only island (a table) sharing its block with anything else;
- a mark or anchor past the text, an empty mark, and a slot holding anything but paragraphs in no container.

`Content::normalize` gives the one canonical form: marks cut at each `\n` and `U+2028`, unioned and sorted; anchors sorted; containers in the order paths first name them, an unused one dropped; island props key-sorted and settled. A value's wire form (the `wire` module) writes every object's keys in ascending order and omits a key whose value is empty, so equal values serialize to equal bytes.

## Ids

Blocks, containers, islands, slots, table rows and table columns carry structural ids. Import, migration and a revise mint them deterministically, `{prefix}{n}` past the highest number a value already holds under that prefix (`b`, `l`, `i`, `q`, `isl-`, `r`, `k`, `s`), so the same markdown always imports to the same bytes. A revise or replace from a stale base mints past the value it lands on as well (§ "Writing a value"). A live client mints its own. Anchor ids are the host's: no content determines what a comment refers to, so Quillmark mints none ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "The id handles").

## Changes

A `Change` is a list of steps. Each step reads the value the step before it left, and `apply` runs all of them or none.

| Step | Shape | What it does |
|---|---|---|
| `splice` | `{at, delete, insert, marks?, blocks?, islands?}` | Replaces text. `delete` is the removed text itself, and the step fails if the value holds anything else there. `marks` is the formatting the inserted text carries; omitted, it carries none. Each `\n` in `insert` brings the record of the block it opens, and each `U+FFFC` its island. The removed records, marks and islands travel with the step as `removed`, for inversion. |
| `setBlock` | `{id, kind?, attrs?, path?}` | Changes a block's kind, attributes or path. Indenting and outdenting are path changes. |
| `container` | `{id, kind, attrs?}` or `{id, remove: true}` | Adds a container or sets an existing one's attributes, or removes one no path names. |
| `mark` | `{start, end, type, attrs?, add, quote?}` | Adds or removes formatting over a range. |
| `anchor` | `{id, start, end, quote?}` or `{id, remove: true}` | Places, moves or removes a comment anchor. |
| `island` | `{id, props, slots?}` or `{at, island, block?}` | Replaces an island's props, or inserts an island with its slot. A slot the new props name and the island lacks takes its content from `slots`, which names no other slot; a slot they no longer name leaves. Deleting the slot in a splice removes the island. |
| `nested` | `{island, slot, change}` | Applies a change inside one slot: a table cell or a footnote. |

`apply` runs the steps in one pass. Each step holds what it touched to what `validate` asks of it, its inserted text, records, marks and islands, and the value the last step leaves is validated once, whole. Block and island ids are unique only there: between steps a change may hold one twice, as a move does when it inserts the copy before it deletes the original, and a step that names an id held twice is `ApplyError::BadStep`, since it cannot say which it means.

Each step holds to what its writer read as far as its precondition reaches. A step whose precondition fails is `ApplyError::Stale`:

- a splice holds its `delete` to the text at `at`;
- a `mark` step and an `anchor` placement hold their range to `quote`, the text it covered when the writer read it;
- an island insert holds `at` to `block`, the id of the block the writer read it in;
- the id-addressed steps fail on a missing id.

`quote` and `block` are optional on input. The normalized change, `Applied.applied`, fills both in, as it fills a splice's `removed`, so a change rebased over another holds every step to what its writer read. `invert` fills them in its own steps, from the value each undoes: each step answers its inverse as it applies, and `invert` is those inverses read back last first.

With no base, a step is refused rather than landing on whatever now sits at its offsets only where its precondition reads the value: a splice, a `mark` step or anchor placement that carries `quote`, and an island insert that names its `block`. A step by id, a `setBlock`, a `container` step or an island props step, lands over whatever the value now holds, so a stale props step that adds a column deletes a row added since.

A writer outside the live session therefore passes the base. A stale write then rebases, and conflicts wherever a step by id would overwrite what changed since (§ "Rebasing a change").

A split inserts a `\n` whose record names the right half; the left half keeps its id, since the record that opens it does not move. A join deletes the right block's `\n` and record, and the joined block keeps the left's id and kind. Splits and joins therefore never rewrite a record a concurrent `setBlock` could be aimed at. Collab's Y form keeps the same boundary, so a split or a join lowers into Y as an insertion or a deletion alone.

Enter at the start of a block keeps the text on its id when the producer inserts the `\n` ahead of the one that opens the block, so the new record names the empty block before it. No `\n` carries the first block's record, so no splice reaches its id: Enter at the start of the first block moves its text to the new record, and `diff` and `replace` keep the stored first id. `gestures::split_block` and `join_blocks` build these changes, the one implementation every editor binding lowers Enter and Backspace through.

`apply` never grows a mark: a splice states the marks its inserted text carries, so every inverse is exact. The growth defaults are for producers, and `gestures::inherited_marks(content, at)` is their one implementation: `strong`, `emph`, `underline` and `strike` grow at their end, `code`, `link` and an `x-` mark at neither edge, and `font_family` at both, the family before `at` winning over the one after. A `\n` and a hard break hold no mark, so at a block's or a line's start only a family carries over. Anchors never grow: a range keeps its edges, and a point stays left of text typed at it.

`mapping` carries a position across a change, after ProseMirror's maps. A `StepMap` reads off one step what it does to positions, and `map_pos` answers a `MapResult`: where the position lands, and whether the characters before it, after it, on its `assoc` side, or across it were deleted. A position always lands:

- one inside deleted text on the deletion's `assoc` edge;
- one inside an island a splice deleted on the island's slot in the text that held it;
- one in a slot a props step removed at its own address, since the step does not say where its island sits.

A splice says which islands it deleted only in `removed`. A change mapped holds it on each splice that deletes a slot, as the normalized change does, and one without it is refused. A `Mapping` is a list of step maps with mirror links, each pairing a map with its inverse, so a position one map deleted and its mirror put back maps to where it was. A rebase maps a step through the inverse of its change's earlier steps, then the change it rebases over, then those steps rebased. `Mapping::map_point` maps a position as `apply` moves a point anchor, and `project` takes a mapping's maps inside one slot.

Text deleted under a range anchor shrinks it to what survives. An anchor whose text is deleted entirely is removed and named in the receipt's `droppedAnchors`; a surviving anchor whose text a write rewrote is named in `movedAnchors`. An island removed by a splice, or a slot a props step no longer names, takes the anchors of its slots with it, and each is named in `droppedAnchors` with the island and slot that held it. Every anchor a change takes out of the value is named there, except one its own `anchor` step removed, which the writer asked for.

## Rebasing a change

`rebase(base, change, over)` moves `change` past `over`, both made from `base`, after ProseMirror's `rebaseSteps` (`src/rebase.rs`). It takes the value because each step re-checks itself on the value it now reads, and because a change's maps read off its normalized form.

Each step maps through the inverse of its change's earlier steps, then `over`, then those steps rebased, each linked to its inverse as its mirror. It then applies to what `over` and the rebased steps before it left. The answer is the writer's own steps at their mapped offsets, with the quotes and blocks it gave.

| Step | Maps | Conflicts where |
|---|---|---|
| `splice` | the edges of its deletion inward; an insert as a point anchor | `over` deleted any of its deletion, or the text there no longer reads `delete`; an insert where `over` deleted the text on both sides; `over` changed an island it deletes, or the record of a block it deletes and writes back |
| `mark`, range `anchor` | its edges inward, as `apply` moves a range anchor | `over` deleted the whole range, or the range no longer reads its `quote` |
| point `anchor`, island insert | as `apply` moves a point anchor | an insert where `over` deleted the text on both sides, or `block` no longer holds `at` |
| `setBlock`, `container`, island props | unchanged | the id is gone, or `over` changed what the step overwrites |
| anchor removal | unchanged | the id is gone |
| `nested` | its change rebased over what `over` did in that slot | the slot is gone, a step inside conflicts, or `over` removed the island or the slot and the slot now reads otherwise |

A step by id holds to what it overwrites as its writer read it: the fields of the block's record a `setBlock` names, the container a `container` step sets or removes, and an island's props with the slots a props step drops. Where `over` changed any of them, the step conflicts as `changed` rather than undoing `over`, so a `setBlock` names only the fields it changes, as a diff's do. A splice that deletes an island `over` changed conflicts the same way; one that deletes a block `over` re-kinded deletes it, as a deletion in ProseMirror does. Where `over` removed an island or slot, the edits in it are not in the mapping, so a `nested` step lands only where the slot reads as it did.

A range `over` deleted entirely conflicts rather than dropping its step: a write is whole or refused, and a step with nothing left to land on is still part of what its writer asked for. A `Conflict` names the step, a `nested` step's index for one inside it, and its `Reason`: `deleted`, `changed`, `missing`, or `refused` where the mapped step fails otherwise or the change leaves an invalid value.

## The laws

Each law is a property test over generated values and generated changes (`tests/change_laws.rs`, `tests/repair_laws.rs`).

1. `apply(apply(d, c), invert(c, d)) == d`. Every change undoes exactly, deleted islands, anchors and block records included.
2. `apply(d, compose(a, b)) == apply(apply(d, a), b)`. Compose concatenates the steps and merges adjacent splices that touch.
3. `mapPos(c, p)` agrees with `apply`: the two edges of a character both say it was deleted, or land around it afterwards in the same order, a character inside a nested slot included. A change's maps followed by their mirrored inverses take every position home.
4. `parse(serialize(d)) == d`, and equal values serialize to equal bytes.
5. Every `Change` yields a valid value or fails and changes nothing.
6. `repair` is total and deterministic: any merged state repairs to a valid value, the same state always to the same value, and a valid value to itself with nothing reported. A write-back reads as the merged state it repaired: by any number of peers where no repair inserts, since the value then holds no character or slot the state does not, and by one writer otherwise.
7. Where `rebase(d, c, o)` succeeds, `apply(apply(d, o), rebase(d, c, o))` succeeds and leaves a valid value, which the rebased change's inverse takes back exactly, and which keeps what `o` did by id: each block kind and path, container, and island's props `o` changed, and each island or slot it changed. `rebase(d, c, ∅) == c`. Two changes to text either side of a character neither touches, joining no blocks, rebase over each other and reach the same value either way round, holding the text each wrote.

## Writing a value

One write primitive and two producers over it, on a card's body or richtext field (`crates/core/src/document/write.rs`):

| Write | Builds the change from | Anchors |
|---|---|---|
| `apply(target, change, base)` | the caller's steps, an editor's transaction lowered | move exactly as the steps move text |
| `revise(target, markdown, base)` | a writer's markdown: the `qm-*` tags of `annotated(target)` it keeps, then a word-aligned diff | kept by the tags kept, otherwise by the diff; drift reported |
| `replace(target, content, base)` | a whole value, through the same diff | each anchor the value does not name carried where the diff keeps its text; the rest reported |

A write names its card by `$id` (`CardRef::Id`, or the main card), never by index: a card moved or inserted between a writer's read and its write shifts an index onto another card, and where that card's value is equal to the one read, the base cannot tell. `Document::apply_all` runs several writes, a base each, all or none, and resolves every card id before it applies any; an id no card holds refuses with `edit::unknown_card_id`.

Each write answers a `Receipt`: `applied`, the change in its normalized form, which `invert(applied, before)` undoes; `hash`, the value's new hash and the next write's base; `droppedAnchors`; `movedAnchors`; a revise's import warnings; `whole_text`; and `rebased`, true where the write rebased over a change since its base.

**Pass the base.** `base` is the hash the writer read (`hash(addr)`). Without one, a write lands over whatever the value now holds, so an edit made between the writer's read and its write reads as text the writer deleted. A value that no longer hashes to the base rebases the write over what changed since:

- Each card keeps a `ChangeLog` per value it holds, in memory only: its recent writes, each with the hash it read, the hash it left, the value it wrote over and its normalized change. No hash, comparison or stored form sees it.
- A base the log holds composes the changes from it to now, and the write rebases over them and applies. A revise or replace diffs from the value the base names, so an edit made since is not read as text the writer deleted. Each block, container and island id the diff introduces that the value now holds is minted afresh past the ids of both (`diff::revise_holding`, `diff::replace_holding`), a replace's own id among them, so two stale writes that each add a paragraph both land; `rebase` itself re-mints nothing.
- A write that conflicts refuses with `edit::rebase_conflict`, carrying `current`, `step` and `reason`.
- A base the log does not reach refuses with `edit::stale_base`, carrying `current`, and the writer reads again.
- The log holds at most `LOG_WRITES` (32) writes and `LOG_STEPS` (4096) steps per value, the newest write whatever its size, the oldest leaving first. A write it did not see, such as a typed writer's revise, breaks the chain, and the log forgets what came before it.

`diff(before, after)` is the producer for a writer with no steps of its own. It diffs the text by line, pairs rewritten lines by word similarity, and diffs a paired line by word, so a rewritten word changes as a whole and a range over it drops rather than riding its scattered letters. An inserted or deleted block brings the `\n` that opens it, so its neighbours keep their records. Block records, containers, marks, anchors and islands follow as their own steps; an anchor `after` holds in another place, the prose or a cell, moves there. A block or island `after` holds in another place keeps its id, moved up or down.

- It refuses an `after` that is not a valid value.
- Where no step-wise change reaches `after`, it rewrites the value or slot as one splice over its whole text, and says so in `Diff::whole_text`. A revise's or replace's receipt carries the flag, and the bindings raise it as an `edit::whole_text_diff` warning.

## Merge repair

A single step cannot break the model, but two valid concurrent edits can merge into a state no step allows. `repair(raw) -> (Content, Repair[])` makes any such state valid (`src/repair.rs`). Repair is how a merged state reads: it is total and deterministic, so peers holding one merged state compute one value without writing anything. Writing a repair back into the shared state is optional, and who may write it back depends on what it writes:

- A repair whose rules only delete or set an attribute may be written back by any peer. Two peers writing back one repair delete the same characters and records and set the same values, so their merge is one write-back, and it reads as the merged state did. A record minted for a `\n` that holds none, a split run's container and a re-minted id are sets too, each under an id derived from the ids involved.
- A rule that inserts, `Repair::inserts` (`inserts: true` on the wire), is written back only by the session's single writer, a server or an elected leader: a split block's `\n` and record, the space a heading's hard break becomes, and a missing cell's empty slot. Two peers inserting at once each insert their own: text lands twice and stays, since the extra copy is valid, and a cell minted twice keeps one peer's slot under its id. Until the single writer writes it back, every peer reads it through repair, and any other peer writes back a report only when none of its repairs inserts.

| Merged state | How it arises | Repair |
|---|---|---|
| A path names a missing container | One peer removes a list while another adds an item to it | The id leaves the path |
| One container under two paths | One peer indents a two-block item while another adds a block to it | Every block of the container takes the path prefix of its first block |
| One container over two runs | A block lands between items of one list, in no container | The later run gets a fresh container, its id derived from the original id and the run's first block id |
| A slot names no island record, or a record no slot names | Concurrent deletion and edit of a table | The slot's character is removed; the orphan record is removed |
| Two slots name one island | A concurrent copy of a slot | The first keeps it; the second holds a copy under a derived id |
| Text beside a block island | Two peers type on either side of a table's slot | The block splits at the island's edges; the first piece keeps the record, as a split's left half does, and each later piece takes a fresh id |
| A range anchor at zero width | Its text was deleted while another peer moved it | Dropped and reported |
| A table cell naming no slot of its own | A concurrent row insert and column insert | The cell gets an empty slot, its id the row and column ids joined |

The raw value is shaped as `Content` with two additions only repair reads. Its `islands` may be an object keyed by island id, with `slot_ids` naming the island each `U+FFFC` carries in slot order, `null` where it carries none; an `islands` array pairs with the slots in order. And an anchor placed as a range carries `range: true`, since a range collapsed to zero width cannot otherwise be told from a point.

Beyond the table, repair is total over any value shaped as content: it removes forbidden characters, mints a missing block record and drops an extra one, re-mints a bad or duplicate id, drops a slot under a bad key, mends a table grid a concurrent row and column edit broke, clips marks and anchors to the text, and turns a rule holding text or a code block holding an island into a paragraph. A fresh id is the ids involved joined by `-`, with a numeric suffix where that id is taken, so two peers writing back one repair write the same bytes.

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
