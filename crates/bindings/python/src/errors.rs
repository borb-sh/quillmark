//! A refusal the engine minted raises `QuillmarkError` carrying a non-empty
//! `.diagnostics` list; an argument the binding cannot convert at all raises
//! `ValueError` before the engine is called, described by no diagnostic. The line
//! runs between shape and content: a card dict that will not deserialize is a
//! `ValueError`, one that deserializes and then violates an invariant is the
//! engine's refusal.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use quillmark_core::{Diagnostic, EditError, RenderError, Severity};
use std::collections::BTreeMap;

create_exception!(_quillmark, QuillmarkError, PyException);

/// Resolve a card index the caller passed, refusing a negative one whatever
/// `len` is; a non-negative index past the end is the calling verb's own to
/// answer.
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
            "backend::page_index_out_of_bounds",
            format!(
                "Page index out of bounds; offending indices: {negative:?}. Page indices are 0-based and count from the first page."
            ),
        )));
    }
    Ok(pages.into_iter().map(|p| p as usize).collect())
}

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

/// A card the wire refuses, under the code the addressed mutator onto the same
/// violation mints. The card is not placed, so the diagnostic carries no `path`.
pub fn convert_wire_error(err: quillmark_core::WireError) -> PyErr {
    let diagnostic = err.to_diagnostic();
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

/// The [`convert_edit_errors`] arm for refusals that carry the whole `DocPath`
/// they anchor at rather than a field name under one base.
fn convert_edit_errors_at(errors: Vec<(quillmark_core::DocPath, EditError)>) -> PyErr {
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
