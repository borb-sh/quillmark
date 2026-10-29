# Quill Versioning System

> **Implementation**: `crates/core/src/`
> **Related**: [QUILL.md](QUILL.md), [ERROR.md](ERROR.md)

## TL;DR

Quills declare a `MAJOR.MINOR.PATCH` `version` in `Quill.yaml`, bumped by what
the change costs a stored document to migrate, and documents carry an optional
`$quill: name@selector` reference. The selector is parsed into `QuillReference`
but never **resolved**: Quillmark loads exactly one Quill, from a path or an
in-memory file tree, and never picks among versions. It is **enforced**: at
render a *name* mismatch or a *version* outside the selector is a hard error,
since a valid document paired with the wrong Quill is a footgun.

## Version Format

`version` is required in the `quill:` section and is `MAJOR.MINOR.PATCH`. Any
other value, a two-segment `1.2` included, fails at load, and an unquoted `1.10`
is a YAML number, never a version. Segments are plain digits: a `+`-signed one
is refused, as is a `$quill` reference whose `@` carries no selector.

## Bump Classes

A bump is ranked by what it costs to migrate a document stored against the
previous version:

| Bump | A stored document | Changes |
|------|-------------------|---------|
| **PATCH** | needs no change | Fixes, presentation changes, a new optional field, a new enum member, a `values:` reorder, a `text` or `richtext` field's `multiline` from `false` to `true` |
| **MINOR** | migrates mechanically | A field renamed, a type changed with a lossless mapping (`date` → `datetime`), an enum member renamed |
| **MAJOR** | migrates with loss of data or function | A field or enum member removed with no counterpart, a field's meaning changed |

This is not standard semver. The compatibility boundary is `MAJOR.MINOR`, the
same below and above `1.0.0`: every patch in one `MAJOR.MINOR` series reads the
documents an earlier patch stored unchanged, and a MINOR bump needs each one
rewritten, mechanically. The quill's seeded, empty and example documents, and
a document `from_authoring` builds, pin `name@MAJOR.MINOR`
(`seed::main_reference`).

**A `values:` reorder is render-safe.** A field's blank is `""`, not `values[0]`,
so reordering changes no document's rendered output. It still changes picker
order, the schemas' `enum` lists, and acroform dropdown order, so it
is a presentation change rather than a no-op. Renaming or removing a member is
not: a stored document carrying it stops validating.

**`multiline` widens one way.** From `false` to `true` it only admits more, so
it is a PATCH. From `true` to `false` it tightens a constraint: a stored
`richtext` holding a second block stops validating
(`validation::not_single_line`) and a stored `text` holding a second line
renders it as a space (`validation::line_break_folded`), so it is a MAJOR
unless each stored value is first reduced to one line.

## Document Syntax

The version selector rides on the root block's `$quill` system-metadata line (see [markdown-spec.md](../references/markdown-spec.md) §3.3):

```
$quill: my_format@2.1.0    # exact
$quill: my_format@2.1      # 2.1.x
$quill: my_format          # any version
```

A selector is `MAJOR.MINOR` or `MAJOR.MINOR.PATCH`, each segment plain digits.
`my_format@2` fails `parse::invalid_quill_reference`: a major spans MINOR bumps,
so it names no compatible series. A reference with no selector omits the `@`;
there is no `@latest`.

A name may carry a collection: `$quill: borb.my_format@2.1`. The collection is
the host's, naming where to resolve the quill; a bare name leaves that choice to
the host. Quillmark reads only the part after the `.`, so a loaded `my_format`
answers a reference under any collection. A quill's own `Quill.yaml` name and
its example never carry one: which collection a quill sits in is the
collection's to say. A widened grammar is no storage event: a stored row naming
a collection reads under the same tag, and a reader older than the grammar
refuses it as an invalid quill reference. A narrowed grammar is a storage event
the tag does not record: a stored row whose `$quill` it refuses, such as the
lone major `my_format@2`, fails to load under the same tag.

No registry consumes the selector: there is no collection of installed versions to pick from, so it is a pin, not a resolver. *Resolution* (matching `name@selector` against a set of installed versions) belongs to a higher layer; Quillmark loads one Quill and *enforces* the reference against it. Detection needs no registry (Quillmark has the loaded Quill's name and version and the document's reference) so `render` and `dry_run` both reject a mismatch with a single-diagnostic [`RenderError`](ERROR.md). They check in order:

- **`quill::name_mismatch`**: the reference *name*, collection aside, differs from the loaded Quill. The name is the prerequisite (a selector belongs to a *named* Quill), so a name mismatch short-circuits and the version is left unevaluated.
- **`quill::version_mismatch`**: names agree but the Quill's `version` falls outside the selector (e.g. `name@2.1` against `2.2.0`). `VersionSelector::matches` decides: `Exact` the identical version, `Minor` any patch in the `MAJOR.MINOR` series, `Any` (no selector) anything.

A quill mismatch is distinct from a validation failure (a malformed document): here the document is well-formed but paired with the wrong Quill, so the remedy is to render with the referenced Quill or amend `$quill`. A bare name matches any version, so correctly-targeted documents never trip either check.

`$quill` is a **pairing assertion**, not a render target or a schema declaration. The caller chooses which Quill renders; the reference only confirms the document was authored against it.

## Error Handling

Three distinct failure paths, and the parser owns one of them outright:

- **`Quill.yaml` version invalid** → a `quill::invalid_version` diagnostic from
  Quill load: `Quill::from_tree` returns it in its `Vec<Diagnostic>`,
  `quill_from_path` in the `RenderError` wrapping that vector.
- **Document `$quill` reference invalid** (e.g. `my_format@bad`) →
  `ParseError::InvalidQuillReference` (`parse::invalid_quill_reference`),
  returned directly by the parser, never through Quill load.
- **Loaded Quill does not satisfy a well-formed `$quill`** → the two mismatch
  codes above, as a `RenderError` from `render`/`dry_run`.

## Ref Immutability

A canonical ref (`name@version`) is **immutable content within the lifespan of a
runtime**: once any layer has materialized a Quill for a ref, the content behind
that ref never changes for that process.

This is a **caching invariant, not a source-control policy.** It constrains the
process, because the caches below key on the ref and none exposes invalidation.
It does not forbid a maintainer from editing a published ref's bytes in the
repository: a later build materializes fresh, and no running process sees
content change underneath it. The constraint that governs such an edit is
**compatibility with persisted artifacts** — stored documents must keep loading,
validating, and rendering as they did. Removing or renaming an enum member fails
that test; a `values:` reorder passes it, since a field's blank is `""` rather
than `values[0]` (see [SCHEMAS.md](SCHEMAS.md) § "Blank-filled render"). An
edit that changes what an existing document *renders* is a version bump's job.
The fixture quiver (`crates/fixtures/resources/quills`, `publish = false`) is
the exception: it ships in no package, so its quills are edited in place at
their declared version as Quillmark moves, and a change to what one renders
is named in that release's migration guide.

The one real hazard is a long-lived process holding a cached Quill across a
deploy that rewrote the ref; a restart resolves it.

Every cache between a document and its rendered output keys on this invariant,
and none of them exposes an invalidation API, **by design**: the wasm `Engine`
caches backend-memory clones in a `WeakMap` keyed on the canonical `Quill`
instance, so a clone's lifetime follows the instance. A consumer holding one
`Quill` per canonical ref inherits the same rule.

"Invalidate" therefore means *replace the instance*: a new `Quill` at a new
ref, and the downstream caches follow automatically (WeakMap + weak refs).
