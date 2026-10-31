# Error Handling

Every failure (parse, validation, quill config, backend compile) travels as a **`Diagnostic`**, and each binding raises a single error type that always carries a non-empty `diagnostics` list. Consumers route on a diagnostic's namespaced `code`, never on an exception subclass.

One thing never reaches a diagnostic: an argument the binding cannot convert at all. Python raises `ValueError` for a non-finite float, an int past 64 bits, a value whose type has no JSON form, or a malformed `path` sequence — core never sees the call.

## The Diagnostic shape

| Field | Meaning |
|---|---|
| `severity` | `Error` (blocks its stage) or `Warning` (never blocks): `Severity.ERROR` / `Severity.WARNING` in Python, `"error"` / `"warning"` in JavaScript |
| `code` | Namespaced id; e.g. `parse::missing_quill`, `validation::type_mismatch`, `quill::version_mismatch`: the machine-routable identity |
| `message` | Human-readable text |
| `location` | Optional text anchor: `file`, `line` (1-based), `column` |
| `path` | Optional document-model anchor: `main.recipient`, `cards.indorsement[0].author` (the document's first card, an `indorsement`; [card numbering](../quills/editor-regions.md#the-address-an-app-reads)) |
| `hint` | Optional actionable suggestion |

`location` (where in the source text) and `path` (which field in the model) are independent and may co-exist.

## Catching errors

=== "Python"

    ```python
    from quillmark import Engine, OutputFormat, QuillmarkError

    engine = Engine()
    try:
        result = engine.render(quill, doc, OutputFormat.PDF)
    except QuillmarkError as exc:
        for d in exc.diagnostics:            # always non-empty
            print(d.severity, d.code, d.message)
            if d.path:
                print("  at", d.path)
    ```

=== "JavaScript"

    ```javascript
    import { isQuillmarkError } from "@quillmark/wasm";

    try {
      const result = await engine.render(quill, doc, { format: "pdf" });
    } catch (err) {
      if (!isQuillmarkError(err)) throw err;
      for (const d of err.diagnostics) {     // always non-empty
        console.error(d.severity, d.code, d.message);
      }
    }
    ```

A multi-problem stage (validation, quill config, backend compile) reports **every** problem in one pass, so `diagnostics` may carry several entries; `diagnostics[0]` is the primary, and an error. A `Warning` after the errors is one raised before the failure that can explain it ([below](#warnings-vs-errors)). The error's `message` follows a count-based rule: the primary message for one error, `"<N> error(s): <first message>"` for more, warnings not counted.

## Codes, not types

The `code` namespaces are the routing surface:

`parse::*` · `validation::*` · `quill::*` · `edit::*` (mutators) · `conform::*` (`Quill::conform` warnings) · `typst::*` · `acroform::*` · `pdf::*` (the AcroForm stamping spine both PDF-producing backends share) · `backend::*` · `engine::*` · `cli::*` · `runtime::*` (the JavaScript runtime).

[Diagnostic Codes](../reference/diagnostic-codes.md) lists every code.

Notable codes: `quill::name_mismatch` / `quill::version_mismatch` (a well-formed document paired with the wrong quill; see [Versioning](../quills/versioning.md)); `engine::backend_not_found` (the quill's declared backend is not registered); `parse::input_too_large`, which carries the two byte-sized [§8 caps](../reference/markdown-spec.md#8-limits) — document size and YAML payload size — distinguished only by its `max` arg, while the count caps arrive as `parse::too_many_cards` and `parse::too_many_fields` (args `count`, `max`) and an over-deep payload as `parse::yaml_error_with_location`.

`parse::payload_not_mapping` is a card-yaml block whose payload is YAML but not a mapping, usually code fenced with `~~~` in a body. It is located at the fence's line. Its `args` carry `actual` (`string`, `number`, `boolean` or `sequence`) and, when the fence has one, `info`, the info string.

`parse::missing_kind` is a card-yaml block after the root whose payload names no `$kind`: a card missing its kind line, or code fenced with `~~~` whose text reads as a mapping. It is located at the fence's line, and its `args` carry `info` when the fence has one.

A Typst compile classifies into four codes: `typst::file_not_found` (a file the quill's world refused — a missing asset is the common one), `typst::unknown_variable`, `typst::type_error`, and `typst::compile` for everything else, warnings included. They are a routing key only: which file was searched for, or which symbol was unknown, is read from `message`.

Before any compile, opening a Typst quill can refuse its plate: `typst::plate_missing` when the quill holds no `plate.typ` and declares no `typst.plate_file`, or `typst.plate_file` names no file in the quill, `typst::plate_path_invalid` when `typst.plate_file` is not a string, names no path, or names a file at a path Typst cannot load, and `typst::helper_import`, one per import, when any `.typ` file of the quill outside `packages/` imports the helper package any way but `#import "@local/quillmark-helper:0.2.0" as qm`, whether or not the plate imports that file.

## Warnings vs errors

Fatality is a two-value ladder: `Error` blocks the stage that emits it; `Warning` never does. There is no lint-level configuration and no warning-to-error promotion. Warnings ride the same `Diagnostic` currency on non-fatal channels:

- **Parse warnings** (e.g. a `~~~` opener missing its blank line) carried on the parsed document (`doc.warnings`). A render does not carry them: they report the load, and a revise can change what the document holds after it.
- **`parse::dropped_construct`**: markdown the content cannot carry, one per construct, carrying `construct` and `count` in `args`. `construct` is a raw tag's lowercase name, such as `div` or an HTML table's `caption`, or the `qm-*` tag that dropped: `qm-keep` for a tag it ignores, `qm-span[color]` for an attribute. An HTML table reports `table` where its block ends before its `</table>`, `table:row` for a row too short for it, `table:text` for text outside every cell, `table:size` for the rows of a table past 100 000 cells, and `<tag>[<attr>]`, such as `td[nowrap]` or `td[colspan]`, for an attribute it does not read other than `border`, `class`, `scope` and `style`, which drop silently.
    - A parse anchors it at the body (`main.body`, `cards.<kind>[<i>].body`) on `doc.warnings`. A `richtext` field's markdown string reports at the field (`main.subject`, `cards.<kind>[<i>].items[0]`): on `doc.warnings` from `quill.parse`, and from `quill.validate(doc)` on a document loaded without its quill. A typed `set` / `setAll` returns them at the field, and `addCard` its fields' ahead of its body's.
    - `doc.revise` returns it in its receipt's `warnings`, and the writer's `reviseBody` / `reviseField` in `{ delta, warnings, droppedAnchors }`, anchored at the address written; Python's `revise_body` / `revise_field` return it among their dict receipt's `warnings`, as `Diagnostic` objects.
    - The writer's `reviseDocument` returns it among its receipt's `warnings`, and so does Python's `revise_document`.
    - `addCard` returns it as a list, anchored under the placed card; `importMarkdown` returns it beside the content, with no path.
- **Validation warnings**: `quill.validate(doc)` returns every diagnostic, and its severity says whether the document renders ([full rule](https://github.com/borb-sh/quillmark/blob/main/prose/canon/SCHEMAS.md#what-blocks-a-render)):
    - An `Error` is input Quillmark cannot read as written, such as a value that is not its field's type or an enum value outside `values:`. The render fails.
    - A `Warning` is input no declaration claims: a card with an undeclared `$kind` (`validation::unknown_card`), a body under `body.enabled: false` (`validation::body_disabled`), a key the schema does not declare (`validation::unknown_field`, whose hint names the likely fix), a stranded variant cell (`validation::out_of_variant`), elements past `max:` (`validation::cardinality`), characters past a text field's `max:` (`validation::over_length`), and the `$seed` checks. The document renders without it. A one-shot render carries these warnings on `result.warnings`, ahead of the compile's; a live session's warnings do not, so an editor reads `quill.validate(doc)` beside them.

    A line break inside a one-line `text` value warns too (`validation::line_break_folded`), and renders as a space.

    A field the document leaves unanswered draws no diagnostic: it renders its `default:`, else its blank.

    On the CLI, `quillmark render` and `quillmark check` both print every validation warning ([CLI Reference](../cli/reference.md#check)).
- **Compile warnings**: a backend's non-fatal diagnostics (font fallback, overfull pages), carried on `result.warnings`.
- **Load warnings**: what the Typst backend skipped loading the quill (`typst::package_manifest`, `typst::package_entrypoint_missing`, `typst::path_skipped`, `typst::font_collection_skipped`), carried ahead of the compile warnings. A render or session `open` whose compile fails carries them too, in the error's `diagnostics` after its errors: an `#import` of a package skipped for its manifest fails as `typst::file_not_found`, and only the warning says why.
- **`backend::declined_construct`**: a construct the backend typesets nothing for, one per content field, carrying `backend`, `construct` and `count` in `args` and the field's path. The Typst backend declines `image`: a markdown image in a `richtext` field reaches no page, because no backend resolves a content image's url. The acroform backend declines every construct but the paragraph. `quill.validate(doc)` returns the same warnings before any render.

A successful render returns artifacts **and** a `warnings` list, so inspect it even on success.

Full model: [ERROR.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/ERROR.md).
