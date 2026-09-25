# Plate Data Injection

> **Implementation**: `crates/backends/typst/src/`

## TL;DR

Plates get document data through a backend-injected virtual Typst package, not a template engine. Data flows in two stages: `Quill::compile_data()` produces validated, blank-filled JSON in which content fields are canonical `Content` objects; `Backend::open()` generates the helper's `lib.typ`, walking each value beside its transform-schema node to lower it, no per-field markdown re-parse.

One rule governs the lowering, at every depth: **a declared type means the same thing wherever it is declared, and every type lowers to its native Typst value unless it has a canonical rendering.** Only the content types have one — the authored text — so only they lower to content; a date lowers to a native `datetime`, because every rendering of `2026-01-02` is a typographic decision the plate owns. Backend-generated *ink* rides beside the value instead, under each dictionary's `$ink` (`ink(row).org`), or is reached by address (`display(addr, ..)`); either way it is born in generated source, which is what makes it laundering-proof.

## Overview

1. `Quill::compile_data()` coerces, validates, normalizes, and **blank-fills** the
   root-block fields, and each composable card's fields against its `card_kind`
   schema, into a plain JSON object: every absent schema field resolves to its
   authored value, else the schema `default:`, else the field's blank.
   Content fields cross as canonical `Content` objects (coercion imports an
   authored markdown string to the `Content` and re-canonicalizes an
   editor-supplied one). An incomplete document still renders: an absent or
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
- `data`, each card, and each typed dictionary carry `$ink`, the [ink twin](#the-ink-twin) of their fields, wherever at least one field has ink, and with it `$path`, their address prefix (`""` on `data`, `refs.0.` on a row). A data key spelling `$ink` or `$path` in any of them is dropped; a dictionary the schema does not type passes through verbatim, keys included
- User payload fields sit flat at the root next to the `$` keys; field names match `[a-z_][a-z0-9_]*` and therefore never collide with `$` metadata

#### A `matrix` field

A matrix reaches the plate **total**: an ordered mapping carrying every declared
member, keyed by member id in roster order, whatever the document ticked. Order
is the one place a matrix departs from the canonical emission below: dict keys
otherwise sort, so the transform schema carries the roster as `quillmark:order`
on the matrix node and the codegen emits those keys in it. The order is a
property of the schema, never of the data, so equal data still produces
byte-equal source.

```json
"qualifications": {
  "flight_cc":  { "held": true,  "title": "Flight CC", "detail": {…} },
  "dodin_ops":  { "held": false, "title": "DODIN Ops", "detail": {…} }
}
```

- `held` is the tick, a boolean the schema synthesizes on every member.
- `title` comes from the roster, not from the document, so a plate prints the
  vocabulary without holding a second copy of it.
- The remaining keys are the field's declared columns, each at its declared
  type.
- **The wire carries the live world only**: an unheld member's columns are their
  blanks whatever the document retains, so `member.detail` reads without a guard
  and never prints a stranded answer.

Member cells are ordinary addresses: `qualifications.flight_cc.held` regions and
binds like any leaf. Each member carries its own `$ink`; the matrix, holding only
members, carries none, so its keys are exactly its roster. `title` is written by the projection rather than held as a
cell, so it carries none.

## Typst Helper Package

The Typst backend injects a virtual package `@local/quillmark-helper:<version>` that exposes the JSON to plates and provides helpers.

```typst
#import "@local/quillmark-helper:0.1.0": data

#data.title                  // plain field access
#ink(data).title             // the same text, keeping its click target anywhere
#data.at("$body")            // root $body: a content object when the main enables a body
#data.date.year()            // date/datetime fields are native datetimes
#display("date", "…")        // …and `display` places the click-to-edit rendering
#for card in data.at("$cards") {
  if card.at("$kind", default: none) == "indorsement" {
    // per-kind handling; $body is present only where the kind enables one,
    // so read it totally: card.<field>, card.at("$body", default: "")
  }
}
```

The `$`-prefixed keys must be accessed via `.at("$...")` because Typst identifiers do not include `$`.

Helper contents (generated in `backends/typst/helper.rs` from `lib.typ.template`):

- `data`: a backend-generated Typst dictionary **literal** of all fields, no runtime processing. `Codegen::emit_value` lowered every value at generation time, dispatching on the schema node beside it.
- **The lowering walk.** `helper::lowering` classifies one schema node; the walk
  recurses on shape and does nothing else:

  | node | lowers to | recursion |
  |---|---|---|
  | `contentMediaType: application/quillmark-content+json` | a `#let _qm_cN = [ .. ]` markup block the data cell references (blank ⇒ `""`) | — |
  | `format: date` / `date-time` | `datetime(year:, month:, day:)` / the six-component form, authored wall-clock, seconds zero-filled (blank ⇒ `none`) | — |
  | `type: array` | a Typst array | each element against `items`, at `{path}.{i}` |
  | `type: object` with `properties` | a Typst dict, keys sorted unless the node carries `quillmark:order` | each value against `properties[key]`, at `{path}.{key}` |
  | anything else, and any key the schema does not declare | its value literal | — |

  The dispatch is a node test, never a table of names, which is what makes it
  **depth-invariant**: `contact.reply_by` is the same `datetime` a card-level
  `date` is, `rows.0.notes` the same markup block a card-level `richtext` is, and
  the addresses every projection keys on fall out of the recursion. A name table
  keyed on a top-level name cannot be depth-invariant; this cannot fail to be.

  A `richtext(inline)` node (`quillmark:inline: true`) lowers via
  `emit::emit_content_inline` to **pure inline** markup: the single `Para`'s
  content with no block terminator, so no `parbreak`. The value therefore
  composes in an inline slot (`par(..)`, a grid cell) without Typst's "parbreak
  may not occur inside of a paragraph" warning. The flag is read at the content
  leaf, so `array<richtext(inline)>` needs no separate rule.

  A `plaintext` field rides the *same* media type (plus an editor-only
  `quillmark:plain: true`), so it classifies identically and lowers through this
  exact path. The codec differs only at authoring/coercion (literal
  `from_plaintext`), never at codegen.

  A non-blank date the shared parsers reject is a `backend::invalid_date` render
  error raised from the walk, at the site that parses it, which is what makes the
  check total over depth.
- **`ink(dict)`** → the dictionary's `$ink` twin, `(:)` where it has none. See
  [The ink twin](#the-ink-twin).
- **`display(field, ..args)`** → content, the one address-keyed projection.
  `_qm-display` binds one `#let _qm_dN = (..args) => text(datetime(..).display(..args))`
  closure per present date, keyed by schema address (`issued`, `stamps.2`,
  `contact.reply_by`, `$cards.<kind>.<n>.<field>`; compose a card address from
  the card's `$path`), and `display` calls it. `display(dict, key, ..)` spells
  the address from the dictionary's `$path`. The address is validated like any
  other (below), so a typo is a compile error rather than ink that quietly goes
  missing; `none` comes back for a known address carrying no date — a blank one,
  or a field that is not a date — so a `== none` fallback still fires. Formatting
  through the date's own `display` inherits its type, so a `date`-only field throws
  Typst's native error on an `[hour]` pattern.

  The reason it is addressed rather than carried on the value is
  **regions**. Its ink is born at the generated node, not at the plate's
  reference site, so it survives being laundered — through a `#let` binding, a
  loop variable, or a vendored package that formats it internally — and one node
  per cell gives a card's date the per-instance identity a shared `card.<field>`
  loop variable lacks. A native `datetime` handed to a package cannot do that:
  the ink is born wherever the package places it.

### The ink twin

A value in `data` is native, for computing; its ink is content born in
`lib.typ`, for printing. `Codegen::emit_value` returns both from one walk, and
each dictionary it closes gathers its fields' ink under a leading `$ink` key:

| field | its ink |
|---|---|
| string, number, boolean | `[#(<literal>)]` inline, parenthesized so a negative number lexes, its window the block: it prints what `#<value>` prints |
| content | the `_qm_cN` binding the data cell holds |
| date | `_qm_dN()`: the closure `display` calls, called with no pattern |
| array of those | the array of their ink |
| any of those at `none` | `none` |
| typed dictionary, and an array of them | none: each carries its own `$ink` |

Only declared fields have ink, so `$quill`, `$kind` and undeclared keys have
none. A date's is its default display, so every ink prints as `#ink(x).f`; a
pattern goes through `display(x, "f", ..)`, which finds the closure by the
dictionary's `$path`. The twin rides on the dictionary rather than on `data` alone because rows
are what plates filter, sort and hand to functions: a twin kept apart loses the
pairing at the first `filter`. The cost is two visible keys, `$ink` and
`$path`, which dictionary equality (and so `contains` and `dedup`), `keys()` and
spreading see.

A scalar's ink window has no segments, so its whole first placement is one
region, as a plate scalar site's is ([PREVIEW.md](PREVIEW.md)). `data`, each
card and each typed dictionary is wrapped in a `{..}` code block: Typst's
incremental reparser swaps such a block alone, and an edit changes a field and
its ink, so an edit inside a card or a row reparses that block. A top-level
field's edit spans `data`'s own `$ink` and the field, with `$cards` between
them, and reparses the whole literal.

### Schema addresses

`form-field(field:)`, `field-region(field)` and `display(field, ..)` name a
schema field, and the generated `_qm-meta` address tree (`_qm-known-path`)
validates that name at compile time rather than leaving it silently unbound:

| Address | Admitted by |
|---|---|
| `subject` | any declared field |
| `refs.2` | an array field — the element step |
| `refs.2.org` | a typed table's row property, after the element step |
| `classification.poc` | a container field — the property step |
| `contact.address.city` | either step again, wherever the schema nests |
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

**An address the grammar admits is a key the plate carries.** The blank-fill is
total at every depth ([SCHEMAS.md](SCHEMAS.md#blank-filled-render)), so a
declared address resolves however much of its container the document left out:
`data.contact.address.city` is a direct read, never a guarded one. This is the
converse of the `$`-metadata rule above — those keys are read with a total
accessor *because* they may be absent, and a declared field may not be.

Cards carry their canonical prefix as `$path`, so a plate composes a card
address without reimplementing the kind+ordinal grammar:
`field-region(card.at("$path") + "$body")`. A body is content rather than a
bindable field, so a `$body` address is the plate grammar's alone: acroform's
resolver roots none.

The same addresses key the preview's region sidecar
([PREVIEW.md](PREVIEW.md)), so a plate that reads one container property or one
row cell surfaces a region a consumer can route back to it.
