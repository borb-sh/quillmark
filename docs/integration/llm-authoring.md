# LLM Authoring

A model writes a Quillmark document as JSON: the quill emits a JSON Schema, and the model replies with a document whose rich-text fields and bodies are Markdown strings. Quillmark Markdown, with its card fences and YAML, stays the surface for people.

## The authoring document

```json
{
  "fields": { "subject": "Request for Waiver: **Fitness** Hours", "memo_for": ["42 ABW/CC"] },
  "body": "Markdown body.\n\n- Subparagraph.",
  "cards": [{ "kind": "indorsement", "fields": { "from": "42 SFS/CC" }, "body": "" }]
}
```

- `fields` and `body` are the main card's; `cards` lists the composable cards in order, each `{kind, fields, body}`.
- A `richtext` field is Markdown, a `text` field its literal string. Every other value is what the field stores: a number, a list, an object, a date string.
- A field left out, or `null`, takes its `default:`, else its blank. `""` and `[]` are answers.
- An enum with `variants:` is an object, `{ "value": "CUI", "poc": "…" }`, as it is in YAML.
- `$quill`, `$ext` and `$seed` never appear: the import stamps the quill's own reference.

The document is a snapshot of the whole state. It has no patch grammar and no card ids; a collaboration layer that needs either keeps them itself and applies the model's output into its own structure.

## The schema

```python
schema = quill.authoring_schema         # Python
```

```javascript
const schema = quill.authoringSchema    // JavaScript
```

`quillmark schema --authoring <quill>` prints it from the command line. It is JSON Schema draft 2020-12:

- Every field carries its `title`, `description`, `default`, and its `example:` as `examples`; every body its `description` and `examples`.
- A `richtext` leaf is `{type: string, contentMediaType: text/markdown}` and a `text` leaf `{type: string}`.
- Every cell admits `null`.
- Every object refuses a key the quill does not declare (`additionalProperties: false`), so an invented or misspelled field fails the schema. `fromAuthoring` still imports one, and `validate` warns on it.
- Card kinds sit under `$defs`, and `cards` items select one by `kind`.
- An enum with `variants:` is a `oneOf` with one branch per world.

Caps (`max:`) ride as `quillmark:maxLength` and `quillmark:maxItems` annotations rather than JSON Schema's asserting keywords, since an over-long value warns rather than failing. The schema is not shaped to one provider's structured-output limits; enforce it with a provider only where the quill fits that provider's caps.

## Import and export

```python
doc = quill.from_authoring(reply)       # a Document, conformed like quill.parse()
diagnostics = quill.validate(doc)
current = quill.to_authoring(doc)       # the document as the model would write it
```

```javascript
const doc = quill.fromAuthoring(reply)
const diagnostics = quill.validate(doc)
const current = quill.toAuthoring(doc)
```

`fromAuthoring` refuses a value that is not the authoring document's shape, with every error at once: `authoring::invalid_shape` with its JSON Pointer in `args.pointer` (`/cards/1/kind`), or an `edit::*` code for a field name or kind no card can hold. A value of the wrong type does not fail the import. It lands with a `conform::*` warning on `doc.warnings`, and `validate` reports it at its document path (`cards.indorsement[0].date`), so a repair turn can send back only the failed fields.

`toAuthoring` exports any document, so an edit turn shows the model the current state. Exporting what you imported gives back the same document. Markdown export is canonical: mark nesting, escaping and trailing whitespace may differ from the string the model sent.

Full model: [AUTHORING.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/AUTHORING.md).
