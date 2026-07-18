# Unified write surface: encoding carries intent

**Status: proposal.** Extends [#957](https://github.com/borb-sh/quillmark/issues/957).
Rewrites the write-surface axioms of [BINDINGS.md](../canon/BINDINGS.md)
("The write surface: two tiers over one primitive"); touches
`core/src/document/edit.rs`, `core/src/writer.rs`, and both bindings.

## Problem

Two tier-1 verbs take the same "markdown in" gesture and desugar to opposite
anchor semantics:

- `writer.set_body(md)` → `Card::revise_body` → `diff_import` — surviving
  identity anchors **rebase** (`crates/core/src/writer.rs:82`,
  `crates/richtext/src/delta.rs:364`).
- `writer.set(name, md)` on a richtext field → `Card::commit_field` →
  `conform_value(Leniency::Write)` → cold `from_markdown` — the previous
  value's anchors are **destroyed**, silently
  (`crates/core/src/quill/config.rs:537`).

The anchor-preserving field write exists only as the schema-blind
`Card::revise_field` / addressed `revise({card, field}, md)`
(`crates/core/src/document/edit.rs:667`), so no write is both typed and
anchor-preserving — #957's diagnosis, confirmed.

The root cause is not either verb: the tier split bundles four orthogonal axes
— addressing idiom (names vs `Addr`), write semantics (install vs revise),
error timing (fail-now vs defer-to-render), and receipts (returned vs
withheld). Two ratified axioms pin the bundle: BINDINGS.md's "a [tier-1]
consumer never meets … a `Delta`" and the corpus lane's "no schema in sight"
(0.93→0.94 migration, #932). The verb #957 wants — typed, anchor-preserving,
receipted — is excluded by construction: "typed" bars it from the corpus lane,
"receipted" bars it from tier 1. #957's `commit_field → Option<Delta>` fix
breaks the first axiom without saying so, and leaves `set_all` (which bypasses
`commit_field` via `resolve_field_write`, `crates/core/src/writer.rs:187`)
cold — reintroducing the same inconsistency one verb over.

## Design

Unbundle the axes. Three axioms replace the tier sentence:

> **1. Encoding carries intent: markdown revises, corpus installs.**
> **2. The lane carries error timing, nothing else: the typed door fails at
> the write; the corpus and opaque lanes defer to validate/render.**
> **3. Receipts are returned wherever text changes, and are always
> ignorable — never withheld.**

Tiering survives purely as addressing idiom: names for consumers, `Addr` for
editors. It no longer selects semantics.

### Axiom 1 — markdown revises, corpus installs

A markdown string is authored prose: the caller is revising text, so the write
diffs against the current value (`diff_import`) and rebases surviving anchors —
body and field alike, absent field diffing from empty. A corpus object is held
state: the caller means exactly this value, identity marks included, so the
write installs. `install_body` / `install_field` remain the spelled-out corpus
case — the only deliberate anchor destroyers — and the anchor-destroying
markdown write is spelled `set(name, import(md))`, generalizing the corpus
lane's "anchor loss is visible in source" principle to the whole surface.

### Axiom 2 — three doors, by error timing

| Door | Input | Semantics | Errors |
|---|---|---|---|
| typed: `writer.set` / `set_all` / `set_body` / `add_card` | markdown, corpus, scalar | md → revise; corpus → install; scalar → coerce | fail now (`richtext(inline)` on the resulting corpus) |
| corpus: `install(addr, rt)` / `applyChange(addr, bundle)` | corpus / change bundle | value / splice | defer to validate/render |
| opaque: `setField` / `setCardField` | anything | verbatim | defer to render |

The blind lane accepts **no markdown**. `revise_field`'s schema-blind markdown
path is a workaround for the typed door being cold (its own doc says so:
"the field-level `diff_import` the write surface previously lacked",
`edit.rs:648`); once the typed door revises, the workaround's audience is
empty. The blind lane's real constituency — the mid-interaction editor — is
served by `applyChange` (transient `inline` violations while typing are legal
there) and `install` (state restore, undo); blur/save commits through the
typed door and is checked. `Card::revise_body` stays as the mechanism under
`set_body`: the body carries no `inline` flag, so nothing is unchecked there.
(The body's one schema bit, `body.enabled`, is today enforced only at
validate/render; the typed `set_body` may additionally reject a body-disabled
kind at the write — optional extension, not load-bearing.)

### Axiom 3 — receipts universal and ignorable

Every revise-shaped write returns its `Delta`; `let _ =` in Rust and an
ignored return value in JS/Python are free. Withholding the receipt from
tier 1 is what manufactured #957's impossible verb: a tier-1 caller needing a
caret was forced into a lane with different *semantics*, not just different
*ergonomics*.

- `Card::commit_field(..) -> Result<Option<Delta>, EditError>` — `Some` for a
  markdown-into-richtext revise, `None` otherwise. `plaintext` is exempt from
  axiom 1: a plain corpus carries no marks by construction
  (`RichText::is_plain`, `crates/richtext/src/model.rs:385`), so there is no
  identity to preserve — its string writes stay value-semantic, receipt `None`.
- `TypedWriter::set` forwards it; `set_body` returns the `Delta` it currently
  discards; `CardWriter` mirrors both.
- `set_all` success returns receipts keyed by name. The batch resolves against
  the pre-batch snapshot (all-or-nothing unchanged); a duplicate key is
  last-wins with one diff from the pre-batch value to the final value.
- Bindings: `commitField` returns `Delta | null`; writer `set` / `setBody`
  likewise.

## Surface delta

0.94.0 shipped the addressed verbs, so removals take the house one-cycle
deprecation (as `replaceBody` did), not pre-release deletion.

| Current | Becomes |
|---|---|
| `Card::commit_field` cold-imports markdown | diffs against the stored corpus; returns `Option<Delta>` (breaking signature, core 0.95) |
| `Card::revise_field(name, md)` | deprecated one cycle → removed; use `writer.set(name, md)` |
| wasm/py `revise(addr, md)` | deprecated one cycle → removed; `revise({}, md)` ≡ `writer.setBody(md)` (now receipted), field targets → `writer.set` / `commitField` |
| `TypedWriter::set_body` discards the delta | returns it |
| `resolve_field_write` (pure dry-run) | takes the current field value as diff base; still applies nothing |
| `install(addr, rt)`, `applyChange`, `setField`, codec verbs | unchanged |
| BINDINGS.md tier sentence | restated per the three axioms; parity table updated |

## Costs

- A richtext `set` reads the current field value (decode as diff base) — a
  read dependency the dry-run resolver doesn't have today; resolved against a
  snapshot, so `set_all`'s collect-every-error contract holds.
- The pedagogical "tier 1 never meets a `Delta`" sentence is spent, replaced
  by "no caller is forced to handle one."
- An editor restoring possibly-schema-stale held state keeps blind `install`;
  nothing lost there. The one live trade: a caller who *wants* a cold markdown
  replace must spell it (`set(name, import(md))`) — deliberate, per axiom 1.
- One-cycle deprecation traffic on `revise` / `revise_field`.

## Relation to #957

Adopts its `commit_field` recommendation (diff on markdown-into-richtext,
inline check on the result, corpus keeps value semantics) and its receipt —
but as one instance of axiom 1/3 rather than a single-verb patch, closing the
`set_all`, `set_body`-receipt, and blind-markdown gaps it leaves open.
