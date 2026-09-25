# card-yaml Blocks

Quillmark documents carry structured metadata in **card-yaml blocks**:
explicitly delimited blocks that isolate YAML data from the surrounding
Markdown prose. The first such block (the *root block*) names the format used
to render the document; later blocks are composable [cards](#card-blocks).

```
~~~
$quill: my_format
$kind: main
title: My Document
author: Jane Doe
date: 2025-01-15
tags: ["important", "draft"]
~~~

# Document content starts here
```

## Block Structure

A card-yaml block has three parts, in order:

1. **Opening fence**: a bare `~~~` (three tildes). No leading indentation. Any
   info string is accepted on input — `~~~card-yaml` and `~~~yaml` among them —
   and re-emits as a bare `~~~`.
2. **YAML payload**: a standard YAML mapping. The reserved keys `$quill`,
   `$kind`, `$ext`, and `$seed` carry system metadata (see below); every
   other key is a user-defined data field.
3. **Closing fence**: a tilde run at least as long as the opener. The canonical opener and closer are both `~~~`; a longer opener (e.g. `~~~~`) requires an equally long closer.

The unstructured Markdown body begins immediately after the closing `~~~`
fence and runs to the next opening fence or the end of the document.

A blank line is required immediately above every `~~~` opener,
*except* when the opener is the very first line of the document. A
`~~~` line without a blank line above it is **not** an opener: it is
treated as an ordinary code block.

Because every column-zero `~~~` block is a card-yaml block, writing a literal
fenced code block in prose requires the escape hatch: use a **backtick fence**.
Tildes offer no escape. Adding more does not help — a `~~~~` block is still a
card (its closer must just be at least as long) — and neither does a language
info string: `~~~rust` opens a card whose payload is your Rust. Code that reads
as a YAML string or list fails at that fence's line and names the backtick
fence to write instead.

## System Metadata (`$`)

The block's YAML payload may contain up to four reserved `$`-prefixed keys.
After parsing, these keys are extracted from the user field set and exposed
on the block's typed metadata.

- **`$quill: <name>@<version>`** names the format used to render the
  document. The root block (the first block, identified by position) **must
  declare `$quill`**: it is the only required `$` entry. If the root block
  is missing `$quill`, parsing fails.
- **`$kind: <kind>`** identifies a card's kind. The root block's kind is
  `main` by position; `$kind: main` may be omitted or declared explicitly:
  any other value is a parse error. A composable card names its kind, matching
  `[a-z_][a-z0-9_]*` other than `main`, from the quill's `card_kinds`. A block
  after the root with no `$kind` fails the parse (`parse::missing_kind`) at its
  fence. A card with a kind the quill does not declare still renders, but
  nothing checks its fields, and a plate shows only the kinds it knows.
  `quill.validate(doc)` warns on it (`validation::unknown_card`) and lists the
  declared kinds.
- **`$ext: <mapping>`** is an opaque YAML mapping reserved for out-of-band
  extension data: UI editor state, agent annotations, anything bespoke to a
  consumer that should not reach the rendered output. Round-trips through
  Markdown and the storage DTO; **never** appears in the plate JSON consumed
  by backends. The value must be a mapping (scalars and sequences are
  parse errors); an empty `$ext: {}` is preserved as a distinct, explicit
  declaration. Consumers namespace inside the map (`$ext.editor`, `$ext.agent`,
  …) to avoid collisions; `$ext.editor.title` is the canonical slot for a
  per-card display name (an editor-side rename).
- **`$seed: <mapping>`** is a **root-only** mapping of per-kind seed overlays,
  keyed by card-kind. Like `$ext` it round-trips through Markdown and storage
  but **never** appears in the plate JSON; the seeding layer interprets it (see
  [CARDS.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/CARDS.md)
  "Per-kind Seed Overlays"). A composable card carrying `$seed` is rejected.

`$` metadata entries may appear anywhere in the block's payload (the
canonical emission puts them first, in the order `$quill`, `$kind`,
`$ext`, `$seed`). Any other `$`-prefixed key is a parse error: the set
is closed. To carry a per-card key of your own, put it in `$ext` under a
namespace you own.

### Version Selectors

Pin a specific version with `@version` syntax on the `$quill` line:

```
~~~
$quill: my_format@2.1
$kind: main
title: Document Title
~~~
```

A bare name selects the latest version; `@latest`, `@2`, `@2.1`, and `@2.1.0`
pin progressively tighter. The [Quill Versioning](../quills/versioning.md#how-authors-select-versions)
page owns the full selector semantics.

Quill names are lowercase letters, digits, and underscores. `Quill.yaml`
requires a leading letter (`[a-z][a-z0-9_]*`); the `$quill` line also parses a
leading underscore, but no loadable quill carries one.

## Payload Data Types

The data payload (everything in the YAML mapping except the `$`-prefixed
metadata keys) is standard YAML.

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
and a `properties:` map; array-valued fields with `type: array` and an
`items:` element schema (e.g. `items: { type: string }`, or `items: { type:
object, properties: … }` for a list of objects). See
[Quill.yaml Reference: Field Types](../quills/quill-yaml-reference.md#field-types).

A `Quill.yaml` declares field names as `[a-z][a-z0-9_]*`, so every schema
field is lowercase. The document parser is wider: it accepts any
`[A-Za-z_][A-Za-z0-9_]*` key and preserves case, so an uppercase or
underscore-led key parses but is always undeclared. An undeclared key stays in
the document, and `quill.validate(doc)` warns on it (`validation::unknown_field`),
naming the declared field it most likely meant where one is close. Only `$`-prefixed keys are
reserved for system metadata.

## Comments

YAML comments are supported in the payload and round-trip through
`toMarkdown`, both own-line comments and inline comments:

```yaml
# An own-line comment.
title: My Document  # an inline comment
```

Comments adjacent to `$` metadata keys (own-line or inline) round-trip
identically to comments on data fields.

## YAML Tags

Custom YAML tags (`!include`, `!env`, `!fill`, …) are not supported: each is
dropped with a `parse::unsupported_yaml_tag` warning and its value kept.

`!must_fill` in block style on a data field drops together with the value under
it: the field or nested property reads as null, and the warning names its path.
Inside `$ext` or `$seed` it drops like any other tag, keeping the value.

```yaml
subject: !must_fill Example   # reads as `subject:` (unanswered)
addr:
  street: !must_fill Main     # reads as `street:`; `city` is kept
  city: Anytown
```

A field awaiting input is written empty (`subject:`) or left out.

## Card Blocks

Every block after the root is a *card*: a composable, repeatable record. A card
declares `$kind: <kind>` (matching `[a-z_][a-z0-9_]*`, never `main`) alongside its
data fields; the Markdown after its closing `~~~` fence is the card's body. A
card of a kind declaring `body.enabled: false` renders without its body, and
`quill.validate(doc)` warns (`validation::body_disabled`).

```
~~~
$quill: my_quill@1.0
$kind: main
title: Main Document
~~~

# Introduction

Some content here.

~~~
$kind: products
name: Widget
price: 19.99
~~~

Widget description.

~~~
$kind: products
name: Gadget
price: 29.99
~~~

Gadget description.
```

Each card is collected into the plate JSON's `$cards` array. Its body Markdown:
everything between that card's closing `~~~` fence and the next block's opener
(or document end): is carried as the card's `$body` value.

Card kinds and their field schemas are declared in `Quill.yaml` under
`card_kinds`; see the
[Quill.yaml Reference](../quills/quill-yaml-reference.md#card_kinds-section).

## Emission

`toMarkdown` always emits the canonical block form: a bare `~~~`
opener, the `$` metadata lines in the canonical order `$quill`, `$kind`,
`$ext`, `$seed`, the remaining data fields, and a `~~~` closer. The root
block emits `$quill` and `$kind: main` plus any `$ext` / `$seed` it
declared (`$seed` is root-only); composable cards emit `$kind: <kind>` plus
any `$ext` they declared. Fence markers,
key ordering, and YAML quoting are normalised; YAML comments (own-line and
inline trailing, including those adjacent to `$` lines) survive the
round-trip.

The payload is coerced and validated against the schema declared in the
Quill's `Quill.yaml` (`main.fields`). See the
[Quill.yaml Reference](../quills/quill-yaml-reference.md) for field
types and constraints.
