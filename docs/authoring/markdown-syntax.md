# Markdown Syntax

Quillmark Markdown is a **strict superset of [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)** with a small set of [GitHub Flavored Markdown](https://github.github.com/gfm/) extensions and **three declared deviations**: [raw HTML](#raw-html-is-not-rendered-except-u-and-br), [footnote-shaped definitions](#a-footnote-shaped-definition-is-text) and [`~~~` fences](#a-column-zero-always-opens-a-card-yaml-block). If you already know CommonMark, you only need to learn what is on this page.

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

CommonMark passes raw HTML through to the output. Quillmark recognises raw HTML syntactically (so it does not break paragraph structure) but **discards every tag**, with two exceptions: `<u>…</u>` renders as underline, and an inline `<br>` is a line break.

```markdown
<u>This is underlined</u>, even <u>across word boundaries</u>.
<span style="color: red">The span's tags drop; its text stays.</span>
<!-- HTML comments are also dropped -->

<div align="center">
| The div's tags drop; | its table stays |
|---|---|
</div>
```

Why: Typst (the rendering backend) has no HTML renderer, and arbitrary HTML passthrough would create injection risks for downstream tooling. `<u>` is allowed because no CommonMark-native syntax covers arbitrary-range underline, and `<br>` because a table row is one line, with no room for a CommonMark hard break.

Consequences:

- `<br>`, `<br/>`, `<br />` (any case) are a line break, in a paragraph or a table cell: `| line one<br>line two |`. Outside a table, a CommonMark hard break does the same: two trailing spaces before a newline, or a trailing `\` before a newline. In a paragraph, a `<br>` with no text before it on its line produces nothing, as does a `<br>` alone on a line.
- HTML entities decode as CommonMark specifies: `Fish &amp; chips, &#65;BC` reads `Fish & chips, ABC`.
- A tag on a line of its own wraps markdown rather than hiding it: `<div>`, `<center>`, `<details>` or any other tag line drops, and the lines between parse as markdown whether or not blank lines surround the tags. Embedded SVG draws nothing: its tags drop like any other, and text it holds reads as text.
- A `<pre>`, `<script>`, `<style>` or `<textarea>` block drops whole, content included.
- HTML comments do not appear in output. Text after a comment's `-->` on the same line still does.
- Each dropped tag is reported as a `parse::dropped_construct` warning naming it, a `<pre>` block's included; comments are not.

### A footnote-shaped definition is text

CommonMark reads `[^1]: Note` as a link reference definition, which turns every
`[^1]` into a link to `Note`. Quillmark keeps both as the text you typed, and
reports the definition as a `parse::dropped_construct` warning, since it
supports no footnotes.

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

The following are recognised by the parser (so they will not corrupt surrounding content) but produce no output:

- **Math** (`$…$`, `$$…$$`): `$` is treated as a literal character.
- **Footnotes**: not supported; see [above](#a-footnote-shaped-definition-is-text).
- **Task lists**, **definition lists**: not supported.

Some constructs (like link titles) are accepted by the parser but may be dropped during rendering when the active backend has no target for them. Those losses are backend-specific; see each backend's documentation.

## Structured data: card-yaml blocks

Quillmark carries structured data in [card-yaml blocks](card-yaml.md),
each followed by its Markdown prose body. The full block-detection rules:
fence syntax, the blank-line rule, and the backtick escape hatch for literal
code blocks: are in
[§4 of the spec](../reference/markdown-spec.md#4-block-detection).
