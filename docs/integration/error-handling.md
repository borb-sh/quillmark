# Error Handling

Every failure (parse, validation, quill config, backend compile) travels as a **`Diagnostic`**, and each binding raises a single error type that always carries a non-empty `diagnostics` list. Consumers route on a diagnostic's namespaced `code`, never on an exception subclass.

One thing never reaches a diagnostic: an argument the binding cannot convert at all. Python raises `ValueError` for a non-finite float, an int past 64 bits, a value whose type has no JSON form, or a malformed `path` sequence — the engine never sees the call.

## The Diagnostic shape

| Field | Meaning |
|---|---|
| `severity` | `Error` (blocks its stage) or `Warning` (never blocks) |
| `code` | Namespaced id; e.g. `parse::missing_quill`, `validation::type_mismatch`, `quill::version_mismatch`: the machine-routable identity |
| `message` | Human-readable text |
| `location` | Optional text anchor: `file`, `line` (1-based), `column` |
| `path` | Optional document-model anchor: `main.recipient`, `cards.indorsement[0].author` |
| `hint` | Optional actionable suggestion |

`location` (where in the source text) and `path` (which field in the model) are independent and may co-exist.

## Catching errors

=== "Python"

    ```python
    from quillmark import QuillmarkError

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
    try {
      const result = await engine.render(quill, doc, { format: "pdf" });
    } catch (err) {
      for (const d of err.diagnostics) {     // always non-empty
        console.error(d.severity, d.code, d.message);
      }
    }
    ```

A multi-problem stage (validation, quill config, backend compile) reports **every** problem in one pass, so `diagnostics` may carry several entries; `diagnostics[0]` is the primary. The error's `message` follows a count-based rule: the primary message for one diagnostic, `"<N> error(s): <first message>"` for more.

## Codes, not types

The `code` namespaces are the routing surface:

`parse::*` · `validation::*` · `quill::*` · `edit::*` (mutators) · `typst::*` · `pdfform::*` · `pdf::*` (the AcroForm stamping spine both PDF-producing backends share) · `backend::*` · `engine::*`.

Notable codes: `quill::name_mismatch` / `quill::version_mismatch` (a well-formed document paired with the wrong quill; see [Versioning](../quills/versioning.md)); `engine::backend_not_found` (the quill's declared backend is not registered); `parse::input_too_large`, which carries the two byte-sized [§8 caps](../reference/markdown-spec.md#8-limits) — document size and YAML payload size — distinguished only by its `max` arg, while the count caps arrive as `parse::too_many_cards` and `parse::too_many_fields` (args `count`, `max`) and an over-deep payload as `parse::yaml_error_with_location`.

A Typst compile classifies into four codes: `typst::file_not_found` (a file the quill's world refused — a missing asset is the common one), `typst::unknown_variable`, `typst::type_error`, and `typst::compile` for everything else, warnings included. They are a routing key only: which file was searched for, or which symbol was unknown, is read from `message`.

## Warnings vs errors

Fatality is a two-value ladder: `Error` blocks the stage that emits it; `Warning` never does. There is no lint-level configuration and no warning-to-error promotion. Warnings ride the same `Diagnostic` currency on non-fatal channels:

- **Parse warnings** (e.g. a `~~~` opener missing its blank line) carried on the parsed document (`doc.warnings`) and spliced into a render's warnings.
- **Validation warnings**: `quill.validate(doc)` returns every diagnostic; `validation::must_fill` and the `$seed` checks are the non-fatal ones. `validation::must_fill` fires on two triggers, named by its `trigger` arg: `marker`, an outstanding `!must_fill` tag in the document, and `unauthored`, a cell the schema obliges (one with no `default:`) that nobody has authored. At most one per path; the marker wins where both apply. The render path never gates on either: an absent field blank-fills.
- **Compile warnings**: a backend's non-fatal diagnostics (font fallback, overfull pages), carried on `result.warnings`.

A successful render returns artifacts **and** a `warnings` list, so inspect it even on success.

Full model: [ERROR.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/ERROR.md).
