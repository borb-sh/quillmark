# The Authoring Document

> **Implementation**: `crates/core/src/quill/`

## TL;DR

The authoring document is the JSON a model reads and writes: the
[values form](SCHEMAS.md#the-values-form) lifted to the whole document, every
content leaf a Markdown or literal string. The quill describes it with a JSON
Schema, `Quill::to_authoring` exports a document into it, and
`Quill::from_authoring` imports one back. Quillmark Markdown stays the surface
for human authors.

## Shape

```json
{
  "fields": { "subject": "Request for Waiver: **Fitness** Hours", "memo_for": ["42 ABW/CC"] },
  "body": "Markdown body.\n\n- Subparagraph.",
  "cards": [{ "kind": "indorsement", "fields": { "from": "42 SFS/CC" }, "body": "" }]
}
```

- `fields` holds the main card's fields and `body` its body; `cards` holds the
  composable cards in order.
- Each card is an envelope `{kind, fields, body}`. `kind` is the wire's `$kind`.
  It sits beside `fields`, so a quill field named `kind` lands at
  `fields.kind` and nothing is reserved at the field level.
- A field's value is its values form: a `richtext` leaf is Markdown, a `text`
  leaf its literal string, and every other value is as stored, at every depth.
- A variant-bearing enum is its container `{value, …the world's cells}`, as it
  is in YAML; the bare discriminant is the shorthand coercion accepts.
- `$quill`, `$ext` and `$seed` never appear. The quill is the request's, and
  the import stamps it.

**Null ≡ absent.** A field left out, or `null`, takes its `default:`, else its
blank, as everywhere else ([SCHEMAS.md](SCHEMAS.md#default)). `""` and `[]` are
answers. The form is sparse: an export never materializes a default.

**A snapshot, never a patch.** The document is a whole-state shape. It carries
no edit grammar and no card identity: a live-collaboration layer (Yjs, say)
owns both, and applies a model's output into its own structure. Deleting a key
there is the null ≡ absent case.

## The schema

`QuillConfig::authoring_schema()` is
`build_transform_schema(config, SchemaCodec::Text)`. The plate's transform
schema is the same builder under `SchemaCodec::Content`, so both read one
per-field projection and differ only where the codec does:

| | `Content` (plate seam) | `Text` (authoring) |
|---|---|---|
| `richtext` leaf | `{type: object, contentMediaType: application/quillmark-content+json}` | `{type: string, contentMediaType: text/markdown}` |
| `text` leaf | the content object plus `quillmark:plain` | `{type: string}` |
| `null` | admitted on an optional cell | admitted on every cell |
| variant-bearing enum | one object, every world's cells flattened | `oneOf`: one branch per world, its `value` a `const`, one branch for the worldless members and the blank, the bare discriminant, `null` |
| annotations | none | `title`, `description`, `default`, `examples` from `example:` |
| envelope | `properties` with `$body`, `$defs.<kind>_card` | `{fields, body, cards}`, each kind a `$defs.<kind>` with `kind` a `const` |

A body is `{type: string, contentMediaType: text/markdown}` carrying the
kind's `body.description` and `body.example`, and is absent on a kind whose
`body.enabled` is `false`. A variant-bearing enum's `default:` and `example:`
name a member, so they annotate as `{value: <member>}`.

`quillmark:multiline`, `quillmark:maxLength`, `quillmark:maxItems` and
`quillmark:roster` cross under both codecs. A cap warns rather than refuses
([SCHEMAS.md](SCHEMAS.md#cardinality)), so it never becomes an asserting
`maxLength`. The schema is written for any provider and not to one provider's
structured-output limits: a consumer that enforces it with a provider does so
only where the quill fits that provider's caps.

## Export

`Quill::to_authoring(&doc)` reads each present field through the values-form
projection `reader.get()` uses, and each body as its Markdown. A field the
schema does not declare, and every field of a card whose kind it does not
declare, rides verbatim. `body` is present where the kind enables one or the
card carries one; `cards` where the document has any.

A leaf that decodes under neither encoding fails the export with
`edit::field_decode` at its document path, as `reader.get()` does.

## Import

`Quill::from_authoring(&value)` is the JSON twin of
[`Quill::parse`](BINDINGS.md): it builds the document the value describes
under this quill's `$quill` (`name@MAJOR.MINOR`), then conforms it, so the
result rests as a parsed document does.

- **Shape is refused.** A value that is not the envelope (a non-object, an
  unknown key, a card with no `kind`, a non-string body) fails with
  `authoring::invalid_shape` at the authoring path (`cards[1].kind`). A name
  or kind no card can store fails with its `edit::*` code at its document path.
  Every such error is collected before the import fails.
- **Values are not.** A field value lands as written and conform canonicalizes
  it. A value the schema refuses stays with a `conform::*` warning, and
  `Quill::validate` reports it at its document-model path
  (`cards.indorsement[0].date`), so a consumer can send back only the failed
  fields. An unknown card kind lands too, as it does from Markdown.
- `Parsed.warnings` carries the conform diagnostics and any
  `parse::dropped_construct` from a body or a `richtext` leaf.

**Round trip.** Export after import is a fixed point: importing an export
yields the same document. What the values form does not carry, the authoring
document does not either ([SCHEMAS.md](SCHEMAS.md#the-values-form)): an import
is a cold write, and Markdown export canonicalizes mark nesting, escaping and
trailing whitespace. A collaboration layer that holds a body as Markdown text
therefore diffs against the canonical export, not the model's string.

## The canonical documents

Two documents bind a plate ([QUILL.md](QUILL.md#the-plate-contract)): the
**empty document** (`Quill::empty_document`) and the **seeded document**
(`Quill::seed_document`). The **example document**
(`Quill::example_document`) fills every cell an `example:` answers. None is an
LLM surface of its own: a model reads the schema and, to edit, the export of
the document at hand.

## Bindings surface

| Binding | Schema | Export | Import |
|---|---|---|---|
| Rust | `QuillConfig::authoring_schema()` | `Quill::to_authoring(&doc)` | `Quill::from_authoring(&value)` |
| Wasm | `Quill.authoringSchema` | `Quill.toAuthoring(doc)` | `Quill.fromAuthoring(value)` |
| Python | `Quill.authoring_schema` | `Quill.to_authoring(doc)` | `Quill.from_authoring(value)` |
| CLI | `quillmark schema --authoring <QUILL_PATH>` | | |
