# Quill Versioning System

> **Implementation**: `crates/core/src/version.rs`

## TL;DR

Quills declare a semantic `version` in `Quill.yaml`, and documents carry an optional `$quill: name@selector` reference. The selector is parsed and stored on `QuillReference`, but never **resolved** — the engine loads exactly one Quill from a path or in-memory file tree, never picking among versions. It is **checked**: at render time the loaded Quill's name and version are compared against the reference, and a mismatch yields an informational warning.

## Version Format

Semantic versioning: `MAJOR.MINOR.PATCH`. Two-segment `MAJOR.MINOR` passes validation in `Quill.yaml` (the raw string is stored as-is; no normalization occurs).

| Increment | When |
|-----------|------|
| **MAJOR** | Breaking changes: layout changes, removed fields, incompatible types |
| **MINOR** | New optional fields, enhancements (backward-compatible) |
| **PATCH** | Bug fixes, corrections (backward-compatible) |

## Document Syntax

The version selector rides on the root block's `$quill` system-metadata line (see [markdown-spec.md](../references/markdown-spec.md) §3.3):

```
$quill: my_format@2.1.0    # exact
$quill: my_format@2.1      # 2.1.x
$quill: my_format@2        # 2.x.x
$quill: my_format@latest   # latest (explicit)
$quill: my_format          # latest (default)
```

No registry consumes the selector — there is no collection of installed versions to pick from, so it is a pin, not a resolver. At render time the engine compares the reference against the one loaded Quill and emits at most one non-fatal warning:

- **`quill::ref_mismatch`** — the reference *name* differs from the loaded Quill. Checked first; the selector is then moot.
- **`quill::version_mismatch`** — names agree but the Quill's `version` fails the selector (e.g. `name@2` against `3.0.0`). `VersionSelector::matches` decides: `Exact` the identical version, `Minor` any patch in the `MAJOR.MINOR` series, `Major` any version in the `MAJOR` series, `Latest` (the default) anything.

Both are advisory — render still succeeds with the loaded Quill. The copy says so, since that Quill may be externally controlled (e.g. tooling that always loads the latest).

## Quill.yaml

```yaml
quill:
  name: my_format
  version: "2.1.0"
  backend: typst
  description: "Short description of this format"
  author: "..."          # optional
  plate_file: "plate.typ" # optional; conventional name
  ui: { ... }            # optional
```

`name`, `backend`, `version`, and `description` are required. `author`, `plate_file`, and `ui` are optional. Unknown keys under `quill:` are a hard error. `version` must parse as `MAJOR.MINOR.PATCH` (or `MAJOR.MINOR`); an invalid or missing value fails at load.

## Error Handling

Two distinct failure paths:

- **`Quill.yaml` version invalid** → `quill::invalid_version` diagnostic → surfaces as `RenderError::QuillConfig` at Quill load.
- **Document `$quill` reference invalid** (e.g. `my_format@bad`) → `ParseError::InvalidQuillReference`, returned directly by the parser, never as `RenderError::QuillConfig`.

See [ERROR.md](ERROR.md) for error patterns.

## Links

- [QUILL.md](QUILL.md) — Quill structure
- [ERROR.md](ERROR.md) — error patterns
