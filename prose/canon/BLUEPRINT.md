# Blueprint Emission (`QuillConfig::blueprint`)

> **Implementation**: `crates/core/src/quill/`

## TL;DR

`blueprint()` produces an annotated Markdown document: the same shape an
author would write, each cell holding its non-blank `default:` or left empty, with
labels and constraint hints as comments. It is the **authoring surface** for
LLM and MCP consumers; [SCHEMAS.md](SCHEMAS.md) covers the validation/form
surface.

A blueprint is the document, not a description of the document. Answer the
empty cells and write the bodies; the structure and `$` metadata come for free.

## Output shape

````
~~~
$quill: <name>@<MAJOR.MINOR> # keep verbatim
$kind: main # <title> — <description>
# <field title> — <field description>
field: # <type>
settled: value # <type>[<format>]
~~~

~~~
$kind: <card_kind> # <card title> — <card description>
# composable (0..N)
# sample card; delete if not needed
...fields...
~~~
````

Every block is a bare `~~~` block (the canonical card-yaml fence; the opener's
info string is ignored; see
[markdown-spec.md](../references/markdown-spec.md) §3): the root block carries
the `$quill` system-metadata line; each composable card carries a
`$kind: <card_kind>` metadata line.

Every body is empty; see "Bodies".

## One emitter, by construction

`blueprint()` does not format YAML itself. It builds a `Document`: the same
typed model a parsed `.md` produces, with prose annotations as comments, and
emits it through the **canonical
`Document::to_markdown`**. There is no second formatter. Two consequences
follow:

- The blueprint round-trips through `Document::parse` and back **by
  construction**: the emitter that produced it is the same one round-trip uses.
- The blueprint inherits `to_markdown`'s representation choices: a **one-space**
  ` # ` inline-comment gap, **block-style** sequences at every level (no inline
  flow), and multi-line strings as **`|` literal blocks** where one reads back
  as the same string, double-quoted elsewhere
  ([markdown-spec.md](../references/markdown-spec.md) §9).
  The sections below reflect those choices.

## Annotation grammar

| Slot | Form | Carries |
|---|---|---|
| **Leading `# …` lines** above a field | `# <prose>`, `# up to <N>` or `# e.g. <members>` | label (single-line prose), an `array`'s element cap, and a matrix's member hint |
| **Inline `# …`** at end of the value line | `# <type>[<format>][?]` | structural metadata: the field's type, an optional format refinement, and the optional-cell marker |
| **Body line** closing a card's payload | `# no body` | that the kind takes no body (see "Bodies") |

The two slots divide by *grammar*, not by subject: the inline slot is the fixed
`<type>[<format>]` expression and takes nothing else, so a constraint that is not
part of a type expression rides the leading slot as prose (`# up to <N>`, the one
such line). No annotation is colon-separated `key: value`, so no annotation
collides with YAML key/value parsing.

Two own-line forms are not annotations: a dormant variant world, which is cells
with a `# ` in front under a `# when <MEMBER>:` header (see "Enum variants"),
and a typed table's field commented out above its live cell, `[]` or empty
where optional (see "Typed tables"). The colons in them are the cells' own.

### Leading lines: order

Per field, in order:

1. `# <title> — <description>`: `title:` and `description:` from
   `Quill.yaml`, each whitespace-collapsed, or whichever one is declared.
   **Single line only**; multi-line descriptions are rejected at `Quill.yaml`
   parse time. The title is the name a person uses for the field, which an
   agent otherwise sees only as the key; it prints only where declared, so a
   title restating the key is visible waste rather than a default.
2. `# up to <N>`: emitted where a field or an `object` property declares
   `max:` — the two positions the leading slot exists at. The cap is the count
   past which the surplus leaves the page or the record, and the author
   most likely to exceed it is the MCP flow that reads the spec once and then
   writes. `schema()` is consulted before that author writes and the
   `validation::cardinality` warning arrives after the overflowing document
   exists, so a limit the blueprint does not show is a limit learned from
   `description:` prose. It rides the own-line slot in the form
   `# composable (0..N)` already takes for a card kind's cardinality, which
   leaves the `<type>[<format>]` grammar untouched. On a `text` or `richtext`
   field it reads `# up to <N> characters`. An array's
   elements have no slot of their own, so a cap its text `items:` declare rides
   the array's line: `# up to 4, each up to 350 characters`, or `# each up to
   350 characters` on an uncapped array.
3. `# e.g. <members>`: emitted for a `matrix` declaring columns or `open`,
   whose empty `{}` cell shows neither the columns nor the spelling of an
   added item. It holds one member, present and so held:
   `# e.g. {flight_cc: {detail: text<plain>, earned: date<YYYY-MM-DD | today>}}`, and on
   an open matrix an added item after it, keyed by the placeholder id
   `new_item`: `# e.g. {flight_cc: {detail: text<plain>}, new_item: {title: text<plain>, detail: text<plain>}}`.
   The member is the roster's first and is illustrative only; the line exists
   to name the columns and the `title` an added item needs. Each column, and
   the added item's `title`, shows its `default:`, else its container shape,
   else its inline annotation's `<type>[<format>]`: a type, not a value, quoted
   where flow syntax would split it. A closed checklist (no columns) has no
   line, the bare tick being its whole spelling; an open one's member is that
   tick, `flight_cc: true`. The line is a comment, so it takes the one-line
   flow form the emitter never writes for a value.

That's it. There is no leading `# required`, `# enum:`, `# default:`, or
`# type:`: those collapse into the inline.

### Inline annotation

Form: **`# <type>[<format>][?]`**

- **Type slot** (mandatory, first): one of
  `integer`, `number`, `boolean`, `array`, `object`,
  `text`, `richtext`, `date`, `datetime`, `enum`, `matrix`.
  Every field is labeled: there is no "self-evident" exemption.
  (`object` requires a `properties` map; freeform untyped objects are not
  supported. `object` also appears in the format slot of typed-table fields
  as `array<object>`.)
- **Format slot** (optional, in `<…>` angle brackets): refines the type
  when the refinement carries information beyond the type name itself.
  - `date<YYYY-MM-DD | today>`: a bare calendar date, or `today`, the render
    date ([SCHEMAS.md](SCHEMAS.md#the-render-date-today));
    `datetime<YYYY-MM-DDThh:mm[:ss]>`: an offset-less wall-clock datetime (no
    offset/space/fractional forms)
  - `richtext<markdown>`, `richtext(multiline)<markdown>`: the `<markdown>`
    slot names the surface encoding an author writes over the content model,
    and `(multiline)` marks a field that admits block structure, one paragraph
    being the default
  - `text<plain>`, `text(multiline)<plain>`: the `<plain>` slot names the
    literal codec (delimiters stay literal), distinct from `<markdown>`, and
    `(multiline)` a field that admits line breaks, one line being the default
  - `array<text<plain>>`, `array<integer>`, `array<object>`, `array<richtext<markdown>>`, …
  - `enum<a | b | c>`
  - `matrix<flight_cc | dodin_ops | cyber_200 | …>`: the roster's member ids in
    declaration order, structurally the `enum` form so no third annotation is
    minted. The cell carries the sparse spelling only — the matrix's
    `default:`, shippable as-is, else `{}` — so a model sees the whole
    vocabulary in the annotation and has nothing to delete. A closed matrix leaves it no
    member to invent; on an open one the ids, the annotation carrying no titles,
    are what steer it to a member rather than an added duplicate. The columns
    and the added spelling ride the leading `# e.g.` line
  - omitted for `integer`, `number`, `boolean`, `object`
    (nothing meaningful to refine).
- **Optional marker** (optional, last): a trailing `?` on an
  [optional cell](SCHEMAS.md#optional-cells)'s expression, as its `Quill.yaml`
  `type:` spells it (`text<plain>?`, `enum<a | b>?`, `date<YYYY-MM-DD | today>?`),
  and on an element type the same way (`array<text<plain>?>`).

The inline annotation is **purely structural**: it carries the type (and
optional format), nothing else. What a reader must *do* is carried by the cell:
an empty cell awaits a value, and a concrete value is the field's `default:`,
shippable as-is (delete or blank the line to fall back to the default).

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
so dropping it is not an error. The card's label rides the `$kind`
line's inline slot, for every card, in the field's form:
`$kind: <kind> # <title> — <description>`, or whichever one is declared. An
own-line `# …` there would read as a leading annotation for the field below
it, and an inline comment belongs to its own line. The root's label is
`main`'s own: `quill.description` describes the package and never reaches the
blueprint. A composable card's kind is carried in its
`$kind: <card_kind>` metadata line. Its `composable (0..N)` role is
emitted as an own-line `# composable (0..N)` comment directly under the
`$kind` line: that comment carries the
card's cardinality, which is structural information rather than a
redundant instruction. A second own-line comment, `# sample card; delete if
not needed`, follows directly under it: the card's fields and body are one
worked example of that kind, and "0..N" alone does not say a reader is free
to drop it.

Examples:

| Line | Reading |
|---|---|
| `name: # text<plain>` | defaultless text: an empty cell awaiting a value |
| `title: "Curriculum Vitae" # text<plain>` | defaulted text: concrete value, shippable as-is (keep or override) |
| `count: 0 # integer` | defaulted integer (type-empty default, shippable as-is) |
| `active: false # boolean` | defaulted boolean (type-empty default, shippable as-is) |
| `bio: # richtext<markdown>` | defaultless richtext (see "Richtext fields") |
| `recipient: # array<text<plain>>` | defaultless array of lines |
| `date: # date<YYYY-MM-DD \| today>` | defaultless date |
| `severity: # enum<low \| medium \| high>` | defaultless enum |
| `endorsements: # array<text<plain>>` under a leading `# up to 3` | a capped array: the cap is the leading line, the type the inline |
| `qualifications: {} # matrix<flight_cc \| dodin_ops>` | a matrix: the whole vocabulary in the annotation, nothing ticked; a leading `# e.g.` names its columns and an open matrix's added spelling, where it has either |
| `$quill: cmu_letter@0.1 # keep verbatim` | quill binding metadata, emitted verbatim; the inline reminder guards against dropping the line |
| `$kind: skill` followed by `# composable (0..N)` and `# sample card; delete if not needed` | repeat the entire `~~~` … `~~~` block per instance, or delete it if none are needed |
| `# no body` above a closing `~~~` | the kind takes no body: write nothing after the fence |

## Cell values

A cell's value is **non-blank `default:` › empty**. An empty cell is null, emitted as a
bare `key:` with its annotation after it (`title: # text<plain>`).

A **blank** `default:` (`""`, `[]` or `{}`, the field's own blank) renders
exactly as no default does, so it prints as none: `notes:` declaring
`default: ""` prints `notes: # text<plain>`, and a matrix column declaring one
reads in the `# e.g.` line as a defaultless column does. `0` and `false` are
values and print (`count: 0 # integer`): an empty `count:` would ask for a
number the quill already answers.

Every cell of the live world renders as **live YAML**; a dormant variant world's
cells are commented out, and only they are. The `# when <MEMBER>:` header says
which, and says they are disabled until that member is chosen. Every live cell
on the page keeps one rule: **an empty cell awaits a value; a concrete value is
shippable as-is**. An empty cell still renders: null ≡ absent, so it blank-fills
like an omitted field (see "Guarantees").

An author's `field: ""` and a blank/null `field:` (or `field: null`) are
different answers once the document is parsed: `""` is the field's own explicit
blank and is kept exactly as written, while `field:` / `field: null` carry no
data (**null ≡ absent**) and fall back to the field's `default:` when one is
declared, else to the field's own blank (`authored` › `default:` › `blank`; see
[SCHEMAS.md](SCHEMAS.md) § "Native validation"). Write `""` on purpose to force
a field empty; leave it blank or `null` to accept its default.

### Enum variants

A variant-bearing `enum` emits its container: the discriminant under `value`,
then every member of `variants:` under a `# when <MEMBER>:` header, in
declaration order (load rejects a member owning no cells, so every member has a
header). A blueprint **is** a document, so one world is live — the one the
discriminant names. The discriminant is the field's `default:`, else `""`, so
only a `default:` world is live. Every other world's cells are commented out, at
the slot they would occupy live. A reader activates a world by setting the
discriminant and deleting `# ` from that world's lines.

```
# Select the classification marking shown in the header and footer banner.
classification: # enum<UNCLASSIFIED | CUI | CONFIDENTIAL | SECRET | TOP SECRET>
  value: ""
  # when CUI:
  # # Office or organization that designated this information as CUI.
  # controlled_by: # text<plain>
  # # CUI category from the DoD CUI Registry. Leave blank to omit.
  # category: # text<plain>
```

One builder and one emitter serve both, so a dormant world is rendered exactly
as a live one and then handed over as own-line comments. `to_markdown` writes
the `# `, so a commented line is byte-for-byte the live line, and a comment
*inside* the world arrives double-prefixed (`# # …`) by the same rule. A cell's
position, type and description all cross.

`Document::parse` reads the block back as the comments it is, so a dormant cell
reaches neither the validator nor the render floor. Uncommenting one without
setting the discriminant strands it, which `validation::out_of_variant` names.

The container line carries the `enum<…>` annotation and `value` carries none:
`value` *is* that enum, so a second annotation would restate it.

A world a typed dictionary holds emits the same block at the dictionary's
indent: the worlds seat themselves in whichever container holds the
discriminant, so the cells a reader uncomments are the cells that container
takes.

### Richtext fields

A richtext field's value cell is markdown: the surface projection of the
content model, which `to_markdown` re-emits: carried under a `# richtext<markdown>`
annotation.

A defaultless `richtext` field renders an empty cell, no block scalar:

```
bio: # richtext<markdown>
```

The LLM writes its markdown content into the cell (a quoted scalar or a block
scalar, the consumer's choice).

When a non-blank `default:` is configured, the field renders its default in
the canonical `to_markdown` string form, a multi-line one as a literal block:

```
bio: |- # richtext<markdown>
  ## About me

  <body>
```

### Reserved characters in format and enum literals

To keep the inline grammar unambiguous, format slot contents; including
enum values: may not contain `>`, `;`, or `|`. These are the closing
delimiter, the role separator, and the enum-value separator respectively.
`Quill.yaml` parsing rejects offending values with
`quill::format_literal_reserved_char`. There is no escape or quoting
fallback; authors needing these characters must reshape their values.

## Typed tables

A field of `type: array` whose `items:` declare a `properties` map. The array is
a **cell**, so it keeps its own literal. The outer key carries
`# array<object>` either way:

- A non-empty `default:` renders as actual rows (no per-property
  annotations on each row), shippable as-is.
- Otherwise the cell is inline `[]`: shippable empty, as an unanswered table
  renders. An optional table (`array<object>?`) prints an empty cell instead,
  since `[]` answers it and it renders unanswered as `none`. The field leads
  either commented out, holding one synthetic row whose
  properties each carry their own description, inline annotation, and cell —
  its `default:`, else empty. That is the config-file spelling of an
  alternative: a reader adds a row by deleting the live line and uncommenting
  the block, and leaving both live is a duplicate key. The row is dormant
  because a live one would ship an empty element into the render. A `max: 0`
  table holds no row, so it has none.

```
# attendees:
#   -
#     # Full name.
#     name: # text<plain>
#     voting: false # boolean
attendees: [] # array<object>
```

The row schema is a namespace, so it declares no `default:` of its own
(`quill::default_on_namespace`). The array's literal is where element values
go, and each element it supplies is completed against `items:`.

## Typed dictionaries

A field of `type: object` with a `properties` map carries **no cell of its own**:
it is a namespace, and a `default:` on it is a load error
([SCHEMAS.md](SCHEMAS.md) § "Cells and namespaces"). So it always expands, and
there is no container-level cascade to choose between:

- Each property is emitted with its own description, inline annotation, and
  cell — `default:` › empty — exactly as a card-level field of
  that type. A property carrying a `default:` therefore renders that concrete
  value.
- The outer key carries `# object`.

The cells the blueprint shows are the cells the render floor fills, from the same
declarations: an author who deletes a defaulted line gets that default back at
render, which is what the "shippable as-is" affordance promises.

An `array` differs because it is a cell: its own `default:` renders, else
inline `[]`, or empty where optional.

```
# The sender's mailing address.
address: # object
  # Street address line.
  street: # text<plain>
  # City name.
  city: # text<plain>
  # ZIP or postal code.
  zip: # text<plain>
```

With a `default:` on the first two properties:

```
# The sender's mailing address.
address: # object
  # Street address line.
  street: 5000 Forbes Ave # text<plain>
  # City name.
  city: Pittsburgh # text<plain>
  # ZIP or postal code.
  zip: # text<plain>
```

A property is an ordinary field carrying whatever type a card-level field
carries, itself included, so the expansion recurses to whatever depth the schema
declares. A freeform `type: object` without a `properties` map is rejected at
`Quill.yaml` parse time (`quill::object_missing_properties`).

## UI metadata honored

Field declaration order is blueprint order, carried by the schema's ordered
field maps. The loader settles a legacy `ui.group` card into display order,
ungrouped fields first ([SCHEMAS.md](SCHEMAS.md#grouping)), so the blueprint
sorts nothing. A group emits no banner line; a top-level `object` prints as any
typed dictionary does, its description above it. The other `ui:` keys (`ui.compact`,
`ui.blank_title`, `ui.layout`) are presentation-only and do not
affect blueprint output, so none may hold a fact a writer needs: a label is
`title`, which the blueprint prints. `ui.layout` names a control an editor
draws, which a text blueprint has no second shape for.

## Bodies

Every body is empty: a body is a cell. A kind under
`body.enabled: false` closes its payload with `# no body`, directly above the
body, whose emptiness would otherwise read as awaiting prose; a kind taking a
body has no body line.

A composable card's emitted block — its `$kind` line, the `composable
(0..N)` / sample-card comments, its fields, and its body — is one sample
instance of that kind, not a required one. Delete the whole block when the
document needs none of that kind.

## Worked example

```
~~~
$quill: cmu_letter@0.1 # keep verbatim
$kind: main # Typeset letters that comply with Carnegie Mellon University letterhead standards.
# The recipient's name and full mailing address.
recipient: # array<text<plain>>
# The signer's information. Line 1: Name, as in First M. Last. Line 2: Title.
signature_block: # array<text<plain>>
# The department or organizational unit name for the letterhead.
department: # text<plain>
# The sender's institutional mailing address.
address: # array<text<plain>>
# The department or university website URL, as in www.ece.cmu.edu.
url: # text<plain>
# The date to appear on the letter.
date: # date<YYYY-MM-DD | today>
~~~
```

## Guarantees

`blueprint()` guarantees the emitted document is **parseable** *and*
**renders**: every field key is present, every value is YAML-valid, the
document round-trips through `Document::parse` and back, and every
cell is type-valid. A defaulted cell coerces and validates against its default;
a defaultless cell is empty, and because **null ≡ absent** (a present-null cell
blank-fills at render, just like an omitted field) it renders cleanly and draws
no diagnostic.

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
  valid *present* input: read via `qm.data.field`,
  `card.at("field", default: …)`, or guarded with `if "field" in qm.data`.
- **A template branching on an `enum` covers `values ∪ blank` exhaustively.**
  The blank is valid present input for every enum, not only defaultless ones,
  so an `else` fallback silently renders a variant nobody chose — the exact
  fabrication the blank exists to close, re-opened one rung lower. Match the
  blank explicitly and decide what it means: omit the parameter, take the
  downstream package's own default, or render nothing. Note that
  `qm.data.at(key, default: X)` is **not** such a guard: under blank-filled
  render every declared key is always present, so its `default:` is dead code
  and the blank flows through. Where a package asserts membership, that is a
  failed compile rather than a quiet mis-render.
- **A template reads a variant field only inside the branch that selects its
  world.** A variant-bearing enum arrives as `{value: …}` carrying exactly the
  live world's fields, so `qm.data.c.value == "CUI"` is what makes `qm.data.c.poc`
  total; reading it outside that branch is a missing-key error on every other
  document. The exhaustive branch above is therefore not just an obligation here
  but the access path (`SCHEMAS.md` § "Enum variants").
- No template asserts that a defaultless field is *non-empty*. The schema
  guarantees *presence*, not non-emptiness.
- "Renders successfully" means "compiles without error," not "produces
  meaningful output." An empty-string title is a blank title: that is
  acceptable.

The contract is enforced by fixture tests that render each bundled quill's
empty document (`quiver_test.rs::every_quill_renders_an_empty_document`) and, for
the `blueprint()` guarantee above, parse, round-trip, and render each quill's
generated blueprint (`quiver_test.rs::every_quill_blueprint_round_trips_and_renders`).

A quill outside that directory is bound by the same contract and reaches it
through `quillmark validate`, which renders all three canonical documents — the
empty document, the blueprint, the seeded document — and reports each failure
as a `cli::canonical_document_failed` error naming which of the three it is
([CLI.md](CLI.md)).

## The blueprint and the seeded document

The blueprint is the **one** annotated reference document. Its structured
counterpart is **seeding**, which materializes a real `Document` rather than a
second annotated string.

| Projection | Intent | Field values | Output | Annotated? |
|---|---|---|---|---|
| `blueprint` | *"give me the form to fill"* | non-blank `default:` › empty | annotated string | yes |
| seeding | *"give me a starter document"* | kind `seed:` › absent | committed `Document` | no |

The **blueprint** column is this doc's contract (above). The **seeding**
column is owned by [SCHEMAS.md](SCHEMAS.md) § "Document seeding": each card
carries its kind's `seed:` and no other field, so a seeded document renders
every other field at the render floor's `default: → blank`
([SCHEMAS.md](SCHEMAS.md) § "Blank-filled render").

## Bindings surface

| Binding | Accessor |
|---|---|
| Rust | `QuillConfig::blueprint() -> String`; the seeded document is `Quill::seed_document() -> Document`; the empty document is `Quill::empty_document() -> Document` |
| Wasm | `Quill.blueprint` getter; `Quill.seedDocument()`; `Quill.emptyDocument()` |
| Python | `Quill.blueprint` property; `Quill.seed_document()`; `Quill.empty_document()` |
| CLI | `quillmark blueprint <QUILL_PATH>`; `render` with no input file renders the **seeded** document; `validate` renders all three |

The Rust example `cargo run -p quillmark-core --example print_blueprint
-- <quill_name> [<version>]` prints the blueprint for any bundled
fixture.
