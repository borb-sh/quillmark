//! Two classes, on either side of the boundary.
//!
//! A refusal the engine minted — an edit, a render, a validation — raises
//! `QuillmarkError` carrying a non-empty `.diagnostics` list; the
//! `EditError::<Variant>` prefix lives in the message, not the type.
//!
//! An argument the binding cannot convert at all — a non-finite float, an int
//! past 64 bits, a type with no JSON form, a malformed `path` sequence, a dict
//! whose shape is not the one the surface reads — raises `ValueError` before the
//! engine is called. No diagnostic describes it, and `ValueError` is what a
//! Python caller catches for its own argument. The WASM binding has one shape
//! for both.
//!
//! The line runs between shape and content: a card dict that will not
//! deserialize is a `ValueError`, while one that deserializes and then violates
//! an invariant — a malformed field name, a `$quill` that is not a reference, a
//! `!must_fill` on a mapping — is the engine's refusal and raises
//! `QuillmarkError` under its code.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use quillmark_core::{Diagnostic, EditError, RenderError, Severity};

create_exception!(_quillmark, QuillmarkError, PyException);

pub fn convert_edit_error(err: EditError) -> PyErr {
    let diagnostic =
        Diagnostic::new(Severity::Error, err.to_string())
            .with_code(err.code().to_string())
            .with_args(err.args());
    let message = diagnostic.message.clone();
    raise_with_diagnostics(vec![diagnostic], message)
}

/// A card the wire refuses, under the code the addressed mutator onto the same
/// violation mints. The card is not placed, so the diagnostic carries no `path`.
pub fn convert_wire_error(err: quillmark_core::WireError) -> PyErr {
    let diagnostic = err.to_diagnostic();
    let message = diagnostic.message.clone();
    raise_with_diagnostics(vec![diagnostic], message)
}

/// Batched twin of [`convert_edit_error`]: one diagnostic per offending field,
/// its `path` the field name.
pub fn convert_edit_errors(errors: Vec<(String, EditError)>) -> PyErr {
    let diags: Vec<Diagnostic> = errors
        .into_iter()
        .map(|(name, err)| {
            Diagnostic::new(Severity::Error, err.to_string())
                .with_code(err.code().to_string())
                .with_args(err.args())
                .with_path(name)
        })
        .collect();
    let message = RenderError::summary_message(&diags);
    raise_with_diagnostics(diags, message)
}

/// The [`convert_edit_errors`] twin for a batch spanning cards, where each
/// refusal carries the whole `DocPath` it anchors at rather than a field name.
pub fn convert_edit_errors_at(errors: Vec<(quillmark_core::DocPath, EditError)>) -> PyErr {
    convert_edit_errors(
        errors
            .into_iter()
            .map(|(path, err)| (path.to_string(), err))
            .collect(),
    )
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
