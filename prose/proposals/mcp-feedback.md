# MCP Authoring-Surface Cleanup

> **Motivation**: friction observed in an MCP consumer's eval, where LLM
> authors of Quillmark Markdown needed a written rulebook to compensate
> for spec choices that fought their priors. This proposal addresses the
> subset of that friction whose fixes do not break the CommonMark-superset
> guarantee or introduce silent coercion.

## TL;DR

Four locked-in changes: drop the redundant `$kind: main` requirement
on the root block, give validation errors a uniform name-field /
show-token / offer-both-exits format, collapse the
`required`/`optional` schema axis into `default`-presence, and add
authoring guidance favouring non-empty defaults. Pre-1.0; breaking
changes acceptable.

## Background

A consumer building an MCP server over Quillmark surfaced six friction
points where LLM authors made systematic mistakes the consumer had to
bandage with prompt-side rules. Three of the six (asymmetric fences,
tildes-only, `---` frontmatter as sugar) defend invariants worth more
than the friction they cost and are not addressed here. The remaining
three are addressed below — two directly via grammar collapses, one
via a uniform validation-error format that subsumes a class of LLM
failures.

A separate observation from the same review: the `role: required |
optional` axis on field schemas was overpromising a validation gate
the system did not implement, because the blueprint pre-fills every
field and absence never reaches validation through the blueprint
pathway. The collapse below removes that overpromise.

## Locked-in changes

### 1. Drop `$kind: main` requirement on the root block

The root block is identified by position (MARKDOWN.md §2). Requiring
`$kind: main` on the root is double bookkeeping that LLMs forget to
satisfy and that adds no validation power.

**Parsing.** Root block with `$kind: main` continues to validate.
Root block without `$kind` validates as well; `main` is the implicit
kind by virtue of position.

**Canonical emission.** Canonical emission auto-injects `$kind: main`
on the root, so the canonical form is unchanged and existing
canonical documents round-trip byte-equal. Non-canonical input
(omitted `$kind` on root) converges to the canonical form on first
emit.

**Rejected:** a composable block declaring `$kind: main` is still a
parse error. The reservation of `main` for the root holds.

**Spec touches:** MARKDOWN.md §3.3 (rules block), §9 (emission
contract), §10 (errors).

**Implementation:** `crates/core/src/document/`.

### 2. Boundary-error uniform format

Validation errors that fire when a YAML scalar's parsed type does not
match the schema's declared type — or when a no-default field arrives
absent / type-empty — emit a single canonical shape:

- Names the field by its path (`recipient`, `cards[2].author`).
- Shows the source token verbatim (`42`, `null`, `true`, `""`).
- Offers both exits when applicable: quote the value, or change the
  schema's declared type.

No silent coercion. The error message is the lever; the parser stays
strict. This subsumes complaint #6 (string-typed fields receiving
integer/boolean shapes) and complaint #5 (`null` on default-bearing
fields — the message points the LLM at "omit the line").

Example messages:

```
Field `build_number` got integer `42`, schema declares `string`.
Either quote the value (`build_number: "42"`) or change the
schema's `type:` to `integer`.
```

```
Field `subtitle` got `null`, schema declares `string` with default
`"My Subtitle"`. Either omit the line (the default will fill in)
or set the value to a string.
```

**Spec touches:** SCHEMAS.md (Native validation section), ERROR.md
(diagnostic format if a new error code is introduced).

**Implementation:** `crates/core/src/quill/` (validation walker;
error construction).

### 3. Collapse `required`/`optional` into `default`-presence

The `role: required | optional` axis in `Quill.yaml` is removed. A
field's required-ness is derived from whether a `default` is declared:

| `default` in schema | Field state |
|---|---|
| absent | required — validation rejects absence; for `string` and `markdown`, additionally rejects type-empty values |
| present (any value, including type-empty) | optional — absence is accepted (default substitutes); any present value is accepted |

**Why two type categories.** For strings and markdown, the type-empty
value (`""`, empty block scalar) is the natural "unset" marker — an
LLM leaving it untouched is the failure case the rule catches. For
booleans, integers, numbers, arrays, and objects, the type-empty
value (`false`, `0`, `[]`, `{}`) is a legitimate domain value, not an
unset marker; rejecting it would reject valid authoring choices.
Schema authors are expected to declare a `default` for these types,
since omitting the field defaults to the type-empty value at
validation anyway — making the no-default state behaviourally
identical to `default: <type-empty>`. A schema lint can warn on
no-default boolean / integer / array / object fields if this becomes
common.

**Blueprint rendering.** The inline annotation slot loses its
mandatory role token. The grammar becomes
`# <type>[<format>][; <extra>...]` where the role slot is gone and
no other token replaces it. Required-vs-optional is conveyed by the
rendered value itself: an empty string field reads as "fill me," a
non-empty string field reads as "value present." For non-string types,
the visual signal is absent and the LLM gets disambiguation from the
schema (presented separately or implied by context) and from the
validation feedback loop in #2.

**Migration.** Pre-1.0; breaking change accepted.

- Existing quills declaring `required: true` and no `default`: drop
  the `required` declaration; behaviour is preserved.
- Existing quills declaring `required: true` and a `default`: drop
  `required`; field becomes optional. The "must customize this
  starting point" semantic is not preserved; if the author wants a
  customize-mandatory placeholder, move the value to `example` and
  remove the default.
- Existing quills declaring `required: false` (or omitting `required`)
  with no `default`: add `default: ""` (or appropriate type-empty
  literal) to preserve optional semantics.
- Existing quills declaring `required: false` (or omitting `required`)
  with a `default`: no change.

A one-shot migration script reads the old form and emits the new
form deterministically.

**Cells lost.** The old four-cell cross-product of
`{required, optional} × {has_default, no_default}` collapses to
three:

- required + no default (was: required + empty default)
- optional + empty default (was: optional + empty default)
- optional + non-empty default (was: optional + non-empty default OR
  required + non-empty default)

The last collapse — "required + non-empty default" merging into
"optional + non-empty default" — is the only behavioural shift.
Real-world cases in this cell were predominantly shippable defaults
("Curriculum Vitae" CV title, today's date) rather than
must-customize placeholders, so the loss is mostly nominal. The rare
must-customize case is reachable via `example:` without a `default`.

**Spec touches:** SCHEMAS.md (field schema definition, validation
section), BLUEPRINT.md (inline annotation grammar, placeholder
cascade).

**Implementation:** `crates/core/src/quill/` (schema model, validator,
blueprint emitter).

### 4. Authoring guidance: prefer shippable defaults

BLUEPRINT.md gains a short authoring-guidance section recommending
that schema authors:

- Declare `default` on a field when the default value is acceptable
  to ship as-is.
- Omit `default` when the author / LLM / user must supply a value.
- Use `example` (not `default`) for illustrative values that should
  not pre-fill the document — e.g., when the value must be
  customised per document and no shippable default exists.

This is documentation, not enforcement. No warning fires from missing
defaults.

**Spec touches:** BLUEPRINT.md (new authoring-guidance section).

## What this proposal does not do

- Does not change fence syntax (`~~~card-yaml` stays asymmetric;
  tildes-only stays).
- Does not accept `---` frontmatter as sugar.
- Does not add type coercion at the parser boundary.
- Does not add a `field_name_type_mismatch` lint. This was considered
  and demoted — naming-convention heuristics produce false positives
  on legitimate fields (`is_a_test`, `count` as a unit suffix) and
  the boundary error in #2 surfaces the real bug per-document.
- Does not add visual signalling of optionality beyond the
  empty-vs-filled placeholder shape. Non-string-type ambiguity is
  resolved reactively via #2's error feedback loop.

## Implementation order

1. **Spec edits** to MARKDOWN.md, SCHEMAS.md, BLUEPRINT.md, and ERROR.md
   reflecting the changes above. Spec edits go first so the
   implementation has a target.
2. **`$kind: main` relaxation** in the document parser and emitter
   (#1). Smallest change; lands independently.
3. **Boundary-error format** in the validation walker (#2). Independent
   of #3.
4. **`role` axis removal** in `QuillConfig` and downstream consumers
   (#3). Largest change; includes blueprint-emitter rewrite of the
   inline annotation grammar and the placeholder cascade.
5. **Quill migration**: rewrite bundled quills (`crates/quillmark/quiver/`)
   to the new schema shape; update fixture tests; verify the
   `every_quill_in_quiver_renders` invariant still holds.
6. **Documentation guidance** added to BLUEPRINT.md (#4).

Each step is a separate commit; the order above is also a safe
landing order (later steps depend on earlier ones for the spec
contract).

## Open questions

- **Schema lint for no-default non-string fields.** Worth adding as a
  warning, or leave to authoring convention? Deferred — revisit after
  the migration shakes out which fields actually omit defaults in
  practice.
- **Error code namespace.** The boundary errors in #2 may need new
  error codes (`validation::type_mismatch`,
  `validation::required_field_empty`, `validation::required_field_absent`).
  Decided during implementation; not a design question.
