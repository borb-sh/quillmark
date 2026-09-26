# Editor Regions

A live preview maps a click on the page back to the schema field that produced the ink under it, through the geometry sidecar (`session.regions()`) and `session.fieldAt(...)`. This page is for plates whose quill will be edited that way. A quill rendered only to files can skip it: nothing here changes the output.

Quillmark attributes ink on its own for a `richtext` or `plaintext` field, for any field printed through [`ink`](#print-with-ink), for a date placed with `display`, and for a scalar read written in the plate or a module it imports (`#data.subject`). A [form-field widget](typst-backend.md#binding-to-a-schema-field) opts in with `field:`. Everything else is unattributed until a `field-region` claims it.

## Print with `ink`

`data`, each card, and each typed dictionary or table row carry a printable copy of their fields, read with `ink`:

```typst
#import "@local/quillmark-helper:0.1.0": data, display, ink

#let action(item) = [#ink(item).owner, due #display(item, "due", "[month]/[day]")]

#for item in data.actions.filter(a => a.status == "open") {
  action(item)
}
```

Each value's ink is born in generated code, so it keeps its field's address however far it travels: a function parameter, a loop variable, a destructuring, a `filter` or `sort`, a package. One rule: **compute with the dictionary, print its ink.** `a.status == "open"` needs the value; `ink(item).owner` is what goes on the page.

| Field | `ink(x).<field>` |
|---|---|
| `string`, `enum`, `integer`, `number`, `boolean` | content printing what `#x.<field>` prints |
| `richtext`, `plaintext` | the content `x.<field>` already is |
| `date`, `datetime` | its default display (`2026-03-04`); for a pattern, `display(x, "<field>", ..)` |
| an array of those | the array of their ink |
| any of those whose value is `none` (a blank date, an unanswered `type: t?`) | `none` |
| a typed dictionary or a table | nothing: each dictionary carries its own, so `ink(x.address).city`, `ink(row).org` |

Ink is content, so styling that wraps it keeps the address: `upper`, `text(..)`, `strong`, `align`, a `box`. Computing a string needs the value, and what it prints is composed ink: `calc.round`, `str(n, base: 16)`, `.slice(..)`, `+` with a string. Claim that with [`field-region`](#tying-composed-content-to-a-field), addressed through the dictionary's `$path`:

```typst
#for row in data.expenses {
  field-region(row.at("$path") + "amount")[#calc.round(row.amount, digits: 2)]
}
```

The copy lives under a `$ink` key, which `ink` reads, beside the dictionary's address prefix as `$path`, the one a card already carries. A plate that walks a typed dictionary's keys, compares one whole, or spreads a row into a call (`f(..row)`) sees both: skip `$`-prefixed keys there.

## Dates: `display` and `data`

- **`display(field, ..args)` renders and is clickable.** It takes the date's *schema address* (`"issued"`), or a dictionary and the date's key within it (`row, "due"`), never its value, and returns Typst *content* whose glyphs carry a region keyed on that address: the atomic, picker-editable click-to-edit target. It accepts the same patterns `datetime.display` does, and a `date`-only field inherits Typst's native error on a `[hour]` pattern. The address is checked against the schema at compile time, the same check `form-field(field:)` and `field-region` apply, so a typo fails the render instead of dropping the date from it. `none` for a blank date, so a `== none` fallback still fires.
- **`data.<field>` is the value.** Reach for it whenever you want a `datetime` — math, comparison, components, handing it to a package. A direct plate reference (`#data.issued.display("…")` written in the plate itself) still regions, the same way any scalar reference site does.

One rule: **want a value → `data.<field>`; want clickable ink → `ink(x).<field>` for the default format, `display(x, "<field>", ..)` for a pattern**, or `display("<field>", ..)` where you hold an address rather than its dictionary. The difference matters exactly when a *package* does the inking: a `datetime` handed to a package draws its glyphs wherever the package places them, so nothing ties them to your schema field, while `display`'s ink is born in generated code and keeps its address however deep it travels.

## Which Reads Get Regions

A scalar printed straight off `data` rather than through its ink is tracked at the expression that draws it, so where you write the read decides whether it surfaces in `session.regions()`. Naming the value first is fine: a `let` bound once to one whole `data` chain is followed, and stepping into a container through that name keeps the cell's address.

```typst
#let c = data.classification
#c.poc                       // regions as `classification.poc`, same as #data.classification.poc
```

A read into a typed table works the same way, one step further: the index and
then the row property, the addresses `form-field(field:)` takes.

```typst
#data.refs.at(0).org         // regions as `refs.0.org`
#data.refs.at(0)             // regions as `refs.0` — each step is its own address
#let row = data.refs.at(0)
#row.org                     // regions as `refs.0.org` too
```

Rebind that name anywhere in its file — a second `let`, a closure parameter, a loop pattern, an assignment — and it stops being followed, because a read can no longer be tied to one value. Three shapes are past what the tracker follows at all:

| Shape | Why |
|---|---|
| a value handed to a function (`#let f(c) = [#c.poc]`) | the parameter is a fresh name bound per call |
| a destructured binding (`#let (poc, ..) = data.classification`) | the pattern names no chain |
| a per-card loop variable (`#for card in data.at("$cards")`) | one shared expression site carries no per-instance identity |

Each of those still renders correctly and loses only the click target, which is why nothing announces it. Print the field's [`ink`](#print-with-ink) instead to get the region back.

**Backend-generated ink needs none of this.** A `richtext` value, any field's `ink`, and a date placed through `display` are born in generated code, so they keep their address through a function, a loop, or a package that rebuilds them. A *value* laundered through any of the shapes above is on the list like every other value, dates included.

## Tying Composed Content to a Field

A live preview routes a click back to the schema field that produced the ink under it, and it finds that field automatically for content it generated: a `richtext` field's markup, a `#data.subject` reference in your plate. Content your plate *composes* — a banner keyed off `data.classification`, an address block a vendored package lays out, a computed table — draws ink Quillmark cannot attribute to anything. `field-region` claims it:

```typst
#import "@local/quillmark-helper:0.1.0": data, field-region

#let banner(level) = box(stroke: 1pt, inset: 6pt)[#upper(level)]

#field-region("classification")[#banner(data.classification)]
```

The banner now appears in `session.regions()` under `classification` and a click on it resolves through `session.fieldAt(...)`, exactly as if the field had drawn it.

`body` is returned untouched, bracketed by two invisible `metadata` markers, so the wrapper changes nothing about layout or output bytes. Unlike a `form-field` widget it reserves no space and draws no click target of its own: it claims the ink that is already there.

### What it claims

A claim is a **fallback**, not an override. Ink already tracked to a field keeps that field, and the wrapper takes only what is left:

```typst
#field-region("recipient")[
  #line(length: 2in)          // no field of its own → claimed for `recipient`
  #data.body                  // a richtext field → stays `body`
  Prepared by #data.author    // a scalar reference → stays `author`
]
```

Nesting therefore reads as ordinary scoping, and wrapping never moves a region off the field that generated it. The flip side: you cannot use `field-region` to *retarget* ink that is already attributed. Ink Typst attributes to no source position at all — list bullets, underline rules — stays unclaimed here as it is everywhere else.

Each **call** claims independently, so `field` need not be a literal and a wrapper used once per card yields one region per card. A card's `$path` is its address prefix, separator included, so appending a field name (`card.at("$path") + "topic"`) or `$body` spells that card's address:

```typst
#for card in data.at("$cards", default: ()) {
  if card.at("$kind", default: none) == "note" {
    field-region(card.at("$path") + "$body", render-card(card))
  }
}
```

The branch names a kind with a body: a card of a kind the quill does not declare, or of one under `body.enabled: false`, has no `$body` address, and `field-region` asserts on it.

A card's own fields need no claim: print them through `ink(card)`.

### Parameters and errors

| Name | Type | Default | Meaning |
|------|------|---------|---------|
| `field` | `str` | required (positional) | Schema address: a field name, an array element like `"refs.2"`, or a card path built from the card's `$path` prefix. |
| `body` | any content | required (positional) | Returned unchanged; its ink is what gets claimed. |

- A `field` that is not a known schema address, or is not a string, raises a Typst assert pointing at `field-region`.
- A claim whose content Typst lays out somewhere else entirely (`#place`, a float) claims whatever ink lands between its markers instead; wrap the placed content rather than the `place` call.
- Emit the call's return value whole. Splitting it — passing `.children` through separately, say — can land the opening marker in a frame without its closing one. Such a claim is bounded by nothing, so rather than let it take every unattributed piece of ink to the end of the document it is dropped entirely and reported as a `typst::unclosed_field_region` warning naming the field.

The label `<__qm_region__>` and metadata `kind: "__qm_region__"` are reserved for this hand-off: the same `query(metadata)` caveat applies.
