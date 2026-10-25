# Typst Backend

The Typst backend generates PDF, SVG, and PNG documents using the [Typst](https://typst.app/) typesetting system. It converts card-yaml payload fields to Typst markup, injects them into the plate via a generated helper package, and compiles to the requested format.

The plate is `plate.typ` at the quill root, found by name. A quill keeping it elsewhere names its path as [`typst.plate_file`](quill-yaml-reference.md#typst-section), and a quill with neither fails to render as `typst::plate_missing`.

## Typst Version

Plates compile against Typst 0.15. `quillmark --version` names the exact release after its own version, and a plate reads the same value as `sys.version`.

typst.app documents the newest Typst release, and its [changelog](https://typst.app/docs/changelog/) lists what each release adds. A plate using a feature added after 0.15 fails to compile.

A plate compiled by an older Typst than it needs fails at the first feature that Typst lacks. An `assert` on `sys.version` fails with the reason instead:

```typst
#assert(sys.version >= version(0, 15), message: "this plate needs Typst 0.15 or later")
```

A release that moves Typst to a new minor version is a breaking change, and its [migration guide](../migrations/index.md) names the new version.

## Data Access

Plates are plain Typst code. Document metadata reaches the plate as a Typst dictionary literal exposed by the virtual `@local/quillmark-helper` package, which a plate imports as a module under a name of its choosing:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#qm.str(qm.data.title).len()            // a declared field: always present
#qm.data.at("logo", default: none)      // an undeclared key: may be absent
#qm.ink(qm.data, "title")               // the field printed, click-to-edit
```

The module form is the only import the backend accepts. `: *`, `: data, ink`, a bare `#import` or another version fails the render with `typst::helper_import` at the import line. Every helper name reaches the plate through the module, so none collides with the plate's own; a plate wanting a bare name binds it, `#let data = qm.data`.

The rule of thumb: **compute with `qm.data.x`, print with `qm.ink(..)`, claim composed ink with `qm.region(..)`.**

Every field arrives at its **native** Typst type — a `date` as a `datetime`, a number as an int or float, an `object` as a dict — with one exception: `text` and `richtext` arrive as Typst content, their text already lowered to markup, because the authored text *is* their rendering. This holds at every depth: a `date` declared inside an `object` or an `array` row is the same `datetime` a top-level one is.

### Text as a str

A `text` value is content, so it prints with its click target, but a plate that computes with it needs a `str`: `set document(author: ..)` takes no content, and neither do `upper`, `split`, `len` or a comparison. `qm.str(value)` returns the value's string, markup escapes, smart quotes and line breaks undone:

```typst
#set document(author: qm.str(qm.data.author))
#let office = upper(qm.str(qm.data.office))
#let recipients = qm.data.memo_for.map(qm.str)       // an array of text
#if qm.str(qm.data.status) == "final" [ … ]
```

Whitespace comes back normalized: a tab reads as a space, a no-break space as a space, whitespace at the end drops, and a run of blank lines reads as one. A blank value (`""`, whitespace only included) and an unanswered optional one (`none`) pass through unchanged. The string is computed, so it carries no region: print the value itself, or `qm.ink(..)`, where the click target matters. On a `richtext` value `qm.str` drops the formatting.

`qm.ink(dict, key)` prints a field by its address: `dict` is `qm.data`, a card, a typed dictionary or table row, or a [`roster`](#matrices) row's `value`, and `key` one or more steps from it: `#qm.ink(qm.data, "title")`, `#qm.ink(row, "org")`, `#qm.ink(qm.data, "address.city")`. It prints what the field prints and keeps the field's click target in an [editor preview](editor-regions.md#print-with-ink) however the plate passes it around. An address the schema does not have fails the render; blank content, a blank date, a typed dictionary and a matrix give `none`.

### Dates

A present `type: date` / `type: datetime` field is a native `datetime`; a blank date is `none` (so `#if qm.data.field != none` guards are unchanged):

```typst
#qm.data.issued.display("[day padding:none] [month repr:long] [year]")  // native string
#qm.data.issued.year()                                                   // components: int
#(qm.data.issued < qm.data.due)                                             // comparison, arithmetic
#some-package(date: qm.data.issued)                                      // any datetime-consuming package
#qm.ink(qm.data, "issued", "[day padding:none] [month repr:long] [year]")  // rendered, click-to-edit
#qm.ink(row, "due", "[year]")                                            // the same, for a row or card in hand
#if qm.data.issued != none { .. }                                        // presence
```

`datetime.today()` returns the render date the host supplied, the same date a `today` field renders as. A render reads no clock, and the host owns the time zone, so an `offset:` is ignored: `datetime.today(offset: 0)` is the host's date, not UTC's.

Everything except the two `qm.ink` calls is ordinary Typst, because the value is an ordinary `datetime`. On a date address, `qm.ink(dict, key, ..args)` prints the date as `datetime.display` would with the same patterns, its default display with none; a blank date gives `none`, and a pattern on any other address fails the render. Reach for `qm.data.<field>` whenever you want the value itself: math, comparison, components, or handing it to a package. The two print the same text and differ only in [editor previews](editor-regions.md#dates-print-through-qmink), where `qm.ink` keeps the printed date clickable.

### Which accessor to reach for

A key's *declaration* decides whether it can be absent, and that decides the accessor. Three cases, no judgement calls:

| Key | Accessor | Why |
|---|---|---|
| A field declared in `Quill.yaml` | `qm.data.subtitle` | Always present: compilation blank-fills every declared field with its authored value, else the schema `default:`, else the field's [blank](#blank-values). |
| A `$`-sigiled key (`$kind`, `$body`, `$cards`, `$path`) | `qm.data.at("$body", default: "")` | Typst identifiers exclude `$`, *and* `$`-metadata is present only where it is defined: `$kind` on every card, `$body` only where the kind enables a body. |
| An undeclared key, or any field of a card whose `$kind` is unknown | `qm.data.at("logo", default: none)` | No schema fills it, so absence is real. `quill.validate(doc)` warns on it (`validation::unknown_field`, `validation::unknown_card`), so a key the plate reads belongs in `Quill.yaml`. |

So a `default:` on a declared field is dead code, and an `#if "field" in qm.data` guard on one is always true. When a declared field may be left blank, guard its *value*, not its presence:

```typst
#if qm.data.subtitle != "" {
  [Subtitle: #qm.ink(qm.data, "subtitle")]
}
```

If a default belongs anywhere, it belongs in `Quill.yaml` — a `default:` restated in the plate is never read, and silently diverges when the schema's own default changes.

An `enum` needs this most: its blank is `""`, which is never one of its `values:`, so branch over `values ∪ blank` and never let an `else` swallow the blank into a variant nobody picked.

```typst
#if qm.data.seal != "" { .. }        // the blank means "no seal", not the first value
```

### Blank values

What an unanswered field holds when it reaches the plate, and the guard that tests for it:

| Declared type | Blank in the plate | Guard |
|---|---|---|
| `enum` | `""` | `qm.data.f != ""` |
| `enum` with `variants:` | `(value: "")` | `qm.data.f.value != ""` |
| `text`, `richtext` | `""` (an authored value arrives as content) | `qm.data.f != ""` |
| `date`, `datetime` | `none` | `qm.data.f != none` |
| `integer`, `number` | `0` | `qm.data.f != 0` |
| `boolean` | `false` | `qm.data.f` |
| `array` | `()` | `qm.data.f.len() > 0` |
| `object` | a dictionary of its properties, each at its own blank | guard the properties |
| `matrix` | `(:)`, nothing held | `"flight_cc" in qm.data.f`, or `row.held` per [`roster`](#matrices) row |
| any optional type (`integer?`, `boolean?`, …) | `none` | `qm.data.f != none` |

`$body` follows the content rule: `qm.data.at("$body", default: "") != ""` is true only when the body has text.

The `0` and `false` guards cannot tell an unanswered field from an authored `0` or `false`. Where the plate must, declare the field [optional](quill-yaml-reference.md#optional-fields-t): it arrives as `none` when nobody answered it:

```typst
#if qm.data.quorum == none [
  _Quorum not recorded._
] else if present >= qm.data.quorum [
  A quorum of #qm.data.quorum was established.
]
```

Printing `none` places nothing, and `+` treats it as absent (`"Dear " + none` is `"Dear "`), so neither needs a guard. Arithmetic, comparison, `if`, and `for` reject `none`: branch on `!= none` before them.

### Body, arrays, and cards

The document body is exposed under the `$body` key, accessed via `qm.data.at("$body")` because Typst identifiers exclude `$`. Arrays come through as Typst arrays. Cards live under the `$cards` key, each carrying its own `$kind` discriminator, fields, and `$body`:

```typst
#qm.data.at("$body", default: "")

#for author in qm.data.authors [- #author]

#for card in qm.data.at("$cards", default: ()) {
  if card.at("$kind", default: none) == "product" {
    [Product: #card.name — #card.at("$body", default: "")]
  }
}
```

A card whose `$kind` the quill does not declare reaches the plate carrying the `$kind` it names and its authored fields verbatim, with no `$body`. The render does not fail on it: `quill.validate(doc)` warns instead. Branch on the kinds the plate knows and let the rest fall through.

### Matrices

A [`matrix`](quill-yaml-reference.md#matrix-a-vocabulary-the-author-ticks) reaches the plate as the members the document ticks, each keyed by id to its columns; a member nobody ticked is absent. `qm.roster(dict, key)` hands the plate the whole vocabulary instead, held or not, so it prints every box without a copy of the roster:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#for row in qm.roster(qm.data, "qualifications") {
  qm.region(row.path)[#(if row.held [☒] else [☐]) #row.title]
  if row.held [ — #qm.ink(row.value, "detail")]
}
```

Each row is `(id, title, held, value, path)`, roster members in roster order and then any items an open matrix's document adds. `value` is the member's columns, `none` where unheld, and `path` its address: claiming it with `qm.region` makes an unticked box clickable in an editor preview. `title` is a `str` on every row. An added item's `title` is a cell of its own, so `qm.ink(row.value, "title")` prints it click-to-edit; a roster member's `title` is the roster's and no address, so guard with `row.held and "title" in row.value`. `dict` is the dictionary that declares the matrix: `qm.data`, a card, or a table row.

### Elements

A `qm-<name>` element renders through the helper's `elements` registry. A plate registers a renderer under the element's name, taking its attributes and its content:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#qm.elements.update(e => e + (stamp: (attrs, body) => block(stroke: red, body)))

#qm.data.at("$body", default: [])
```

The dispatcher reads the registry's final value, so the update may stand anywhere in the plate. An element around nothing, `<qm-sig></qm-sig>`, calls its renderer with a body that draws nothing, `[ ]`, which suits a signature line or a stamp. `attrs` is a dictionary of the element's attributes keyed by name, each value a string with its entities decoded: `note="a &amp; b"` arrives as `"a & b"`. With no renderer registered, the built-in `keep` holds its content on one page, and any other element draws its content alone and warns `typst::unregistered_element` at its field. To draw a name as plain content without the warning, register `(attrs, body) => body` under it.

The warning rides a document-global `metadata` the dispatcher places ahead of such an element. Its label `<__qm_element__>` and `kind: "__qm_element__"` are reserved, and a plate that reads its own config through `query(metadata)` filters to its own elements, as under [form fields](#form-fields).

### Task lists

A task item draws a box before its content, ticked when done. A plate replaces the box by setting the helper's `tasks` state to a renderer taking whether the task is done and the item's content. This one strikes through a done task and draws no box:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#qm.tasks.update(_ => (done, body) => if done { strike(body) } else { body })
```

As with `elements`, the renderer is read from the state's final value, so the update may stand anywhere in the plate.

### Footnotes, equations and definition lists

A footnote lowers to Typst's `footnote`, so `#set footnote(..)` and `#set footnote.entry(..)` style it. An equation lowers to Typst math, inline or on its own line, so `#set math.equation(..)` and `#show math.equation: ..` reach it. A definition list lowers to Typst's `terms.item`, one per term holding every description under it, so `#set terms(..)` and `#show terms.item: ..` reach it.

Typst sets an equation in `New Computer Modern Math` and lays one out only in a font with a math table. A quill shipping no font of that family has the backend's embedded Noto Sans Math answer to the name, as well as to its own, so equations draw whatever text fonts the quill ships; a plate naming `New Computer Modern Math` gets that sans face. To set them in a math font of its own, a quill bundles it under `assets/fonts/` and names it:

```typst
#show math.equation: set text(font: "Libertinus Math")
```

### Tables

A table in content lowers to a call of Typst's `table`, so `#set table(..)` and `#show table: ..` reach it. The call always passes `columns`, and passes `align` where the document aligns a column, so a set rule's `columns` never applies and its `align` applies only to a table aligning no column. A `qm-table` wrapper's `widths` lower to `fr` tracks, `auto` for an auto-fit column, and its `align` places the table with `align(..)` without moving the text in its cells. A `qm-cell` pair's `align` and `valign` lower to `table.cell(align: ..)`, `middle` as `horizon`, which Typst combines with the column's alignment, so `valign="bottom"` in a right-aligned column draws at the bottom right.

A `qm-table` wrapper's `headless` lowers the first row as a body row, with no `table.header`. A show rule on `table.header` does not reach its cells, so a plate styling the header row reads whether the table has one:

```typst
#show table: it => if it.children.any(c => c.func() == table.header) {
  show table.cell.where(y: 0): set text(weight: "bold")
  it
} else { it }
```

## Modules

A plate imports the quill's other `.typ` files by the paths Typst resolves: a
bare path from the importing file, a `/`-rooted one from the quill root.
`packages/` is the exception; its files load under their package spec.

```
my-quill/
├── plate.typ          #import "parts/header.typ": header
├── theme.typ
└── parts/
    └── header.typ     #import "/theme.typ": accent
```

A module reads the data by importing the helper itself, as a module like the
plate does. Its prints keep their click targets under the rules a plate's do
([Editor Regions](editor-regions.md)).

## Typst Packages

Quillmark never downloads a package. A plate imports only packages the quill
vendors under `packages/`, one directory each, and an `#import` of any other
fails the render with `typst::file_not_found`.

```
my-quill/
└── packages/
    └── my-helper/
        ├── typst.toml
        └── lib.typ
```

The package's `typst.toml` names the spec the plate imports. The directory
name plays no part in it:

```toml
[package]
name = "my-helper"
version = "0.1.0"
entrypoint = "lib.typ"
```

```typst
#import "@local/my-helper:0.1.0": letterhead
```

| Manifest key | Required | Meaning |
|---|---|---|
| `name` | yes | The `<name>` in `@<namespace>/<name>:<version>` |
| `version` | yes | `major.minor.patch` |
| `entrypoint` | yes | The file the import evaluates, relative to the package directory |
| `namespace` | no | Defaults to `local` |

No other key is read, `compiler` included: a package needing a newer Typst than the [embedded one](#typst-version) loads, then fails at the first feature that Typst lacks.

A package's own `#import`s resolve the same way, so every package it depends on
is vendored beside it. A package copied from Typst Universe ships a manifest
with no `namespace`, so it loads as `@local/…`, while its sources import their
dependencies as `@preview/…`. Adding `namespace = "preview"` to its manifest and
to each dependency's lets the plate and the package import them as published.

A `packages/<dir>/` whose `typst.toml` is absent, does not parse, or lacks a
required key is skipped at load with a `typst::package_manifest` warning, and
an entrypoint the package does not ship warns `typst::package_entrypoint_missing`.
Either way, the plate's `#import` for it then fails as `typst::file_not_found`,
and the failed render carries the warning after that error.

## Fonts

A quill carries its own fonts. The backend loads every `.ttf`, `.otf`, `.ttc` and `.otc` under `assets/fonts/` and inside vendored `packages/`. A `.ttc` or `.otc` collection loads every face it holds, up to 256; one declaring more loads none and warns `typst::font_collection_skipped`. An asset font wins a family a package also ships. A quill bundling none renders in the embedded Roboto, upright and italic, at any weight from 100 to 900. A character no loaded font covers draws as an empty box, and the render warns `typst::missing_glyph` naming it.

An app can add a font library when it builds its engine: `Engine::with_fonts` in Rust, the `fonts` option of `new Engine(..)` in JavaScript, `Engine(fonts=..)` in Python, `--font` on the CLI. Each file is a `.ttf`, `.otf`, `.ttc` or `.otc`; any other format, WOFF and WOFF2 included, fails the engine with `engine::font_unsupported`. The machine's installed fonts are never read, so `#set text(font: "Arial")` names a family the compile cannot find unless the quill ships it.

A library's fonts set only text a writer marks with a family. A plate naming a library family, a bold or italic the quill lacks, and a character no quill font covers never reach a library face, so text without a mark renders the same in every app. A family name starting with `qm-host:` is reserved.

Equations draw in the embedded Noto Sans Math unless the quill ships a math font of its own (see [Footnotes, equations and definition lists](#footnotes-equations-and-definition-lists)).

To bundle fonts with the quill, drop them in `assets/fonts/`:

```
my-quill/
└── assets/
    └── fonts/
        ├── CustomFont-Regular.ttf
        └── CustomFont-Bold.ttf
```

Then reference them by family name (`#set text(font: "CustomFont")`).

### Font runs in content

Text a writer sets in a family, a [`qm-span`](../authoring/markdown-syntax.md#font-family) pair, lowers through the helper. The family goes ahead of the font list in force, so a character it lacks falls through to the plate's font, and a family the app's library lacks keeps the plate's font and warns `typst::unknown_font_family`. A writer picks only from the app's library: the quill's own fonts set the plate alone. Code spans and equations keep their own font. Ship every face the plate names, so the quill renders as designed with no font from the app; a family a writer picks deviates from that default.

## Images

A plate draws a file under `assets/` by its path, which resolves like an
import's: `"assets/logo.svg"` from a plate at the quill root, `"/assets/logo.svg"`
from any file.

```typst
#image("/assets/logo.svg", width: 2cm)
```

**A markdown image in a `richtext` field draws nothing.** `![logo](assets/logo.svg)` in document content reaches no page, and the render warns under `backend::declined_construct`, naming the field and how many images it holds; `quill.validate(doc)` warns the same.

The backend resolves such a url under no reading: not as a file in this quill, a path beside the document, or a remote address. A document is portable across every version a `$quill` selector admits, so a path into one quill's file tree is not a binding it can take. The construct still stores and round-trips; only the page declines it.

## Typesetting

Plate authors style output with Typst's standard `#set` directives:

```typst
#set page(paper: "us-letter", margin: 1in, numbering: "1")
#set text(font: "Figtree", size: 11pt, lang: "en")  // the quill ships assets/fonts/Figtree-Regular.ttf
#set par(justify: true, leading: 0.65em)
```

See the [Typst tutorial](https://typst.app/docs/tutorial/) for the full styling vocabulary. For a worked plate that combines data access with real layout, read [A larger plate](creating-quills.md#a-larger-plate); for larger ones, the `plate.typ` of the `usaf_memo` and `taro` quills in `crates/fixtures/resources/quills/`.

## Form Fields

`form-field` drops an AcroForm widget at its call site: a clickable field in PDF, reserved invisible layout space in SVG and PNG. It backs two widget kinds, text inputs and signature boxes. A plate wanting an interactive checkbox or dropdown is an `acroform` quill.

```typst
#import "@local/quillmark-helper:0.2.0" as qm
```

Value binding is the plate author's job: pass `value:` straight from your data; there is no resolver on the Typst side.

`signature-field` is the ten-line wrapper for the common case — it is exactly `qm.form-field(name, type: "signature", width: width, height: height, field: field)`, with `height` defaulting to `50pt` instead of `20pt`:

```typst
Approving authority:
#qm.signature-field("approver")

Witness:
#qm.signature-field("witness", width: 220pt, height: 60pt)
```

### Positioning

A widget is ordinary Typst inline content sized `width × height`, laid out the way `#box(width: 200pt, height: 50pt)` would be: it sits on the line it is called on, and the line grows to its height. Two modes:

**In-flow (reserves layout space).** Drop the call where you want to claim that space and let the rest of the document flow around it. A paragraph break on each side of the call gives the widget a line of its own:

```typst
Sign here:

#qm.signature-field("approver")  // its own paragraph: 200×50pt below the label

The above signature acknowledges receipt.
```

**Overlay (no displacement).** Wrap in `#place(...)` to anchor the widget without consuming flow. This is what you want when the surrounding template *already* reserves space, for example, the four blank lines above a typed-name signature block in a USAF memo:

```typst
// At the cursor position where the typed-name signature block begins:
#place(dx: 0pt, dy: -3.5in,
       qm.signature-field("approver", width: 3in, height: 0.5in))
```

`#place` without an alignment argument anchors the widget at the current cursor (then offsets by `dx`/`dy`); `#place(top + left, ...)` anchors to the containing block's top-left. Either way, the call consumes no flow space and the surrounding template stays put.

Inside `#box`, `#table`, `#figure`, `#footnote`, `#move`, `#pad`: a widget tracks the layout system normally. Multi-page documents work; each field's `page` is the page it lays out on, not where it was written in source.

### Parameters

| Name | Type | Default | Notes |
|---|---|---|---|
| `name` | `str` | required (positional) | Widget `/T` name: unique within the document, matching `[A-Za-z0-9_.]+` (`.` allowed for fully-qualified names). One uniqueness domain, `signature-field` calls included. |
| `type` | `str` | `"text"` | `"text"` or `"signature"`. |
| `value` | per type | `none` | The delivered field value; interpretation depends on `type` (see below). |
| `multiline` | `bool` | `false` | Toggles the multi-line flag for `type: "text"`; ignored otherwise. |
| `width` | `length` | `200pt` | Absolute length (`pt`/`mm`/`cm`/`in`); relative lengths (`2em`, `50%`) are rejected. |
| `height` | `length` | `20pt` | Same constraint as `width`. |
| `field` | `str` or `none` | `none` | Schema-field address this widget's region is keyed on (see "Binding to a schema field"). |
| `font` | `str` | `"helvetica"` | `"helvetica"`, `"times"`, or `"courier"`; `"text"` only (see "Styling the value text"). |
| `size` | `length` or `auto` | `auto` | Absolute length, or `auto` for the viewer's fit-to-box. `"text"` only. |
| `align` | `str` | `"left"` | `"left"`, `"center"`, or `"right"`. `"text"` only. |

`signature-field` takes `name`, `width`, `height` and `field`, and forwards them; the rest are `form-field`'s alone.

### The two field types

`value:` is forwarded verbatim; the Rust adapter maps it to the AcroForm value per `type`:

**Text**: `value` is a string, a number, a boolean, or `none`, so it takes a data field: `enum`, `integer`, `number`, or `boolean`, or a `text` field through `qm.str(..)`. A number or boolean stringifies, and a blank value emits no `/V`. A `text` or `richtext` value reaches the plate as content and a `date` or `datetime` value as a `datetime`, and passing either as it is fails the render. Set `multiline: true` for a multi-line box.

```typst
#qm.form-field("full_name", type: "text", value: qm.str(qm.data.name))
#qm.form-field("bio", type: "text", value: qm.str(qm.data.bio), multiline: true, height: 80pt)  // bio: { type: text, multiline: true }
```

**Signature**: `value` is ignored. PDF output gains a clickable SigField widget at the call site, which Acrobat — or any reader that supports form signing — presents as a "Sign Here" affordance.

```typst
#qm.form-field("approver", type: "signature", height: 50pt)
```

**The widget is unsigned.** Quillmark performs no cryptography: to produce a signed PDF, run the output through pyHanko, Acrobat, endesive, or another signing tool.

### Binding to a schema field

By default a widget's only identity is its `/T` name. Pass `field:` to additionally key the widget's region on a schema-field address, so it surfaces in the geometry sidecar (`session.regions()`) and resolves under `session.fieldAt(...)`:

```typst
#qm.form-field("Signature", type: "signature", field: "signature_block")
```

`field:` is **region-only**: the `/T` widget name stays `name`; only the sidecar entry keys on `field:`. The address must be a real schema field: a bare field name, an array element like `"refs.2"`, a container property like `"classification.poc"`, or a card path built from the card's `$path` prefix (a bad address raises a Typst assert). Omit `field:` and the widget exposes no region: a click has no schema field to route to.

A one-step suffix is checked against what the field actually offers, so `"refs.2"` needs an `array` and `"classification.poc"` a container — an `object` field, or an `enum` declaring `variants:`, whose cells and `value` discriminant address alike. `"subject.0"` and `"subject.poc"` are both rejected on a scalar `subject`.

### Styling the value text

A widget carrying a `value:` bakes it into an appearance stream, and a viewer re-synthesizes that appearance when someone fills the field. `font`, `size`, and `align` are what both read — except that the baked stream always draws from the box's left edge, so `align` moves the value only once a viewer re-synthesizes. All three apply to `"text"` only, a signature field having no variable text, and passing a non-default there raises an assert rather than silently doing nothing.

```typst
#qm.form-field("memo_date", type: "text", field: "date",
            font: "times", size: 12pt, align: "right", width: 1.2in, height: 16pt)
```

**`size` is worth setting whenever the value has to match surrounding text.** The default `auto` is the AcroForm auto-size, which fits text to the box *and refits as the user types*, so a long value renders smaller than a short one in the same field. An explicit size is the only way to make the rendered size predictable.

**`align` is the only way to pin a value to an edge.** A fillable box has to be sized for the longest plausible value, not the value actually typed, so its width says nothing about where the text lands: under the default `"left"` the value starts at the box's left edge and the leftover space trails off to the right. Right-aligning the box in Typst does not help, because that moves the box, not the text inside it. Reach for `align: "right"` wherever a template calls for a right-aligned fill-in, as a USAF memo does for its date.

**`font` is limited to the three base-14 families** (`"helvetica"`, `"times"`, `"courier"`), which every PDF viewer is required to have. A widget cannot carry a font program, so a quill's own bundled fonts are not reachable here; pick the base-14 family closest to the surrounding type. `"times"` is a close match for the Times-alike faces most formal templates use.

These affect the PDF only. SVG and PNG reserve the same invisible layout space regardless.

### Errors

- Duplicate `name` across any `form-field`/`signature-field` calls → `typst::duplicate_form_field`.
- A non-absolute `width`/`height`/`size`, a `type` outside the two values, a `font`/`align` outside its set, a name violating `[A-Za-z0-9_.]+`, or a `field:` that is not a known schema address → a Typst assert pointing at `form-field`.
- `font`/`size`/`align` set to a non-default on a `"signature"` field → a Typst assert.

The label `<__qm_field__>` and metadata `kind: "__qm_field__"` are reserved for this hand-off: don't use them for unrelated metadata in your plate.

> A widget emits a document-global `metadata` element (standard Typst
> introspection). If your plate or its packages read config via
> `query(metadata)`, filter to your own elements rather than assuming a single
> or last metadata element.

## Output Formats

PDF renders as a single artifact. SVG and PNG render one artifact per page.

Python binding (rendering lives on the engine, not the quill):

```python
from quillmark import Engine, OutputFormat
engine = Engine()
result = engine.render(quill, doc, OutputFormat.PDF)   # or .SVG, .PNG
```

WASM/JS binding (rendering lives on the engine, not the quill):

```javascript
await engine.render(quill, doc, { format: 'png' });           // 144 PPI
await engine.render(quill, doc, { format: 'png', ppi: 300 });  // print quality
```

PNG resolution is set via the `ppi` option (default **144**, 2× at 72pt/inch, suitable for retina previews):

| PPI | Use case |
|-----|----------|
| 72  | Low-res web thumbnails |
| 144 | Retina screen preview (2×) |
| 192 | High-DPI screen display |
| 300 | Standard print quality |
| 600 | High-quality print / archival |

## Iterating on a Plate

The plate imports a helper Quillmark generates per render, so Typst's own tools
cannot compile it from the quill alone. `quillmark workspace` writes that helper
for one document, beside the quill's packages and fonts, and prints the
`typst watch` command that compiles the plate from them:

```bash
quillmark workspace ./my-quill input.md -o ws --today 2026-03-14
typst watch --root ./my-quill --package-path ws/packages --font-path ws/fonts --ignore-system-fonts --ignore-embedded-fonts --creation-timestamp=1773489600 ./my-quill/plate.typ ws/plate.pdf
```

Edits to the plate recompile on save. The helper holds `input.md`'s data, so
rerun `workspace` after changing the document. `--creation-timestamp` carries
the render date, so `datetime.today()` agrees with the render's. Tinymist takes
the same flags through `tinymist.typstExtraArgs`, for completion on `qm.data.`
fields and a live preview.

Typst reads more of the quill than a render does: any file under `--root`, a
vendored package's files by their paths, and a package the quill does not
vendor, and draws an equation only in a math font the quill ships, the
embedded one being Quillmark's alone. A plate that compiles here can still fail
to render; the
[CLI reference](../cli/reference.md#workspace) lists what each one loads. The
command also runs the installed `typst`, whose release can differ from the one
`quillmark --version` names.

## Resources

- [Typst Documentation](https://typst.app/docs/), which describes the newest Typst release; [Typst Version](#typst-version) names the one plates compile against

## Next Steps

- [Create your own Typst quill](creating-quills.md)
- [Editor Regions](editor-regions.md): keeping plate ink clickable in a live preview
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
