# Schema Model (`QuillConfig`)

> **Implementation**: `crates/core/src/quill/`

## TL;DR

`QuillConfig` is the only schema model in quillmark. Validation, coercion, defaults extraction, and public schema emission all read directly from it.

## Quill.yaml DSL

Schema authoring lives in `Quill.yaml` under:

- `main.fields`
- `card_kinds.<card_name>.fields`
- optional `ui` and `body` blocks on `main` and each card kind

**What earns a key here.** A key earns its place when some surface is otherwise
*wrong*, not merely underserved, and wrong needs a witness: a document that
overflows, misrenders, validates falsely, or is edited by the wrong control. A
preference is not a witness. A key then states its behavior on all four
surfaces — plate, editor, blueprint, `validate` — and "inert here, deliberately"
is a valid answer that is written down. A key costs its teaching surface from
the day it exists, which is why the bar is the defect and not the convenience.

Supported field types:

| Quill.yaml Type | Meaning |
|---|---|
| `string` | Open scalar UTF-8 text: a value the template computes with (URL, path, identifier, reference key), not prose it lays out |
| `enum` | Closed string domain of **choices**; requires a `values:` list. Also accepts its blank (`""`), which is never a declared member — declaring one is a load error (`quill::enum_blank_member`). Projects to JSON-Schema `{type: string, enum: ["", …]}`. `values:` on any other type, and `enum:` on any type at all, is a load error |
| `number` | Numeric value (integers and decimals) |
| `integer` | Integer-only numeric value |
| `boolean` | `true` / `false` |
| `array` | Ordered list; requires an `items:` element schema (e.g. `items: { type: string }` for `string[]`, `items: { type: object, properties: … }` for a typed table). Optional `max:`, the element count past which the surplus leaves the page ([Cardinality](#cardinality)) |
| `matrix` | A closed vocabulary someone ticks; requires a `members:` roster. A namespace whose keys the roster fixes, each member an object of a synthesized `held` plus the field's `properties:` (the columns). See [Matrix](#matrix) |
| `object` | Structured map; requires `properties:` |
| `date` | A strict calendar date `YYYY-MM-DD`, or the keyword `today` ([The render date](#the-render-date-today)). Rejects any time component (a time-bearing string is a `datetime`, not a truncated date). The common case in a document engine, so it is the unmarked date type. Stored verbatim; lowers to a native Typst `datetime(year:, month:, day:)`, with `display(<addr>, ..)` for a click-to-edit rendering (see `PLATE_DATA.md`) |
| `datetime` | A strict offset-less wall-clock datetime `YYYY-MM-DDThh:mm[:ss]`, seconds optional (zero-filled). Rejects timezone offsets (`Z`, `±HH:MM`), the space separator, fractional seconds, and a bare date (which is a `date`). An offset is **rejected, never dropped**: the engine does no zone math, keeping wall-clock semantics end to end. Stored verbatim; lowers the same way over the six-component `datetime(year:, .., second:)` |
| `plaintext` | Navigable **unformatted** prose over the same canonical content (`Content`) as `richtext` (same media type, nav, and regions) but a **literal** codec (`from_plaintext`/`to_plaintext`): delimiters stay literal, no markup, verbatim round-trip. Declare `inline: true` for the single-line variant. Constrained mark-/island-free (`Content::is_plain`); a formatted wire content is rejected (`validation::not_plain`), not stripped. **Rests as the literal string** — in the *document*. A plate receives the content object, exactly as for `richtext` (no backend reads the `plaintext` annotation): there is deliberately no plate-side content→`str` projection, since no plate has needed one |
| `richtext` | Rich **formatted** prose over a canonical content (`Content`); markdown is a projection of it. Declare `inline: true` for the single-line variant (exactly one `Para` line, no container, no islands). The pre-richtext `markdown` spelling and the retired `type: richtext(inline)` token are schema load errors (`quill::field_parse_error`). **Rests as the canonical content object** |

### Optional cells

A trailing `?` on a cell's type token (`integer?`, `boolean?`, `string?`,
`enum?`, `date?`, `richtext?`, `array?`, …) makes the cell **optional**: left
unanswered, it renders `none` rather than its type's blank. The `?` moves the
render floor and nothing else.

| Declaration | Unanswered renders | Plate reads |
|---|---|---|
| `type: t` | the type's [blank](#blank-filled-render) | always a `t` |
| `type: t` + `default:` | the default | always a `t` |
| `type: t?` | `none` | a `t` or `none` |

- **Exclusive with `default:`** (`quill::optional_default`). A default answers
  for an unanswered cell, so the cell would never render `none`. `example:`
  illustrates without answering.
- **A cell only.** `object`, `matrix` and a variant-bearing `enum` are
  namespaces with no rung of their own (`quill::optional_namespace`); their cells
  take the `?` instead.
- **An authored value is an answer**, `0`, `false`, `""` and `[]` included. An
  `enum?`'s `""` is the exception: it is the blank's own spelling, so it renders
  `none`.
- **The wire admits `null`.** The transform schema projects `type: [t, "null"]`,
  and an `enum?` lists `null` beside its blank. The declaration view and the
  blueprint annotation keep the `?`.
- **A plate guards `none` only where a schema says `?`.** Printing `none`
  places nothing and `+` absorbs it; arithmetic, comparison, `if` and `for`
  reject it, so those branch on `!= none` first.
- **An editor offers a way back to unanswered** on an optional cell:
  `removeField`, the one unset verb ([Native validation](#native-validation)).

### Enum variants

An `enum` may declare `variants:`, a per-member field set that exists only in the
world where the discriminant holds that member:

```yaml
classification:
  type: enum
  values: [UNCLASSIFIED, CUI, CONFIDENTIAL, SECRET, TOP SECRET]
  variants:
    CUI:
      controlled_by: { type: string }
      poc:           { type: string }
      category:      { type: string, default: "" }
```

`variants:` is the one key that changes a field's **resting shape**: the field
rests as `{value: <member>, …the member's fields}` at every projection, where a
variantless enum rests as a bare string. A document authors it as the container,
and the bare scalar (`classification: CUI`) is accepted as the spelling of a
world carrying no variant answers — coercion normalizes both, so one shape
reaches every surface downstream. `value` is reserved
(`quill::variant_reserved_field_name`): it names the discriminant.

This is the DSL's only cross-field shape, and it buys two things the flat map
could not say:

- **Existence.** A `cui_`-style name prefix is hand-written namespacing that only
  prose can scope. Nesting supplies the namespace structurally, and the names
  shorten to what they mean.
- **A UI signal.** `variants:` is keyed by member on the declaration view
  ([`Quill::schema`](#schema-emission)), so an editor shows and retires
  cells as the discriminant changes instead of hard-coding the rule.

**The wire carries exactly the live world, and totality is per-world.** The
render floor emits `value` plus the selected member's fields — blank-filled as
usual — and nothing else: the container is a *closed* shape, so a payload never
reaches a plate under a tag that disowns it. A plate is already obliged to branch
over `values ∪ blank` ([Blank-filled render](#blank-filled-render)); inside that
branch every declared field of that world is present, so the access needs no
guard, and outside it there is nothing to guard. The blank owns no field set, so
an unanswered discriminant renders `{value: ""}` and the empty-document contract
is untouched.

**A stranded value is carried, not dropped.** An authored cell whose variant is
out of play stays in the document and draws the non-fatal
`validation::out_of_variant`; the render floor omits it. Dropping it at coercion
would spend the author's answers on the ordinary editor gesture — choose CUI,
fill the block, flip to UNCLASSIFIED to compare, flip back — and gating render
would hand them an undraftable document. Only the wire is strict.

The ceiling is deliberate and enforced at load rather than discovered at render:

| Rule | Code |
|---|---|
| `variants:` on a non-enum field | `quill::variants_on_non_enum` |
| a key outside `values:` (the blank owns no variant) | `quill::variant_unknown_value` |
| `variants:` outside a card field or a typed dictionary's property | `quill::variant_placement` |
| an empty `variants:` map, or an empty variant | `quill::variant_empty` |
| a variant field named `value` | `quill::variant_reserved_field_name` |
| a name two variants declare *differently* | `quill::variant_field_collision` |

A variant cell carries any type a card field may — prose, dates and containers included. Every surface reaches it through the same dispatcher a card field uses — coercion through `conform_value`, validation through `validate_value`, the render floor through `resolve_value`, lowering through the schema-node walk ([PLATE_DATA.md](PLATE_DATA.md)), the content read through `get_content_at` — so it behaves as a card-level field of that type does. What a cell does not carry is `ui.group` (it inherits the discriminant's) or `variants:` of its own.

**Where a world may open.** Every other container's shape is a function of the
schema; a variant's is a function of the schema *and* the discriminant. The
transform schema projects the union of the worlds — at schema time there is no
live world — and the wire carries whichever one the document selects. Four
things hold because that gap is exactly one level deep:

- `variant_field` resolves a name by a flat scan across the worlds, which is
  what lets `quill::variant_field_collision` guarantee one name is one cell.
- A form binds once at open, so a cell is *unconditionally addressable* while
  only *conditionally live*.
- A plate branches once over `values ∪ blank` and needs no guard inside that
  branch.
- `validation::out_of_variant` names one discriminant rather than a chain.

So the rule is about that gap, not about depth. A **typed dictionary's property**
carries `variants:` for the same reason a card field does: the dictionary's own
live shape is the schema's, so a world opened there deepens the *address* and
leaves the gap one level. A card's `header.classification` reads and binds as
`classification` does, one step down.

Every other position would deepen the gap instead, and each is refused at load:
an array element, whose liveness would vary per index against a form bound once;
a matrix column, inside a grid the page prints in full; and another variant's
cell, which would make the plate's one branch a tree and the strand diagnostic a
chain. The ban is **sticky** — an object inside any of them inherits it, rather
than laundering a world into a position that cannot hold one.

Two limits follow from the container shape and are accepted, not worked around:
[`resolve()`](#the-resolved-value-view-resolve) reports **one** rung for the whole
container — the strongest that contributed — as it does for a typed dictionary;
and a field set **shared** across
several members is spelled by repeating it or sharing a YAML anchor, since a
variant keys on one member.

A cell is addressable one step down, exactly as a typed dictionary's property is
([PLATE_DATA.md](PLATE_DATA.md#schema-addresses)): `classification.poc` binds a
`form-field` widget or a `field-region` claim on either backend, and
`classification.value` the discriminant. Addressing is against the *schema*, so a
cell is bindable in every world — a form is built once and the document selects
its world later. The whole container is not bindable: its value is the container
object, which no widget coerces.

A repeated name is one **cell** of the container, not one per world: the coercion
lookup and the transform schema both key on the name alone, never the
discriminant. So every variant declaring a name must declare it identically —
`quill::variant_field_collision` rejects disagreement at load, rather than letting
a live value coerce under another world's type.

The text-ish types form a **data vs content** × **open/plain vs closed/formatted**
2×2: `enum` (closed data), `string` (open data), `plaintext` (plain content),
`richtext` (formatted content). Navigation/regions are a property of the content
model, so `plaintext` and `richtext` share the entire nav/region/preview
stack and the same backend lowering (both carry `contentMediaType:
application/quillmark-content+json`); `plaintext` additionally carries
`quillmark:plain: true`, an editor-only annotation backends ignore.

### Matrix

A `matrix` is a closed vocabulary the page prints in full, where the author
ticks what they hold and may annotate a tick:

```yaml
qualifications:
  type: matrix
  members:                        # ordered; id: Title
    sq_cc_candidate: Sq/CC Candidate
    flight_cc: Flight CC
    dodin_ops: DODIN Ops
    dco: DCO (Defensive)
  properties:                     # the columns; empty is a checklist
    detail: { type: plaintext, inline: true, default: "" }
```

**A namespace.** The roster fixes the keys, so the matrix carries no literal of
its own: `default:` / `example:` on it is
`quill::{default,example}_on_namespace`, the [Cells and
namespaces](#cells-and-namespaces) rule with no exception. Every member is an
`object` of a synthesized `held: {type: boolean, default: false}` beside the
declared columns, so a matrix is skippable by construction and an absent one
blank-fills to every member unheld, columns at their blanks.

**Members.** Ids are snake_case identifiers
(`quill::invalid_matrix_member`); titles are display. Ids are what the wire, the
address and the document speak. The roster is a mapping, so it has one slot per
id exactly as the stored value does and a duplicate is unspellable on both
sides. The two keys the matrix writes onto every member itself — `held`,
`title` — are reserved as column names (`quill::matrix_reserved_column`): a
column under one of them would load, validate and address, then lose to the
projection.

**Document.** A mapping keyed by member id, sparse. A bare scalar is the tick
itself, and coercion normalizes it to the member object — the [variant
precedent](#enum-variants), where the bare `classification: CUI` is the spelling
of a world carrying no variant answers. A mapping is the member object already,
so the tick it names no key for takes the ordinary ladder as every other absent
cell does, to the synthesized `default: false`.

| Stored | Means |
|---|---|
| key absent | not held |
| `cyber_200: true` | held, columns at whatever the ordinary ladder gives them |
| `flight_cc: { held: true, detail: X }` | held, with columns |
| `flight_cc: { detail: X }` | not held; the detail is retained in the document |
| `flight_cc: { held: false, detail: X }` | not held; the detail is retained in the document |

A mapping has one slot per key, so a duplicate is unspellable. An id outside the
roster is refused as an out-of-domain enum member is
(`validation::enum_violation`).

**Plate.** Total, like every container: every member present in declaration
order, each `{held, title, …columns}`. A held member's columns cut the
ordinary ladder — the authored value, else the column's `default:`, else its
blank. `title` is the projection's, written from the roster rather than held as
a cell, so it carries no address and a document authoring one is overwritten.
**The wire carries the
live world only**: an unheld member's columns render at their blanks whatever
the document retains, the closed shape variants already hold, so a plate reads
`held` and its columns without a guard and never prints a stranded answer. At
the plate and under [`resolve()`](#the-resolved-value-view-resolve),
`held: false` and an absent key are one value, the boolean blank; a retained
answer is a fact about the stored form alone, which is what makes tick, type,
untick, retick lossless.

**Address.** `qualifications.flight_cc.held` and
`qualifications.flight_cc.detail` are ordinary cells: a region on the Typst
backend, a widget on acroform. Writing `held` touches no sibling, an editor
unticking by writing `held: false` rather than dropping the key.

**Blueprint.** The blueprint shows the vocabulary through its roster
([BLUEPRINT.md](BLUEPRINT.md#inline-annotation)) and the columns through one
illustrative held member in the `# e.g.` line; a filled specimen is the quill's
maximal fixture.

**Implementation.** Sugar over a typed dictionary: the loader expands members
into an `object` whose properties are the member ids, reached through
`FieldSchema::namespace_props`, so coercion, validation, blank-fill and
addressing are inherited. Two things are the type's own — `title` written onto
the wire, and the closed wire for unheld members — and one walk is overridden
rather than inherited: the blueprint, which emits the sparse cell instead of
expanding every member.

### Cardinality

`max:` on an `array` is the element count past which the surplus leaves the page
the field is laid out on: page geometry, not style. A non-negative integer, and
a `default:` / `example:` longer than it is `quill::{default,example}_over_max`:
a default past the cap overflows the page of a document nobody authored, and an
example past it teaches the overflow.

`Quill::validate` warns `validation::cardinality` at the field's own path, args
`{max, actual}`, at every depth: an array nested in a typed dictionary, a matrix
member, a live variant world, or another array's elements is capped by its own
declaration. The surplus is unclaimed input
([What blocks a render](#what-blocks-a-render)), never a gate — fatal ≡ won't-render is an
invariant of the diagnostic model ([ERROR.md](ERROR.md#warning-flow)), and a
document over the limit renders with the plate's own rule for the surplus.

There is no `min:`. `min: 1` is `required:` by another name, and no cell is
required: an unanswered one renders its `default:` or its blank
([`default` and `example`](#default-and-example)). It would also contradict a
sibling `default: []`.

### Content fields rest per codec

A content field's **resting form** is the shape it is stored in once anything
schema-aware has written it: the typed writer, the seeder, or `Quill::conform`
(the bound door, [BINDINGS.md](BINDINGS.md)). It is per-codec, and the split is
forced, not chosen:

| Codec | Rest | Why |
|---|---|---|
| `richtext` | the canonical content object | the markdown projection is lossy (anchors, island ids, content-only marks), so string rest loses identity |
| `plaintext` | the literal string | `from_plaintext`/`to_plaintext` are inverses on plain content and `is_plain` excludes every mark, so string rest loses nothing, while object rest corrupts at emit |

Emit is schema-free: `project_content_field` routes every canonical content
object in a field's value, at any depth, through `export::to_markdown`, and it
cannot sniff the codec from the shape (a `richtext` content that happens to be plain is
indistinguishable from a `plaintext` one). An object-rest `plaintext` field
holding `a *literal* line` would therefore emit markdown-escaped
(`a \*literal\* line`), and a re-parse would read the backslashes as
characters. String rest removes that; the plate is unaffected, since the render
floor still coerces `plaintext` to the content object backends receive
([PLATE_DATA.md](PLATE_DATA.md)).

Rest is enforced only for a **declared content field**: one whose type tree
bears a content leaf (`field_contains_content`), and its whole subtree conforms
with it. Non-content-typed fields keep their authored shorthands; the typed
write remains their canonicalizer.

**A content leaf is readable at its codec wherever it sits in that subtree, not
only when the field itself is one.** `reader.get_content(name)` answers for a
whole-field leaf; `reader.get_content_at(name, path)` walks the same `items` /
`properties` / `variants` axis conform walks, reaching an `array<richtext>`
element, an `object`'s content property, a leaf under both, or a variant's cell.
`reader.get(name)` projects the same subtree in one read, exporting at every
leaf it reaches: an `array<richtext>` reads as an array of markdown strings, and
a mixed `object` reads its content property as text beside its verbatim
scalars. A field whose type tree bears no content leaf reads as stored, the walk
being the identity on it, and a present-null reads `null` at every type. That
is the [values form](#the-values-form).
Without it the caller reads the stored element and decides for itself what the
bytes mean, which is the judgement the resting form exists to remove. The caller
also has less to decide with: the codec is a schema fact, and the stored shape
does not carry it.

### A declared type change rewrites stored values

Changing a field's declared type reinterprets every stored value in that field at the next bound load: `Quill::conform` derives rest from the current schema and value, holding no record of the type a value was written under, so the reinterpretation is unconditional and carries no diagnostic.

The mechanism is deliberate. A type change migrates a deployed corpus with no migration script, and read-repair is the documented convergence path ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Byte-stability").

Scalar → content is the lossy direction: the stored string enters the codec's import, markup delimiters are consumed as structure, and the authored text is not recoverable. A `subject` holding `Cost * Benefit Analysis *DRAFT*` rests verbatim under `string`; under `richtext` it rests as content whose text is `Cost * Benefit Analysis DRAFT` carrying an emph mark, the literal asterisks gone.

**A declared type change is therefore a new quill version**, the rule [VERSIONING.md](VERSIONING.md) § "Ref Immutability" states for any content behind a canonical ref. Nothing enforces it: `check_quill_reference` compares the document's `$quill` name and selector against the loaded quill, and an in-place schema edit at the same version leaves both matching, so a document pinned to `name@0.2.0` conforms against a schema it was never authored under and the mismatch is unrepresentable.

## Type coercion

`QuillConfig::coerce_payload` and `coerce_card` run before validation.

- Returns `Result<IndexMap<String, QuillValue>, CoercionError>`
- Coerces top-level fields and per-card fields to their declared types
- Fails fast (`Err`) on the first value that cannot be coerced

**Coercion is the type predicate; validation reads its result.** `validate_value`
conforms each document value through `conform_value` at `Leniency::Render`
before judging it, so a type has one predicate. **A fatal `validation::*`
diagnostic means the document does not render.** The pairing holds in both
directions: a value the floor refuses carries one.

The value a document rests at is a separate question from the value the floor
builds. `letterhead_caption: HEADQUARTERS` rests as the authored scalar and is
valid, because the floor wraps it: a bare scalar is a spelling of a one-element
list, and the schema is what disambiguates it. Landing a document at a canonical
resting form is `Quill::conform`'s job, not validation's.

Conforming runs per node, so an element the floor refuses does not mistype its
siblings: `counts: [true, "abc"]` under `integer` items is one mismatch, at
`counts[1]`. A leaf the floor refuses is judged as authored, so the type check
names the value the author wrote, and is a `type_mismatch` even where the
authored shape reads well-typed — a content object that is not canonical
content, an integer past `i64` — unless a shape check already names the refusal
(`not_inline`, `not_plain`, `format_violation`). A container's refusal is the
element's or property's, reported at that path.

Validation adds the arbitration coercion cannot carry — the enum domain, the
datetime grammar, indexed element paths, the inline/plain content shapes — over
the value the floor built. A scalar the floor stringifies into an enum field is
therefore domain-checked on that string: `grade: 5` is `enum_violation`, not
`type_mismatch`.

Schema literals (`example:`, `default:`) are judged as written, uncoerced: the
blueprint would otherwise emit a spelling it then teaches authors to write.

Coercion rules per type:

| Type | Rule |
|---|---|
| `array` | array wrapping plus element-wise coercion against the `items` schema; a bad element fails at its indexed path, e.g. `counts[1]` |
| `boolean` | from string, int, or float |
| `number` / `integer` | from string, or from boolean (`true→1`, `false→0`). An `integer` is an `i64`; a literal past that range is refused, and only `number` carries it |
| `string` | unwraps a length-1 string array into the bare string; identity otherwise |
| `richtext` | commits the canonical content form (the model): an authored markdown string imports via `quillmark-content::import`, an editor-supplied content object revalidates and re-canonicalizes. The length-1-array-unwrap and bare-scalar-stringify leniencies feed the import |
| `date` / `datetime` | per-type strict-grammar validation, stored verbatim: a `date` rejects any time component, a `datetime` rejects offsets/space/fractional/bare-date. Neither truncates |
| `object` | property recursion |
- **`inline` richtext enforcement.** A `richtext` field with `inline: true`
  requires its content to be exactly one `Para` line, in no container, with no
  islands (`Content::is_inline`). The empty content satisfies it, so a blank or
  blank-filled inline field passes. The constraint is checked in three places:
  coercion (`CoercionError` for a document value), validation
  (`validation::not_inline`, the `TypeMismatch` fatality class, as a backstop for a
  content that bypassed coercion), and load-time example import (a schema literal
  that violates it is a load error). Blueprint still annotates inline fields as
  `richtext(inline)<markdown>`; `build_transform_schema` emits
  `quillmark:inline: true`
- **`plaintext` coercion and enforcement.** A `plaintext` value rides the same
  content as `richtext`, differing only at the codec:
  - a string imports through the **literal** codec (`from_plaintext`,
    verbatim: no markdown parse, no escaping);
  - an editor-supplied content object is validated **plain**
    (`Content::is_plain`: no marks, no islands, all `Para` lines) rather than
    markdown-decoded. A formatted wire content is rejected, not stripped.

  Enforcement mirrors the `inline` precedent, in the same three places:
  coercion (`CoercionError`); validation (`validation::not_plain`, the
  `TypeMismatch` fatality class); load-time literal import. An `inline: true`
  plaintext field additionally requires a single line. The load-time content
  cache (`default_content`), the load-time `example:` import, and the
  render-floor blank (the empty content) cover `plaintext` exactly as
  `richtext`: both are content leaves (`field_contains_content`)
- **`enum` domain validation.** An `enum` field coerces as a string; domain membership is a *value* check (`validation::enum_violation`), not a type check, so an out-of-domain string is well-typed but invalid. `type: enum` requires a non-empty `values:` list; `values:` on any other type is a load error (`quill::field_parse_error`), as is `enum:` on any type
- **The domain rides the type token.** It is the `FieldType::Enum` payload, so a consumer that has matched the token holds it: the render floor, the acroform widget kind, the blueprint annotation, and the transform-schema projection to `{type: string, enum: […]}`. A variant-bearing branch enters through `variants:` with no token in hand and reads it through `FieldSchema::domain()`. A domain admits its members and the blank, so an empty one admits only the blank
- **Null short-circuits coercion.** A null value (`field:`, `field: null`,
  `field: ~`) passes coercion unchanged for *every* type: null ≡ absent, so
  it carries no data to coerce. The value reaches the render floor and
  blank-fills (authored › `default:` › blank) exactly like an omitted
  field
- **Bare scalars stringify into `string`/`richtext` fields.** A bare boolean,
  integer, or number written where a `string` is expected adopts its canonical
  scalar token (`true`, `47`, `1.0`) instead of failing: it is unambiguously
  text (null and collections are excluded); a `richtext` field then imports that
  token as its markdown source. The leniency is scoped to
  *document* payloads via the shared `scalar_as_string` predicate; a quill
  author's own `default:`/`example:` literals stay strict, so the blueprint
  keeps quoting ambiguous string literals. The same strictness rejects a
  container-shaped literal on a variant-bearing enum
  (`quill::{default,example}_type_mismatch`): the container is a *document*
  spelling, and a schema literal names the discriminant alone; the cells inside
  a world carry their literals on their own declarations

## Native validation

Validation is implemented by a native walker over `QuillConfig` in `quill/validation.rs`.

- Entry point: `QuillConfig::validate_document(&Document)` (dispatches to `validate_typed_document`)
- Returns `Result<(), Vec<ValidationError>>`
- Collects all errors (does not short-circuit)
- Emits path-aware errors for top-level fields and card fields
- Judges a card's fields only when its `$kind` names a declared kind. A card
  of an undeclared kind is unclaimed input, and so are body
  prose under `body.enabled: false` (a whitespace-only body is empty) and a
  key the schema does not declare.
  `Quill::validate` warns on each, and neither gates render
  ([What blocks a render](#what-blocks-a-render))
- `body.enabled: false` also drops `$body` from `build_transform_schema`'s `properties` for that kind: absent, not present-and-empty. This cascades into the Typst helper's generated `_qm-meta` address tables, so `form-field(field:)` rejects a `$body` address on that kind at compile time (see `PLATE_DATA.md`)
- **Null ≡ absent.** A present-null value (`field:`, `field: null`,
  `field: ~`) carries no data: it is treated exactly like an omitted field.
  It validates clean (no `TypeMismatch`) and blank-fills at render
  (authored › `default:` › blank; see
  [Blank-filled render](#blank-filled-render)).
- **Null ≡ absent is a 1.0 commitment, not a stopgap.** The identification is
  chosen and final: `field: null` and an omitted field are one state,
  indistinguishable by design. The consequences are accepted, not worked
  around: "explicitly cleared" and "never touched" cannot be told apart, so
  there is no uniform "blank, not default" for a non-string type, and
  `removeField` (drop the key) stays the sole unset verb; a present-null
  carries no distinct "cleared" signal. The tri-state alternative (absent /
  null / value) is foreclosed: it doubles every field's state space for one
  rarely-authored distinction, breaks YAML round-trip sanity (a loaded-then-
  saved document must not sprout `field: null` lines), and buys nothing the
  ladder does not already give. The simpler model is the contract.
- **Absence semantics**: a missing (or present-null) field with a `default:`
  accepts the default; without a `default:` it blank-fills. Either way it
  coerces and validates clean, and draws no diagnostic: absence is never
  *malformed*, and no code names it. `Quill::validate` on an incomplete
  document is clean.

Field-level type errors render under a uniform shape:
field path, verbatim source token, schema declaration, and both exits
when applicable. See `ERROR.md` § "Validation message contract".

## Value sources and projections

Every field value comes from one of a small set of **sources**, ordered by
*commitment*: how strongly the value claims to be the real answer. This is the
**commitment ladder**:

The ladder is cut per **cell**, not per field: see
[Cells and namespaces](#cells-and-namespaces) for which shapes are which, and
what follows for literals and for absence.

| Rung | Source | Persisted into a `Document`? | Renders? |
|---|---|---|---|
| top | authored value | yes: it *is* the document content | yes |
| | `default:` | **never** by the engine: lives in the schema, interpolated only into the ephemeral render projection | yes: the fidelity value |
| floor | the field's `blank` (`blank`) | never ([Non-persist invariant](#blank-filled-render)) | last resort |

`example:` is no rung: nothing persists it and nothing renders it. It is
schema guidance, which the blueprint shows as a `# e.g.` line.

A `default` is never written back into a document: it lives in `Quill.yaml`,
the render path interpolates it into the plate-JSON projection only, and seeding
deliberately omits it (persisting it would be redundant and would freeze it
against a schema change). The lone way a default's *value* becomes document
content is indirect: `blueprint()` emits it as literal text in its reference
*string* (the concrete default value, shippable as-is), and if a consumer authors from it and saves
it, that value is now ordinary **authored** content: the consumer committed
it, not the engine.

No surface owns a precedence *policy*; each **projection cuts the same ladder**
at a different rung, and the per-rung producers are shared (`blank` for the
floor; field ordering is declaration order, carried by the schema's ordered
field maps rather than a sort key):

| Projection | Per-field precedence | Floor | Output |
|---|---|---|---|
| render (fidelity) | authored › `default:` › blank | blank | plate JSON: [Blank-filled render](#blank-filled-render) |
| `blueprint` document | `default:` › empty | empty cell (blank at render) | annotated string, [BLUEPRINT.md](BLUEPRINT.md) |
| seeding | absent | (deferred to render floor) | committed `Document`: [Document seeding](#document-seeding) |
| add-card (into a document) | `$seed` overlay › absent | (deferred to render floor) | a new composable `Card`: [Document seeding](#document-seeding) |
| editor (consumer-side) | authored › `default:` › blank, resolved per field and **tagged with its source rung** | blank | the engine's [`resolve()`](#the-resolved-value-view-resolve) resolved-value view: value and source rung per field |
| values (`reader.get()`) | authored only, as stored: an absent field stays absent, a scalar shorthand stays a shorthand | **none**: an absent field reads absent | the [values form](#the-values-form): the field's value with content leaves as their codec's text |

### Cells and namespaces

A **cell** holds a value and cuts the ladder for it. A **namespace** holds cells,
and its value is what they compose to. The split decides where a literal may be
declared and how absence travels:

| Shape | Kind | Why |
|---|---|---|
| every leaf | cell | it is the value |
| `array` | cell | `items` fixes the element type, never the **arity**: `default: []` and `default: [{…}]` say what no element declaration can |
| `enum` discriminant | cell | the member is a leaf choice |
| `object` with `properties` | namespace | the schema fixes the keys, so nothing in the value is absent from its cells |
| a variant's field set | namespace | same, once the discriminant selects the world |
| `matrix` | namespace | same: the roster fixes the keys, and every member's `held` fixes its own |

Two rules follow, and between them the plate is total at every depth:

- **A literal is declared where its cell is.** A `default:` / `example:` on a
  typed dictionary is a load error (`quill::{default,example}_on_namespace`)
  naming the properties that hold it, as a container-shaped literal on a
  variant-bearing enum already is
  (`quill::{default,example}_type_mismatch`). The container spelling is a
  *second* declaration of a fact the cells already carry, free to disagree with
  a property's own `default:`. Schema literals are
  strict where document payloads are lenient ([Type coercion](#type-coercion)),
  so this is the same strictness, not a new kind.
- **Absence is inherited, not terminal.** An absent namespace makes every cell
  below it absent, and each cell then cuts its own ladder — so a property's
  `default:` is reached whether or not the document authored the container above
  it, at any depth. Resolution is therefore a **descent**: the rung supplies a
  *seed*, and the same composition runs over it whichever rung it came from. A
  partial element inside an `array` `default:` is completed against `items`
  exactly as an authored element is, and writing `contact: {}` is a no-op rather
  than an edit that changes the render.

A namespace has no rung of its own, so what
[`resolve()`](#the-resolved-value-view-resolve) reports for one is derived: see
that section.

The consumer-side `Document`-payload × schema join is a **non-goal**:
[`resolve()`](#the-resolved-value-view-resolve) supersedes it. The
editor reads value and source rung from one engine call rather than re-cutting
the ladder in consumer code. Completeness and errors stay `Quill::validate`'s
(a consumer merges it with its own diagnostic producers regardless), and schema
guidance (`example:`, labels, groups) reads from `Quill::schema`.

One seam is deliberate, not uniform: `blank` is a property of the field
rather than a member of the type's domain — an `enum`'s blank is `""`, outside
`values:` (there is no empty enum member). It is detailed below.

### The resolved-value view (`resolve()`)

`reader.resolve()` (core `TypedReader::resolve`, over the `Quill::resolve`
producer) cuts the render ladder into
observable data: for every declared field, the value `compile_data` would emit
into the plate, tagged with its source rung (`authored` / `default` / `blank`):
byte-for-byte with the plate on every fixture. The shape is nested: a `main`
card and a `cards` list, each card's `fields` an ordered array of `{ name,
value, source }` rows in declaration order: order is structural, not object-key
order. The card body is a `body` sibling on the card, not a row in `fields`:
present iff the kind enables a body (`enabled: false` undeclares it, so `body` is
`null`), its source only ever `authored` (non-blank) or `blank` (blank).
Source is one **top-level** rung per field; a nested blank-fill inside an authored
dict or array is a projection detail of the value, not a per-subpath source. The
rung is therefore coarser the deeper a field nests, and a container authored at
all reads `authored` however much of it the document left to the floor.

The coarseness is recoverable, because **the ladder is per-address local**:
`resolve_value_sourced` descends a present container with the property's own
value against the property's own schema node, so a nested rung is a function of
those two and never of the container's. A surface holding both derives it. Two
positions are not derivable that way, and are what a per-subpath view would be
for: a **content**-typed `default:`, whose plate form is the `Content` imported
at load rather than the schema literal; and an **absent** container, which
resolves to its container-level `default:` whole, with no property-level fill
inside it. Neither has a caller.

Value and provenance only. The view carries no diagnostics: completeness and
errors stay `Quill::validate`'s, which a consumer merges with its own producers
(session warnings, render errors) regardless, so bucketing here would delete no
consumer code. Schema guidance (`example:`, labels, groups) reads from
`Quill::schema`. Python is out of scope until a Python consumer names a call
site (the Tier-1 cut, [BINDINGS.md](BINDINGS.md)).

### The values form

A document has three forms, one per question. **Stored** is the at-rest
value, verbatim and quill-free ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)).
**Values** is stored with every content leaf decoded to its codec's text, so a
consumer edits plain values. **Resolved**
([`resolve()`](#the-resolved-value-view-resolve)) is values blank-filled and
render-coerced, each cell tagged with its rung. `reader.get()` answers what the
document *carries*; `reader.resolve()` answers what the render projection
*would use*. A read never coerces a scalar: `qty: "3"` is `"3"` in `get` and
`3` only in `resolve()`, because canonicalizing is what a write does
([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Byte-stability").

`reader.get(name)` reads one main-card field in the form and
`reader.card(i).get(name)` one field of one card. Every content leaf is its
codec's text — `richtext` markdown, `plaintext` literal — at **every depth the
field's type tree reaches**: an `array<richtext>` is an array of markdown
strings, a mixed `object` projects its content property and passes its scalars
verbatim, a variant carries its discriminant verbatim and each cell through its
own codec. A present-null rides as `null` at every type, apart from
authored-empty.

The form is **sparse**: an absent field reads absent, never materialized from
its `default:` ([Non-persist invariant](#blank-filled-render)). A leaf that
decodes under neither encoding raises `edit::field_decode`; the verbatim
`payload().get` is the read that opens a document too dirty to project, and the
load that admitted it already warned.

`set` writes one cell of the form and `set_all` a batch, every field resolving
before any is applied, so a consumer submitting a whole form sees every typo in
one pass. Both **canonicalize what they write**: a cell keeps its authored
shorthand until that cell is written. A content cell written through either is
a cold import — anchors on it do not survive, and `revise_field` is the write
that keeps them.

**A projection, never a storage format** ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)).
What a read does not carry, and what a write does to it:

| Not carried | Written back |
|---|---|
| identity anchors, content-only marks | lost on the written cell, which is a cold import |
| nested YAML comments | cleared on the written cell, as every write path clears them |
| the author's exact markdown | export canonicalizes: mark nesting, escaping, trailing whitespace. The *document* round-trips, not the string a consumer sent |
| `default:` rungs, blanks, `example:` | never read (sparse) |

`$ext` is the consumer's own card key ([PROGRAMMATIC.md](PROGRAMMATIC.md)
§ "Addressing cards for re-render") and no field write reaches it: it has its
own verbs (`Card::ext` / `Card::store_ext`), so bookkeeping stamped there
survives every write through this lane.

## Blank-filled render

**A document need not be complete to render**: render success is not a
completeness signal. Shippability is the author's judgment; the engine's only
hard requirement is that the document be *well-formed*
([What blocks a render](#what-blocks-a-render)). An absent or present-null cell
renders and draws no diagnostic (see [Native validation](#native-validation)).

Rendering and the *completeness verdict* are orthogonal. The render path
(`QuillConfig::compile_data` and the ladder it cuts, `ladder_sourced`, both in
core's `quill::compose`; the engine calls it) uses **blank-filled render**:
every absent schema **cell** is resolved by precedence: an authored value, else
the `default:`, else the field's blank (`blank`, defined below): in the
plate-JSON projection that feeds the backend **only, never in the persisted
document**.

The fill is **total at every depth**, which is what lets a plate read a declared
address directly rather than through a guarded accessor
([PLATE_DATA.md](PLATE_DATA.md)). Totality does not depend on which rung supplied
a value, or on how much of a container the document authored: absence is
inherited and each cell cuts its own ladder
([Cells and namespaces](#cells-and-namespaces)), so a declared address is present
whether its container was written, left out, or seeded from an `array`
`default:`.

- **Non-persist invariant.** The blank-fill lives only in the ephemeral
  projection and must never be written back. A blank is
  indistinguishable from authored-empty, so persisting it would erase the
  absence signal (which keys on a field being unwritten) and blind a future
  schema migration to author intent.

**A field's blank is a property of the field, not a member of the type's value
domain.** It is both the render floor and the value a reader recognizes as
"nobody said anything":

| Type | Blank |
|---|---|
| `string`, `date`, `datetime` | `""` (a date's `""` lowers to Typst `none`) |
| `enum` | `""` — reserved, and never a member of `values:` |
| `richtext`, `plaintext` | the empty content |
| `array` | `[]` |
| `object` | every property at its own blank, recursively |
| `integer`, `number` | `0` |
| `boolean` | `false` |
| `enum` with `variants:` | `{value: ""}` — the container holding the blank |
| any optional cell (`t?`) | `null`, lowered to Typst `none` ([Optional cells](#optional-cells)) |

Nothing forces an enum's blank to sit inside `values:`, and putting it there
destroys it: the floor would return a real choice nobody made, and a cosmetic
`values:` reorder would change what an unanswered document renders. So
**`values:` is for choices; the blank is for the absence of one**, and a quill
declaring `""` in `values:` fails to load (`quill::enum_blank_member`). Where
the empty state is itself a decision the document should record, it is a member
— `undecided`, `waived`, `n_a` — not the blank.

The accepted domain is therefore `values ∪ blank` everywhere a value is checked
or projected, at element position inside an `array` as well as at the top level.
The two projections differ deliberately: `Quill::schema` is the *declaration*
view and emits `values:` verbatim (injecting the blank would emit a schema that
fails to load), while the transform schema is the *wire* contract and emits
`enum: ["", …values]`, so a standard JSON-Schema validator accepts what the
engine accepts. A consumer's picker keeps unset a real, re-selectable option,
never a vanishing placeholder. Unset is `removeField`, the one unset verb; a
stored `""` is an [answer](#optional-cells) and outranks a `default:`.
`ui.blank_title` labels the blank wherever a consumer draws it.

**At `integer`, `number` and `boolean` the blank reads as an answer**, and so
does any `object` or `array` over them, since their blank is the recursive one:
`0` and `false` are indistinguishable at the plate from an authored `0` and
`false`. A uniform wire `none` for those types would be type-*absent* rather than
type-*minimal*, and Typst arithmetic and comparison reject it, which would cost
every plate the totality the floor exists to buy. So the floor stays
type-minimal, and a cell whose plate must see "unanswered" declares it:
`type: integer?` renders `none` ([Optional cells](#optional-cells)).

`blank` is the shared producer behind the render floor: for authored, blank, and
seeded documents alike (see [BLUEPRINT.md](BLUEPRINT.md)).

**A plate must branch exhaustively over `values ∪ blank`.** The blank is valid
present input, so an `else` fallback re-opens exactly the fabrication the blank
closes: the cell renders a variant nobody chose, and the plate cannot tell the
two apart. This is a retrofit obligation on existing plates, not only guidance
for new ones. Where the enum declares `variants:` the obligation also earns
something: the branch is what makes the world's fields readable without a guard
(see [Enum variants](#enum-variants)).

### The render date (`today`)

`today` is a `date` value standing for the day of the render. It is valid
wherever a `date` value is: authored, as a `default:` or an `example:`, at any
depth. It is not a `datetime` value.

The engine reads no clock. The date is an input to the compile, supplied by
the host at every door that turns a document into plate data:
`compile_data` / `compile_checked`, `Quill::resolve` / `reader.resolve`, and
`Quillmark::open` / `Quillmark::render`. A session keeps the date it was opened
with for every `update`.

| Compile given | A `today` cell renders | A Typst plate's `datetime.today()` |
|---|---|---|
| a date | that date | that date |
| none | the date's blank (`none`) | fails the render |

- **It stores as written.** `reader.get()`, storage and the blueprint carry
  `today`; a document never holds the day it was rendered.
- **`resolve` reports it under its own rung.** The value is the rendered date,
  the source is the rung that wrote `today` (`authored` or `default`).
- **Floating is a per-document choice.** `default: today` floats every document
  that leaves the field unset; an authored `today` floats one document; a
  written date pins it.
- **The host owns the time zone.** The CLI, the WASM runtime and the Python
  binding supply the local date unless given one ([BINDINGS.md](BINDINGS.md)).

A field without a date renders blank because a field has a floor to render at;
`datetime.today()` has none, so it fails rather than print a date nobody chose.

## What blocks a render

**A render fails only where the engine would have to invent what the author
wrote.** Everything else renders, and input no declaration claims warns. Input
short of a complete, well-formed answer falls in one of three classes:

| Class | Input | Render | Signal |
|---|---|---|---|
| Incomplete | a declared cell left absent or present-null | blank-fills it | none |
| Malformed | markup the grammar cannot read, or a value that will not read as its declared cell's type | fails | `parse::*` errors; `validation::type_mismatch`, `enum_violation`, `format_violation`, `coercion_failed`, `not_inline`, `not_plain` |
| Unclaimed | input no declaration reads | renders; no declared cell reads it | a warning naming the input |

The `validation::*` severity is the class: an `Error` is malformed, a `Warning`
unclaimed ([ERROR.md](ERROR.md#warning-flow)). No render reads
`$seed`, so every check on it warns, a malformed overlay value included.

**Malformed is fatal because the plate is total.** A declared cell always holds
a value ([Blank-filled render](#blank-filled-render)), so a value the engine
cannot read leaves it only substitutes, and each asserts something false. The
blank says nobody answered. A `default:` says the author accepted it. A guessed
coercion picks a meaning. `meeting_type: Regular` against
`values: [regular, special]` fails for that reason.

Markup is the same case one level up. The grammar decides what a line is, never
its content. A `~~~` line with no blank line above is not an opener, so it reads
as a code block and warns (`parse::card_fence_missing_blank`). A `~~~` block
whose payload is not a mapping is a card that cannot be read, and fails, as does
a `$` key outside the closed set: an unknown `$` key may change what a document
means, so ignoring one is a guess. A block after the root whose payload names
no `$kind` fails for the same reason (`parse::missing_kind`): it is a card
missing its kind line or code fenced with tildes, and reading it as either is a
guess. An undeclared kind is different: the block says it is a card, and only
the schema does not know the name.

**Unclaimed renders because it displaces nothing.** Every declared cell still
resolves from its own ladder, so the page asserts nothing the author did not
write. What it loses is the author's intent, and a typo is the usual cause. So
the input warns: a render that ignored it otherwise looks identical to one that
read it.

| Unclaimed input | Code | On the plate |
|---|---|---|
| a card whose `$kind` the quill does not declare | `validation::unknown_card` | in `$cards`, fields verbatim, `$kind` as authored, no `$body` |
| body prose under `body.enabled: false` | `validation::body_disabled` | absent |
| a variant cell outside the selected world | `validation::out_of_variant` | absent ([Enum variants](#enum-variants)) |
| elements past an array's `max:` | `validation::cardinality` | verbatim; the plate's own rule leaves the surplus off the page ([Cardinality](#cardinality)) |
| a `$seed` overlay naming no declared kind or field | `validation::seed_unknown_kind`, `seed_unknown_field` | absent, as `$seed` always is |
| a key the schema does not declare at its position, at any depth of a claimed card | `validation::unknown_field` | verbatim; absent inside a variant container |

`validation::unknown_field` names the likeliest fix. A key one world of a
sibling variant declares is that world's cell written beside its discriminant
rather than under it, so the hint says to nest it (args `container`,
`variant`). Otherwise a declared name the document leaves unwritten, within a
third of the key's length in edits and ignoring case, is the likely typo
(`suggestion`). A key another world declares is `out_of_variant`'s, and a
matrix key naming no member is malformed (`enum_violation`), so neither draws
it.

**Unclaimed input stays in the document.** It stores and round-trips as
authored; only the render passes it by. A schema that later declares the key
or the kind reads it with no migration.

**Nothing declared reads it.** The schema is a floor, not an allowlist: a
payload key crosses to the plate verbatim and uncoerced, and a card no kind
claims keeps its place in `$cards`, so the array stays index-aligned with the
document. A plate reads such input only through a total accessor and falls
through on a kind it does not know ([PLATE_DATA.md](PLATE_DATA.md#data-shape)).
`$body` is schema-defined and a variant's wire is closed, so the prose and
cells in the table marked absent do not cross.

## Document seeding

**Seeding** builds a starter `Document` from the schema for editor consumers
("new document"): the main card and one card per composable kind, each carrying
an empty body and **no field**. Every field is left absent and is
interpolated at the compilation layer by
[blank-filled render](#blank-filled-render) (`default:`, else the field's
blank), exactly as for any authored document.

No `example:` is committed, a field's or `body.example`. An example documents
shape, not an answer: committed, it would render a value nobody chose and read
as authored content. A field's surfaces in the blueprint's `# e.g.` line
instead, and `body.example` in the blueprint's body region, where an editor may
also show it as the empty body's placeholder. Starter content someone chose
lives in a template document's body, or in a card kind's `$seed` `$body`.
Persisting a `default` would be redundant (the floor interpolates it anyway)
and would *freeze* it against a later schema change; persisting a blank is forbidden
([Non-persist invariant](#blank-filled-render)). So a fresh seed renders exactly
as the empty document does, plus its cards, and a split-screen
editor/preview stays consistent: absent fields resolve identically in both
panes.

**Seed-commits-rest.** A seeded content value — a `$seed` overlay's content
field, and its `$body` — commits its codec's resting form (a richtext field and
the body the canonical content, a plaintext field its literal string), so a
seeded document is at rest from birth: `conform` of a seed is a byte no-op, and
a seed → store → load → conform cycle cannot move a hash on a document nobody
edited. An overlay field commits through the same strict write the typed writer
uses, which is what makes the seeder and the bound door agree rather than
merely coincide.

Content literals are imported once at quill load into a `#[serde(skip)]`
companion cache on the schema (`FieldSchema::default_content`), a pure function
of the `Quill.yaml` bytes; the render floor reads that cache rather than
re-importing markdown per document. The render floor injects `default_content`
into the plate uncoerced. The authored markdown literal is retained untouched: it is the source
of truth the schema emits and the blueprint prints; the content is a derived
projection of it.

The load pass walks the **schema**, not the card's field map, so companions are
populated at every declaration whose type tree bears a content leaf
(`field_contains_content`) — the leaf wherever it sits, and the container above
it carrying a literal of its own.

The render floor reads the companion off whichever declaration it resolves, so
covering every position is what makes an absent companion mean "no literal"
rather than "not reached"; a gap blank-fills and drops the author's `default:`
silently. The cache is also the gate: a content-bearing tree with no companion
blank-fills rather than falling through to the raw literal, which would cross as
unimported markdown. Importing is also checking, so a nested `richtext(inline)`
violation is a load error there, in a `default:` or an `example:`. An
`example:` or `body.example` is imported for that check alone and cached
nowhere.

- **Composable cards** are seeded one instance per declared kind.
- **The main card** carries `$quill` and `$kind: main`, so a seed round-trips
  through Markdown like an authored document.
- **Provenance is untracked in the persisted document.** A seeded overlay
  value is committed as ordinary authored content, indistinguishable from
  hand-authored input; whether it came from seeding or later authoring is not
  recorded, and correctness and renderability do not depend on the distinction.
  The commitment *rung* is a separate axis, reported on read: the
  [`resolve()`](#the-resolved-value-view-resolve) projection tags each
  field `authored` / `default` / `blank`, and a seeded value reads as
  `authored`, being document content.

The blueprint is the annotated form to fill ([BLUEPRINT.md](BLUEPRINT.md)); the
seed is a committed `Document` to edit. Implemented by `Quill::seed_document`
(with `seed_main` / `seed_card`) in `quillmark-core`.

### Per-document seed overlays (`$seed`)

Seeding a *new card into an existing document*: `Quill::seed_card(kind,
overlay)`, adds one rung: a curated, per-document **overlay** read from the
main card's `$seed` map. Per field the precedence is **`$seed` overlay ›
absent**, committed in field declaration order, each overlay value taken whole;
the body is **overlay `$body` › empty**. `default` / the blank
stay deferred to the render floor exactly as everywhere else, so the "never
persist a `default`" invariant holds. The overlay is *sparse*: fields it omits
stay absent and track an evolving quill's `default:` rather than freezing a
snapshot. This is how a template author customizes the values new cards spawn
with; it lives in the document (a template *is* a document), so markdown writers
and MCP agents see the same source. See [CARDS.md](CARDS.md) "Per-kind Seed
Overlays" for the `$seed` mechanics. The document seeding above is the
`overlay = None` case (a fresh document carries no `$seed`).

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields (`group`, `compact`, `multiline`, `title`, `blank_title`, `layout`) and on cards (`title`, plus the `groups` registry that `group` references). Field display order is not a hint: it is the key order of the emitted `fields`/`properties` maps (declaration order)
- `body` blocks on cards (`enabled`, `example`)

The schema describes only the user-fillable fields. The quill reference
(`name@version`, available from quill metadata) and card-kind
discriminators (the `card_kinds` map keys themselves) are document-level
metadata, not schema fields, and do not appear in `fields`.

`QuillConfig::schema_yaml()` is a YAML wrapper over the same value. The schema is pinned by serde attributes on `FieldSchema`, `CardSchema`, `UiFieldSchema`, `UiCardSchema`, and `BodyCardSchema`: there is no parallel mirror struct.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md): `blueprint()` emits a document-shaped, pre-filled Markdown reference that's denser than schema for prompt-time use.

Top-level schema keys: `main`, optional `card_kinds` (map keyed by card name).
`main` and each entry in `card_kinds` share the same `CardSchema` shape:
`fields` (map keyed by field name), optional `description`, optional `ui`,
optional `body`. Each `FieldSchema` includes `type`, optional
`description`/`default`/`example`/`enum`/`values`/`members`/`variants`/`inline`/`properties`/`items`/`max`/`ui`.
The type-gated keys:

- `inline`: valid only on the prose types (`richtext`, `plaintext`).
- `values`: declares an `enum` field's domain, required there.
- `members`: declares a `matrix` field's roster, required there. The desugared
  per-member objects are derived and never emitted: `members` and `properties`
  (the columns) are the authored carriers, so the view round-trips.
- `max`: an `array`'s element cap ([Cardinality](#cardinality)), valid only there.
- `ui.layout`: the control a field asks an editor to draw where the shape admits
  more than one and the default reads wrong. `table` is valid only on an `array`
  whose `items` is an `object`; `quill::invalid_ui` names the field anywhere
  else. The key carries two halves:
  - A **contract**: every column is a leaf, refused at load as
    `quill::table_column_not_flat` naming the column (`appendices[].entries`).
    The boundary is containment, not height — prose is a column whatever its
    `inline`, since how tall a cell renders is the consumer's judgement and what
    it contains is not. `FieldType::is_leaf` is the one definition, exhaustive
    over the vocabulary; `acroform` binds an array's elements through the same
    predicate.
  - A **request**: draw it as a grid. The plate, `validate` and the blueprint are
    inert on it by design, and a consumer that cannot draw the control — a width
    that will not hold the columns, a cell renderer that will not take a block —
    falls back to its own choice for the type. Shape is not among its reasons:
    the contract already settled it.
- `variants`: per-member field sets on an `enum` field, valid only there and only
  where a world may open (see [Enum variants](#enum-variants)). `schema()` emits it as
  authored, keyed by member; the transform schema instead projects the container,
  flattening every world's fields under `properties` with no member scoping.
- `items`: the element schema, itself a `FieldSchema`; required on `array`
  fields and rejected elsewhere.
- `properties`: used by `object` fields, and by an array's `object`-typed
  `items`.

### `default` and `example`

`default` and `example` are both type- and shape-valid values, but they
encode opposite author intents:

- **`default`** is the value the *majority* of authors want. Because most
  authors want it, the field can be omitted entirely: at render time the
  default fills any field the document leaves out (an
  authored value always wins: `ladder_sourced` in core's
  `quill::compose`). The blueprint renders that concrete default value with a
  type-only annotation. Type-empty defaults (`default: ""`, `[]`, `false`, `0`)
  are the canonical way to mark a "skippable" cell; a *dictionary* is skippable
  by its properties each carrying one, since the container holds no literal
  ([Cells and namespaces](#cells-and-namespaces)).
- **`example`** matches the semantic and type *shape* of the desired
  value but is *not* the value most authors want. It documents shape, not
  the choice: it never takes a cell, is never committed, and never renders. The
  blueprint shows it as a `# e.g.` line above the field.

`default:` means only the value an unanswered cell renders; a `?` on the type
moves that floor to `none` ([Optional cells](#optional-cells)). The schema asks
nothing further of a cell: no declaration makes one required, and no diagnostic
names an unanswered one. Null ≡ absent holds on every surface. There is no
`required:` key; an unknown key is a load error (`quill::field_parse_error`).

See [BLUEPRINT.md](BLUEPRINT.md) for how `default` and `example` render into
cells.

Identity fields (`name`, `version`, `backend`, `author`, `description`) live on the parent metadata object (Wasm: `Quill.metadata` getter; Python: `Quill.metadata`). Both bindings also expose `backend_id`/`backendId` directly; Python additionally exposes `quill_ref`, a derived `name@version` string.

### Bindings surface

| Binding | Schema accessor |
|---|---|
| Rust | `QuillConfig::schema()` (JSON) / `schema_yaml()` (YAML) |
| Wasm | `Quill.schema` getter (JSON) |
| Python | `Quill.schema` getter (dict) |
| CLI | `quillmark schema <path>` |

### Where the discriminators come from

The schema response omits discriminator fields. Consumers that need to
construct a document derive the discriminators from elsewhere:

- The root block's `$quill` value is `<name>@<version>`, built from
  `quill.metadata.name` and `quill.metadata.version`.
- Each composable card's `$kind` is the key under which it is declared
  in `card_kinds` (e.g. a card listed under `card_kinds.indorsement` is
  written as `$kind: indorsement`).
