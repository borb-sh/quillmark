# Surface Parity

> **Implementation**: `crates/fixtures/resources/parity/`, `crates/quillmark/tests/`
> **Related**: [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [CONVERT.md](CONVERT.md), [BINDINGS.md](BINDINGS.md)

## TL;DR

A content construct has one form, its stored JSON, and markdown, the op wire
and the Typst lowering are codecs or projections of it. This page states the
invariants every surface keeps and a matrix of what each surface does with each
construct. A conformance corpus pins every row: an engine test asserts it, and
`@quillmark/wasm` ships it for downstream codecs.

## Invariants

1. **One model.** Every construct has one canonical form in the stored content
   ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies"), and
   every other surface is a codec or a projection of it. Markdown is a
   projection some consumers use, never the storage boundary.
2. **Pure import.** `from_markdown` is a function of its text alone: it reads no
   quill, declaration or session.
3. **Fixed point.** `from_markdown(to_markdown(c)) == c`, except for anchors,
   which a markdown write keeps by diff-rebase (`revise`, `rebase`), never by
   the projection.
4. **No silent lane.** For every construct, quill and backend, the outcome is
   honored, inert with a signal, or refused at the write. The signal reaches
   every door: parse, import, revise, `validate` and render.
5. **Storage honesty.** Discriminators are closed and payload carriers are
   opaque. A knob is a key wherever a carrier exists, and a kind only where none
   can hold it.
6. **Carry or refuse.** A surface that cannot spell a construct carries it
   opaquely or refuses the write. It never re-encodes without it.
7. **One declaration.** What a quill honors is declared once in `Quill.yaml`,
   and render, `validate`, `schema()` and the blueprint read that declaration.
   `Quill.yaml` declares no body construct, so every Typst quill honors the set
   [CONVERT.md](CONVERT.md) maps.

Where the code falls short of an invariant, the matrix cell says so:
`drops silently` is the outcome invariants 4 and 6 rule out, and on a stored
construct's markdown cell it is also where the fixed point fails.

## The matrix

One row per construct the content holds, and one per markdown spelling the
import meets. The columns:

| Column | What it reads |
|---|---|
| Markdown | `from_markdown` and `to_markdown` ([markdown-spec.md](../references/markdown-spec.md) §6) |
| Stored JSON | the canonical content, `serial::to_canonical_value` ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies") |
| Op wire | the authored lane: `overwrite` (`serial::from_authored_value`) and a `ChangeBundle`'s ops |
| Typst lowering | `emit_content`, as [CONVERT.md](CONVERT.md) § "Element mapping" maps it |
| Blueprint | what `QuillConfig::blueprint` teaches ([BLUEPRINT.md](BLUEPRINT.md) § "Richtext fields"): no body construct, so `n/a` on every row |
| Validate | `Quill::validate`'s verdict on the stored construct |
| Signal | the code a cell reports under, with its `construct` arg |

| Cell | Meaning |
|---|---|
| spells | the surface has a form for the construct and reads it back as written |
| carries opaquely | the surface keeps the construct without reading it: an opaque carrier, or a markdown write through `revise` keeping an anchor by diff-rebase |
| honors | the lowering draws it as CONVERT.md maps it |
| declines with a signal | the surface drops it and reports so: an import under `parse::dropped_construct`, a render under `backend::declined_construct`, `validate` under `validation::declined_construct` |
| refuses | the surface rejects the write |
| silent: honored | no signal, and none is owed: the outcome is the one the construct asks for |
| n/a | the surface never meets the construct: past the markdown column, a spelling the import does not store |
| drops silently | outside the vocabulary: the surface loses the construct and nothing reports it; in the validate column, the render loses it and `validate` says nothing |
| inert | the lowering reads no declaration for the key and draws as if it were absent, and `validate` says nothing; borb-sh/quillmark#2083 brings the signal |

### Lines

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Blueprint | Validate | Signal |
|---|---|---|---|---|---|---|---|
| `line.para`: paragraph | spells | spells | spells | honors | n/a | silent: honored | none |
| `line.heading`: heading and its `level` | spells | spells | spells | honors | n/a | silent: honored | none |
| `line.code`: code fence | spells | spells | spells | honors | n/a | silent: honored | none |
| `line.code.lang`: code fence with a `lang` | spells | spells | spells | honors | n/a | silent: honored | none |
| `line.code.html`: a fence holding `<!-- a --> b` | spells | spells | spells | honors | n/a | silent: honored | none |
| `line.rule`: thematic break | spells | spells | spells | honors | n/a | silent: honored | none |
| `line.continues`: hard break | spells | spells | spells | honors | n/a | silent: honored | none |

### Containers

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Blueprint | Validate | Signal |
|---|---|---|---|---|---|---|---|
| `container.list_item.bullet`: bullet item | spells | spells | spells | honors | n/a | silent: honored | none |
| `container.list_item.ordered`: ordered item | spells | spells | spells | honors | n/a | silent: honored | none |
| `container.list_item.start`: ordered item, custom `start` | spells | spells | spells | honors | n/a | silent: honored | none |
| `container.list_item.instance`: two adjacent ordered lists | spells | spells | spells | honors | n/a | silent: honored | none |
| `container.list_item.instance.bullet`: two adjacent bullet lists | spells | spells | spells | drops silently | n/a | drops silently | none |
| `container.quote`: block quote | spells | spells | spells | honors | n/a | silent: honored | none |
| `container.quote.instance`: two adjacent quotes | spells | spells | spells | honors | n/a | silent: honored | none |

Two adjacent bullet lists lower to items a blank line parts, which Typst reads
as one wide list: the `instance` boundary does not reach the page.

### Marks

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Blueprint | Validate | Signal |
|---|---|---|---|---|---|---|---|
| `mark.strong` | spells | spells | spells | honors | n/a | silent: honored | none |
| `mark.emph` | spells | spells | spells | honors | n/a | silent: honored | none |
| `mark.underline` | spells | spells | spells | honors | n/a | silent: honored | none |
| `mark.strike` | spells | spells | spells | honors | n/a | silent: honored | none |
| `mark.code` | spells | spells | spells | honors | n/a | silent: honored | none |
| `mark.link` | spells | spells | spells | honors | n/a | silent: honored | none |
| `mark.anchor` | carries opaquely | spells | spells | honors | n/a | silent: honored | none |

An anchor draws nothing, which is how the lowering honors it, and a cold
`to_markdown` → `from_markdown` loses it ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Anchor-id identity").
The annotated read spells it at its start as a `quill-anchor` tag the import
drops, so the markdown cell stays `carries opaquely`: a write keeps an anchor by
diff-rebase, never by its tag. The row's corpus entry pins the spelling under
`annotated`.

### Islands

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Blueprint | Validate | Signal |
|---|---|---|---|---|---|---|---|
| `island.table.aligns`: table with column `aligns` | spells | spells | spells | honors | n/a | silent: honored | none |
| `island.table.cell.marks`: a cell holding marks | spells | spells | spells | honors | n/a | silent: honored | none |
| `island.table.cell.break`: a cell's `\n`, spelled `<br>` | spells | spells | spells | honors | n/a | silent: honored | none |
| `island.table.props.unnamed`: a props key the engine does not name | drops silently | carries opaquely | carries opaquely | drops silently | n/a | drops silently | none |
| `island.table.cell.unnamed`: a cell key the engine does not name | drops silently | carries opaquely | carries opaquely | drops silently | n/a | drops silently | none |
| `island.table.props.widths`: column weights, `null` an auto-fit column | spells | spells | spells | inert | n/a | inert | none |
| `island.table.props.widths.auto`: every column auto-fit, the default | spells | spells | spells | silent: honored | n/a | silent: honored | none |
| `island.table.props.align`: the table's placement | spells | spells | spells | inert | n/a | inert | none |
| `island.table.props.breakable`: `false`, the table kept on one page | spells | spells | spells | inert | n/a | inert | none |
| `island.table.props.breakable.true`: `true`, the default | spells | spells | spells | silent: honored | n/a | silent: honored | none |
| `island.image` | spells | spells | spells | declines with a signal | n/a | declines with a signal | `backend::declined_construct`, `validation::declined_construct`, `image` |

A table re-imports from its pipe syntax, so `to_markdown` and `revise` both
mint it without a key the engine does not name.

`widths`, `align` and `breakable` are spelled on a `quill-table` wrapper
([markdown-spec.md](../references/markdown-spec.md) §6.4), and each is absent
at its default, so a default row stores no key.

### Spellings

Rows for a markdown spelling rather than a construct: an alias of a construct
above, or markup the content does not store.

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Blueprint | Validate | Signal |
|---|---|---|---|---|---|---|---|
| `html.br`: inline `<br>`, a hard break | spells | spells | spells | honors | n/a | silent: honored | none |
| `html.u`: inline `<u>` in any case, underline | spells | spells | spells | honors | n/a | silent: honored | none |
| `html.u.attrs`: `<u>` carrying an attribute | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `u` |
| `html.span`: other inline HTML | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `span` |
| `html.block6.table`: a type 6 block, `<div>`, around a table | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `div` |
| `html.block6.text`: a type 6 block, `<center>`, around text | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `center` |
| `html.block1`: a type 1 block, `<pre>` | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `pre` |
| `html.comment` | silent: honored | n/a | n/a | n/a | n/a | n/a | none |
| `html.tag_line.paragraph`: a tag line under paragraph text | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `div` |
| `html.tag_line.list`: a tag line between list items | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `div` |
| `carrier.block`: a `quill-*` wrapper nothing models, a type 7 block | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `quill-keep` |
| `carrier.inline`: a `quill-*` pair nothing models, inside a line | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `quill-keep` |
| `carrier.table`: the reserved `quill-table` around a table | spells | spells | spells | inert | n/a | inert | none |
| `carrier.table.holds_other`: a `quill-table` around anything but one table | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `quill-table` |
| `carrier.table.attr`: a `quill-table` attribute the engine does not name | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `quill-table[foo]` |
| `carrier.table.value`: a `quill-table` attribute value outside its spelling | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `quill-table[widths]` |
| `carrier.anchor`: an echoed `quill-anchor` | silent: honored | n/a | n/a | n/a | n/a | n/a | none |
| `markdown.footnote_definition`: `[^1]: Word` | declines with a signal | n/a | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `footnote_definition` |
| `markdown.link_title`: a link's title | drops silently | n/a | n/a | n/a | n/a | n/a | none |
| `markdown.cell_image`: an image in a table cell | drops silently | n/a | n/a | n/a | n/a | n/a | none |

`html.br` and `html.u` land as a hard break and an underline, so their cells
past markdown read those constructs. What a markup row wraps imports as
markdown and has its own row. A cell image's alt text lands as the cell's text
and its url nowhere.

## The corpus

`crates/fixtures/resources/parity/parity.json` is one JSON array, an entry per
matrix row:

| Key | Holds |
|---|---|
| `name` | the row's name |
| `markdown` | the row's spelling, or `null` where markdown spells none (`mark.anchor`, the unnamed keys) |
| `annotated` | `to_markdown_annotated(content)`'s markdown, on a row whose construct has a read-only spelling (`mark.anchor`); absent elsewhere |
| `content` | the canonical stored content, `serial::to_canonical_value`, pretty-printed so a change diffs by line |
| `typst` | substrings the body's lowering contains, never a whole emission |
| `signals.import` | `{construct, count}` per `parse::dropped_construct` the import raises, in order |
| `signals.render` | the codes a one-shot render's warnings carry, in order |
| `signals.validate` | the codes `Quill::validate` reports, in order; absent is none |

`crates/quillmark/tests/parity.rs` asserts each entry and names every one that
fails:

- `content` decodes through `serial::from_canonical_value` and re-encodes to
  itself.
- A spelled entry imports to `content`, warning `signals.import`, and so does
  a body holding it through `Document::parse`.
- A spelled entry's `to_markdown(content)` re-imports to `content`, the fixed
  point. An entry markdown cannot spell is checked on the other surfaces only.
- An entry's `annotated` is `to_markdown_annotated(content)`'s markdown, and
  imports, warning nothing, to `content` without its anchors.
- The body's block in the generated helper, which is `emit_content`'s markup,
  contains every `typst` substring.
- A render through the `table_demo` fixture quill warns exactly
  `signals.render`.
- `Quill::validate` on that document reports exactly `signals.validate`, and
  its `validation::declined_construct` set (path, construct, count) is the
  render's `backend::declined_construct` set.

A row whose construct the import does not store still has its `typst`: what the
markup wrapped reaches the page.

`scripts/build-wasm.sh` ships the file at the root of `@quillmark/wasm` as
`parity.json`, and the package's `parity.test.js` round-trips every spelled
entry through `importMarkdown` and `exportMarkdown`, and imports each
`annotated` read. A downstream codec pins the copy in the package version it
imports: decoding each `content` into its own state and encoding it back yields
`content`.

A construct enters the engine with its row and its entry, and a change to what
a surface does with one edits both.
