# Quillmark Markdown Specification

> **Status**: Authoritative specification
> **Base**: [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
> **Implementation**: `crates/core/src/document/`

Quillmark Markdown is a **strict superset of CommonMark** with three declared
deviations (§6.2). It layers a structured-data system (the **card-yaml**
format) on top of ordinary markdown, and selects a small, stable set of GFM
extensions.
This document is the authoritative syntax standard.

## 1. Superset Statement

Every valid CommonMark 0.31.2 document parses to the same block / inline
structure under this spec, *except* for the three deviations declared in §6.2:
raw HTML, a link reference definition labelled `^…`, and a column-zero `~~~`
block with a blank line above it, which is a card-yaml block rather than a
fenced code block whatever its info string (§3.2; an indented `~~~` is not a
card-yaml opener). Additionally, this spec defines:

- **Structured data**: card-yaml blocks (§3).
- **Extensions**: strikethrough, pipe tables, and `<u>` for underline
  (§6.1).

A document containing no card-yaml blocks is ordinary CommonMark, parsed as
such.

## 2. The card-yaml Format

The card-yaml format isolates structured data from markdown prose.

A document is a sequence of **blocks**. Each block is one card-yaml block
followed by its prose body:

```
Document = (CardYamlBlock ProseBody)+
```

- **Root block**: the first block, identified purely by position. Its
  `$quill` metadata declares the quill that renders the document.
- **Subsequent blocks**: zero or more *cards*. Each declares a composable
  structured record.
- **Prose body**: the markdown content between one block's closing fence and
  the next block's opening fence (or EOF).

### 2.1 Worked Example

```
~~~
$quill: example@0.1.0
$kind: main
from: "bob"
to: "alice"
~~~

This is the primary document container body text.

~~~
$kind: endorsement
from: "charlie"
role: "reviewer"
clearance: "alpha"
~~~

I have reviewed the contents and officially endorse this flight plan.
```

The first block is the root block (by position); its `$quill` entry binds
the document to the `example` quill at version `0.1.0`. The second block is a
card whose `$kind` is `endorsement`. The text after each closing `~~~` fence
is that block's prose body.

## 3. card-yaml Blocks

### 3.1 Structural Rules

A card-yaml block has three parts, in order:

1. **Opening fence**: `~~~` (three tildes; see §3.2). Any info string is
   accepted on input and dropped on emit.
2. **YAML payload**: a standard YAML mapping containing both system
   metadata (`$`-prefixed reserved keys; see §3.3) and the block's data
   fields (see §3.4).
3. **Closing fence**: exactly `~~~` (see §3.2).

The prose body begins immediately after the closing `~~~` fence and runs to
the next opening fence or EOF.

### 3.2 Delimiter and Info String

- **Delimiter.** Blocks open and close with a run of tildes. The canonical
  fence is exactly three tildes (`~~~`), and `toMarkdown` (§9) always emits
  three. An opener of four or more tildes is accepted (non-canonical) and
  re-emits as `~~~`; its closing fence must be at least as long as the opener,
  per CommonMark's fenced-code-block rule.
- **Info string.** The info string is **not read**. A card-yaml opener is any
  `~~~` satisfying the rules below, whether it is bare, `~~~card-yaml`,
  `~~~yaml`, or `~~~rust`. The canonical form carries no info string, and
  `toMarkdown` (§9) always emits that bare form, so an info string is dropped
  on round-trip.
- **Escape hatch.** Because every column-zero `~~~` block is a card-yaml block,
  write a literal fenced *code* block in prose with a **backtick fence**
  (```` ``` ````), including a YAML one (```` ```yaml ````). Tildes offer no
  escape: neither a longer run nor a language info string opens a code block.
  A tilde-fenced code block reaches the YAML parser as a payload, so it fails
  under §10 unless it happens to be a well-formed card: after the root, one
  whose text reads as a mapping fails for naming no `$kind` (§3.3).
- **Indentation.** Both fences are at column zero: **no leading spaces**.
  An indented opener (1–3 spaces) is *not* a card-yaml opener: it is
  delegated to CommonMark as an ordinary fenced code block, exactly like an
  opener that fails the blank-line rule below. The closing `~~~` must also
  be at column zero: the payload between the fences is YAML, where
  indentation is structural, so an indented `~~~` is payload (e.g. a line
  of a block scalar), never a closer. (This deliberately tightens
  CommonMark's closing-fence rule, which tolerates 1–3 leading spaces:
  that leniency exists for indented openers and list contexts, neither of
  which applies to card-yaml blocks, and honouring it would let a tilde
  fence inside a `|` block-scalar value silently truncate the block.)
- **Line endings.** `\n` and `\r\n` are equally accepted.
- **Blank-line rule.** A blank line is required immediately above every
  `~~~` opener, *except* when the opener is the very first line of the
  document. A `~~~` line without a blank line above it is **not** a card-yaml
  opener: it is treated as an ordinary CommonMark fenced code block.

### 3.2.1 `---` Is CommonMark's

A card-yaml block is fenced with `~~~`, at every position. A `---` line is
CommonMark's throughout — a thematic break or a setext-heading underline —
so a document opening with `---` front matter has no root block and fails
with `MissingQuill` (§10), whose message names the fence to write instead.

`---` front matter is what broader-internet YAML conventions train an author
(or an LLM) to reach for, so the miss is worth one specific diagnostic rather
than a parse rule: a tolerance would be root-only, since composable cards can
have no `---` form, and half a rule is harder to learn than none.

### 3.3 System Metadata (`$`)

A block's YAML payload may contain **`$`-prefixed reserved keys** that carry
system metadata. The set is **closed**: only `$quill`, `$kind`, `$ext`,
and `$seed` are accepted. Any other `$`-prefixed key is a parse error. These
keys are ordinary YAML: they are read by the same YAML parser that handles
the rest of the payload, but they are **extracted** from the user field
set after parsing; they are not part of the data model's field map (§3.4).

In the typed model, the `$` entries live as typed variants of the
unified payload-item list (`PayloadItem::Quill`, `PayloadItem::Kind`, and
`PayloadItem::Meta` keyed by `MetaKey::Ext` / `MetaKey::Seed`), interleaved in
source order with user fields and YAML comments. They are surfaced through typed
accessors: `card.quill()`, `card.kind()`, `card.ext()`,
`card.seed()`: which return `Option<…>`. On a successfully parsed document the root
card always returns `Some(_)` for both `quill()` and `kind()` (with
`kind() == "main"`); composable cards return `None` for `quill()`, and
`kind()` returns the declared kind (any value other than `"main"`). The root's
`$kind: main` is synthesised when omitted in source (see §3.3 rules),
so the typed-accessor invariant holds regardless of whether the
author wrote the line.

- **`$quill: <name>@<version>`**: binds the document to a quill (see §3.5
  for the version-selector forms). The root block (the first block) must
  declare it; no other block may. The value is parsed into a typed quill
  reference as the block is read.
- **`$kind: <value>`**: identifies a card's kind. The value is
  name-validated at parse time and must match `[a-z_][a-z0-9_]*`. The kind
  `main` is **reserved for the document root**: the root block's `$kind` is
  `main` by position. An explicit `$kind: main` is accepted (round-trips
  byte-equal); omitting it is also accepted and synthesised at parse time.
  A non-`main` `$kind` on the root is a parse error. No composable card may
  declare `$kind: main`. Every block after the root is a composable card and
  must declare `$kind`: one that names none is a parse error (§10). Whether a
  kind names anything a quill declares is the schema's question, not the
  parser's.
- **`$ext: <mapping>`**: an opaque, optional **mapping** reserved for
  out-of-band extension data (UI editor state, agent annotations, …).
  Required to be a YAML mapping (object); scalars and sequences are
  rejected. Contents are carried verbatim through Markdown and storage
  DTO round-trips, and **never** appear in the plate JSON consumed by
  backends. Bespoke consumers namespace their state inside the
  map; e.g. `$ext.editor.title`, the canonical slot for a per-card
  display name (an editor-side rename).
  An empty `$ext: {}` is preserved as a distinct, explicit declaration.
- **`$seed: <mapping>`**: an optional **mapping keyed by composable
  card-kind**, present on the **root block only**; a composable block carrying
  `$seed` is a parse error, exactly like `$quill`. Each entry is a *sparse
  overlay*: the user fields (plus an optional reserved `$body` string) that a
  newly-added card of that kind starts with (`overlay › absent`, the body
  `overlay › empty`). Required to be a YAML
  mapping; scalars and sequences are rejected. Like `$ext` it carries verbatim
  through Markdown and storage DTO round-trips and **never** appears in the
  plate JSON consumed by backends; unlike `$ext` the seeding layer interprets
  it. Overlays are validated advisorily by
  `Quill::validate` and never gate render. An empty `$seed: {}` is preserved.

- `$` metadata entries may appear at any position within the payload, and
  may be interleaved with data fields. The emitter preserves source order
  (see §9); newly constructed metadata that does not have a source-order
  is emitted in the canonical key order `$quill`, `$kind`, `$ext`, `$seed`.
- A duplicate `$key` within a single block is a parse error (a YAML mapping
  cannot carry two entries under the same key).
- An unknown `$key` (anything outside `{quill, kind, ext, seed}`) is a parse
  error. A consumer needing a per-card key of its own carries it in `$ext` under
  its own namespace, uninterpreted and unguaranteed.
- An invalid `$quill` reference is a parse error.
- A `$`-prefixed key whose value type is wrong for the key (e.g. a sequence
  under `$quill`, a scalar under `$ext`) is a parse error.
- **YAML comments on `$` lines.** Inline trailing comments (`$quill: foo  #
  bound at build`) and adjacent own-line comments round-trip through the
  unified payload-item list: the same mechanism that preserves comments
  on data fields (§3.4). Both flavors survive parse → emit → parse.

### 3.4 Data Payload

User-defined fields sit in the same YAML payload as the `$` metadata keys
(§3.3); after metadata extraction, the remaining mapping entries are the
data payload.

- **Field names.** Every field name matches `/^[A-Za-z_][A-Za-z0-9_]*$/`. The
  pattern excludes `$`, so a data field name can never collide with any
  `$`-prefixed system key. Lowercase is the canonical, recommended convention,
  but uppercase ASCII letters are accepted and preserved verbatim; case is
  significant, so `title` and `Title` are distinct fields.
- **Whitespace-only payload.** A block whose payload (after metadata
  extraction) is only whitespace yields an empty field set.
- **Booleans.** A plain scalar is a boolean only when it spells `true` or
  `false`, in any letter case. YAML 1.1's `y`, `n`, `yes`, `no`, `on` and
  `off`, in any case, are strings.
- **YAML comments.** Both own-line comments (`# …` on their own line) and
  inline comments (`field: value  # note`) are supported on data fields and
  round-trip through `toMarkdown`. Comments inside nested YAML values
  (arrays, maps) are also preserved: the pre-scan reads each nested comment's
  container and slot from the YAML parser's event stream, and the emitter
  re-injects it there. The YAML's structure places a comment, whatever its
  indentation: a sequence written at its key's column, or a comment indented
  less than the block it sits in, keeps its slot.
  - Indentation decides only where the structure leaves a choice. After a
    collection's last child, a comment indented past the key holding the
    collection is inside it, and so is one at or past the first key or dash of
    a sequence item's collection; any other belongs to the collection around
    it. A comment indented under `key: []`, `key: {}` or a bare `key:` is
    inside that empty value, and so is one under a bare key whose tag reads
    as null, `key: !!null` or `key: !custom`. Under `key: !!str` or `key: !`
    the value is an empty string, and the comment follows the entry.
  - A comment inside a flow collection or on a multi-line scalar's line is the
    trailer of the entry holding the value, or follows it when one already
    trails it. A trailer on a sequence item's dash line is the item's, though
    the line holds the item's first key.
  - A mapping holding a merge (`<<`) reads its own keys, then each key the
    merge brings that it does not already hold, and `toMarkdown` writes them in
    that order. A comment at an own key stays with that key, so one between a
    merge and the next own key sits with that key, ahead of the merged keys.
    One ahead of a merge or on its line sits ahead of the keys the merge brings,
    one inside its value keeps its slot among them, and one closing the mapping
    after its merge follows them. A comment inside a merged key the mapping
    already holds sits ahead of where that key would. A sequence item whose
    mapping holds nothing but merges bringing no key emits as `{}`, and the
    comments inside it follow the item.
- **Tags.** A YAML tag on any node (`!include`, `!env`, a core `!!str`) is
  dropped with a `parse::unsupported_yaml_tag` warning: on a block value, a
  sequence element, a key, or a node inside a flow collection. The value is
  kept, and the tag does not round-trip. The warning carries the node's rooted
  `path` (`main.addr.street`, `main.to[0]`), or none under a `$` key, whose
  value has no document address. A core tag (`!!str 5`) takes effect as the
  value is read before it drops.

### 3.5 Version Selectors

The `$quill` value is `<name>` or `<name>@<version>`, where `<version>` is one
of:

| Form | Meaning |
|---|---|
| `name@2.1.0` | exact version |
| `name@2.1` | any `2.1.x` |
| `name@2` | any `2.x.x` |
| `name` | any version (`@version` omitted) |

Each version segment is ASCII digits. `name@latest` and `name@` fail as
`parse::invalid_quill_reference`: a reference with no selector omits the `@`.

Quill names match `/^[a-z_][a-z0-9_]*$/`. The selector is a pin, not a
resolver: this spec fixes the surface syntax accepted on the `$quill` line,
and matching a partial selector against a set of installed versions belongs to
a layer above the engine.

## 4. Block Detection

A single detector runs over the line stream. A `~~~` line, whatever its info
string, opens a card-yaml block **iff** all of the following hold:

**D0: Column zero.** The `~~~` opener has no leading spaces.

**D1: Blank line above.** The `~~~` line is line 1 of the document, or the
line immediately above it is blank.

**D2: Closing fence.** A matching `~~~` line at **column zero** appears
later in the document. An indented `~~~` line is payload (§3.2), never a
closer.

A `~~~` line that fails D0 (an indented opener) or D1 is **not** a card-yaml
opener; it is delegated to CommonMark, where an indented `~~~` is still a
valid fenced code block.

YAML content between recognised fence markers is opaque to detection: a
`~~~` line inside an open block is part of that block's payload, not a new
opener (though the canonical payload never produces such a line). In
particular, an *indented* `~~~` inside the payload; e.g. a tilde code fence
embedded in a `|` block-scalar value: is payload by the column-zero closer
rule (D2). A *column-zero* `~~~` can never be block-scalar content (YAML
requires scalar content to be indented past its key), so the closer is
unambiguous.

Failure of D0, D1, or D2 delegates the `~~~` line to CommonMark (an unclosed
`~~~` opener becomes a code block to EOF, with a non-fatal unclosed-fence
warning). A document with no closed root block fails with `MissingQuill`
(§10).

### 4.1 Worked Example

```
~~~
$quill: resume@1.0.0
$kind: main
title: CV
~~~

Main body text.

***

A thematic break in prose stays a thematic break.

~~~
$kind: profile
name: "Alice"
~~~

Profile body.
```

The first `~~~` is the root block (line 1, D1 satisfied). The second opens a
`profile` card (blank line above). The `***` is an ordinary CommonMark
thematic break: card-yaml does not reserve any thematic-break syntax.

## 5. Data Model

Parsing yields the `Document` model: a root block plus zero or more composable
cards, in document order. System metadata rides on the closed set of
`$`-prefixed keys (§3.3); user payload fields sit flat alongside them and cannot
collide, because field names exclude the `$` sigil.

- Root-block fields and card-field names may collide freely; each card is its
  own scope.
- Body text is preserved verbatim: whitespace, line endings, and inline
  CommonMark are untouched by the splitter.

How the engine serialises this model onto the wire for backends (the plate JSON)
is an engine concern, outside this markdown standard.

## 6. Markdown Content

Body regions (the root body and every card body) are rendered as CommonMark
0.31.2 with the extensions and deviations below.

### 6.1 Extensions

| Feature | Syntax | Notes |
|---|---|---|
| Strikethrough | `~~text~~` | GFM rules: word-bounded delimiter runs only. |
| Pipe tables | GFM pipe-table syntax with alignment rows | Supports `:---`, `:---:`, `---:` alignment. |
| Underline (HTML) | `<u>text</u>` | Allowlisted HTML (see §6.2). The only syntax for underline; handles intraword and arbitrary-range cases. |

### 6.2 Declared Deviations from CommonMark

**Raw HTML produces no output of its own, except an inline `<u>…</u>`, which
renders as underline, and an inline `<br>`, which is a hard break.** The parser
recognises HTML per CommonMark §4.6 / §6.6 and discards the HTML itself. What
else an HTML block holds depends on its type:

| HTML block (CommonMark §4.6) | What imports |
|---|---|
| Type 6 or 7: a tag line such as `<div>`, `<center>`, `<details>`, `<span>` or `<quill-keep>` | Everything but the tags. Each line holding only tags drops, and every other line parses as markdown, as though a blank line stood above and below each tag line; a line opening with a type-6 tag keeps the text after it. A `quill-*` tag is the carrier §6.4 defines, whose elements wrap what their tags hold. |
| Types 1–5: `<pre>`, `<script>`, `<style>` or `<textarea>`; a comment; a processing instruction; a declaration; CDATA | Nothing: the block drops whole. Text after its end marker (`-->`, `?>`, `>`, `]]>`, the closing tag) on its last line is a paragraph of its own. |

A type 1 block ends at the first line holding `</pre>`, `</script>`,
`</style>` or `</textarea>`, in any case and whatever tag opened it, as
CommonMark ends it.

Inside a type 6 or 7 block, a line opening a type 1–5 block or a fence keeps
that construct whole where it closes inside the block, and drops with the rest
of the block where it does not, so nothing in a block swallows what follows it.
A line holding only tags is a tag line wherever it stands outside code:

- Under a paragraph's text, where CommonMark reads it as inline HTML (a type 7
  tag cannot interrupt a paragraph), it ends the paragraph and opens a type 7
  block running to the paragraph's end, inside the paragraph's containers. A
  lazy line leaves the quotes it lacks.
- Under a setext heading's text, it moves below the heading's underline, one
  tag per line, so the heading keeps its lines.
- As a pipe-table row, it ends the table rather than adding a row.
- Between list items, it keeps the item before it open as a blank line would,
  so the items on either side stay one list.

A tag line holding an element's tag (§6.4) stands where its own indentation
puts it: under a paragraph's text it keeps its own container prefix, and it
does not continue the list item before it. Its element opens or closes there.

The allowlist is inline: `<u>` or `<br>` alone on its line is a tag line like
any other. An inline `<u>` pairs with a `</u>` as HTML pairs them, inside its
paragraph, heading, list item's text or table cell: a `</u>` closes the
innermost `<u>` still open, whatever marks lie between, so an underline crosses
`**` or `~~` freely. A `<u>` carrying an attribute drops and still takes its
`</u>`, and one still open where its text ends drops. An import reports each
dropped opening tag by its lowercase name, under `parse::dropped_construct`; a
closing tag, a comment, the content of a type 1–5 block, `quill-anchor` and an
element that closes (§6.4) report nothing.

Rationale: Typst has no HTML renderer, and arbitrary passthrough would create
an injection vector for downstream HTML-producing tooling; `<u>` is an
exception because no CommonMark-native syntax covers underline, and `<br>`
because a pipe-table row is one source line, with no room for a native hard
break. A tag line is transparent because authors write one around markdown
(`<div align="center">` above a table), and CommonMark runs a type 6 or 7 block
to the next blank line, which would drop the markdown with the tag.

**A link reference definition whose label starts with `^` is literal text**,
and so is every reference to its label: `[^1]: Word` imports as the text
`[^1]: Word`, and `text[^1]` as `text[^1]`. CommonMark reads the line as a
definition, making `[^1]` a link to `Word`; the syntax is the footnote of other
dialects, which this spec does not support (§6.3).

**A column-zero `~~~` with a blank line above it opens a card-yaml block,
not a fenced code block, whatever its info string** (§3.2, §4). A backtick
fence is the one fence for code. Rationale: the card-yaml format claims the
tilde fence outright, so whether a block is data never depends on its info
string.

No other syntax deviates from CommonMark. Delimiter-run semantics for `*`,
`_`, `**`, `__`, and `~~` follow CommonMark and GFM exactly: in particular,
`__text__` renders as strong emphasis, identical to `**text**`.

### 6.3 Limited or Out of Scope

The following are parsed where CommonMark or pulldown-cmark already
handles them, but produce limited or no Quillmark-specific output; fuller
support may come in a future revision:

- Images (`![alt](src)`): parsed into an `image` island carrying `{url, alt}`,
  which stores, round-trips to markdown, and reaches an editor — but no backend
  typesets one. The Typst backend draws nothing for it and warns under
  `backend::declined_construct`. `src` names no space: a document is portable
  across the versions its `$quill` selector admits and declares every other
  thing it references, so a path into one quill's file tree is not a binding a
  document may take.
- Math (`$…$`, `$$…$$`), task lists, definition lists: not supported; each
  imports as the literal text it is. In markdown body text `$` is literal;
  inside a `~~~` card-yaml payload `$` is reserved as the prefix for
  system-metadata keys (§3.3).
- Footnotes: not supported. A footnote-shaped definition (`[^1]: Word`) and its
  references import as literal text (§6.2), and the import reports each
  definition under `parse::dropped_construct` as `footnote_definition`.
- HTML comments: accepted syntactically, not rendered (see §6.2).
- `<br>` (any case, with attributes or a closing `/`) inside a paragraph or a
  table cell: a hard break. In a paragraph, one with no text before it on its
  line is dropped; in a heading it is a space. One alone on its line is a tag
  line (§6.2): it drops, and ends a paragraph it stands under. Outside a
  table, export writes the CommonMark-native hard break (trailing `\\` plus
  newline); inside a cell it writes `<br>`.

### 6.4 The `quill-*` Carrier

A `quill-*` element spells what CommonMark has no syntax for, such as
per-instance layout and anchors. Each is a CommonMark raw-HTML tag and a valid
custom-element name, which an HTML renderer draws as its children.

**Names.** A carrier tag's name is `quill-` and an element name matching
`[a-z][a-z0-9]*(-[a-z0-9]+)*`: `quill-keep`, `quill-table`, `quill-a1-b`.
CommonMark tag names admit no `:`, so the prefix stands where XML would write a
namespace (`quill:keep`). A tag name reads ASCII-case-insensitively, as HTML
names do, and the canonical spelling is lowercase. `quill-`, `quill-a--b` and
`quill-9` carry no element.

**Reserved names.** `table` and `cell` are reserved for the construct they
wrap, and `anchor` for the anchor spelling. A reserved name folds into its
construct where a construct declares the fold; none is ever an element of its
own, and a quill cannot declare one.

**Attributes.** A name matches `[a-z][a-z0-9_]*` and is none of `style`,
`class`, `id`, `href`, `src` and `name`, nor any name opening `on`, so the
carrier never holds markup a downstream HTML renderer would act on (§6.2's
rationale). An attribute outside that grammar, or one repeating a name already
read, is refused by name. A value reads double-quoted, single-quoted or
unquoted, and decodes `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;` and decimal or
hexadecimal references to a Unicode scalar value; any other `&` is text.

**Scope by syntax.** Import decides a carrier tag's scope from its line, with
no quill: a tag alone on its line is a block wrapper (a tag line, §6.2), and a
pair inside a line is inline.

**Canonical spelling.** An element is written with:

- attributes sorted by name, each value double-quoted, with `&`, `<`, `>` and
  `"` as `&amp;`, `&lt;`, `&gt;` and `&quot;`;
- a `|`, a control character, a bidi control or a line separator in a value as
  a hexadecimal reference (`&#x7C;`): a `|` ends a table cell, a line ending
  ends the tag's line, and §7 rewrites the rest;
- a block wrapper with each tag alone on its line and a blank line between it
  and what it wraps, inside the containers it sits in;
- an inline pair on one line with the text around it.

```markdown
<quill-keep>

**Signed**
J. Doe

</quill-keep>

Text with a <quill-keep note="a &amp; b">pair</quill-keep> inside a line.
```

**An element** of any name but the reserved three is stored, whatever quill
reads the document:

- A pair of tag lines wraps the blocks between them in the element, inside the
  containers around its open tag. The close tag closes the element where it is
  the innermost container open; a close tag naming no innermost element drops
  without a report. A pair wrapping nothing holds one empty paragraph.
- A pair inside one inline run (a paragraph's, a heading's, a list item's or a
  table cell's) marks the text between. A close tag closes the innermost open
  element of its name, so an element crosses other marks and elements freely.
- Each attribute in the grammar is kept as its string. One refused drops alone,
  reported as `quill-<name>[<attr>]`.
- Two adjacent block runs of one element stay two. Inline, a run unions with
  an adjacent or overlapping run of the same name and attributes.
- A Typst plate renders an element through the renderer it registers under
  the name; with none, `keep` holds what it wraps on one page and any other
  element renders what it wraps.

**An element that does not close** is transparent: its tags drop, what it wraps
imports, and `parse::dropped_construct` reports it under its tag name
(`quill-keep`), as any raw tag (§6.2). That covers a block element still open
where its list item, quote or body ends, an inline one still open at its run's
end, an inline pair holding nothing, a self-closing tag, and an open tag in an
image's alt text. A `quill-*` tag outside the grammar is a raw tag reported
the same way.

Export writes an inline element's tags, and `<u>` and `</u>`, outside the
emphasis delimiters closing and opening where they stand, so a tag never sits
at a delimiter run's edge, where its `<` or `>` would change the run's
flanking. A delimiter run spanning that position stays open around the tag.

```markdown
**bold <quill-hl tone="warm">both** highlit</quill-hl>
```

**`quill-table`** is a block wrapper around one pipe table, and folds its
attributes into the table's layout, whatever quill reads the document:

| Attribute | Value | Default |
|---|---|---|
| `widths` | whitespace-separated column weights, each a positive decimal integer or `auto` for an auto-fit column | every column `auto` |
| `align` | the table's placement: `left`, `center` or `right` | the plate's placement |
| `breakable` | `true`, or `false` to keep the table on one page | `true` |

```markdown
<quill-table align="center" breakable="false" widths="1 2 auto">

| Item | Description | Qty |
| --- | --- | --- |
| A | First | 1 |

</quill-table>
```

- Weights are relative and divide by their GCD: `2 4` reads as `1 2`. A
  `widths` shorter than the table pads with `auto`, and a longer one drops its
  extra entries.
- Each attribute at its default stores nothing, and export writes the wrapper
  only around a table holding a value other than its default.
- Column alignment stays in the delimiter row. Its dash counts carry no width,
  since a formatter pads them to the column.
- A wrapper holding anything but exactly one table drops whole: its tags drop,
  what it holds imports, and `parse::dropped_construct` reports `quill-table`.
- An attribute other than these three, and one whose value is outside its
  spelling, drops alone, reported as `quill-table[<name>]`.

**`quill-cell`** is an inline pair around a table cell's whole content, in the
header row or the body, and folds its attributes into the cell, whatever quill
reads the document:

| Attribute | Value | Default |
|---|---|---|
| `align` | the cell's horizontal alignment: `left`, `center` or `right` | its column's, from the delimiter row |
| `valign` | the cell's vertical alignment: `top`, `horizon` or `bottom` | `top` |

```markdown
| Item | Qty |
| --- | ---: |
| <quill-cell valign="bottom">Total</quill-cell> | <quill-cell align="center">42</quill-cell> |
```

- A pair folds when its open tag is the cell's first inline and its close tag
  the cell's last, and the cell holds no other `quill-cell` tag. What it wraps
  is the cell's content as written, edge whitespace included.
- A pair that does not wrap the whole cell folds nothing: text or markup
  before or after it, a second pair, a nested pair and an unclosed pair each
  leave the cell as written, its `quill-cell` tags dropped, and
  `parse::dropped_construct` reports each open tag as `quill-cell`.
- An `align` equal to its column's and a `valign` of `top` store nothing, and
  export writes the pair only around a cell holding a value other than its
  default.
- An attribute other than these two, and one whose value is outside its set,
  drops alone, reported as `quill-cell[<name>]`.
- A `quill-cell` outside a table cell is transparent, reported as
  `quill-cell`.

**`quill-anchor`** is reserved for an anchor's read-only spelling,
`<quill-anchor ref="…"></quill-anchor>`, which the annotated export writes and
no plain export does. Import drops it without a report, inline or alone on its
line. The annotated export writes one inline at each anchor's start:

- after the delimiters of the marks closing there and before those opening
  there;
- at a code span's or link's start, for an anchor inside one;
- in no code block, block island's line, table cell or empty line;
- at the end of a line whose tags in place would change what it imports to,
  and nowhere on a line where the end changes it too.

```markdown
A <quill-anchor ref="c1"></quill-anchor>**flagged** phrase.
```

**Strip.** Stripping the carrier from a markdown string removes every `quill-*`
tag the import reads as markup and keeps what a wrapper holds; a tag in a code
span, a fence, a comment or another tag's attribute stays. A line left holding
only container markers becomes a blank line inside them, and a list item's
marker left bare loses the blank lines after it, which would end the item.
Every other byte stays.

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
3. **Line-separator spacing.** Replace U+000B (LINE TABULATION), U+000C
   (FORM FEED), U+0085 (NEXT LINE), U+2028 (LINE SEPARATOR) and U+2029
   (PARAGRAPH SEPARATOR) with a single U+0020 space. CommonMark reads none
   of them as a line ending, but a backend lexer may, in which case the
   text after one is read as a block marker the author never wrote and two
   in a row split the paragraph. All five are Unicode whitespace, so a
   space keeps the words they part apart.
4. **Parser-guided repair.** The text is parsed, edited inside the spans that
   parse locates, and parsed again; text in a fenced or indented code block is
   never edited.
   - In a type 6 or 7 HTML block, each line holding only tags gets a blank
     line above and below, and a line opening with a type-6 tag splits after
     it, so the block's other lines reach the markdown parser (§6.2).
   - On a type 1–5 block's last line, text after the end marker moves to a
     line of its own.
   - A type 1 block's first closing tag is respelled as the block's own
     closing tag in lowercase, so the parse ends the block where CommonMark
     does.
   - A line holding only tags under a paragraph's text gets a blank line above
     it and one tag per line, opening a type 7 block. A line carrying the
     paragraph's quote markers is written inside the paragraph's containers.
   - A line holding only tags under a setext heading's text moves below the
     underline, one tag per line, written in the containers the line above
     would write it in.
   - A pipe-table row holding only tags gets a blank line above it, ending the
     table.
   - A block of tag lines that ends a list item is indented into the item, a
     blank line between its lines.
   - A line holding an element's tag keeps its own prefix in both cases above:
     it is neither written inside the paragraph's containers nor indented into
     the item. A piece split off it keeps the line's indentation.
   - A link reference definition labelled `^…` has its `[` backslash-escaped.

   A blank line written inside a container carries the container's `>`
   markers, and one closes freed text the next line would otherwise continue
   lazily. A freed line opening a container that holds an HTML block of its
   own (`> <div>`) is repaired by a further round.

Normalization is applied identically to the root body and every card
body. It is not applied to YAML payload values.

## 8. Limits

Conforming parsers MUST enforce these limits and MUST surface a parse
error when any is exceeded:

| Limit | Value |
|---|---|
| Document size | 10 MiB |
| YAML payload size per block | 1 MiB |
| Field count per block | 1000 |
| Card count per document | 1000 |

A conforming parser MUST also bound YAML nesting depth, at whatever depth
its YAML parser accepts, so that deeply nested input is refused rather than
exhausting the stack. The depth itself is the parser's to choose.

A block whose comments and tags record more than 64 bytes of path per byte of
the block, plus 64 KiB, is refused as `parse::invalid_structure`: each comment
and tag records the path of the collection holding it, each level costing its
key's bytes and a fixed overhead, and the bound keeps that memory linear in the
block.

Markdown block nesting depth (100) is enforced at import time by the
markdown→content parser (`Document::parse`); the Typst backend re-checks
at render as a backstop for content built without importing.

## 9. Emission Contract

`toMarkdown` always emits the **canonical form** of every block:

```
~~~
<payload items in source order>
~~~
```

That is: a bare `~~~` opener, the YAML payload (typed `$` system
metadata, user data fields, and YAML comments interleaved in source
order), and a `~~~` closer. The root block must declare `$quill`;
canonical emission also writes `$kind: main` on the root, synthesising
it when the input omitted the line (see §3.3). A composable card emits
`$kind: <kind>` when it declares one. A document round-trips to this canonical
shape: fence markers and YAML quoting are normalised, and an opener's info
string re-emits as bare `~~~`.
YAML comments (own-line and inline, including those adjacent to `$` lines)
survive the round-trip.

**Empty containers.** An empty mapping emits as `key: {}` and an empty
sequence as `key: []`, at every nesting level and under `$ext` / `$seed`
alike. Neither collapses to a bare `key:`, which reads back as null.

**Multi-line strings.** A string spanning lines emits as a `|` literal block
scalar, `|-` when it ends without a newline, its lines indented past the key,
at every nesting level. It emits double-quoted with `\n` escapes where a block
would not read back as the same string or would not survive an editor: a first
line opening on whitespace, whitespace ending a line, more than one trailing
newline, or a `\r`, control character, U+2028, U+2029 or U+FEFF.

Programmatically constructed metadata that does not have a source-order
emits in the canonical key order `$quill`, `$kind`, `$ext`, `$seed`: the
typed mutators (`set_quill` / `set_kind` / `set_ext` / `set_seed`)
insert at these positions.

### 9.1 Canonical Idempotence

A document in canonical form round-trips byte-equal under both
`toMarkdown ∘ fromMarkdown` and `fromStored ∘ toStored`:

- **`toMarkdown(fromMarkdown(canonical)) == canonical`**: the canonical
  form is a parse-emit fixed point.
- **`toMarkdown(fromMarkdown(arbitrary)) == toMarkdown(fromMarkdown(
  toMarkdown(fromMarkdown(arbitrary))))`**: at most one round-trip
  canonicalises any valid input; further round-trips are no-ops.
- **`toStored(fromStored(toStored(x))) == toStored(x)`** for any in-memory
  `Document x`: JSON serialization is byte-deterministic within a schema
  version.
- **The Markdown and JSON forms agree:** `toMarkdown(fromStored(toStored(x)))
  == toMarkdown(x)` for every `Document x` produced by
  `fromMarkdown(arbitrary)`. The two persistence formats canonicalise to
  the same in-memory model.

Arbitrary (non-canonical) input parses successfully when it satisfies §1–8
and converges to the canonical form on the first emit. Type fidelity (a
quoted `"42"` survives as a string, an unquoted `42` survives as an
integer) is preserved, along with the source positions of `$` metadata
keys and YAML comments; fence-marker length and quoting style are not.
The canonical form is what consumers should content-hash, content-address,
or compare for equality.

## 10. Errors

Parse errors include:

- The document has no recognised root block (`MissingQuill`). This covers an
  unclosed root fence: an unclosed `~~~` opener is delegated to CommonMark
  (§4) rather than erroring on its own, but with no closed root block the
  document still fails here. When the document *does* open with a `~~~`
  declaring `$quill`, the message names that opener's line and the missing
  closer (and the failed closer line, for a `~~` run or an indented `~~~`)
  rather than the generic shape. When it opens with `---` front matter
  declaring `$quill`, the message names the `~~~` fence to write instead
  (§3.2.1).
- The root block missing its `$quill` entry.
- The root block declaring a non-`main` `$kind` (an omitted `$kind` on
  the root is accepted and synthesised; only an explicit non-`main`
  value is rejected).
- A composable (non-root) block declaring `$quill`, or declaring
  `$kind: main` (which is reserved for the document root).
- A composable block whose mapping payload names no `$kind`
  (`MissingKind`), located at the opener's line. Its hint names the
  `$kind:` line and the backtick fence (§6.2), since tilde-fenced code whose
  text reads as a mapping is the other usual source.
- A duplicate `$key` within a single block (caught by the YAML parser as a
  duplicate mapping key).
- An unknown `$key` outside the closed set `{quill, kind, ext, seed}`.
- An invalid `$quill` reference.
- A `$` metadata key whose value type is incompatible with the key.
- A data-field name failing `/^[A-Za-z_][A-Za-z0-9_]*$/`.
- Invalid YAML inside any block payload.
- A payload that is YAML but not a mapping: a string, number, boolean or
  sequence (`PayloadNotMapping`). An empty or null payload is an empty
  mapping. The error is located at the opener's line and names the backtick
  fence (§6.2), since tilde-fenced code is the usual source.
- Any §8 limit exceeded.

## 11. References

- [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
- [GitHub Flavored Markdown](https://github.github.com/gfm/): pipe tables
  and strikethrough.
