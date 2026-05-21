# LLM Authoring

> **Implementation**: `crates/core/src/document/`

## TL;DR

The spec choices in [MARKDOWN.md](MARKDOWN.md) collide with common LLM
training priors in predictable ways. This page enumerates the collisions
and their fixes. For generation scaffolding (pre-filled documents with
typed hints), see [BLUEPRINT.md](BLUEPRINT.md); this page is for
hand-authoring and free-form generation.

## Hard rules

- **Fences are `~~~`, not `` ``` ``.** Three tildes. Opener
  `~~~card-yaml`, closer bare `~~~`. A backtick fence is inert — the
  block becomes an ordinary CommonMark code block with no error.
- **Blank line above every opener** (except line 1). Without it the
  fence is demoted silently to a code block inside the preceding
  paragraph.
- **Root block declares both `$quill: <name>@<version>` and
  `$kind: main`.** Composable cards declare `$kind: <other>` and must
  not declare `$quill`.
- **`$` keys are closed at `{quill, kind, id}`.** Anything else is a
  hard parse error.
- **Data field names are `/^[a-z_][a-z0-9_]*$/`.** No camelCase,
  kebab-case, capitals, or `$`. `QUILL`, `CARD`, `BODY`, `CARDS` are
  reserved.

## Prior-vs-spec

| LLM prior | Spec (MARKDOWN.md §) | Fix |
|---|---|---|
| `` ```card-yaml ``, `` ```yaml `` | `~~~card-yaml` (§3.2) | Three tildes, exact info string. |
| `~~~~` adjacent to `~~strike~~` | Fence is `~~~` exactly (§3.2) | Mind the fencepost; never extend to four. |
| `firstName`, `to-address`, `From` | snake_case (§3.4) | `first_name`, `to_address`, `from`. |
| `$type`, `$schema`, `$version`, `$ref` | Closed `$` set (§3.3) | Use a regular field, or `$kind` / `$id`. |
| `$quill: resume` | `<name>@<version>` (§3.5) | Add a selector (`@1`, `@1.2`, `@latest`, or exact). |
| `<b>`, `<i>`, `<em>`, `<br>` | All HTML discarded except `<u>` (§6.2) | `**bold**`, `*italic*`, two-space hard break. |
| `$x$` math, `![img](…)`, `- [ ]`, footnotes, def lists | Out of scope (§6.3) | Drop, or fold into prose. `$` is literal in body. |
| `---` between cards | Cards are fence-pair-delimited (§3.1) | Open and close each card with `~~~card-yaml … ~~~`. |
| `!include`, `!env`, other YAML tags | Only `!fill` (§3.4) | Inline the value; reserve `!fill` for placeholders. |
| Paragraph immediately followed by `~~~card-yaml` | Blank line required (§3.2) | Insert the blank line. |

## Preferred path: JSON in, canonical Markdown out

§9.1 guarantees `toMarkdown(fromJson(toJson(x)))` is canonical. Generating
against the [SCHEMAS.md](SCHEMAS.md) JSON surface and emitting markdown
deterministically sidesteps every row above. Reach for the markdown
surface only when the round-trip target is a human-edited file.
