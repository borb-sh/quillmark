# Crate API Compatibility

> **Implementation**: `crates/core/src`, `.github/workflows`
> **Related**: [VERSIONING.md](VERSIONING.md)

## TL;DR

Every publishable crate carries one workspace version and one SemVer promise to
crates.io consumers. No CI job checks it: the rules below are what holds it, and
a writer decides them as the change is written. This page is about the Rust
crate API: [VERSIONING.md](VERSIONING.md) is about quill versions, a separate
axis.

## What the promise covers

The published crates are `quillmark-core`, `quillmark`, `quillmark-content`,
`quillmark-pdf`, `quillmark-typst`, `quillmark-pdfform`, and `quillmark-cli`.
The Python, WASM, fuzz, and fixture crates are `publish = false`: they path-dep
the core beside them and ship as one build, so nothing there is a compatibility
surface.

Two seams are `pub` but explicitly outside the promise, each saying so in its
own rustdoc:

| Seam | Why it is exempt |
|---|---|
| `Backend` + `SessionHandle` | Sealed, with the seal's module and `SessionHandle` `#[doc(hidden)]`; an out-of-workspace backend writes against items this crate does not hold stable. |
| `quillmark_typst::emit` | `pub` only so `quillmark-fuzz` can drive the escapers directly. |

Deleting a row promises the seam. Publishing a contract later is additive and
retracting a published one is a major, so a row stays until a release
deliberately publishes that seam.

## `#[non_exhaustive]`

The attribute is narrower than it reads, and the two halves cost different
things.

| On | Forbids out-of-crate | Still allowed out-of-crate |
|---|---|---|
| enum | exhaustive `match` | constructing any variant |
| struct | struct literals, exhaustive destructuring | reading and assigning every `pub` field |

Inside the defining crate it does nothing, so an exhaustive match in `core` is
still a compile error when a variant lands. The guardrail stays where the work
happens.

### The enum rule

Mark it, unless a downstream `_` arm can be **silently** wrong.

The question is what a missed variant does, not whether the concept feels
closed. Ontological arguments: "a severity is two levels", "a tree is files and
directories": are the ones that get falsified, and `OutputFormat` has already
lost a variant. Ask instead:

- **Loud on a miss** → mark it. A `_` arm that raises, refuses, or degrades to a
  documented lower bound loses nothing.
- **Silent on a miss** → keep it exhaustive, say so in the rustdoc, and accept
  semver-major as the price.

Three enums keep the exemption, each saying so in its own rustdoc:

| Enum | What a missed variant costs |
|---|---|
| `quillmark_pdf::FieldType` | `pdfform`'s value resolver and its content-stream flattener both dispatch over the whole set from another crate: the field **draws nothing on the page and reports nothing**. |
| `KnownIslandType` | The Typst emitter dispatches over the whole set from another crate: the island **leaves the projection entirely**. (Its markdown twin is in `quillmark-content` itself, where the attribute changes nothing.) |
| `Fidelity` | A ladder a consumer reads to decide what to warn about, with no safe rung to fall through to. |

For all three the compile error is the guardrail, and a new member is a
semver-major. The storage DTOs are exhaustive on separate grounds: a shipped
schema version is frozen, so it does not grow at all.

### Picking the fallback

A forced `_` arm is a new opportunity to re-spell an existing behavior slightly
differently. Two rules keep that from drifting:

- Take the direction that cannot be silently wrong. `Severity` escalates to
  `Error`: over-reporting shows a note too loudly, the other direction hides a
  fatal. `HitGranularity` degrades to `Segment`, a lower bound on precision.
- Where no safe direction exists, refuse. No fallback `OutputFormat` is honest;
  every one of them promises bytes the caller did not ask for.

When the arm restates a diagnostic another site already builds, share the
constructor rather than hand-writing the body. One code carries one payload, so
`quillmark_core::unsupported_format` is the single place the format refusal is
built and three call sites cannot spell it three ways.

### The struct rule

Mark it, and give it a constructor when another crate builds one.

A struct with private fields needs nothing: it is already unconstructible from
outside. For one with `pub` fields the attribute buys exactly one freedom:
adding a field, and no others, since every field stays readable and assignable.
Making a field private, changing its type, or computing it lazily still needs
private fields plus accessors, the shape `YamlError` uses.

Constructor shape: `new` takes the fields a value *always* carries; everything
optional starts absent and has a `with_*` setter beside it. A `new` that takes
every field is exactly as brittle as the literal it replaced.

Three structs keep the exemption, each saying so in its own rustdoc:

| Struct | Why the literal is worth keeping |
|---|---|
| `Delta` | The one freedom the attribute buys, this type cannot spend: `{ops}` *is* the serde wire, so a second field is a wire change every binding's codec learns either way. |
| `Segment` | A derived view `line_segments` recomputes whole, holding one line's position in the two coordinate spaces the model has. A third space moves the model first. |
| `BaseLengthMismatch` | Two lengths are the whole of one comparison. |

For all three a new field is a semver-major.

An integration test sits out-of-crate, so `crates/content/tests/` builds through
the constructors like any consumer, and the attribute is under test where the
crate's own unit tests cannot reach it.

`RenderOptions` is the type this cost is real for. `#[non_exhaustive]` forbids
functional update, so `RenderOptions { .., ..Default::default() }` does not
compile out-of-crate and `RenderOptions::default().with_output_format(…)` is the
path.

A tag is the cheapest moment to take a break like that, and the last one for the
major it opens. Declining at the tag is a decision for the whole `1.x` series:
an option that cannot be added in 1.4 costs a 2.0.

## What the attribute does not cover

Most of what breaks a consumer is not a new variant or a new field. Adding a
method to an unsealed public trait, removing or renaming anything, widening a
bound, a public type losing `Send`/`Sync` through a private field, and a public
signature naming a dependency's own type, which chains this crate's major to
that dependency's (see [ERROR.md](ERROR.md) on the YAML boundary), are all
majors that no attribute sweep sees.

Nothing mechanical catches that class, so it rides on the writer and the
reviewer. `cargo semver-checks check-release` covers most of it and is worth
running by hand against a release baseline when a change's blast radius is
unclear; it wants a real version bump to compare against, since cargo reads a
`0.x` minor bump as major and a working tree still carrying the last released
version compares that version against itself.
