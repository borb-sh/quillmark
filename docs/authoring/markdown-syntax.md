# Markdown Syntax

Quillmark Markdown is a **strict superset of [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)** with a small set of [GitHub Flavored Markdown](https://github.github.com/gfm/) extensions and **two declared deviations**: [raw HTML](#raw-html-is-not-rendered-except-u-and-br) and [`~~~` fences](#a-column-zero-always-opens-a-card-yaml-block). If you already know CommonMark, you only need to learn what is on this page.

For the authoritative grammar, block-detection rules, normalization, and limits, see the formal [Markdown specification](../reference/markdown-spec.md).

## Foundation

Body content (the prose after each [card-yaml block](card-yaml.md), including any [card](card-yaml.md#card-blocks)) is parsed as CommonMark 0.31.2. Headings, emphasis, links, images, lists, code blocks, blockquotes, thematic breaks, and inline code all behave exactly as the [CommonMark spec](https://spec.commonmark.org/0.31.2/) defines them. (An image parses, stores and round-trips, but **no backend typesets one**: the Typst backend drops it and warns under `backend::declined_construct`, because what a content image's `src` names is undecided.)

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

Task lists, autolinks beyond CommonMark's, and other GFM features are **not** enabled.

## Deviations from CommonMark

### Raw HTML is not rendered, except `<u>` and `<br>`

CommonMark passes raw HTML through to the output. Quillmark recognises raw HTML as CommonMark does (so it does not break paragraph structure) but **discards every tag**, except the ones it supports: `<u>…</u>` renders as underline, an inline `<br>` is a line break, and a `quill-*` pair is an element.

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
- A `<pre>`, `<script>`, `<style>` or `<textarea>` block drops whole, content included.
- HTML comments do not appear in output. Text after a comment's `-->` on the same line still does.
- A `quill-<name>` pair of tag lines is kept as an element around the blocks between them: `<quill-keep>` above a signature and `</quill-keep>` below it, each with a blank line on either side. With no blank line between a tag and the markdown beside it, the tag opens an HTML block like any other and drops it, as does a pair inside a line. An empty pair, `<quill-sig>` on the line above `</quill-sig>`, is an element that holds nothing. A quill whose plate registers a renderer for the element draws it that way, and any other renders what it holds. One left unclosed drops like any other tag.
- Each dropped tag is reported as a `parse::dropped_construct` warning naming it in lowercase and counting its opening tags, a `<pre>` block's included; comments, `<quill-anchor>` tags and a `quill-*` element that closes are not.

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
- **Task lists**, **definition lists**: they render as the literal text written.
- **Footnotes**: CommonMark reads `[^1]: Note` as a link reference definition, which drops the line and turns every `[^1]` into a link to `Note`. Quillmark reads it the same way and reports the definition as a `parse::dropped_construct` warning.

A link's title (`[text](url "Title")`) drops at import, with no warning. A construct the active backend has no target for, such as an image under Typst, drops at render with a `backend::declined_construct` warning; see each backend's documentation.

## Structured data: card-yaml blocks

Quillmark carries structured data in [card-yaml blocks](card-yaml.md),
each followed by its Markdown prose body. The full block-detection rules:
fence syntax, the blank-line rule, and the backtick escape hatch for literal
code blocks: are in
[§4 of the spec](../reference/markdown-spec.md#4-block-detection).
