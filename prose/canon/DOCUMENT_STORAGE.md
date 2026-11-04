# Document Storage Serialization

> **Implementation**: `crates/core/src/document/`

## TL;DR

`Document` is the typed in-memory model of a Quillmark Markdown file, held in
one Automerge document. A document persists as the **versioned JSON
envelope**, `StoredDocument`, whose wire format is frozen per schema version:
the row is the envelope, and a `0.134.0` row reads into a new document. The
Automerge document's **bytes** are the form of a live editing session: the
revision is the heads, a write's patch is its receipt, peers move changes as
bytes and sync is Automerge's own (§ "The Automerge layout"). A session lasts
as long as its peers share one history; a reload from the row starts a new one.

## When to use it

| Form | Round-trips? | Stable for storage? |
|---|---|---|
| Markdown (`Document::to_markdown`) | No: anchors and ids drop ([CONTENT.md](CONTENT.md) § "What markdown keeps") | No: syntax evolves |
| `StoredDocument` JSON (`Document::to_stored`) | Yes: lossless but for history | Yes: the row, frozen per schema version |
| Automerge bytes (`Document::save`) | Yes: lossless, history included | No: a live session's form; its root's `schema` names the layout |

Store the JSON: it is the row a `Document` survives a process restart or a
crate upgrade as, and the form a reader holding no Automerge reads. Move the
bytes within a live editing session: `Document::load` of a `save`, and a peer
synced from it, hold every heads the saving document held, so a write at heads
read anywhere in the session resolves (`WriteOptions.heads`, [CONTENT.md](CONTENT.md)
§ "Live state and merge"). A reload from the row (`Document::from_stored`)
starts a new session: no heads read before it resolve, and peers rejoin from
the row, which every process loads to one origin, since a document's first
change is written under one actor at time 0. History is session state, not a
row's: a changelog serves live editing, and nothing in storage reads it.

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
   type tree (`DocumentV0_134_0`, `CardV0_134_0`, …). These are never changed
   once shipped, and are internal to `dto.rs`: a host reads and writes the
   JSON, and `StoredDocument` is `#[non_exhaustive]`, so a new tag breaks no
   match.
3. **Decoupled from the live model**: internal refactors of `Document` and
   its components only touch conversion code, never the wire format.
4. **Transparent API**: `Document` serializes to the envelope from its cards
   and deserializes through `#[serde(try_from)]`; callers use `serde_json`
   directly.

## The Format

The current schema (`quillmark/document@0.135.0`) carries each card's full
ordered payload: typed `$` system metadata, user fields, and YAML
comments interleaved in source order: as a single discriminated-union
item list. A composable card's `$id` is an `{type: id, value}` item after its
`$kind` (§ Card-id identity). This is what makes inline-comment preservation symmetric across
the `$`/non-`$` boundary. A field item is `{type: field, key, value}`;
`body` is the content-model value ([CONTENT.md](CONTENT.md)) embedded
structurally (a nested object, not a markdown string), and a `richtext` field
is stored as one inside its item's `value`; see Byte-stability.
A stored value names its blocks, containers and islands by place, `p{n}`
counted over its blocks, then its containers, then its islands, never by the
Automerge object ids a live session holds them under, so a row's bytes are a
function of its content and not of which actors wrote it.

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
      ],
      "nested_comments": []
    },
    "body": { "blocks": [ { "id": "p0", "kind": "para" } ], "text": "Hi" }
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
before backends see it. `nested_comments` holds the YAML comments inside a
field's value, each at its container path and position.
Load-time warnings (a parse's, plus the `conform::*` diagnostics when the
document came through the bound input path) live on the `Parsed` record, not on
`Document`, so they never reach this format; the bindings stash them on
their `Document` handle as session state and exclude them from `equals` and
the DTO alike.

### Earlier schemas

A row under `0.134.0` reads through the current tree (§ Schema Versioning). It
is the oldest tag read: a row under any earlier tag (`0.81.0` through
`0.131.0`) fails as an unknown schema version, and the last release that
reads it is the one to load and save it under a tag still read. A body
spelled in the line form, the encoding those tags held, fails the load, and
a field value in it loads as the plain value it spells, read as no content. A
composable card naming no `$kind` fails the load.

## The Automerge layout

The document's root holds `schema` (`quillmark/document@0.135.0`), `main`,
`cards` and `order`: `cards` maps each composable card's `$id` to its card,
and `order` lists the `$id`s in document order. A card holds `layout`, its
payload items' order and comments; `fields`, each field by key; `ext`; `seed`;
`nestedComments`; and `body`. A body, and each content value in a field (an
object holding a `text` string and a `blocks` array, the one test every layer
asks, `content::units::is_content_value`: a `richtext` or `text` field in its
stored form, [SCHEMAS.md](SCHEMAS.md)
§ "Content fields are stored per codec"), the field's whole value or one at
any depth in it, an `array<richtext>` element or an object's content
property, is an Automerge text object with its blocks as markers, maps keyed
`type`, `attrs` and `parents`, its marks and anchors as Automerge marks, and
its containers and islands as records named by their object ids, so it merges
as a body does and each read takes it through repair. The rest of a field, a
string as a `text` field holds before a schema-aware write, or any other JSON,
is Automerge maps, lists and scalars, merged as data, a map's key order held
in its `\u{1f}order` list, since an Automerge map holds none. `Document::load` refuses bytes whose
root holds no document in this shape, or names a schema this build does not
read (`edit::bad_live_bytes`). The root's `schema` tag names the layout
version of the live document.

Concurrent changes merge as Automerge merges them: a field, a card or a record
two writers set reads as the later in Automerge's order, text both edited
keeps both edits, and a card one writer removed while another edited it reads
as Automerge leaves the two. A whole-value write over a body or content field
diffs into its text object and never puts a new one: by the change between the
value it reads as and the value written, and where that change does not lower,
by Automerge's own diff of the text (`update_spans`), so text a concurrent
writer typed into it keeps.

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
written through the same form. `Content::write_canonical` writes those bytes as it
reads the value, building no `Value` tree; the bytes are the ones the tree
serializes to, unchanged. Sortedness is semantic *inside* the content
(mark, island and attribute order carry no meaning, so the serializer commits
to one bit pattern); insertion order is semantic *outside* it (payload item
order is source order, and matters).

**Both directions validate.** `StoredContent` checks the body's invariants
(`Content::validate`) on the way out as well as in, failing the write with a
serializer error. A store that checked only on load would accept bytes it could
not read back; every write a `Card` takes already holds a valid value, so the
outbound check is the backstop.

**A canonical value reads as itself; another moves once.** A write's decode
takes one spelling per value and refuses the rest ([CONTENT.md](CONTENT.md)
§ "Validity"), so a value a write stored re-encodes to its own bytes and its
hash holds across a read. A stored body or field value spelled otherwise reads as its canonical form, its
hash moving once, with a `content::noncanonical_read` warning naming where, so
the host stores the row again ([CONTENT.md](CONTENT.md) § "Validity").

The guarantee follows from: struct field order is fixed in the frozen
DTO tree; `Vec` fields preserve order by definition; the two disciplines
above each hold at their respective level. No whitespace normalization is
applied: the output is `serde_json`'s compact form otherwise. Bumping the
`schema` version is the only event that may change the byte layout of a
document written by the current writer.

**Stored form.** Byte-equality is over `Document` values, so it is only
as useful as the model's agreement on what a given document *is*. A
content field is stored as the canonical content object, a `text` field's
spelling `codec: "plain"` ([SCHEMAS.md](SCHEMAS.md) § "Content fields are
stored per codec").
`Quill::conform` is what holds that: it walks a document's declared
content fields through the same strict write the typed writer commits
through, so a parse-then-conform and a typed write of the same values are
byte-equal. Documents that have converged hash equal when they are
semantically equal, so `equals` and content hashing hold for content
fields by construction rather than by construction history.
Non-content-typed fields keep their authored shorthands (`qty: "3"` stays
a string until something writes it); the typed write remains their
canonicalizer.

A document that entered through the transport input path (`Document::parse`, a
stored row, an unchecked `store_field` of a string) stays as authored until it is conformed. That is
a named state, not a second stored form: it is readable, round-trippable,
and one bound load away from converging.

A `text` field stored as its literal string is the authored form: it reads as literal text, holds no anchor, and is stored as
the `codec: "plain"` object once the bound input path conforms it. Its hash moves
then, once, and the stored row needs no migration.

A comment's text is stored without the whitespace ending it, as a parse reads its
line, so a stored row holding such text loads without it and rewrites once.

**Migrated rows.** A row under an earlier tag still read migrates forward on
read (§ Earlier schemas) and is stored under the current tag once rewritten. Rewriting it
once read (read-repair) is the same path `Quill::conform` uses: a row read
through the bound input path converges to its stored form and is eligible for rewrite
under the current tag. A population converges once and then holds.

## Content vocabularies

The envelope's version-and-reject discipline covers the document's **shape**:
the schema tag, the DTO tree, the keys a `body` object carries. The content's
four discriminators are covered by their own rule: each is a **closed set**
([CONTENT.md](CONTENT.md) § "The value" lists them), and three of them hold
the reserved `x-` names beside it.

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

**A name under the reserved `x-` prefix rides as written.** A block kind, mark
type or island type that is `x-` and at least one more character, an id as a
whole (`ids::is_ext_name`), decodes into the axis's `Ext` member with its
`attrs` or `props` held as they are, and re-encodes to the same bytes, so its
hash does not move. A reader holds it without knowing it: validation is
structural only (the name, the ranges, and the JSON depth of its bag), and the
renderers read an `x-` block as a paragraph, an `x-` mark as no formatting, and
an `x-` island as nothing. Steps may set and remove one. Containers hold
no `x-` name: a container kind is a nesting rule (an item only after a list),
and a reader cannot check a path through a container whose rule it does not
know.

The consequence, and the point: **adding a built-in member to any of these
vocabularies is a storage-version event.** An older reader refuses a row
carrying a newer build's construct, and the schema tag is what lets it say
"build too old" rather than "corrupt" (§ Schema Versioning). An `x-` name is
not a member, so writing one needs no event: every reader of the tag holds it.
The tag stays an exact match per format, with no rule for reading a newer one:
a construct that has to open in an older build ships under an `x-` name, and
promoting it to a built-in is the storage event.

**Payload rides `attrs`, for every member.** A `heading`'s `level`, a `code`'s
`lang`, a `link`'s `url`, a list's `ordered` and `start` and an item's
`checked` are all `attrs` entries: `{"kind":"heading","attrs":{"level":1}}`.
The envelope keys stay siblings (`id`, `kind`/`type`, `path`, a mark's
`start`/`end`) because they belong to the object rather than to the member it
names. An empty bag is omitted, so `normalize` collapses `{}` and an absent bag
to one spelling. A foreign bag key on a built-in is refused like an unknown
envelope key (below). One spelling per name
is also what makes the canonical mark order a function of the bytes:
`MarkKind::sort_key` is the `(type, attrs)` pair the wire carries.

`font_family` carries a family's name under `attrs.name`, stored as written and
naming no font file: a backend resolves it at render. The name is not empty,
has no space at either edge, and holds no control character, bidi control or
line separator (`is_font_family_name`).

`font_family` and `link` are keyed (`MarkKind::is_keyed`): the tag is a key and
the attrs its value, so a character holds at most one family and one link.
Normalization resolves overlap in raw input once, before the same-kind union:
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
  untouched, and so do a table cell's own entry under `cell_attrs` and an
  `x-` block's or mark's `attrs`.
- **Envelopes refuse what they do not name.** The content value, and each
  block, container, mark, anchor and island object in it, slots included,
  holds only the keys its shape defines, and a built-in member's `attrs` only
  the entries the member names. A write's decode refuses any other as
  `ParseError::UnknownKey`, so a key a newer build adds fails loudly in an
  older reader rather than dropping on a read. The stored read
  (`Content::from_value_lenient`) drops it and reports the value rewritten,
  so the row's warning names it. Growing those shapes is still a schema
  event.

A `footnote` island holds its note in the slot `note`, and an `image` its `url`
and `alt` in `props`. A `math` island's props hold the equation's TeX under
`tex`, a string on one line with no edge whitespace, and `display: true` on an
equation set on its own line, absent at `false`. The markdown input paths refuse a
`tex` markdown cannot write between its dollars (an unescaped `$`, braces that
do not balance, an empty inline one) as `ParseError::Shape`; the stored read
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
storage-version event. `Content::normalize` fixes each span so it covers
only what it can (`island::table_spans`):

- Spans claim cells in reading order, so the earlier span wins.
- A span covers only cells holding no text, no mark and no anchor that no
  other span covers, inside the grid and inside its own section: the `head`
  rows or the body. The cell a comment targets stays drawn.
- A span is stored as it covers, absent at 1, and a covered cell's span keys
  are absent. A covered cell keeps its `align` and `valign`, which it draws
  again once no span covers it; markdown spells no covered cell, so
  `from_markdown(to_markdown(c))` drops them ([CONTENT.md](CONTENT.md) § "What
  markdown keeps").

A table or cell key holding a value outside its set is read as absent and
rides untouched, as an unnamed key does. Markdown writes a table as a GFM pipe
table or as HTML, whichever holds it
([markdown-spec.md](../references/markdown-spec.md) §6.8). A cell, like a
footnote's note, is one paragraph, its lines parted by `U+2028`; a stored cell
or note of several paragraphs reads joined ([CONTENT.md](CONTENT.md) § "Validity").

Five rules bound the payload:

- **Payload depth is capped at `MAX_JSON_DEPTH` (128).** Island `props` and an
  `x-` member's `attrs` are host JSON of arbitrary shape, but not arbitrary
  depth: key canonicalization, the
  content-hash key, and `serde_json::Value`'s own `Drop` each recurse one frame
  per level, so an unbounded bag overflows the stack: on wasm32, a trap that
  takes the module down rather than an error the host can catch. The cap is the
  one `serde_json::from_str` already enforces, so it refuses nothing a stored
  blob can carry. A bag is refused where the decoder reads it, before it is
  cloned into the model, and `Content::validate` restates it as
  `Invariant::JsonTooDeep`.
- **A table's grid is capped at `MAX_TABLE_CELLS` (100 000) cells**, its rows
  times its columns. The decoder refuses such a table as
  `Invariant::TableTooLarge`; markdown import drops the rows past the cap
  instead, its tables sharing one cap, warning `table:size`.
- **Nesting is capped at `MAX_NESTING_DEPTH` (12)** items and quotes on one
  path, the depth the markdown codecs and the Typst lowering share.
- **Marks and anchors are capped against the text they sit on.** A text, a
  field's or a slot's, holds at most `max_marks(len)`: 64 and 4 per
  character, far above the one a document nears, and a value at most
  `MAX_ANCHORS` (100 000) anchors. An `x-` mark over many blocks is cut at
  each, and marks of distinct attributes never union, so a few bytes could
  otherwise ask for marks times blocks of them, on wasm32 an allocation that
  traps. The decoder counts each mark once per block and line it spans and
  refuses past the cap before normalizing, as `Invariant::TooManyMarks` and
  `Invariant::TooManyAnchors` (`edit::too_many_marks`,
  `edit::too_many_anchors`), and a change refuses at the step that would
  pass it.
- **Inputs are capped at the input paths.** Markdown an import reads, a revise's
  and `importMarkdown`'s as well as a parse's, is capped at
  `MAX_MARKDOWN_SIZE` (10 MiB), refused as `ImportError::TooLarge`
  (`edit::input_too_large`). The bindings refuse a content value whose text
  and its slots' texts pass the same 10 MiB before decoding it, and a change
  of more than `MAX_CHANGE_STEPS` steps, nested ones counted
  (`edit::too_many_steps`); `CardMut::write` refuses such a change too. A
  footnote reference copies its note, so an import shares
  `MAX_NOTE_COPIES` (1 000 000) characters among every copy past each
  note's first; a reference past it stays as its `[^label]` text, warning
  `footnote:size`.

The opaque props are hash input like everything else in the canonical form, so
they are recursively key-sorted along with the rest (see Byte-stability). Also a
schema event: a change to the content object's own structure, a new top-level
key beside `text`/`blocks`/`containers`/`marks`/`anchors`/`islands`, or a
changed meaning for an existing discriminator.

## The id handles

Structural parts, anchors and composable cards each carry an id, and all are
the same handle: **opaque, unique within a scope, hash input, stable for the
session, kept through edits and never rewritten.** They differ on one axis:
**who mints one, and why only they can.** Uniqueness scope, collision response,
and markdown round-trip all follow from that.

|                     | Block, container, island, slot, row, column `id`  | Anchor `id`                                        | Card `$id`                                          |
| ------------------- | -------------------------------------------------- | -------------------------------------------------- | --------------------------------------------------- |
| Minted by           | Quillmark at import, migration and revise; the caller in a step | the caller                            | the author or caller, else Quillmark where the card is placed, under the placing writer's actor |
| Because             | content determines it: the nth part minted         | the referent is external; no content determines it | the card is the referent, and markdown can spell its id |
| Unique across       | its kind within the `Content`; slots, rows and columns within their island | the `Content`, its slots included | the document's composable cards |
| On collision        | a change that leaves one held twice refuses, and a step naming one held twice; `validate` refuses | the step refuses; `validate` refuses               | a parse re-mints and warns; a placing edit rejects; a load refuses |
| Markdown round-trip | re-minted identically by a cold import; kept by a revise | spelled by the annotated read, which a revise reads back | spelled as `$id`, read back as written |

The main card is addressed as `main` and carries no `$id`.

Each section below states one handle's policy whole.

## Island-id determinism

A structural id is part of the canonical form, so it is hash input like every
other field, and byte-stability's promise of equal bytes for equal content
holds for a cold import, a migration and a revise: equal content from them
carries equal ids. The rule: **an id Quillmark mints is a deterministic
function of the values in play and the writer's actor, never drawn from an
ambient source** (RNG, wall-clock, UUID, allocation identity, process state).
An ambient id would make re-importing the same markdown yield different bytes
for the same document, silently breaking divergence detection and cache keys.

The scheme is positional: `{prefix}{n}` past the highest number the value
already holds under the prefix (`ids::Minter`), so a cold import is a pure
function of its markdown and an island takes `isl-{n}`. Ids then travel with
their part across edits: deleting an island character drops that island and survivors keep
their ids, so an id is *stable within a session*, not re-derived from position.
The invariant that holds for every `Content`, checked by `Content::validate`,
is therefore **uniqueness**, not `id == isl-{index}`.

A step names the ids of what it lands, and the apply refuses one already live
in its scope. A live client mints none of its own: a record it puts carries a
placeholder the write fills with the record's object id, under the client's
Automerge actor, so two clients never mint one id
([CONTENT.md](CONTENT.md) § Ids). A live document's ids and Automerge bytes
therefore depend on which actors wrote it; its stored row names each record
by place, so equal content stores as equal bytes. Merge repair derives each id it mints
from the ids involved ([CONTENT.md](CONTENT.md) § "Merge repair").

A markdown revise keeps the ids of what it kept (`diff::align`), an island
equal to a stored one or named by a `qm-island` tag keeping its id, and mints
every other part past the highest id the value held, so none takes a dropped
part's id. How a revise or replace mints over a stale base, reading values
alone, is CONTENT.md § "Writing a value".

Re-landing a dropped part is not a mint: outside a document an id travels
with its part; inside one, `undo` puts the record again under a new object id.

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
  slot holds is refused, as is a move of one its text does not hold, and
  `Content::validate` refuses a value holding one twice, so a stored value
  holds each id once and a host's colliding mint fails loudly. The empty id is
  refused as a degenerate handle.
- **Opaque and invariant.** The runtime never rewrites an id. Positions move
  through steps exactly; the id passes untouched, and an anchor whose text is
  deleted entirely drops whole, named in the receipt ([CONTENT.md](CONTENT.md)
  § Changes).
- **Not in plain markdown.** A cold export and import loses every anchor
  ([CONTENT.md](CONTENT.md) § "What markdown keeps").

The annotated read (`Card::annotated`, WASM
`doc.toMarkdown(addr, { annotated: true })`, Python
`doc.to_markdown_at(field, annotated=True)`) spells each prose anchor as
`<qm-anchor ref="ID"></qm-anchor>` at its start, and each island whose markup
does not read back as `<qm-island ref="ID"></qm-island>`
([markdown-spec.md](../references/markdown-spec.md) §6.4). A revise reads them
back ([CONTENT.md](CONTENT.md) § "Writing a value"): a range anchor reads as its
start and keeps its range, a tag the writer kept pins its anchor's start where
the tag stands, a tag the writer dropped leaves the anchor to the diff, and a
tag naming an anchor the stored value does not hold drops, reported. An anchor
in a table cell or a footnote's note lives in its slot, so it moves with the
cell wherever its row moves.

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
- **Mint.** A card without a valid id takes a token, `c` and five base32
  letters of SHA-256 over `quillmark/card-id\n{n}`, the first `n` whose id no
  card holds (`card_id::Mint`). A parse, a load and a seed mint the token
  alone, positionally, so equal markdown parses to equal ids. A placement
  through a handle with an actor set (`Document::set_actor`), by
  `Unchecked::insert_card` or the writer's `add_card`, mints `{actor}-{token}`: an actor ([CONTENT.md](CONTENT.md) § Ids) is short
  enough that the id is a card id, no actor's id is a positional one
  and two actors never mint one id. A live client places each card it inserts
  under its actor and carries the `$id` the placement answers to its peers, so
  two replicas never mint two ids for one card. The mint is a function of the
  document and the writer's actor, so the never-ambient rule of § Island-id
  determinism holds, narrowed as it is there: a document's ids depend on
  which actors placed its cards. An actor's mint never reissues an id: each
  `Document` value holds, beside its cards and outside its equality and its
  stored row, the index past the last token it minted under its actor since
  its parse, load or seed or since its actor last changed, and its next mint
  starts there, so an id
  an actor minted is never minted under it again in that value's life, though
  the card holding it is removed. A positional mint keeps no such record.
- **Every input path normalizes the id.** A parse, a load and a placing edit each leave every
  composable card holding a distinct valid id. A parse keeps the first card
  spelling an id, re-mints a later duplicate or an id outside the grammar, and
  warns `parse::card_id_reminted` at the card. `insert_card` mints for a card
  spelling none, answering the id it placed, and refuses a bad or held one (`edit::invalid_card_id`,
  `edit::duplicate_card_id`). A load refuses a bad or duplicated stored id,
  and mints for a stored card naming none.
- **Revise.** `Document::revise` pairs an incoming card spelling a stored
  card's `$id` with it, when both are of one `$kind`. Of several spelling one
  id, the one whose text most resembles the stored card's keeps it. Where the
  incoming cards spell any stored id, the writer read the ids, so a stored card
  holding an id none spells was removed and pairs with nothing. The rest
  align by text ([BINDINGS.md](BINDINGS.md) § The write surface). A card revising a stored one
  holds its id whatever it spells. An inserted card keeps a valid id no stored
  card held and mints past every stored id otherwise, so a card a revise
  removes never hands its id to a card that revise inserts. Past the actor
  mint above, the document keeps no record of an id it no longer holds: a
  positional mint, or a mint under an actor in a value loaded after the id was
  freed, may mint it again. A consumer keeping selectors by card id drops a removed card's when the
  receipt's `from` names that card in no entry.
- **Never rendered.** `$id` never reaches the plate JSON, as `$ext` does not.

## Schema Versioning

The schema version is tied to the **crate version at which the `Document`
wire format was last changed**: not the running crate version. The current
format was fixed in `0.135.0`, so every save writes
`quillmark/document@0.135.0`; every later patch release writes that same
value, because patches do not change the format.

The format is the *bytes*, not only the envelope: dropping a key or retiring a
kind with the DTO tree untouched is a format change, because the writer emits
different bytes for the same document.

`0.134.0` stores each body as the content model (`DocumentV0_134_0`,
`StoredContent`): a field item is `{type: field, key, value}`, and a document,
card, payload or payload item carrying a key its type does not name fails the
load. `0.135.0` shares that tree and adds the `codec` key to a content value,
which a `0.134` reader does not decode, so a `0.134` build refuses the row as
a newer version rather than reading a `text` field as rich text. Only the
newest DTO converts to the live `Document`.

## Adding a Schema Version

When the `Document` wire format changes again:

1. **Freeze** the current `DocumentV0_134_0` type tree: leave its struct
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
5. **Write the migration**: `From<DocumentV0_134_0> for DocumentV0_NN_0` if
   the mapping cannot fail, or `TryFrom` if it can reject, as a hop that
   decodes a raw body does for one that will not decode. This is the only real labor: it
   encodes how old fields map to the new model.
6. **Extend** the reader. Each older blob migrates to the newest DTO through
   the chain of hops, every fallible arm threading `?`.

A bump that only adds vocabulary members needs only the tag: it aliases the
newest tree, and the reader matches both tags into it.

**A migration reaches the body for certain, and a field value only by its
shape.** Every other stored content sits inside an opaque payload value carrying
no schema tag. A hop can migrate a field value by its shape, but `$seed` and
`$ext` are arbitrary by contract and a walk cannot tell content there from host
data that merely looks like it. So a change to the *content* encoding keeps its
old spelling readable for as long as its tag is read, and once the tag retires
a value still in that spelling reads as no content.

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
Migrations chain (`V0_134_0 → V0_NN_0 → …`); only the newest DTO
converts to the live `Document`, so each migration step stays small as versions
accumulate. The cost is one frozen type tree per schema version plus one
migration function per version bump.

A legacy variant may be **retired**, its DTO tree, migration and tests
deleted, only in a release that marks it a break: a major release, or, while
pre-1.0, a minor carrying a `!`. Quillmark cannot count its hosts' rows, and
one tag's rows span every release between its bump and the next, so that
release's migration guide names the last release that reads the tag, where a
host loads and saves each such row. A row that later surfaces in a retired
shape fails as an unknown version. Every tag from `0.81.0` through `0.131.0`
is retired.

**What a row holds stays readable.** Between two releases that retire a tag:

- The lenient stored read (`Content::from_value_lenient`) stays: a value a
  write would refuse as non-canonical reads as its canonical form, warning
  `content::noncanonical_read`, however long ago it was stored.
- The bounds a value is held to (`MAX_TABLE_CELLS`, `MAX_NESTING_DEPTH`,
  `MAX_JSON_DEPTH`, `max_marks`, `MAX_ANCHORS`, `MAX_CARD_COUNT`) only
  loosen, since a lowered bound makes a stored row unreadable.

## Gotchas

- The schema version is a hand-set constant (`STORAGE_V0_135_0`), **not**
  `CARGO_PKG_VERSION`: bumping it is a deliberate act tied to a model change.
- Unknown schema versions are rejected on read, never silently ignored.
- DTO type names carry version suffixes with underscores
  (`DocumentV0_134_0`); `non_camel_case_types` is allowed module-wide for this.
- No file extension is part of the storage contract: the interchange forms
  are card-yaml markdown **text** and `StoredDocument` **JSON**.

## Links

- [ARCHITECTURE.md](ARCHITECTURE.md): `Document` in the core type overview
- [markdown-spec.md](../references/markdown-spec.md): Markdown syntax and the in-memory data model
- [VERSIONING.md](VERSIONING.md): quill version resolution (a separate concern)
- `QuillValue` (`crates/core/src/value.rs` rustdoc): value type stored inside payload fields
