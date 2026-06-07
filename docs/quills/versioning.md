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

Use semantic versioning (`MAJOR.MINOR.PATCH`) to communicate compatibility:

- **MAJOR**: breaking changes to fields, cards, or expected document shape
- **MINOR**: backward-compatible additions (new optional fields, non-breaking behavior)
- **PATCH**: fixes and small improvements without shape changes

## How Authors Select Versions

Authors can target versions through the root block's `$quill` system metadata:

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

A selector is a **pin**, not a resolver: Quillmark renders with the Quill it was
handed and never picks among versions. At render time (and in `dry_run`) it
checks that Quill against the reference:

- If the loaded Quill's **version** falls outside the selector (e.g. `my_quill@2`
  against a `3.0.0` Quill), rendering **fails** with a `quill::version_mismatch`
  error. Rendering a document against an incompatible format is a footgun, so it
  is rejected rather than silently produced. Fix it by pointing at a Quill whose
  version satisfies the selector, or by widening the selector (e.g. `@3` or
  `@latest`).
- If the **name** differs, a non-fatal `quill::name_mismatch` warning fires
  instead and the version selector goes unchecked — the engine assumes you
  deliberately rendered with a different Quill.

## Practical Guidelines

1. Start at `1.0.0` for your first stable internal format release.
2. Increase versions on every format change, even if small.
3. Treat field renames/removals as breaking (`MAJOR`) changes; prefer additive changes (new optional fields/cards).

## Related Pages

- [Creating Quills](creating-quills.md)
- [Quill.yaml Reference](quill-yaml-reference.md)
- [card-yaml Blocks](../authoring/card-yaml.md)
