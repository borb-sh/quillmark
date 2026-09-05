//! Two classes, on either side of the boundary.
//!
//! A refusal the engine minted — an edit, a render, a validation — raises
//! `QuillmarkError` carrying a non-empty `.diagnostics` list; the
//! `EditError::<Variant>` prefix lives in the message, not the type.
//!
//! An argument the binding cannot convert at all — a non-finite float, an int
//! past 64 bits, a type with no JSON form, a malformed `path` sequence — raises
//! `ValueError` before the engine is called. No diagnostic describes it, and
//! `ValueError` is what a Python caller catches for its own argument. The WASM
//! binding has one shape for both.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use quillmark_core::{Diagnostic, EditError, RenderError, Severity};

create_exception!(_quillmark, QuillmarkError, PyException);

/// One diagnostic, its `path` the [`DocPath`](quillmark_core::DocPath) the
/// error anchors at relative to `base`: the card root the mutator ran against,
/// empty for a card built before placement.
pub fn convert_edit_error(err: EditError, base: &quillmark_core::DocPath) -> PyErr {
    let mut diagnostic =
        Diagnostic::new(Severity::Error, err.to_string())
            .with_code(err.code().to_string())
            .with_args(err.args());
    if let Some(path) = err.doc_path(base) {
        diagnostic = diagnostic.with_path(path.to_string());
    }
    let message = diagnostic.message.clone();
    raise_with_diagnostics(vec![diagnostic], message)
}

/// Batched twin of [`convert_edit_error`]: one diagnostic per offending field,
/// each anchored at its name under `base`.
pub fn convert_edit_errors(
    errors: Vec<(String, EditError)>,
    base: &quillmark_core::DocPath,
) -> PyErr {
    convert_edit_errors_at(
        errors
            .into_iter()
            .map(|(name, err)| (base.field(&name), err))
            .collect(),
    )
}

/// The [`convert_edit_errors`] twin for a batch spanning cards, where each
/// refusal carries the whole `DocPath` it anchors at rather than a field name
/// under one base.
pub fn convert_edit_errors_at(errors: Vec<(quillmark_core::DocPath, EditError)>) -> PyErr {
    let diags: Vec<Diagnostic> = errors
        .into_iter()
        .map(|(path, err)| {
            Diagnostic::new(Severity::Error, err.to_string())
                .with_code(err.code().to_string())
                .with_args(err.args())
                .with_path(path.to_string())
        })
        .collect();
    let message = RenderError::summary_message(&diags);
    raise_with_diagnostics(diags, message)
}

/// The message is the primary diagnostic's for a single diagnostic, an
/// `"<N> error(s): <first>"` aggregate for more.
pub fn convert_render_error(err: RenderError) -> PyErr {
    debug_assert!(
        !err.diagnostics().is_empty(),
        "RenderError always carries at least one diagnostic"
    );
    let message = err.to_string();
    raise_with_diagnostics(err.into_diagnostics(), message)
}

pub fn raise_with_diagnostics(diags: Vec<Diagnostic>, message: String) -> PyErr {
    Python::attach(|py| {
        let py_err = QuillmarkError::new_err(message);
        let py_diags: Vec<crate::types::PyDiagnostic> = diags
            .into_iter()
            .map(|d| crate::types::PyDiagnostic { inner: d })
            .collect();
        let _ = py_err.value(py).as_any().setattr("diagnostics", py_diags);
        py_err
    })
}
