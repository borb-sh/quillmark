//! The content model across the Python boundary: `Content` and `Change` dicts,
//! offsets in code points, which are storage's own scalar values. The write
//! primitive and its producers are `Document` methods; the pure helpers over
//! changes are module functions.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use quillmark_content::change::{self, Change};
use quillmark_content::model::Content;
use quillmark_content::wire::ParseError;
use quillmark_core::document::{Document, Edit, EditError, Receipt, Target, check_change, check_content_value};
use quillmark_core::path::DocPath;

use crate::errors::convert_edit_error;
use crate::types::{json_to_py, py_diagnostics, py_to_json};

fn change_error(e: change::ApplyError) -> PyErr {
    convert_edit_error(EditError::change(e), &DocPath::new())
}

pub(crate) fn content_from_py(value: &Bound<'_, PyAny>, ctx: &str) -> PyResult<Content> {
    let json = py_to_json(value)?;
    if !json.is_object() {
        return Err(PyValueError::new_err(format!("{ctx}: expected a Content dict")));
    }
    check_content_value(&json).map_err(|e| convert_edit_error(e, &DocPath::new()))?;
    Content::from_value(&json).map_err(|e| match &e {
        ParseError::Invalid(inv) if EditError::bound(inv).is_some() => {
            convert_edit_error(EditError::bound(inv).expect("a bound"), &DocPath::new())
        }
        _ => PyValueError::new_err(format!("{ctx}: not a Content: {e}")),
    })
}

pub(crate) fn change_from_py(value: &Bound<'_, PyAny>, ctx: &str) -> PyResult<Change> {
    let change =
        Change::from_value(&py_to_json(value)?).map_err(|e| PyValueError::new_err(format!("{ctx}: not a Change: {e}")))?;
    check_change(&change).map_err(|e| convert_edit_error(e, &DocPath::new()))?;
    Ok(change)
}

pub(crate) fn content_to_py<'py>(py: Python<'py>, c: &Content) -> PyResult<Bound<'py, PyAny>> {
    json_to_py(py, &c.to_value())
}

/// Run one write on `card` (`None` the main card) of `doc`, its receipt as it
/// crosses.
pub(crate) fn write<'py>(
    py: Python<'py>,
    doc: &mut Document,
    card: Option<usize>,
    base_path: &DocPath,
    field: Option<&str>,
    edit: Edit<'_>,
    base: Option<&str>,
) -> PyResult<Bound<'py, PyDict>> {
    let path = match field {
        None => base_path.body(),
        Some(f) => base_path.field(f),
    };
    let mut card = match card {
        None => doc.main_mut(),
        Some(i) => doc.card_mut(i).expect("the card index is resolved"),
    };
    let to_py = |e: EditError| convert_edit_error(e, base_path);
    let receipt: Receipt = card.write(Target::from(field), edit, base).map_err(to_py)?;
    let card_id = card.id().map(str::to_string);
    let warnings = py_diagnostics(receipt.diagnostics(Some(&path)));
    let dropped: Vec<_> = receipt
        .dropped_anchors
        .into_iter()
        .map(|a| {
            let mut entry = serde_json::json!({
                "id": a.context.id,
                "path": path.to_string(),
                "card_id": card_id,
                "quote": a.context.quote,
                "prefix": a.context.prefix,
                "suffix": a.context.suffix,
            });
            if let Some(slot) = a.slot {
                entry["island"] = slot.island.into();
                entry["slot"] = slot.slot.into();
            }
            entry
        })
        .collect();
    let moved: Vec<_> = receipt
        .moved_anchors
        .into_iter()
        .map(|m| serde_json::json!({ "id": m.id, "before": m.before, "after": m.after, "reason": m.reason.name() }))
        .collect();
    let out = PyDict::new(py);
    out.set_item("applied", json_to_py(py, &receipt.applied.to_value())?)?;
    out.set_item("hash", receipt.hash)?;
    out.set_item("dropped_anchors", json_to_py(py, &serde_json::Value::Array(dropped))?)?;
    out.set_item("moved_anchors", json_to_py(py, &serde_json::Value::Array(moved))?)?;
    out.set_item("warnings", warnings)?;
    out.set_item("rebased", receipt.rebased)?;
    Ok(out)
}

/// The change that undoes `change` on `before`. Raises `QuillmarkError`
/// (`edit::content_apply`) where `change` does not apply to `before`.
#[pyfunction]
pub fn invert<'py>(py: Python<'py>, change: &Bound<'_, PyAny>, before: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let before = content_from_py(before, "invert")?;
    let change = change_from_py(change, "invert")?;
    let undo = change::invert(&change, &before).map_err(change_error)?;
    json_to_py(py, &undo.to_value())
}

/// A change taking `before` to `after`, word-aligned, for a writer with no
/// steps of its own. Raises `QuillmarkError` (`edit::content_apply`) where
/// `after` is not a valid value.
#[pyfunction]
pub fn diff<'py>(py: Python<'py>, before: &Bound<'_, PyAny>, after: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let (before, after) = (content_from_py(before, "diff")?, content_from_py(after, "diff")?);
    let change = quillmark_content::diff::diff(&before, &after).map_err(|e| change_error(e.into()))?;
    json_to_py(py, &change.to_value())
}

/// The valid value every peer holding the merged state `raw` reads, what was
/// mended to reach it, and the change from `raw`'s text to the value's, which
/// a peer maps its positions through: `{"content", "repairs", "change"}`.
/// Raises `ValueError` only on a `raw` not shaped as content.
#[pyfunction]
pub fn repair<'py>(py: Python<'py>, raw: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let json = py_to_json(raw)?;
    check_content_value(&json).map_err(|e| convert_edit_error(e, &DocPath::new()))?;
    let raw = quillmark_content::repair::Raw::from_value(&json)
        .map_err(|e| PyValueError::new_err(format!("repair: not shaped as content: {e}")))?;
    let (content, repairs, change) = quillmark_content::repair::repair(raw);
    let out = PyDict::new(py);
    out.set_item("content", content_to_py(py, &content)?)?;
    let repairs: Vec<serde_json::Value> = repairs.iter().map(quillmark_content::repair::Repair::to_value).collect();
    out.set_item("repairs", json_to_py(py, &serde_json::Value::Array(repairs))?)?;
    out.set_item("change", json_to_py(py, &change.to_value())?)?;
    Ok(out)
}
