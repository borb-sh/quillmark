# Blueprint Emission (`QuillConfig::blueprint`)

> **Implementation**: `crates/core/src/quill/`

## TL;DR

`blueprint()` produces an annotated Markdown document: the same shape an
author would write: pre-filled with placeholders, examples, and
constraint hints. It is the **authoring surface** for LLM and MCP
consumers; [SCHEMAS.md](SCHEMAS.md) covers the validation/form surface.

A blueprint is the document, not a description of the document. Fill in
the placeholders; the structure, `$` metadata, and body markers come for
free.

## Output shape

````
~~~
$quill: <name>@<version> # keep verbatim
$kind: main
# <description>

# <field description>
field: !must_fill # <type>
  - <example item>
settled: value # <type>[<format>]
~~~

Write main body here.

~~~
$kind: <card_kind>
# composable (0..N)
# sample card; delete if not needed
# <card description>
...fields...
~~~

Write <card_kind> body here.
````

Every block is a bare `~~~` block (the canonical card-yaml fence; the opener's
info string is ignored; see
[markdown-spec.md](../references/markdown-spec.md) §3): the root block carries
the `$quill` system-metadata line; each composable card carries a
`$kind: <card_kind>` metadata line.

When `body.example` is set, its text replaces the body marker entirely.
When `body.enabled` is false the marker is omitted entirely.

## One emitter, by construction

`blueprint()` does not format YAML itself. It builds a `Document`: the same
typed model a parsed `.md` produces, with prose annotations as comments and
`!must_fill` as fill flags, and emits it through the **canonical
`Document::to_markdown`**. There is no second formatter. Two consequences
follow:

- The blueprint round-trips through `Document::parse` and back **by
  construction**: the emitter that produced it is the same one round-trip uses.
- The blueprint inherits `to_markdown`'s representation choices: a **one-space**
  ` # ` inline-comment gap, **block-style** sequences at every level (no inline
  flow), and **inline double-quoted** multi-line strings (no `|`/`>` block
  scalars). The sections below reflect those choices.

## Annotation grammar

| Slot | Form | Carries |
|---|---|---|
| **Leading `# …` lines** above a field | `# <prose>` or `# e.g. <value>` | description (single-line prose) and an illustrative example |
| **Inline `# …`** at end of the value line | `# <type>[<format>]` | structural metadata: the field's type and an optional format refinement |

The two slots have disjoint purposes: leading is prose, inline is
structural. No colon-separated `key: value` annotation syntax appears in
either slot, so neither pattern collides with YAML key/value parsing.

### Leading lines: order

Per field, in order:

1. `# <description>`: `description:` from `Quill.yaml`,
   whitespace-collapsed. **Single line only**; multi-line descriptions are
   rejected at `Quill.yaml` parse time.
2. `# e.g. <value>`: emitted whenever `example:` is configured **and a
   `default:` already holds the cell**. Independent of type. There the example
   never becomes the rendered value, so it surfaces as a hint; where no
   `default:` holds the cell the example inlines *as* the value (see
   "Placeholder value precedence") and a separate hint would be redundant. The
   one exception is `richtext`, which never inlines its example as the value: a
   defaultless richtext field with an `example:` therefore keeps the `# e.g.`
   line (see "Richtext fields").

That's it. There is no leading `# required`, `# enum:`, `# default:`, or
`# type:`: those collapse into the inline.

### Inline annotation

Form: **`# <type>[<format>]`**

- **Type slot** (mandatory, first): one of
  `string`, `integer`, `number`, `boolean`, `array`, `object`,
  `richtext`, `plaintext`, `date`, `datetime`, `enum`.
  Every field is labeled: there is no "self-evident" exemption.
  (`object` requires a `properties` map; freeform untyped objects are not
  supported. `object` also appears in the format slot of typed-table fields
  as `array<object>`.)
- **Format slot** (optional, in `<…>` angle brackets): refines the type
  when the refinement carries information beyond the type name itself.
  - `date<YYYY-MM-DD>`: a bare calendar date; `datetime<YYYY-MM-DDThh:mm[:ss]>`:
    an offset-less wall-clock datetime (no offset/space/fractional forms)
  - `richtext<markdown>`, `richtext(inline)<markdown>`: the `<markdown>` slot
    names the surface encoding an author writes over the content model
  - `plaintext<plain>`, `plaintext(inline)<plain>`: the `<plain>` slot names
    the literal codec (delimiters stay literal), distinct from `<markdown>`
  - `array<string>`, `array<integer>`, `array<object>`, `array<richtext<markdown>>`, …
  - `enum<a | b | c>`
  - omitted for `string`, `integer`, `number`, `boolean`, `object`
    (nothing meaningful to refine).

The inline annotation is **purely structural**: it carries the type (and
optional format), nothing else. What a reader must *do* is carried by the cell:
a `!must_fill` marker asks for a value, and its absence says the cell is
shippable as-is (delete or blank the line to fall back to the default). The two
can co-occur — a marked cell may still carry a concrete value to review.

The `$`-prefixed system-metadata keys (`$quill`, `$kind`, …) carry no
inline type annotation: they are not user-defined data fields, so there
is no `# <type>` slot to fill. (A `$` line *can* carry an ordinary YAML
comment: both an inline trailing ` # comment` and an adjacent own-line
comment parse and round-trip faithfully, exactly like comments on data
fields; see [markdown-spec.md](../references/markdown-spec.md) §3.3.)

The root block's `$quill` line is emitted verbatim and carries an inline
**`# keep verbatim`** reminder: an in-band guard against the
`parse::missing_quill` failure, where an LLM author omits the `$quill` line
entirely and the document fails to bind to a quill. The reminder rides only
on `$quill`: it is the one line whose omission is a hard error. `$kind: main`
carries no reminder: an omitted root `$kind` is synthesised at parse time,
so dropping it is not an error, and a `# …` line in that slot would only
read as a leading annotation for the field below it. A composable card's kind is carried in its
`$kind: <card_kind>` metadata line. Its `composable (0..N)` role is
emitted as an own-line `# composable (0..N)` comment directly under the
`$kind` line, ahead of the card description: that comment carries the
card's cardinality, which is structural information rather than a
redundant instruction. A second own-line comment, `# sample card; delete if
not needed`, follows directly under it: the card's fields and body are one
worked example of that kind, and "0..N" alone does not say a reader is free
to drop it.

Examples:

| Line | Reading |
|---|---|
| `name: !must_fill # string` | must-fill string, no example: bare marker, replace before shipping |
| `name: !must_fill Jane Doe # string` | must-fill string with an `example`: the example is the suggested value, still marked |
| `title: "Curriculum Vitae" # string` | defaulted string: concrete value, shippable as-is (keep or override) |
| `count: 0 # integer` | defaulted integer (type-empty default, shippable as-is) |
| `active: false # boolean` | defaulted boolean (type-empty default, shippable as-is) |
| `notes: "" # string` | defaulted empty string (the "skippable" cell) |
| `classification: !must_fill UNCLASSIFIED # enum<UNCLASSIFIED \| CUI>` | an `example` on a defaultless enum: the suggested marking fills the cell, still asking a human to confirm it |
| `bio: !must_fill # richtext<markdown>` | must-fill richtext: bare marker (see "Richtext fields") |
| `recipient: !must_fill # array<string>` | must-fill array of strings |
| `date: !must_fill # date<YYYY-MM-DD>` | must-fill date |
| `severity: !must_fill # enum<low \| medium \| high>` | must-fill enum |
| `$quill: cmu_letter@0.1.0 # keep verbatim` | quill binding metadata, emitted verbatim; the inline reminder guards against dropping the line |
| `$kind: skill` followed by `# composable (0..N)` and `# sample card; delete if not needed` | repeat the entire `~~~` … `~~~` block per instance, or delete it if none are needed |

## Placeholder value precedence

The blueprint emits along **two axes**, and one declaration answers both. The
*value axis* decides what data the cell carries; the *marker axis* decides
whether the cell is stamped `!must_fill`.

**Value** is `default:` › `example:` › bare (null for scalars, empty for a
container). **Marker** is the field's derived `must_fill` (`SCHEMAS.md` § "Value
and obligation"), which is `default:`'s absence. The full grid:

| Field state | Value rendered | Marker |
|---|---|---|
| `default` | the default | none |
| no `default`, has `example` | the `example` | `!must_fill` |
| no `default`, no `example` | bare null/empty | `!must_fill` |

An `example` takes the cell only when no `default:` holds it, and surfaces in
the `# e.g.` leading line otherwise. The `default:` that vacates the cell is the
one that marks it, so a suggested value arrives under its marker. The rule holds
uniformly for scalars, arrays and typed tables, and for each **property** of a
typed dictionary (which holds no cell of its own): **except `richtext`**, which
never inlines its example as a value at all; its `example:` always surfaces as
the `# e.g.` line (see "Richtext fields").

All fields render as **live YAML**: no commented-out fields. The `!must_fill`
marker is the sole "must fill" signal on this surface: a reader's mental model
is one rule, **`!must_fill` on a field → replace before shipping; otherwise the
value cell is shippable as-is**. A marked document still renders (the cell
blank-fills, or uses its suggested value); the marker only drives the non-fatal
`validation::must_fill` warning (see "Guarantees").

A value shown under `!must_fill` is the schema's own `example:` —
illustrative, not real data. Dropping the tag while keeping that value
verbatim ships the schema's example as if it were the answer; replace the
value along with the tag.

The blueprint's own `!must_fill` cells aside, an author's `field: ""` and a
blank/null `field:` (or `field: null`) are different answers once the document
is parsed: `""` is the field's own explicit blank and is kept exactly as
written, while `field:` / `field: null` carry no data (**null ≡ absent**) and
fall back to the field's `default:` when one is declared, else to the field's
own blank (`authored` › `default:` › `blank`; see [SCHEMAS.md](SCHEMAS.md)
§ "Native validation"). Write `""` on purpose to force a field empty; leave it
blank or `null` to accept its default.

The marker is stamped where the LLM types the value. This table is also the
**cell set the `unauthored` trigger addresses**: the schema-side predicate warns
at exactly these paths, so the blueprint and a document that never saw one
speak about the same cells (`SCHEMAS.md` § "Native validation").

| Type | Marker position | Example |
|---|---|---|
| `string`, `integer`, `number`, `boolean`, `date`, `datetime`, `enum`, `plaintext` | On the field | `name: !must_fill # string` |
| `array<scalar>` | On the field | `recipient: !must_fill # array<string>` |
| `richtext` | On the field (bare; no block scalar) | `bio: !must_fill # richtext<markdown>` |
| `object` (typed dict) | Per-property recursion | leaves carry `!must_fill` |
| `array<object>` (typed table) | Per-property recursion in one synthetic row | leaves carry `!must_fill` |
| `enum` with `variants:` | On the `value` cell, plus per-field recursion in the live world | `value: !must_fill` |

### Enum variants

A variant-bearing `enum` emits its container: the discriminant under `value`,
then the fields of the world that discriminant names (`default:` › `example:` ›
blank). A blueprint **is** a document, so it can show only one world; the others
are named instead, one `# when <MEMBER>: <fields>` leading line each. Every member
owning a field set gets a line, the shown one included if it owns one — a member
that brings no cells has nothing to announce, which is why the example below,
sitting in the blank world, carries only `CUI`'s. The lines are the map, the cells
are the position.

```
# Select the classification marking shown in the header and footer banner.
# when CUI: controlled_by, poc, category, limited_dissemination
classification: # enum<UNCLASSIFIED | CUI | CONFIDENTIAL | SECRET | TOP SECRET>
  value: ""
```

The container line carries the `enum<…>` annotation and `value` carries none:
`value` *is* that enum, so a second annotation would restate it. The marker sits
on `value` rather than the container, since `!must_fill` is rejected on a
mapping.

This is the one place the `unauthored` cell set is **value-dependent**: which
cells the schema-side predicate addresses follows from the discriminant a
document authored, so the blueprint and that document speak about the same cells
only once both are in the same world. That is the point of the feature rather
than a seam in it — an obligation that ignored the discriminant is exactly the
unconditional `must_fill` variants exist to refine.

### Richtext fields

A richtext field's value cell is markdown: the surface projection of the
content model, which `to_markdown` re-emits: carried under a `# richtext<markdown>`
annotation.

A defaultless `richtext` field renders as a bare marker on the field:
no block scalar:

```
bio: !must_fill # richtext<markdown>
```

The LLM replaces the marked field with its markdown content (a quoted scalar
or a block scalar, the consumer's choice); the marker signals "fill me."

Unlike other scalars, a richtext field never inlines its `example:` as the
marker's suggested value (a block-scalar placeholder would be indistinguishable
from real content). Instead the `example:` surfaces as a `# e.g.` leading hint:

```
# e.g. Hello world
bio: !must_fill # richtext<markdown>
```

When a `default:` is configured, the field renders its
default as an **inline double-quoted scalar** with `\n` escapes: the canonical
`to_markdown` string form (no `|`/`>` block scalars):

```
bio: "## About me\n\n<body>" # richtext<markdown>
```

If the default is empty (`default: ""`), the cell is the inline empty string
`bio: "" # richtext<markdown>`: the "skippable" richtext cell.

### Multi-element example arrays

The `example` of a defaultless array field rides the `!must_fill` marker as a
**block-style sequence**: the canonical `to_markdown` form at every nesting
level:

```
recipient: !must_fill # array<string>
  - Mr. John Doe
  - 123 Main St
  - Anytown, USA
```

Items are quoted only when their plain form would re-parse differently
(`to_markdown`'s scalar rule); in block context a leading/embedded comma does
not force quoting.

### Reserved characters in format and enum literals

To keep the inline grammar unambiguous, format slot contents; including
enum values: may not contain `>`, `;`, or `|`. These are the closing
delimiter, the role separator, and the enum-value separator respectively.
`Quill.yaml` parsing rejects offending values with
`quill::format_literal_reserved_char`. There is no escape or quoting
fallback; authors needing these characters must reshape their values.

## Typed tables

A field of `type: array` whose `items:` declare a `properties` map. The array is
a **cell**, so it keeps its own literal and the cascade is the uniform one:
`default:` (any default, including `[]`) is shippable as-is; without one:

- A non-empty `default:` renders as actual rows (no per-property
  annotations on each row). The outer key carries `# array<object>`.
- `default: []` renders inline as `[]` with `# array<object>`:
  shippable empty. Inline row shape is not surfaced under an empty
  default; use `example:` to document row shape.
- Without a `default:`, one synthetic row is emitted with each property
  carrying its own description, inline annotation, and its own derived
  `!must_fill` marker — so a row property holding a `default:` renders that
  value unmarked. The container key itself is untagged: you tag the leaves, not
  the container (per [markdown-spec.md](../references/markdown-spec.md) §3.4).
  The outer key carries `# array<object>`.

The row schema is a namespace, so it declares no `default:` / `example:` of its
own (`quill::default_on_namespace`). The array's literal is where element values
go, and each element it supplies is completed against `items:`.

An `example:` never renders as rows. Like every other field type, it
surfaces only in the `# e.g.` leading line: as a one-line flow
sequence, e.g. `# e.g. [{org: ACME, year: 2020}]`.

## Typed dictionaries

A field of `type: object` with a `properties` map carries **no cell of its own**:
it is a namespace, and a `default:`/`example:` on it is a load error
([SCHEMAS.md](SCHEMAS.md) § "Cells and namespaces"). So it always expands, and
there is no container-level cascade to choose between:

- Each property is emitted with its own description, inline annotation, and
  its own cell cascade — `default:` › `example:` › blank under the derived
  `!must_fill` marker — exactly as a card-level field of that type. A property
  carrying a `default:` therefore renders that concrete value unmarked, and a
  wholly skippable dictionary is spelled by each property carrying a type-empty
  one.
- The container key itself is untagged: you tag the leaves, not the
  container (per [markdown-spec.md](../references/markdown-spec.md) §3.4). The
  outer key carries `# object`.

The cells the blueprint shows are the cells the render floor fills, from the same
declarations: an author who deletes a defaulted line gets that default back at
render, which is what the "shippable as-is" affordance promises.

An `array` differs because it is a cell: its own `default:` renders, and
`default: []` stays inline `[]`.

A property's `example:` behaves as a card-level field's does — it takes the cell
when no `default:` holds it, and surfaces in the `# e.g.` leading line otherwise.
The container declares none (`quill::example_on_namespace`).

```
# The sender's mailing address.
address: # object
  # Street address line.
  street: !must_fill # string
  # City name.
  city: !must_fill # string
  # ZIP or postal code.
  zip: "" # string
```

With a `default:` on the first two properties and an `example:` on the third:

```
# The sender's mailing address.
address: # object
  # Street address line.
  street: 5000 Forbes Ave # string
  # City name.
  city: Pittsburgh # string
  # ZIP or postal code.
  zip: !must_fill "15213" # string
```

A property is an ordinary field carrying whatever type a card-level field
carries, itself included, so the expansion recurses to whatever depth the schema
declares. A freeform `type: object` without a `properties` map is rejected at
`Quill.yaml` parse time (`quill::object_missing_properties`).

## UI metadata honored

Field declaration order controls field ordering within the document:
carried structurally by the schema's ordered field maps, not a `ui`
key. The `ui:` keys (`ui.group`, `ui.compact`, `ui.multiline`,
`ui.title`) are presentation-only and do not affect blueprint output. In
particular, `ui.group` emits no banner lines; fields within the same
`ui.group` cluster together while preserving declaration order.

## Body markers

- `Write main body here.` after the root block's closing `~~~`
- `Write <card_kind> body here.` after each card block's closing `~~~`
- When `body.example` is set, its text replaces the marker verbatim.

`body.enabled: false` suppresses the marker entirely for body-less cards
(e.g., a `skills` card whose data is purely structured).

A composable card's emitted block — its `$kind` line, the `composable
(0..N)` / sample-card comments, its fields, and its placeholder body — is
one sample instance of that kind, not a required one. Delete the whole block
when the document needs none of that kind.

A `body.example` whose text contains a line that would parse as a
card-yaml opener (any column-zero `~~~`) is rejected at `Quill.yaml` parse
time (`quill::body_example_contains_fence`) to prevent corrupting the
blueprint's document structure.

## Worked example

```
~~~
$quill: cmu_letter@0.1.0 # keep verbatim
$kind: main
# Typeset letters that comply with Carnegie Mellon University letterhead standards.
# The recipient's name and full mailing address.
recipient: !must_fill # array<string>
  - Mr. John Doe
  - 123 Main St
  - Anytown, USA
# The signer's information. Line 1: Name. Line 2: Title.
signature_block: !must_fill # array<string>
  - First M. Last
  - Title
# The department or organizational unit name for the letterhead.
# e.g. Department of Electrical and Computer Engineering
department: "" # string
# The sender's institutional mailing address.
address: !must_fill # array<string>
  - 5000 Forbes Avenue
  - Pittsburgh, PA 15213-3890
# The department or university website URL.
# e.g. www.ece.cmu.edu
url: "" # string
# The date to appear on the letter.
date: !must_fill # date<YYYY-MM-DD>
~~~

Write main body here.
```

## Guarantees

`blueprint()` guarantees the emitted document is **parseable** *and*
**renders**: every field key is present, every value is YAML-valid, the
document round-trips through `Document::parse` and back, and every
cell is type-valid. A defaulted cell coerces and validates against its default;
a defaultless cell carries the `!must_fill` marker on a value that is either the
field's `example` (a real, type-valid suggested value) or bare null/empty:
and because **null ≡ absent** (a present-null cell blank-fills at render, just
like an omitted field), even a bare-marked cell renders cleanly. A surviving
marker is surfaced by `Quill::validate` as the **non-fatal**
`validation::must_fill` warning: never a render gate. A strict consumer
(e.g. an LLM authoring loop) treats any outstanding marker as "not done."

Rendering still depends on the quill's `plate.typ` and its packages, which
`blueprint()` does not control. That is a separate **quill authoring
contract**:

> A quill's `plate.typ` MUST render an **empty document** (just `$quill` /
> `$kind: main`, no fields) to a successful (non-error) output. Under
> blank-filled render, every absent field is filled with its blank in the
> plate projection, so an empty document is by
> construction the *type-minimal valid input*.

It is the worst-case-but-renderable document, so a plate that renders it
degrades gracefully on every type-valid input shape. The contract requires:

- Templates treat blanks (`""`, `0`, `false`, `[]`, empty richtext body) as
  valid *present* input: read via `data.field`,
  `card.at("field", default: …)`, or guarded with `if "field" in data`.
- **A template branching on an `enum` covers `values ∪ blank` exhaustively.**
  The blank is valid present input for every enum, not only defaultless ones,
  so an `else` fallback silently renders a variant nobody chose — the exact
  fabrication the blank exists to close, re-opened one rung lower. Match the
  blank explicitly and decide what it means: omit the parameter, take the
  downstream package's own default, or render nothing. Note that
  `data.at(key, default: X)` is **not** such a guard: under blank-filled
  render every declared key is always present, so its `default:` is dead code
  and the blank flows through. Where a package asserts membership, that is a
  failed compile rather than a quiet mis-render.
- **A template reads a variant field only inside the branch that selects its
  world.** A variant-bearing enum arrives as `{value: …}` carrying exactly the
  live world's fields, so `data.c.value == "CUI"` is what makes `data.c.poc`
  total; reading it outside that branch is a missing-key error on every other
  document. The exhaustive branch above is therefore not just an obligation here
  but the access path (`SCHEMAS.md` § "Enum variants").
- No template asserts that a must-fill field is *non-empty*. The schema
  guarantees *presence*, not non-emptiness; the `!must_fill` marker
  is an authoring signal, not a render-time precondition.
- "Renders successfully" means "compiles without error," not "produces
  meaningful output." An empty-string title is a blank title: that is
  acceptable.

The contract is enforced by fixture tests that render each bundled quill's
empty document (`quiver_test.rs::every_quill_in_quiver_renders`) and, for the
`blueprint()` guarantee above, parse, round-trip, and render each quill's
generated blueprint (`quiver_test.rs::every_quill_blueprint_round_trips_and_renders`).

## The blueprint and its filled-out twin

The blueprint is the **one** annotated reference document. Its "show me a
filled-out one" counterpart is **seeding**, which materializes a real
`Document` rather than a second annotated string: nothing consumes a
filled-out document for its annotations, so that projection is committed
content, not prose.

| Projection | Intent | Value precedence | Output | Markers? |
|---|---|---|---|---|
| `blueprint` | *"give me the form to fill"* | value: `default:` › `example:` › bare; marker: the derived `must_fill` | annotated string | yes (`!must_fill`) |
| seeding | *"give me a filled-out one"* | `example:` › absent | committed `Document` | no |

The **blueprint** column is this doc's contract (above). The **seeding**
column: value precedence `example: → absent`, with `default:`/`blank` deferred
to the render floor: is owned by [SCHEMAS.md](SCHEMAS.md) § "Document
seeding"; a seeded document renders each field's `example:` where present, else
the render floor's `default: → blank` (`blank`, [SCHEMAS.md](SCHEMAS.md)
§ "Blank-filled render").

## Bindings surface

| Binding | Accessor |
|---|---|
| Rust | `QuillConfig::blueprint() -> String`; the filled-out twin is `Quill::seed_document() -> Document` |
| Wasm | `Quill.blueprint` getter; `Quill.seedDocument()` |
| Python | `Quill.blueprint` property; `Quill.seed_document()` |
| CLI | `quillmark blueprint <QUILL_PATH>`; `render` with no input file renders the **seeded** document |

The Rust example `cargo run -p quillmark-core --example print_blueprint
-- <quill_name> [<version>]` prints the blueprint for any bundled
fixture.

## Writing the literal text `!must_fill` as content

The placeholder is a YAML **tag**, not a string sentinel, so there is no
collision and no quoting escape-hatch to learn. The literal text `!must_fill`
written as an ordinary *value* (`note: "!must_fill"`, or even an unquoted
scalar that merely contains those characters) is just content; a real marker
is the YAML tag attached to a field (`note: !must_fill`). The two are
structurally distinct, so nothing special is required to author the literal
text.
