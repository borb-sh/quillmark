# Quill.yaml Reference

Complete reference for authoring `Quill.yaml` configuration files. For a hands-on introduction, see [Creating Quills](creating-quills.md).

## File Structure

A `Quill.yaml` has these top-level sections:

```yaml
quill:        # Required: quill metadata
  ...

main:         # Optional, main entry-point card: field schemas and optional title/ui/body
  title:      # optional label for the card
  fields:
    ...
  ui:         # optional UI hints
  body:       # optional body-region config (enabled, description, example)

card_kinds:   # Optional: additional composable card kinds
  ...

typst:        # Optional: backend-specific configuration
  ...
```

Root-level `fields:` is not supported; define the main document's field schemas under `main.fields`.

`Quill.yaml` is parsed strictly. Unknown keys in the `quill:` section or the backend's section, unknown top-level sections, malformed `ui:` blocks, and field schemas that can't be parsed all produce errors: they are never silently dropped. Every error is collected in a single pass, so authors see all problems at once; the one exception is a [`requires`](#quill-section) floor above the running Quillmark release, which is reported alone. Run `quillmark validate <quill_dir>` to surface them.

---

## `quill` Section

Every Quill.yaml must have a `quill` section with the quill's metadata.

`quill.name` must be `snake_case` (`^[a-z][a-z0-9_]*$`), at most 64 characters.

| Key              | Type   | Required | Description |
|------------------|--------|----------|-------------|
| `name`           | string | yes      | Unique identifier for the quill |
| `backend`        | string | yes      | Rendering backend (e.g. `typst`) |
| `description`    | string | yes      | Human-readable description of the quill package, for a gallery or a quill picker (non-empty), and the title of a Typst PDF whose render and plate name none. The authoring schema never carries it: the document's label is `main`'s own `title` and `description`. |
| `version`        | string | yes      | `MAJOR.MINOR.PATCH`, bumped by what the change costs a stored document ([Quill Versioning](versioning.md)) |
| `author`         | string | no       | Creator of the quill (defaults to `"Unknown"`) |
| `license`        | string | no       | The quill's license, as an SPDX license expression |
| `homepage`       | string | no       | Where the quill's source or docs live, as an absolute `http` or `https` URL |
| `ui`             | object | no       | Document-level UI metadata |
| `requires`       | string | no       | The oldest Quillmark this quill loads on, as `">=MAJOR.MINOR"` or `">=MAJOR.MINOR.PATCH"` |

`requires` is quoted: a bare `>` opens a YAML folded scalar. A Quillmark release older
than it refuses the quill with one `quill::requires_unmet` error, ahead of every
other check, so keys it has not learned yet do not read as typos; a release from
before `requires` existed reports `requires` itself as `quill::unknown_key`. Any
other spelling, whitespace included, is `quill::invalid_requires`.

`license` is an SPDX license expression on one line, such as `MIT`,
`Apache-2.0 OR MIT` or `GPL-3.0-or-later WITH Classpath-exception-2.0`. Each id
is spelled as the [SPDX License List](https://spdx.org/licenses/) spells it, and
the operators `AND`, `OR` and `WITH` are uppercase. A license off that list is
`LicenseRef-<name>`. Anything else fails the load with `quill::invalid_license`:
`/` for `OR`, `NOASSERTION`, a line break, or a value that is not a string. The
hint names the fix where one is plain, such as `GPL-2.0-or-later` for
`GPL-2.0+`. A deprecated id such as `GPL-3.0` loads, with a
`quill::deprecated_license` warning. Spaces around the expression are dropped.

`homepage` is an absolute `http` or `https` URL naming a host, such as
`https://github.com/org/quills`. A user name or password before the host, a
port past 65535, and an invisible formatting character such as a zero-width
space each fail the load with `quill::invalid_homepage`, as anything else does.

Neither key reaches the plate or the authoring schema; both surface on
`Quill.metadata` and `quillmark info`. They arrive in the same release as
`requires`, so an older Quillmark reports them as `quill::unknown_key` whether or
not the quill sets `requires`.

A backend's own settings live under its backend-named section, not in `quill:`.
The Typst plate's path, for example, is `typst.plate_file`, which defaults to
`plate.typ` (see the [`typst` Section](#typst-section) below).

```yaml
quill:
  name: usaf_memo
  version: "0.1.0"
  backend: typst
  description: Typesetted USAF Official Memorandum
  author: TongueToQuill
```

### Example document

A quill's example document shows it filled in. It is derived from `Quill.yaml`: each field's [`example`](#example) and each body's [`body.example`](#card-level-body), committed over the seeded document (`Quill.example_document()`). Its values are made up and nobody keeps them. Starter content people keep is a template, not an example.

- A field without an `example` is left out and renders its default. A typed table without one gets one row of its columns' examples, a matrix its first member with its columns' examples, and a `variants:` enum the world its `example` names.
- An `example` outranks a card kind's `seed` where both name a field.
- A quill declaring no example has no example document. A file named `example.md` is an ordinary file.
- `quillmark validate` renders it after the empty document and the seeded document, and any diagnostic on it fails the quill, a warning included.

---

## `main` Section

The main document card holds **root-block field schemas** under `main.fields`. Optional `main.description` describes the root card, the authoring schema's own `description` (`quill.description` describes the package and never appears there), and optional `main.title` labels the card as a [card kind's `title`](#title_1) does. Optional `main.ui` sets container-level UI for that card. `quill.ui` is a fallback for `main.ui`, not a merge: any `main.ui` (even an empty `ui: {}`) wins wholesale, and `quill.ui` applies only when `main.ui` is absent.

Field order under `main.fields` **is** display order in UIs: the declaration order of the keys is carried structurally through parsing and schema emission, so consumers walk the fields in key order. There is no `ui.order` knob: to reorder fields, reorder them in `Quill.yaml`. Related fields group under a top-level `object` (see [Grouping](#grouping)).

Field keys must be `snake_case` (`^[a-z][a-z0-9_]*$`). Capitalized field keys are reserved, and `body` is refused (`quill::reserved_field_name`): it is the address of the card's own body.

A card declares at most 1000 fields — the width one card-yaml block carries — and past it the quill fails to load (`quill::too_many_fields`). The count is per card, and only its top-level keys are charged: a nested `properties` map, an array's `items`, and a `variants:` cell set all ride inside the one field that declares them.

```yaml
main:
  fields:
    subject:          # Field name (used as the card-yaml payload key)
      type: text
      description: Be brief and clear.
```

### Field Properties

| Property      | Type              | Required | Description |
|---------------|-------------------|----------|-------------|
| `type`        | string            | yes      | Data type (see [Field Types](#field-types)); a trailing `?` makes the field [optional](#optional-fields-t) |
| `title`       | string            | no       | The field's label, a literal (see [`title`](#title)) |
| `description` | string            | no       | Help text for the writer, carried by the authoring schema as the field's `description` |
| `default`     | matches `type`    | no       | The value the **majority of authors want**. When the cell is omitted, the default is filled in — at any depth, whether or not the container above it was authored — and the authoring schema carries it as the field's `default` (see [`default`](#default)). Declared on a **cell**: a leaf, an `array` or a `matrix`. On an `object` it is a load error (`quill::default_on_namespace`), since its properties hold their own. |
| `example`     | matches `type`    | no       | A made-up value, checked at load as `default` is: it answers nothing, `schema()` exposes it, and the [example document](#example-document) commits it (see [`example`](#example)). On an `object` it is a load error (`quill::example_on_namespace`). |
| `values`      | array of strings  | for `enum` | The closed set of allowed string values: the **choices**. Required on every `enum` field. Declaring `""` is a load error — every enum also accepts its [blank](#the-blank-values-is-for-choices-not-for-the-absence-of-one), which Quillmark supplies. |
| `ui`          | object            | no       | UI rendering hints (see [UI Properties](#ui-properties)) |
| `items`       | object            | for `array` | Element schema for an `array` field (a nested field schema). Required on every array. |
| `properties`  | object            | for `object` | Nested field schemas for an `object` typed dictionary (or an array's `object`-typed `items`). Required on every `object` field. |
| `multiline`   | boolean           | for `richtext` | For `text` and `richtext` only: `true` admits more than one line (`text`) or block structure (`richtext`). Absent or `false`, the field is one line or one paragraph. A `text` value's trailing whitespace drops before the check, the newline a YAML `|` block keeps included. A write refuses a line break inside a `text` value. A stored one renders as a space and warns `validation::line_break_folded`. A `richtext` declares it either way. |

### `title`

The label an editor shows beside the input, and the name a person uses for the
field ("set the point of contact"). Absent, consumers humanize the key
(`memo_for` → "Memo For"), so declare it only where that reads wrong:

```yaml
main:
  fields:
    memo_for:
      type: array
      title: To       # "Memo For" would confuse users unfamiliar with memo conventions
      items:
        type: text
```

The authoring schema carries it as the field's `title`, beside its
`description`, so an agent reading the schema knows the field by the name a
person says.

It is a literal. A `{field}` token in it is a load error
(`quill::title_template`): nothing interpolates it. An array's `items` takes
none (`quill::title_on_items`): the array's own title names the list.

### `default`

One question per field: **does it have a value when nobody types anything?**
`default` is the answer, and the only value a field declares.

```yaml
# The value most authors want.
closing:
  type: text
  default: Sincerely

# No default: the cell stays empty until an author answers it.
classification:
  type: enum
  values: [UNCLASSIFIED, CUI]
```

The authoring schema carries them as:

```json
"closing": { "type": ["string", "null"], "default": "Sincerely" },
"classification": { "type": ["string", "null"], "enum": [null, "", "UNCLASSIFIED", "CUI"] }
```

A blank `default:` (`""`, `[]`, `{}`) is legal and renders as no default does:
the field renders its blank either way.
The schema still carries it and `resolve()` still reports the field's source as
`default`, so an editor reading either may treat the two differently. `0` and
`false` are values, and print.

An unanswered field renders its `default:`, else its
[blank](#the-blank-values-is-for-choices-not-for-the-absence-of-one), else
`none` where the type is [optional](#optional-fields-t).

A sample value is the field's [`example`](#example), not its default. A format hint is `description` text.

Nothing is required. There is no `required:` key: an unanswered field renders,
and `Quill::validate` reports nothing about it.

### `example`

A made-up value showing what an answer looks like. It never answers a cell: an unanswered field still renders its `default:`, else its blank.

```yaml
signer:
  type: array
  items: { type: text }
  description: "Line 1: Name as signed, as in FIRST M. LAST, Capt, USAF. Line 2: Duty title."
  example:
    - JANE Q. DOE, Capt, USAF
    - Chief, Knowledge Operations
```

- It is checked at load as `default` is, under `quill::example_*` codes: its type, an enum's `values`, a date's format, `max`, and a richtext's import (`quill::richtext_example_import`).
- It may sit beside a `default`, and on an optional field.
- It goes on any field at any depth: a dictionary's property, a table's column, a matrix's column, a variant's field. An `object` takes none, its properties holding their own; a `variants:` enum's is the member alone.
- A table's own `example` is its rows; a matrix's is the members it holds.
- `schema()` carries it after `default`, and the authoring schema as the field's `examples`.

### Field Types

| Type       | Notes |
|------------|-------|
| `enum`     | A closed set of string values; requires a `values:` list. Also accepts its [blank](#the-blank-values-is-for-choices-not-for-the-absence-of-one) (`""`), which is not a declared member. The [schema](#reading-the-schema-programmatically) emits `values:` as declared, with no `""` |
| `text`     | Open, **unformatted** text over the canonical content: the same nav/regions as `richtext`, but a literal codec (delimiters stay literal, no markup). One line; add `multiline: true` to admit line breaks. Optional `max:`, a character cap (see [`max`](#max-what-the-page-holds)). A plate that computes with the value reads it as a `str` through [`qm.str`](typst-backend.md#text-as-a-str) |
| `number`   | Numeric scalar (integers and decimals) |
| `integer`  | Integer-only numeric scalar, sized as an `i64`; a literal past that range takes `number` |
| `boolean`  | `true` or `false` |
| `array`    | Ordered list; requires an `items:` element schema. Optional `max:`, the element count the page holds (see [`max`](#max-what-the-page-holds)) |
| `date`     | A strict calendar date `YYYY-MM-DD`, or `today` (see [Dated by the render](#dated-by-the-render-today)); rejects any time component |
| `datetime` | A strict offset-less wall-clock datetime `YYYY-MM-DDThh:mm[:ss]`; rejects offsets, the space separator, fractional seconds, and bare dates |
| `richtext` | Rich, **formatted** prose over a canonical content; backends lower it to the target format. Markdown is its import/export projection. One paragraph; `multiline: true` admits paragraphs, lists and other blocks. Declare `multiline:` either way, `true` or `false`: leaving it out is a load error. Optional `max:`, a character cap |
| `object`   | Structured map; requires a `properties:` map |
| `matrix`   | A vocabulary the author ticks; requires a `members:` roster, and `open: true` lets a document add items. Its value is the set of held members, each an object of the field's `properties:` (see [Matrix](#matrix-a-vocabulary-the-author-ticks)) |

#### Dated by the render: `today`

A certificate or a form issued on the spot is dated by the day it is rendered. A `date` value may be the word `today`, authored in a document or as a `default:`:

```yaml
issued:
  type: date
  default: today   # every document leaving the field unset renders on its render date
```

The document stores `today`, not a date. The render supplies the date: the CLI and the WASM and Python bindings use the local date unless given one (`quillmark render --today 2026-03-14`, `engine.render(quill, doc, opts, "2026-03-14")`, `engine.render(..., today=date(2026, 3, 14))`). A plate's `datetime.today()` returns the same date. A document that writes a date instead pins it.

#### Optional fields: `t?`

An unanswered field reaches the plate as its type's blank: `0` for a number, `false` for a boolean. Those read as answers, so a plate cannot tell "0 votes against" from "nobody entered a tally". Append `?` to the type where it must:

```yaml
quorum:
  type: integer?      # none when unanswered, an integer otherwise
confidential:
  type: boolean
  default: false      # always a boolean
```

| Declaration | Renders when unanswered | The plate reads |
|---|---|---|
| `type: t` | the type's [blank](typst-backend.md#blank-values) | a `t`, but `none` for a `date` or `datetime` |
| `type: t` with `default:` | the default | a `t`, but `none` for a `date` or `datetime` defaulting to `""` |
| `type: t?` | `none` | a `t` or `none` |

- Any cell takes the `?`: every scalar type, `text`, `richtext`, `date`, `enum`, `array` and `matrix`. An `object` and an `enum` with `variants:` do not (`quill::optional_namespace`); mark the fields inside them instead.
- `?` and `default:` are exclusive (`quill::optional_default`): a default answers for the author, so the field would never be `none`.
- An authored value is kept as written: `0`, `false`, `""`, and `[]` are answers. On an `enum?`, `""` is the blank and reads `none`.

Reading an optional field in a plate: [Blank values](typst-backend.md#blank-values).

#### Choosing among `enum`, `text`, and `richtext`

Two questions resolve it:

1. Is the set of allowed values closed? Then it is an `enum`, a stored id the plate branches on.
2. Otherwise the field is open text: should markdown delimiters format it (`richtext`: `*text*` becomes emphasis) or stay literal (`text`: `*text*` stays four characters)?

Then decide the shape: `text` is one line and `richtext` one paragraph, and `multiline: true` admits more.

| Declaration | Holds |
|---|---|
| `{ type: text }` | one line, literal |
| `{ type: text, multiline: true }` | lines, literal |
| `{ type: richtext, multiline: false }` | one formatted paragraph |
| `{ type: richtext, multiline: true }` | formatted paragraphs, lists and other blocks |

A name, a URL or a reference key is `text` too: the plate prints it as content, and where it computes with it (`set document(author: ..)`, `upper`, a comparison) it reads `qm.str(..)`, the value's string ([Text as a str](typst-backend.md#text-as-a-str)).

Any field the plate prints through `qm.ink`, `enum` included, gets a region and click-to-edit in editor consumers ([Print with `ink`](editor-regions.md#print-with-ink)). A content field rides the canonical content model and adds navigation inside its value: a click lands a caret in the text. `text` and `richtext` share that entire stack and the same backend lowering, so they are indistinguishable in an editor and diverge only at emit, where the codec decides whether a delimiter is markup or a character.

Changing a declared type reinterprets every stored value in that field at the next bound load, with no diagnostic, and `text` → `richtext` is the lossy direction: the stored string enters the markdown import and its delimiters are consumed as structure, leaving the literal characters unrecoverable. A declared type change is a new quill version ([Quill Versioning](versioning.md)); audit the corpus before publishing one, as under [Date and Datetime Grammars](#date-and-datetime-grammars).

### Date and Datetime Grammars

The two grammars are **disjoint**: `date` rejects any time component, `datetime` rejects a bare date, and neither truncates. A field holding a mix of `2026-06-01` and `2026-06-01T09:30` has no correct declaration, since whichever type it takes strands the other half. Normalize the values, or split the field in two.

Coercion runs when the document lowers into the backend's data, **upstream of the plate**. A value outside its declared grammar fails the render before any template code executes, so a plate that never mentions the field fails identically and no plate-side coercion repairs it. `quill.validate(doc)` is what names the field and its path. A `date` or `datetime` field is a scalar, so `quill.conform(doc)` passes it in silence: the conform walk covers content fields only.

**Changing a declared date type on a deployed corpus is a corpus operation, not a schema edit.** The stored string is never rewritten: neither the transport door nor `conform` touches a scalar. Nothing is lost, and every document holding a value the new grammar rejects strands at render. Audit before publishing the change: load each stored row through the transport door, read the field, and test it against the target grammar. `reader.get` returns the stored string verbatim for both date types.

```js
const doc = Document.fromStored(row);
const value = quill.reader(doc).get('issued');
if (value && !/^\d{4}-\d{2}-\d{2}$/.test(value)) {
  // Strands under `type: date`. Repair before the schema change ships.
  quill.writer(doc).set('issued', value.slice(0, 10));
}
```

`writer.set` refuses the same values the render does (`edit::field_coercion_failed`), so the repair writes the corrected string rather than the original.

### Enum Constraints

Declare a closed string domain with `type: enum` and a required `values:` list:

```yaml
main:
  fields:
    format:
      type: enum
      values:
        - standard
        - informal
        - separate_page
      default: standard
      description: "Format style for the endorsement."
```

`values:` on any other type is a load error, as is the retired `enum:`
modifier on any type.

A value is an id: it is what a document stores and what the plate reads.
Renaming one stops every document holding it from validating
(`validation::enum_violation`; see [Versioning](versioning.md)). Spell it as a
stable token (`separate_page`, not `Separate page`). A Typst plate prints
display text by mapping each id to its words; a PDF-form dropdown and an
editor's picker list the ids themselves. Where the text is itself the standard,
as a marking such as `UNCLASSIFIED` is, the id is that text.

#### The blank: `values:` is for choices, not for the absence of one

Every `enum` accepts one value beyond its `values:` list — the **blank**, spelled
`""`. Quillmark supplies it; you never declare it, and declaring `""` in
`values:` is a load error (`quill::enum_blank_member`).

That is what a document says when nobody has answered. A field with no `default:`
renders its blank rather than the first variant, so reordering `values:` never
changes what an unanswered document renders, and no reader ever sees a choice
nobody made.

The two keys range over different sets, which is why `""` is rejected in one and
accepted in the other:

```yaml
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI, SECRET]   # the choices — no "" here
      default: ""                           # the blank — a value, and legal
```

`values:` enumerates *choices*. `default:`, your documents, and the projections
all range over `values ∪ blank`. `default: ""` renders as no default does;
whether an author should answer the field is its `description`'s to say.

Where the empty state is itself a decision someone makes and the document should
record it, make it a member — `undecided`, `waived`, `n_a` — not the blank. The
blank means nobody chose; a member means someone chose "none".

Name the blank's label with `ui.blank_title` when a bare empty row would read
badly; absent one, consumers supply their own conventional label.

If you arrive from **web forms**, your prior transfers: this is HTML's
placeholder `<option value="">`, Django's `("", "---------")`, Rails'
`include_blank:`. One caveat — the affordance carries over, the enforcement does
not. There is no `required:`; an unanswered field draws no diagnostic and
renders. If you arrive from **protobuf**, your
prior is a near-miss: proto3 reserves slot 0 *inside* the enum
(`FOO_UNSPECIFIED = 0`), whereas here the sentinel lives outside the domain and
your `values:` list stays clean.

> **Writing a plate against an enum:** branch over `values ∪ blank`
> exhaustively. An `else` fallback silently renders a variant nobody chose, and
> `qm.data.at(key, default: X)` is not a guard — every declared key is always
> present at render, so its `default:` never fires and the blank flows through.

#### Variants: fields that exist only for one choice

Some fields belong to one choice and are meaningless beside any other. Declare
them under `variants:`, keyed by the member that brings them into play:

```yaml
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI, CONFIDENTIAL, SECRET, TOP SECRET]
      default: ""
      variants:
        CUI:
          controlled_by: { type: text }        # exists only on a CUI memo
          poc:           { type: text }
          category:      { type: text, default: "" }
```

The field then holds a **container** instead of a bare string, and a document
writes the choice under `value` with that world's answers beside it:

```yaml
classification:
  value: CUI
  controlled_by: SAF/AA
  poc: Capt J. Smith, DSN 555-1234
```

A world with nothing to fill in still writes plainly — `classification:
UNCLASSIFIED` is accepted and means the same as `{value: UNCLASSIFIED}`. A cell
written beside the field instead of under it (`poc:` at card level next to
`classification: CUI`) is an undeclared key that no declared field reads, and
`quill.validate(doc)` warns `validation::unknown_field` with a hint to nest it.

The container is a *document* shape. The schema's own `default:` names the
discriminant alone (`default: ""`, `default: CUI`); a container-shaped one is a
load error, because each cell in a world carries its literal on its own
declaration.

Two things follow, and they are the reason to reach for this over a
`cui_`-prefixed row of flat fields:

- **The names shorten.** The prefix was hand-written namespacing; nesting
  supplies it structurally, so `cui_poc` becomes `poc`.
- **Editors know.** The schema says which cells are out of play, so a form
  retires them instead of showing a CUI block on an unclassified memo.

**Writing a plate against variants.** The live world's fields arrive only inside
the branch that selects it, which is the branch you already owe every enum:

```typst
..if qm.data.classification.value == "CUI" {
  (controlled_by: qm.data.classification.controlled_by, poc: qm.data.classification.poc)
},
```

Inside that branch every declared field of the world is present and blank-filled,
so no guarded access is needed. Outside it there is nothing to read: an
unanswered `classification` renders `{value: ""}`, and the blank brings no field
set at all.

**Flipping the choice keeps the answers.** Selecting `UNCLASSIFIED` after filling
in a CUI block leaves those values in the document and warns
(`validation::out_of_variant`); they simply stop rendering. Flip back and they
are still there. Remove the field to drop the value for good.

A variant cell is an ordinary field: any type a card field may carry, prose,
dates and containers included, a typed dictionary (`type: object`) among them,
reaching the plate exactly as a card-level one
does. What it cannot carry is `variants:` of its own. `variants:` itself
is valid on a `type: enum` field that is a card's own field or a typed
dictionary's property — `header.classification` reads, binds and branches as
`classification` does, one step down — and nowhere else: an array element, a
matrix column and another variant's cell each refuse one, as does an object
inside any of them. It
keys only on declared members (never `""`, which owns no field set), and cannot
declare a field named `value`. Variant fields sit beside the discriminant,
inside its [group](#grouping) `object` when it is grouped; a `ui.group` on a
variant field is `quill::nested_group_not_supported`. A field set shared by
several members is repeated or shared with a YAML anchor — a variant keys on one
member — but every variant declaring a given name must declare it *identically*,
since the name is one cell of the container whichever world brings it into play
(`quill::variant_field_collision`).

### Primitive Arrays, Typed Tables, and Typed Dictionaries

Every array declares its element type under `items:`. For a **primitive list**, give `items` a scalar type, coercion and validation then apply element-wise (e.g. each element of an `integer[]` is coerced to an integer, and a bad element fails at its indexed path like `counts[1]`):

```yaml
main:
  fields:
    tags:
      type: array
      items:
        type: text
    counts:
      type: array
      items:
        type: integer
    sections:
      type: array
      items:
        type: richtext   # each element's content is lowered to backend markup
        multiline: true
```

For a **typed table** (a list of structured rows) give `items` an `object` type with its own `properties:`. Coercion recurses into each element and converts property values to their declared types:

```yaml
main:
  fields:
    cells:
      type: array
      items:
        type: object
        properties:
          category:
            type: text
          score:
            type: number
```

A document may write a single value where an array is declared: it is a spelling of a one-element array, and the schema is what disambiguates it. `tags: one` renders as `tags: [one]` does, and `quillmark check` passes it. The element is then judged against `items`, so a mapping written for `tags` is a `validation::type_mismatch` at `tags[0]`, while one written for `cells` is its one row. Likewise, a bare boolean, integer, or number written where `text` is declared adopts its canonical scalar token (`true`, `47`, `1.0`; `1.50` reads `1.5`) instead of failing.

Use `type: object` with `properties:` for a single structured mapping:

```yaml
main:
  fields:
    address:
      type: object
      properties:
        street:
          type: text
        city:
          type: text
```

Containers nest freely: a property or an element is an ordinary field, so it carries whatever type a card-level field carries, itself included. `object<array<text>>`, `array<array<integer>>` and a typed table whose row holds a typed dictionary are all declarable, and each leaf is addressable by the schema address its path spells (`contact.address.city`, `grid.0.0` — see [PLATE_DATA.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PLATE_DATA.md#schema-addresses)).

**A dictionary holds no `default:` of its own** — its properties do:

```yaml
address:
  type: object
  # default: { street: "5000 Forbes Ave" }   # quill::default_on_namespace
  properties:
    street: { type: text, default: "5000 Forbes Ave" }
    city:   { type: text }
```

The container spelling would be a second declaration of a value the property
already holds, free to disagree with it. Each property's `default:` is reached whether or not a document authors the container
above it, so writing `address: {}` changes nothing.

An `array` **does** keep its own `default:`: `items:` fixes the
element type but never the arity, so `default: [{…}]` says something no
property declaration can. Each element it supplies is completed
against `items` exactly as an authored element is.

A legacy `ui.group` holds card-level fields only: an object's properties and an array's items cannot join one. `variants:` reaches one step further — a card field or a typed dictionary's property carries one, and an array element, a matrix column and another variant's cell do not (see [Variants](#variants-fields-that-exist-only-for-one-choice)).

### `max`: what the page holds

A form laid out on a page holds a fixed number of rows. Say so with `max:`, and
the limit stops being prose only a human reads:

```yaml
experience:
  type: array
  max: 37                 # the form's 37 ruled lines
  items:
    type: object
    properties:
      duty: { type: text, default: "" }
```

`max:` is a non-negative integer counting what the field holds, in the field's
own unit: elements on an `array`, characters on the text types (below). It is
never a bound on a number's value. A `default:` longer than it fails to load. A document holding more elements draws
a **warning**, `validation::cardinality`, at the field's path with `max` and
`actual` in its args — never an error: the document still renders, and the plate
decides what happens to the surplus. Each declaration carries its own cap, so a
capped array nested inside a row is checked against its own.

The authoring schema carries the cap as `quillmark:maxItems`, an annotation
rather than JSON Schema's asserting `maxItems`, since the cap never refuses.

A `text` or `richtext` field takes `max:` too, counting characters: a record
limit, or a box printed for a fixed number of them.

```yaml
citation:
  type: richtext
  multiline: true
  max: 1350               # the record system's limit
statements:
  type: array
  max: 4
  items: { type: text, max: 350 }   # each statement
```

A character is a Unicode scalar value of the plain text: a richtext's markup
does not count, and neither does a table or an image; a line break counts as
one, and trailing line breaks, or a one-line `text`'s trailing spaces, do not
count. An unquoted number counts as its text, so `ssn: 1234567890` is ten
characters. A longer value draws a **warning**,
`validation::over_length`, at its own path with `max` and `actual`, and renders
as written; a `default:` holding a value longer than its cap, at any depth, or a kind's `seed:` fails to load
(`quill::default_over_length`, `quill::seed_over_length`).
The authoring schema carries it as `quillmark:maxLength`. On an `acroform`
quill the cap is the widget's `/MaxLen`.

There is no `min:`. `min: 1` would be `required:` under another name, and no
field is required ([`default`](#default)); it would
also contradict a sibling `default: []`.

### Matrix: a vocabulary the author ticks

Some fields are neither a list the author grows nor a choice they make once:
they are a fixed vocabulary printed in full, where the author ticks what they
hold and may annotate a tick. `type: matrix` is that shape.

```yaml
qualifications:
  type: matrix
  description: Tick each qualification held.
  members:                      # ordered; `id: Title`
    sq_cc_candidate: Sq/CC Candidate
    flight_cc: Flight CC
    dodin_ops: DODIN Ops
  properties:                   # the columns; omit them for a plain checklist
    detail:
      type: text
      default: ""
```

Member **ids** are snake_case: they are what the wire, the address and the
document speak, while the **title** is display only. The roster is a mapping,
so it has one slot per id and a member cannot be declared twice. `held` and
`title` cannot be column names.

A document ticks sparsely. A member is held by being present:

```yaml
qualifications:
  flight_cc: true                  # held; columns take their defaults
  dodin_ops: { detail: "2024" }    # held, with columns
  sq_cc_candidate: false           # not held, as if absent
```

A mapping is held whatever it carries, `{}` included, and a bare scalar is the
tick. The document never stores `held`: a mapping carrying one fails
validation (`validation::held_stored`), so untick a member by removing it, and
its answers go with it.

A member the roster does not declare is refused, as an out-of-domain `enum`
value is. A mapping has one slot per key, so a member cannot be ticked twice.

The plate receives the held set: each held member's columns, keyed by id, in
roster order. A member the document does not tick is absent, so print the
vocabulary through `qm.roster`, which hands your plate every member, held or not,
without a copy of the roster of its own:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#for row in qm.roster(qm.data, "qualifications") {
  qm.region(row.path)[#(if row.held [☒] else [☐]) #row.title]
  if row.held [ — #qm.ink(row.value, "detail")]
}
```

Each row is `(id, title, held, value, path)`: `value` is the member's columns,
`none` where unheld, and `path` its address. Pass the dictionary that declares
the matrix and its name: `qm.roster(card, "qualifications")` on a card,
`qm.roster(row, "quals")` in a table row.

Three things to author against. Declare a `default:` on each column to say
what a ticked member's unanswered column renders; without one it renders the
column's blank. The matrix is a cell, so a `default:` on it is the set a
document that leaves it out holds, and `type: matrix?` renders an unanswered one
`none`; without either it renders `{}`, nothing ticked. A fresh document ticks
nothing, and the authoring schema shows the vocabulary as the matrix's
properties, one per member, and its roster as `quillmark:roster`. And each member is an
address — `qualifications.flight_cc` is its tick, a region on Typst and a
checkbox on acroform checked by its presence, and
`qualifications.flight_cc.detail` its column.

#### Open rosters: items the author adds

A record rarely fits a fixed vocabulary. `open: true` lets a document add items
beside the roster, each keyed by a snake_case id of the author's choosing and
carrying a `title`:

```yaml
qualifications:
  type: matrix
  open: true
  members: { flight_cc: Flight CC, dodin_ops: DODIN Ops }
  properties:
    detail: { type: text, default: "" }
```

```yaml
qualifications:
  flight_cc: { detail: UMS Flt/CC }
  wing_ig: { title: Wing IG, detail: 81 TRW }   # an added item
```

- An added item is held by being present and takes the same columns a member
  does. Its `title` is a one-line `text`, and a key outside the roster with no `title`
  or a blank one (`""` or whitespace alone), or one that is not a snake_case
  id, is refused (`validation::enum_violation`, hinted toward the added
  spelling).
- The plate receives the held roster members in roster order, then each added
  item sorted by id, `{title, …columns}`, and `qm.roster` returns them in that
  order. A plate wanting another order sorts the rows itself. An added item's
  `title` is a cell with an address, so `qm.ink(row.value, "title")` prints it with
  its click-to-edit region, where `row.title` prints it plainly.
- Quillmark does not compare titles: an added item titled as a roster member
  prints beside it.
- The ids in the authoring schema's matrix properties are all a model sees of
  the roster, so on an open matrix name them as their titles read
  (`joint_qualified_officer`, not `jqo`), or a model adds a duplicate it could
  have ticked.
- An id your roster later declares reads as that member, so an item authors
  added under it becomes the member with no migration; its stored `title` then
  warns as `validation::unknown_field` until removed.
- `open` defaults to `false`, and the acroform backend refuses it at load
  (`quill::open_matrix_unsupported`): a form has no widget for an item a
  document adds.

Full model: [SCHEMAS.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/SCHEMAS.md#matrix); the wire shape is [PLATE_DATA.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PLATE_DATA.md#a-matrix-field).

### Grouping

Related fields group under a top-level `object`. Its `title` and `description` label the group, which an editor frames and the authoring schema carries:

```yaml
main:
  fields:
    subject:
      type: text
    addressing:
      type: object
      title: Addressing
      description: Who the memo is for and from.
      properties:
        memo_for:
          type: array
          items: { type: text }
        memo_from:
          type: array
          items: { type: text }
          default: []
    letterhead:
      type: object
      title: "Letterhead & Seal"
      properties:
        letterhead_title:
          type: text
```

A group is data. The document nests its keys under the group's id, and a plate reads `data.addressing.memo_for`:

```markdown
~~~
$quill: memo@1.0
subject: Annual Records Management Review
addressing:
  memo_for: [123 OG/CC, 123 MXG/CC]
~~~
```

So moving a field between groups is an address change, a MINOR bump like a rename. Fields beside the objects stay ungrouped, and an editor draws them above every group: one declared after a top-level `object` warns `quill::field_after_object`, since the order it was declared in is not the order it draws in. Nothing requires grouping: a flat questionnaire stays a flat `fields:` map. A card has no `sections:` key; one is `quill::invalid_card_schema`, its hint naming this form.

#### Legacy `ui.group`

The older spelling still loads: a `ui.groups` registry on the card, and a `ui.group` reference on each field naming its entry.

```yaml
main:
  ui:
    groups:
      addressing: {}
      letterhead: { title: "Letterhead & Seal" }
  fields:
    memo_for:
      type: array
      items: { type: text }
      ui: { group: addressing }
    letterhead_title:
      type: text
      ui: { group: letterhead }
```

A group here is a frame, not data: the document stores `memo_for:` and a plate reads `data.memo_for`. It warns `quill::deprecated_groups` and loads with ungrouped fields first, then each group's fields in registry order, whatever order they were declared in. To rewrite it as [Grouping](#grouping), make each group an `object` keyed by its id, in registry order, carrying the registry `title:`; move each grouped field into its `properties`; and drop `ui.group` and `ui.groups`.

A group holding one `object` and nothing else needs no wrapper: that object carries the group's `title:` and stands at the top level itself, its value and addresses unchanged. Documents then nest each group's keys under its id, and plates gain the one path segment.

The registry's own checks still apply to the legacy spelling: a reference the registry does not declare (`quill::unknown_group`), a reference with no registry (`quill::implicit_group`), a repeated id (`quill::duplicate_group`), an id that is not snake_case (`quill::invalid_group_id`), and a `ui.group` below card level (`quill::nested_group_not_supported`). The registry is also read from `quill.ui` when `main.ui` is absent.

---

## UI Properties

The `ui` property on fields controls how form builders and wizards render the field. These are UI hints, not validation constraints.

`ui` keys never reach the authoring schema, so a fact a writer needs belongs in
`description` or `title`. A field's label is its [`title`](#title);
`ui` holds none, and `ui: { title: … }` is a load error whose hint names it.
Nor does `ui` carry a field's line shape: that is the field's own `multiline:`,
which validation, the editor and a PDF form all read, and `ui: { multiline: … }`
is a load error.

### field order

Field display order is **declaration order**: the order the keys appear in `Quill.yaml`. This holds at every level: card-level fields, and the properties of a typed dictionary or typed-table row. The order is carried structurally (the schema's field maps preserve key order, and `schema()` re-emits that order), so no per-field knob is involved.

There is no `ui.order` key: an authored `ui: { order: N }` is a load error (`quill::field_parse_error`). To move a field, move its block in `Quill.yaml`.

### `blank_title`

Labels an `enum`'s [blank](#the-blank-values-is-for-choices-not-for-the-absence-of-one) option in a picker. Absent, consumers render a conventional label of their own, so this is only worth setting when a bare empty row would read badly:

```yaml
main:
  fields:
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI, SECRET]
      default: ""
      ui:
        blank_title: "(no marking)"
```

It labels the blank, never a member: `values:` carries no entry for it. Consumers must keep **unset** selectable and re-selectable — returning to it is how an author clears a cell, so a disabled placeholder that vanishes once a choice is made is the wrong idiom. Unset and a stored `""` differ where `default:` names a member: the stored blank is an answer and outranks the default.

### `compact`

Asks an editor to set a card-level field beside its neighbours in one row, where it would take a row of its own. Absent, the field's type decides:

| Packs by default | Full row by default |
|---|---|
| `date`, `datetime`, `enum` (without `variants:`), `integer`, `number`, `boolean` | `text`, `richtext`, `array`, `object`, `matrix` |

A text field packs only under `compact: true`, since its type cannot say whether a value is a rank or a subject line; `compact: false` keeps a packing type on its own row.

```yaml
main:
  fields:
    rank:
      type: text
      ui:
        compact: true
    due:
      type: date
      ui:
        compact: false
```

It is a request: an editor grants it only where a neighbour packs too, so a packing field alone between full-row fields still takes its row.

- An `array`, an `object`, an `enum` with `variants:` and a `multiline: true` `text` or `richtext` never pack, whatever the key says, since their height grows with the document.
- On a `matrix`, `compact: true` sets the roster's members side by side instead.
- The properties of a card-level `object`, and of an `object` nested in it, follow the same rule, so a field moved into a group keeps its row.
- The cells of an array's records, a variant, a matrix member and a typed table are laid out by their container and read no `compact`.

### `layout`

Names the control a field asks an editor to draw, where the shape admits more
than one and the default reads wrong. `table` is valid only on a **typed
table** — an `array` whose `items` is an `object`:

```yaml
main:
  fields:
    tours:
      type: array
      ui:
        layout: table       # a grid: one row per element, one column per property
      items:
        type: object
        properties:
          unit:     { type: text, default: "" }
          duration: { type: integer, default: 0 }
```

Without it, an editor draws a typed table as a stack of collapsed rows that open
one at a time — right for a row of long prose, wrong for four short cells.

Declaring it says two things.

**A contract: every column is a leaf.** A column holding an `array`, an `object`
or a `matrix` addresses below itself and is not one cell, so it is a load
error naming the column — `quill::table_column_not_leaf` on
`appendices[].entries`. Text is a column whatever its `multiline`: how tall a cell
renders is the editor's call, what it contains is not. On any field that is not a
typed table at all, the key is `quill::invalid_ui`.

**A request: draw it as a grid.** An editor may still decline — a width that will
not hold the columns, a cell renderer that will not take a block `richtext` — and
fall back to the record list. What it will not meet is a shape it cannot draw at
all; the contract settled that at load. Nothing else reads the key: the plate,
document validation and the authoring schema are all deliberately inert on it, and `schema()`
echoes it verbatim for the editor to find.

`flat` is deprecated and warns `quill::deprecated_flat`, naming the field: an
editor no longer honors it. A card's top-level `object` is already a panel whose
properties stand flat inside it, and a nested one draws as a subform. It still
loads where it loaded, an `object` other than an array's element, and is
`quill::invalid_ui` anywhere else.

### A row's label

A consumer labels a collapsed row from its own values, the first property
leading, so a list of rows reads as a list of things rather than "Item 1, Item
2". Declare first the property that names the row. `items` takes no `title`.

---

## `card_kinds` Section

`card_kinds` define composable, repeatable content blocks (the *kinds*: a document can then carry zero or more *instances* of each kind, interleaved with body content). Each entry is shaped exactly like `main:` (`fields`, optional `title`, `description`, `ui`, `body`); think of `main:` as the single mandatory card-kind for the document body, and `card_kinds:` as the library of additional kinds that may attach to it.

Card-kind names (the keys under `card_kinds`) must match `[a-z_][a-z0-9_]*` (leading underscore is allowed). `main` is reserved for the document root.

```yaml
card_kinds:
  indorsement:                    # Card-kind name
    description: Chain of routing endorsements.
    fields:
      format:
        type: enum
        values: [standard, informal, separate_page]
        default: standard
      from:
        type: text
```

Invalid card-kind names include:

- `BadCard` (uppercase letters)
- `my-card` (hyphen)
- `2nd_card` (starts with a digit)

### When not to declare one

A card is a **part someone writes**; a repeated record someone fills in is a
**row**, and a row is an `array<object>` field on the card that owns it — not a
card kind. The test is a disjunction: a unit that stands in document order among
units of *other kinds*, **or** whose substance is flowing prose, is a card;
otherwise it is a row.

A kind declaring `body.enabled: false` draws a warning,
`quill::bodiless_card_kind`, pointing at the row shape. It is advice and not a
refusal: no body is only a proxy for "not prose", and a bodiless kind that truly
interleaves — a page break, a rule, an inserted signature block, all of which
exist for their position — is a card by the first clause, which no schema can
state. `main` is never warned: a form has no root prose.

Rows are cheaper where they fit. They nest lexically, so a row belongs to its
parent by position in the file rather than by counting cards in the stream, and
a plate reads `card.rows` instead of reassembling a run. What rows give up is
the fence-native body: a row's prose is a `richtext` cell, one quoted YAML
scalar, which is fine for a few lines and wrong for a memo's worth.

### Card Properties

| Property      | Type   | Required | Description |
|---------------|--------|----------|-------------|
| `title`       | string | no       | The kind's label, a literal (see [`title`](#title_1)) |
| `description` | string | no       | Help text describing the card's purpose |
| `fields`      | object | no       | Field schemas (same structure as top-level fields) |
| `ui`          | object | no       | Container-level UI hints (see [Card-level `ui`](#card-level-ui)) |
| `body`        | object | no       | Body-region config (see [Card-level `body`](#card-level-body)) |
| `seed`        | object | no       | What a new card of this kind starts with (see [Card-level `seed`](#card-level-seed)) |

#### `title`

The kind's label: what a picker, an add-card menu or a section header shows.
Absent, consumers humanize the kind name. The map key (e.g. `indorsement`) is
the `$kind` a document stores, so a title renames freely without invalidating
stored documents.

```yaml
card_kinds:
  indorsement:
    title: Routing indorsement
    description: Chain of routing endorsements.
    fields:
      from:
        type: text
```

The authoring schema carries it as the kind's `title` under `$defs`, beside its
`description`.

It is a literal, naming the kind and never one instance: a `{field}` token is a
load error (`quill::title_template`). A consumer labels an instance from its own
values, so field order decides which value leads: declare first the field that
names the instance.

### Card-level `ui`

| Property | Type   | Description |
|----------|--------|-------------|
| `groups` | list or object | The legacy group registry (see [Legacy `ui.group`](#legacy-uigroup)) |

### Card-level `body`

| Property  | Type   | Description |
|-----------|--------|-------------|
| `enabled`     | bool   | Whether the body editor is enabled (default: true). When false, consumers must not accept or store body content for this card kind. |
| `description` | string | What the body holds, for the writer. |
| `example`     | markdown | A made-up body, which the [example document](#example-document) commits. It must import without dropping anything (`quill::richtext_example_import`). |

#### `body.enabled`

When `false`, the card kind has no body/content area, and an editor should offer none. The authoring schema gives the kind no `body`. A document that provides body content for such a card anyway still renders without it: the plate receives no `$body`, and `quill.validate(doc)` warns with `validation::body_disabled`.

```yaml
card_kinds:
  page_break:
    body:
      enabled: false    # Card has fields only, no body/content area
    fields:
      style:
        type: text
```

Loading this draws `quill::bodiless_card_kind`, which asks whether the kind is a card at all: a repeated record with no prose is a row (see [When not to declare one](#when-not-to-declare-one)). The example above stands because a page break exists for its position in the stream, which no row can occupy. `main` is never warned.

`description` and `example` describe a body, so either beside `enabled: false` is a load error (`quill::invalid_body`), as is any other key under `body`.

#### `body.description` and `body.example`

```yaml
main:
  body:
    description: Plain paragraphs, numbered for you; a `-` item under one becomes a lettered subparagraph.
    example: |
      Each group reviews every office of record it owns by the suspense in the subject line.

      - The file plan lists every series the office keeps.
```

`schema()` carries both, the [example document](#example-document) commits the example as the body, and the authoring schema carries both on the body. An example is made up; starter text a new card keeps goes in its kind's [`seed`](#card-level-seed), and for the document in a template's own body.

### Card-level `seed`

`seed` is what `seed_card` writes into a new card of the kind: fields of the kind, plus an optional `$body`. It is the shape a document's `$seed.<kind>` takes, and a block copies unchanged between the two.

```yaml
card_kinds:
  indorsement:
    seed:
      signature_block:
        - FIRST M. LAST, Rank, USAF
        - Duty Title
      $body: Write the indorsement here.
    fields:
      signature_block:
        type: array
        items: { type: text }
```

- A document's `$seed.<kind>` replaces it whole, `$body` included: a template keeping part of the kind's seed copies that part, and `$seed.<kind>: {}` seeds nothing.
- Once written, a seeded value is the card's own content: it prints, and a later change to `seed` does not reach cards already made. What should render when nobody answers is a `default:` instead, which tracks the quill.
- A placeholder prints until someone replaces it. A format hint belongs in the field's `description:`.
- Each value must pass its field's checks, `$body` needs the body enabled, and a seed that renders as the unanswered field does (an empty `seed: {}`, a `null`, a blank `$body`, the field's `default:`, its blank when it has none, or `{}` for a typed dictionary) is refused (`quill::seed_redundant`). Each failure is a load error under `quill::seed_*`.
- `main` declares no `seed`.

### Using Cards in Markdown

Card kinds defined here are authored as `~~~` blocks (with a `$kind: <kind>` line) in the document body. See [card-yaml Blocks](../authoring/card-yaml.md#card-blocks) for the markdown syntax.

---

## `typst` Section

The Typst backend compiles `plate.typ` at the quill root, found by name as `Quill.yaml` is. The optional `typst` section moves the plate elsewhere.

```
my_quill/1.0.0/
├── Quill.yaml
└── plate.typ      # the plate, with no `typst` section
```

| Key          | Type   | Required | Description |
|--------------|--------|----------|-------------|
| `plate_file` | string | no       | Path from the quill root to the Typst template the backend compiles, steps separated by `/`. Default `plate.typ` |

```yaml
typst:
  plate_file: tpl/main.typ
```

A quill holding no `plate.typ` and declaring no `plate_file` fails to render as `typst::plate_missing`, its hint naming both. Core reads no backend file at load: the backend resolves the plate when it opens a session, and `quillmark validate` checks it.

A `.` step or an empty one drops, so `./plate.typ` names `plate.typ` and `tpl//layout.typ` names `tpl/layout.typ`, the path every diagnostic and `quillmark workspace` give. A leading `/` or a `..` step is refused as `typst::plate_missing`, its hint naming the spelling from the quill root, and so is a `plate_file` naming no file. A `plate_file` naming a file at a path holding a `\`, which Typst refuses, fails as `typst::plate_path_invalid`: separate the path with `/`, and rename a file whose own name holds a `\`. So does a `plate_file` that is not a string, or that names no path once its `.` and empty steps drop, such as `""` or `./`.

Any other key under `typst` fails the load as `quill::unknown_backend_key`, its hint listing the valid keys. A `typst` section that is a scalar or a sequence fails as `quill::invalid_backend_section`, and an empty one, `typst:` alone or holding only comments, reads as no keys. A quill using a key a later release adds declares that release in [`requires`](#quill-section), so an older Quillmark refuses the quill by its floor rather than reporting the key. Packages are not declared here: a quill vendors them under `packages/`, as the [Typst Backend Guide](typst-backend.md#typst-packages) describes.

---

## Reading the schema programmatically

Quillmark emits a public schema contract derived from `Quill.yaml`. Accessors:

- Rust: `QuillConfig::schema()` (JSON) / `QuillConfig::schema_yaml()` (YAML)
- Python: `quill.schema` (structured dict)
- WASM: `quill.schema` (JSON)
- CLI: `quillmark schema <path>`

`ui:` hints are preserved verbatim in the output. See [SCHEMAS.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/SCHEMAS.md) for the emitted shape.

---

## Complete Example

```yaml
quill:
  name: project_report
  version: "1.0.0"
  backend: typst
  description: Monthly project status report
  author: Engineering Team

main:
  fields:
    project_name:
      type: text

    status:
      type: enum
      values: [on_track, at_risk, blocked]

    risk_description:
      type: text
      default: ""
      description: Describe the risk or blocker. Only needed when status is not on_track.

    date:
      type: date

    team:
      type: object
      properties:
        members:
          type: array
          items:
            type: text
          default: []

    financials:
      type: object
      properties:
        budget:
          type: number
          default: 0

card_kinds:
  milestone:
    description: A project milestone with target date and status.
    fields:
      name:
        type: text
      target_date:
        type: date
      completed:
        type: boolean
        default: false
```

---

## Next Steps

- [Creating Quills](creating-quills.md): hands-on tutorial
- [Markdown Syntax](../authoring/markdown-syntax.md): document authoring syntax
- [CLI Reference](../cli/reference.md): validating quills with the `validate` command
