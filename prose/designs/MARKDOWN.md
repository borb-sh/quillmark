# Quillmark Markdown

**Status:** Draft specification
**Editor:** Quillmark Team
**Base:** [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)

Quillmark Markdown is a **strict superset of CommonMark** with one declared
deviation. It layers a structured-data system — the **card-yaml** format — on
top of ordinary markdown, and selects a small, stable set of GFM extensions.
This document is the authoritative syntax standard.

## 1. Superset Statement

Every valid CommonMark 0.31.2 document parses to the same block / inline
structure under this spec, *except* for the deviation declared in §6.2.
Additionally, this spec defines:

- **Structured data** — card-yaml cards (§3).
- **Extensions** — strikethrough, pipe tables, and `<u>` for underline
  (§6.1).

Markdown with no `~~~card-yaml` fences is ordinary CommonMark, parsed as
such.

## 2. The card-yaml Format

The card-yaml format isolates structured metadata from markdown prose. By
keeping the data payload inside an explicitly delimited fenced region,
separate from the unstructured body that follows it, the format keeps LLM
generation stable
and prevents state corruption — a generator editing prose cannot accidentally
disturb the structured fields, and vice versa.

A document is a sequence of **cards**. Each card is a `~~~card-yaml` fence
followed by its prose body:

```
Document = Card+
```

- **Main card** — the first card, identified purely by position. Its `#@`
  header declares the quill that renders the document (`#@quill`) and the
  reserved kind `#@kind: main`. "Main" is a positional role, not a separate
  type — the main card has the same anatomy as every other card.
- **Subsequent cards** — zero or more cards. Each declares a structured
  record discriminated by its `#@kind`.
- **Prose body** — the markdown content between one card's closing fence and
  the next card's opening fence (or EOF).

### 2.1 Worked Example

```
~~~card-yaml
#@quill: example@0.1.0
#@kind: main
from: "bob"
to: "alice"
~~~

This is the main card's body text.

~~~card-yaml
#@kind: endorsement
#@id: rev-1
from: "charlie"
role: "reviewer"
clearance: "alpha"
~~~

I have reviewed the contents and officially endorse this flight plan.
```

The first card is the main card (by position); its `#@quill` header binds
the document to the `example` quill at version `0.1.0`, and `#@kind: main`
names its role. The second card's `#@kind` is `endorsement`. The text after
each closing `~~~` fence is that card's prose body.

## 3. card-yaml Cards

### 3.1 Structural Rules

A card has four parts, in order:

1. **Opening fence** — exactly `~~~card-yaml` (see §3.2). The info string
   alone identifies the card; no further declaration is needed.
2. **System metadata header** — a leading run of `#@key: value` lines
   inside the fence (see §3.3). The main card must declare `#@quill` and
   `#@kind: main`; on every other card all `#@` entries are optional.
3. **Data payload** — standard YAML key/value pairs below the `#@` header
   (see §3.4).
4. **Closing fence** — exactly `~~~` (see §3.2).

The prose body begins immediately after the closing `~~~` fence and runs to
the next opening fence or EOF.

### 3.2 Delimiter and Info String

- **Delimiter.** Cards open and close exclusively with `~~~` — three tildes,
  no more, no fewer. The closing fence is exactly `~~~` and carries no info
  string.
- **Info string.** The opening fence must be exactly `~~~card-yaml`. No other
  info string opens a card.
- **Indentation.** The `~~~card-yaml` opener and its closing `~~~` are at
  column zero — no leading spaces.
- **Line endings.** `\n` and `\r\n` are equally accepted.
- **Blank-line rule.** A blank line is required immediately above every
  `~~~card-yaml` opener, *except* when the opener is the very first line of
  the document. A `~~~card-yaml` line without a blank line above it is **not**
  a card-yaml opener — it is treated as an ordinary CommonMark fenced code
  block. Requiring the blank line keeps prose-body round-tripping stable and
  prevents a card from being absorbed into a preceding paragraph.

### 3.3 System Metadata (`#@`)

A card may begin with a **system metadata header** — a leading run of
`#@key: value` lines (the main card always has one). The header is a
**closed set** of three keys — `#@quill`, `#@kind`, `#@id`; any other
`#@key` is a parse error. These lines are kept out of the YAML payload's
user field set (§3.4); the `#@` header is not part of the data model's
field map.

In the typed model, the header parses to a `CardMetadata` struct with three
optional fields — `quill`, `kind`, `id`.

- **`#@quill: <name>@<version>`** — binds the document to a quill (see §3.5
  for the version-selector forms). It must be declared by the **main card**
  (the first card) and is parsed into a typed quill reference as the card is
  read. `#@quill` is **exclusive to the main card** — any other card
  declaring `#@quill` is a parse error.
- **`#@kind: <value>`** — names a card's kind. The value is name-validated
  at parse time: it must match `[a-z_][a-z0-9_]*`, otherwise parsing fails.
  `main` is a **reserved kind**: the main card must declare exactly
  `#@kind: main`, and no other card may use it.
- **`#@id: <value>`** — an opaque, optional identifier. It is plain metadata:
  no validation, no uniqueness requirement; it is carried through the
  round-trip unchanged.

Rules:

- The main card must declare both `#@quill` and `#@kind: main`. On every
  other card the `#@` header is optional; a non-main card must **not**
  declare `#@quill` or `#@kind: main`.
- `#@` header lines may appear in any order. The emitter emits them in the
  canonical key order `quill`, `kind`, `id` (see §9).
- A duplicate `#@key` within a single card is a parse error.
- A malformed `#@` line (not of the form `#@key: value`) is a parse error.
- An unknown `#@key` (anything outside `{quill, kind, id}`) is a parse error.
- An invalid `#@quill` reference is a parse error.
- **Comments are not supported on a `#@` header line itself.** Unlike the
  payload (§3.4), a `#@` line carries no trailing `#` comment.

### 3.4 Data Payload

Standard YAML key/value pairs sit directly below the `#@` metadata header.

- **Field names.** Every field name matches `/^[a-z_][a-z0-9_]*$/`.
- **Reserved names.** `QUILL`, `CARD`, `BODY`, and `CARDS` are reserved and
  may not be used as field names.
- **Whitespace-only payload.** A card whose payload is only whitespace
  yields an empty field set.
- **YAML comments.** Both own-line comments (`# …` on their own line) and
  inline comments (`field: value  # note`) are supported in the payload and
  round-trip through `toMarkdown`. (A `#@` header line is the one
  exception — see §3.3.) Comments inside nested YAML values (arrays, maps)
  are also preserved: the pre-scan captures each nested comment with a
  structural path and the emitter re-injects it at the matching position.
- **The `!fill` tag.** `!fill` is the single supported YAML tag; it marks a
  top-level field as a placeholder awaiting user input and round-trips
  through emit. `!fill` may be applied to scalars (string, integer, float,
  bool, null) and sequences; it is rejected on mappings because Quillmark's
  schema has no top-level `type: object`. Any other custom tag (`!include`,
  `!env`, …) is dropped with a `parse::unsupported_yaml_tag` warning; the
  scalar value is kept but the tag does not round-trip.

### 3.5 Version Selectors

The `#@quill` value is `<name>@<version>`, where `<version>` is one
of:

| Form | Meaning |
|---|---|
| `name@2.1.0` | exact version |
| `name@2.1` | latest `2.1.x` |
| `name@2` | latest `2.x.x` |
| `name@latest` | latest overall (explicit) |
| `name` | latest overall (default — `@version` omitted) |

Quill names match `/^[a-z][a-z0-9_]*$/`. See [VERSIONING.md](./VERSIONING.md)
for resolution semantics.

## 4. Card Detection

A single detector runs over the line stream. A `~~~card-yaml` line opens a
card **iff** both of the following hold:

**D1 — Blank line above.** The `~~~card-yaml` line is line 1 of the document,
or the line immediately above it is blank.

**D2 — Closing fence.** A matching `~~~` line appears later in the document.

YAML content between recognised fence markers is opaque to detection — a
`~~~card-yaml` line inside an open card is part of that card's payload, not
a new opener (though the canonical payload never produces such a line).

A `~~~card-yaml` line that fails D1 is delegated to CommonMark as an ordinary
fenced code block. A `~~~card-yaml` opener with no matching `~~~` closer
before EOF is a hard parse error (§9).

### 4.1 Worked Example

```
~~~card-yaml
#@quill: resume@1.0.0
#@kind: main
title: CV
~~~

Main card body text.

***

A thematic break in prose stays a thematic break.

~~~card-yaml
#@kind: profile
name: "Alice"
~~~

Profile body.
```

The first `~~~card-yaml` is the main card (line 1, D1 satisfied). The second
opens a `profile` card (blank line above). The `***` is an ordinary
CommonMark thematic break — card-yaml does not reserve any thematic-break
syntax.

## 5. Data Model

Parsing yields a document with two parts — the `main` card and the `cards`
array. Both the main card and every other card serialize through the **same**
card-object shape; the only difference is cardinality (one `main`, an array
of `cards`):

```typescript
interface Document {
  main: MainCard;         // the main card
  cards: Card[];          // the remaining cards, in document order
}

interface MainCard {
  QUILL: string;          // quill name@version, from #@quill
  CARD: "main";           // the reserved kind discriminator
  BODY: string;           // the main card's prose body
  [field: string]: any;   // payload fields
}

interface Card {
  CARD: string;           // card kind, matches /^[a-z_][a-z0-9_]*$/
  BODY: string;           // card prose body
  [field: string]: any;   // payload fields
}
```

- `cards` is always present, possibly empty.
- Field names may collide freely across cards; each card is its own scope.
- Body text is preserved verbatim — whitespace, line endings, and inline
  CommonMark are untouched by the splitter.

## 6. Markdown Content

Body regions (the main card's body and every other card's body) are rendered
as CommonMark 0.31.2 with the extensions and deviations below.

### 6.1 Extensions

| Feature | Syntax | Notes |
|---|---|---|
| Strikethrough | `~~text~~` | GFM rules: word-bounded delimiter runs only. |
| Pipe tables | GFM pipe-table syntax with alignment rows | Supports `:---`, `:---:`, `---:` alignment. |
| Underline (HTML) | `<u>text</u>` | The one allowlisted HTML tag (see §6.2). The only syntax for underline; handles intraword and arbitrary-range cases. |

### 6.2 Declared Deviation from CommonMark

**Raw HTML is accepted syntactically but produces no output, except
`<u>…</u>` which renders as underline.** The parser recognises HTML per
CommonMark §4.6 / §6.11, discards every event, and re-emits only the
`<u>` wrapper. Rationale: Typst has no HTML renderer, and arbitrary
passthrough would create an injection vector for downstream
HTML-producing tooling; `<u>` is the one exception because no
CommonMark-native syntax covers underline.

No other syntax deviates from CommonMark. Delimiter-run semantics for `*`,
`_`, `**`, `__`, and `~~` follow CommonMark and GFM exactly — in particular,
`__text__` renders as strong emphasis, identical to `**text**`.

### 6.3 Out of Scope

The following are parsed where CommonMark or pulldown-cmark already
handles them, but produce no Quillmark-specific output and may be
implemented in a future revision:

- Images (`![alt](src)`) — reserved for the asset-resolver integration;
  required for v1 of this spec.
- Math (`$…$`, `$$…$$`), footnotes, task lists, definition lists — not
  supported; `$` is literal.
- HTML comments — accepted syntactically, not rendered (see §6.2).
- `<br>`, `<br/>`, `<br />` — follow the raw-HTML rule (non-rendering);
  authors use CommonMark-native hard breaks (trailing two spaces plus
  newline, or trailing `\\` plus newline).

Backends MAY drop semantic data (e.g., link titles, image alt text)
that has no equivalent in their render target. Such losses are backend
concerns and are documented per backend, not in this spec.

## 7. Input Normalization

Before CommonMark parsing, each body region is normalized:

1. **Line-ending canonicalization.** `\r\n` and bare `\r` sequences are
   converted to `\n`. YAML scalars receive this treatment from the YAML
   parser itself; the body region does not, so this step ensures both
   layers agree. Authors editing on Windows or pasting from sources that
   emit CR-bearing line terminators otherwise leave bare `\r` bytes in
   the body, which some backends render as visible garbage.
2. **Bidi control stripping.** Remove U+061C, U+200E, U+200F,
   U+202A–U+202E, U+2066–U+2069. These invisible characters can
   desynchronize delimiter runs when copy-pasted from bidi-aware sources.
3. **HTML comment fence repair.** If `-->` is followed by non-whitespace
   text on the same line, insert a newline after `-->` so the trailing
   text reaches the paragraph parser instead of being consumed by the
   CommonMark HTML-block rule (type 2).

Normalization is applied identically to the main card's body and every
other card's body. It is not applied to YAML payload values.

## 8. Limits

Conforming parsers MUST enforce these limits and MUST surface a parse
error when any is exceeded:

| Limit | Value |
|---|---|
| Document size | 10 MB |
| YAML payload size per card | 1 MB |
| YAML nesting depth | 100 |
| Markdown block nesting depth | 100 |
| Field count per card | 1000 |
| Card count per document | 1000 |

## 9. Emission Contract

`toMarkdown` always emits the **canonical form** of every card:

```
~~~card-yaml
<#@ header lines, in canonical order>
<payload>
~~~
```

That is: a `~~~card-yaml` opener, the `#@` system-metadata lines (in the
canonical key order `quill`, `kind`, `id`), the YAML payload, and a `~~~`
closer. The main card's `#@` header includes `#@quill` and `#@kind: main`;
other cards emit whatever `#@` entries they declared, or none. A document
round-trips to this canonical shape — fence markers, key ordering, and YAML
quoting are
normalised. `!fill` tags and payload comments (own-line and inline) survive
the round-trip; the `#@` header lines are emitted without a comment.

## 10. Errors

Parse errors include:

- A `~~~card-yaml` opener with no matching `~~~` closer before EOF.
- The main card missing its `#@quill` entry.
- The main card missing its `#@kind: main` entry, or declaring a different
  `#@kind`.
- A non-main card declaring `#@quill`.
- A non-main card declaring `#@kind: main` (the kind is reserved).
- A malformed `#@` header line (not of the form `#@key: value`).
- A duplicate `#@key` within a single card.
- An unknown `#@key` outside the closed set `{quill, kind, id}`.
- An invalid `#@quill` reference.
- A field name failing `/^[a-z_][a-z0-9_]*$/`.
- Use of a reserved name (`QUILL`, `CARD`, `BODY`, `CARDS`) as a field name.
- Invalid YAML inside any card payload.
- Any §8 limit exceeded.

## 11. References

- [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
- [GitHub Flavored Markdown](https://github.github.com/gfm/) (pipe tables,
  strikethrough)
- [`VERSIONING.md`](./VERSIONING.md) — quill version-selector resolution
- [`CARDS.md`](./CARDS.md) — downstream card-handling semantics
