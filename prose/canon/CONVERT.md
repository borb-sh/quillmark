# Content → Typst Lowering

> **Implementation**: `crates/backends/typst/src/` (the `emit` module)

## TL;DR

The Typst backend lowers a `Content` value to Typst markup with
`emit_content`, which walks the content: lines, anchored marks, embedded
islands, and never re-parses markdown. Alongside the markup it records a
per-segment source map (`content ↔ generated` byte windows). This is the only
markup-producing path in the render engine. Markdown reaches the content once, at
ingest, in `quillmark-content::import`; the normative rules for *which* markdown
a content can hold live in [markdown-spec.md §6](../references/markdown-spec.md);
this page documents how the backend lowers the content it produces.

## Pipeline

```
emit_content(&Normalized) -> Result<Emission, EmitError>
  ├─ block walk    lines → headings, paragraphs, code fences, lists, quotes, islands
  ├─ mark sweep    anchored marks → nested #strong[…] / #emph[…] / #link(…)[…] / …
  └─ source map    per-segment (content ↔ gen) windows + one (content, gen) pair per run
```

`Emission { markup: String, segments: Vec<SegmentMap> }`. The content is a
single Unicode-scalar-value (USV) `text` carrying `lines` (line attributes and
container nesting), `marks` (anchored `[start, end)` ranges), and `islands`
(tables/images at reserved slot chars). The walk is a terminator-model block
tree over `lines`; the inline pass sweeps `marks` and islands within each line.

A **segment** is a maximal run of lines joined by `Line::continues`: one
paragraph, one heading, one whole code fence, one island line. It is what
"paragraph-level" means against the content, and the unit a region keys on.

Only a `para`, `code` or unknown block takes continuations
(`LineKind::takes_continuations`), and a continuation stays inside one container
path. A heading, an island and a rule are one line, and both emitters render
that line alone, so a `continues` line after one would be text neither
projection reaches: `Content::normalize` clears the flag there,
`Content::validate` rejects it (`Invariant::ContinuesSingleLineBlock`,
`ContinuesAcrossContainers`), and `LineOp::SetContinues` refuses to write it.

## Escape functions

Two escapers guard the two Typst contexts; both live in `emit`:

- **`escape_markup`**: text in markup context. Escapes
  `\ // ~ * _ ` `` ` `` ` # [ ] { } $ < > @`, and the head of each shorthand
  Typst substitutes a codepoint for: `--` and `---` (dashes), `-?` (an invisible
  soft hyphen), `-` before a number (a minus sign), `...` (an ellipsis). What a
  shorthand leaves behind is too short to re-form it. Applied to plain text runs
  and to a table cell's text.

  **Smart quotes are the quill's.** `'` and `"` pass through, so Typst's
  language-aware substitution applies and a quill chooses with
  `#set smartquote(enabled: false)`. Escaping them here would settle that for
  every quill with no way back — unlike the shorthands, which the lexer decides
  and no set rule reaches.
- **`escape_string`**: text inside a Typst string literal. Escapes
  `\ " \n \r \t` and other control characters as `\u{…}`. Applied to `#link`
  URLs, code content, and code-fence language tags.

Both are position-blind. Typst's heading `=`, list `-`/`+`/`N.`, and term `/`
are special only as a line's first token, each firing on a space after it or on
the line ending there, so a text run landing in that position takes a single
`\` prefix. The byte sits outside every source-map run window, so
`generated == escape_markup(content)` stays exact. Unprefixed, a paragraph
holding one bare `/` is a term list whose colon is missing, and the compile
fails.

That position is Typst's `at_start`, and it is four places: column 0, a list
item's body head, the head of every content block `[…]` the emitter opens — one
per wrap, one per table cell — and the spaces or tabs behind any of them, which
Typst reads as trivia. A heading's body is none of them. The guard lands on the
marker rather than ahead of the indentation: `\` before a space is Typst's
linebreak, not an escape.

The same `\` guards the tail of a `#…` expression. Typst reads a `(` directly
after one as that call's arguments, a `.` before an identifier as a field
access, and a `;` as the expression's terminator, so an emitted `#raw(…)`, a
wrap's closing `]` and an island's `)` would each run on into the document text
behind them — or, for the `;`, eat the character. Trivia between the two ends
the expression on its own.

And a third time, at a **seam**: a position where the emitter writes nothing
between two runs, which an island this build renders as nothing (an unknown
type, an empty table) leaves behind. The escapers are per run, so a
multi-character rule sees one side of such a join at a time and the pair
straddling it escapes neither: `a/`, that island, `/b` would write `a//b`, a
comment that eats the rest of the line.

Debug builds parse every emission with Typst's own parser: a syntax error there
is a lowering bug, never a document's.

## Element mapping

| Content construct | Typst |
|---|---|
| `LineKind::Heading{level}` | `=` … `======` (`level` × `=`) |
| `LineKind::Para` | inline content; a hard break (a `continues` line join) emits `#linebreak()`, a soft break is a space (both settled at import) |
| `LineKind::Code{lang}` (code fence) | `#raw(block: true, lang: "…", "…")`; `lang:` emitted only when the language tag is non-empty |
| `LineKind::Rule` (thematic break) | `#line(length: 100%)` |
| `LineKind::Unknown` (open set) | inline content, as `Para`: the role is lost to the projection, not to storage |
| `MarkKind::Strong` | `#strong[…]` |
| `MarkKind::Emph` | `#emph[…]` |
| `MarkKind::Underline` | `#underline[…]` |
| `MarkKind::Strike` | `#strike[…]` |
| `MarkKind::Code` | `#raw("…")` (inline) |
| `MarkKind::Link{url}` | `#link("url")[…]` (`escape_string` on the url) |
| `MarkKind::Anchor` / `Unknown` | nothing |
| `Container::ListItem` (bullet) | `- ` |
| `Container::ListItem` (ordered) | `+ ` auto-numbered; the run's first item emits `N. `, which restarts Typst's running counter so an adjacent list numbers from its own `start` |
| `Container::Quote` | `#quote(block: true)[…]` |
| `Container::Unknown` (open set) | nothing: transparent; its run lowers at the enclosing level, one block, no wrapper |
| `image` island | nothing, plus one `backend::declined_construct` warning per field (see [Declined images](#declined-images)) |
| `table` island | `#table(columns: N, align: (…), table.header(…), …)` |

Table alignment maps `none→auto`, `left`, `center`, `right`; the `align:`
argument is emitted only when at least one column is non-default. A table cell is
canonical `{text, marks}`, lowered through the same mark sweep as prose: a
formatted cell reaches `#strong[…]` / `#emph[…]` / `#raw(…)` / `#link(…)[…]`, not
an escaped source slice.

**Block quotes render** as `#quote(block: true)[…]`: the one lowering
divergence from a flat inline pass; a quote's inner blocks lower under the
block-level discipline.

**Every generated line opens at the enclosing list depth**, two spaces per item.
Typst ends a list at a block written to column 0, so one emitter rule indents
leaves and containers alike: what the content nests, the markup nests.

Anchor and unknown marks emit nothing; unknown island types emit nothing
(parallel to the HTML rule at import). An unknown line kind lowers as a
paragraph and an unknown container as nothing at all: every content vocabulary
is open, so a build that predates a construct renders it plainly instead of
failing
([DOCUMENT_STORAGE § Open vocabularies](DOCUMENT_STORAGE.md#open-vocabularies)).
Content that import never admits into the content: raw HTML other than `<u>`,
HTML comments, `<br>`, math, footnotes, task lists, definition lists
(markdown-spec §6.3): is absent here.

### Declined images

An `image` island lowers to nothing, and the compile that dropped it carries one
[`backend::declined_construct`](ERROR.md#warning-flow) warning per field:
`args` `{backend: "typst", construct: "image", count}`, `path` the field's
`DocPath`. Declined, not unknown — the count comes off `islands`, so an image
counts wherever it sits, and a field holding one says so rather than losing it
quietly.

What such a `url` names is undecided. A document is quill-free but for
`$quill`, which is a *selector* ([VERSIONING](VERSIONING.md)) — `memo@1` admits
1.0.0 and 1.1.0 alike — and everything else a document references is
self-contained or declared. A path into one quill's file tree is neither, and
no `DocPath`, `validate` signal or url space makes it either. A backend that
resolved the string anyway would bind documents to a reading nothing versions.

The sibling lane `plate::unsupported_construct`
(`crates/core/src/quill/support.rs`) is a *quill's* declaration about a body;
this one is the backend's own observation about every content field.

### Island props

An island's `props` is a per-type canonical object: the shape this lowering
reads and the shape the WASM boundary pins:

- **`table`** → `{ header: Cell[], rows: Cell[][], aligns: Align[] }`. `header`
  and each row hold `Cell = { text, marks }` (marks lowered through the sweep
  above); `aligns` is one `none | left | center | right` per column. Import
  normalizes to a single column count: header, every row, and `aligns` padded
  to the widest, so `columns:` and `align:` agree.
- **`image`** → `{ url, alt }`; `alt` is the empty string when the source omits
  it. What `url` names is undecided (see [Declined images](#declined-images)).

The `KnownIslandType` dispatch (`crates/content/src/island.rs`) owns these
shapes engine-side; the WASM surface pins them as `TableProps` / `ImageProps` /
`TableCell` and types `ContentIsland.props` per the open `type`
(`crates/bindings/wasm/src/engine.rs`). An island of any other type keeps opaque
`props` and lowers to nothing, as above.

## Mark sweep

Marks anchor freely and may overlap (Peritext semantics from an editor); Typst
markup nests. The sweep opens marks by priority `(start, longer-span-first,
kind-ord)` and closes-and-reopens deeper survivors at each overlap boundary, so
free overlap lowers to properly nested markup: `strong[0,4)` over `emph[2,6)`
on `abcdef` becomes `#strong[ab#emph[cd]]#emph[ef]`, bracket-balanced. `code`
marks render atomically as `#raw("…")` (their content is a string literal, so no
inner mark applies).

## Source map

Each segment records a `SegmentMap`:

```rust
struct SegmentMap {
    content: Range<usize>,                                 // USV, the segment's content span
    gen:    Range<usize>,                                 // bytes into `markup`
    runs:   Vec<(Range<usize>, Range<usize>, EscapeCtx)>, // (content USV, gen bytes) per text run
}
enum EscapeCtx { Markup, StringLit }
```

A **run** is one plain-text stretch between marks, islands, and line breaks;
`gen` slices exactly `escape_markup(content_slice)` (or `escape_string` for code /
string-literal runs). Structural bytes: mark delimiters, container syntax,
`#linebreak()`: fall between runs, inside `gen` but under no run. This is the
only place a per-segment source map can be produced, because it is the only place
that both lowers the content and knows the resulting byte layout.

Per-character spans within a run are **recomputed**, not stored: a one-scan
treats the `//`→`\/\/` markup escape as a 2-char/4-byte cluster and every other
character as its own. The `escape_tripwire` test pins that scan against
`escape_markup` / `escape_string` byte-for-byte, so an escape-rule change fails
loud.

## Where markdown is parsed

The markdown engine (`pulldown-cmark`) appears exactly once in the workspace, in
`quillmark-content::import`, run at ingest. `import` normalizes, parses, and
lowers markdown into the content (markdown-spec §6 is its normative acceptance
surface); every downstream render walks the content. No render path parses
markdown.

The one other caller is `export` itself, deliberately: its safety net settles a
line's markdown by re-importing it and dropping marks until the text comes back
intact, because CommonMark's emphasis algorithm has corners no local rule
captures. So export's correctness is *defined* by import: the two codecs cannot
be changed independently, and `to_markdown` transitively depends on the parser.
A line's re-parses are bounded by a probe budget, not by its mark count; the
trade is stated in the `export` module doc.

## Codegen integration

`generate_lib_typ` (`helper.rs`) lowers each content field's content to a markup
**block** binding `#let _qm_cN = [ .. ]` via `emit_content`, then rebases the
emitter's segment map from block-relative to `lib.typ`-relative offsets, yielding
one `ContentMap { path, block, segments }` per content field. The generated
`data` dict references `_qm_cN`; a blank content stays an empty string literal.
The file parser parses each block once: no runtime `eval`, no `json()` blob.

## Gotchas

- **All code is `#raw(...)`, not backtick markup.** Both inline code and code
  fences put content into a string literal where backtick runs are inert: no
  delimiter can collide, and `escape_string` covers the only specials (`"` / `\`).
  The function form makes inline-vs-block explicit via `block:`. A fence buffers
  its lines into one string joined by escaped `\n`.
- **Ordered-list start.** Typst's `+` marker always restarts at 1. A start
  number `≠ 1` is preserved by writing the explicit number on the first item
  (`5. …`); Typst then continues the following `+` items from there.
- **List markers.** Bullet items become Typst `-`; ordered items become Typst `+`
  (its enumeration marker), not `-`.
