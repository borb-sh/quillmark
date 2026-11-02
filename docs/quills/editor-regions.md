# Editor Regions

A live preview maps a click on the page back to the schema field that produced the ink under it, through the geometry sidecar (`session.regions()`) and `session.fieldAt(...)`. This page is for plates whose quill will be edited that way. A quill rendered only to files can skip it: nothing here changes the output.

Regions come from three producers, each one your plate declares:

- **Generated ink**: a `text` or `richtext` field, and any field printed through [`qm.ink`](#print-with-ink).
- **A claim**: [`qm.region`](#tying-composed-content-to-a-field) on ink the plate composes.
- **A widget**: a [form field](typst-backend.md#binding-to-a-schema-field) bound with `field:`.

A value the plate prints itself — `#qm.data.subject`, `#text(size: 9pt, qm.data.name)` — carries no region. One rule covers the plate: **compute with `qm.data.x`, print with `qm.ink(..)`, claim composed ink with `qm.region(..)`.** `quillmark check --regions` lists the filled fields of a document that no click reaches.

## Print with `ink`

`qm.ink(dict, key)` prints the field at `key` of `dict`: `qm.data`, a card, a typed dictionary or table row, or a [`roster`](typst-backend.md#matrices) row's `value`. `key` is one or more address steps from it: `"owner"`, `"address.city"`, `"tags.0"`.

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#let action(item) = [#qm.ink(item, "owner"), due #qm.ink(item, "due", "[month]/[day]")]

#for item in qm.data.actions.filter(a => a.status == "open") {
  action(item)
}
```

Each value's ink is born in generated code, so it keeps its field's address however far it travels: a function parameter, a loop variable, a `filter` or `sort`, a package. `a.status == "open"` needs the value; `qm.ink(item, "owner")` is what goes on the page.

| Address holds | `qm.ink(x, "<key>")` |
|---|---|
| `enum`, `integer`, `number`, `boolean` | content printing what `#x.<key>` prints |
| `text`, `richtext` | the content `x.<key>` already is |
| `date`, `datetime` | its default display (`2026-03-04`); for a pattern, `qm.ink(x, "<key>", ..)` |
| an array of those | the array of their ink; one element by its own address, `"tags.0"` |
| a blank value (a blank date, empty content, an unanswered `type: t?`) | `none` |
| a typed dictionary, a table, a matrix | `none`: address the value position, `qm.ink(x, "address.city")`, `qm.ink(row, "org")` |

An address the schema does not have fails the render, and so does a pattern on an address that is not a date's: a roster member's `title` is the roster's, not an address, so guard with `row.held and "title" in row.value` (an unheld row's `value` is `none`).

Ink is content, so styling that wraps it keeps the address: `upper`, `text(..)`, `strong`, `align`, a `box`. Computing a string needs the value, and what it prints is composed ink: `calc.round`, `str(n, base: 16)`, `.slice(..)`, `+` with a string. Claim that with [`qm.region`](#tying-composed-content-to-a-field), addressed through the dictionary's `$path`:

```typst
#for row in qm.data.expenses {
  qm.region(row.at("$path") + "amount")[#calc.round(row.amount, digits: 2)]
}
```

`$path` is the dictionary's address prefix, which `qm.data`, every card and every typed dictionary carry and `qm.ink` reads. A plate that walks a typed dictionary's keys, compares one whole, or spreads a row into a call (`f(..row)`) sees it: skip `$`-prefixed keys there.

## Dates: print through `qm.ink`

- **`qm.ink(x, "<date>", ..args)` renders and is clickable.** It returns Typst *content* whose glyphs carry a region keyed on the date's address: the atomic, picker-editable click-to-edit target. It accepts the same patterns `datetime.display` does, and a `date`-only field inherits Typst's native error on a `[hour]` pattern. `none` for a blank date, so a `== none` fallback still fires.
- **`qm.data.<field>` is the value.** Reach for it whenever you want a `datetime`: math, comparison, components, handing it to a package. What the plate prints from it (`#qm.data.issued.display("…")`) carries no region.

The difference matters most when a *package* does the inking: a `datetime` handed to a package draws its glyphs wherever the package places them, so nothing ties them to your schema field, while `qm.ink`'s ink is born in generated code and keeps its address however deep it travels.

## What carries no region

A value the plate prints itself carries the plate's own source position, which names no field. That holds however the plate spells the read or wraps it: `#qm.data.subject`, `#upper(qm.data.subject)`, `#text(size: 9pt, qm.data.name)`, a row cell in a computed table. Each renders correctly and loses only the click target, which is why nothing announces it. Print the field through [`qm.ink`](#print-with-ink), or claim the composed ink with [`qm.region`](#tying-composed-content-to-a-field). `quillmark check --regions ./my-quill doc.md` names every filled field no click reaches, and each one only an enclosing field's region reaches.

## Tying Composed Content to a Field

A live preview routes a click back to the schema field that produced the ink under it, and it finds that field automatically for content it generated: a `richtext` field's markup, a field printed through `qm.ink`. Content your plate *composes* — a banner keyed off `qm.data.classification`, an address block a vendored package lays out, a computed table — draws ink Quillmark cannot attribute to anything. `qm.region` claims it:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#let banner(level) = box(stroke: 1pt, inset: 6pt)[#upper(level)]

#qm.region("classification")[#banner(qm.data.classification)]
```

The banner now appears in `session.regions()` under `main.classification` and a click on it resolves through `session.fieldAt(...)`, exactly as if the field had drawn it.

`body` is returned untouched, bracketed by two invisible `metadata` markers, so the wrapper changes nothing about layout or output bytes. Unlike a `qm.form-field` widget it reserves no space and draws no click target of its own: it claims the ink that is already there.

### What it claims

A claim is a **fallback**, not an override. Ink already tracked to a field keeps that field, and the wrapper takes only what is left:

```typst
#qm.region("recipient")[
  #line(length: 2in)                       // no field of its own → claimed for `recipient`
  #qm.data.body                            // a richtext field → stays `body`
  Prepared by #qm.ink(qm.data, "author")   // a field's ink → stays `author`
]
```

Nesting therefore reads as ordinary scoping, and wrapping never moves a region off the field that generated it. The flip side: you cannot use `qm.region` to *retarget* ink that is already attributed.

What else a claim takes:

- Text Typst draws with no source position of its own is the claim's: a string turned into content inside a function call (`text(size: 9pt, "LIT")`), a list's bullets.
- An underline or strike stroke stays with the content it decorates.
- A claim running across a page break leaves the header, footer, background and page number it crosses alone. A claim placed in the header or footer itself, such as a classification banner, takes that marginal's ink.
- A footnote or a float is never claimed from outside, wherever it is anchored. To claim one, put the `qm.region` inside it: `#footnote[#qm.region("citation")[...]]`.
- A claim running across a page break does take that page's `foreground`. Wrap a foreground watermark in `pdf.artifact` to keep it out.

Each **call** claims independently, so `field` need not be a literal and a wrapper used once per card yields one region per card. A card's `$path` is its address prefix, separator included, so appending a field name (`card.at("$path") + "topic"`) or `$body` spells that card's address:

```typst
#for card in qm.data.at("$cards", default: ()) {
  if card.at("$kind", default: none) == "note" {
    qm.region(card.at("$path") + "$body", render-card(card))
  }
}
```

The branch names a kind with a body: a card of a kind the quill does not declare, or of one under `body.enabled: false`, has no `$body` address, and `qm.region` asserts on it.

A card's own fields need no claim: print them through `qm.ink(card, ..)`.

### Parameters and errors

| Name | Type | Default | Meaning |
|------|------|---------|---------|
| `field` | `str` | required (positional) | Schema address: a field name, an array element like `"refs.2"`, or a card path built from the card's `$path` prefix. |
| `body` | any content | required (positional) | Returned unchanged; its ink is what gets claimed. |

- A `field` that is not a known schema address, or is not a string, raises a Typst assert pointing at `region`.
- A claim whose content Typst lays out somewhere else entirely (`#place`, a float) claims whatever ink lands between its markers instead; wrap the placed content rather than the `place` call.
- Emit the call's return value whole. Splitting it — passing `.children` through separately, say — can land the opening marker in a frame without its closing one. Such a claim is bounded by nothing, so rather than let it take every unattributed piece of ink to the end of the document it is dropped entirely and reported as a `typst::unclosed_region` warning naming the field.

The label `<__qm_region__>` and metadata `kind: "__qm_region__"` are reserved for this hand-off: the same `query(metadata)` caveat applies.

## The address an app reads

A plate spells an address in plate space; `session.regions()`, `fieldAt`, `Diagnostic.path` and `quillmark check --regions` report the same place as a document path. The two count cards differently: plate space by kind, a document path by position in the document. With cards `note`, `annotation`, `note` in that order:

| Plate space (`qm.region`, `qm.ink`; `Form.yaml` binds fields, not bodies) | Document path (regions, diagnostics) |
|---|---|
| `classification` | `main.classification` |
| `refs.0.org` | `main.refs[0].org` |
| `$body` | `main.body` |
| `$cards.note.0.topic` | `cards.note[0].topic` |
| `$cards.note.1.topic` | `cards.note[2].topic` |
| `$cards.annotation.0.text` | `cards.annotation[1].text` |
| `$cards.note.1.$body` | `cards.note[2].body` |

`cards.note[2]` is the document's third card, which is a `note`; `$cards.note.1` is its second `note`. Key an editor's click map on the document path the session reports, never on the spelling the plate uses.
