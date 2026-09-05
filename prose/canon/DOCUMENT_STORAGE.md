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
| `DocumentValues` JSON (`reader.values()`) | The *document* does: `set_values(reader.values())` is a no-op. The values canonicalize once | **No**: a consumer projection, lossy by design |

Use `StoredDocument` JSON whenever a `Document` must survive a process
restart or a crate upgrade: database rows, caches, message payloads.

`reader.values()` is the values form, the one a consumer edits: the stored
value with content as its codec's text, sparse, and carrying neither anchors
nor `$quill` ([SCHEMAS.md](SCHEMAS.md) § "The values form"). It is an API
shape, never a row.

`Document::to_plate_json` also exists as a lossy, one-way export to
Plate-shaped backends; it is core-only (not exposed by the WASM or Python
bindings) and never a storage option.

## Design Principles

1. **Versioned envelope**: every blob carries a `schema` tag; readers
   dispatch on it and reject unknown versions.
2. **Frozen DTO per version**: each schema version has its own standalone
   type tree (`DocumentV0_92_0`, `CardV0_92_0`, …). These are never changed
   once shipped.
3. **Decoupled from the live model**: internal refactors of `Document` and
   its components only touch conversion code, never the wire format.
4. **Transparent API**: `Document` serializes through the envelope via
   `#[serde(into / try_from)]`; callers use `serde_json` directly.

## The Format

The current schema (`quillmark/document@0.112.0`) carries each card's full
ordered payload: typed `$` system metadata, user fields, and YAML
comments interleaved in source order: as a single discriminated-union
item list. This is what makes inline-comment preservation symmetric across
the `$`/non-`$` boundary. The payload shape is unchanged since `0.92.0`;
`body` is the canonical `Content` embedded structurally (a nested
object, not a markdown string); see Byte-stability.

```json
{
  "schema": "quillmark/document@0.112.0",
  "main": {
    "payload": {
      "items": [
        { "type": "quill", "value": "usaf_memo@0.1" },
        { "type": "kind",  "value": "main" },
        { "type": "ext",   "value": { "presentation": { "title": "Greeting Card" } } },
        { "type": "field", "key": "title", "value": "Hi" }
      ]
    },
    "body": { "islands": [], "lines": [ { "containers": [], "kind": "para" } ], "marks": [], "text": "Hi" }
  },
  "cards": [ ... ]
}
```

`StoredDocument` is an internally-tagged enum (`#[serde(tag = "schema")]`);
each variant carries a frozen DTO tree. Quill references are stored as
strings (parsed back via `QuillReference::from_str`). The discriminator on
payload items is `type` (not `kind`) to keep it unambiguous next to the
`$kind` metadata semantic. The full variant set is `quill | kind | id |
ext | seed | field | comment`; the `ext` and `seed` variants carry the
`$ext` / `$seed` maps verbatim and are stripped from `to_plate_json()`
before backends see it.
Load-time warnings (a parse's, plus the `conform::*` diagnostics when the
document came through the bound door) live on the `Parsed` record, not on
`Document`, so they never reach this format; the bindings stash them on
their `Document` handle as session state and exclude them from `equals` and
the DTO alike.

### Legacy schemas (V0_93_0, V0_92_0, V0_82_0, V0_81_0)

Documents written under `"schema": "quillmark/document@0.93.0"` carry the same
tree as the current one, over the content form that spelled a built-in's payload
as named siblings (`{"kind":"heading","level":1}`). The hop is the `body`
decode and nothing else: the content decoder reads that spelling wherever it
meets it, which it must, because most stored content in that spelling reaches no
migration at all — a `richtext` field rests as a content object inside an opaque
payload value, under no schema tag of its own.

Documents written before `0.93.0` carry
`"schema": "quillmark/document@0.92.0"` and store the card `body` as a
markdown string rather than the embedded canonical content. Readers accept
them and migrate forward to V0_112_0 on load; writers do not produce this
shape. The one hop that can reject is the body cold-import (see
Byte-stability).

`"schema": "quillmark/document@0.82.0"` is the same item list without
`nested_fills` or `$seed`, plus `$id`. Its tag names a shape **union**
rather than one frozen format: `$ext` entered under it unchanged, and many
release versions stamped it. The reader accepts the union, which is what
lets a row from any of those writers load.

That hop is the one **lossy** migration: `$id` is dropped. The live model
has no counterpart for it, so the alternative is refusing the row; `$id`
reached no backend, which is what makes dropping it the cheaper loss.

`"schema": "quillmark/document@0.81.0"` is the oldest tag that exists, not
just the oldest one read: `0.81.0` is where `Document` serialization begins, and
no build before it serialized a `Document` at all. Every stored blob therefore
carries one of the four tags above, and the reader set is complete. Its shape
is pre-unification: a separate `sentinel` beside a `frontmatter` item list. It
carries neither `$id` nor `$ext`, so its hop to V0_82_0 is lossless.

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
**canonical content form** (`CanonicalContent` in `dto.rs`): byte-identical
to `rt.to_canonical_json()` and independent of `preserve_order`, even in a
consumer crate graph that lacks the feature. Sortedness is semantic
*inside* the content (mark/island/attribute order carries no meaning, so the
serializer commits to one bit pattern); insertion order is semantic
*outside* it (payload item order is source order, and matters).

**Both directions validate.** `CanonicalContent` checks the body's
invariants on the way out as well as in, failing the write with a serializer
error. The token a body rests on (`Normalized`) states that
`Content::normalize` has run, which is weaker than validity: `normalize`
repairs where `validate` rejects, and `Card::overwrite_body` takes a
caller's content on that token alone. A store that checked only on load
would accept bytes it could not read back.

The guarantee follows from: struct field order is fixed in the frozen
DTO tree; `Vec` fields preserve order by definition; the two disciplines
above each hold at their respective level. No whitespace normalization is
applied: the output is `serde_json`'s compact form otherwise. Bumping the
`schema` version is the only event that may change the byte layout of a
document written by the current writer.

**Resting form.** Byte-equality is over `Document` values, so it is only
as useful as the model's agreement on what a given document *is*. A
content field has one resting form per codec: a `richtext` field rests as
the canonical content object, a `plaintext` field as its literal string
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

**Migrated rows: a conditional caveat.** The guarantee above is unconditional
for a document the current writer serializes directly. A row still carrying
a legacy schema tag migrates forward on read, and the `0.92.0` hop
cold-imports the stored markdown `body` string through the same
Markdown → richtext path `Document::parse` uses. Byte-stability of
*that* row across a crate upgrade is therefore conditional on
`pulldown-cmark` parsing the body the same way: a forced parser or security
bump can move the migrated bytes even though the schema tag does not
change. Two ways to manage this:

- **Read-repair.** Rewrite a row under its current schema tag once it has
  been read and migrated, so the content form: not the legacy markdown
  string: becomes its byte-stable resting state. This is the same lane
  `Quill::conform` uses, the second named driver of read-repair byte
  movement: a row read through the bound door converges to canonical rest
  and is eligible for rewrite under its current tag. Neither is a
  schema-version event; a population converges once and then holds.
- **Accept the movement.** For rows left un-repaired, treat a forced
  parser/security bump as either a schema-version event (if a hard
  guarantee is required) or an accepted, logged hash movement on
  not-yet-migrated rows.

## Open vocabularies

The envelope's version-and-reject discipline covers the document's **shape**:
the schema tag, the DTO tree, the keys a `body` object carries. It does *not*
cover the content's **vocabularies**. Every discriminator inside a `body` is an
open set: a mark `type`, an island `type`, a line `kind`, a container name, and
an island's `loss` class this build does not recognize each round-trip
byte-identically and project as their nearest safe neighbor.

**Two mechanisms, split by payload.** A block axis carries one, so its built-ins
decode eagerly into typed fields (`LineKind::Heading { level }`) and an
unrecognized member needs a sibling `Unknown` arm to hold its payload. The two
island axes carry none, so the wire string *is* the stored value and the closed
set is a **view** over it (`KnownIslandType::parse`, `Loss::fidelity`). The
split decides which axes need the reserved-name rule below: a carrier axis has
two in-memory arms per wire object and must keep one from impersonating the
other, a view axis has one. On the wire both spell a payload alike.

| Axis | Carrier | Unknown value projects as |
|---|---|---|
| Mark `type` | `Unknown { tag, attrs }` | no delimiters (the text renders bare) |
| Line `kind` | `Unknown { tag, attrs }` | a paragraph |
| Container | `Unknown { tag, attrs }` | transparent: its lines render at the enclosing level |
| Island `type` | the `String` itself | a placeholder comment; the props survive in storage |
| Island `loss` | the `String` itself | `Fidelity::Unrepresentable`, via `Loss::fidelity`: never a claim of fidelity on a class this build cannot read |

The consequence, and the point: **adding a construct to any of these
vocabularies is not a schema-version event.** An older reader degrades a future
callout to a plain paragraph rather than refusing the document, and a reader
that does understand it sees it whole, because the tag and attrs round-tripped
untouched. Openness is the same on all five axes: the block axes are open on the
mark axis' terms, not one step behind it, and both island axes carry their raw
string rather than rewriting it: a reader that merely opens a document must not
move its content hash (§ Byte-stability).

**Container identity is path plus contiguity, and `instance` is what completes
it.** Two adjacent lines sit in the same container iff their whole container
path is equal, so without a discriminator two adjacent runs of one shape read as
one: `[Quote], [Quote]` would be a single two-paragraph quote, and two one-item
lists a single item with an unnumbered continuation paragraph. `instance` is the
field that breaks that tie, on every container arm including `Unknown`, which is
why the round-trip above is a *total* promise rather than one holding up to an
adjacency quotient. `Content::normalize` canonicalizes it to `0`, flipping to
`1` only where the adjacent preceding sibling run would otherwise weld, so a
document needing no discriminator carries none and its stored bytes are the
bytes it had before the field existed (§ Byte-stability). That is why the key
was additive within `@0.93.0` rather than a schema-version event: no blob
written before it moves. The cost is in the other direction, and only for a
document that spends the key — a build predating the field ignores it and reads
the two runs welded, so a row written here and re-saved there loses the
boundary. A container field added later inherits that trade, since a reader is
frozen at the vocabulary it shipped with. The Markdown
projection spells the same boundary with the idiom CommonMark already reads: a
change of bullet char (`-`/`+`) or of ordered delimiter (`.`/`)`) for lists, the
blank line for quotes. An `Unknown` container has no Markdown syntax at all, so
that one boundary lives in storage only — the same place its `tag` and `attrs`
already live.

The omission is storage's alone. A binding read is also a binding write input,
so the seam encoder (`serial::to_seam_value`) spells the field on every
container, which is what lets a binding's read type require it
([BINDINGS.md](BINDINGS.md)). The two forms decode to one value; only storage
buys byte-stability with the omission.

**A new container owes its projections a separator.** `instance` makes the
boundary storable; it does not make it *writable*. A container whose Markdown
spelling cannot separate two adjacent instances has a boundary in storage that
the projection drops, so `from_markdown(to_markdown(rt)) == rt` fails on
re-import — the model reads one container where it stored two. Spell the
separator when the container is specced, not after: a container defined on
CommonMark §5.1's terms (a per-line marker, blank-line terminated, as
`Container::Quote` is) gets one for free, because the blank line already ends
it. One defined on §5.2/§5.3's terms (a list, which a blank line leaves open)
does not, and needs the marker alternation `Container::ListItem` uses. The same
obligation falls on the Typst lowering: two adjacent instances must not lower
into one another's markup.

Unknown *keys* survive in designated carriers only, and the boundary is worth
stating because it is not the discriminator boundary:

- **Opaque carriers keep what they hold.** Unknown `attrs` on all three block
  axes, island `props`, a table island's top-level props, and a table **cell's**
  own keys all round-trip untouched.
- **Envelopes drop what they do not name, by design.** An island, line, mark, or
  container object is decoded into a struct and re-minted from its fields, so an
  unrecognized sibling key beside `id`/`kind`/`start` does not survive. Growing
  those shapes is a schema event; that is what the envelope's version-and-reject
  discipline is for.

A table cell sits on the first side because it never became a struct, and it
earns the place: cells are where the `table` type is likeliest to grow
(`colspan`, `rowspan`, a per-cell style handle). Canonicalization therefore
rewrites a cell's `text` and `marks` in place rather than minting a fresh
`{text, marks}` object.

Three rules bound the openness:

- **Payload depth is capped at `MAX_JSON_DEPTH` (128).** An opaque bag is host
  JSON of arbitrary shape, but not arbitrary depth: key canonicalization, the
  content-hash key, and `serde_json::Value`'s own `Drop` each recurse one frame
  per level, so an unbounded bag overflows the stack: on wasm32, a trap that
  takes the module down rather than an error the host can catch. The cap is the
  one `serde_json::from_str` already enforces, so it refuses nothing a stored
  blob can carry. It is stated on its own because the `Value` lane: the
  host-authored one, which `overwrite` reaches: is not parsed from a string, so
  nothing else bounds it. A bag is refused where the decoder reads it off the
  wire, before it is cloned into the model; `Content::validate` restates it as
  `Invariant::JsonTooDeep` for content that never went through a decoder. This
  is `Invariant::NestingTooDeep`'s container cap on the payload axis.

- **Payload rides `attrs`, for every member.** A `heading`'s `level`, a `code`'s
  `lang`, a `link`'s `url`, a `list_item`'s shape and an unknown's whole opaque
  bag are all `attrs` entries: `{"kind":"heading","attrs":{"level":1}}`. The
  envelope keys stay siblings — `kind`/`type`/`container`, `containers`,
  `continues`, a mark's `start`/`end`, a container's `instance` — because they
  belong to the object rather than to the member it names. An empty bag is
  omitted, on the rule `continues: false` already follows: presence is a pure
  function of the value, so `normalize` collapses `{}` and an absent bag to one
  spelling.

  One spelling per name is what makes promotion cost nothing (see Promoting a
  vocabulary member). It is also what a *new* construct needs: its payload is
  in the place a reader that predates it already looks.

- **No reserved name reuse.** An unknown may not take a built-in's name
  (`heading`, `quote`, `link`, …): it would serialize as the built-in and parse
  back as one, dropping any attrs the built-in's own payload does not name.
  `Content::validate` rejects this (`Invariant::ReservedUnknownTag` /
  `ReservedUnknownLineKind` / `ReservedUnknownContainer`) for an in-process Rust
  construction, which is now the only place it can arise: with one spelling per
  name there is no second wire form to collide with a built-in's, so no lane has
  a collision to reject. The rule is the three carrier axes' alone, and the
  `Unknown` arm is what makes it necessary: a view axis has one value per wire
  string. It is construction hygiene, and what keeps
  `from_canonical_json(to_canonical_json(rt)) == rt` unconditional for a
  validate-clean value.

  A foreign bag on a built-in is a different thing and is permitted: on a
  member carrying no payload it drops unread, on one that does it is read past.
  That is "the carrier preserves unknown tags, not unknown payloads on known
  tags" at attrs-key granularity.

The lanes still split, on what is now the only ambiguous shape — a built-in's
payload spelled as a **named sibling**, which is how every release through
`@0.93.0` wrote it:

  - **The storage lane reads it, and must.** `serial::payload` falls back to the
    sibling when the bag is absent — an *empty* bag being an absent one here as
    everywhere. Most stored content cannot be migrated: a `richtext` field rests
    as a content object inside an opaque payload value, under no schema tag, so
    the decoder's tolerance is the only thing that opens it. It is also not
    retirable on tag evidence, no tag naming those rows. Read-repair converges
    the population; cold rows keep it alive.
  - **The authored lane rejects it.** A legacy payload sibling is a shape error
    (`serial::line_kind_from_authored_value` and its two twins for the op wire,
    `serial::from_authored_value` for a whole content). A host writing it now
    holds a stale copy of the encoding, and where a bag sits beside it the
    sibling it meant is not the one the decoder reads. Reads that hand back
    stored content (`exportMarkdown`, `rebase`) are storage-lane, not this one.

  Both read one frozen table (`serial::legacy_*_keys`): the fallback reads
  exactly the keys the rejection refuses, so a key a later promotion adds is in
  neither and the two spellings stay split by release rather than by which names
  a build knows.

  The authored lane is the whole-content doors — `overwrite`, `install`,
  `CardInput.body`, the op wire. A **typed field write** is not among them:
  `document::canonical_richtext_value` decodes storage-lane on purpose, sharing
  that entry point with a quill's schema literals, which are read out of a
  `Quill.yaml` that may predate the release. It re-canonicalizes what it
  decodes, so the value rests in the current spelling either way.

The same split governs an unreadable **table-cell mark**. Storage skips it:
`serial::parse_cell` is lenient, and normalization makes the skip permanent. The
authored lane refuses it, because a host's malformed mark vanishing with no
signal is the silent corruption the split exists to catch.

It governs a **block-only island's placement** too. Markdown writes a `table` as
a block (`KnownIslandType::block_only`, which an inline `image` is not), so a
slot sharing its line with prose has no inline spelling: written there it lands
as pipes inside the paragraph, which re-imports as prose with the island gone.
The authored lanes refuse the placement — `ApplyError::BlockIslandNotAlone` from
`IslandOp::Insert` and from a `Set` that retypes an inline island, a shape error
from `serial::from_authored_value` — so a host learns at the write. Storage takes
what it holds and the projection settles it: `export::to_markdown` breaks the
line around the slot, so the prose on each side becomes its own block and the
island keeps the line its markup needs.

The opaque attrs are hash input like everything else in the canonical form, so
they are recursively key-sorted along with the rest (see Byte-stability). What
*does* remain a schema event is a change to the content object's own structure:
a new top-level key beside `text`/`lines`/`marks`/`islands`, or a changed
meaning for an existing discriminator.

### What openness buys a consumer

"Project an unknown as its nearest safe neighbor" serves one consumer posture of
two:

- **Render-only.** Degrade and move on. An unknown line is a paragraph, an
  unknown container is absent; nothing is written back, so nothing is lost.
- **Read-modify-write.** An editor lowers a whole-field diff, restating every
  line's `kind` and `containers` whenever any of them changed. A construct its
  tree cannot hold is gone on the next keystroke: the document opens intact and
  saves mangled. Such a consumer carries unknowns *inertly* instead: a carrier
  node per axis that renders as the nearest safe neighbor and re-emits the tag
  and `attrs` verbatim.

Classifying known-vs-unknown is therefore the read-modify-write consumer's
problem, and the boundary answers it: `isUnknownLine` / `isUnknownContainer` /
`isUnknownMark` / `isUnknownIsland` on the WASM surface, `KnownIslandType::parse`
and the `RESERVED_*` lists in Rust. A consumer that re-derives the built-in list
has re-coupled to a closed set, and misreads the first release that adds a
built-in.

The bound: **the carrier preserves unknown tags, not unknown payloads on known
tags.** A future `kind: "footnote"` carrying an `attrs.ref` loses `ref` at any
consumer that predates it, predicates or no. What the first rule above (*payload
rides `attrs`, for every member*) buys is the other half: a future *name* keeps
its whole payload, because the place that payload sits does not depend on
knowing the name.

## Promoting a vocabulary member

Adding an unknown is not a schema event, and neither is the reverse trip: a
later release **promoting** a tag to a built-in, which is what the open set
exists for. Promotion is **not an encoding change**: the bytes a build wrote
while `callout` was unknown are exactly the bytes the build that knows it reads,
because both spell the payload in `attrs`. Add the arm, add the name to the
list, and the same decoder that carried the tag opaque now types it.

Two things do still move, and only for a *mark*:

**Promoting a mark into the formatting class changes stored meaning**, since
adjacent runs that round-tripped as two marks begin to union
(`MarkKind::is_formatting`). It is a canonical-byte event, and takes the
read-repair-or-accept-the-movement treatment that § Byte-stability sets out for
migrated rows.

**`RESERVED_*` growth rejects a previously-valid in-process construction.** A
Rust embedder still building `Unknown { tag: "callout" }` after the promotion
gets `ReservedUnknownLineKind` from `validate`. Nothing on the wire changes: a
host sending `{"kind":"callout","attrs":{…}}` is writing the promoted spelling
already, either side of the release. So this is a Rust API note, not a wire
break.

The sort order is not among them. The canonical tie-break after `(start, end)`
is `MarkKind::sort_key` — the `(type, attrs)` pair the wire carries, read back
off the value — so a build that knows a member and a build that reads it as
`Unknown` compute the same key from the same bytes, whichever members each
knows. The block axes sort by nothing.

The island axes have no gap here: `props` is the payload carrier for known and
unknown types alike, and `loss` carries no payload.

## The two id handles

Islands and anchors each carry an id, and both are the same handle: **opaque,
unique within a scope, hash input, stable for the session, rebased through edits
and never rewritten.** They differ on one axis: **who mints one, and why only
they can.** Uniqueness scope, collision response, and markdown round-trip all
follow from that.

|                     | Island `id`                                        | Anchor `id`                                        |
| ------------------- | -------------------------------------------------- | -------------------------------------------------- |
| Minted by           | the engine at import, the caller on an insert op   | the caller                                         |
| Because             | content determines it: the nth island minted      | the referent is external; no content determines it |
| Unique across       | the `Content`'s islands                            | the `Content`'s prose marks                        |
| Required            | yes: `insert` rejects the empty id                 | yes: the empty id is rejected                     |
| On collision        | `insert` rejects; `validate` scans                 | `add` rejects                                      |
| Markdown round-trip | re-minted identically                              | lost: export emits none, import mints none        |

A card is addressed by index and carries no id handle. A consumer needing a durable per-card key carries one in `$ext` under its own namespace ([CARDS.md](CARDS.md) § Out-of-band Metadata), which the engine round-trips and never interprets.

Each section below states one handle's policy whole.

## Island-id determinism

An island's `id` is part of the canonical form (`{id, type, props, loss}`),
so it is hash input like every other field, and byte-stability's promise:
equal content → equal bytes, *whatever the producer*: requires that equal
islands carry equal ids. The rule: **an id is a deterministic function of
content, never drawn from an ambient source** (RNG, wall-clock, UUID,
allocation identity, session or process state). An ambient id would make
re-importing the same markdown yield different bytes for the same document,
silently breaking divergence detection and cache keys.

The normative scheme is the importer's positional `isl-{n}`: the nth island
minted takes `isl-{n-1}` (`mint_island`), so cold import is a pure function
of its markdown; export drops ids and re-import re-mints the same sequence.
Ids then travel with their island across edits: deleting a slot drops that
island and survivors keep their ids, so an id is *stable within a session*,
not re-derived from position. The invariant that holds for every `Content`,
checked by `Content::validate`, is therefore **uniqueness**
(`Invariant::IslandIdCollision`), not `id == isl-{index}`: after an edit
`isl-1` may legitimately sit at index 0.

The id stays in the hash input, so "canonical bytes == hash input" holds exact: no id-stripping, no separate hash form.

The importer is not the only minter: `IslandOp::Insert` carries the id of the island it lands, and the apply refuses the empty id and one already live in the field (`ApplyError::EmptyIslandId`, `ApplyError::IslandIdCollision`), since `IslandOp::Set` addresses by it. That producer is bound by the same never-ambient rule: continue the positional sequence past the field's highest `isl-{n}`, never a UUID, a clock reading, or a session counter. Import purity is unaffected either way, since export drops island ids and no edit-minted id reaches markdown; the rule holds at edit time so two producers making the same edit reach the same bytes.

The continuation rule mints a *new* island. Re-landing a dropped one is not a mint: the delete freed the id and it travels back with its island, so restoring a deletion restores the bytes. A pasted copy of a live island is new and mints fresh. Swapping the two cases is `IslandIdCollision` on the paste, or a silently renamed island on the restore.

The two compose because minting reads the *live* ids: deleting the highest island frees its number for the next mint, and restoring that delete would then collide. Linear undo never reaches that state, since a mint made after a delete is undone before it. A producer with non-linear history (selective undo, a merge) owns the case, and the apply refuses it rather than aliasing two islands.

## Anchor-id identity

An anchor (`MarkKind::Anchor { id }`) sits at the caller-minted end of the mint
axis (§ The two id handles) because it has **no markdown projection**: it
names an external referent (a comment thread, an editor bookmark) that no
content determines. The never-ambient rule therefore cannot apply, and need not:
that rule exists to keep *import* a pure function of markdown, and import mints
no anchor, so equal markdown still imports to equal bytes.

The policy: **an anchor id is caller-supplied, unique per `Content`, opaque and
invariant while the mark lives; the mark is best-effort under edits and absent
from markdown.**

- **Caller-supplied.** The engine mints no anchor id and cannot: only the
  consumer knows the referent. Each consumer (editor, MCP writer) supplies its
  own; the runtime persists it verbatim. Engine-minting is a non-goal by
  design: a counter is history-dependent (the same final content reached two
  ways carries different ids: the ambient source the twin rule forbids) and
  forfeits referent-convergence (two `add`s for one thread would mint two
  handles). A client wanting generated ids mints them client-side and supplies
  them; the auto-fallback middle ground (engine mints when the caller omits) is
  a non-goal too.
- **Unique per `Content`.** `add` *rejects* a collision: not replace (which
  silently retargets a live thread) nor coexist (already incoherent: an anchor
  is *the* handle `RemoveAnchor { id }` retains-out, so a shared id makes
  removing one destroy both). The empty id is rejected as a degenerate handle.
  Enforced at op time in the `Add` arm and as a `Content::validate` invariant
  (`Invariant::AnchorIdCollision`, the uniqueness scan the island check shares).
  Scope is prose `marks`: an anchor inside an island cell is unreachable by
  `RemoveAnchor` (it scans prose marks only), so it sits outside the op surface
  and outside this uniqueness check: explicit partial enforcement, not silent
  half-enforcement.
- **Opaque and invariant.** The runtime never rewrites an id. Positions rebase
  through splices (`map_pos`); the id passes untouched. A mark whose text is
  deleted, or moved-and-rewritten in one round, drops *whole*: never
  partially, never re-id'd (the documented diff-rebase residual).

No markdown round-trip guarantee: export emits nothing for an anchor and import
mints none, so a cold export→import loses every anchor. Anchors are edit-lane
infrastructure: they survive only through diff-rebase (`revise` / `rebase`).
Non-rendering is a property of review-time metadata, not a gap; a future render
projection (proof annotations, PDF destinations) would render the referent or a
position, never the id, so this policy holds either way.

## Schema Versioning

The schema version is tied to the **crate version at which the `Document`
wire format was last changed**: not the running crate version. The
current format was fixed in `0.112.0`, so the version tag is
`quillmark/document@0.112.0`; every later patch release writes that same
value, because patches do not change the format.

The format is the *bytes*, not only the envelope: `0.112.0` left the DTO tree
untouched and moved every built-in's payload into `attrs` inside the `body`
(§ Open vocabularies), which is a format change because the writer emits
different bytes for the same document.

`0.92.0` is a unified payload-item list (typed `$` entries living alongside
user fields and comments in a single `Vec<PayloadItem>`), a per-field
`nested_fills` list so `!must_fill` markers nested inside a field value
survive a storage round-trip (the JSON `value` projection is fill-free), and
the `seed` payload-item variant (the `$seed` per-card-kind overlay map).
`0.93.0` leaves the payload model unchanged and instead embeds the card
`body` as the **canonical content**: structurally, as a nested object, not a
markdown string (see Byte-stability). `0.112.0` leaves the tree unchanged in
turn and moves every built-in's payload into `attrs` inside that content.

The V0_92_0 hop cold-imports the stored markdown `body` string through the same
Markdown → richtext path `Document::parse` uses, so a pathologically
over-nested legacy body is rejected (`StorageError::Malformed`) rather than
silently truncated. The V0_93_0 hop decodes its raw `body`, and can reject for
the same reason a load can.

`0.81.0` is the oldest tag read, and migrations chain
(`V0_81_0 → V0_82_0 → V0_92_0 → V0_112_0`, with `V0_93_0 → V0_112_0` entering
directly); only the newest DTO converts to the live `Document`. The V0_92_0
chain lands on V0_112_0 rather than passing through V0_93_0: a cold import
yields the live content, and re-spelling it *back* to `@0.93.0` only to read it
forward again would need an encoder for that form, which this crate lacks. The
`V0_81_0` hop is structural, the `V0_82_0` hop is lossy in exactly one place —
`$id` is dropped (see "Legacy schemas").

## Adding a Schema Version

When the `Document` wire format changes again:

1. **Freeze** the current `DocumentV0_112_0` type tree: leave its struct
   /enum definitions and serde derives untouched so existing rows still parse.
   Replace any field whose type tracks the live crate — `CanonicalContent` is
   the one — with raw `serde_json::Value`, or the frozen tree will *write* the
   new format under the old tag.
2. **Remove** the conversions binding the old DTO to the *live* `Document`
   (`From<&Document>` and `TryFrom<… for Document>`): a frozen tree cannot
   convert to a model it predates, and step 3 supersedes them.
3. **Add** a new frozen tree `DocumentV0_NN_0` reflecting the new model,
   plus its `From<&Document>` and `TryFrom<… for Document>` conversions.
4. **Add** the `StoredDocument::V0_NN_0` variant, tagged
   `#[serde(rename = "quillmark/document@0.NN.0")]`.
5. **Write the migration**: `From<DocumentV0_112_0> for DocumentV0_NN_0` if
   the mapping cannot fail (a purely structural rename/restructure), or
   `TryFrom<DocumentV0_112_0> for DocumentV0_NN_0` if it can reject, as the
   V0_92_0 cold-import does for an over-nested legacy body and the V0_93_0 hop
   does for a body that will not decode. This is the only real labor: it
   encodes how old fields map to the new model (renames, restructures,
   defaults for new fields, and: for a `TryFrom` hop: which malformed inputs
   get rejected).
6. **Extend** the reader. Each older blob migrates to the newest DTO, hopping
   where a hop exists and entering directly where re-spelling backwards would
   need an encoder the crate lacks. Every arm below the newest is
   fallible today, so every one threads `?`:
   ```rust
   match stored {
       StoredDocument::V0_NN_0(p) => Document::try_from(p),
       StoredDocument::V0_112_0(p) => Document::try_from(DocumentV0_NN_0::try_from(p)?),
       StoredDocument::V0_93_0(p) => Document::try_from(DocumentV0_NN_0::try_from(
           DocumentV0_112_0::try_from(p)?,
       )?),
       StoredDocument::V0_92_0(p) => Document::try_from(DocumentV0_NN_0::try_from(
           DocumentV0_112_0::try_from(p)?,
       )?),
   }
   ```

**A migration reaches the `body` and nothing else.** Every other stored content
— a `richtext` or `plaintext` field, a `$seed` overlay — sits inside an opaque
payload value carrying no schema tag, and a walk cannot tell one from host data
that merely looks like it (`$ext` is arbitrary by contract). So a change to the
*content* encoding is not migratable, and has to be readable instead: the
decoder reads the old spelling wherever it meets it, and the schema bump records
that the writer moved. Design a content change so a decoder can absorb it, or it
cannot ship.

A new frozen DTO can also reject at parse time through a custom
`Deserialize` rather than through a `TryFrom` migration: `CanonicalContent`
(the current `body` field's type) normalizes and validates the embedded content,
failing with a serde error before any `TryFrom` in the chain above runs.
Design a new DTO's `Deserialize` to fail the same way if it embeds
structured (non-string) data of its own. Freezing it per step 1 moves that check
into the hop.

Old and new DTOs **coexist** in `dto.rs`, so a row written by any
still-supported past version always loads. Migrations chain
(`V0_92_0 → V0_112_0 → V0_NN_0 → …`); only the newest DTO converts to the live
`Document`, so each migration step stays small as versions accumulate. The
cost is one frozen type tree per schema version plus one migration function
per version bump.

A legacy variant may be **retired**: its DTO tree, migration, and tests
deleted: once a product/release-history call confirms no stored population
remains in that shape. A row that later surfaces in a retired shape fails as
an unknown version, so the evidence for "no live rows" is what the whole call
turns on.

Two things that evidence is not. A release version is not a schema tag: the
tag a build stamps is the newest one its DTO carries, so one tag's population
spans every release between its bump and the next. And prose is not registry
state — whether a version was yanked is checkable against crates.io, npm, and
PyPI, and only those answer it.

Retirement is cheap to undo while the deleted tree is recoverable from
history. That is the argument for retiring when the evidence holds, and for
sourcing it from a row count rather than a version number.

## Gotchas

- The schema version is a hand-set constant (`STORAGE_V0_112_0`), **not**
  `CARGO_PKG_VERSION`: bumping it is a deliberate act tied to a model change.
- Unknown schema versions are rejected on read, never silently ignored.
- DTO type names carry version suffixes with underscores
  (`DocumentV0_92_0`); `non_camel_case_types` is allowed module-wide for this.
- No file extension is part of the storage contract: the interchange forms
  are card-yaml markdown **text** and `StoredDocument` **JSON**.

## Links

- [ARCHITECTURE.md](ARCHITECTURE.md): `Document` in the core type overview
- [markdown-spec.md](../references/markdown-spec.md): Markdown syntax and the in-memory data model
- [VERSIONING.md](VERSIONING.md): quill version resolution (a separate concern)
- `QuillValue` (`crates/core/src/value.rs` rustdoc): value type stored inside payload fields
