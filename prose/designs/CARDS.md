# Composable Cards Architecture

> **Status**: Implemented
> **Related**: [SCHEMAS.md](SCHEMAS.md), [QUILL.md](QUILL.md)

## Overview

Cards are structured metadata records inline within document content. A document is a sequence of cards. The first card is the **main card** — a positional role, not a separate type: it is an ordinary card that bears the document's `#@quill` reference and declares `#@kind: main`. Every other card is discriminated by its `#@kind`.

## Data Model

```rust
pub struct CardSchema {
    pub name: String,
    pub description: Option<String>,
    pub fields: HashMap<String, FieldSchema>,
    pub ui: Option<UiCardSchema>,
    pub body: Option<BodyCardSchema>,
}
```

The static display label for a card kind lives on `UiCardSchema::title`, not on `CardSchema` directly — see `ui.title` below. Body behavior (whether body content is permitted and optional guide text) lives under `body` — see `body.enabled` and `body.description` below.

`QuillConfig` exposes the entry-point card as `main: CardSchema` and the additional named card-kinds as `card_kinds: Vec<CardSchema>`. Look up a named card-kind by name via `card_kind(name)` or get a name-keyed map via `card_kinds_map()`.

## Quill.yaml Configuration

```yaml
main:
  fields:
    # ... main-card fields ...

card_kinds:
  indorsement:
    description: Chain of routing endorsements for multi-level correspondence.
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
        description: Office symbol of the endorsing official.
      for:
        type: string
        description: Office symbol receiving the endorsed memo.
      signature_block:
        type: array
        required: true
        ui:
          group: Addressing
        description: Name, grade, and duty title.
```

`ui.title` is the display label for UI consumers (section headers, chips, picker entries, per-instance list titles). It may be a literal string or a template containing `{field_name}` tokens that consumers interpolate with live field values (e.g. `"{from} → {for}"`). It's decoupled from the snake_case map key (`indorsement`), which is the on-the-wire `CARD` discriminator — so authors can rename the label without breaking stored documents.

## Public Schema YAML Output

```yaml
card_kinds:
  indorsement:
    description: Chain of routing endorsements for multi-level correspondence.
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
      for:
        type: string
      signature_block:
        type: array
        required: true
        ui:
          group: Addressing
```

`QuillConfig::schema()` emits the schema (with `ui` and `body` hints retained) and `schema_yaml()` is the YAML wrapper. The output keeps the same `card_kinds.<name>.fields` shape as `Quill.yaml` and injects a required `CARD` discriminator field whose `const` value is the card name. The `card_kinds` key is omitted entirely when no named card-kinds are defined. See `SCHEMAS.md` for the full surface.

## Markdown Syntax

A card is a `~~~card-yaml` fence led by a `#@` system-metadata header. The
main card declares `#@quill: <ref>` and `#@kind: main`; every other card
declares `#@kind: <kind>`. The kind is the on-the-wire `CARD` discriminator;
the card's payload is its YAML data, and the markdown after the closing
`~~~` fence is the card's body.

````markdown
~~~card-yaml
#@kind: indorsement
from: ORG1/SYMBOL
for: ORG2/SYMBOL
signature_block:
  - "JOHN DOE, Lt Col, USAF"
  - "Commander"
~~~

Indorsement body content.
````

See [`MARKDOWN.md`](./MARKDOWN.md) §3 for the full syntax specification.

## Backend Consumption

- **All backends**: the document is delivered as `{ "main": <card>, "cards": [<card>, ...] }`. Each `<card>` is an object containing a `CARD` discriminator, the card's fields, and a `BODY` field with the card's body Markdown. The `main` card additionally carries the `QUILL` reference (`CARD` is `"main"`).
- **`Quill::compile_data()`** returns the fully coerced and validated JSON in this `{ main, cards }` shape.
