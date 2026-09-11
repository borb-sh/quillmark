# Migration Guides

A release that breaks the document syntax, the plate-JSON wire format, or a
public API ships a guide for that version step. Most breaks are hard cutovers —
the old form stops parsing or compiling — so the guide is the path forward, not
an optional read.

Each guide covers one step. To cross several versions, work through them in
order; each states its own breaks in full. The rows below name the step's
headline break, enough to pick the guide.

No step requires migrating a stored document: the storage DTO's bytes and tag
hold across every step here except 0.111 → 0.112, where canonical bytes move
(content hashes recompute) and the tag becomes `quillmark/document@0.112.0`.

## Guides

| Step | What changes |
|---|---|
| [0.112 → 0.113](0.112-to-0.113.md) | The `pdfform` backend becomes `acroform` at every layer — crate, `backend:` id, feature, `pdfform::*` codes — with no alias, so every form quill's `Quill.yaml` stops rendering until edited. The content vocabularies close: a line `kind`, container, mark `type`, island `type` or `loss` outside the built-ins stops opening a stored row. Every column-zero `~~~` block is a card whatever its info string, so a `~~~rust` fence in a body opens one where it opened a code block — a backtick fence is the escape hatch. A content line op the text or block contradicts lands and the terminal normalize settles it, so `Ok` stops meaning the op landed as written. Also `reader.get` in the values form, `reader.resolve()`, the `toStored` / `fromStored` / `loadStored` renames, and a wide pruning sweep. |
| [0.111 → 0.112](0.111-to-0.112.md) | One spelling per name: every vocabulary member carries its payload in `attrs`, built-ins included, so a host reading `line.level` or `mark.url` reads them under `attrs` and the authored lane refuses the old spelling. |
| [0.110 → 0.111](0.110-to-0.111.md) | A fix release: four surfaces refuse input they were producing something wrong from. The one break that can stop a render is a pdfform background carrying its own `/AcroForm` (`pdf::existing_acroform`). |
| [0.109 → 0.110](0.109-to-0.110.md) | Every hand-built container literal must spell `instance`: the discriminator becomes a required field on the whole-document write lanes, and `ContentContainerInput` is deleted. |
| [0.108 → 0.109](0.108-to-0.109.md) | Canonical form becomes a type — `Normalized`, which every codec returns and every projection takes — and `Container` gains `instance`, so an exhaustive `match` needs `..`. |
| [0.107 → 0.108](0.107-to-0.108.md) | A cell holds a value, a namespace holds cells: a `default:` / `example:` on a typed dictionary fails load, an absent container reaches its properties' defaults (a render-output change), and `must_fill:` retires. |
| [0.106 → 0.107](0.106-to-0.107.md) | A declared type means the same thing at every depth. The break no checker reports: a plate read of a typed-table row cell regions on the cell (`main.refs[0].org`). A `date` lowers to a native `datetime`. |
| [0.105 → 0.106](0.105-to-0.106.md) | A schema address may step one property into a declared container — so region addresses shift — and an `enum` may declare `variants:`, which rests that field as `{value: <member>, …}` rather than a bare string. |
| [0.104 → 0.105](0.104-to-0.105.md) | A field's blank becomes a property of the field rather than a member of its type's domain: an absent no-`default:` enum renders blank instead of `values.first()`, and every enum accepts `""` as input. |
| [0.103 → 0.104](0.103-to-0.104.md) | `type: enum` with `values:` is the one spelling of a closed string domain (`enum:` is a load error), and a geometry address spells an array element bracketed: `main.references[0]`. |
| [0.102 → 0.103](0.102-to-0.103.md) | WASM: `init()` resolves to the core surface, and `Quill`, `Document` and the free functions leave the static exports — destructure them from `await init()`. CLI: `render --verbose` writes its progress to stderr. |
| [0.101 → 0.102](0.101-to-0.102.md) | The pre-1.0 vocabulary reset: `install*` → `overwrite*`, `setBody` → `reviseBody`, `LiveSession.apply` → `update`, and the `edit::*` codes rename. `@quillmark/wasm` ships `--target web`, so `await init()` is required. |
| [0.100 → 0.101](0.100-to-0.101.md) | Two Rust constructors leave the published surface (`Document::from_main_and_cards`, `QuillConfig::from_yaml`), and the change-bundle verbs take one `ChangeBundle` struct, which gains an island channel. |

## Related

For how Quills themselves are versioned and how authors target a version, see
[Quill Versioning](../quills/versioning.md).
