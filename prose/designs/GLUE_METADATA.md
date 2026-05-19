# Plate Data Injection

> **Status**: Implemented  
> **Scope**: How parsed document data reaches plates/backends

## Overview

Quillmark does not use a template engine for plates. Data flows in two stages:

1. `Quill::compile_data()` coerces, validates, normalizes, and applies schema defaults to the main-card and card fields, producing a plain JSON object.
2. `Backend::open()` receives that JSON and performs any backend-specific field transformations (e.g., converting markdown strings to Typst markup) before compilation.

### Data Shape

The JSON object has two keys:

- `main` — the main card's object: normalized main-card fields plus `QUILL`, `CARD` (always `"main"`), and `BODY`
- `cards` — an array of card objects, each carrying its own `CARD` discriminator, normalized fields, and `BODY`

```json
{
  "main": { "QUILL": "<ref>", "CARD": "main", "<field>": "...", "BODY": "<main-body>" },
  "cards": [ { "CARD": "<kind>", "<field>": "...", "BODY": "<card-body>" } ]
}
```

- Defaults from the Quill schema are applied before serialization in stage 1
- Markdown-to-Typst conversion and date parsing happen in stage 2, inside the backend

## Typst Helper Package

The Typst backend injects a virtual package `@local/quillmark-helper:<version>` that exposes the JSON to plates and provides helpers.

```typst
#import "@local/quillmark-helper:0.1.0": data

#data.main.title     // plain main-card field access
#data.main.BODY      // BODY is automatically converted to content
#data.main.date      // date fields are auto-converted to datetime
#data.cards          // array of card objects
```

Helper contents (generated in `backends/typst/helper.rs` from `lib.typ.template`):
- `data`: parsed JSON dictionary of all fields; a `__meta__` key injected by the backend carries the list of markdown and date fields to process, then is consumed by the helper before `data` is exposed to plates — plates never see `__meta__`
- Markdown fields (`contentMediaType: text/markdown`) are auto-evaluated into Typst content; date fields (`format: date`) are converted to Typst `datetime`

## Guarantees

- Plates see no internal shadow keys; `__meta__` is injected by the backend and consumed by the helper package before `data` is exposed
- `Quill::compile_data` returns the pre-transformation JSON (coerced + normalized + defaults); markdown/date conversion occurs inside `Backend::open` and is not separately observable
