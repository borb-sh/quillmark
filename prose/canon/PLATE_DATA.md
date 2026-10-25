# Plate Data Injection

> **Implementation**: `crates/backends/typst/src/`

## TL;DR

Plates get document data through a backend-injected virtual Typst package, not a template engine. Data flows in two stages: `Quill::compile_data()` produces validated, blank-filled JSON in which content fields are canonical `Content` objects; `Backend::open()` generates the helper's `lib.typ`, walking each value beside its transform-schema node to lower it, no per-field markdown re-parse.

One rule governs the lowering, at every depth: **a declared type means the same
thing wherever it is declared, and every type lowers to its native Typst value
unless it has a canonical rendering.** Only the content types have one — the
authored text — so only they lower to content; a date lowers to a native
`datetime`, because every rendering of `2026-01-02` is a typographic decision
the plate owns. Backend-generated *ink* is reached by address instead
(`qm.ink(row, "org")`), from one table born in generated source, which is what
makes it laundering-proof.

## Overview

1. `Quill::compile_data()` coerces, validates, normalizes, and **blank-fills** the
   root-block fields, and each composable card's fields against its `card_kind`
   schema, into a plain JSON object: every absent schema field resolves to its
   authored value, else the schema `default:`, else the field's blank.
   Content fields cross as canonical `Content` objects (coercion imports an
   authored markdown string to the `Content` and re-canonicalizes an
   editor-supplied one). An unanswered field renders: an absent or
   present-null field blank-fills. Only a malformed value: one that won't coerce or
   validate to its type: errors. A `today` date renders as the render date the
   host supplied ([SCHEMAS.md](SCHEMAS.md#the-render-date-today)).
2. `Backend::open()` receives that JSON and generates the helper package. `Codegen::emit_value` walks the data beside the schema node declaring it: a content node lowers via `emit::emit_content`, a date node to `datetime(..)`, an `array` recurses on `items`, an `object` on `properties`, everything else to a value literal. There is no markdown-string transform, and no name table: the walk is the inverse of the one `field_to_schema` built the node with, so it cannot be shallower than the schema is. It also takes the render date, which `datetime.today()` returns: a render reads no clock.

### Data Shape

- Document-level metadata uses `$`-prefixed keys: `$quill` (quill ref string), `$body` (root prose body, a canonical `Content` object, present when the main enables a body), `$cards` (array of card objects)
- Each card object carries its user fields flat, its `$kind` discriminator, and a `$body` (card prose body, a content object) when the card's kind enables a body
- **`$`-metadata is present exactly where the schema defines it** ("absent on
  undefined"). Which definition gates the key splits the rule:
  - `$kind` is *document-defined*: every card authors one, whether or not the
    quill declares it.
  - `$body` is *schema-defined*: present iff a declared kind enables a body,
    absent for a body-disabled or unknown kind. A present `$body` is always a
    content object, never a raw object needing a type check.

  Absence is the signal. Read `$`-metadata with a total accessor:
  `card.at("$kind", default: none)`, `card.at("$body", default: "")`: never a
  bare `card.$body`
- A card whose `$kind` the quill does not declare keeps its place in `$cards`, fields verbatim and uncoerced, so a plate's `$cards` loop falls through on a kind it does not know ([SCHEMAS.md](SCHEMAS.md#what-blocks-a-render))
- `data`, each card, and each typed dictionary carry `$path`, their address prefix (`""` on `data`, `refs.0.` on a row), for [`qm.ink`](#the-ink-table) and [`qm.roster`](#a-matrix-field) to address by. A matrix's own dictionary carries none, so its keys are exactly the members it holds
- `$path` is a visible key: dictionary equality (and so `contains` and `dedup`), `keys()` and spreading see it. A data key spelling `$path` is dropped; a dictionary the schema does not type passes through verbatim, keys included
- User payload fields sit flat at the root next to the `$` keys; a field name matches `[A-Za-z_][A-Za-z0-9_]*` (a declared one `[a-z][a-z0-9_]*`, [QUILL.md](QUILL.md)) and therefore never collides with `$` metadata

#### A `matrix` field

A matrix reaches the plate as the set it holds: each held member's id mapped to
its columns, roster members in roster order, then each item an open matrix's
document adds, sorted by id. An unheld member is absent. Order is the one place
a matrix departs from the canonical emission below: dict keys otherwise sort,
so the transform schema carries the roster as `quillmark:roster`, `[id, title]`
pairs in declaration order, and the codegen emits held roster members in it,
the added items trailing in the order every other dictionary takes. The order is
a property of the schema and the ids, never of the document's key order, so
equal data still produces byte-equal source.

```json
"qualifications": {
  "flight_cc": { "detail": {…} },
  "wing_ig":   { "title": "Wing IG", "detail": {…} }
}
```

- A member's keys are the field's declared columns, each at its declared type,
  and on an added item its `title`, a one-line `text` cell.
- A plate branches on presence first (`"flight_cc" in qm.data.qualifications`),
  then reads a held member's columns without a guard.
- Each member carries its own `$path`; the matrix, holding only members, carries
  none, so its keys are exactly the members it holds.

The vocabulary the page prints, held or not, is the roster, and it reaches the
plate once, in the generated address tables. **`qm.roster(dict, key)`** reads it:
every roster member in roster order, then each added item, as rows
`(id, title, held, value, path)`. `title` is the roster's, or an added item's
own; `value` is the member's dictionary, `none` where unheld; `path` is the
member's address. `dict` is the dictionary declaring the matrix, whose `$path`
the address starts from, as `qm.ink(dict, key)`'s does:

```typst
#for row in qm.roster(qm.data, "qualifications") {
  qm.region(row.path)[#(if row.held [☒] else [☐]) #row.title]
  if row.held [ — #qm.ink(row.value, "detail")]
}
```

A claim on `row.path` makes an unticked box clickable too. An added item's
`title` is a cell, so `qm.ink(row.value, "title")` places it with a region of its
own. A roster member's `title` is the roster's and no address, so `qm.ink`
refuses it, and an unheld row's `value` is `none`: a plate reads a title as
`if row.held and "title" in row.value { qm.ink(row.value, "title") } else
{ row.title }`.

## Typst Helper Package

The Typst backend injects a virtual package `@local/quillmark-helper:<version>` that exposes the JSON to plates and provides helpers. A plate imports it as a module, under a name it chooses:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#qm.str(qm.data.title).len()         // compute with a text value's str
#qm.ink(qm.data, "title")             // print its ink: the click target survives anywhere
#qm.data.at("$body")                  // root $body: a content object when the main enables a body
#qm.data.date.year()                  // date/datetime fields are native datetimes
#qm.ink(qm.data, "date", "[year]")    // …and their ink takes a display pattern
#for card in qm.data.at("$cards") {
  if card.at("$kind", default: none) == "indorsement" {
    // per-kind handling; $body is present only where the kind enables one,
    // so read it totally: card.<field>, card.at("$body", default: "")
  }
}
```

The plate rule is one sentence: compute with `qm.data.x`, print with
`qm.ink(..)`, claim composed ink with `qm.region(..)`. A value the plate prints
itself carries no region ([PREVIEW.md](PREVIEW.md)).

**The module form is the only one.** `open` refuses any other import of the
helper in a project source — `: *`, `: data, ink`, a bare `#import`, another
version — with `typst::helper_import`, located at the import and naming the form
that compiles. The only name the helper puts in a plate's scope is the one the
plate chose, so a helper export never shadows a plate's own `ink` or `data`; a
plate wanting the bare name binds it (`#let data = qm.data`).

The `$`-prefixed keys must be accessed via `.at("$...")` because Typst identifiers do not include `$`.

Helper contents (generated in `backends/typst/helper.rs` from `lib.typ.template`), each read through the module name:

| member | meaning |
|---|---|
| `data` | the data literal |
| `ink(dict, key, ..args)` | the ink at address `dict.$path + key` ([The ink table](#the-ink-table)) |
| `region(field, body)` | a marker claim ([PREVIEW.md](PREVIEW.md)) |
| `roster(dict, key)` | a matrix's rows ([A `matrix` field](#a-matrix-field)) |
| `str(value)` | a `text` value's verbatim `str` ([Text as a str](#text-as-a-str)) |
| `form-field(..)`, `signature-field(..)` | a form widget, bound to a field by `field:` |
| `tasks`, `elements` | the task-item and element renderer registries |

Internals keep a `_qm-` prefix.

- `data`: a backend-generated Typst dictionary **literal** of all fields, no runtime processing. `Codegen::emit_value` lowered every value at generation time, dispatching on the schema node beside it.
- **The lowering walk.** `helper::lowering` classifies one schema node; the walk
  recurses on shape and does nothing else:

  | node | lowers to | recursion |
  |---|---|---|
  | `contentMediaType: application/quillmark-content+json` | a `#let _qm_cN = [ .. ]` markup block the data cell references (blank ⇒ `""`) | — |
  | `format: date` / `date-time` | `datetime(year:, month:, day:)` / the six-component form, authored wall-clock, seconds zero-filled (blank ⇒ `none`) | — |
  | `type: array` | a Typst array | each element against `items`, at `{path}.{i}` |
  | `type: object` with `properties` | a Typst dict, keys sorted unless the node carries `quillmark:roster` | each value against `properties[key]`, else `additionalProperties`, at `{path}.{key}` |
  | anything else, and any key the schema does not declare | its value literal | — |

  The dispatch is a node test, never a table of names, which is what makes it
  **depth-invariant**: `contact.reply_by` is the same `datetime` a card-level
  `date` is, `rows.0.notes` the same markup block a card-level `richtext` is, and
  the addresses every projection keys on fall out of the recursion. A name table
  keyed on a top-level name cannot be depth-invariant; this cannot fail to be.

  A one-line content node (no `quillmark:multiline: true`) lowers via
  `emit::emit_content_inline` to **pure inline** markup: the single `Para`'s
  content with no block terminator, so no `parbreak`. The value therefore
  composes in an inline slot (`par(..)`, a grid cell) without Typst's "parbreak
  may not occur inside of a paragraph" warning. The flag is read at the content
  leaf, so a one-line `array<richtext>` needs no separate rule.

  A `text` field rides the *same* media type (plus an editor-only
  `quillmark:plain: true`), so it classifies identically and lowers through this
  exact path. The codec differs only at authoring/coercion (literal
  `from_plaintext`), never at codegen.

### Text as a str

A `text` value reaches the plate as content, as `richtext` does, so it keeps
nav, regions and click-to-edit. A plate that computes with it — a
`set document(author:)`, which takes no content, an `upper`, a `split`, a
comparison — reads it through **`qm.str(value)`**, the value's string. The
projection inverts the lowering: a markup escape, a shorthand the emitter
spelled `~` and a smart quote come back as the characters the value holds, a
line break as `\n`. It recovers every character of a `text` value, which
carries no marks (`validation::not_plain`), but not every whitespace: markup
folds a tab into a space, a no-break space reads as a space, whitespace at the
value's end drops, and a run of blank lines reads as one. On `richtext` it
drops the formatting. A blank value, whitespace only included, lowers to `""` and an unanswered optional one to `none`, and `qm.str` passes
both through. The string is computed, so it carries no region: print the value
itself, or `qm.ink`, where the click target matters.

  A non-blank date the shared parsers reject is a `backend::invalid_date` render
  error raised from the walk, at the site that parses it, which is what makes the
  check total over depth.

### The ink table

A value in `data` is native, for computing; its ink is content born in
`lib.typ`, for printing. `Codegen::emit_value` returns the value and records
the ink of every address that has some in one table, `_qm-ink`, keyed by
address:

| address holds | its entry |
|---|---|
| string, number, boolean | `[#(data.at(..)..)]`, reading the cell off `data`; it prints what `#<value>` prints |
| content | the `_qm_cN` binding the data cell holds |
| date | `(..args) => text(data.at(..)..display(..args))`; `(..args) => none` where blank |
| array of those | the array of the elements' ink, each element also keyed on its own |
| typed dictionary, matrix, blank content, `none` | no entry |

**`qm.ink(dict, key, ..args)`** looks up `dict.$path + key`. `dict` is
`qm.data`, a card, a typed dictionary or a roster row's `value`, and `key` one
or more address steps from it: `"year"`, `"address.city"`, `"awards.0"`,
`"$body"`. A miss consults `_qm-node`, so the rules are total:

- An address the schema does not have is a compile error: a typo, a disabled
  card `$body`, a roster member's `title`.
- A known address with no entry is `none`: blank content, a `none` value, a
  typed dictionary, a matrix. A blank date's entry answers `none` too, so a
  `== none` fallback still fires. A blank string prints nothing.
- `args` is a date's display pattern, formatted through the date's own
  `display`, so a `date`-only field throws Typst's native error on an `[hour]`
  pattern. On any other address, an array of dates included, `args` is an
  assertion error: address the element.

The ink is addressed rather than carried on the value because of **regions**.
It is born at its generated entry, not at the plate's print, so it survives
being laundered — through a `#let` binding, a function parameter, a loop
variable, or a vendored package that formats it internally — and one entry per
address gives a card's field the per-instance identity a shared `card` loop
variable lacks. A native `datetime` handed to a package cannot do that: the ink
is born wherever the package places it. Each entry's byte range is its window;
a scalar's has no segments, so its whole first placement is one region. An
array's entry windows its elements beside their own entries, never around
them, so windows never nest.

The entries read their cells off `data` rather than repeating them, so a value
edit changes the data literal and leaves the table's text alone; only an entry
appearing or going (a blank filled, a row added) changes it. `data`, each card
and each typed dictionary is wrapped in a `{..}` code block, a unit Typst's
incremental reparser swaps alone, so an edit inside a card or a row reparses
that block, not the whole literal.

### Schema addresses

`qm.form-field(field:)`, `qm.region(field, ..)` and `qm.ink(dict, key, ..)`
name a schema field, and the generated `_qm-meta` address tree (`_qm-node`)
validates that name at compile time rather than leaving it silently unbound:

| Address | Admitted by |
|---|---|
| `subject` | any declared field |
| `refs.2` | an array field — the element step |
| `refs.2.org` | a typed table's row property, after the element step |
| `classification.poc` | a container field — the property step |
| `contact.address.city` | either step again, wherever the schema nests |
| `qualifications.flight_cc` | a matrix field — the member step, whose own address is the tick |
| `qualifications.wing_ig.title` | an open matrix — any key spelled as a member id, then the added item's cells |
| `$cards.<kind>.<n>.<field>` | a card field, `<n>` the per-kind ordinal |
| `$cards.<kind>.<n>.<field>.<suffix>` | any of those suffixes, on a card field |

Every step is gated on what the node it steps out of actually offers, not on the
name alone, so `subject.0` and `subject.poc` are both rejected on a scalar
`subject`: a scalar has neither an element nor a property for the address to
resolve to. A **container** is a typed dictionary or a variant container — both
project as `type: object` carrying `properties`, so a variant's cells and its
`value` discriminant are addressable exactly as a dictionary's keys are
([SCHEMAS.md](SCHEMAS.md#enum-variants)). The grammar stops where the schema
does, at whatever depth that is. This is the acroform resolver's grammar
(`backends/acroform/src/bind.rs`), so one address binds on either backend. The
grammar is written twice, in two languages, and held to one table by
`quillmark/tests/address_grammar.rs`.

**An address the grammar admits is a key the plate carries**, past the two
steps whose keys the value decides: an array's index and a matrix's member. The
blank-fill is total at every depth ([SCHEMAS.md](SCHEMAS.md#blank-filled-render)),
so a declared address resolves however much of its container the document left
out: `qm.data.contact.address.city` is a direct read, never a guarded one. This is
the converse of the `$`-metadata rule above — those keys are read with a total
accessor *because* they may be absent, and a declared field may not be.

The two value-decided steps are judged on the schema alone, so the tables are
the same for every render. `refs.9` is admitted against one ref, a roster
member's address whether or not the value holds it, and on an open matrix any
key spelled as a member id, the added item's node (`qualifications.wing_ig`). A
plate reaches either through its container: a loop over the array, or
[`qm.roster`](#a-matrix-field) over the matrix.

Cards carry their canonical prefix as `$path`, so a plate composes a card
address without reimplementing the kind+ordinal grammar:
`qm.region(card.at("$path") + "$body", ..)`, `qm.ink(card, "$body")`. A body
is content rather than a
bindable field, so a `$body` address is the plate grammar's alone: acroform's
resolver roots none.

The same addresses key the preview's region sidecar
([PREVIEW.md](PREVIEW.md)), so a plate that prints one container property or
one row cell through `qm.ink` surfaces a region a consumer can route back to it.
