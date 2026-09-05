# Quill.yaml Reference

Complete reference for authoring `Quill.yaml` configuration files. For a hands-on introduction, see [Creating Quills](creating-quills.md).

## File Structure

A `Quill.yaml` has these top-level sections:

```yaml
quill:        # Required: format metadata
  ...

main:         # Optional, main entry-point card: field schemas and optional ui/body
  fields:
    ...
  ui:         # optional UI hints (e.g. title)
  body:       # optional body-region config (e.g. enabled, example)

card_kinds:   # Optional: additional composable card kinds
  ...

typst:        # Optional: backend-specific configuration
  ...
```

Root-level `fields:` is not supported; define the main document's field schemas under `main.fields`.

`Quill.yaml` is parsed strictly. Unknown keys in the `quill:` section, unknown top-level sections, malformed `ui:` blocks, and field schemas that can't be parsed all produce errors: they are never silently dropped. Every error is collected in a single pass, so authors see all problems at once. Run `quillmark validate <quill_dir>` to surface them.

---

## `quill` Section

Every Quill.yaml must have a `quill` section with format metadata.

`quill.name` must be `snake_case` (`^[a-z][a-z0-9_]*$`).

| Key              | Type   | Required | Description |
|------------------|--------|----------|-------------|
| `name`           | string | yes      | Unique identifier for the Quill |
| `backend`        | string | yes      | Rendering backend (e.g. `typst`) |
| `description`    | string | yes      | Human-readable description of the quill itself (non-empty). Independent of `main.description`, which is the optional schema description authored under `main:`. |
| `version`        | string | yes      | Semantic version (`MAJOR.MINOR` or `MAJOR.MINOR.PATCH`) |
| `author`         | string | no       | Creator of the Quill (defaults to `"Unknown"`) |
| `ui`             | object | no       | Document-level UI metadata |

A backend's own settings live under its backend-named section, not in `quill:`.
The Typst template, for example, is declared as `typst.plate_file` (see the
[`typst` Section](#typst-section) below).

```yaml
quill:
  name: usaf_memo
  version: "0.1"
  backend: typst
  description: Typesetted USAF Official Memorandum
  author: TongueToQuill

typst:
  plate_file: plate.typ
```

---

## `main` Section

The main document card holds **root-block field schemas** under `main.fields`. Optional `main.description` describes the schema itself (independent of `quill.description`, which describes the quill package). Optional `main.ui` sets container-level UI for that card. `quill.ui` is a fallback for `main.ui`, not a merge: any `main.ui` (even an empty `ui: {}`) wins wholesale, and `quill.ui` applies only when `main.ui` is absent.

Field order under `main.fields` **is** display order in UIs: the declaration order of the keys is carried structurally through parsing and schema emission, so consumers walk the fields in key order. There is no `ui.order` knob: to reorder fields, reorder them in `Quill.yaml`.

Field keys must be `snake_case` (`^[a-z][a-z0-9_]*$`). Capitalized field keys are reserved.

```yaml
main:
  fields:
    subject:          # Field name (used as the card-yaml payload key)
      type: string
      description: Be brief and clear.
```

### Field Properties

| Property      | Type              | Required | Description |
|---------------|-------------------|----------|-------------|
| `type`        | string            | yes      | Data type (see [Field Types](#field-types)) |
| `description` | string            | no       | Detailed help text |
| `default`     | matches `type`    | no       | The value the **majority of authors want**. When the cell is omitted, the default is filled in — at any depth, whether or not the container above it was authored — and the blueprint renders that concrete value with a type-only annotation, shippable as-is. Declaring it also makes the field unobliged (see [Obligation](#obligation)). Declared on a **cell**: a leaf or an `array`. On an `object` it is a load error (`quill::default_on_namespace`), since its properties hold their own. |
| `example`     | matches `type`    | no       | A value matching the **type and shape** of what the author wants, but **not** the value desired most of the time. Documents shape only, never rendered as the value: it takes the blueprint cell when no `default` holds it, and surfaces in the `# e.g.` line otherwise. Declared on a **cell**, as `default` is (`quill::example_on_namespace`). |
| `values`      | array of strings  | for `enum` | The closed set of allowed string values: the **choices**. Required on every `enum` field. Declaring `""` is a load error — every enum also accepts its [blank](#the-blank-values-is-for-choices-not-for-the-absence-of-one), which the engine supplies. |
| `ui`          | object            | no       | UI rendering hints (see [UI Properties](#ui-properties)) |
| `items`       | object            | for `array` | Element schema for an `array` field (a nested field schema). Required on every array. |
| `properties`  | object            | for `object` | Nested field schemas for an `object` typed dictionary (or an array's `object`-typed `items`). Required on every `object` field. |
| `inline`      | boolean           | no       | For `richtext` and `plaintext` only: constrain the content to a single paragraph/line (a one-line editor surface). |

### Obligation

One question per field: **does it have a value when nobody types anything?**
`default` is the answer, and its absence is what obliges a human.

```yaml
# Optional, with nothing to suggest: the type's blank is the answer "nothing".
internal_note:
  type: string
  default: ""

# A suggested marking a human must still confirm: no default, so the cell is
# obliged, and the example fills it under the marker.
classification:
  type: enum
  values: [UNCLASSIFIED, CUI]
  example: UNCLASSIFIED
```

There is no `must_fill:` key: declaring one is a load error naming the `default:`
or `example:` to write instead.

An obliged field is one that carries the `!must_fill` marker in the blueprint,
is stamped when seeding commits its `example`, and raises the non-fatal
`validation::must_fill` warning from `Quill::validate` while the document leaves
it unauthored. Three things discharge it: authoring a value, authoring the
field's [blank](#the-blank-values-is-for-choices-not-for-the-absence-of-one)
(deliberately nothing is an answer), or dropping the marker by hand.

**It is an affordance, not a submit gate.** If you arrive from web forms, the
familiar half transfers — the editor knows which fields to mark — and the
enforcement half does not. An unfilled must-fill field **renders**; nothing
refuses it. "Must pick" is this warning plus whatever policy a consumer layers
on top, canonically *a strict consumer treats any outstanding marker as not
done*. There is no `required:` and no severity knob: on this surface
`Severity::Error` already means "won't render".

On a **typed dictionary** the key is inert on the container — the obligation
lives on its leaves, which is where the blueprint marks and the warning
anchors. An **array** is its own cell, so `[]` is an authored answer that
discharges it.

### Field Types

| Type       | Notes |
|------------|-------|
| `string`   | Open scalar UTF-8 text: a value the template computes with (a URL, path, identifier, or reference key), not prose it lays out |
| `enum`     | A closed set of string values; requires a `values:` list. Also accepts its [blank](#the-blank-values-is-for-choices-not-for-the-absence-of-one) (`""`), which is not a declared member. Projects to JSON-Schema `{type: string, enum: ["", …]}` |
| `plaintext`| Navigable, **unformatted** prose over the canonical content: the same nav/regions as `richtext`, but a literal codec (delimiters stay literal, no markup). Add `inline: true` for the single-line variant |
| `number`   | Numeric scalar (integers and decimals) |
| `integer`  | Integer-only numeric scalar, sized as an `i64`; a literal past that range takes `number` |
| `boolean`  | `true` or `false` |
| `array`    | Ordered list; requires an `items:` element schema |
| `date`     | A strict calendar date `YYYY-MM-DD`; rejects any time component |
| `datetime` | A strict offset-less wall-clock datetime `YYYY-MM-DDThh:mm[:ss]`; rejects offsets, the space separator, fractional seconds, and bare dates |
| `richtext` | Rich, **formatted** prose over a canonical content; backends lower it to the target format. Markdown is its import/export projection. Add `inline: true` for the single-paragraph variant |
| `object`   | Structured map; requires a `properties:` map |

#### Choosing among `string`, `enum`, `plaintext`, and `richtext`

The four text-ish types form a 2×2 of **data vs content** × **open/plain vs closed/formatted**, and two questions resolve it:

1. Does the author write prose here, or does the plate compute with the value? Prose is content; a name, URL, path, identifier, or reference key is data.
2. Then: is the domain closed (`enum` over `string`), or should markdown delimiters format the text rather than stay literal (`richtext` over `plaintext`)?

| | data — the plate computes with it | content — the author writes and navigates prose |
|---|---|---|
| **open / literal** | `string` | `plaintext`: `*text*` stays literal |
| **closed / formatted** | `enum`: a `values:` domain | `richtext`: `*text*` becomes emphasis |

A content field rides the canonical content model, so it carries navigation, regions, and click-to-edit in editor consumers; `string` and `enum` carry none of that. `plaintext` and `richtext` share that entire stack and the same backend lowering, so they are indistinguishable in an editor and diverge only at emit, where the codec decides whether a delimiter is markup or a character.

Changing a declared type reinterprets every stored value in that field at the next bound load, with no diagnostic, and data → content is the lossy direction: the stored string enters the codec's import and its delimiters are consumed as structure, leaving the literal characters unrecoverable. A declared type change is a new quill version ([Quill Versioning](versioning.md)); audit the corpus before publishing one, as under [Date and Datetime Grammars](#date-and-datetime-grammars).

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

#### The blank: `values:` is for choices, not for the absence of one

Every `enum` accepts one value beyond its `values:` list — the **blank**, spelled
`""`. The engine supplies it; you never declare it, and declaring `""` in
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
all range over `values ∪ blank`. Keeping `default: ""` is how you say "this field
is optional"; dropping it makes the field one an author is expected to answer.

Where the empty state is itself a decision someone makes and the document should
record it, make it a member — `undecided`, `waived`, `n_a` — not the blank. The
blank means nobody chose; a member means someone chose "none".

Name the blank's label with `ui.blank_title` when a bare empty row would read
badly; absent one, consumers supply their own conventional label.

If you arrive from **web forms**, your prior transfers: this is HTML's
placeholder `<option value="">`, Django's `("", "---------")`, Rails'
`include_blank:`. One caveat — the affordance carries over, the enforcement does
not. There is no `required:`; an unanswered field is a warning plus consumer
policy, never a load or render failure. If you arrive from **protobuf**, your
prior is a near-miss: proto3 reserves slot 0 *inside* the enum
(`FOO_UNSPECIFIED = 0`), whereas here the sentinel lives outside the domain and
your `values:` list stays clean.

> **Writing a plate against an enum:** branch over `values ∪ blank`
> exhaustively. An `else` fallback silently renders a variant nobody chose, and
> `data.at(key, default: X)` is not a guard — every declared key is always
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
          controlled_by: { type: string }        # obliged, but only on a CUI memo
          poc:           { type: string }
          category:      { type: string, default: "" }
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
UNCLASSIFIED` is accepted and means the same as `{value: UNCLASSIFIED}`.

The container is a *document* shape. The schema's own `default:` and `example:`
name the discriminant alone (`default: ""`, `example: CUI`); a container-shaped
one is a load error, because each cell in a world carries its literal on its own
declaration.

Three things follow, and they are the reason to reach for this over a
`cui_`-prefixed row of flat fields:

- **The names shorten.** The prefix was hand-written namespacing; nesting
  supplies it structurally, so `cui_poc` becomes `poc`.
- **`must_fill` becomes conditional.** `controlled_by` declares no `default:`,
  so it is obliged — but only where `classification` reads `CUI`. On every other
  memo the same schema asks for nothing. That is the one cross-field rule the
  engine checks rather than describes in `description:` prose.
- **Editors know.** The schema says which cells are out of play, so a form
  retires them instead of showing a CUI block on an unclassified memo.

**Writing a plate against variants.** The live world's fields arrive only inside
the branch that selects it, which is the branch you already owe every enum:

```typst
..if data.classification.value == "CUI" {
  (controlled_by: data.classification.controlled_by, poc: data.classification.poc)
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
dates and containers included, reaching the plate exactly as a card-level one
does. What it cannot carry is `variants:` of its own. `variants:` itself
is valid only on a card-level `type: enum` field,
keys only on declared members (never `""`, which owns no field set), and cannot
declare a field named `value`. Variant fields inherit the discriminant's
`ui.group`; declaring one inside a variant is an error. A field set shared by
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
        type: string
    counts:
      type: array
      items:
        type: integer
    sections:
      type: array
      items:
        type: richtext   # each element's content is lowered to backend markup
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
            type: string
          score:
            type: number
```

Use `type: object` with `properties:` for a single structured mapping:

```yaml
main:
  fields:
    address:
      type: object
      properties:
        street:
          type: string
        city:
          type: string
```

Containers nest freely: a property or an element is an ordinary field, so it carries whatever type a card-level field carries, itself included. `object<array<string>>`, `array<array<integer>>` and a typed table whose row holds a typed dictionary are all declarable, and each leaf is addressable by the schema address its path spells (`contact.address.city`, `grid.0.0` — see [PLATE_DATA.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PLATE_DATA.md#schema-addresses)).

**A dictionary holds no `default:` or `example:` of its own** — its properties do:

```yaml
address:
  type: object
  # default: { street: "5000 Forbes Ave" }   # quill::default_on_namespace
  properties:
    street: { type: string, default: "5000 Forbes Ave" }
    city:   { type: string, default: "" }    # type-empty: a skippable cell
```

The container spelling would be a second declaration of a value the property
already holds, and the two disagree: `must_fill` derives per property, so a
container default renders a value *and* still reports the cell unauthored. Each
property's `default:` is reached whether or not a document authors the container
above it, so writing `address: {}` changes nothing — which also makes a whole
dictionary skippable by giving each property a type-empty default.

An `array` **does** keep its own `default:` / `example:`: `items:` fixes the
element type but never the arity, so `default: []` and `default: [{…}]` say
something no property declaration can. Each element it supplies is completed
against `items` exactly as an authored element is.

Two keys are card-level regardless of depth: `ui.group` (grouping never descends) and `variants:` (see [Variants](#variants-fields-that-exist-only-for-one-choice)).

---

## UI Properties

The `ui` property on fields controls how form builders and wizards render the field. These are UI hints, not validation constraints.

### `title`

Overrides the display label shown next to the input. Form builders derive a label automatically from the snake_case field key (`memo_for` → "Memo For"), so `ui.title` is only needed when that automatic label is wrong or misleading:

```yaml
main:
  fields:
    memo_for:
      type: array
      items:
        type: string
      ui:
        title: To       # "Memo For" would confuse users unfamiliar with memo conventions
```

### `group` and the group registry

Groups organize fields into visual sections. A group has two parts: a **registry** declared once on the card (`ui.groups`), and a per-field **reference** (`ui.group`) into that registry.

```yaml
main:
  ui:
    groups: [addressing, letterhead]   # declaration order = display order
  fields:
    memo_for:
      type: array
      items:
        type: string
      ui:
        group: addressing              # a reference, validated against the registry

    memo_from:
      type: array
      items:
        type: string
      ui:
        group: addressing

    letterhead_title:
      type: string
      ui:
        group: letterhead
```

The registry is the card's table of contents. Its keys are **snake_case ids** (same discipline as field keys), and their declaration order fixes the group display order: the contract every consumer follows, exactly as field declaration order fixes field display order. A field's `ui.group` names one of those ids; a value with no matching registry key is a load error (`quill::unknown_group`), so a one-character typo cannot silently split a section.

**Identity is the id, not the label.** Consumers derive a group's display label from its id (`addressing` → "Addressing"), just as a field label is derived from its key. Override the derived label with `title:`, which requires the mapping form of the registry:

```yaml
main:
  ui:
    groups:
      addressing: {}                       # label derived: "Addressing"
      letterhead: { title: "Letterhead & Seal" }   # label overridden
```

The two registry forms are interchangeable: a bare sequence of ids (`[addressing, letterhead]`) when no labels need overriding, or a mapping of id → attributes when they do. Renaming a label touches one line and never breaks a `ui.group` reference or persisted per-group editor state.

`group` applies only to card-level fields (those directly under a card's `fields:`). Grouping never descends into an object's properties or an array's items, so a `group` on a nested property is a hard error (`quill::nested_group_not_supported`) rather than a silently inert knob.

**Implicit groups (deprecated).** A `ui.group` with no `ui.groups` registry on the card works: each distinct value is an implicit group whose label *is* the value, ordered by first appearance. It emits a `quill::implicit_group` deprecation warning and will become an error in a future release. Declare a registry to silence it.

### field order

Field display order is **declaration order**: the order the keys appear in `Quill.yaml`. This holds at every level: card-level fields, and the properties of a typed dictionary or typed-table row. The order is carried structurally (the schema's field maps preserve key order, and `schema()` re-emits that order), so no per-field knob is involved.

There is no `ui.order` key: an authored `ui: { order: N }` is a load error (`quill::field_parse_error`) directing you to reorder the fields instead. To move a field, move its block in `Quill.yaml`.

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

It labels the blank, never a member: `values:` carries no entry for it. Consumers must keep the blank **selectable and re-selectable** — returning to it is how an author clears a cell back to unset, so a disabled placeholder that vanishes once a choice is made is the wrong idiom.

### `compact`

When `true`, the UI renders this field in a compact style (smaller vertical footprint).

```yaml
main:
  fields:
    tag:
      type: string
      ui:
        compact: true
```

### `multiline`

Controls the initial size of the text input for `string` and `richtext` fields. When `true`, the UI starts with a larger text box instead of a single-line input:

```yaml
main:
  fields:
    summary:
      type: richtext
      description: Executive summary
      ui:
        multiline: true   # start as a larger text box

    notes:
      type: string
      description: Free-form notes
      ui:
        multiline: true

    tagline:
      type: richtext
      description: One-sentence tagline
      # no multiline: single-line input that expands on demand
```

Meaningful on `string` and `richtext` fields; ignored on other types.

---

## `card_kinds` Section

`card_kinds` define composable, repeatable content blocks (the *kinds*: a document can then carry zero or more *instances* of each kind, interleaved with body content). Each entry is shaped exactly like `main:` (`fields`, optional `description`, `ui`, `body`); think of `main:` as the single mandatory card-kind for the document body, and `card_kinds:` as the library of additional kinds that may attach to it.

Card-kind names (the keys under `card_kinds`) must match `[a-z_][a-z0-9_]*` (leading underscore is allowed).

```yaml
card_kinds:
  indorsement:                    # Card-kind name
    description: Chain of routing endorsements.
    fields:
      from:
        type: string
        ui:
          group: Addressing
      format:
        type: enum
        values: [standard, informal, separate_page]
        default: standard
```

Invalid card-kind names include:

- `BadCard` (uppercase letters)
- `my-card` (hyphen)
- `2nd_card` (starts with a digit)

### Card Properties

| Property      | Type   | Required | Description |
|---------------|--------|----------|-------------|
| `description` | string | no       | Help text describing the card's purpose |
| `fields`      | object | no       | Field schemas (same structure as top-level fields) |
| `ui`          | object | no       | Container-level UI hints (see [Card-level `ui`](#card-level-ui)) |
| `body`        | object | no       | Body-region config (see [Card-level `body`](#card-level-body)) |

### Card-level `ui`

| Property | Type   | Description |
|----------|--------|-------------|
| `title`  | string | Display label for the card kind. Literal string or `{field}` template |

### Card-level `body`

| Property  | Type   | Description |
|-----------|--------|-------------|
| `enabled`     | bool   | Whether the body editor is enabled (default: true). When false, consumers must not accept or store body content for this card kind. |
| `example`     | string | Default body text used when seeding a card of this kind and shown in the blueprint body region; falls back to `Write <kind> body here.` when absent. |

#### `title`

A human-readable display label for the card kind. UI consumers should prefer it over the snake_case map key when rendering section headers, chips, picker entries, or per-instance titles in a list.

The label is decoupled from the map key (e.g. `indorsement`), which is the on-the-wire `$kind` discriminator. Authors can rename the label freely without invalidating stored documents.

**Two flavors:**

A literal string serves as a static type label:

```yaml
card_kinds:
  indorsement:
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
```

A template containing `{field_name}` tokens lets UI consumers produce a per-instance title by interpolating live field values:

```yaml
card_kinds:
  endorsement:
    ui:
      title: "{from} → {for}"
    fields:
      from:
        type: string
      for:
        type: string
```

With the template form, a UI rendering a list of cards can title each instance (e.g. `"ORG1/SYM → ORG2/SYM"`) instead of falling back to a generic `"Card (2)"`.

**Interpolation rules (for UI consumers):**
- `{field_name}` is replaced with the current value of that field.
- A title with no `{}` tokens is rendered verbatim: it's just a literal label.
- If a referenced field is absent or empty, the token resolves to an empty string.
- UI consumers are responsible for trimming degenerate separators (e.g. `": "` with one empty side).

When omitted, UI consumers fall back to the prettified map key.

#### `body.enabled`

When `false`, the card kind has no body/content area. Consumers must not accept or store body content for instances of this card kind. The validator enforces this: a document instance that provides body content for a `body.enabled: false` card kind is rejected with a `BodyDisabled` error.

```yaml
card_kinds:
  metadata_block:
    body:
      enabled: false    # Card has fields only, no body/content area
    fields:
      category:
        type: string
```

#### `body.example`

Default body text seeded into a card of this kind and shown verbatim in the blueprint body region (it falls back to `Write <kind> body here.` when absent). Has no effect when `body.enabled` is false.

```yaml
card_kinds:
  experience:
    body:
      example: Describe your role, responsibilities, and key achievements.
    fields:
      company:
        type: string
```

#### `body.unsupported`

The block constructs your plate does not typeset in this body. Empty by default: declare nothing and nothing changes.

A plate is free to reinterpret a construct — absorb it into a neighbour, move its text, typeset nothing at all — and only you know it did. Declaring it here says so once, as data, and buys two things: an editor reads it off the schema and can decline the gesture before the author makes it, and a body holding the construct anyway (from an import, a repack, the CLI) draws a `plate::unsupported_construct` warning on the pre-render walk. The warning is non-fatal and carries the body's path, the construct name, and how many the body holds.

```yaml
main:
  body:
    # This template has no dividers, at any depth.
    unsupported: [rule]
  fields: {}
```

Valid names: `heading`, `rule`, `code`, `list`, `quote`, `table`, `image`. The set is closed, so a misspelling is a load error rather than a declaration that quietly matches nothing. There is no paragraph name: a paragraph is the floor and cannot be declined, and there are no context-qualified forms (a heading is declined everywhere or nowhere).

Nothing verifies a declaration against what your plate actually does. It is documentation with a diagnostic attached: declaring `rule` does not make rules disappear, and omitting a construct your plate drops keeps that drop as silent as before.

### Using Cards in Markdown

Card kinds defined here are authored as `~~~` blocks (with a `$kind: <kind>` line) in the document body. See [card-yaml Blocks](../authoring/card-yaml.md#card-blocks) for the markdown syntax.

---

## `typst` Section

Backend-specific configuration for the Typst renderer.

| Key          | Type   | Required | Description |
|--------------|--------|----------|-------------|
| `plate_file` | string | no       | Path (relative to the quill root) to the Typst template the backend compiles |
| `packages`   | array  | no       | Typst packages the template depends on |

```yaml
typst:
  plate_file: plate.typ
  packages:
    - "@preview/appreciated-letter:0.1.0"
```

See the [Typst Backend Guide](typst-backend.md) for details.

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
  version: "1.0"
  backend: typst
  description: Monthly project status report
  author: Engineering Team

typst:
  plate_file: plate.typ

main:
  fields:
    project_name:
      type: string
      ui:
        group: Header

    status:
      type: enum
      values: [on_track, at_risk, blocked]
      ui:
        group: Header

    risk_description:
      type: string
      default: ""
      ui:
        group: Header
      description: Describe the risk or blocker. Only needed when status is not on_track.

    date:
      type: date
      ui:
        group: Header

    team_members:
      type: array
      items:
        type: string
      default: []
      ui:
        group: Team

    budget:
      type: number
      default: 0
      ui:
        group: Financials

card_kinds:
  milestone:
    description: A project milestone with target date and status.
    fields:
      name:
        type: string
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
