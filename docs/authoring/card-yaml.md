# card-yaml Cards

Quillmark documents carry structured metadata in **cards** —
explicitly delimited records that isolate YAML data from the surrounding
Markdown prose. A document is a sequence of cards. The first card (the *main
card*) names the format used to render the document; later cards are
composable [cards](cards.md).

```
~~~card-yaml
#@quill: my_format
#@kind: main
title: My Document
author: Jane Doe
date: 2025-01-15
tags: ["important", "draft"]
~~~

# Document content starts here
```

## Card Structure

A card has four parts, in order:

1. **Opening fence** — exactly `~~~card-yaml` (three tildes plus the info
   string). No leading indentation. The info string alone identifies a
   card.
2. **System metadata header** — an optional leading run of `#@key: value`
   lines inside the card. The main card must declare `#@quill` and
   `#@kind: main`; all other `#@` entries are optional.
3. **Data payload** — standard YAML key/value pairs directly below the
   `#@` header.
4. **Closing fence** — exactly `~~~`.

The unstructured Markdown body begins immediately after the closing `~~~`
fence and runs to the next opening fence or the end of the document.

A blank line is required immediately above every `~~~card-yaml` opener,
*except* when the opener is the very first line of the document. A
`~~~card-yaml` line without a blank line above it is **not** an opener — it is
treated as an ordinary fenced code block.

## System Metadata (`#@`)

A card may begin with a **system metadata header** — an optional leading run
of `#@key: value` lines. These lines carry no parser semantics; they are kept
out of the YAML payload's user field set.

- **`#@quill: <name>@<version>`** names the format used to render the
  document. The main card (the first card, identified by position) **must
  declare `#@quill`**. If the main card is missing `#@quill`, parsing fails.
- **`#@kind: <kind>`** identifies a card's kind, where `<kind>` must match
  `[a-z_][a-z0-9_]*` — an invalid kind is a parse error. `main` is a
  **reserved card kind**: the main card **must** declare `#@kind: main`, and
  no other card may declare it. It is a parse error for the main card to omit
  `#@kind: main`, or for any non-first card to declare it.
- **`#@id: <value>`** is an opaque, optional identifier — plain metadata with
  no validation or uniqueness requirement, carried through the round-trip.

The canonical `#@` header order is `quill`, `kind`, `id`. `#@` header lines
may appear in any order; the emitter preserves their source order. A duplicate
`#@key` within a single card, or a malformed `#@` line, is a parse error.

### Version Selectors

Pin a specific version with `@version` syntax on the `#@quill` line:

```
~~~card-yaml
#@quill: my_format@2.1
#@kind: main
title: Document Title
~~~
```

| Syntax | Meaning |
|--------|---------|
| `format` | Latest version (default) |
| `format@latest` | Latest version (explicit) |
| `format@2` | Latest 2.x.x |
| `format@2.1` | Latest 2.1.x |
| `format@2.1.0` | Exact version 2.1.0 |

Quill names must match `[a-z][a-z0-9_]*` (lowercase letters, digits, and
underscores; must start with a lowercase letter).

## Payload Data Types

The data payload below the `#@` metadata header is standard YAML.

**Strings:**
```yaml
title: Simple String
quoted: "String with special chars: $%^"
multiline: |
  This is a
  multiline string
```

**Numbers:**
```yaml
count: 42
price: 19.99
```

**Booleans:**
```yaml
published: true
draft: false
```

**Arrays:**
```yaml
tags: ["tech", "tutorial"]
# or
authors:
  - Alice
  - Bob
```

**Objects:**
```yaml
author:
  name: John Doe
  email: john@example.com
```

Object-valued fields must be schematized in `Quill.yaml` with `type: object`
and a `properties:` map. Nesting beyond one level is not supported. See
[Quill.yaml Reference: Field Types](../format-designer/quill-yaml-reference.md#field-types).

Field names must match `[a-z_][a-z0-9_]*`.

## Comments

YAML comments are supported in the payload and round-trip through
`toMarkdown` — both own-line comments and inline comments:

```yaml
# An own-line comment.
title: My Document  # an inline comment
```

Comments are **not** supported on a `#@` header line itself.

## Placeholder Fields (`!fill`)

A top-level field tagged `!fill` marks the value as a placeholder awaiting
input. The tag round-trips through parsing and emit, so editors and bindings
can detect and update placeholders without losing them.

```yaml
recipient: !fill
department: !fill Department Here
tags: !fill []
```

`!fill` is valid on scalars (string, number, bool, null) and sequences. It is
rejected on mappings. Other custom YAML tags (`!include`, `!env`, …) are
dropped with a warning.

## Reserved Field Names

`QUILL`, `CARD`, `BODY`, and `CARDS` are reserved and cannot be used as field
names — the parser rejects documents that include them. `BODY` holds the
card's Markdown body; `QUILL` and `CARD` hold the values declared by the
`#@quill` and `#@kind` metadata.

## Cards

Every card after the main card is a **card** carrying a `#@kind: <kind>`
metadata line, where `<kind>` matches `[a-z_][a-z0-9_]*` (and is never
`main`). All such cards are collected into the `cards` array available to
templates.

```
~~~card-yaml
#@kind: endorsement
from: ORG/SYMBOL
for: ORG2/SYMBOL
~~~

Endorsement body text here.
```

See [Cards](cards.md) for details on card syntax and usage.

## Emission

`toMarkdown` always emits the canonical card form — a `~~~card-yaml` opener,
the `#@` metadata lines in source order, the YAML payload, and a `~~~` closer.
The main card's header includes `#@quill` and `#@kind: main`; other cards emit
the `#@kind` entry they declared plus any optional `#@` entries. Fence
markers, key ordering, and YAML quoting are normalised; `!fill` tags and
payload comments survive the round-trip.

The payload is coerced and validated against the schema declared in the
Quill's `Quill.yaml` (`main.fields`). See the
[Quill.yaml Reference](../format-designer/quill-yaml-reference.md) for field
types and constraints.
