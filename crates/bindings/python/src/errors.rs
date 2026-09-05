//! Two classes, on either side of the boundary.
//!
//! A refusal the engine minted — an edit, a render, a validation — raises
//! `QuillmarkError` carrying a non-empty `.diagnostics` list; the
//! `EditError::<Variant>` prefix lives in the message, not the type.
//!
//! A negative index is a refusal of that first kind: addressing nothing is what
//! an out-of-range index is, so it raises under the code an index past the end
//! carries.
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
use std::collections::BTreeMap;

create_exception!(_quillmark, QuillmarkError, PyException);

/// Resolve a card index the caller passed. Indices count from the front, so a
/// negative one is out of range whatever `len` is; a non-negative index past the
/// end is the calling verb's own to answer.
///
/// Index parameters are signed for this: a `usize` parameter refuses a negative
/// int in extraction, raising the `OverflowError` no contract here mentions. The
/// diagnostic is minted rather than taken from [`convert_edit_error`] because
/// `EditError::IndexOutOfRange` carries a `usize` and cannot hold the index.
pub fn card_index(index: isize, len: usize) -> PyResult<usize> {
    usize::try_from(index).map_err(|_| {
        let mut args = BTreeMap::new();
        args.insert("index".to_string(), serde_json::json!(index));
        args.insert("len".to_string(), serde_json::json!(len));
        let message = format!("index {index} is out of range (len = {len})");
        let diagnostic = Diagnostic::new(Severity::Error, message.clone())
            .with_code("edit::index_out_of_range".to_string())
            .with_args(args);
        raise_with_diagnostics(vec![diagnostic], message)
    })
}

/// Resolve the `pages` selection the caller passed. Page indices are 0-based and
/// count from the first page, so a negative one selects no page; the refusal
/// carries the code the backend mints for a page past the last.
pub fn page_indices(pages: Vec<isize>) -> PyResult<Vec<usize>> {
    let negative: Vec<isize> = pages.iter().copied().filter(|&p| p < 0).collect();
    if !negative.is_empty() {
        return Err(convert_render_error(RenderError::coded(
            "typst::page_index_out_of_bounds",
            format!(
                "Page index out of bounds; offending indices: {negative:?}. Page indices are 0-based and count from the first page."
            ),
        )));
    }
    Ok(pages.into_iter().map(|p| p as usize).collect())
}

pub fn convert_edit_error(err: EditError) -> PyErr {
    let diagnostic =
        Diagnostic::new(Severity::Error, err.to_string())
            .with_code(err.code().to_string())
            .with_args(err.args());
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
