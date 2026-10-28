# Quill Versioning

## Version Field in `Quill.yaml`

Each quill declares its version as `MAJOR.MINOR.PATCH`:

```yaml
quill:
  name: my_quill
  backend: typst
  description: A professional document format
  version: "1.2.0"
```

`version` is required, and a missing or invalid value fails at load. A
two-segment `1.2` fails with `quill::invalid_version`: write `1.2.0`. An
unquoted `1.10` is YAML's number `1.1`, never a version.

## Choosing the Bump

Bump by what the change costs a document already stored against the quill:

| Bump | A stored document | Changes |
|---|---|---|
| **PATCH** | needs no change | Fixes, presentation changes, a new optional field, a new enum member, a `values:` reorder, a `text` or `richtext` field's `multiline` from `false` to `true` |
| **MINOR** | migrates mechanically | A field renamed, a type changed with a lossless mapping (`date` → `datetime`), an enum member renamed |
| **MAJOR** | migrates with loss of data or function | A field or enum member removed with no counterpart, a field's meaning changed |

This is not standard semver. The compatibility boundary is `MAJOR.MINOR`: every
patch in one series reads the documents an earlier patch stored, unchanged. The
rule is the same below and above `1.0.0`, so `0.x` promises no less than `1.x`.

Reordering a field's `values:` is PATCH: a field's blank is `""` rather than
`values[0]`, so no document's rendered output changes. It still moves picker
order, the schemas' `enum` lists, and acroform dropdown order.

Turning a field's `multiline` from `true` to `false` is MAJOR unless every
stored value is first reduced to one line: a `richtext` holding a second block
stops validating (`validation::not_single_line`), and a `text` holding a second
line renders it as a space (`validation::line_break_folded`).

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
| `my_quill` | Any version |
| `my_quill@1.2` | Any 1.2.x |
| `my_quill@1.2.0` | Exact version |

A selector is digits only. `my_quill@1` fails to parse: a major spans MINOR
bumps, each of which needs stored documents rewritten, so it selects no
compatible series. There is no `@latest` either: the bare name matches any
version.

## Compatibility Checks

A selector is a **pin, not a resolver**: Quillmark renders with the quill it was
handed and never picks among versions. At render time (and in `dry_run`) it
checks that quill against the reference and rejects a mismatch —
`quill::name_mismatch` where the names differ, `quill::version_mismatch` where
the names agree and the version falls outside the selector.

Fix either by rendering with the quill the document targets, or by amending the
`$quill` line: correct the name, or correct the selector, or drop it to match
any version. A bare name matches any version, so a document that targets its
quill correctly never trips these checks.

The host picks which matching quill to hand Quillmark; in Rust,
`quillmark_core::version::VersionSelector::matches` tests a candidate's version
against the selector.

## Practical Guidelines

1. Bump on every change to a published quill, even a fix.
2. Prefer PATCH changes: add an optional field or an enum member rather than
   reshaping one.
3. Where a field has to change, prefer a rename or a lossless type change
   (MINOR) to a removal (MAJOR).
4. Pin documents with `@MAJOR.MINOR`, as the quill's seeded, empty and example
   documents, and a document built from the authoring form, do: every patch in that series reads them
   unchanged.

## Related Pages

- [Creating Quills](creating-quills.md)
- [Quill.yaml Reference](quill-yaml-reference.md)
- [card-yaml Blocks](../authoring/card-yaml.md)

Full model: [VERSIONING.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/VERSIONING.md).
