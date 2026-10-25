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
| Footnotes | `text[^1]` and a `[^1]: note` definition | The note at the foot of the page, numbered. See [Footnotes](#footnotes). |
| Math | `$x^2$` inline, `$$\frac{a}{b}$$` on its own line | TeX math. See [Math](#math). |
| Definition lists | `Term` and `: description` on the line under it | A term and what describes it. See [Definition lists](#definition-lists). |

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

## Definition lists

A paragraph with a line opening `: ` under it is a term and its description:

```markdown
Plate
: The Typst file a quill renders through.

Card
: A block of structured data.
: Its body, the prose after it.
```

A term takes several descriptions, one `: ` line each. A description holding more than one block indents the rest two spaces under its `: `. A line of prose that should open with a colon writes it `\:`.

## Table layout and elements: `qm-*` tags

Markdown has no syntax for column widths, table placement, a run of text's font or keeping a block on one page. Quillmark spells them with `qm-*` tags: HTML custom elements, which a browser or GitHub draws as just what they wrap.

Each `qm-*` tag stands on a line of its own, or beside other open and close `qm-*` tags with no text between. Quillmark reads such a line wherever it stands, tight against a paragraph or a table included, and export writes a blank line above and below it, which GitHub and other CommonMark viewers need to render the markdown it wraps:

```markdown
<qm-keep>

**Signed**
J. Doe

</qm-keep>
```

A tag inside a line of text, in a heading, emphasis or a link, or left open drops, keeping what it wraps, with a `parse::dropped_construct` warning whose hint names the fix. A self-closing tag (`<qm-keep/>`) is an open tag, as HTML reads it, so it drops too. A tag line directly under a quote's or list item's text continues it, as CommonMark reads it, so its pair belongs in that quote or item.

### Table layout

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
| `headless` | No value: `<qm-table headless>`. The first row draws as an ordinary row, without the quill's header styling. | The first row is the header. |

Text alignment within a column stays in the delimiter row (`:---:`). A `widths` with fewer entries than the table has columns leaves the rest `auto`, and one with more ignores the extra entries. A `qm-table` around anything but one table drops, keeping what it holds.

### Cell alignment

A `qm-cell` pair around a cell's whole content aligns that one cell, in the header row or the body:

```markdown
| Item | Notes | Qty |
| --- | --- | ---: |
| <qm-cell valign="bottom">Total</qm-cell> | one<br>two | <qm-cell align="center">**42**</qm-cell> |
```

| Attribute | Value | Without it |
|---|---|---|
| `align` | `left`, `center` or `right`. | The column's alignment from the delimiter row. |
| `valign` | `top`, `middle` or `bottom`. | The quill's vertical alignment. |

The open tag comes first in the cell and the close tag last. A pair with anything else in the cell beside it drops, keeping what it wraps, with a `parse::dropped_construct` warning.

### Font family

A `qm-span` pair inside a line sets the font family of the text it wraps:

```markdown
The <qm-span family="Carlito">phrase sits in **bold <qm-span family="Arimo">sans</qm-span>**</qm-span> text.
```

| Attribute | Value | Without it |
|---|---|---|
| `family` | A font family the app's font library supplies, such as `Carlito`. Case does not matter. | The surrounding family. |

- It pairs as `<u>` does: inside a paragraph, heading, list item, table cell or footnote, across `**` or `~~` freely. Pairs nest, and the innermost pair's family sets its text, so the outer family resumes past it.
- A family the app's library lacks keeps the surrounding font, and the render warns `typst::unknown_font_family` once per family and field. A quill's own fonts set its design and are not a choice here. An app lists the faces a writer can pick with `engine.fontFaces(quill)`.
- Code spans and equations keep their own font, and a footnote's note keeps its own: a pair inside the note sets it.
- In a table cell, a `qm-cell` pair goes outside any `qm-span` pair.
- A family is a run's, never a block's: `qm-span` tag lines around blocks drop with a `qm-span` warning, and the blocks stay.
- An attribute other than `family` drops alone with a `parse::dropped_construct` warning, keeping the family. A tag setting no family or left open drops, keeping what it wraps.

### Elements

Any other `qm-<name>` pair is an element around the blocks between its tags, and the quill decides how to draw it. A name is lowercase letters and digits in words joined by single hyphens, opening with a letter, such as `qm-sig` or `qm-stamp-2`; `qm-anchor` is reserved. `qm-keep` is built into every Typst quill and keeps what it wraps on one page; around a `qm-table` it keeps the table from splitting. A quill with no renderer for an element draws what it wraps as if the tags were absent, and the render warns `typst::unregistered_element`. The warning's hint names the elements the quill renders, so a misspelled `qm-kep` shows up there.

An element around nothing is its two tags on one line, which suits a signature line or a stamp the quill draws:

```markdown
<qm-sig></qm-sig>
```

An element's attributes are strings the quill reads, such as `<qm-stamp tone="urgent">`. An attribute name is lowercase letters, digits and `_`, opening with a letter. `style`, `class`, `id`, `href`, `src` and names opening `on` are refused, so the tags never carry markup a browser would act on.

## Deviations from CommonMark

### Raw HTML is not rendered, except `<u>` and `<br>`

CommonMark passes raw HTML through to the output. Quillmark recognises raw HTML as CommonMark does (so it does not break paragraph structure) but **discards every tag**, except the ones it supports: `<u>…</u>` renders as underline, an inline `<br>` is a line break, and a `qm-*` tag spells [table layout, a font run or an element](#table-layout-and-elements-qm-tags).

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

A link's title (`[text](url "Title")`) drops at import, with no warning. A construct the active backend has no target for, such as an image under Typst, drops at render with a `backend::declined_construct` warning; see each backend's documentation.

## Structured data: card-yaml blocks

Quillmark carries structured data in [card-yaml blocks](card-yaml.md),
each followed by its Markdown prose body. The full block-detection rules:
fence syntax, the blank-line rule, and the backtick escape hatch for literal
code blocks: are in
[§4 of the spec](../reference/markdown-spec.md#4-block-detection).
