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
- A content write's revise or replace (`Card::revise`, `Card::replace`) whose change removes an `x-` island refuses with `edit::drops_ext_island`, since its writer may never have seen it; a change through `apply` removes one. The line-form revise verbs (`revise_body`, `revise_field`, `Document::revise`) do not check, and a plain markdown revise through them drops the island.
- Steps set and remove `x-` names like any other.

## Validity

`Content::validate` is what every stored value holds to, and what every change leaves. It refuses:

- a `\r`, a bidi control, or a line separator other than `U+2028` in the text;
- a block count other than the segment count, or an island count other than the slot count;
- a block, container or island id that is empty, longer than 128 scalar values, or holds whitespace, a control or a slot, an anchor id that is empty (the rest of an anchor id is the host's), and an id held twice: blocks, containers and islands each in their scope, anchors once over the value and every slot;
- a path naming a missing container, a path whose kinds do not nest (an item only right after a list, a list only right before an item), or one nesting past `MAX_NESTING_DEPTH` (12);
- a container whose blocks are not one contiguous run, that sits under two different parent paths, or that no block's path names;
- a heading holding a hard break, a rule holding text, a code block holding an island, and a block-only island (a table) sharing its block with anything else;
- an island whose props do not read as its type's (a table whose grid names a row, column or cell no slot backs, or a slot no cell names), and a table of more than `MAX_TABLE_CELLS` cells;
- a mark or anchor past the text, an empty mark, and a slot holding anything but paragraphs in no container;
- more marks on one text, a field's or a slot's, than `max_marks` of its length: `MARKS_BASE` (64) and `MARKS_PER_CHAR` (4) per character, and more than `MAX_ANCHORS` (100 000) anchors over the value and its slots.

Normalizing spends the mark bound before validating can refuse: a mark crosses as many blocks as it spans, and `x-` marks of distinct attributes never union, so marks times blocks of them come out of a value that names each once. The decoder (`Content::from_value`) therefore checks the bound first, counting each mark once per `\n` and `U+2028` it spans plus one, the most it can be cut into (`Content::check_bounds`), and so does a change: a step whose marks would pass the bound refuses before it settles them, as `Invariant::TooManyMarks`. Repair keeps the marks in order up to the bound and the anchors in reading order up to theirs, and reports the rest.

`Content::normalize` gives the one canonical form: marks cut at each `\n` and `U+2028`, unioned and sorted; anchors sorted; containers in the order paths first name them, an unused one dropped; island props key-sorted and settled. A value's wire form (the `wire` module) writes every object's keys in ascending order and omits a key whose value is empty, so equal values serialize to equal bytes.

## Ids

Blocks, containers, islands, slots, table rows and table columns carry structural ids. Import, migration and a revise mint them deterministically, `{prefix}{n}` past the highest number a value already holds under that prefix (`b`, `l`, `i`, `q`, `isl-`, `r`, `k`, `s`), so the same markdown always imports to the same bytes. A revise or replace re-mints an id it introduces that the value it lands on holds (§ "Writing a value").

A live client mints its own by a rule of its own; a write re-mints one it introduces that the value holds, and a rebase renames one two writers both introduced (§ "Rebasing a change"). Anchor ids are the host's: no content determines what a comment refers to, so Quillmark mints none ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "The id handles").

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

A step whose object holds a key its kind does not define, a splice's `delet` or a mark's `qoute`, is `ParseError::UnknownKey` at every decoder, so a misspelled precondition never decodes as a step without it. The `attrs` and `props` a step carries stay open.

`quote` and `block` are optional on input. The normalized change, `Applied.applied`, fills both in, as it fills a splice's `removed`, so a change rebased over another holds every step to what its writer read. `invert` fills them in its own steps, and each splice's `removed`, from the value each undoes: each step answers its inverse as it applies, and `invert` is those inverses read back last first.

With no base, a step is refused rather than landing on whatever now sits at its offsets only where its precondition reads the value: a splice that deletes text, a `mark` step or anchor placement that carries `quote`, and an island insert that names its `block`. A step by id, a `setBlock`, a `container` step or an island props step, lands over whatever the value now holds, so a stale props step that adds a column deletes a row added since.

A writer outside the live session therefore passes the base. A stale write then rebases, and conflicts wherever a step by id would overwrite what changed since (§ "Rebasing a change").

A split inserts a `\n` whose record names the right half; the left half keeps its id, since the record that opens it does not move. A join deletes the right block's `\n` and record, and the joined block keeps the left's id and kind. Splits and joins therefore never rewrite a record a concurrent `setBlock` could be aimed at. Collab's Y form keeps the same boundary, so a split or a join lowers into Y as an insertion or a deletion alone.

Enter at the start of a block keeps the text on its id when the producer inserts the `\n` ahead of the one that opens the block, so the new record names the empty block before it. No `\n` carries the first block's record, so no splice reaches its id: Enter at the start of the first block moves its text to the new record, and `diff` and `replace` keep the stored first id. `gestures::split_block` and `join_blocks` build these changes; WASM binds them for an editor, and the one editor today lowers each transaction by a whole-field `diff` instead ([BINDINGS.md](BINDINGS.md) § "The write surface: object placement over one primitive").

`apply` never grows a mark: a splice states the marks its inserted text carries, so every inverse is exact. The growth defaults are for producers, and `gestures::inherited_marks(content, at)` states them, for a producer that asks: `strong`, `emph`, `underline` and `strike` grow at their end, `code`, `link` and an `x-` mark at neither edge, and `font_family` at both, the family before `at` winning over the one after. A `\n` and a hard break hold no mark, so at a block's or a line's start only a family carries over. Anchors never grow: a range keeps its edges, and a point stays left of text typed at it.

The structural gestures build their changes the same way, so an editor that lowers through them builds no container path; the one editor today builds its own. A container a gesture opens takes an id no container holds.

- `delete_range` deletes a range. Where it runs from a block's start into a later block, the block that keeps the text takes the kind, attributes and path of the block the range ends in, as ProseMirror's delete does, and keeps the first block's id.
- `indent` (Tab) nests the items of the innermost list holding a range under the item before them, joining that item's list where it ends in one and else opening a list of their own list's kind.
- `outdent` (Shift-Tab) lifts a range out of its innermost item or quote. A nested item becomes a sibling of its parent, taking the items after it and the rest of its parent as its own; a top-level item leaves its list; a quote's children leave it. What follows the lifted run stays in its container, or opens one of its kind where the run split it.
- `wrap` opens a quote or a list around the children of the range's innermost shared item or quote, a list the range sits in wrapped whole.

Tab then Shift-Tab over the same blocks puts the value back.

`mapping` carries a position across a change, after ProseMirror's maps. A `StepMap` reads off one step what it does to positions, and `map_pos` answers a `MapResult`: where the position lands, and whether the characters before it, after it, on its `assoc` side, or across it were deleted. A position always lands:

- one inside deleted text on the deletion's `assoc` edge;
- one inside an island a splice deleted on the island's slot in the text that held it;
- one in a slot a props step removed at its own address, since the step does not say where its island sits.

An island a change removes and inserts again under its id, a removal paired with the next insert of that id or the other way round and the slots going as they came, as `diff` and `replace` move one, is moved rather than deleted across the two: a position in its slots keeps its address.

A splice says which islands it deleted only in `removed`. A change mapped holds it on each splice that deletes a slot, as the normalized change does, and one without it is refused. A `Mapping` is a list of step maps with mirror links, each pairing a map with its inverse, so a position one map deleted and its mirror put back maps to where it was. A rebase maps a step through the inverse of its change's earlier steps, then the change it rebases over, then those steps rebased. `Mapping::map_point` maps a position as `apply` moves a point anchor, and `project` takes a mapping's maps inside one slot.

Text deleted under a range anchor shrinks it to what survives. An anchor whose text is deleted entirely is removed and named in the receipt's `droppedAnchors`; a surviving anchor whose text a write rewrote is named in `movedAnchors`. An island removed by a splice, or a slot a props step no longer names, takes the anchors of its slots with it, and each is named in `droppedAnchors` with the island and slot that held it. Every anchor a change takes out of the value is named there, except one its own `anchor` step removed, which the writer asked for.

## Rebasing a change

`rebase(base, change, over)` moves `change` past `over`, both made from `base`, after ProseMirror's `rebaseSteps` (`src/rebase.rs`). It takes the value because each step re-checks itself on the value it now reads, and because a change's maps read off its normalized form.

Each step maps through the inverse of its change's earlier steps, then `over`, then those steps rebased, each linked to its inverse as its mirror. It then applies to what `over` and the rebased steps before it left. The answer is the writer's own steps at their mapped offsets, with the quotes and blocks it gave. Rebase is an ordered transform, not a convergent one: whether a write lands, and the value it leaves, can depend on which write reached the log first, but for the pairs law 7 names.

| Step | Maps | Conflicts where |
|---|---|---|
| `splice` | the edges of its deletion inward; an insert as a point anchor | `over` deleted any of its deletion, or the text there no longer reads `delete`; an insert where `over` deleted the text on both sides; `over` changed an island it deletes, or the record of a block it deletes |
| `mark`, range `anchor` | its edges inward, as `apply` moves a range anchor | `over` deleted the whole range, or the range no longer reads its `quote` |
| point `anchor`, island insert | as `apply` moves a point anchor | an insert where `over` deleted the text on both sides, or `block` no longer holds `at` |
| `setBlock`, `container`, island props | unchanged | the id is gone or `over` removed it, or `over` changed what the step overwrites |
| anchor removal | unchanged | the id is gone |
| `nested` | its change rebased over what `over` did in that slot | the slot is gone, a step inside conflicts, or `over` removed the island or the slot and the slot now reads otherwise |

A step holds to what it overwrites as its writer read it, its own earlier steps included, and conflicts as `changed` where `over` changed that, rather than undoing `over`:

- a `setBlock` holds the fields of the block's record it names, so a `setBlock` names only the fields it changes, as a diff's do;
- a `container` step holds the container it sets or removes, and an island props step the props and the slots it drops;
- a splice holds each island and block record it deletes, so a stale delete or move of a block `over` re-kinded conflicts where ProseMirror's would delete it.

Where `over` removed an island or slot, the edits in it are not in the mapping, so a `nested` step lands only where the slot reads as it did.

Identity is by incarnation, not by spelling:

- A block record, container or island `over` removed is gone for the rebased change, even where a later step of `over` put one back under the same id, as an editor does that deletes a block and mints its freed id for a new one.
- A step naming such an id conflicts as `missing` rather than landing on the new holder: a `setBlock`, a `container` step, an island props step, an island insert's `block`, or a path a step writes. An island `over` only moves keeps its identity.
- An id the change introduces that `over` also introduced, as two writers minting past one value do, is renamed past the highest id either holds, from the step that introduces it through the change's later steps, and so in each slot both edit. The rename depends on `base`, the change and `over` alone.

A range `over` deleted entirely conflicts rather than dropping its step: a write is whole or refused, and a step with nothing left to land on is still part of what its writer asked for. A `Conflict` names the step, a `nested` step's index for one inside it, and its `Reason`: `deleted`, `changed`, `missing`, or `refused` where the mapped step fails otherwise or the change leaves an invalid value.

## The laws

Each law is a property test over generated values and generated changes (`tests/change_laws.rs`, `tests/repair_laws.rs`).

1. `apply(apply(d, c), invert(c, d)) == d`. Every change undoes exactly, deleted islands, anchors and block records included.
2. `apply(d, compose(a, b)) == apply(apply(d, a), b)`. Compose concatenates the steps and merges adjacent splices that touch.
3. `map_pos(c, p)` agrees with `apply`: the two edges of a character both say it was deleted, or land around it afterwards in the same order, a character inside a nested slot included, and one in an island the change moves keeps its place. A change's maps followed by their mirrored inverses take every position home.
4. `parse(serialize(d)) == d`, and equal values serialize to equal bytes.
5. Every `Change` yields a valid value or fails and changes nothing.
6. `repair` is total and deterministic: any merged state repairs to a valid value, the same state always to the same value, and a valid value to itself with nothing reported. A write-back reads as the merged state it repaired: by any number of peers where no repair inserts, since the value then holds no character or slot the state does not, and by one writer otherwise.
7. Where `rebase(d, c, o)` succeeds, `apply(apply(d, o), rebase(d, c, o))` succeeds and leaves a valid value, which the rebased change's inverse takes back exactly, and which keeps what `o` did by id: each block kind and path, container, and island's props `o` changed, each block it changed, and each island or slot it changed that `c` drops without editing what `o` changed in it. `rebase(d, c, ∅) == c`. Two changes to text either side of a character neither touches, joining no blocks, rebase over each other and reach the same value either way round, holding the text each wrote, but for the id of a block both introduced, which the rebased one renames.

## Writing a value

One write primitive and two producers over it, on a card's body or richtext field (`crates/core/src/document/write.rs`):

| Write | Builds the change from | Anchors |
|---|---|---|
| `apply(target, change, base)` | the caller's steps, an editor's transaction lowered | move exactly as the steps move text |
| `revise(target, markdown, base)` | a writer's markdown: the `qm-*` tags of `annotated(target)` it keeps, then a word-aligned diff, rebased over what the read does not spell | kept by the tags kept, otherwise by the diff; drift reported |
| `replace(target, content, base)` | a whole value, through the same diff | each anchor the value does not name carried where the diff keeps its text; the rest reported |

**A revise keeps what its read does not spell.** Markdown spells less than a valid value can hold: an editor's empty paragraph, formatting a link's text does not carry, a cell's hard break before a paragraph break, a table past the import's budget. So a revise does not diff the writer's markdown against the value cold (`diff::revise_keeping`):

- It imports `annotated(target)` under the value's ids (`read`), and takes the change from `read` to the value (`over`), exactly what the read lost.
- It revises `read` with the writer's markdown, and rebases that change over `over`. What the read lost and the writer left alone rides back in. An edit over it lands as written where the rebase maps it, and refuses with `edit::rebase_conflict` where it rewrites what the read re-spelled.
- Where `read` is the value itself, `over` is empty and the revise is the cold diff, at the cost of one import more.

A binding's write names its card by `$id`, or the main card, never by index: a card moved or inserted between a writer's read and its write shifts an index onto another card, and where that card's value is equal to the one read, the base cannot tell. An id no card holds refuses with `edit::unknown_card_id`.

Each write answers a `Receipt`: `applied`, the change in its normalized form, which `invert(applied, before)` undoes; `hash`, the value's new hash and the next write's base; `droppedAnchors`; `movedAnchors`; a revise's import warnings; and `rebased`, true where the write rebased over a change since its base.

**Pass the base.** `base` is the hash the writer read (`hash(addr)`). Without one, a write lands over whatever the value now holds, so an edit made between the writer's read and its write reads as text the writer deleted. A value that no longer hashes to the base rebases the write over what changed since:

- Each card keeps a `ChangeLog` per value it holds, in memory only: one snapshot, the value its oldest write read, and each write's normalized change and the hash it left. No hash, comparison or stored form sees it.
- A base the log holds composes the changes from it to now, and the write rebases over them and applies. The value the base names is the snapshot with the changes before it applied as one composed change; one that does not hash to the base refuses as stale. A revise or replace diffs from the value the base names, so an edit made since is not read as text the writer deleted.
- Each block, container and island id a revise or replace introduces is minted afresh where the value now holds it, past the ids of the value the base names, the value now and the write (`diff::revise_holding`, `diff::replace_holding`), a replace's own id among them.
- Minting reads those two values and never the log, so equal values written over equal values store equal bytes, and `normalize` predicts what `replace` stores up to the first block's id, which a replace keeps.
- Two stale writes that each add a paragraph both land. An id a write freed may be minted again: `rebase` holds a stale step by it to the block it read, and renames an id the write introduces that a write since also introduced (§ "Rebasing a change").
- A write that conflicts refuses with `edit::rebase_conflict`, carrying `current`, `step` and `reason`.
- A base the log does not reach refuses with `edit::stale_base`, carrying `current`, and the writer reads again.
- The log is bounded by steps, nested ones counted, not by writes. A base rebases while the writes since it hold at most `LOG_STEPS` (4096) steps, and the base the newest write read rebases whatever that write's size. At one write a keystroke, that is minutes of typing, and a composed run of typing is one splice, so the rebase over it stays small.
- A write that takes the log past twice `LOG_STEPS` folds its oldest writes into the snapshot, one apply of their composed change, until it holds at most `LOG_STEPS`. A value's log holds one copy of the value and at most `2 × LOG_STEPS` steps, or the newest write alone where it holds more, and a fold comes at most once per `LOG_STEPS` steps written, or right after a write that alone holds more.
- A write the log did not see, such as a typed writer's revise, breaks the chain, and the log forgets what came before it.
- A card keeps the hash each write left on its body or field, which the next write reads as the value's hash instead of hashing it again; a write with no base hashes nothing before it applies. Any other write to the body or the payload forgets it.

`diff(before, after)` is the producer for a writer with no steps of its own. It diffs the text by line and then by word, and a revise carries its anchors through the same diff, but for one whose tag the writer kept, which stands where the tag does.

- The lines of a rewritten run pair by their best total word similarity, so an inserted line that resembles an edited one takes neither its record nor its anchors.
- A paired line diffs by word at UAX #29 word boundaries, each punctuation mark a word of its own, so a rewritten word changes as a whole and a range over it drops rather than riding its scattered letters, and a changed full stop leaves the word before it.
- An inserted or deleted block brings the `\n` that opens it, so its neighbours keep their records.
- Block records, containers, marks, anchors and islands follow as their own steps; an anchor `after` holds in another place, the prose or a cell, moves there. A block or island `after` holds in another place keeps its id, moved up or down.

- It refuses an `after` that is not a valid value, and reaches every valid one step-wise.
- `revise(c, annotated(c))` changes nothing, for every valid `c` the model reaches, markdown-born or not (`tests/change_laws.rs`).

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

The raw value is shaped as `Content` with two additions only repair reads. Its `islands` may be an object keyed by island id, with `slot_ids` naming the island each `U+FFFC` carries in slot order, `null` where it carries none; an `islands` array pairs with the slots in order. And an anchor placed as a range carries `range: true`, since a range collapsed to zero width cannot otherwise be told from a point. A v1 value, `lines[]` with no `blocks`, is read as `migrate::from_lines` lifts it.

Beyond the table, repair is total over any value shaped as content: it keeps marks and anchors within their bounds (§ Validity), removes forbidden characters, mints a missing block record and drops an extra one, re-mints a bad or duplicate id, drops a slot under a bad key, mends a table grid a concurrent row and column edit broke, clips marks and anchors to the text, and turns a rule holding text or a code block holding an island into a paragraph. A fresh id is the ids involved joined by `-`, with a numeric suffix where that id is taken, so two peers writing back one repair write the same bytes.

## The line form

The renderers and the markdown codecs read the line form under the `lines` module, one line per block with marks and islands inline. `migrate::to_lines` lowers a value to it and `migrate::from_lines` lifts a v1 value, minting ids deterministically; a markdown import runs through the line form and lifts ([CONVERT.md](CONVERT.md), [PARITY.md](PARITY.md)). A card lowers its body when the line form is first read, not on each write, so a write no renderer reads pays nothing for it.

The line form is a faithful view of a value but for what `to_markdown_checked` names as its loss, which a law holds over generated values (`tests/change_laws.rs`): an `x-` block lowers to a paragraph and an `x-` mark drops (`Loss::ExtBlock`, `Loss::ExtMark`), and a slot's text is spelled with a block break as two line breaks, so a hard break beside a block break or at the slot's edge reads back otherwise (`Loss::SlotBreaks`). What it loses, a content write's revise keeps (§ "Writing a value"), and the anchor contexts and receipts read the stored text, not the line form.

| v1 | The model |
|---|---|
| `lines[]` with a container path | `blocks[]` with a path of container ids; the records become `containers[]` |
| `continues: true` lines | One block, its lines joined by hard breaks |
| A code block's `lang` on every line | Once, on the block |
| `ordinal`, `instance` | Gone: numbers are computed, and runs are told apart by container id |
| `anchor` marks | `anchors[]` |
| Table cells as `{text, marks}` in `props` | Slots with ids; a cell's anchors move into its slot |
| A footnote's note in `props` | The `note` slot |
