# card-yaml Blocks

Quillmark documents carry structured metadata in **card-yaml blocks** —
explicitly delimited blocks that isolate YAML data from the surrounding
Markdown prose. The first such block (the *root block*) names the format used
to render the document; later blocks are composable [cards](cards.md).

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

## Block Structure

A card-yaml block has four parts, in order:

1. **Opening fence** — exactly `~~~card-yaml` (three tildes plus the info
   string). No leading indentation.
2. **System sentinels** — the leading non-blank lines inside the block,
   prefixed with `#@`. Every block declares `#@kind: <block_type>`; the root
   block additionally declares `#@quill: <name>@<version>` and its kind is the
   reserved value `main` (`#@kind: main`).
3. **Data payload** — standard YAML key/value pairs directly below the
   sentinels.
4. **Closing fence** — exactly `~~~`.

The unstructured Markdown body begins immediately after the closing `~~~`
fence and runs to the next opening fence or the end of the document.

A blank line is required immediately above every `~~~card-yaml` opener,
*except* when the opener is the very first line of the document. A
`~~~card-yaml` line without a blank line above it is **not** an opener — it is
treated as an ordinary code block.

## The `#@quill:` and `#@kind: main` Sentinels

The root block declares two system sentinels: `#@quill: <name>@<version>`,
which names the format used to render the document, and `#@kind: main`, the
reserved kind of the root block. Canonically `#@quill:` comes first, then
`#@kind: main` on the next line. If `#@quill:` is missing, parsing fails;
`#@quill:` is permitted only in the root block. The root block must declare
`#@kind: main`, and `main` is **reserved** — no composable card block may use
it.

### Version Selectors

Pin a specific version with `@version` syntax on the sentinel:

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

The data payload below the system sentinel is standard YAML.

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

Comments are **not** supported on the `#@` sentinel line itself.

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
block's Markdown body; `CARDS` holds the array of card blocks; `QUILL` and
`CARD` hold the values declared by the `#@quill:` and `#@kind:` sentinels.

## Card Blocks

Every card-yaml block after the root block is a **card** — it carries a
`#@kind: <kind>` sentinel, where `<kind>` matches `[a-z_][a-z0-9_]*`. All card
blocks are collected into the `CARDS` array available to templates.

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

`toMarkdown` always emits the canonical block form — a `~~~card-yaml` opener,
the system sentinels, the YAML payload, and a `~~~` closer. The root block
emits `#@quill:` first, then `#@kind: main`; composable blocks emit a single
`#@kind:` sentinel. Fence markers, the sentinel form and order, key ordering,
and YAML quoting are normalised; `!fill` tags and payload comments survive the
round-trip.

The payload is coerced and validated against the schema declared in the
Quill's `Quill.yaml` (`main.fields`). See the
[Quill.yaml Reference](../format-designer/quill-yaml-reference.md) for field
types and constraints.
