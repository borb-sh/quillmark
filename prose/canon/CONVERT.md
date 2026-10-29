# Content → Typst Lowering

> **Implementation**: `crates/backends/typst/src/` (the `emit` module)

## TL;DR

The Typst backend lowers a `Content` value to Typst markup with
`emit_content`, which walks the content: lines, anchored marks, embedded
islands, and never re-parses markdown. Alongside the markup it records a
per-segment source map (`content ↔ generated` byte windows). This is the render
path's only markup producer. Markdown reaches the content once, at
ingest, in `quillmark-content::import`; the normative rules for *which* markdown
a content can hold live in [markdown-spec.md §6](../references/markdown-spec.md);
this page documents how the backend lowers the content it produces.

## Pipeline

```
emit_content(&Normalized) -> Result<Emission, EmitError>
  ├─ block walk    lines → headings, paragraphs, code fences, lists, quotes, islands
  ├─ mark sweep    anchored marks → nested #strong[…] / #emph[…] / #link(…)[…] / #_qm-font(…)[…] / …
  └─ source map    per-segment (content ↔ gen) windows + one (content, gen) pair per run
```

`Emission { markup: String, segments: Vec<SegmentMap> }`. The content is a
single Unicode-scalar-value (USV) `text` carrying `lines` (line attributes and
container nesting), `marks` (anchored `[start, end)` ranges), and `islands`
(tables/images at reserved slot chars). The walk is a terminator-model block
tree over `lines`; the inline pass sweeps `marks` and islands within each line.

A **segment** is a maximal run of lines joined by `Line::continues`: one
paragraph, one heading, one whole code fence, one block island's line. It is what
"paragraph-level" means against the content, and the unit a region keys on.

Only a `para` or `code` block takes continuations
(`LineKind::takes_continuations`), and a continuation stays inside one container
path. A heading and a rule are one line, and so is a `para` whose sole content
is a block island's slot — the kind cannot tell that one from prose, so
`Content::block_island_at` reads it off the island's type. Both emitters
render such a line alone, so a `continues` line after one would be text neither
projection reaches: `Content::normalize` clears the flag there, as it does on
the first line and across a container boundary. `LineOp::SetContinues` writes
what it is given and the mint settles it.

## Escape functions

Three escapers guard two Typst contexts; all live in `emit`:

- **`escape_markup`**: text in markup context. Escapes
  `\ // ~ * _ ` `` ` `` ` # [ ] { } $ < > @`, and the head of each shorthand
  Typst substitutes a codepoint for: `--` and `---` (dashes), `-?` (an invisible
  soft hyphen), `-` before a number (a minus sign), `...` (an ellipsis). What a
  shorthand leaves behind is too short to re-form it. Applied to plain text runs
  and to a table cell's text.

  **Stray newlines lower to a space.** Every character Typst's lexer reads as a
  newline besides `\n` — `\r`, VT, FF, NEL, U+2028, U+2029 — becomes a space.
  The content ingress drops or spaces all six, so one reaches the emitter only
  from a content built or decoded some other way. No escape neutralizes one: a
  `\` before whitespace is Typst's linebreak. One reopens `at_start`, so the
  text behind it parses as a heading, list or term marker; two in a row split
  the paragraph. One character to one byte, so the per-character span scan
  stays exact, and the space is trivia the line-anchor guard holds open behind.

  **Spaces hold their width.** Typst reads a run of spaces as one, so a space
  with another behind it becomes `~`: `a.  b` writes `a.~ b`, two spaces wide
  and still breakable at the last. Line layout trims a trailing `~` with the
  space, so a run at a line's end adds nothing.

  **Smart quotes are the quill's.** `'` and `"` pass through, so Typst's
  language-aware substitution applies and a quill chooses with
  `#set smartquote(enabled: false)`. Escaping them here would settle that for
  every quill with no way back — unlike the shorthands, which the lexer decides
  and no set rule reaches.
- **`escape_indent`**: the spaces, tabs and stray newlines opening a line,
  which Typst drops whole. `escape_markup` with every space `~`, so typed
  indentation reaches the page, cannot wrap off the word behind it, and keeps
  the span scan 1:1. A line opens at the line anchor below and behind a
  `#linebreak()`. A tab stays trivia: the editor draws it at its own width, and
  no one byte matches that.
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
per wrap, one per table cell — and the tabs behind any of them, which Typst
reads as trivia. A heading's body is none of them. A `~` from `escape_indent`
ends it. The guard lands on the marker rather than ahead of the tabs: `\`
before whitespace is Typst's linebreak, not an escape.

The same `\` guards the tail of a `#…` expression. Typst reads a `(` directly
after one as that call's arguments, a `.` before an identifier as a field
access, and a `;` as the expression's terminator, so an emitted `#raw(…)`, a
wrap's closing `]` and an island's `)` would each run on into the document text
behind them — or, for the `;`, eat the character. Trivia between the two ends
the expression on its own.

And a third time, at a **seam**: a position where the emitter writes nothing
between two runs, which an island this build renders as nothing (an image, an
empty table) leaves behind. The escapers are per run, so a
multi-character rule sees one side of such a join at a time and the pair
straddling it escapes neither: `a/`, that island, `/b` would write `a//b`, a
comment that eats the rest of the line.

Debug builds parse every emission with Typst's own parser: a syntax error there
is a lowering bug, never a document's.

## Element mapping

| Content construct | Typst |
|---|---|
| `LineKind::Heading{level}` | `=` … `======` (`level` × `=`); a table's slot on the line closes the heading, the table sets between, and the text after it opens a heading of the same level |
| `LineKind::Para` | inline content; a hard break (a `continues` line join) emits `#linebreak()`, a soft break is a space (both settled at import). A slot alone on the line is the island's block, lowered by the island dispatch below; a table's slot sharing its line lowers in place, and Typst sets the table as a block between the prose either side |
| `LineKind::Code{lang}` (code fence) | `#raw(block: true, lang: "…", "…")`; `lang:` emitted only when the language tag is non-empty |
| `LineKind::Rule` (thematic break) | `#line(length: 100%)` |
| `MarkKind::Strong` | `#strong[…]` |
| `MarkKind::Emph` | `#emph[…]` |
| `MarkKind::Underline` | `#underline[…]` |
| `MarkKind::Strike` | `#strike[…]` |
| `MarkKind::Code` | `#raw("…")` (inline) |
| `MarkKind::Link{url}` | `#link("url")[…]` (`escape_string` on the url) |
| `MarkKind::Anchor` | nothing |
| `MarkKind::FontFamily{name}` | `#_qm-font("name")[…]`, one call per stretch one family sets (see [Font runs](#font-runs)) |
| `Container::ListItem` (bullet) | `- ` |
| `Container::ListItem` (ordered) | `+ ` auto-numbered; the run's first item emits `N. `, which restarts Typst's running counter so an adjacent list numbers from its own `start` |
| `Container::ListItem{checked}` (task) | the item's marker, then `#_qm-task(done)[…]` around its body (see [Tasks](#tasks)) |
| `Container::Quote` | `#quote(block: true)[…]` |
| `image` island | nothing, plus one `backend::declined_construct` warning per field (see [Declined images](#declined-images)) |
| `table` island | `#table(columns: N, align: (…), table.header(…), …)` |
| `footnote` island | `#footnote[…]`, the note lowered as a table cell is |
| `math` island | `$…$`, or `$ … $` where `display` (see [Equations](#equations)) |

Table alignment maps `none→auto`, `left`, `center`, `right`; the `align:`
argument is emitted only when at least one column is non-default.

The table's layout keys lower as:

| Knob | Lowering |
|---|---|
| `widths` | `columns: (2fr, 1fr, auto)`, a weight to `fr` and `null` to `auto`, in place of `columns: N` |
| `align` | `align(center, table(…))` under `context`, with `table.cell` set to the horizontal `align.alignment` the table stands in, so the placement moves the table and no text inside it, save a horizontal alignment the plate's own `table.cell` show rule sets |
| `head` | that many rows from the top lower inside one `table.header`, which Typst repeats on each page the table runs onto; at `0` none does, and `header` lowers as the first body row |
| a cell's `align` / `valign` | `table.cell(align: right + bottom)[…]`, `middle` as `horizon`, which Typst folds with the column's alignment; a value outside its set lowers as absent |
| a cell's `colspan` / `rowspan` | `table.cell(colspan: 2, rowspan: 3)[…]`, each cell its span covers left out, so Typst's own placement, which skips a slot a span above fills, lands every cell where the grid holds it |

A table cell is canonical `{text, marks}`, lowered through the same mark sweep
as prose: a formatted cell reaches `#strong[…]` / `#emph[…]` / `#raw(…)` /
`#link(…)[…]`, not an escaped source slice. A `\n` in a cell's text is a line break and lowers to
`#linebreak()`, as a prose hard break does. A word holding an `@`, or two dots each between alphanumerics and a letter, takes an empty `#box()` after each `@` and each such dot, a code span's text counting toward none of it: Typst breaks only at UAX #14 opportunities, which an email address or dotted name has none of, so one wider than its column would run into the next. The box is a break opportunity that adds no text, so the address copies out of the PDF whole. A footnote's note takes none.

**Block quotes render** as `#quote(block: true)[…]`: the one lowering
divergence from a flat inline pass; a quote's inner blocks lower under the
block-level discipline.

**Every generated line opens at the enclosing list depth**, two spaces per item.
Typst ends a list at a block written to column 0, so one emitter rule indents
leaves and containers alike: what the content nests, the markup nests.

Anchor marks emit nothing; an `image` island emits nothing (see
[Declined images](#declined-images)).
Raw HTML other than an inline `<u>`, `<br>` (a hard break), a `<table>`
(markdown-spec §6.8), which imports as a `table` island, and the `qm-*`
carrier (markdown-spec §6.4) never enters the content, so it is absent here.

### Tasks

A task item's body lowers into `_qm-task(done, body)`, which the helper
defines. It calls the renderer a plate sets on the
helper's `tasks` state as `render(done, body)`, else draws a box ahead of the
body, ticked when `done`. It reads the state's final value, so a plate's update
may stand after the content it renders:

```typst
#import "@local/quillmark-helper:0.2.0" as qm
#qm.tasks.update(_ => (done, body) => if done { strike(body) } else { body })
```

The call opens on the marker's line and closes after the item's last block, so
an item's later blocks and nested lists stay in it.

### Equations

A `math` island's `tex` lowers to Typst math through a translator of its own
(`crates/backends/typst/src/tex.rs`), never a package. Each atom it writes is
one character, a number, a string or a call, space-separated, so no run of
letters reads as a Typst identifier and no run of punctuation as a shorthand.
A symbol command lowers to its Unicode character rather than a Typst symbol
name, ASCII punctuation is escaped, and a non-ASCII character is a `\u{…}`
escape, one token whatever grapheme rules would join it to. Fractions, roots,
scripts, accents, font commands, `\left…\right`, spacing, operator names,
`\text`, and the `matrix` family, `cases` and `aligned` environments lower to
the Typst calls that draw them.

A command outside that vocabulary draws as its own name, and the compile
carries one [`typst::unsupported_tex`](ERROR.md#warning-flow) warning per
content field naming each, as does a `\not` with no negated form or a
`\left` delimiter outside the vocabulary, both of which draw nothing. A `%`
comment runs to its line's end, and storage drops it before it spaces the
lines. Groups and arguments nest 32 deep; past that the lowering draws their
characters and symbols flat and warns once per field. Whatever the TeX, the
lowering parses, and no line of it ends on the bare `\` of a linebreak, which
would escape what follows.

Typst sets an equation in the family `New Computer Modern Math` unless a plate
names another with `show math.equation: set text(font: ..)`, and lays one out
only in a face with an OpenType `MATH` table. A quill shipping no face of that
family has the embedded Noto Sans Math, cut to the blocks an equation commonly
draws from, answer to the name and to its own, so an equation draws whatever
text fonts the quill ships.

### Font runs

Family marks never overlap ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies"). The
sweep cuts a line at every edge of a family mark and of a code mark, and wraps
each stretch in a call to the helper's `_qm-font` for the one family a mark
sets there, so `Arimo[0,3)`, `Carlito[3,6)`, `Arimo[6,9)` lowers as three
calls:

```typst
#_qm-font("Arimo")[abc]#_qm-font("Carlito")[def]#_qm-font("Arimo")[ghi]
```

- Codegen writes the families a plate can name, the quill's and the
  fallback's, lowercased, into the helper as `_qm-families`, math faces left
  out, and the host library's as `_qm-offered`, each to the alias its faces
  are filed under ([ARCHITECTURE.md](ARCHITECTURE.md) § Fonts). `_qm-font`
  sets a family only where `_qm-offered` holds it, its alias ahead of the font
  list in force filtered to `_qm-families`, so a character the family lacks
  falls through to the surrounding font and Typst's own "unknown font family"
  warning never fires. A workspace maps each family to itself, since Typst
  there loads the library's files under their own families.
- A family the library lacks keeps the surrounding font, and the compile
  carries one [`typst::unknown_font_family`](ERROR.md#warning-flow) warning per
  family and content field, `path` the field's `DocPath`.
- A stretch stops at a code span, whose `raw` keeps its own font, and
  at a math or table island's slot. A footnote's slot stays inside it: a
  footnote entry takes none of its reference's styles, so the note is set by
  its own runs.

### Declined images

An `image` island lowers to nothing, and the compile that dropped it carries one
[`backend::declined_construct`](ERROR.md#warning-flow) warning per field:
`args` `{backend: "typst", construct: "image", count}`, `path` the field's
`DocPath`. Declined, not unknown — the count comes off `islands`, so an image
counts wherever it sits, and a field holding one says so rather than losing it
quietly.

No backend resolves such a `url`. A document is quill-free but for
`$quill`, which is a *selector* ([VERSIONING](VERSIONING.md)) — `memo@1.2` admits
1.2.0 and 1.2.1 alike — and everything else a document references is
self-contained or declared. A path into one quill's file tree is neither, and
no `DocPath`, `validate` signal or url space makes it either. A backend that
resolved the string would bind documents to a reading nothing versions.

### Island props

An island's `props` is a per-type canonical object: the shape this lowering
reads and the shape the WASM boundary pins:

- **`table`** → `{ header: Cell[], rows: Cell[][], aligns: Align[] }`. `header`
  and each row hold `Cell = { text, marks }` (marks lowered through the sweep
  above); `aligns` is one `none | left | center | right` per column. Import
  normalizes to a single column count: header, every row, and `aligns` padded
  to the widest, so `columns:` and `align:` agree.
  The optional layout keys `widths`, `align` and `head`, and a cell's `align`,
  `valign`, `colspan` and `rowspan`
  ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies"), lower
  as [above](#element-mapping); an absent one draws at its default: auto-fit,
  at the plate's placement, in the column's alignment, under one header row,
  in one slot.
- **`image`** → `{ url, alt }`; `alt` is the empty string when the source omits
  it. No backend resolves `url` (see [Declined images](#declined-images)).
- **`footnote`** → `Cell`, the note, with no `\n` at either edge. Its `\n` is
  a line break, as a cell's is.
- **`math`** → `{ tex, display? }`: the TeX as written between the dollars,
  and `display: true` on a `$$…$$` equation (see [Equations](#equations)).

The `IslandType` dispatch (`crates/content/src/island.rs`) owns these shapes
Rust-side; the WASM surface pins them as `TableProps` / `ImageProps` /
`FootnoteProps` / `MathProps` / `TableCell` and types `ContentIsland.props` per the closed `type`
(`crates/bindings/wasm/src/engine.rs`).

## Mark sweep

Marks anchor freely and may overlap (Peritext semantics from an editor); Typst
markup nests. The sweep opens marks by priority `(start, longer-span-first,
kind-ord)` and closes-and-reopens deeper survivors at each overlap boundary, so
free overlap lowers to properly nested markup: `strong[0,4)` over `emph[2,6)`
on `abcdef` becomes `#strong[ab#emph[cd]]#emph[ef]`, bracket-balanced. `code`
marks render atomically as `#raw("…")` (their content is a string literal, so no
inner mark applies). Font marks join the sweep as [font runs](#font-runs),
already cut so no two overlap.

## Source map

Each segment records a `SegmentMap`:

```rust
struct SegmentMap {
    content:   Range<usize>,                                 // USV, the segment's content span
    generated: Range<usize>,                                 // bytes into `markup`
    runs:      Vec<(Range<usize>, Range<usize>, EscapeCtx)>, // (content USV, generated bytes) per text run
}
enum EscapeCtx { Markup, StringLit }
```

A **run** is one plain-text stretch between marks, islands, and line breaks;
its generated bytes are exactly `escape_markup(content_slice)` (or `escape_string` for code /
string-literal runs). Structural bytes: mark delimiters, container syntax,
`#linebreak()`: fall between runs, inside `generated` but under no run. This is the
only place a per-segment source map can be produced, because it is the only place
that both lowers the content and knows the resulting byte layout.

Per-character spans within a run are **recomputed**, not stored: a one-scan
treats the `//`→`\/\/` markup escape as a 2-char/4-byte cluster and every other
character as its own. The `escape_tripwire` test pins that scan against
`escape_markup` / `escape_string` byte-for-byte, so an escape-rule change fails
loud.

## Where markdown is parsed

The Markdown parser (`pulldown-cmark`) appears exactly once in the workspace, in
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
