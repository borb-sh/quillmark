# Quill Versioning

## Version Field in `Quill.yaml`

Each Quill declares a semantic version:

```yaml
quill:
  name: my_quill
  backend: typst
  description: A professional document format
  version: "1.2.0"
```

**Quote the value.** An unquoted `1.10` is YAML's number `1.1` and loads as
that version — a different one from the one you wrote. `version` is required,
and an invalid or missing value fails at load.

Use semantic versioning (`MAJOR.MINOR.PATCH`, or two-segment `MAJOR.MINOR`) to
communicate compatibility:

- **MAJOR**: breaking changes to fields, cards, or expected document shape —
  including an `enum` member removed or renamed, which stops a stored document
  validating
- **MINOR**: backward-compatible additions (new optional fields, a new enum
  member, non-breaking behavior)
- **PATCH**: fixes and small improvements without shape changes

Reordering a field's `values:` is neither: a field's blank is `""` rather than
`values[0]`, so no document's rendered output changes. It still moves picker
order, the blueprint's `enum` annotation, and acroform dropdown order.

## How Authors Select Versions

Authors target a version through the root block's `$quill` system metadata:

```markdown
~~~
$quill: my_quill@1.2
$kind: main
title: Quarterly Report
~~~
```

Supported selectors:

| Selector | Meaning |
|---|---|
| `my_quill` | Latest available version |
| `my_quill@latest` | Latest available version (explicit) |
| `my_quill@1` | Latest 1.x.x |
| `my_quill@1.2` | Latest 1.2.x |
| `my_quill@1.2.0` | Exact version |

## Compatibility Checks

A selector is a **pin, not a resolver**: Quillmark renders with the Quill it was
handed and never picks among versions. At render time (and in `dry_run`) it
checks that Quill against the reference and rejects a mismatch —
`quill::name_mismatch` where the names differ, `quill::version_mismatch` where
the names agree and the version falls outside the selector.

Fix either by rendering with the Quill the document targets, or by amending the
`$quill` line: correct the name, or widen the selector (`@3`, `@latest`). A bare
name or `@latest` matches any version, so a document that targets its Quill
correctly never trips these checks.

## Practical Guidelines

1. Start at `1.0.0` for your first stable internal format release.
2. Increase versions on every format change, even if small.
3. Treat field renames and removals as breaking (`MAJOR`); prefer additive
   changes (new optional fields and cards).
4. Where the quill ships an [example document](quill-yaml-reference.md#example),
   bump the `$quill: <name>@<version>` line inside it along with the version:
   an example documents the version it pins.

## Related Pages

- [Creating Quills](creating-quills.md)
- [Quill.yaml Reference](quill-yaml-reference.md)
- [card-yaml Blocks](../authoring/card-yaml.md)

Full model: [VERSIONING.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/VERSIONING.md).
