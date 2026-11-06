# The Content Model

> **Implementation**: `crates/content/`
> **Related**: [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) (how a value is stored), [COMMENTS.md](COMMENTS.md) (what anchors serve), [BINDINGS.md](BINDINGS.md) (the write surface)

## TL;DR

One content value per card body, `richtext` field or `text` field: flat text, explicit blocks and containers, range marks, comment anchors, and islands that hold rich text of their own. Every structural part has an id and nothing derived is stored. Every edit is a `Change` that applies, inverts and composes, and a value two concurrent edits merged into an invalid state repairs deterministically.

## Glossary

Each term below means one thing across `prose/canon`. The last column lists the words and senses retired for it; `scripts/check-canon.mjs` fails on any of them anywhere in canon outside this table, a trailing `*` standing for any word ending.

| Term | Meaning | Retired |
|---|---|---|
| block | One `\n`-separated segment of a value's text and its record in `blocks`: kind, attributes and container path. | |
| container | A list, list item or quote. A block's path names the containers it sits in, outermost first. | |
| mark | Formatting over a range `[start, end)` of a value's text: `strong`, `link` and the rest of the mark axis. | "punctuation mark" |
| anchor | A comment handle over a range or a point of a value's text, under an id the host mints ([COMMENTS.md](COMMENTS.md)). | "YAML anchor" |
| island | A table, image, footnote, equation or `x-` island, held at one `U+FFFC` in the text, its island character, with props and slots. | "slot character" |
| slot | A nested `Content` an island holds under an id: a table cell's content, a footnote's `note`, an `x-` island's own. Only an island has slots. | "slot count", "slot order", "field slot", "card slot" |
| cell | One row and column of a table. Its content is a slot holding one paragraph. | "optional cell*", "variant cell*", "content cell*", "declared cell*", "unanswered cell*", "cells and namespaces" |
| incarnation | One life of a block record, container or island under an id, from the step that puts it to the step that removes it. An id put back after a removal is a new incarnation. | |
| scalar | A Unicode scalar value, the unit every stored offset counts. A binding converts its host's units at its boundary. | "USV*" |
| canonical form | The one spelling `normalize` gives a valid value. A write takes a value only in it (§ Validity). | "settl*" |
| stored form | The shape a field is stored in once a schema-aware write lands it: `Quill::conform`, the typed writer or the seeder. It is what `toStored` writes ([SCHEMAS.md](SCHEMAS.md) § "Content fields are stored per codec"). | "resting*", "rests", "rests as", "at rest" |
| input path | A way a document or a value enters the engine. The bound input path (`quill.parse`, `quill.conform`, `quill.writer`) conforms what it takes; the transport input path (`Document::parse`, a stored row) holds it as authored. | "door*" |
| write path | A way a value is written: the typed writer (`quill.writer`), the content write on `Document` (`apply`, `revise`, `replace`), or the schema-free `unchecked` store. The verb names the path ([BINDINGS.md](BINDINGS.md) § "The write surface: object placement over one primitive"). | "lane*" |
| structural edit | `split_block`, `join_blocks`, `delete_range`, `indent`, `outdent` and `wrap`: verbs that build a change from the value and write it. `inherited_marks` is a read beside them. | "gesture*" |
| source | Where a resolved field value came from: authored, `default:` or blank ([SCHEMAS.md](SCHEMAS.md) § "Value sources and projections"). | "rung*" |

Five verbs make a value or a document valid, each for its own caller:

| Verb | Takes | A host calls it |
|---|---|---|
| `normalize` | a `Content` | to put a value it built by hand in canonical form before writing it |
| `validate` | a `Content` | to ask whether a value holds to § Validity; every write does it too |
| `repair` | a merged state | to read a collab state two peers merged (§ "Merge repair") |
| `conform` | a `Document` and its quill | to land a document from the transport input path in its stored form |
| `migrate` | a stored row or a v1 value | never: a read of an earlier storage version runs it ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Legacy schemas") |

## The value

A short field as it is stored. A `\n` opens a block, `U+2028` is a hard break inside a block, and `U+FFFC` is an island character, the one character an island occupies.

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
- **Islands** are tables, images, footnotes and equations, one per island character in text order. A table's cells and a footnote's note are slots, each a nested `Content`: a cell and a note are each one paragraph, its lines parted by hard breaks (`U+2028`), and `validate` refuses a `\n` in either; an `x-` island's slot is paragraphs. Rows, columns and slots have ids, so an anchor in a cell stays in its slot wherever the row moves.
- **`codec`** is `"plain"` on a `text` field's value and absent on every other. A plain value is read and written as its literal text, a paragraph break a blank line and a hard break a lone `\n` both ways: markdown projects it so, a revise reads its input as text, and its annotated read spells no tag ([SCHEMAS.md](SCHEMAS.md) § "Content fields are stored per codec"). Any other name is `ParseError::UnknownName` with axis `codec`. The key is part of the canonical bytes, and a change, which applies to the value it was made from, never sets it.

Offsets are Unicode scalar values in storage. Each binding speaks its host's units at its boundary and converts inside ([BINDINGS.md](BINDINGS.md) § "Content offsets").

| Axis | Members |
|---|---|
| Block `kind` | `para`, `heading` (`level` 1 to 6), `code` (optional `lang`), `rule` |
| Container `kind` | `list` (`ordered`, `start`), `item` (optional `checked`), `quote` |
| Mark `type` | `strong`, `emph`, `underline`, `strike`, `code`, `link` (`url`), `font_family` (`name`) |
| Island `type` | `table`, `image` (`url`, `alt`), `footnote` (slot `note`), `math` (`tex`, optional `display`) |

Each axis is a closed set, and a name outside it is `ParseError::UnknownName` at every decoder, so a value holding one does not open. Block kinds, mark types and island types also hold the reserved `x-` names: `x-` and at least one more character, an id as a whole. One decodes into the axis's `Ext` member, `attrs` or `props` held as written, so a value a newer build wrote opens and re-encodes to the same bytes ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies"). Container kinds hold none, since a reader cannot check a path through a nesting rule it does not know.

- Validation of an `x-` member is structural only: the name, the ranges, and its bag's JSON depth. An `x-` island holds any slots, each of paragraphs.
- The renderers and the markdown export read an `x-` block as a paragraph, drop an `x-` mark, and keep an `x-` island with its slots as cells. The renderers draw the island as nothing, and markdown writes it as nothing, reporting the loss; the annotated read spells it as its token, so a revise keeps it.
- A revise keeps a kept block's `x-` kind where the markdown reads back as a paragraph, and an `x-` mark over the text the diff keeps.
- A revise or replace at one address (`Card::revise`, `Card::replace`, the typed field revise) whose change removes an `x-` island refuses with `edit::drops_ext_island`, since its writer may never have seen it; a change through `apply` removes one. A whole-document revise (`Document::revise`) does not check, and a plain markdown revise of the document drops the island.
- Steps set and remove `x-` names like any other.

## Validity

`Content::validate` is what every stored value holds to, and what every change leaves. It refuses:

- a `\r`, a bidi control, or a line separator other than `U+2028` in the text;
- a block count other than the segment count, or an island count other than the island-character count;
- a block, container or island id that is empty, longer than 128 scalar values, or holds whitespace, a control or an island character, an anchor id that is empty (the rest of an anchor id is the host's), and an id held twice: blocks, containers and islands each in their scope, anchors once over the value and every slot;
- a path naming a missing container, a path whose kinds do not nest (an item only right after a list, a list only right before an item), or one nesting past `MAX_NESTING_DEPTH` (12);
- a container whose blocks are not one contiguous run, that sits under two different parent paths, or that no block's path names;
- a heading holding a hard break, a rule holding text, a code block holding an island, and a block-only island (a table) sharing its block with anything else;
- an island whose props do not read as its type's (a table whose grid names a row, column or cell no slot backs, or a slot no cell names), and a table of more than `MAX_TABLE_CELLS` cells;
- a mark or anchor past the text, an empty mark, a slot holding anything but paragraphs in no container, and a table cell or a footnote's note holding more than one paragraph (`Invariant::BreakInCell`);
- a `codec: "plain"` value holding a mark, an island, a container or a block other than a paragraph (`Invariant::NotPlain`), so no step formats a plain value and repair flattens one that a merge formatted;
- more marks on one text, a field's or a slot's, than `max_marks` of its length: `MARKS_BASE` (64) and `MARKS_PER_CHAR` (4) per character, and more than `MAX_ANCHORS` (100 000) anchors over the value and its slots.

Normalizing spends the mark bound before validating can refuse: a mark crosses as many blocks as it spans, and `x-` marks of distinct attributes never union, so marks times blocks of them come out of a value that names each once. The decoder (`Content::from_value`) therefore checks the bound first, counting each mark once per `\n` and `U+2028` it spans plus one, the most it can be cut into (`Content::check_bounds`), and so does a change: a step whose marks would pass the bound refuses before it normalizes them, as `Invariant::TooManyMarks`. Repair keeps the marks in order up to the bound and the anchors in reading order up to theirs, and reports the rest.

`Content::normalize` gives the one canonical form: marks cut at each `\n` and `U+2028`, unioned and sorted; anchors sorted; containers in the order paths first name them, an unused one dropped; island props key-sorted and normalized (a table's layout keys absent at their defaults and `head` clamped to its rows, an equation's `tex` trimmed and `display` absent at `false`). A value's wire form (the `wire` module) writes every object's keys in ascending order and omits a key whose value is empty, so equal values serialize to equal bytes.

**One spelling per value.** The decoder a write reads through (`Content::from_value`, and every binding's `Content` input) checks the bounds, then validates the value as decoded, then refuses one `normalize` would rewrite or drop (`Content::check_canonical`):

- a mark not cut, resolved or sorted, an `x-` mark over only a break among them, an anchor or container out of order, or island props `normalize` would change, as `Invariant::NotCanonical` naming the part;
- a reversed or empty mark and an unused container, which are already invalid;
- a key the value's shape does not define, on the value or any record or slot in it, and an `attrs` entry a built-in member does not name, as `ParseError::UnknownKey` ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies").

So a value decodes only from the bytes it encodes to, up to the wire's own spelling of an empty key (absent or empty reads as one), and decoding then encoding is the identity on every value a write accepts. `diff::replace` and `Card::replace` refuse the same values a decode does. `normalize` is the producer's verb: a producer that builds a value by hand normalizes it before it writes it.

**A stored value reads leniently.** The read of a value already stored (`Content::from_value_lenient`: a stored row's body and fields, a field a document holds, the renderers' read) checks the bounds, joins a table cell or a footnote's note of several paragraphs with `U+2028`, normalizes and validates, and says whether it rewrote anything. `Document::from_stored` warns `content::noncanonical_read` at each body or field it rewrote, so a host stores the row again ([ERROR.md](ERROR.md) § "Warning flow").

## Ids

Blocks, containers, islands, slots, table rows and table columns have structural ids. Quillmark mints every one, by one rule (`ids`, `mint`):

- Import, migration and every write mint positionally, `{prefix}{n}` past the highest number the values in play hold under that prefix (`b`, `l`, `i`, `q`, `isl-`, `r`, `k`, `s`), so the same markdown always imports to the same bytes and one writer never mints an id the value holds. Two copies of a value minting apart can mint one id; telling their records apart when they merge is `@quillmark/collab`'s, which mints its own under each client and re-mints what collides.
- A record a change puts may carry a placeholder in place of its id: `?` and an id, such as `"?1"`, which other steps of the change may name. No id opens with `?`. The write that lands the change mints an id for each placeholder and names it in the receipt's `minted`, `{"?1": "b4"}`; a placeholder put twice, or named and never put, refuses the change.
- A write fills a placeholder past every id the value it lands on holds (§ "Writing a value").

Determinism holds for import, migration and every write ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Island-id determinism"). Anchor ids are the host's: no content determines what a comment refers to, so Quillmark mints none ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "The id handles").

## Changes

A `Change` is a list of steps. Each step reads the value the step before it left, and `apply` runs all of them or none.

A `Change` is session-scoped, not a storage format: its JSON carries no version, and a later build may decode a change an earlier one wrote otherwise or refuse it. A host persists the value, never the changes that reached it; a receipt's `applied` serves undo within the process that took the write (§ "Writing a value").

| Step | Shape | What it does |
|---|---|---|
| `splice` | `{at, delete, insert, marks?, blocks?, islands?}` | Replaces text. `delete` is the removed text itself, and the step fails if the value holds anything else there. `marks` is the formatting the inserted text carries; omitted, it carries none. Each `\n` in `insert` brings the record of the block it opens, and each `U+FFFC` its island. The removed records, marks and islands travel with the step as `removed`, for inversion. |
| `setBlock` | `{id, kind?, attrs?, path?}` | Changes a block's kind, attributes or path. Indenting and outdenting are path changes. |
| `container` | `{id, kind, attrs?}` or `{id, remove: true}` | Adds a container or sets an existing one's attributes, or removes one no path names. |
| `mark` | `{start, end, type, attrs?, add, quote?}` | Adds or removes formatting over a range. |
| `anchor` | `{id, start, end, quote?}`, `{id, start, end, quote?, move: true}` or `{id, remove: true}` | Places an anchor, moves one, or removes one. A placement of an id the value or any slot holds is refused, so two hosts minting one id collide loudly; a move or removal of an id its text does not hold is refused. |
| `island` | `{id, props, slots?}` or `{at, island, block?}` | Replaces an island's props, or inserts an island with its island character. A slot the new props name and the island lacks takes its content from `slots`, which names no other slot; a slot they no longer name leaves. Deleting the island character in a splice removes the island. |
| `nested` | `{island, slot, change}` | Applies a change inside one slot: a table cell or a footnote. |

`apply` runs the steps in one pass. Each step holds what it touched to what `validate` asks of it, its inserted text, records, marks and islands, and the value the last step leaves is validated once, whole. Block and island ids are unique only there: between steps a change may hold one twice, as a move does when it inserts the copy before it deletes the original, and a step that names an id held twice is `ApplyError::BadStep`, since it cannot say which it means.

Each step holds to what its writer read as far as its precondition reaches. A step whose precondition fails is `ApplyError::Stale`:

- a splice holds its `delete` to the text at `at`;
- a `mark` step and an `anchor` placement hold their range to `quote`, the text it covered when the writer read it;
- an island insert holds `at` to `block`, the id of the block the writer read it in;
- the id-addressed steps fail on a missing id, an anchor move and removal included, and an anchor placement on a held one (`ApplyError::AnchorHeld` where its text or a slot in it holds the id, the whole value's validation where another slot does).

A step whose object holds a key its kind does not define, a splice's `delet` or a mark's `qoute`, is `ParseError::UnknownKey` at every decoder, so a misspelled precondition never decodes as a step without it. So is a change holding a key beside `steps`, and a record or island a step carries holding a key its shape does not define, as a value's decode refuses one ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies"). A step's `props` and an `x-` member's `attrs` stay open.

`quote` and `block` are optional on input. The normalized change, `Applied.applied`, fills both in, as it fills a splice's `removed`, so it holds every step to what its writer read. `invert` fills them in its own steps, and each splice's `removed`, from the value each undoes: each step answers its inverse as it applies, and `invert` is those inverses read back last first.

With no base, a step is refused rather than landing on whatever now sits at its offsets only where its precondition reads the value: a splice that deletes text, a `mark` step or anchor placement that carries `quote`, and an island insert that names its `block`. A step by id, a `setBlock`, a `container` step or an island props step, lands over whatever the value now holds, so a stale props step that adds a column deletes a row added since.

A writer outside the live session therefore passes the base, and a write whose base the value has moved on from refuses (§ "Writing a value").

A split inserts a `\n` whose record names the right half; the left half keeps its id, since the record that opens it does not move. A join deletes the right block's `\n` and record, and the joined block keeps the left's id and kind. Splits and joins therefore never rewrite a record a concurrent `setBlock` could be aimed at. Collab's Y form keeps the same boundary, so a split or a join lowers into Y as an insertion or a deletion alone.

Enter at the start of a block keeps the text on its id when the producer inserts the `\n` ahead of the one that opens the block, so the new record names the empty block before it. No `\n` carries the first block's record, so no splice reaches its id: Enter at the start of the first block moves its text to the new record, and `diff` and `replace` keep the stored first id.

An editor lowers a transaction in two ways, both exact. The structural keys go through the structural edits, which are verbs on the document:

- `split_block`, `join_blocks`, `delete_range`, `indent`, `outdent` and `wrap` on `Card` (`doc.splitBlock(addr, at, opts)` and the rest in WASM) build the edit's change from the value and write it as `apply` does, base and receipt included.
- A record a structural edit opens takes a placeholder, `?0` first, which the write fills; `split_block` takes the new block's id or mints one. The `structural` module holds the same changes as functions over a value, and an editor that lowers through them builds no container path.
- `block_at(addr, offset)` (`blockAt`) answers the block holding an offset, its index and its container path, so an editor builds no path of its own.

Text edits go through `diff_range(before, after, from, to)` (`diffRange(before, after, {from, to})`), the exact lowering of a range an editor already knows, ProseMirror's `ReplaceStep`:

- `from` and `to` are offsets into `before`; `after` holds the same text before `from`, and from `to + after.len - before.len` on the text `before` holds from `to`.
- The change is one splice over the characters of that range that differ, so typing one letter into a word is a one-character splice, never coarser than the range. Where the records beside it place an inserted `\n` on the other side of an equal one, as Enter at a block's start does, the splice slides to meet them.
- Records, containers, marks, anchors and islands follow as `diff`'s do, so applying it reaches what `diff` reaches.
- A range the two values do not agree outside, in text or in the id a `\n` or island character holds, is not the change's range, and the change is `diff`'s.

`diff` stays for a writer with no steps (§ "Writing a value").

`apply` never grows a mark: a splice states the marks its inserted text carries, so every inverse is exact. The growth defaults are for producers, and the read `structural::inherited_marks(content, at)` states them, for a producer that asks: `strong`, `emph`, `underline` and `strike` grow at their end, `code`, `link` and an `x-` mark at neither edge, and `font_family` at both, the family before `at` winning over the one after. A `\n` and a hard break hold no mark, so at a block's or a line's start only a family grows. Anchors never grow: a range keeps its edges, and a point stays left of text typed at it.

A collab session merges marks by Yjs's rules, not by these defaults. Where one peer formats a range while another types at its edge, client order decides whether the typed text takes the mark, so a session's merge can differ from the two edits applied one after the other.

What each structural edit does to the blocks it reaches:

- `delete_range` deletes a range. Where it runs from a block's start into a later block, the block that keeps the text takes the kind, attributes and path of the block the range ends in, as ProseMirror's delete does, and keeps the first block's id.
- `indent` (Tab) nests the items of the innermost list holding a range under the item before them, joining that item's list where it ends in one and else opening a list of their own list's kind.
- `outdent` (Shift-Tab) lifts a range out of its innermost item or quote. A nested item becomes a sibling of its parent, taking the items after it and the rest of its parent as its own; a top-level item leaves its list; a quote's children leave it. What follows the lifted run stays in its container, or opens one of its kind where the run split it.
- `wrap` opens a quote or a list around the children of the range's innermost shared item or quote, a list the range sits in wrapped whole.

Tab then Shift-Tab over the same blocks puts the value back.

`mapping` maps a position across a change, after ProseMirror's maps. A `StepMap` reads off one step what it does to positions, and `map_pos` answers a `MapResult`: where the position lands, and whether the characters before it, after it, on its `assoc` side, or across it were deleted. A position always lands:

- one inside deleted text on the deletion's `assoc` edge;
- one inside an island a splice deleted on the island's character in the text that held it;
- one in a slot a props step removed at its own address, since the step does not say where its island sits.

An island a change removes and inserts again under its id, a removal paired with the next insert of that id or the other way round and the slots going as they came, as `diff` and `replace` move one, is moved rather than deleted across the two: a position in its slots keeps its address.

A splice says which islands it deleted only in `removed`. A change mapped holds it on each splice that deletes an island character, as the normalized change does, and one without it is refused. A `Mapping` is a list of step maps with mirror links, each pairing a map with its inverse, so a position one map deleted and its mirror put back maps to where it was. `Mapping::map_point` maps a position as `apply` moves a point anchor, and `project` takes a mapping's maps inside one slot.

Text deleted under a range anchor shrinks it to what survives. An anchor whose text is deleted entirely is removed and named in the receipt's `droppedAnchors`; a surviving anchor the write moved, a point included, is named in `movedAnchors` with its `reason` (below). An island removed by a splice, or a slot a props step no longer names, takes the anchors of its slots with it, and each is named in `droppedAnchors` with the island and slot that held it. Every anchor a change takes out of the value is named there, except one its own `anchor` step removed, which the writer asked for.

A moved anchor's `reason` is the first of these that holds:

- `position`: an `anchor` step put it other than where the write's text edits move it, or in another slot;
- `block`: its start sits in another block;
- `context`: its text, or the `CONTEXT` (8) characters either side of it, read otherwise.

## The laws

Each law is a property test over generated values and generated changes (`tests/change_laws.rs`, `tests/repair_laws.rs`).

1. `apply(apply(d, c), invert(c, d)) == d`. Every change undoes exactly, deleted islands, anchors and block records included.
2. `apply(d, compose(a, b)) == apply(apply(d, a), b)`. Compose concatenates the steps and merges adjacent splices that touch.
3. `map_pos(c, p)` agrees with `apply`: the two edges of a character both say it was deleted, or land around it afterwards in the same order, a character inside a nested slot included, and one in an island the change moves keeps its place. A change's maps followed by their mirrored inverses take every position home.
4. `parse(serialize(d)) == d`, and equal values serialize to equal bytes. `parse` refuses a value `normalize` would change, and the lenient read of it reaches one `parse` accepts (`tests/properties.rs`).
5. Every `Change` yields a valid value or fails and changes nothing.
6. `repair` is total and deterministic: any merged state repairs to a valid value, the same state always to the same value, and a valid value to itself with nothing reported. A write-back of a repair none of whose mends inserts reads as the merged state it repaired, however many times it is written, since the value then holds no character or island the state does not.

## Writing a value

One write primitive and two producers over it, on a card's body or richtext field (`crates/core/src/document/write.rs`):

| Write | Builds the change from | Anchors |
|---|---|---|
| `apply(target, change, opts)` | the caller's steps, an editor's transaction lowered, or a structural edit's | move exactly as the steps move text |
| `revise(target, markdown, opts)` | a writer's markdown: the `qm-*` tags of the annotated read it keeps, then a word-aligned diff from the value | kept by the tags kept, otherwise by the diff or a verbatim move; drift reported |
| `replace(target, content, opts)` | a whole value, through the same diff | each anchor the value does not name kept where the diff keeps its text; the rest reported |

**A revise diffs from the value.** It imports the writer's markdown, holds the anchors and islands whose `qm-*` tags the markdown keeps, and diffs the value to that (`diff::revise`). Markdown spells less than a valid value can hold: an editor's empty paragraph, two hard breaks in a row in a cell, a table past the import's budget, an `x-` mark. What the value holds and the markdown does not spell is gone from the result, as `to_markdown_checked` names it, so a host whose writer must keep it writes through `apply`. An `x-` island is the exception: a revise or replace that would remove one refuses with `edit::drops_ext_island`.

- An anchor whose text the diff deletes is placed again where the revision inserted that text verbatim, as a paragraph moved whole is, when its quote is at least four characters (`MIN_MOVE`); a point anchor follows the text beside it under the same floor. A shorter quote is too weak to follow, and the anchor drops, reported.

**There is one revise**, at every address:

- `Card::revise` builds it, at the body or a field.
- The typed writer's revise (`TypedWriter::revise`, WASM `writer.revise`) is it, a field read under its schema's codec and then conformed.
- `Document::revise` runs it on each body and content field of each card it aligns. A value whose markdown imports to one that does not validate keeps its stored value, with an `edit::content_apply` warning at its address.
A binding's write names its card by `$id`, or the main card, never by index: a card moved or inserted between a writer's read and its write shifts an index onto another card, and where that card's value is equal to the one read, the base cannot tell. An id no card holds refuses with `edit::unknown_card_id`.

Each write answers a `Receipt`: `applied`, the change in its normalized form, which `invert(applied, before)` undoes; `hash`, the value's new hash and the next write's base; `droppedAnchors`; `movedAnchors`; a revise's import warnings; and `minted`, each placeholder and the id the write gave it. A write's one option is `base` (`WriteOptions`).

**Pass the base.** `base` is the hash the writer read (`hash(addr)`). Without one, a write lands over whatever the value now holds, so an edit made between the writer's read and its write reads as text the writer deleted. Core is single-writer:

- A value that no longer hashes to the base refuses the write with `edit::stale_base`, carrying `current`, the hash now, and writes nothing. The writer reads again and writes from that.
- A document keeps no history of its writes, so the check reads alike in every process, a document loaded from its stored row included.
- Writers that answer late and must merge with what landed since, a person typing beside an LLM, share a `@quillmark/collab` session, whose Yjs document merges them, and land the merged value through `replace`.

- Each block, container and island id a revise or replace introduces is minted afresh where the value holds it, past the ids of the value and the write (`diff::revise_holding`, `diff::replace_holding`), a replace's own id among them.
- Minting reads the value and the write alone, so equal values written over equal values store equal bytes, and `normalize` predicts what `replace` stores up to the first block's id, which a replace keeps.
- A card keeps the hash each write left on its body or field, which the next write reads as the value's hash instead of hashing it again; a write with no base hashes nothing before it applies. Any other write to the body or the payload forgets it.

`diff(before, after)` is the producer for a writer with no steps of its own. It diffs the text by line and then by word, and a revise keeps its anchors through the same diff, but for one whose tag the writer kept, which stands where the tag does.

- The lines of a rewritten run pair by their best total word similarity, so an inserted line that resembles an edited one takes neither its record nor its anchors.
- A paired line diffs by word at UAX #29 word boundaries, each punctuation character a word of its own, so a rewritten word changes as a whole and a range over it drops rather than riding its scattered letters, and a changed full stop leaves the word before it.
- An inserted or deleted block brings the `\n` that opens it, so its neighbours keep their records.
- Block records, containers, marks, anchors and islands follow as their own steps; an anchor `after` holds in another place, the prose or a cell, moves there. A block or island `after` holds in another place keeps its id, moved up or down.

- It refuses an `after` that is not a valid value, and reaches every valid one step-wise.
- `revise(c, annotated(c))` changes nothing, for every valid `c` the model reaches whose checked export names no loss, markdown-born or not (`tests/change_laws.rs`).

The interactive target is a field of 20000 blocks or 500000 characters: a keystroke's `apply` lands in 5 ms at 100000 characters, a one-word revise or a replace of a 20000-item list in 300 ms, a repair of a valid 10000-item list in 50 ms, and import and decode grow linearly (`crates/content/tests/speed.rs`, `crates/core/tests/speed.rs`).

## Merge repair

A single step cannot break the model, but two valid concurrent edits can merge into a state no step allows. `repair(raw) -> (Content, Repair[], Change)` makes any such state valid (`src/repair.rs`).

Its `Change` takes the merged text to the value's: each character a rule deletes, inserts or rewrites as a splice, a slot's edits as a `nested` step, and a table's slots a rule removed or added as a props step. A position mapped through it lands where repair puts an anchor there, which a law holds (`tests/repair_laws.rs`), so a peer maps its caret, or lowers an exact change into the merged state's coordinates, through it.

Repair is how a merged state reads: it is total and deterministic, so peers holding one merged state compute one value without writing anything. Whether and by whom a repair is written back into the shared state is `@quillmark/collab`'s; what core states is which write-backs are idempotent:

- A mend that only deletes or sets an attribute is idempotent. Two write-backs of it delete the same characters and records and set the same values, so their merge is one write-back, and it reads as the merged state did. A record minted for a `\n` that holds none, a split run's container and a re-minted id are sets too, each under an id derived from the ids involved.
- A mend that inserts, `Repair::inserts` (`inserts: true` on the wire), is not: a split block's `\n` and record, the space a heading's hard break becomes, the hard break a cell's paragraph break becomes, and a missing cell's empty slot. Two write-backs of it each insert their own: text lands twice and stays, since the extra copy is valid, and a cell minted twice keeps one write's slot under its id.

| Merged state | How it arises | Repair |
|---|---|---|
| A path names a missing container | One peer removes a list while another adds an item to it | The id leaves the path |
| One container under two paths | One peer indents a two-block item while another adds a block to it | Every block of the container takes the path prefix of its first block |
| One container over two runs | A block lands between items of one list, in no container | The later run gets a fresh container, its id derived from the original id and the run's first block id |
| An island character names no island record, or a record no island character names | Concurrent deletion and edit of a table | The island character is removed; the orphan record is removed |
| Two island characters name one island | A concurrent copy of an island character | The first keeps it; the second holds a copy under a derived id |
| Text beside a block island | Two peers type on either side of a table's island character | The block splits at the island's edges; the first piece keeps the record, as a split's left half does, and each later piece takes a fresh id |
| A range anchor at zero width | Its text was deleted while another peer moved it | Dropped and reported |
| A table cell naming no slot of its own | A concurrent row insert and column insert | The cell gets an empty slot, its id the row and column ids joined |
| A table cell or a footnote's note of several paragraphs | A merge that splits a block inside a cell or a note | Each `\n` in the slot becomes `U+2028` and the slot keeps its first record |

The raw value is shaped as `Content` with two additions only repair reads. Its `islands` may be an object keyed by island id, with `slot_ids` naming the island each `U+FFFC` holds in text order, `null` where it holds none; an `islands` array pairs with the island characters in order. And an anchor placed as a range holds `range: true`, since a range collapsed to zero width cannot otherwise be told from a point. A v1 value, `lines[]` with no `blocks`, is read as `migrate::from_lines` lifts it.

Beyond the table, repair is total over any value shaped as content: it keeps marks and anchors within their bounds (§ Validity), removes forbidden characters, mints a missing block record and drops an extra one, re-mints a bad or duplicate id, drops a slot under a bad key, mends a table grid a concurrent row and column edit broke, clips marks and anchors to the text, and turns a rule holding text or a code block holding an island into a paragraph. A fresh id is the ids involved joined by `-`, with a numeric suffix where that id is taken, so two peers writing back one repair write the same bytes.

A heading's hard break becomes a space as a splice rewrites it, so a range anchor that starts or ends on the break shrinks off it, as `apply` moves one.

## What markdown keeps

Markdown is a projection of a value, not its storage, so `from_markdown(to_markdown(c))` is `c` only where `c` is itself a cold import of markdown. This is the one list of what the round trip loses; the other pages point here.

- **Anchors.** The plain export spells none and a cold import mints none. The annotated read spells each one as a `qm-anchor` tag and each island it cannot write as a `qm-island` token, and a revise reads both back; a cold import drops the tags and reports each token ([markdown-spec.md](../references/markdown-spec.md) §6.4).
- **Ids.** A cold import mints every block, container, island, row, column and slot id positionally (§ Ids), so a value whose ids a live session or an earlier revise minted reads back under others. A revise keeps the ids of what it kept.
- **What markdown cannot spell.** An empty paragraph; a hard break beside a block break or at a slot's edge (`Loss::SlotBreaks`); an `x-` block, which reads back as a paragraph, and an `x-` mark, which drops (`Loss::ExtBlock`, `Loss::ExtMark`); a covered cell's alignment; a table or cell key Quillmark does not name; a table that shares its block's line, which the export breaks onto a line of its own. The annotated read spells an island that would lose a key as its token.
- **Spelling.** The export canonicalizes mark nesting, escaping, trailing whitespace and a table's pipe or HTML form, so the markdown a writer sent does not come back byte for byte.

`to_markdown_checked` names each loss of the third kind for one value. A revise and a cold import keep none of it, but for an `x-` island, which a revise refuses to remove (§ "Writing a value").

## The line form

The line form under the `lines` module, one line per block with marks and islands inline, is what the anchor contexts and a revise's tag read still read. It is internal to the workspace: hidden from the documented surface, and no public API takes or answers it. `migrate::to_lines` lowers a value to it and `migrate::from_lines` lifts a v1 value, minting ids deterministically. The markdown import walks markdown into lines and `from_lines` lifts the walk, normalizing it once, as the model ([CONVERT.md](CONVERT.md)); core's card bodies, field decodes, projections and text coercion hold the value that lift makes, and `carry` takes a revise's anchors and island ids across a rewrite of the model.

The markdown export and the renderers read the value itself, through `view`: one line per run of text between block and hard breaks, with the container path, the item ordinals and the run parity markdown spells computed from the blocks and containers.

The line form is a faithful view of a value but for what `to_markdown_checked` names as its loss, which a law holds over generated values (`tests/change_laws.rs`), and the anchor contexts and receipts read the stored text, not the line form.

| v1 | The model |
|---|---|
| `lines[]` with a container path | `blocks[]` with a path of container ids; the records become `containers[]` |
| `continues: true` lines | One block, its lines joined by hard breaks |
| A code block's `lang` on every line | Once, on the block |
| `ordinal`, `instance` | Gone: numbers are computed, and runs are told apart by container id |
| `anchor` marks | `anchors[]` |
| Table cells as `{text, marks}` in `props` | Slots with ids; a cell's anchors move into its slot |
| A footnote's note in `props` | The `note` slot |
