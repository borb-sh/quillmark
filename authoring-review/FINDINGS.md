# Authoring review: a quill written from `docs/` alone

`meeting_minutes/` was written using only `docs/`, then rendered with a
locally built `quillmark` CLI treated as a black box. It uses cards with
bodies, an `enum` with `variants:`, typed tables with `ui.layout: table`, `date`
and `datetime` fields, inline `richtext`/`plaintext`, `field-region`,
`display` and `signature-field`. It passes `quillmark validate` and renders
`example.md` to two pages.

The happy path was short. The Quill.yaml reference and the Typst backend page
were enough to get a validated quill on the first try. The findings below are
about what happened off the happy path, and about what an open-source
ecosystem of quills needs that the engine does not yet give it.

Each finding carries the probe that showed it. Findings marked *docs* come from
reading the docs, not from running anything.

## Severity key

- **P0**: silently produces a wrong document, or blocks an ecosystem use case outright.
- **P1**: a new author hits it in their first hour, and it costs them.
- **P2**: friction or inconsistency.

---

## Schema layer

### P0: Undeclared document keys are accepted silently, so a typo drops data

| Probe | Result |
|---|---|
| `secretery: Grace Hopper` in the root block | Renders with exit code 0 and no warning. The "Recording" row disappears. |
| `presentr: Alan Turing` in a card | Renders with exit code 0 and no warning. "Presented by" disappears. |
| Variant cells written flat (`outcome: motion` with `moved_by:` beside it, not nested under `outcome`) | Renders with exit code 0 and no warning. The motion block prints "Moved by ." and "Vote: 0 for, 0 against, 0 abstaining." |

The typed writer refuses the same mistake (`edit::unknown_field`,
integration/programmatic.md). Markdown is the main authoring surface, and it is
the one that doesn't. The docs spell out that an undeclared key parses
(card-yaml.md, "uppercase or underscore-led key parses but is always
undeclared"). They never say it goes unreported.

The variant case is the worst of the three. The flat spelling is what anyone
coming from a flat YAML schema writes first. Also, `outcome: motion` alone is
valid shorthand ("a world with nothing to fill in still writes plainly"), which
makes the mistake easier to fall into.

**Direction:** make an undeclared key under a known kind a `validation::*`
warning at minimum, anchored at its path. Do this on both the render path and
`Quill::validate`. If a quill wants open fields, it can opt in.

### P0: The CLI has no way to check a document

`quillmark validate` validates a quill. There is no command that checks a
document, and `render` does not print the `validation::must_fill` warnings.
Rendering the seed, the empty document, and the flat-variant document above
all printed no warnings, even though each leaves obliged cells unanswered.

So the obligation model — "a strict consumer treats any outstanding marker as
not done" — has no strict consumer in the reference CLI. A CI job for a
document repository has nothing to gate on.

**Direction:** `quillmark check <quill> <doc>`, or `render --strict`, which
prints every `Quill::validate` diagnostic and exits non-zero on
`must_fill`/unknown-field warnings.

### P1: A blank number can't be told apart from zero

The blank of `integer`/`number` is `0` and the blank of `boolean` is `false`
(typst-backend.md, "Blank values"). A plate therefore can't distinguish "0
votes against" from "nobody entered the tally", or "not confidential" from
"unanswered".

In this quill, `quorum` needs a sentinel ("0 omits the quorum line"), and the
vote tally can't flag an unfilled motion. Dates escape this problem because
their blank is `none`. Scalars need the same treatment, or an opt-in
`nullable`.

The blank-fill design ("partial documents are always renderable") is the right
call for a live editor. For the batch pipeline — "database row → PDF",
invoices, anything with money — it produces documents that look complete but
aren't.

### P1: Documents born from the blueprint break on the quill's next patch release

The blueprint emits `$quill: meeting_minutes@1.0.0 # keep verbatim`. A document
started from it fails with `quill::version_mismatch` when the quill ships
`1.0.1`. That was confirmed by bumping the version and rendering the blueprint
output.

The versioning page calls PATCH "fixes … without shape changes", yet the
generated documents reject every patch release. Every LLM- or MCP-authored
document starts from the blueprint, so every one is born over-pinned.

**Direction:** emit `@MAJOR` (or `@MAJOR.MINOR`), which is the selector that
matches the semver promise.

### P1: The blueprint hides the shape of a typed table's rows

`attendees: [] # array<object>` and `action_items: [] # array<object>` are all
the blueprint shows. The row properties (`name`, `role`, `voting`, `present` /
`owner`, `task`, `due`) and their types appear nowhere in it.

The blueprint is billed as "the authoring surface for LLM and MCP consumers".
Yet an LLM given only the blueprint can't fill this quill's two most important
fields without also being given `schema()`.

By contrast, variant cells are shown (as `# when motion:` blocks), so the
machinery exists.

**Direction:** for an empty array of objects, emit one commented sample row
built from `items.properties`, the way dormant variant worlds are emitted.

### P1: Unquoted `version: 1.10` loads as 1.1

`quillmark info` reports `Version: 1.1`, and a document pinned `@1.10` then
mismatches. The docs warn about this (versioning.md). The engine could simply
refuse a non-string `version`, and a warning in the docs does not replace that
refusal.

### P2: One schema carries three audiences

A field's declaration serves three audiences at once:

- validation (`type`, `values`, `max`)
- the editor (`ui.*`, `plaintext` vs `string`, `layout`, `blank_title`,
  `body.unsupported`)
- the LLM blueprint (`example`, `description`)

The type system itself carries an editor concern: `string` and
`plaintext inline: true` differ only in editor navigation and regions, with the
same output. A new author has to answer "will this be edited in a live preview?"
before they can pick a type for a person's name.

The reference that teaches all this is 43 KB — the longest page in `docs/`
outside the migrations. It spends paragraphs on web-forms and protobuf priors,
matrix semantics and variant collision rules before it ever shows an
end-to-end example.

**Direction:** keep the data vocabulary small (`string`, `text` with a
`format: plain|markdown`, `enum`, numbers, dates, containers). Move the
editor-only distinctions under `ui:`, and teach from a data-only core outward.

### P2: No way to share or reuse a schema

Nothing in the docs lets you share field sets across card kinds or across
quills, extend a base quill, or import a common letterhead/signature block. The
only mechanism mentioned is a YAML anchor inside one file. An ecosystem of
quills ("our org's memo, letter, and minutes") will copy-paste its common core,
and the copies will drift.

### P2: Kindless cards: the docs contradict the engine

typst-backend.md says "A card block with no `$kind:` line is a *kindless* card:
it reaches the plate carrying its authored fields verbatim". card-yaml.md says
every card must declare a kind. The engine refuses one with
``unknown card kind `` `` (empty backticks). An unknown `$kind` is a hard
error too, which makes the documented `card.at("$kind", default: none)`
defensive read dead code.

---

## Markdown layer

### P0: `~~~` is repurposed, which breaks the CommonMark promise

markdown-syntax.md opens with "a **strict superset of CommonMark 0.31.2**".
Then every column-zero `~~~` block is a card, including `~~~python`.

Appending a routine tilde code fence to `example.md` failed the whole render
with `Invalid YAML in card block: expected a mapping (parse::invalid_structure)`.
That error has no line number and no hint that the tilde rule is the cause.
Tilde fences are common in pasted READMEs, in Pandoc output, and in LLM output.

The same syntax also makes whitespace semantic. A card opener without a blank
line above it becomes a code block, and the render continues (with a warning)
with the card's YAML typeset as code in the body.

**Direction:** at minimum, when a card payload fails to parse as a mapping, say
"this `~~~` block opened a card; use a backtick fence for code", and give its
line.

Longer term, consider an unambiguous card opener. `~~~` plus the blank-line
rule is two invisible rules, and the widely known frontmatter idiom (`---`) is
the thing new users will try first.

### P1: What a general-purpose engine doesn't typeset

Each of these parses and then vanishes:

- math (`$…$` stays literal)
- footnotes
- task lists
- images in body or `richtext` (dropped with `backend::declined_construct`)
- raw HTML other than `<u>`

Each is defensible alone. Together they rule out the documents open-source
authors most often write: technical reports, papers, READMEs with diagrams,
docs with screenshots.

Images are the sharpest case. The decline is on principle ("what a url names is
undecided"), but it leaves the Typst backend with no way to put a figure in
prose. A quill-scoped resolution (a `src` resolves against the quill's
`assets/`, or against document-attached assets carried in `$ext`) would unblock
it without deciding the global question.

### P2: The spec isn't in `docs/`

`docs/reference/markdown-spec.md` is one `--8<--` include of
`prose/references/markdown-spec.md`. `creating-quills.md` §6 ("A second plate")
is two includes of fixture files. Anyone reading `docs/` on GitHub or in a
checkout — as this exercise did — sees neither the grammar the syntax page
calls authoritative, nor the only worked plate with cards.

---

## Typst layer

### P0: Typst Universe packages don't work, though the docs advertise them

typst-backend.md "Typst Packages" shows
`packages: ["@preview/appreciated-letter:0.1.0"]` and says "Browse the full
catalog at Typst Universe". operations.md says the engine "never downloads a
Typst package".

Declaring `@preview/cetz:0.3.4` and importing it fails every canonical render
with `file not found (searched at typst.toml)`. The error doesn't name the
package or say "vendor it under `packages/`".

Vendoring a package under `packages/<dir>/` works. But the docs never say what
spec to import it by. `@local/<name>:<version>` works, while
`@preview/<name>:<version>` of the same vendored package does not. So a Universe
package's own `@preview/…` imports of its dependencies presumably won't resolve
either. What the `packages:` key in Quill.yaml does is undocumented and not
visible from outside.

Typst's value as a foundation is mostly its package ecosystem, and today no
tool reaches it. **Direction:** a `quillmark vendor <quill>` that resolves
`packages:` (with transitive dependencies) into `packages/` under their original
namespace, and a load error that names the missing package.

### P1: A plate is one file

With `lib.typ` next to `plate.typ`, `#import "lib.typ"` fails. So do `/lib.typ`,
`./lib.typ`, `src/lib.typ`, `assets/lib.typ` and `/assets/lib.typ`, all with
`typst::file_not_found`. Yet `#read("assets/x.txt")` and
`#image("assets/logo.svg")` resolve.

A plate can therefore only be modularized by writing a vendored `@local`
package with its own `typst.toml`. That is a heavy tax for "put the helpers in
another file". The docs never say plates are single-file, either.

### P1: Diagnostics name a file that doesn't exist

Every plate error and warning points at `main.typ:<line>:<col>`. The author's
file is `plate.typ`, and `Quill.yaml` names it as `plate_file`. The line numbers
match `plate.typ`, so the fix is to report the configured filename.

### P1: No fonts beyond one sans family

"A Quill bundling none renders in the embedded Figtree faces." In practice:

- `Libertinus Serif`, `New Computer Modern` and `DejaVu Sans Mono` all warn
  `unknown font family` and fall back.
- The docs' own "Typesetting" example (`font: "Linux Libertine"`) warns the same
  way.

Every quill needing a serif face — or a monospace face for code — must vendor
font files. Standalone Typst embeds all three, so authors coming from Typst
lose fonts they already had.

**Direction:** ship Typst's default font set, or make it an opt-in, and fix the
docs example.

### P1: Editor regions change how you write ordinary Typst

A read made through a function parameter, a destructuring, or a loop variable
silently loses its click-to-edit region (editor-regions.md, "Each of those still
renders correctly and loses only the click target, which is why nothing
announces it"). The fix is `field-region` with a hand-concatenated string
address: `card.at("$path") + "outcome.moved_by"`.

This quill has five such concatenations. Every one is stringly typed and checked
only at compile.

For a file-only author this is noise. For a live-preview author it punishes the
idioms that make Typst code maintainable: helper functions, packages, loops.
The loss is invisible without an editor to click in. `quillmark validate` could
report each unattributed scalar read, which would make the cost visible.

### P2: Blank-fill makes the plate defensive

The empty-document contract obliges a plate to render with every field blank.
This plate spends about 20 guards on it: `!= ""`, `!= none`, `len() > 0`,
`o.result == ""`, and a `rows` accumulator so an empty metadata table disappears.

Combined with `data.at("$body", default: "")` for every `$` key (Typst
identifiers can't contain `$`), a noticeable part of any plate is ceremony
rather than layout.

**Direction:**
- Expose `$`-keys under plain names in the helper as well (`body(data)`,
  `cards(data, kind: "agenda_item")`).
- Provide a `present(x)` / `blank(x)` helper that knows each type's blank.

### P2: No inner dev loop outside Quillmark

A plate imports the virtual `@local/quillmark-helper`. Typst's own tooling —
`typst watch`, the Tinymist LSP and its live preview — can't compile it or
complete `data.` fields. `--output-data` doesn't close the gap, because its JSON
is pre-lowering: richtext is a `{text, lines, marks, islands}` object, and a date
is a string.

The CLI has no `--watch`. Each edit is a full `render` call and opening a new
PDF.

**Direction:** `quillmark render --watch`, plus a way to emit a standalone
`.typ` project (helper package included) that Typst's tooling can open.

### P2: Smaller surprises

- `render -f png -o x.pdf` writes PNG bytes to `x-1.pdf` and `x-2.pdf`. The
  output extension isn't reconciled with the format.
- `display(address, …)` and `data.x.display(…)` print the same ink, and the docs
  spend a page on which to use. The distinction exists only for editor regions
  (see above).
- The blueprint puts `main.description` and the first field's description on
  adjacent `#` lines, so the card's description reads as the field's.
- `cli::missing_description` fires for card fields but not for variant cells or
  typed-table columns, which the blueprint and editors show just the same.

---

## What worked

- Strict `Quill.yaml` loading, with every error collected in one pass.
  `quillmark validate` rendering the empty, blueprint and seed documents caught
  plate/schema drift before a document existed.
- Enum, datetime and coercion errors are precise and path-anchored
  (`value Regular not in allowed set …`). YAML syntax errors carry a code frame
  and an absolute `input.md` line.
- `variants:` models "fields that exist only for one choice" better than any
  flat-schema convention, and it round-trips through the blueprint.
- The acroform backend's single-source-of-truth binding (widget kind derived
  from the schema) is a strong idea for a form ecosystem.

## Priorities for an ecosystem foundation

1. Report undeclared keys, and give the CLI a strict document check (P0).
2. Make packages reachable — vendoring tool, namespaces, transitive
   dependencies — and let a plate import its own files (P0/P1).
3. Make the `~~~` card fence fail loudly and helpfully, or change it (P0).
4. Emit a semver-compatible selector from the blueprint, and show typed-table
   row shape in it (P1).
5. Make scalars nullable, so "unanswered" is visible to plates and pipelines (P1).
