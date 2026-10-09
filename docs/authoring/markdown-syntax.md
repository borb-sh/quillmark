# Markdown Syntax

Quillmark Markdown is a **strict superset of [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)** with a small set of [GitHub Flavored Markdown](https://github.github.com/gfm/) extensions and **two declared deviations**: [raw HTML](#raw-html-is-not-rendered-except-u-and-br) and [`~~~` fences](#a-column-zero-always-opens-a-card-yaml-block). If you already know CommonMark, you only need to learn what is on this page.

For the authoritative grammar, block-detection rules, normalization, and limits, see the formal [Markdown specification](../reference/markdown-spec.md).

## Foundation

Body content (the prose after each [card-yaml block](card-yaml.md), including any [card](card-yaml.md#card-blocks)) is parsed as CommonMark 0.31.2. Headings, emphasis, links, images, lists, code blocks, blockquotes, thematic breaks, and inline code all behave exactly as the [CommonMark spec](https://spec.commonmark.org/0.31.2/) defines them. (An image parses, stores and round-trips, but **no backend typesets one**: the Typst backend drops it and warns under `backend::declined_construct`, because no backend resolves a content image's `src`.)

For the conventional syntax of these elements, refer to:

- [CommonMark spec](https://spec.commonmark.org/0.31.2/): the base grammar.
- [GFM spec](https://github.github.com/gfm/): pipe tables and strikethrough.

## Selected GFM extensions

Quillmark enables a small, stable subset of GFM:

| Feature | Syntax | Notes |
|---|---|---|
| Strikethrough | `~~text~~` | Standard GFM rules; word-bounded delimiter runs. |
| Pipe tables | `\| col \| col \|` with alignment row | Supports `:---`, `:---:`, `---:` alignment. |
| Underline | `<u>text</u>` | An allow-listed raw-HTML tag (see [the deviation below](#raw-html-is-not-rendered-except-u-and-br)). |
| Task lists | `- [ ] open`, `- [x] done` | A checkbox before the item's text, ticked for `[x]`. |
| Footnotes | `text[^1]` and `[^1]: The note.` | See [Footnotes](#footnotes). |

Autolinks beyond CommonMark's and other GFM features are **not** enabled.

### Footnotes

A reference `[^label]` prints a footnote holding its definition's note. The label is any name, matched without regard to case, and the definition can sit anywhere in the body.

```markdown
The figure is from the annual report.[^src]

[^src]: Finance office, *Annual Report*, page 4.
```

A note is one short passage: its paragraphs and line breaks keep their breaks, and its emphasis, code and links keep their formatting. A list, heading, code block, quote, table or rule inside a note keeps its text as lines of the note, and the import warns under `parse::dropped_construct` with `footnote[list]` and the like. A definition nothing references, a second definition of one label, and a reference inside a table cell or another note drop with a `footnote` warning. A note cited twice prints twice, once for each reference.

Quillmark writes footnotes back as `[^1]`, `[^2]`, … in reading order, each definition after the body.

## Table layout and elements: `qm-*` tags

Markdown has no syntax for column widths, table placement or keeping a block on one page. Quillmark spells them with `qm-*` tags: HTML custom elements, which a browser or GitHub draws as just what they wrap.

Each `qm-*` tag stands alone on its line, with a blank line above and below:

```markdown
<qm-keep>

**Signed**
J. Doe

</qm-keep>
```

Written tight against markdown, a tag line opens an HTML block that runs to the next blank line, and the block drops whole, markdown included, with a `parse::dropped_construct` warning whose hint names the fix. A tag inside a line, a self-closing tag (`<qm-keep/>`) and a tag left open drop too, keeping what they wrap.

### Table widths and placement

A `qm-table` around one pipe table sets its layout:

```markdown
<qm-table widths="2 1 auto" align="center">

| Item | Description | Qty |
| --- | --- | --- |
| A | First | 1 |

</qm-table>
```

| Attribute | Value | Without it |
|---|---|---|
| `widths` | One weight per column: a positive whole number, or `auto` to fit the column to its content. Weights are relative, so `2 1` makes the first column twice as wide as the second. | Every column fits its content. |
| `align` | Where the table sits: `left`, `center` or `right`. | The quill's placement. |

Text alignment within a column stays in the delimiter row (`:---:`). A `widths` with fewer entries than the table has columns leaves the rest `auto`, and one with more ignores the extra entries. A `qm-table` around anything but one table drops, keeping what it holds.

### Elements

Any other `qm-<name>` pair is an element around the blocks between its tags, and the quill decides how to draw it. A name is lowercase letters and digits in words joined by single hyphens, opening with a letter, such as `qm-sig` or `qm-stamp-2`; `qm-anchor` is reserved. `qm-keep` is built into every Typst quill and keeps what it wraps on one page; around a `qm-table` it keeps the table from splitting. A quill with no renderer for an element draws what it wraps as if the tags were absent.

An element around nothing is its two tags on adjacent lines, which suits a signature line or a stamp the quill draws:

```markdown
<qm-sig>
</qm-sig>
```

An element's attributes are strings the quill reads, such as `<qm-stamp tone="urgent">`. An attribute name is lowercase letters, digits and `_`, opening with a letter. `style`, `class`, `id`, `href`, `src` and names opening `on` are refused, so the tags never carry markup a browser would act on.

## Deviations from CommonMark

### Raw HTML is not rendered, except `<u>` and `<br>`

CommonMark passes raw HTML through to the output. Quillmark recognises raw HTML as CommonMark does (so it does not break paragraph structure) but **discards every tag**, except the ones it supports: `<u>…</u>` renders as underline, an inline `<br>` is a line break, and a `qm-*` tag spells [table layout or an element](#table-layout-and-elements-qm-tags).

```markdown
<u>This is underlined</u>, even <u>across word boundaries</u>.
<span style="color: red">The span's tags drop; its text stays.</span>
<!-- HTML comments are also dropped -->

<div align="center">

| The blank lines keep this table: | the div's tags drop |
|---|---|

</div>
```

Why: Typst (the rendering backend) has no HTML renderer, and arbitrary HTML passthrough would create injection risks for downstream tooling. `<u>` is allowed because no CommonMark-native syntax covers arbitrary-range underline, and `<br>` because a table row is one line, with no room for a CommonMark hard break.

Consequences:

- `<br>`, `<br/>`, `<br />` (any case) are a line break, in a paragraph or a table cell: `| line one<br>line two |`. Outside a table, a CommonMark hard break does the same: two trailing spaces before a newline, or a trailing `\` before a newline. In a paragraph, a `<br>` with no text before it on its line produces nothing. A `<br>` alone on a line opens an HTML block, below.
- HTML entities decode as CommonMark specifies: `Fish &amp; chips, &#65;BC` reads `Fish & chips, ABC`.
- An HTML block drops as CommonMark reads it. A line starting with a tag such as `<div>`, `<center>` or `<details>` opens a block that runs to the next blank line, so markdown on the lines under it drops with it; a blank line after the tag line keeps what follows. Embedded SVG draws nothing.
- A `<pre>`, `<script>`, `<style>` or `<textarea>` block drops whole, content included, through the end of its closing tag's line.
- HTML comments do not appear in output. Text after a comment's `-->` on the same line still does.
- Each dropped tag is reported as a `parse::dropped_construct` warning naming it in lowercase and counting its opening tags, a `<pre>` block's included. Comments and a `qm-*` element that closes are not reported, and neither are closing tags and `<qm-anchor>` tags, except where the block they open drops markdown. A tag in a `richtext` field's value in the card-yaml block drops the same way, reported at the field.

### A column-zero `~~~` always opens a card-yaml block

CommonMark reads a `~~~` fence as a code block, like a backtick fence. Quillmark
reads a `~~~` at column zero with a blank line above it as a
[card-yaml block](card-yaml.md), whatever its info string: `~~~python` opens a
card, not Python.

````markdown
```python
print("hi")
```
````

Why: card-yaml blocks claim the tilde fence outright, so whether a block is
data never depends on its info string.

Consequences:

- Fence code with backticks. A longer tilde run or a language tag does not
  escape.
- Tilde-fenced code in a body is parsed as a card, so the parse fails at the
  fence's line, with the backtick fence to write instead in its hint. Code that
  reads as a YAML string or list fails as `parse::payload_not_mapping`; code
  that reads as a mapping names no `$kind`, and fails as `parse::missing_kind`.
- A `~~~` indented by one to three spaces, or with no blank line above it, stays
  a CommonMark code block.

## Out of scope

The following are not supported:

- **Math** (`$…$`, `$$…$$`): `$` is treated as a literal character.
- **Definition lists**: they render as the literal text written.

A link's title (`[text](url "Title")`) drops at import, with no warning. A construct the active backend has no target for, such as an image under Typst, drops at render with a `backend::declined_construct` warning; see each backend's documentation.

## Structured data: card-yaml blocks

Quillmark carries structured data in [card-yaml blocks](card-yaml.md),
each followed by its Markdown prose body. The full block-detection rules:
fence syntax, the blank-line rule, and the backtick escape hatch for literal
code blocks: are in
[§4 of the spec](../reference/markdown-spec.md#4-block-detection).
