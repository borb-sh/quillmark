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
| HTML tables | `<table>`, `<tr>`, `<th>`, `<td>` | Column widths, placement, cell alignment and merged cells. See [Tables](#tables). |
| Underline | `<u>text</u>` | An allow-listed raw-HTML tag (see [the deviation below](#raw-html-is-not-rendered-except-u-and-br)). |
| Task lists | `- [ ] open`, `- [x] done` | A checkbox before the item's text, ticked for `[x]`. |
| Footnotes | `text[^1]` and a `[^1]: note` definition | The note at the foot of the page, numbered. See [Footnotes](#footnotes). |
| Math | `$x^2$` inline, `$$\frac{a}{b}$$` on its own line | TeX math. See [Math](#math). |

Autolinks beyond CommonMark's and other GFM features are **not** enabled.

## Footnotes

A reference `[^label]` in prose and one definition `[^label]: note` anywhere in the body make a footnote, the note drawn at the foot of the page. Labels match case-insensitively, and the number drawn is the footnote's place in the document, not its label.

```markdown
The figure comes from the 2024 survey.[^src]

[^src]: Office of Budget, *Annual Survey*, 2024.
```

A note is inline text. A paragraph, list item or other block inside a definition becomes a line of the note and warns under `parse::dropped_construct` (`footnote[list]`, `footnote[quote]`, …). A definition nothing references, a second definition of one label, and a reference inside a table cell, an image's alt text or another note drop, warning `footnote`. A reference with no definition stays as the text written.

## Math

`$…$` sets an equation inside a line and `$$…$$` one on its own line, both written in TeX:

```markdown
The area is $\pi r^2$, and

$$\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$$
```

The opening `$` takes no space after it and the closing `$` none before it, and braces inside balance, so `$5 and $10` stays text. Write `\$` for a dollar sign that should never open an equation. A `%` comments out the rest of its line, as in TeX. An equation in a table cell, an image's alt text or a footnote stays as its source text and warns under `parse::dropped_construct` (`math`).

The Typst backend draws:

| Kind | TeX |
|---|---|
| Scripts and primes | `x^2`, `x_i`, `x_i^2`, `f'`, `{}^{14}C` |
| Fractions, binomials, roots | `\frac`, `\dfrac`, `\tfrac`, `\binom`, `\sqrt{x}`, `\sqrt[n]{x}` |
| Greek | `\alpha` … `\omega`, `\Gamma` … `\Omega`, the `\var` forms |
| Operators and relations | `\pm`, `\times`, `\cdot`, `\leq`, `\neq`, `\approx`, `\equiv`, `\in`, `\subseteq`, `\to`, `\Rightarrow`, `\iff`, … and `\not` before a relation |
| Large operators | `\sum`, `\prod`, `\int`, `\iint`, `\oint`, `\bigcup`, `\bigcap`, …, with `\limits` and `\nolimits` |
| Function names | `\sin`, `\cos`, `\log`, `\exp`, `\lim`, `\max`, `\min`, `\det`, … and `\operatorname{name}` |
| Accents | `\hat`, `\bar`, `\vec`, `\dot`, `\ddot`, `\tilde`, `\check`, `\breve`, `\overline`, `\underline`, `\overbrace`, `\underbrace` |
| Fonts | `\mathbf`, `\boldsymbol`, `\mathrm`, `\mathit`, `\mathbb`, `\mathcal`, `\mathscr`, `\mathfrak`, `\mathsf`, `\mathtt` |
| Text | `\text{…}`, `\textbf`, `\textit`, `\mbox` |
| Delimiters | `\left( … \right)`, `\middle\|`, `\big(` and its sizes, `\langle`, `\lfloor`, `\lceil`, `\{`, `\\|` |
| Spacing | `\,`, `\:`, `\;`, `\!`, `\quad`, `\qquad`, `~` |
| Environments | `matrix`, `pmatrix`, `bmatrix`, `Bmatrix`, `vmatrix`, `Vmatrix`, `cases`, `aligned`, `gathered`, `array` |
| Other | `\dots`, `\cdots`, `\infty`, `\partial`, `\nabla`, `\forall`, `\exists`, `\emptyset`, `\pmod`, `\bmod`, `\displaystyle` |

A command outside these draws as its own name and warns `typst::unsupported_tex`, naming it.

## Tables

A table is a GFM pipe table or an HTML table, and Quillmark reads both. A pipe table holds one header row, its cells and each column's text alignment, set in the delimiter row (`:---:`). Anything else a table holds is spelled in HTML:

```markdown
<table widths="1 2 2" aligns="left left right" align="center">
<thead>
<tr><th colspan="3">FLIGHT X PERSONNEL</th></tr>
<tr><th>Date</th><th colspan="2">POC</th></tr>
</thead>
<tbody>
<tr><td rowspan="2" valign="middle">12 Nov</td><td>SSgt Doe</td><td>555-0101</td></tr>
<tr><td>SrA Roe</td><td>555-0102</td></tr>
</tbody>
</table>
```

On `<table>`:

| Attribute | Value | Without it |
|---|---|---|
| `widths` | One weight per column: a positive whole number, or `auto` to fit the column to its content. Weights are relative, so `2 1` makes the first column twice as wide as the second. | Every column fits its content. |
| `aligns` | One text alignment per column: `none`, `left`, `center` or `right`, as a pipe table's delimiter row sets it. | Every column `none`. |
| `align` | Where the table sits: `left`, `center` or `right`. | The quill's placement. |

On `<td>` and `<th>`:

| Attribute | Value | Without it |
|---|---|---|
| `colspan` | The columns the cell spans, a whole number. | One column. |
| `rowspan` | The rows the cell spans, a whole number. | One row. |
| `align` | `left`, `center` or `right`. | The column's alignment. |
| `valign` | `top`, `middle` or `bottom`. | The quill's vertical alignment. |

- The rows in `<thead>` are the header rows, as many as the table needs, and a header cell can span rows within them. A table with no `<thead>` takes its opening rows of `<th>` cells as its header, in a `<tbody>` or not, and with none of those has no header row: its first row draws as an ordinary row, without the quill's header styling.
- A cell that a span covers is left out of its row, as HTML leaves it out. In the example, the second body row holds two cells, since `12 Nov` covers its first slot.
- A cell holds inline markdown, on one line: `<td>**Total**</td>` is bold, and `<br>` is its line break. GitHub draws the grid and the spans, but shows the markdown in a cell as written.
- A table is one HTML block, so a blank line inside it ends the table there; keep a table's lines together. Text right after `</table>` reads as ordinary markdown.
- A table holds at most 100 000 cells, rows times columns; the rows past that drop, warning `table:size`.
- `widths` or `aligns` with fewer entries than the table has columns leaves the rest `auto` or `none`, and one with more ignores the extra entries.
- Quillmark reads what HTML leaves out: unquoted values, a missing `<tbody>`, and missing `</td>`, `</th>` and `</tr>` tags. `<tfoot>` rows read as body rows at the end.
- A row too short for the table, which is as wide as its widest row, fills with empty cells. Any other tag, an attribute not on this page and text outside every cell, such as a `<caption>`'s, drop, and each warns `parse::dropped_construct`, naming what dropped (`td[nowrap]`, `table:row`). `border`, `class`, `scope` and `style` drop without a warning.

Export writes a table as pipes when pipes hold all of it, and as HTML otherwise, whichever spelling the document used: a table that gains a merged cell writes as HTML, and one that loses its last merge, width or header row change writes as pipes again.

## `qm-*` tags

Markdown has no syntax for a run of text's font. Quillmark spells it with a `qm-span` tag pair: an HTML custom element, which a browser or GitHub draws as just what it wraps.

A `qm-*` tag inside a line of text is read only as `qm-span`. A line holding only `qm-*` tags reads as a tag line wherever it stands, tight against a paragraph or a table included. No tag line wraps anything: its tags drop and the blocks around them stay. Any other `qm-<name>` tag, such as `<qm-keep>`, carries nothing: the open tag drops with a `parse::dropped_construct` warning naming it, the close tag drops silently, and what lies between imports as written. A tag line still ends the paragraph above it, as any tag line does.

### Font family

A `qm-span` pair inside a line sets the font family of the text it wraps:

```markdown
The <qm-span family="Carlito">phrase sits in **bold <qm-span family="Arimo">sans</qm-span>**</qm-span> text.
```

| Attribute | Value | Without it |
|---|---|---|
| `family` | A font family the app's font library supplies, such as `Carlito`. Case does not matter. | The surrounding family. |

- It pairs as `<u>` does: inside a paragraph, heading, list item, table cell or footnote, across `**` or `~~` freely. Pairs nest: an inner pair's family sets its own text, and the outer family resumes past it. Saved and exported again, nested pairs come back side by side, one per run.
- A family the app's library lacks keeps the surrounding font, and the render warns `typst::unknown_font_family` once per family and field. A quill's own fonts set its design and are not a choice here. An app lists the faces a writer can pick with `engine.fontFaces(quill)`.
- Code spans and equations keep their own font, and a footnote's note keeps its own: a pair inside the note sets it.
- A family is a run's, never a block's: `qm-span` tag lines around blocks drop with a `qm-span` warning, and the blocks stay.
- An attribute other than `family` drops alone with a `parse::dropped_construct` warning, keeping the family. A tag setting no family or left open drops, keeping what it wraps.

## Deviations from CommonMark

### Raw HTML is not rendered, except `<u>` and `<br>`

CommonMark passes raw HTML through to the output. Quillmark recognises raw HTML as CommonMark does (so it does not break paragraph structure) but **discards every tag**, except the ones it supports: `<u>…</u>` renders as underline, an inline `<br>` is a line break, a `<table>` is a [table](#tables), and a `qm-span` pair sets [a font run](#font-family).

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

- `<br>`, `<br/>`, `<br />` (any case) are a line break, in a paragraph or a table cell: `| line one<br>line two |`. Outside a table, a CommonMark hard break does the same: two trailing spaces before a newline, or a trailing `\` before a newline. In a paragraph, a `<br>` at the paragraph's start or right after another line break produces nothing, and a newline beside a line break adds no space: `line one<br>` on one line and `line two` on the next are two lines, the second unindented. A `<br>` alone on a line opens an HTML block, below.
- HTML entities decode as CommonMark specifies: `Fish &amp; chips, &#65;BC` reads `Fish & chips, ABC`.
- An HTML block drops as CommonMark reads it, unless it opens `<table>`. A line starting with a tag such as `<div>`, `<center>` or `<details>` opens a block that runs to the next blank line, so markdown on the lines under it drops with it; a blank line after the tag line keeps what follows. Embedded SVG draws nothing.
- A `<pre>`, `<script>`, `<style>` or `<textarea>` block drops whole, content included, through the end of its closing tag's line.
- HTML comments do not appear in output. Text after a comment's `-->` on the same line still does.
- Each dropped tag is reported as a `parse::dropped_construct` warning naming it in lowercase and counting its opening tags, a `<pre>` block's included. Comments are not reported, and neither are closing tags and `<qm-anchor>` tags, except where the block they open drops markdown. A tag in a `richtext` field's value in the card-yaml block drops the same way, reported at the field.

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

A link's title (`[text](url "Title")`) drops at import, with no warning. A construct the active backend has no target for, such as an image under Typst, drops at render with a `backend::declined_construct` warning; see each backend's documentation.

## Structured data: card-yaml blocks

Quillmark carries structured data in [card-yaml blocks](card-yaml.md),
each followed by its Markdown prose body. The full block-detection rules:
fence syntax, the blank-line rule, and the backtick escape hatch for literal
code blocks: are in
[§4 of the spec](../reference/markdown-spec.md#4-block-detection).
