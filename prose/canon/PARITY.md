# Surface Parity

> **Implementation**: `crates/fixtures/resources/parity/`, `crates/quillmark/tests/`
> **Related**: [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [CONVERT.md](CONVERT.md), [BINDINGS.md](BINDINGS.md)

## TL;DR

A content construct has one form, its stored JSON, and markdown, the op wire
and the Typst lowering are codecs or projections of it. This page states the
invariants every surface keeps and a matrix of what each surface does with each
construct. A conformance corpus pins every row: an engine test asserts its
markdown, stored JSON, op wire, Typst lowering, validate and signal cells.

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
   every door that meets the construct: a markdown drop the parse, import or
   revise that drops it, and a decline `validate` and render.
5. **Storage honesty.** Discriminators are closed and payload carriers are
   opaque. A knob is a key wherever a carrier exists, and a kind only where none
   can hold it.
6. **Carry or refuse.** A surface that cannot spell a construct carries it
   opaquely or refuses the write. It never re-encodes without it.

Where the code falls short of an invariant, the matrix cell says so:
`drops silently` is the outcome invariants 4 and 6 rule out, and on a stored
construct's markdown cell it is also where the fixed point fails.

The matrix reads a body. A `richtext` field's markdown string reports what
it drops where a conform, a revise or `validate` imports it; a typed `set`
and a card inserted with a string body drop it with no signal
([ERROR.md](ERROR.md#warning-flow)).

## The matrix

One row per construct the content holds, and one per markdown spelling the
import meets. The columns:

| Column | What it reads |
|---|---|
| Markdown | `from_markdown` and `to_markdown` ([markdown-spec.md](../references/markdown-spec.md) §6) |
| Stored JSON | the canonical content, `serial::to_canonical_value` ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Content vocabularies") |
| Op wire | the authored lane: `overwrite` (`serial::from_authored_value`) and a `ChangeBundle`'s ops |
| Typst lowering | `emit_content`, as [CONVERT.md](CONVERT.md) § "Element mapping" maps it |
| Validate | `Quill::validate`'s verdict on the stored construct |
| Signal | the code a cell reports under, with its `construct` arg |

| Cell | Meaning |
|---|---|
| spells | the surface has a form for the construct and reads it back as written |
| carries opaquely | the surface keeps the construct without reading it: an opaque carrier, or a markdown write through `revise` keeping an anchor by diff-rebase |
| honors | the lowering draws it as CONVERT.md maps it |
| declines with a signal | the surface drops it and reports so: an import under `parse::dropped_construct`, a render under `backend::declined_construct`, or `typst::unregistered_element` for an element, `validate` under `validation::declined_construct` |
| refuses | the surface rejects the write |
| silent: honored | no signal, and none is owed: the outcome is the one the construct asks for |
| n/a | the surface never meets the construct: past the markdown column, a spelling the import does not store |
| drops silently | outside the vocabulary: the surface loses the construct and nothing reports it; in the validate column, the render loses it and `validate` says nothing |

### Lines

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Validate | Signal |
|---|---|---|---|---|---|---|
| `line.para`: paragraph | spells | spells | spells | honors | silent: honored | none |
| `line.heading`: heading and its `level` | spells | spells | spells | honors | silent: honored | none |
| `line.code`: code fence | spells | spells | spells | honors | silent: honored | none |
| `line.code.lang`: code fence with a `lang` | spells | spells | spells | honors | silent: honored | none |
| `line.code.html`: a fence holding `<!-- a --> b` | spells | spells | spells | honors | silent: honored | none |
| `line.rule`: thematic break | spells | spells | spells | honors | silent: honored | none |
| `line.continues`: hard break | spells | spells | spells | honors | silent: honored | none |

### Containers

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Validate | Signal |
|---|---|---|---|---|---|---|
| `container.list_item.bullet`: bullet item | spells | spells | spells | honors | silent: honored | none |
| `container.list_item.ordered`: ordered item | spells | spells | spells | honors | silent: honored | none |
| `container.list_item.start`: ordered item, custom `start` | spells | spells | spells | honors | silent: honored | none |
| `container.list_item.instance`: two adjacent ordered lists | spells | spells | spells | honors | silent: honored | none |
| `container.list_item.instance.bullet`: two adjacent bullet lists | spells | spells | spells | drops silently | drops silently | none |
| `container.list_item.task`: a task item, ticked and open | spells | spells | spells | honors | silent: honored | none |
| `container.quote`: block quote | spells | spells | spells | honors | silent: honored | none |
| `container.quote.instance`: two adjacent quotes | spells | spells | spells | honors | silent: honored | none |
| `container.element`: an element around blocks | spells | spells | spells | honors | silent: honored | none |
| `container.element.attrs`: an element carrying an attribute | spells | spells | spells | honors | silent: honored | none |
| `container.element.instance`: two adjacent runs of one element | spells | spells | spells | honors | silent: honored | none |
| `container.element.in_item`: an element in a list item | spells | spells | spells | honors | silent: honored | none |
| `container.element.around_list`: an element around a list | spells | spells | spells | honors | silent: honored | none |
| `container.element.unregistered`: an element no renderer takes | spells | spells | spells | declines with a signal | drops silently | `typst::unregistered_element` |

Two adjacent bullet lists lower to items a blank line parts, which Typst reads
as one wide list: the `instance` boundary does not reach the page.

An element lowers through the helper's dispatcher, which draws it with the
renderer a plate registers under its name ([CONVERT.md](CONVERT.md#elements)).
One no renderer takes draws what it wraps, and only a compile reads the
registry, so `validate` cannot report it.
A task item's body lowers the same way, through the renderer a plate sets on
`tasks`, else as a box ticked when done.

### Marks

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Validate | Signal |
|---|---|---|---|---|---|---|
| `mark.strong` | spells | spells | spells | honors | silent: honored | none |
| `mark.emph` | spells | spells | spells | honors | silent: honored | none |
| `mark.underline` | spells | spells | spells | honors | silent: honored | none |
| `mark.strike` | spells | spells | spells | honors | silent: honored | none |
| `mark.code` | spells | spells | spells | honors | silent: honored | none |
| `mark.link` | spells | spells | spells | honors | silent: honored | none |
| `mark.anchor` | carries opaquely | spells | spells | honors | silent: honored | none |

An anchor draws nothing, which is how the lowering honors it, and a cold
`to_markdown` → `from_markdown` loses it ([DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § "Anchor-id identity").
The annotated read spells it at its start as a `qm-anchor` tag the import
drops, so the markdown cell stays `carries opaquely`: a write keeps an anchor by
diff-rebase alone. The row's corpus entry pins the spelling under
`annotated`.

### Islands

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Validate | Signal |
|---|---|---|---|---|---|---|
| `island.table.aligns`: table with column `aligns` | spells | spells | spells | honors | silent: honored | none |
| `island.table.cell.marks`: a cell holding marks | spells | spells | spells | honors | silent: honored | none |
| `island.table.cell.break`: a cell's `\n`, spelled `<br>` | spells | spells | spells | honors | silent: honored | none |
| `island.table.cell.align`: a cell's horizontal alignment | spells | spells | spells | honors | silent: honored | none |
| `island.table.cell.valign`: a cell's vertical alignment | spells | spells | spells | honors | silent: honored | none |
| `island.table.cell.align_valign`: both, over a cell holding marks | spells | spells | spells | honors | silent: honored | none |
| `island.table.props.unnamed`: a props key the engine does not name | drops silently | carries opaquely | carries opaquely | drops silently | drops silently | none |
| `island.table.cell.unnamed`: a cell key the engine does not name | drops silently | carries opaquely | carries opaquely | drops silently | drops silently | none |
| `island.table.cell.value`: a cell `valign` outside its set | drops silently | carries opaquely | carries opaquely | drops silently | drops silently | none |
| `island.table.props.value`: a table `align` outside its set | drops silently | carries opaquely | carries opaquely | drops silently | drops silently | none |
| `island.table.props.widths`: column weights, `null` an auto-fit column | spells | spells | spells | honors | silent: honored | none |
| `island.table.props.widths.auto`: every column auto-fit, the default | spells | spells | spells | silent: honored | silent: honored | none |
| `island.table.props.align`: the table's placement | spells | spells | spells | honors | silent: honored | none |
| `island.table.props.headless`: a table drawn with no header row | spells | spells | spells | honors | silent: honored | none |
| `island.image` | spells | spells | spells | declines with a signal | declines with a signal | `backend::declined_construct`, `validation::declined_construct`, `image` |

A table re-imports from its pipe syntax, so `to_markdown` and `revise` both
mint it without a key the engine does not name.

`widths`, `align` and `headless` are spelled on a `qm-table` wrapper
([markdown-spec.md](../references/markdown-spec.md) §6.4). Each is absent at
its default, so a default row stores no key.

A cell's `align` and `valign` are spelled on a `qm-cell` pair around the cell's
whole content (§6.4).

A table or cell key holding a value outside its set rests as written and rides
as a key the engine does not name does.

### Spellings

Rows for a markdown spelling rather than a construct: an alias of a construct
above, or markup the content does not store.

| Row | Markdown | Stored JSON | Op wire | Typst lowering | Validate | Signal |
|---|---|---|---|---|---|---|
| `html.br`: inline `<br>`, a hard break | spells | spells | spells | honors | silent: honored | none |
| `html.u`: inline `<u>` in any case, underline | spells | spells | spells | honors | silent: honored | none |
| `html.u.attrs`: `<u>` carrying an attribute | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `u` |
| `html.span`: other inline HTML | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `span` |
| `html.block6.table`: a type 6 block, `<div>`, with blank lines around a table | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `div` |
| `html.block6.text`: a type 6 block, `<center>`, tight around text, dropping it | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `center` |
| `html.block1`: a type 1 block, `<pre>` | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `pre` |
| `html.comment` | silent: honored | n/a | n/a | n/a | n/a | none |
| `html.tag_line.paragraph`: a type 6 tag line under paragraph text, dropping what follows to the blank line | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `div` |
| `html.tag_line.list`: a tag line between list items, ending the list | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `div` |
| `carrier.element.tight`: element tag lines tight around markdown | spells | spells | spells | honors | silent: honored | none |
| `carrier.element.void`: an element around nothing | spells | spells | spells | honors | silent: honored | none |
| `carrier.element.unclosed`: an element still open where the body ends | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-keep` |
| `carrier.element.self_closing`: a self-closing element tag | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-keep` |
| `carrier.element.inline`: an element pair inside a line | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-hl` |
| `carrier.element.attr`: an element attribute outside the grammar | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-keep[onclick]` |
| `carrier.element.stray_close`: an element close tag with nothing open | silent: honored | n/a | n/a | n/a | n/a | none |
| `carrier.element.stray_close.tight`: an element close tag with nothing open, tight above markdown it keeps | silent: honored | n/a | n/a | n/a | n/a | none |
| `carrier.table`: the reserved `qm-table` around a table | spells | spells | spells | honors | silent: honored | none |
| `carrier.table.holds_other`: a `qm-table` around anything but one table | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-table` |
| `carrier.table.attr`: a `qm-table` attribute the engine does not name | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-table[foo]` |
| `carrier.table.value`: a `qm-table` attribute value outside its spelling | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-table[widths]` |
| `carrier.cell.partial`: a `qm-cell` pair around part of a cell | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-cell` |
| `carrier.cell.value`: a `qm-cell` attribute value outside its spelling | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `qm-cell[valign]` |
| `carrier.anchor`: an echoed `qm-anchor` | silent: honored | n/a | n/a | n/a | n/a | none |
| `markdown.link_title`: a link's title | drops silently | n/a | n/a | n/a | n/a | none |
| `markdown.cell_image`: an image in a table cell | drops silently | n/a | n/a | n/a | n/a | none |
| `markdown.footnote`: a footnote definition, its reference kept as text | declines with a signal | n/a | n/a | n/a | n/a | `parse::dropped_construct`, `footnote` |

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
| `reimports` | what `from_markdown(to_markdown(content))` lands on, on a row markdown cannot spell (`mark.anchor`, the unnamed keys); absent where it is `content` |
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
- `content` lands through `serial::from_authored_value`, and through one
  `ChangeBundle` read by `change_bundle_from_value` and applied to an empty
  field: the text by `delta`, each island by an `insert` at its slot, each
  line's kind, containers and `continues` by line ops, and every mark by an
  `add`.
- `to_markdown(content)` re-imports, warning nothing, to `reimports`: the fixed
  point where the entry has none.
- A body holding `content`, revised with `to_markdown(content)`, lands on
  `reimports` plus every anchor `content` holds, warning nothing.
- An entry's `annotated` is `to_markdown_annotated(content)`'s markdown,
  imports, warning nothing, to `content` without its anchors, and revises a
  body holding `content` to `content`.
- The rest runs through the fixture quill `table_demo`, against `typst` and
  `signals`.
- The body's block in the generated helper, which is `emit_content`'s markup,
  contains every `typst` substring.
- A one-shot render warns exactly `signals.render`.
- `Quill::validate` on that document reports exactly `signals.validate`, and
  its `validation::declined_construct` list (path, construct, count) is the
  render's `backend::declined_construct` list.
- The matrix above has a row per entry and an entry per row, and a row's
  Markdown, Typst lowering and Validate cells read `declines with a signal`
  exactly where the entry's signals warn there, its Signal cell naming each
  code and imported construct.

A row whose construct the import does not store still has its `typst`: what the
markup wrapped reaches the page.

Each binding holds what only it can break, the crossing: every entry's
`content` and import warnings cross out of a markdown import and a parse, and
`content` crosses back in through a write and storage. The WASM binding's
`parity.test.js` goes through `importMarkdown`, `fromMarkdown`, `overwrite` and
`fromStored`; Python's `tests/test_parity.py` through `Document.from_markdown`
and a stored load.

A construct enters the engine with its row and its entry, and a change to what
a surface does with one edits both.
