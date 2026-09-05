# Schema Model (`QuillConfig`)

> **Implementation**: `crates/core/src/quill/`

## TL;DR

`QuillConfig` is the only schema model in quillmark. Validation, coercion, defaults extraction, and public schema emission all read directly from it.

## Quill.yaml DSL

Schema authoring lives in `Quill.yaml` under:

- `main.fields`
- `card_kinds.<card_name>.fields`
- optional `ui` and `body` blocks on `main` and each card kind

Supported field types:

| Quill.yaml Type | Meaning |
|---|---|
| `string` | Open scalar UTF-8 text: a value the template computes with (URL, path, identifier, reference key), not prose it lays out |
| `enum` | Closed string domain of **choices**; requires a `values:` list. Also accepts its blank (`""`), which is never a declared member — declaring one is a load error (`quill::enum_blank_member`). Projects to JSON-Schema `{type: string, enum: ["", …]}`. `values:` on any other type, and `enum:` on any type at all, is a load error |
| `number` | Numeric value (integers and decimals) |
| `integer` | Integer-only numeric value |
| `boolean` | `true` / `false` |
| `array` | Ordered list; requires an `items:` element schema (e.g. `items: { type: string }` for `string[]`, `items: { type: object, properties: … }` for a typed table) |
| `object` | Structured map; requires `properties:` |
| `date` | A strict calendar date `YYYY-MM-DD`. Rejects any time component (a time-bearing string is a `datetime`, not a truncated date). The common case in a document engine, so it is the unmarked date type. Stored verbatim; lowers to a native Typst `datetime(year:, month:, day:)`, with `display(<addr>, ..)` for a click-to-edit rendering (see `PLATE_DATA.md`) |
| `datetime` | A strict offset-less wall-clock datetime `YYYY-MM-DDThh:mm[:ss]`, seconds optional (zero-filled). Rejects timezone offsets (`Z`, `±HH:MM`), the space separator, fractional seconds, and a bare date (which is a `date`). An offset is **rejected, never dropped**: the engine does no zone math, keeping wall-clock semantics end to end. Stored verbatim; lowers the same way over the six-component `datetime(year:, .., second:)` |
| `plaintext` | Navigable **unformatted** prose over the same canonical content (`Content`) as `richtext` (same media type, nav, and regions) but a **literal** codec (`from_plaintext`/`to_plaintext`): delimiters stay literal, no markup, verbatim round-trip. Declare `inline: true` for the single-line variant. Constrained mark-/island-free (`Content::is_plain`); a formatted wire content is rejected (`validation::not_plain`), not stripped. **Rests as the literal string** — in the *document*. A plate receives the content object, exactly as for `richtext` (no backend reads the `plaintext` annotation): there is deliberately no plate-side content→`str` projection, since no plate has needed one |
| `richtext` | Rich **formatted** prose over a canonical content (`Content`); markdown is a projection of it. Declare `inline: true` for the single-line variant (exactly one `Para` line, no container, no islands). The pre-richtext `markdown` spelling and the retired `type: richtext(inline)` token are schema load errors (`quill::field_parse_error`). **Rests as the canonical content object** |

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

This is the DSL's only cross-field shape, and it buys three things the flat map
could not say:

- **Existence.** A `cui_`-style name prefix is hand-written namespacing that only
  prose can scope. Nesting supplies the namespace structurally, and the names
  shorten to what they mean.
- **Obligation, conditionally.** `must_fill` inside a variant keeps its ordinary
  `default:`-presence derivation, so it reads *"required in this world"*: `poc`
  is obliged on a CUI memo and silent on every other one. This is the one
  cross-field constraint the engine checks rather than describes.
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
| `variants:` below card level, or inside another variant | `quill::variant_placement` |
| an empty `variants:` map, or an empty variant | `quill::variant_empty` |
| a variant field named `value` | `quill::variant_reserved_field_name` |
| a name two variants declare *differently* | `quill::variant_field_collision` |

A variant cell carries any type a card field may — prose, dates and containers included. Every surface reaches it through the same dispatcher a card field uses — coercion through `conform_value`, validation through `validate_value`, the render floor through `resolve_value`, lowering through the schema-node walk ([PLATE_DATA.md](PLATE_DATA.md)), the content read through `get_content_at` — so it behaves as a card-level field of that type does. What a cell does not carry is `ui.group` (it inherits the discriminant's) or `variants:` of its own.

**Why `variants:` alone stays card-level.** Every other container's shape is a
function of the schema; a variant's is a function of the schema *and* the
discriminant. The transform schema projects the union of the worlds — at schema
time there is no live world — and the wire carries whichever one the document
selects. Four things hold because that gap is exactly one level deep:

- `variant_field` resolves a name by a flat scan across the worlds, which is
  what lets `quill::variant_field_collision` guarantee one name is one cell.
- A form binds once at open, so a cell is *unconditionally addressable* while
  only *conditionally live*.
- A plate branches once over `values ∪ blank` and needs no guard inside that
  branch.
- `validation::out_of_variant` names one discriminant rather than a chain.

Nesting spends all four, so the restriction is a rule about `variants:` rather
than about depth.

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
object it finds through `export::to_markdown`, and it cannot sniff the codec
from the shape (a `richtext` content that happens to be plain is
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
is the [values form](#the-values-form-values--set_values), and `get` is
`values()` restricted to one field.
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
  caches (`default_content`/`example_content`) and the render-floor blank (the
  empty content) cover `plaintext` exactly as `richtext`: both are content
  leaves (`field_contains_content`)
- **`enum` domain validation.** An `enum` field coerces as a string; domain membership is a *value* check (`validation::enum_violation`), not a type check, so an out-of-domain string is well-typed but invalid. `type: enum` requires a non-empty `values:` list; `values:` on any other type is a load error (`quill::field_parse_error`), as is `enum:` on any type. The domain rides one carrier (`FieldSchema::enum_values`), and every consumer keys on that carrier rather than on the `Enum` token: the render floor, the pdfform widget kind, the blueprint annotation, and the transform-schema projection to `{type: string, enum: […]}`
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
- Validates each card's `$kind` matches a known card kind
- Enforces `body.enabled: false` on the main card and on each card kind: body content for a body-disabled card emits `ValidationError::BodyDisabled` (whitespace-only bodies are treated as empty)
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
- **Null ≡ absent holds on the value ladder; the obligation surface splits
  them.** The identification above is about *values*, and it stays unqualified:
  null and absent blank-fill identically. `must_fill` asks a different question
  — did a human make a call — and writing the field's blank is one while
  clearing the key is not, so `field: ""` discharges the warning and
  `field: null` does not. Two verbs therefore part company: `removeField` and
  writing the blank are one act on the value ladder and two here, and a UI
  rendering both as an empty box shows nothing of the difference. That is the
  price of letting a human answer "deliberately nothing" at all: keying the
  obligation on the resolved source rung instead would leave the deliberate
  blank unspellable and go blind to a must-fill leaf inside a touched container.
- **`validation::must_fill` → non-fatal warning, from two triggers.**
  `Quill::validate` emits it at **`Severity::Warning`** when either holds, with a
  `trigger` arg naming which:
  - `marker` — a `!must_fill` marker is present (root or nested, main card or
    composable card), whether or not it carries a value. The marker is
    document-sovereign: it fires without consulting the schema, and a human
    dropping it is a decision nothing re-derives.
  - `unauthored` — the schema obliges the cell and the document leaves it
    absent or present-null.

  Neither subsumes the other. A hand-written or programmatically built document
  carries no marker; a seeded `example` is present, in-domain, and structurally
  indistinguishable from authored content. Where both would fire on one path
  (a bare marker on an unauthored cell) one diagnostic is emitted and the marker
  wins: its hint is the actionable one. It **never gates render**: the cell
  blank-fills, or uses its suggested value. A strict consumer (e.g. an LLM
  authoring loop) treats any outstanding warning as "not done."
- **Absence semantics**: a missing (or present-null) field with a `default:`
  accepts the default; without a `default:` it blank-fills. Either way it
  coerces and validates clean — absence is never *malformed*, and there is no
  `field_absent` code. On the editor surface it is surfaced where the schema
  obliges it: an unauthored must-fill cell warns. So `Quill::validate` on an
  incomplete document is not clean, and the count is per *document* — a card
  kind obliges nothing until an instance of it exists.

Field-level type and presence errors render under a uniform shape:
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
| | `example:` | only by [seeding](#document-seeding) | yes: once committed by seeding |
| floor | the field's `blank` (`blank`) | never ([Non-persist invariant](#blank-filled-render)) | last resort |
| (signal) | `!must_fill` marker | yes: rides on the value as a YAML tag | yes: the marked value (suggested value or blank-fill); raises the non-fatal `validation::must_fill` warning |

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
| `blueprint` document | value: `default:` › `example:` › blank; marker: the derived `must_fill` | blank (under the marker) | annotated string, [BLUEPRINT.md](BLUEPRINT.md) |
| seeding | `example:` › absent, stamped `!must_fill` where the schema obliges | (deferred to render floor) | committed `Document`: [Document seeding](#document-seeding) |
| add-card (into a document) | `$seed` overlay › `example:` › absent | (deferred to render floor) | a new composable `Card`: [Document seeding](#document-seeding) |
| editor (consumer-side) | authored › `default:` › blank, resolved per field and **tagged with its source rung** | blank | the engine's [`resolve()`](#the-resolved-value-view-resolve) resolved-value view: value and source rung per field |
| values (`reader.values()`) | authored only, as stored: an absent field stays absent, a scalar shorthand stays a shorthand | **none**: the shape is sparse | the [values form](#the-values-form-values--set_values): `DocumentValues`, content leaves as their codec's text |

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

Two rules follow, and between them the plate is total at every depth:

- **A literal is declared where its cell is.** A `default:` / `example:` on a
  typed dictionary is a load error (`quill::{default,example}_on_namespace`)
  naming the properties that hold it, as a container-shaped literal on a
  variant-bearing enum already is
  (`quill::{default,example}_type_mismatch`). The container spelling is a
  *second* declaration of a fact the cells already carry, and the two axes read
  different ones: `default: {name: A}` renders `A` while `must_fill` derives per
  property and still warns that nobody authored `name`. Schema literals are
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

Two seams are deliberate, not uniform: on `blueprint` the floor still
blank-fills like every other projection (a must-fill cell with no `example`
carries bare null/empty under its marker), but the projection additionally
**stamps the `!must_fill` marker** on every must-fill field: the marker
rides *alongside* the value rather than replacing it; and `blank` is a property
of the field rather than a member of the type's domain — an `enum`'s blank is
`""`, outside `values:`
(there is no empty enum member). Both are detailed below.

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

### The values form (`values()` / `set_values()`)

A document has three forms, one per question. **Stored** is the at-rest
value, verbatim and quill-free ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)).
**Values** is stored with every content leaf decoded to its codec's text, so a
consumer edits plain values. **Resolved**
([`resolve()`](#the-resolved-value-view-resolve)) is values blank-filled and
render-coerced, each cell tagged with its rung. `reader.values()` answers what
the document *carries*; `reader.resolve()` answers what the render projection
*would use*. A read never coerces a scalar: `qty: "3"` is `"3"` in `get` and
in `values()` and `3` only in `resolve()`, because canonicalizing is what a
write does ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Byte-stability"),
which is what makes `reader.get(name)` equal `reader.values().fields[name]` on
every field that decodes.

The shape is `{fields, body, cards: [{kind, fields, body, ext}], ext}`, every
axis present on a read. Every content leaf is its codec's text — `richtext`
markdown, `plaintext` literal — at **every depth the field's type tree
reaches**: an `array<richtext>` is an array of markdown strings, a mixed
`object` projects its content property and passes its scalars verbatim, a
variant carries its discriminant verbatim and each cell through its own codec.
`fields` carries the declared fields the card holds in declaration order, then
undeclared ones verbatim in authored order: the schema is a floor, not an
allowlist, here as in `resolve`. A present-null rides as `null` at every type,
apart from authored-empty. `kind` is `null` for a kindless card; `ext` is `null`
for a card carrying no `$ext` and `{}` for an explicit `$ext: {}`.

It is **sparse**: an absent field is absent here too, never materialized from
its `default:`, so writing the shape back cannot erase an absence signal
([Non-persist invariant](#blank-filled-render)). It is **total**: it never
raises, a leaf that decodes under neither encoding riding out as stored where
the single-cell `get` raises — the load that admitted it already warned, and an
ingestion must open a document it can repair. Whole-scope reads are total;
single-cell reads raise.

`TypedWriter::set_values(values)` is the write twin, the typed lane widened
from one field (`set`) to the document; `writer.card(i).set_values` is the same
restricted to one slot. **An absent axis is untouched; a present one is
replaced**, at both scopes:

- `fields` is the whole truth for declared names: a named one is written, an
  unnamed one removed. An undeclared name the card holds at that value is
  accepted, since the read emits it; changed or new it is
  `edit::unknown_field`; unnamed it is left alone. A kind the schema does not
  declare is therefore readable and immutable through this lane.
- `cards` *is* the card list: a position whose kind matches (or whose `kind`
  is absent) is patched in place, a differing kind rebuilds the slot, an entry
  past the end appends, document cards past the list are removed. A position
  holding no card and naming no kind, or naming `null`, is refused at
  `cards[<i>]`: a kindless card is a parse artifact the mutators cannot build.
- `body` is replaced from markdown.
- `ext: null` removes `$ext`, `{}` records an explicit empty one, a map
  replaces.

So `DocumentValues::default()` is the empty patch, and `set_all` remains the
merge batch beside it. All-or-nothing, every refusal carrying the `DocPath` it
anchors at.

**A cell whose incoming value equals its projection is not written.** That
guard, comparing at the projection rather than at the stored bytes, is what
makes `set_values(reader.values())` a byte no-op on any document the bound door
admits — comparing at the storage level would call an anchor-bearing content
changed, its re-import lacking the anchor. So an untouched cell keeps what a
re-import cannot reproduce, and **nothing is normalized that the consumer did
not change**: a scalar shorthand (`qty: "3"`) stays as authored until that cell
is edited.

**A projection, never a storage format** ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)).
What it does not carry, and what a cycle does to it:

| Not carried | A cycle |
|---|---|
| identity anchors, content-only marks | kept on an untouched cell; lost on an edited one, which is a cold import |
| `!must_fill` markers, nested YAML comments | kept on an untouched cell; cleared on a written one, as every write path clears them. The marker's suggested value reads as a plain value: the marker is the obligation plane's (`validate`, `isFill`), not this one's |
| the author's exact markdown | export canonicalizes: mark nesting, escaping, trailing whitespace. The *document* round-trips, not the string a consumer sent |
| `default:` rungs, blanks, `example:` | never present (sparse) |
| `$quill`, `$seed` | absent from the shape and untouched by `set_values` |
| card identity | position + kind is the only match, so deleting, inserting or reordering an entry rewrites every card after it. Structural edits belong to the structural verbs |

`$ext` **is** carried, on the main card and each card: it is the consumer's own
card key ([PROGRAMMATIC.md](PROGRAMMATIC.md) § "Addressing cards for
re-render"), so a shape that dropped it would break the bookkeeping it exists to
serve. It is an open namespace this caller may not be the only writer of, which
is why only a present `ext` touches it.

Unlike `resolve`, both verbs reach Python: this shape is the API contract a
consumer reads, edits, and hands back, and it is the element type of a bulk
generation spec's `documents` list.

## Blank-filled render

**A document need not be complete to render**: render success is not a
completeness signal. Shippability is the author's judgment; the engine's only
hard requirement is that the document be *well-formed* (values coerce). A
`!must_fill` marker and a present-null cell are both renderable, and neither
surfaces as a diagnostic beyond the non-fatal `validation::must_fill` warning
(see [Native validation](#native-validation)).

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

- **Incomplete is renderable.** A document that merely omits a field (or
  leaves it present-null) renders fine: the field is blank-filled in the
  projection, and coercion/validation pass. A must-fill field it leaves
  unauthored warns on the editor surface and still renders.
- **Malformed is fatal.** The only malformed case is a value that cannot
  coerce to (or validate against) its declared type. Placeholders and null
  are *not* malformed: a `!must_fill` marker renders, using its suggested
  value or blank-filling, and a present-null cell blank-fills like an absent
  field.
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
engine accepts. A consumer's picker offers the blank as a real, re-selectable
option labelled by `ui.blank_title` — never a vanishing placeholder, because
returning to it is how a human clears a cell back to unset.

**The table has a permanent seam at `integer`, `number` and `boolean`**, and any
`object` or `array` over them, since their blank is the recursive one: `0` and
`false` are indistinguishable at the plate from an authored `0` and `false`. A
wire `none` for those types would be type-*absent* rather than type-*minimal*,
and Typst arithmetic and comparison reject it, which would cost the totality the
floor exists to buy. An author needing to spell "unset" for a number models it
as an `enum`, which has a real blank — at the cost of forfeiting arithmetic.

`blank` is the shared producer behind the render floor: for authored, blank, and
seeded documents alike (see [BLUEPRINT.md](BLUEPRINT.md)).

**A plate must branch exhaustively over `values ∪ blank`.** The blank is valid
present input, so an `else` fallback re-opens exactly the fabrication the blank
closes: the cell renders a variant nobody chose, and the plate cannot tell the
two apart. This is a retrofit obligation on existing plates, not only guidance
for new ones. Where the enum declares `variants:` the obligation also earns
something: the branch is what makes the world's fields readable without a guard
(see [Enum variants](#enum-variants)).

## Document seeding

**Seeding** builds a starter `Document` from the schema for editor consumers
("new document"): each **cell** that declares an `example:` is committed, and
**every other cell is left absent**. The seeding cascade is therefore
`example: → absent`: absent cells are never written; they are interpolated at
the compilation layer by [blank-filled render](#blank-filled-render) (`default:`,
else the field's blank), exactly as for any authored document.

Seeding descends a namespace for the same reason the render floor does: a typed
dictionary carries no `example:` of its own, so its seed is composed from
whatever its properties commit, and it stays absent when none of them commit
anything. The commit is **sparse** at every depth — only the cells with an
`example:` appear, and the rest defer to the render floor — so a nested
`example:` is reachable at all. It otherwise would not be: the render floor never
emits an `example`, and the blueprint is a different document.

**Seed-commits-rest.** A seeded content field commits its codec's resting form
(a richtext field and the body the canonical content, a plaintext field its
literal string), so a seeded document is at rest from birth: `conform` of a
seed is a byte no-op, and a seed → store → load → conform cycle cannot move a
hash on a document nobody edited. The commit runs through the same strict write
the typed writer uses, overlay values included, which is what makes the seeder
and the bound door agree rather than merely coincide. The content is imported
once at quill load into a
`#[serde(skip)]` companion cache on the schema (`FieldSchema::default_content` /
`example_content`, `BodyCardSchema::example_content`), a pure function of the
`Quill.yaml` bytes; seeding and the render floor read that cache rather than
re-importing markdown per document. The cache is the *content* either way, since
the render floor injects `default_content` into the plate uncoerced; only the
seed's commit takes the extra step to the field's rest. The authored markdown literal is retained
untouched: it is the source of truth the schema emits and the blueprint prints;
the content is a derived projection of it.

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
violation is a load error there, in a `default:` or an `example:`.

Committing *only* `example` is the whole design. The render ladder already
produces `default` and the blank at compile time but **never `example`** (example
is excluded from the render path; see [BLUEPRINT.md](BLUEPRINT.md)), so
`example` is the one source the render floor cannot reproduce. Persisting a
`default` would be redundant (the floor interpolates it anyway) and would
*freeze* it against a later schema change; persisting a blank is outright
forbidden ([Non-persist invariant](#blank-filled-render)). So the seed writes
exactly the one source that wouldn't otherwise appear and leaves the rest to
the floor. This keeps a split-screen editor/preview consistent: the document
carries real content, the preview renders it, and absent fields resolve
identically in both panes.

The seed is **illustration-first**: a field carrying *both* an `example` and a
`default` commits (and therefore renders) its **`example`**, not its default.
So a seeded document is *not* the plain fidelity render. The fidelity render
path's "`default:` wins" rule applies to authored and blank documents, where no
`example` is ever present; in a seed the `example` is present, so it wins.

- **Composable cards** are seeded one instance per declared kind; `body.example`
  fills the body when bodies are enabled.
- **The main card** carries `$quill` and `$kind: main`, so a seed round-trips
  through Markdown like an authored document.
- **A seeded `example` on a must-fill field commits carrying its marker.** An
  `example` documents *shape*, so a seeded one is not an answer. Stamping it is
  what makes the blueprint and its filled-out twin stamp the same cells: a fresh
  seed reports incomplete in exactly the cells a hand-written document does. A `$seed` overlay value is exempt — supplying one is a template
  author deciding, which is the act the marker asks for.
- **Provenance is otherwise untracked in the persisted document.** A seeded
  value is committed as ordinary authored content, indistinguishable from
  hand-authored input; whether it came from seeding or later authoring is not
  recorded, and correctness and renderability do not depend on the distinction.
  The marker is not provenance — a human may drop it without changing the value,
  and nothing re-derives it (see [Native validation](#native-validation)). The
  commitment *rung* is a separate axis, reported on read: the
  [`resolve()`](#the-resolved-value-view-resolve) projection tags each
  field `authored` / `default` / `blank`: a seeded and a hand-authored value both
  read as `authored`, both being document content.

Seeding is the **filled-out twin of the blueprint**
([BLUEPRINT.md](BLUEPRINT.md) § "The blueprint and its filled-out twin"): the
blueprint shows the form to fill (`!must_fill` markers, `# e.g.` hints), while the seed
hands back a committed `Document` already carrying the `example:` values, the
rest deferred to the render floor for fidelity. It is the only "filled-out"
projection: there is no annotated `example` string. Implemented by
`Quill::seed_document` (with `seed_main` / `seed_card`) in `quillmark-core`.

### Per-document seed overlays (`$seed`)

Seeding a *new card into an existing document*: `Quill::seed_card(kind,
overlay)`, adds one more rung above `example:`: a curated, per-document
**overlay** read from the main card's `$seed` map. Per field the precedence is
**`$seed` overlay › `example:` › absent** (ordered by field declaration order), and `default`
/ the blank stay deferred to the render floor exactly as everywhere else, so the
"never persist a `default`" invariant holds. The overlay is *sparse*: fields it
omits keep flowing from the schema seed, so it tracks an evolving quill rather
than freezing a snapshot. This is how a template author customizes the values
new cards spawn with; it lives in the document (a template *is* a document), so
markdown writers and MCP agents see the same source. See
[CARDS.md](CARDS.md) "Per-kind Seed Overlays" for the `$seed` mechanics. The
`example: → absent` document-seeding above is the `overlay = None` case (a fresh
document carries no `$seed`).

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields (`group`, `compact`, `multiline`, `title`) and on cards (`title`, plus the `groups` registry that `group` references). Field display order is not a hint: it is the key order of the emitted `fields`/`properties` maps (declaration order)
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
`description`/`default`/`example`/`enum`/`values`/`variants`/`inline`/`properties`/`items`/`ui`.
The type-gated keys:

- `inline`: valid only on the prose types (`richtext`, `plaintext`).
- `values`: declares an `enum` field's domain, required there.
- `variants`: per-member field sets on an `enum` field, valid only there and only
  at card level (see [Enum variants](#enum-variants)). `schema()` emits it as
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
  the choice, so it never becomes the rendered value; it takes the cell in the
  blueprint only when no `default:` holds it, and surfaces as a `# e.g.` line
  otherwise.

### Value and obligation: one declaration

A field asks one question, and `default:` is the whole answer: *does this field
have a value when nobody types anything?*

- The **value** axis is `default:` › `example:` › the field's blank. It decides
  what a cell holds.
- The **obligation** axis is `default:`'s **absence**. It decides whether a human
  must author that cell.

Declare a `default:` and the document renders with it, unasked; the type's blank
(`""`, `[]`, `0`, `false`) is how a field declares the answer "nothing". Leave it
off and the field is the engine's to ask about. The derivation reads
`default`'s *presence*, so a
`default: ""` stays a skippable cell rather than becoming a marker. The
derivation lives on the field schema, so it applies at every nesting level.

`example:` is commentary either way: it shows what goes there, seeds new
documents, and never renders. On a defaultless field it takes the blueprint cell
*under* the marker, and seeds carrying it. That is how a quill suggests a value a
human must still confirm — a classification marking, an effective date — and
until somebody accepts the suggestion the document renders the blank rather than
asserting a value nobody chose.

There is no `must_fill:` key and no `required:` one. Declaring `must_fill:` is a
load error (`quill::field_parse_error`), and the message names the `default:` or
`example:` that field carries in its place.

Obligation is a **warning, never a gate**: an unauthored must-fill field
blank-fills and renders, and the signal is the non-fatal
`validation::must_fill` (see [Native validation](#native-validation)). There is
no severity knob on this one — severity already *is* the render-gate signal, so
an `Error` that renders fine would break every consumer routing Error ≡
won't-render. An editor's "can't submit" is consumer policy over the warning: *a
strict consumer treats any outstanding marker as not done*. A quill author
arriving from web forms has the right prior for the affordance and the wrong one
for the enforcement.

Obligation belongs to a **cell**. A namespace holds none of its own: `!must_fill`
is rejected on a mapping, so the obligation lives on the leaves, and the
blueprint and the predicate both address them there. An array is its own cell,
including an array of objects.

The transform schema carries no obligation at all. That projection is the wire
*validity* contract, and an unauthored must-fill cell is wire-valid by design —
the blank leads every emitted `enum` domain precisely so a generated validator
accepts it. The declaration view carries `default:`, so a consumer wanting the
obligation reads it there.

See [BLUEPRINT.md](BLUEPRINT.md) for how value and obligation render into cells.

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
