# Typst Backend

The Typst backend generates PDF, SVG, and PNG documents using the [Typst](https://typst.app/) typesetting system. It converts card-yaml payload fields to Typst markup, injects them into the plate via a generated helper package, and compiles to the requested format.

## Data Access

Plates are plain Typst code. Document metadata reaches the plate as a Typst dictionary literal exposed by the virtual `@local/quillmark-helper` package:

```typst
#import "@local/quillmark-helper:0.1.0": data

#data.title                                  // a declared field: always present
#data.at("logo", default: none)              // an undeclared key: may be absent
```

Every field arrives at its **native** Typst type — a `date` as a `datetime`, a number as an int or float, an `object` as a dict — with one exception: `richtext` and `plaintext` arrive as Typst content, their text already lowered to markup, because the authored text *is* their rendering. This holds at every depth: a `date` declared inside an `object` or an `array` row is the same `datetime` a top-level one is.

### Dates

A present `type: date` / `type: datetime` field is a native `datetime`; a blank date is `none` (so `#if data.field != none` guards are unchanged):

```typst
#data.issued.display("[day padding:none] [month repr:long] [year]")  // native string
#data.issued.year()                                                   // components: int
#data.issued < data.due                                               // comparison, arithmetic
#some-package(date: data.issued)                                      // any datetime-consuming package
#display("issued", "[day padding:none] [month repr:long] [year]")     // rendered, click-to-edit
#if data.issued != none { .. }                                        // presence
```

Everything except the last two is ordinary Typst, because the value is an ordinary `datetime`. `display(field, ..args)` takes the field's *schema address* rather than its value, and prints the date as `datetime.display` would with the same patterns; an unknown address fails the render, and a blank date gives `none`. Reach for `data.<field>` whenever you want the value itself: math, comparison, components, or handing it to a package. The two print the same ink and differ only in [editor previews](editor-regions.md#dates-display-and-data), where `display` keeps the printed date clickable.

### Which accessor to reach for

A key's *declaration* decides whether it can be absent, and that decides the accessor. Three cases, no judgement calls:

| Key | Accessor | Why |
|---|---|---|
| A field declared in `Quill.yaml` | `data.subtitle` | Always present: compilation blank-fills every declared field with its authored value, else the schema `default:`, else the field's [blank](#blank-values). |
| A `$`-sigiled key (`$kind`, `$body`, `$cards`, `$path`) | `data.at("$body", default: "")` | Typst identifiers exclude `$`, *and* `$`-metadata is present only where it is defined: `$kind` only on a card that authors one, `$body` only where the kind enables a body. |
| An undeclared key, or any field of a card whose `$kind` is unknown | `data.at("logo", default: none)` | No schema fills it, so absence is real. |

So a `default:` on a declared field is dead code, and an `#if "field" in data` guard on one is always true. When a declared field is optional, guard its *value*, not its presence:

```typst
#if data.subtitle != "" {
  [Subtitle: #data.subtitle]
}
```

If a default belongs anywhere, it belongs in `Quill.yaml` — a `default:` restated in the plate is never read, and silently diverges when the schema's own default changes.

An `enum` needs this most: its blank is `""`, which is never one of its `values:`, so branch over `values ∪ blank` and never let an `else` swallow the blank into a variant nobody picked.

```typst
#if data.seal != "" { .. }        // the blank means "no seal", not the first value
```

### Blank values

What an unanswered field holds when it reaches the plate, and the guard that tests for it:

| Declared type | Blank in the plate | Guard |
|---|---|---|
| `string`, `enum` | `""` | `data.f != ""` |
| `enum` with `variants:` | `(value: "")` | `data.f.value != ""` |
| `richtext`, `plaintext` | `""` (an authored value arrives as content) | `data.f != ""` |
| `date`, `datetime` | `none` | `data.f != none` |
| `integer`, `number` | `0` | `data.f != 0` |
| `boolean` | `false` | `data.f` |
| `array` | `()` | `data.f.len() > 0` |
| `object` | a dictionary of its properties, each at its own blank | guard the properties |
| `matrix` | every member, each with `held: false` | `m.held` per member |

`$body` follows the content rule: `data.at("$body", default: "") != ""` is true only when the body has text.

### Body, arrays, and cards

The document body is exposed under the `$body` key, accessed via `data.at("$body")` because Typst identifiers exclude `$`. Arrays come through as Typst arrays. Cards live under the `$cards` key, each carrying its own `$kind` discriminator, fields, and `$body`:

```typst
#data.at("$body", default: "")

#for author in data.authors [- #author]

#for card in data.at("$cards", default: ()) {
  if card.at("$kind", default: none) == "product" {
    [Product: #card.name — #card.at("$body", default: "")]
  }
}
```

A card block with no `$kind:` line is a *kindless* card: it reaches the plate carrying its authored fields verbatim and no `$kind`, so a bare `card.at("$kind")` panics on it. A card whose `$kind` the quill does not declare reaches the plate the same way, carrying the `$kind` it names. Neither carries `$body`, and the render does not fail on either: `quill.validate(doc)` warns instead. Read the discriminator with a default and let unrecognized kinds fall through.

## Typst Packages

Declare packages in `Quill.yaml`, then `#import` them from the plate:

```yaml
typst:
  packages:
    - "@preview/appreciated-letter:0.1.0"
```

```typst
#import "@local/quillmark-helper:0.1.0": data
#import "@preview/appreciated-letter:0.1.0": letter

#show: letter.with(sender: data.sender, recipient: data.recipient)
```

Browse the full catalog at [Typst Universe](https://typst.app/universe/).

A package vendored into the quill under `packages/<dir>/` carries its own
`typst.toml`, which names the spec the plate imports:

```toml
[package]
name = "my-helper"
version = "0.1.0"
entrypoint = "lib.typ"
```

A `packages/<dir>/` without one is skipped at load with a
`typst::package_manifest` warning, and the plate's `#import` for it then fails
as an unresolved file.

## Fonts

A Quill carries its own fonts. The backend loads every `.ttf` and `.otf` under `assets/fonts/` and inside vendored `packages/`, an asset font winning a family a package also ships. A Quill bundling none renders in the embedded Figtree faces. The host's installed fonts are not among them, so `#set text(font: "Arial")` names a family the compile cannot find.

To bundle fonts with the Quill, drop them in `assets/fonts/`:

```
my-quill/
└── assets/
    └── fonts/
        ├── CustomFont-Regular.ttf
        └── CustomFont-Bold.ttf
```

Then reference them by family name (`#set text(font: "CustomFont")`).

## Images

A plate draws a file under `assets/` by its path from the Quill root:

```typst
#image("assets/logo.svg", width: 2cm)
```

**A markdown image in a `richtext` field draws nothing.** `![logo](assets/logo.svg)` in document content reaches no page, and the render warns under `backend::declined_construct`, naming the field and how many images it holds.

What such a url names — a file in this Quill, a path beside the document, a remote address — is undecided. A document is portable across every version a `$quill` selector admits, so a path into one Quill's file tree is not a binding it can take. The construct still stores and round-trips; only the page declines it.

## Typesetting

Plate authors style output with Typst's standard `#set` directives:

```typst
#set page(paper: "us-letter", margin: 1in, numbering: "1")
#set text(font: "Linux Libertine", size: 11pt, lang: "en")
#set par(justify: true, leading: 0.65em)
```

See the [Typst tutorial](https://typst.app/docs/tutorial/) for the full styling vocabulary. For a worked plate that combines data access with real layout, read [A second plate](creating-quills.md#6-a-second-plate); for larger ones, the `plate.typ` of the `usaf_memo` and `taro` quills in `crates/fixtures/resources/quills/`.

## Form Fields

`form-field` drops an AcroForm widget at its call site: a clickable field in PDF, reserved invisible layout space in SVG and PNG. It backs two widget kinds, text inputs and signature boxes. A plate wanting an interactive checkbox or dropdown is an `acroform` quill.

```typst
#import "@local/quillmark-helper:0.1.0": form-field, signature-field
```

Value binding is the plate author's job: pass `value:` straight from your data; there is no resolver on the Typst side.

`signature-field` is the ten-line wrapper for the common case — it is exactly `form-field(name, type: "signature", width: width, height: height, field: field)`, with `height` defaulting to `50pt` instead of `20pt`:

```typst
Approving authority:
#signature-field("approver")

Witness:
#signature-field("witness", width: 220pt, height: 60pt)
```

### Positioning

A widget is ordinary Typst inline content sized `width × height`. It participates in layout the same way `#rect(width: 200pt, height: 50pt)` would: content after it gets pushed by the box's dimensions. Two modes:

**In-flow (reserves layout space).** Drop the call where you want to claim that block of space and let the rest of the document flow around it:

```typst
Sign here:
#signature-field("approver")  // reserves 200×50pt below the label
The above signature acknowledges receipt.
```

**Overlay (no displacement).** Wrap in `#place(...)` to anchor the widget without consuming flow. This is what you want when the surrounding template *already* reserves space, for example, the four blank lines above a typed-name signature block in a USAF memo:

```typst
// At the cursor position where the typed-name signature block begins:
#place(dx: 0pt, dy: -3.5in,
       signature-field("approver", width: 3in, height: 0.5in))
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

**Text**: `value` is a string (numbers stringify). A blank value emits no `/V`. Set `multiline: true` for a multi-line box.

```typst
#form-field("full_name", type: "text", value: data.name)
#form-field("bio", type: "text", value: data.bio, multiline: true, height: 80pt)
```

**Signature**: `value` is ignored. PDF output gains a clickable SigField widget at the call site, which Acrobat — or any reader that supports form signing — presents as a "Sign Here" affordance.

```typst
#form-field("approver", type: "signature", height: 50pt)
```

**The widget is unsigned.** Quillmark performs no cryptography: to produce a signed PDF, run the output through pyHanko, Acrobat, endesive, or another signing tool.

### Binding to a schema field

By default a widget's only identity is its `/T` name. Pass `field:` to additionally key the widget's region on a schema-field address, so it surfaces in the geometry sidecar (`session.regions()`) and resolves under `session.fieldAt(...)`:

```typst
#form-field("Signature", type: "signature", field: "signature_block")
```

`field:` is **region-only**: the `/T` widget name stays `name`; only the sidecar entry keys on `field:`. The address must be a real schema field: a bare field name, an array element like `"refs.2"`, a container property like `"classification.poc"`, or a card path built from the card's `$path` prefix (a bad address raises a Typst assert). Omit `field:` and the widget exposes no region: a click has no schema field to route to.

A one-step suffix is checked against what the field actually offers, so `"refs.2"` needs an `array` and `"classification.poc"` a container — an `object` field, or an `enum` declaring `variants:`, whose cells and `value` discriminant address alike. `"subject.0"` and `"subject.poc"` are both rejected on a scalar `subject`.

### Styling the value text

A widget carrying a `value:` bakes it into an appearance stream, and a viewer re-synthesizes that appearance when someone fills the field. `font`, `size`, and `align` are what both read — except that the baked stream always draws from the box's left edge, so `align` moves the value only once a viewer re-synthesizes. All three apply to `"text"` only, a signature field having no variable text, and passing a non-default there raises an assert rather than silently doing nothing.

```typst
#form-field("memo_date", type: "text", field: "date",
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
from quillmark import OutputFormat
result = engine.render(quill, doc, OutputFormat.PDF)   # or .SVG, .PNG
```

WASM/JS binding (rendering lives on the engine, not the quill):

```javascript
engine.render(quill, doc, { format: 'png' });           // 144 PPI
engine.render(quill, doc, { format: 'png', ppi: 300 });  // print quality
```

PNG resolution is set via the `ppi` option (default **144**, 2× at 72pt/inch, suitable for retina previews):

| PPI | Use case |
|-----|----------|
| 72  | Low-res web thumbnails |
| 144 | Retina screen preview (2×) |
| 192 | High-DPI screen display |
| 300 | Standard print quality |
| 600 | High-quality print / archival |

## Resources

- [Typst Documentation](https://typst.app/docs/)
- [Typst Universe](https://typst.app/universe/): package directory

## Next Steps

- [Create your own Typst Quill](creating-quills.md)
- [Editor Regions](editor-regions.md): keeping plate ink clickable in a live preview
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
