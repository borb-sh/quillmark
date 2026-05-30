# Blueprint / Example — Two Named Reference Documents

> **Motivation**: `blueprint_filled(FillBehavior)` exposes an internal fill
> strategy as a public enum, overloading what a "blueprint" *is*. Split the
> public surface into two intent-named reference documents — `blueprint`
> (canonical `<must-fill>`) and `example` (illustrative) — demote the fill
> strategy to an internal detail, and retire the pure-zero blueprint mode as
> a dead branch.

## TL;DR

A blueprint is **always** the canonical `<must-fill>` authoring document —
no modes. Add a second named output, `example`, built from the field
`example:` values (falling back to the zero value). Demote `FillBehavior`
from a public parameter to an internal `FillSource` strategy. The pure-zero
mode (today's `TypeEmpty` blueprint) is removed: its only consumer — the
quiver authoring contract — instead zero-filled-renders an empty document
(see [zero-filled-render.md](zero-filled-render.md)).

Pre-1.0; not yet implemented. When built, graduates into
[BLUEPRINT.md](../canon/BLUEPRINT.md).

## Background

The current public surface (in `crates/core/src/quill/`) is `blueprint()`
plus `blueprint_filled(FillBehavior)` with three variants:

| `FillBehavior` | Fills Must Fill cells with | Consumer |
|---|---|---|
| `Strict` | `<must-fill>` sentinel (identical to `blueprint()`) | authoring surface |
| `Preview` | the field's `example:`, else the zero value | CLI `render` with no input file |
| `TypeEmpty` | zero value everywhere (`""`, `0`, `false`, `[]`, `{}`, first enum) | quiver authoring-contract test |

Two problems:

- **"Blueprint" is overloaded.** A consumer must understand three fill
  strategies to know what a blueprint even is. The canonical authoring
  surface and a populated sample are not the same artifact, yet both are
  "a blueprint."
- **It leaks an internal concern as a public flavor.** `TypeEmpty` is the
  render *floor* (the type-minimal valid input), not a document anyone
  authors from. Exposing it as a blueprint variant conflates "document I
  fill in" with "minimal render input."

## Two named reference documents

| Output | Intent | Fill source | Sentinels? |
|---|---|---|---|
| `blueprint` | *"give me the form to fill"* | `<must-fill>` | yes |
| `example` | *"show me a filled-out one"* | example-else-zero | no |

- `default:` always wins when present, in both outputs (an Endorsed field
  renders its default, never a sentinel or example).
- `example` is example-*else*-zero: a field without an `example:` renders at
  its zero value. So it is "examples where defined, blank otherwise" — the
  most illustrative document available, not a fully populated one. State
  this in the contract so no one expects every field filled.
- **Naming.** `example()` collides with the `example:` field-schema
  property. Consider `sample`, `example_document`, or `demo` for the path.
  The concept is settled; the label is open.

This is the same axis canonized at the field level — placeholder-to-fill
(`<must-fill>` / no `default`) vs. illustrative value (`example:`) — lifted
to the document level.

## Internal unification — one emitter, a `FillSource` strategy

The emitter already walks the schema and pulls a per-field value from a
source. Demote `FillBehavior` to an **internal** `FillSource`:

- `Sentinel` → `blueprint`
- `ExampleElseZero` → `example`

`default:` wins across both. The per-field **zero value** (`must_fill_value`
/ `first_enum`, factored into a `QuillValue` producer) is a shared leaf
utility — called by the `ExampleElseZero` fallback here *and* by zero-filled
render. The two proposals share one zero-value producer; this is the
"unified internally" goal.

## The dead branch: pure-zero blueprint

`blueprint_filled(TypeEmpty)` emits a document with every Must Fill field at
its zero value. Its only caller is the quiver authoring-contract test
(`every_quill_in_quiver_renders`). Once zero-filled render exists, this mode
has no reason to live:

- The contract — *"the plate renders type-minimal valid input"* — is more
  directly expressed as **zero-filled render of an empty document** for each
  quill. No blueprint string is generated or re-parsed.
- The canonical blueprint can't serve as that fixture anyway: its
  `<must-fill>` sentinels are **malformed** under zero-filled render (they
  error), so "render the blueprint" was never the literal test.

So the blueprint emitter keeps exactly two fill sources — `Sentinel` and
`ExampleElseZero`. The pure-`Zero` *mode* leaves blueprint logic entirely;
the zero *value* survives as the example fallback and the render floor.

`Preview` likewise stops being a blueprint mode: "render with no input"
becomes "render the `example` document" (or a zero-filled render of an empty
doc, when blank is wanted).

## Bindings surface

Replace the Rust `blueprint_filled(behavior)` escape hatch with `blueprint()`
+ `example()` (or the chosen name). Wasm/Python/CLI already expose only
`blueprint`; add the example accessor. CLI `render` with no input renders the
`example` document.

## Rejected / open

- **Keep `FillBehavior` public** — rejected. It is an internal strategy; the
  public surface should name *intents* (blueprint vs. example), not
  strategies.
- **Open** — the second output's name (collision with the `example:` field
  property).

## Graduation

Fold into [BLUEPRINT.md](../canon/BLUEPRINT.md): a "Two reference documents"
section replaces "Filled blueprints" / the `FillBehavior` table, and the
zero-value producer cross-links to the zero-filled-render section. Delete
this proposal.
