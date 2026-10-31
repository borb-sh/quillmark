# Document Storage Serialization

> **Implementation**: `crates/core/src/document/`

## TL;DR

`Document` is the typed in-memory model of a Quillmark Markdown file. Its
layout tracks the evolving Quillmark model and is **not** a stable interface.
To persist documents (e.g. in a database) without storing Markdown (whose
syntax also evolves), `Document` serializes to a **versioned JSON envelope**,
`StoredDocument`, whose wire format is frozen per schema version.

## When to use it

| Form | Round-trips? | Stable for storage? |
|---|---|---|
| Markdown (`Document::to_markdown`) | Yes | No: syntax evolves |
| `StoredDocument` JSON | Yes: lossless | Yes: frozen per schema version |

Use `StoredDocument` JSON whenever a `Document` must survive a process
restart or a crate upgrade: database rows, caches, message payloads.

What a schema-bound read hands a consumer is the values form: the stored value
with content as its codec's text, sparse, and carrying neither anchors nor
`$quill` ([SCHEMAS.md](SCHEMAS.md) § "The values form"). It is a projection,
never a row.

The plate JSON a render hands a backend is a lossy, one-way export,
crate-internal to `quillmark-core` and never a storage option.

## Design Principles

1. **Versioned envelope**: every blob carries a `schema` tag; readers
   dispatch on it and reject unknown versions.
2. **Frozen DTO per version**: each schema version has its own standalone
   type tree (`DocumentV0_129_0`, `CardV0_129_0`, …). These are never changed
   once shipped.
3. **Decoupled from the live model**: internal refactors of `Document` and
   its components only touch conversion code, never the wire format.
4. **Transparent API**: `Document` serializes through the envelope via
   `#[serde(into / try_from)]`; callers use `serde_json` directly.

## The Format

The current schema (`quillmark/document@0.135.0`) carries each card's full
ordered payload: typed `$` system metadata, user fields, and YAML
comments interleaved in source order: as a single discriminated-union
item list. A composable card's `$id` is an `{type: id, value}` item after its
`$kind` (§ Card-id identity). This is what makes inline-comment preservation symmetric across
the `$`/non-`$` boundary. A field item is `{type: field, key, value}`;
`body` is the content-model value ([CONTENT.md](CONTENT.md)) embedded
structurally (a nested object, not a markdown string), and a `richtext` field
rests as one inside its item's `value`; see Byte-stability.

```json
{
  "schema": "quillmark/document@0.135.0",
  "main": {
    "payload": {
      "items": [
        { "type": "quill", "value": "usaf_memo@0.1" },
        { "type": "kind",  "value": "main" },
        { "type": "ext",   "value": { "presentation": { "title": "Greeting Card" } } },
        { "type": "field", "key": "title", "value": "Hi" }
      ]
    },
    "body": { "blocks": [ { "id": "b0", "kind": "para" } ], "text": "Hi" }
  },
  "cards": [ ... ]
}
```

`StoredDocument` is an internally-tagged enum (`#[serde(tag = "schema")]`);
each variant carries a frozen DTO tree. Quill references are stored as
strings (parsed back via `QuillReference::from_str`). The discriminator on
payload items is `type` (not `kind`) to keep it unambiguous next to the
`$kind` metadata semantic. The full variant set is `quill | kind | id | ext |
seed | field | comment`; the `ext` and `seed` variants carry the
`$ext` / `$seed` maps verbatim and are stripped from the plate JSON
before backends see it.
Load-time warnings (a parse's, plus the `conform::*` diagnostics when the
document came through the bound door) live on the `Parsed` record, not on
`Document`, so they never reach this format; the bindings stash them on
their `Document` handle as session state and exclude them from `equals` and
the DTO alike.

### Legacy schemas (V0_124_0 to V0_134_0)

The tags `0.124.0`, `0.129.0`, `0.131.0` and `0.134.0` hold the line form,
the v1 content encoding ([CONTENT.md](CONTENT.md) § "The line form"). A row
under one loads through the `0.135.0` hop: its body, and each field value
that decodes as line-form content at any depth, migrates through
`migrate::from_lines`, minting ids deterministically, so the same row
always migrates to the same bytes. A `$seed` or `$ext` value stays as stored,
and the decoders read a line-form value wherever they meet one.

`0.124.0` carries the content vocabulary without the `footnote` and `math`
islands, the `term` line kind or the `description` container. It is the oldest
tag read: a row under any earlier tag (`0.81.0` through `0.116.0`) fails as an
unknown schema version (§ Schema Versioning).

A composable card naming no `$kind`, which an earlier parse accepted as a card and stored,
folds on load, under any tag, into the body above it: its fields and comments
as an untagged backtick code block, whose text is admitted as a body import
admits it, then its own body. Its `$ext` drops, since `$ext` reaches no render
and the body does. What the fold lands is new content: an island id the body
above already holds is minted anew (§ Island-id determinism), and an anchor
whose id it already holds drops whole, since Quillmark mints no anchor id
(§ Anchor-id identity). Every later card moves up one index.

## Byte-stability

Serialization is **byte-deterministic** within a given schema version:
equal `Document`s (by `PartialEq`) produce byte-equal JSON, and the same
document re-serialized in any later patch or minor release tagged with
the same `schema` produces the same bytes. This is load-bearing for
consumers that content-hash stored documents (template-divergence
detection, cache keys).

**Two disciplines in one envelope.** The outer envelope: struct field
order, the `cards` array, payload field values: stays compact,
insertion-ordered `serde_json`: `serde_json::Value` inside payload field
values keeps YAML insertion order via the workspace's
`serde_json/preserve_order` feature, and no whole-envelope key sort is
applied. Every `body` subtree, by contrast, is the recursively key-sorted
**canonical content form** (`StoredContent` in `dto.rs`): byte-identical
to `Content::to_value` and independent of `preserve_order`, even in a
consumer crate graph that lacks the feature. A `richtext` field's value is
written through the same form. Sortedness is semantic *inside* the content
(mark, island and attribute order carry no meaning, so the serializer commits
to one bit pattern); insertion order is semantic *outside* it (payload item
order is source order, and matters).

**Both directions validate.** `StoredContent` checks the body's invariants
(`Content::validate`) on the way out as well as in, failing the write with a
serializer error. A store that checked only on load would accept bytes it could
not read back; every write a `Card` takes already holds a valid value, so the
outbound check is the backstop.

The guarantee follows from: struct field order is fixed in the frozen
DTO tree; `Vec` fields preserve order by definition; the two disciplines
above each hold at their respective level. No whitespace normalization is
applied: the output is `serde_json`'s compact form otherwise. Bumping the
`schema` version is the only event that may change the byte layout of a
document written by the current writer.

**Resting form.** Byte-equality is over `Document` values, so it is only
as useful as the model's agreement on what a given document *is*. A
content field has one resting form per codec: a `richtext` field rests as
the canonical content object, a `text` field as its literal string
([SCHEMAS.md](SCHEMAS.md) § "Content fields rest per codec").
`Quill::conform` is what holds that: it walks a document's declared
content fields through the same strict write the typed writer commits
through, so a parse-then-conform and a typed write of the same values are
byte-equal. Documents that have converged hash equal when they are
semantically equal, so `equals` and content hashing hold for content
fields by construction rather than by construction history.
Non-content-typed fields keep their authored shorthands (`qty: "3"` stays
a string until something writes it); the typed write remains their
canonicalizer.

A document that entered through the transport door (`Document::parse`, a
stored row, `store_field`) rests as authored until it is conformed. That is
a named state, not a second resting form: it is readable, round-trippable,
and one bound load away from converging.

A comment's text rests without the whitespace ending it, as a parse reads its
line, so a stored row holding such text loads without it and rewrites once.

**Migrated rows.** A row under an earlier tag migrates forward on read
(§ Legacy schemas) and rests under the current tag once rewritten. Rewriting it
once read (read-repair) is the same lane `Quill::conform` uses: a row read
through the bound door converges to canonical rest and is eligible for rewrite
under the current tag. A population converges once and then holds.

## Content vocabularies

The envelope's version-and-reject discipline covers the document's **shape**:
the schema tag, the DTO tree, the keys a `body` object carries. The content's
four discriminators are covered by their own rule: each is a **closed set**
([CONTENT.md](CONTENT.md) § "The value" lists them).

| Axis | Rust type |
|---|---|
| Block `kind` | `model::BlockKind` |
| Container `kind` | `model::ContainerKind` |
| Mark `type` | `model::MarkKind` |
| Island `type` | `IslandType` |

A name outside one of them is `ParseError::UnknownName { axis, name }` at every
decoder, so **a row holding one does not open**. Refusing is the only reading
that keeps the reader honest: projecting an unrecognized `kind` as `para` would
re-encode the row as `{"kind":"para"}`, moving canonical bytes on a read with no
edit (§ Byte-stability), and dropping the value loses content on a mere open. A
malformed discriminator is a different failure and stays `ParseError::Shape`:
the closed set answers for names, and a non-string is not a name.

Three names a line-form row may hold sit outside the sets: the `term` line
kind, and the `description` and `element` containers, which earlier tags
spelled for a definition list and a `qm-*` element. Each reads as what it
held, so the row opens and its text stays, and the migration writes the row
without them. A `term` line reads as a `para`, as the retired `island` line
kind does.

The consequence, and the point: **adding a member to any of these vocabularies
is a storage-version event.** An older reader refuses a row carrying a newer
build's construct, and the schema tag is what lets it say "build too old"
rather than "corrupt" (§ Schema Versioning).

**Payload rides `attrs`, for every member.** A `heading`'s `level`, a `code`'s
`lang`, a `link`'s `url`, a list's `ordered` and `start` and an item's
`checked` are all `attrs` entries: `{"kind":"heading","attrs":{"level":1}}`.
The envelope keys stay siblings (`id`, `kind`/`type`, `path`, a mark's
`start`/`end`) because they belong to the object rather than to the member it
names. An empty bag is omitted, so `normalize` collapses `{}` and an absent bag
to one spelling. A foreign bag key on a built-in drops unread: the decode reads
the entries the member names and re-mints from its fields. One spelling per name
is also what makes the canonical mark order a function of the bytes:
`MarkKind::sort_key` is the `(type, attrs)` pair the wire carries.

`font_family` carries a family's name under `attrs.name`, stored as written and
naming no font file: a backend resolves it at render. The name is not empty,
has no space at either edge, and holds no control character, bidi control or
line separator (`is_font_family_name`).

`font_family` and `link` are keyed (`MarkKind::is_keyed`): the tag is a key and
the attrs its value, so a character holds at most one family and one link.
Normalization settles overlap in raw input once, before the same-kind union:
each stretch between two edges goes to the mark over it that starts later, then
to the shorter, then to the later in canonical order. A `mark` step adding a
keyed kind sets its value over its range, replacing every mark of the tag there.
Storing one value per character keeps every edit local: a text edit never
changes the family or url of text it keeps.

A formatting mark never spans a `\n` or a hard break: `Content::normalize` cuts
it at each one, so a mark over a break is one mark per line, the form markdown
writes and the Typst lowering draws.

**A `Content` stores its value, not how the value came about.** What a markdown
import drops on the way in, a table cell's inline image whose alt text lands as
the cell's text and whose url lands nowhere, is a fact about that import, not
about the island it mints: two tables whose `props` and slots are equal are
equal, however each was written. Surfacing a drop belongs to whoever runs the
import, where it happens. The import reports the raw tags it drops beside the
content ([markdown-spec.md](../references/markdown-spec.md) §6.2 names the
silent ones), and core surfaces those of a body's parse and of a revise as
`parse::dropped_construct` ([ERROR.md](ERROR.md) § "Warning flow").

Unknown *keys* survive in designated carriers only:

- **Opaque carriers keep what they hold.** An island's `props` round-trip
  untouched, and so does a table cell's own entry under `cell_attrs`.
- **Envelopes drop what they do not name, by design.** A block, container,
  mark, anchor or island object is decoded into a struct and re-minted from
  its fields, so an unrecognized sibling key does not survive. Growing those
  shapes is a schema event.

A `footnote` island holds its note in the slot `note`, and an `image` its `url`
and `alt` in `props`. A `math` island's props hold the equation's TeX under
`tex`, a string on one line with no edge whitespace, and `display: true` on an
equation set on its own line, absent at `false`. The authored lanes refuse a
`tex` markdown cannot write between its dollars (an unescaped `$`, braces that
do not balance, an empty inline one) as `ParseError::Shape`; the stored lane
takes it, and markdown writes such an equation as other markup.

A table island's props name its grid and its layout (`table` module):

| Key | Value | Default |
|---|---|---|
| `columns` | the column ids, in order | (required) |
| `rows` | `{id, cells}` per row, `cells` mapping each column id to the slot holding the cell; `rows[0]` is the header row | (required) |
| `head` | how many rows from the top are header rows, at most the row count; `0` draws the first row as a body row | `1` |
| `aligns` | a column id's alignment: `left`, `center` or `right` | none |
| `widths` | a column id's integer weight, from 1 to 2⁵³ − 1; an absent column auto-fits | every column auto-fits |
| `align` | the table's placement: `left`, `center` or `right` | the plate's placement |
| `cell_attrs` | a slot id's own keys | none |

`widths` are weights, not lengths, so each plate decides what full width is.
Keying them by column id means a column inserted or moved leaves every other
column's layout where it was. A cell names four keys of its own under
`cell_attrs`, each absent by default:

| Key | Value | Default |
|---|---|---|
| `align` | the cell's horizontal alignment: `left`, `center` or `right` | its column's `aligns` entry |
| `valign` | the cell's vertical alignment: `top`, `middle` or `bottom` | the plate's |
| `colspan` | the columns the cell covers, a whole number from 1 | `1` |
| `rowspan` | the rows the cell covers, a whole number from 1 | `1` |

A cell's value equal to its column's is an override that stays when the
column changes. The grid stays rectangular under a span: each cell a span
covers keeps its slot as an empty placeholder, so a merge needs no
storage-version event. `Content::normalize` settles each span so it covers
only what it can (`island::table_spans`):

- Cells claim slots in reading order, so the earlier span wins.
- A span covers only cells holding no text, no mark and no anchor that no
  other span covers, inside the grid and inside its own section: the `head`
  rows or the body. The cell a comment targets stays drawn.
- A span is stored as it covers, absent at 1, and a covered cell's span keys
  are absent. A covered cell keeps its `align` and `valign`, which it draws
  again once no span covers it; markdown spells no covered cell, so
  `from_markdown(to_markdown(c))` drops them.

A table or cell key holding a value outside its set is read as absent and
rides untouched, as an unnamed key does. Markdown writes a table as a GFM pipe
table or as HTML, whichever holds it
([markdown-spec.md](../references/markdown-spec.md) §6.8); a cell of several
paragraphs writes as HTML.

Three rules bound the payload:

- **Payload depth is capped at `MAX_JSON_DEPTH` (128).** Island `props` is host
  JSON of arbitrary shape, but not arbitrary depth: key canonicalization, the
  content-hash key, and `serde_json::Value`'s own `Drop` each recurse one frame
  per level, so an unbounded bag overflows the stack: on wasm32, a trap that
  takes the module down rather than an error the host can catch. The cap is the
  one `serde_json::from_str` already enforces, so it refuses nothing a stored
  blob can carry. A bag is refused where the decoder reads it, before it is
  cloned into the model, and `Content::validate` restates it as
  `Invariant::JsonTooDeep`.
- **A table's grid is capped at `MAX_TABLE_CELLS` (100 000) slots**, its rows
  times its columns. The decoder refuses such a table as
  `Invariant::TableTooLarge`; markdown import drops the rows past the cap
  instead, its tables sharing one cap, warning `table:size`.
- **Nesting is capped at `MAX_NESTING_DEPTH` (12)** items and quotes on one
  path, the depth the markdown codecs and the Typst lowering share.

The opaque props are hash input like everything else in the canonical form, so
they are recursively key-sorted along with the rest (see Byte-stability). Also a
schema event: a change to the content object's own structure, a new top-level
key beside `text`/`blocks`/`containers`/`marks`/`anchors`/`islands`, or a
changed meaning for an existing discriminator.

## The id handles

Structural parts, anchors and composable cards each carry an id, and all are
the same handle: **opaque, unique within a scope, hash input, stable for the
session, carried through edits and never rewritten.** They differ on one axis:
**who mints one, and why only they can.** Uniqueness scope, collision response,
and markdown round-trip all follow from that.

|                     | Block, container, island, slot, row, column `id`  | Anchor `id`                                        | Card `$id`                                          |
| ------------------- | -------------------------------------------------- | -------------------------------------------------- | --------------------------------------------------- |
| Minted by           | Quillmark at import, migration and revise; the caller in a step | the caller                            | the author or caller, else Quillmark where the card is placed |
| Because             | content determines it: the nth part minted         | the referent is external; no content determines it | the card is the referent, and markdown can spell its id |
| Unique across       | its kind within the `Content`; slots, rows and columns within their island | the `Content`, its slots included | the document's composable cards |
| On collision        | the step refuses; `validate` refuses              | the step refuses; `validate` refuses               | a parse re-mints and warns; a placing edit rejects; a load refuses |
| Markdown round-trip | re-minted identically by a cold import; kept by a revise | spelled by the annotated read, which a revise reads back | spelled as `$id`, read back as written |

The main card is addressed as `main` and carries no `$id`.

Each section below states one handle's policy whole.

## Island-id determinism

A structural id is part of the canonical form, so it is hash input like every
other field, and byte-stability's promise: equal content → equal bytes,
*whatever the producer*: requires that equal content carry equal ids. The rule:
**an id Quillmark mints is a deterministic function of content, never drawn
from an ambient source** (RNG, wall-clock, UUID, allocation identity, session
or process state). An ambient id would make re-importing the same markdown
yield different bytes for the same document, silently breaking divergence
detection and cache keys.

The scheme is positional: `{prefix}{n}` past the highest number the value
already holds under the prefix (`ids::Minter`), so a cold import is a pure
function of its markdown and an island takes `isl-{n}`. Ids then travel with
their part across edits: deleting a slot drops that island and survivors keep
their ids, so an id is *stable within a session*, not re-derived from position.
The invariant that holds for every `Content`, checked by `Content::validate`,
is therefore **uniqueness**, not `id == isl-{index}`.

A step names the ids of what it lands, and the apply refuses one already live
in its scope. A producer minting in a step is bound by the same rule where it
wants equal edits to reach equal bytes; a live client mints its own at random,
so a live document's hash is stable from the moment it is stored rather than
equal to a cold import's. Merge repair derives each id it mints from the ids
involved ([CONTENT.md](CONTENT.md) § "Merge repair").

A markdown revise keeps the ids: `diff::align` gives the value read from the
markdown the ids of what it kept, an island equal to a stored one keeping that
island's id and a `qm-island` tag naming one keeping it outright, and every
other part mints past the highest id the value held before it, so none takes a
dropped part's id.

Re-landing a dropped part is not a mint: the delete freed the id and it travels
back with its part in the inverse, so restoring a deletion restores the bytes.

## Anchor-id identity

An anchor sits at the caller-minted end of the mint axis (§ The id handles)
because it has **no markdown codec**: it names an external referent (a comment
thread, an editor bookmark) that no content determines. The never-ambient rule
therefore cannot apply, and need not: that rule exists to keep *import* a pure
function of markdown, and import mints no anchor, so equal markdown still
imports to equal bytes.

The policy: **an anchor id is caller-supplied, unique per `Content`, opaque and
invariant while the anchor lives; the anchor is best-effort under rewrites and
read-only in markdown.**

- **Caller-supplied.** Quillmark mints no anchor id and cannot: only the
  consumer knows the referent. A client wanting generated ids mints them
  client-side and supplies them.
- **Unique per `Content`.** An `anchor` step placing an id the value or any
  slot holds elsewhere is refused, and `Content::validate` refuses a value
  holding one twice, so a stored value holds each id once. The empty id is
  refused as a degenerate handle.
- **Opaque and invariant.** The runtime never rewrites an id. Positions move
  through steps exactly; the id passes untouched. An anchor whose text is
  deleted entirely drops *whole*, named in the receipt's `droppedAnchors`, and a
  surviving anchor whose text a write rewrote is named in `movedAnchors`. A
  rewrite diffs by line, then by word within a line it pairs, so an anchor keeps
  its place wherever its text is untouched.

No markdown round-trip guarantee: `to_markdown` emits nothing for an anchor and
import mints none, so a cold export→import loses every anchor.

The annotated read (`Card::body_annotated` and `field_annotated`, WASM
`doc.annotated(addr)`, Python `doc.annotated(field)`) spells each prose anchor as
`<qm-anchor ref="ID"></qm-anchor>` at its start, and each island whose markup
does not read back as `<qm-island ref="ID"></qm-island>`
([markdown-spec.md](../references/markdown-spec.md) §6.4). It is two-way: a
revise reads both against the value at its address, and a cold import drops
both, the token reported. A range anchor reads as its start and keeps its range
through `revise`; a tag the writer kept pins its anchor's start where the tag
stands, a tag the writer dropped leaves the anchor to the diff, and a tag naming
an anchor the stored value does not hold drops, reported. An anchor in a table
cell or a footnote's note lives in its slot, so it moves with the cell wherever
its row moves.

The writer's whole-document revise keeps anchors through `Document::revise`,
which aligns cards by `$id`, `$kind` and text and revises each aligned body and
content field; an anchor on a card it cannot align, or one the diff drops,
drops, and the receipt names it.

A comment thread hangs on an anchor by its id: [COMMENTS.md](COMMENTS.md)
states that contract, the selector each anchor reads as, and the exact rule
that places a dropped anchor again.

## Card-id identity

A composable card's `$id` names the card across writes, so a whole-document
revise and a comment selector find it wherever it moved. The policy: **a card
id is a written key, unique per document, minted deterministically where none
is written, and never rewritten while its card lives.**

- **Grammar.** 1 to 64 of `[A-Za-z0-9_-]`, opening with a letter or digit
  (`card_id::is_card_id`). `$id` on the main card is a parse error.
- **Mint.** A card without a valid id takes `c` and five base32 letters of
  SHA-256 over `quillmark/card-id\n{n}`, the first `n` whose token no card
  holds (`card_id::Mint`). The sequence is a function of the document alone,
  so equal markdown parses to equal ids and the never-ambient rule of
  § Island-id determinism holds.
- **Every door settles.** A parse, a load and a placing edit each leave every
  composable card holding a distinct valid id. A parse keeps the first card
  spelling an id, re-mints a later duplicate or an id outside the grammar, and
  warns `parse::card_id_reminted` at the card. `insert_card` mints for a card
  spelling none and refuses a bad or held one (`edit::invalid_card_id`,
  `edit::duplicate_card_id`). A load refuses a bad or duplicated stored id,
  and mints for a row written before `0.134.0`.
- **Revise.** `Document::revise` pairs an incoming card spelling a stored
  card's `$id` with it, when both are of one `$kind`. Of several spelling one
  id, the one whose text most resembles the stored card's keeps it. Where the
  incoming cards spell any stored id, the writer read the ids, so a stored card
  holding an id none spells was removed and pairs with nothing. The rest
  align by text ([BINDINGS.md](BINDINGS.md) § The write surface). A card revising a stored one
  holds its id whatever it spells. An inserted card keeps a valid id no stored
  card held and mints past every stored id otherwise, so a card a revise
  removes never hands its id to a card that revise inserts. The document keeps
  no record of an id it no longer holds: a later edit may mint it again. A
  consumer keeping selectors by card id drops a removed card's when the
  receipt's `from` names that card in no entry.
- **Never rendered.** `$id` never reaches the plate JSON, as `$ext` does not.

## Schema Versioning

The schema version is tied to the **crate version at which the `Document`
wire format was last changed**: not the running crate version. The current
format was fixed in `0.135.0`, so every save writes
`quillmark/document@0.135.0`; every later patch release writes that same
value, because patches do not change the format.

The format is the *bytes*, not only the envelope: `0.115.0` left the DTO tree
untouched, dropped an island's `loss` key and retired the `island` line kind,
which is a format change because the writer emits different bytes for the same
document.

`0.124.0`, `0.129.0`, `0.131.0` and `0.134.0` share one tree over the line
form: a field item is `{type: field, key, value}`, and a document, card,
payload or payload item carrying a key its type does not name fails the load.
`0.129.0` adds `footnote` and `math` to the island vocabulary, `term` to the
line kinds and `description` to the containers. `0.131.0` adds `font_family`
to the marks and a table's `head` and spans. `0.134.0` adds the `id` payload
item. `0.135.0` keeps the payload tree and stores each body as the content
model (`DocumentV0_135_0`, `StoredContent`).

The V0_124_0 → V0_129_0 hop decodes its raw `body`, and can reject for the same
reason a load can. Only the newest DTO converts to the live `Document`.

## Adding a Schema Version

When the `Document` wire format changes again:

1. **Freeze** the current `DocumentV0_135_0` type tree: leave its struct
   /enum definitions and serde derives untouched so existing rows still parse.
   Replace any field whose type tracks the live crate (`StoredContent` is
   the one) with raw `serde_json::Value`, or the frozen tree will *write* the
   new format under the old tag.
2. **Remove** the conversions binding the old DTO to the *live* `Document`
   (`From<&Document>` and `TryFrom<… for Document>`): a frozen tree cannot
   convert to a model it predates, and step 3 supersedes them.
3. **Add** a new frozen tree `DocumentV0_NN_0` reflecting the new model,
   plus its `From<&Document>` and `TryFrom<… for Document>` conversions.
4. **Add** the `StoredDocument::V0_NN_0` variant, tagged
   `#[serde(rename = "quillmark/document@0.NN.0")]`.
5. **Write the migration**: `From<DocumentV0_135_0> for DocumentV0_NN_0` if
   the mapping cannot fail, or `TryFrom` if it can reject, as the V0_124_0 hop
   does for a body that will not decode. This is the only real labor: it
   encodes how old fields map to the new model.
6. **Extend** the reader. Each older blob migrates to the newest DTO through
   the chain of hops, every fallible arm threading `?`.

A bump that only adds vocabulary members needs only the tag: it aliases the
newest tree, and the reader matches both tags into it.

**A migration reaches the body for certain, and a field value only by its
shape.** Every other stored content sits inside an opaque payload value carrying
no schema tag. The `0.135.0` hop migrates a field value that decodes as
line-form content, but `$seed` and `$ext` are arbitrary by contract and a walk
cannot tell content there from host data that merely looks like it. So a change
to the *content* encoding has to stay readable too: the decoder reads the old
spelling wherever it meets it, and the schema bump records that the writer
moved. The reader that absorbs it is permanent, since the untagged half is
reachable by no hop.

A new frozen DTO can also reject at parse time through a custom
`Deserialize` rather than through a `TryFrom` migration: `StoredContent`
(the current `body` field's type) normalizes and validates the embedded content,
failing with a serde error before any `TryFrom` in the chain above runs.
Design a new DTO's `Deserialize` to fail the same way if it embeds
structured (non-string) data of its own. Freezing it per step 1 moves that check
into the hop.

Old and new DTOs **coexist** in `dto.rs`, so a row written by any
still-supported past version loads, unless a later reader refuses what it
holds under the same tag: a name outside a closed set (§ Content vocabularies),
a body nested past `MAX_NESTING_DEPTH`, a `$quill` the reference grammar
refuses, such as the lone major `memo@1` ([VERSIONING.md](VERSIONING.md)), or
more cards than `MAX_CARD_COUNT`, the ceiling markdown parse and card
placement both hold. Load measures no emitted size, since emission can
outgrow the source a document parsed from.
Migrations chain (`V0_124_0 → V0_129_0 → V0_135_0 → …`); only the newest DTO
converts to the live `Document`, so each migration step stays small as versions
accumulate. The cost is one frozen type tree per schema version plus one
migration function per version bump.

A legacy variant may be **retired**: its DTO tree, migration, and tests
deleted: once a product/release-history call confirms no stored population
remains in that shape. A row that later surfaces in a retired shape fails as
an unknown version, so the evidence for "no live rows" is what the whole call
turns on. Every tag from `0.81.0` through `0.116.0` is retired.

Two things that evidence is not. A release version is not a schema tag: the
tag a build stamps is the newest one its DTO carries, so one tag's population
spans every release between its bump and the next. And prose is not registry
state — whether a version was yanked is checkable against crates.io, npm, and
PyPI, and only those answer it.

Retirement is cheap to undo while the deleted tree is recoverable from
history. That is the argument for retiring when the evidence holds, and for
sourcing it from a row count rather than a version number.

## Gotchas

- The schema version is a hand-set constant (`STORAGE_V0_135_0`), **not**
  `CARGO_PKG_VERSION`: bumping it is a deliberate act tied to a model change.
- Unknown schema versions are rejected on read, never silently ignored.
- DTO type names carry version suffixes with underscores
  (`DocumentV0_124_0`); `non_camel_case_types` is allowed module-wide for this.
- No file extension is part of the storage contract: the interchange forms
  are card-yaml markdown **text** and `StoredDocument` **JSON**.

## Links

- [ARCHITECTURE.md](ARCHITECTURE.md): `Document` in the core type overview
- [markdown-spec.md](../references/markdown-spec.md): Markdown syntax and the in-memory data model
- [VERSIONING.md](VERSIONING.md): quill version resolution (a separate concern)
- `QuillValue` (`crates/core/src/value.rs` rustdoc): value type stored inside payload fields
