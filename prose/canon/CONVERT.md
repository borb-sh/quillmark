# Markdown to Typst Conversion

> **Implementation**: `crates/backends/typst/src/`

## TL;DR

The Typst backend turns markdown body content into Typst markup via
`mark_to_typst`. It parses with `pulldown_cmark` (CommonMark + strikethrough +
pipe tables), post-processes the event stream to allowlist `<u>` and strip
other raw HTML, then emits Typst markup with all reserved characters escaped.
The normative rules for *which* markdown is accepted live in
[markdown-spec.md §6](../references/markdown-spec.md); this page documents how
the backend renders it.

## Pipeline

```
mark_to_typst(markdown) -> Result<String, ConversionError>
  ├─ pulldown_cmark::Parser   (ENABLE_STRIKETHROUGH, ENABLE_TABLES)
  ├─ MarkdownFixer            event post-processor (see below)
  └─ push_typst               stateful event → Typst markup
```

`MarkdownFixer` does two things before events reach the emitter:

1. **Raw HTML.** `<u>…</u>` (case-insensitive) is rewritten to strong-emphasis
   events tagged as underline; every other `Event::Html`/`Event::InlineHtml`
   is dropped. This realises the spec's one HTML deviation (§6.2): HTML
   produces no output except underline.
2. **`***` adjacency.** Fixes delimiter runs where pulldown splits combined
   bold/italic markers awkwardly, so `***x***` and friends nest cleanly.

`push_typst` carries small pieces of state — line-start tracking, list stack,
list-item block position, code-block buffering, table alignments, image
suppression, and a nesting-depth counter that errors with
`ConversionError::NestingTooDeep` past `MAX_NESTING_DEPTH`.

## Escape functions

Two escapers guard the two Typst contexts:

- **`escape_markup`** — text in markup context. Escapes (backslash first)
  `\ // ~ * _ ` `` ` `` ` # [ ] { } $ < > @`. Applied to all body text and
  link display text.
- **`escape_string`** — text inside a Typst string literal. Escapes
  `\ " \n \r \t` and other control characters as `\u{…}`. Applied to `#link`
  and `#image` URLs.

## Element mapping

| Markdown | Typst |
|---|---|
| `#` … `######` | `=` … `======` |
| `**bold**`, `__bold__` | `#strong[…]` |
| `*italic*`, `_italic_` | `#emph[…]` |
| `~~strike~~` | `#strike[…]` |
| `<u>text</u>` | `#underline[…]` |
| `` `code` `` | backtick-delimited raw (delimiter widened past any inner run) |
| fenced / indented code block | ```` ```lang … ``` ```` raw block (fence widened past any inner backtick run) |
| `[text](url)`, autolinks | `#link("url")[text]` (link title dropped) |
| `![alt](src)` | `#image("src")` (alt text dropped) |
| `-`, `*`, `+` bullet | `- ` |
| ordered list | `+ ` auto-numbered; first item emits `N. ` when the list starts at `N ≠ 1` |
| hard break | `#linebreak()` |
| soft break | space |
| GFM pipe table | `#table(columns: N, align: (…), table.header(…), …)` |

Table alignment maps `none→auto`, `:---→left`, `:---:→center`, `---:→right`;
the `align:` argument is emitted only when at least one column is non-default.

Not rendered (parsed but produce no output, per spec §6.3): raw HTML other than
`<u>`, HTML comments, `<br>`, math, footnotes, task lists, definition lists.
Block quotes are not wrapped — their text flows through inline.

## Gotchas

- **Backslash first.** `escape_markup` replaces `\` before any other character,
  or later escapes would be double-escaped.
- **Backtick fences are content-sized.** Both inline code and code blocks size
  their backtick delimiter to `longest_run_in_content + 1` (min 3 for blocks),
  so content containing ```` ``` ```` cannot close the raw span early. Block
  content is buffered until `TagEnd::CodeBlock` so the full run is known before
  the fence is written.
- **Underline vs bold.** `MarkdownFixer` emits `<u>` as strong-emphasis events;
  the emitter distinguishes underline from real `**`/`__` by peeking the source
  range at the tag's start.
- **Ordered-list start.** Typst's `+` marker always restarts at 1. CommonMark
  start numbers are preserved by writing the explicit number on the first item
  (`5. …`); Typst then continues the following `+` items from there.
- **List markers.** Markdown bullets become Typst `-`; ordered lists become
  Typst `+` (its enumeration marker), not `-`.

## Integration

`mark_to_typst` backs the `Content` filter (`crates/backends/typst/src/`),
which embeds rendered body markup into Typst templates. Markup is wrapped for
`eval(…, mode: "markup")`, so the output passes through `escape_string` on the
way into the template.
