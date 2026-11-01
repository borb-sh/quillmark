//! The content model across the Python boundary: `Content` and `Change` dicts,
//! offsets in code points, which are storage's own scalar values. The write
//! primitive and its producers are `Document` methods; the pure helpers over
//! changes are module functions.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use quillmark_content::change::{self, Assoc, Change};
use quillmark_content::model::Content;
use quillmark_core::document::{Document, EditError, Receipt, Target};
use quillmark_core::path::DocPath;

use crate::errors::convert_edit_error;
use crate::types::{json_to_py, py_diagnostics, py_to_json};

/// How a write builds its change from the value it read.
pub(crate) enum Edit<'a> {
    Apply(Change),
    Revise(&'a str),
    Replace(Content),
}

pub(crate) fn target(field: Option<&str>) -> Target<'_> {
    match field {
        None => Target::Body,
        Some(name) => Target::Field(name),
    }
}

fn change_error(e: change::ApplyError) -> PyErr {
    convert_edit_error(EditError::Change(e), &DocPath::new())
}

pub(crate) fn content_from_py(value: &Bound<'_, PyAny>, ctx: &str) -> PyResult<Content> {
    let json = py_to_json(value)?;
    if !json.is_object() {
        return Err(PyValueError::new_err(format!("{ctx}: expected a Content dict")));
    }
    Content::from_value(&json).map_err(|e| PyValueError::new_err(format!("{ctx}: not a Content: {e}")))
}

pub(crate) fn change_from_py(value: &Bound<'_, PyAny>, ctx: &str) -> PyResult<Change> {
    Change::from_value(&py_to_json(value)?).map_err(|e| PyValueError::new_err(format!("{ctx}: not a Change: {e}")))
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
    let receipt: Receipt = match edit {
        Edit::Apply(change) => card.apply(target(field), &change, base),
        Edit::Revise(markdown) => card.revise(target(field), markdown, base),
        Edit::Replace(content) => card.replace(target(field), &content, base),
    }
    .map_err(to_py)?;
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
        .map(|m| serde_json::json!({ "id": m.id, "before": m.before, "after": m.after }))
        .collect();
    let out = PyDict::new(py);
    out.set_item("applied", json_to_py(py, &receipt.applied.to_value())?)?;
    out.set_item("hash", receipt.hash)?;
    out.set_item("dropped_anchors", json_to_py(py, &serde_json::Value::Array(dropped))?)?;
    out.set_item("moved_anchors", json_to_py(py, &serde_json::Value::Array(moved))?)?;
    out.set_item("warnings", warnings)?;
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

/// `a` then `b` as one change: their steps, adjacent splices that touch
/// merged.
#[pyfunction]
pub fn compose<'py>(py: Python<'py>, a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let (a, b) = (change_from_py(a, "compose")?, change_from_py(b, "compose")?);
    json_to_py(py, &change::compose(&a, &b).to_value())
}

/// Where `pos` lands after `change`, and what around it the change deleted,
/// as `{"pos", "deleted", "deleted_before", "deleted_after",
/// "deleted_across"}`. `assoc`, `"before"` or `"after"`, is the side of an
/// insertion or a deletion at `pos` it keeps to. Raises `QuillmarkError`
/// (`edit::content_apply`) for a splice that deletes an island and names none
/// in `removed`: map the receipt's `applied`.
#[pyfunction]
pub fn map_pos<'py>(
    py: Python<'py>,
    change: &Bound<'_, PyAny>,
    pos: &Bound<'_, PyAny>,
    assoc: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let change = change_from_py(change, "map_pos")?;
    let pos = quillmark_content::wire::pos_from_value(&py_to_json(pos)?)
        .map_err(|e| PyValueError::new_err(format!("map_pos: not a Pos: {e}")))?;
    let assoc = match assoc {
        "before" => Assoc::Before,
        "after" => Assoc::After,
        _ => return Err(PyValueError::new_err("map_pos: assoc must be \"before\" or \"after\"")),
    };
    let r = change::map_pos(&change, &pos, assoc).map_err(change_error)?;
    let out = PyDict::new(py);
    out.set_item("pos", json_to_py(py, &quillmark_content::wire::pos_value(&r.pos))?)?;
    out.set_item("deleted", r.deleted)?;
    out.set_item("deleted_before", r.deleted_before)?;
    out.set_item("deleted_after", r.deleted_after)?;
    out.set_item("deleted_across", r.deleted_across)?;
    Ok(out)
}

/// A change taking `before` to `after`, word-aligned, for a writer with no
/// steps of its own, as `{"change", "warnings"}`: an `edit::whole_text_diff`
/// warning where it rewrote a whole text as one splice.
#[pyfunction]
pub fn diff<'py>(py: Python<'py>, before: &Bound<'_, PyAny>, after: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let (before, after) = (content_from_py(before, "diff")?, content_from_py(after, "diff")?);
    let diffed = quillmark_content::diff::diff(&before, &after).map_err(|e| change_error(e.into()))?;
    let out = PyDict::new(py);
    out.set_item("change", json_to_py(py, &diffed.change.to_value())?)?;
    out.set_item("warnings", py_diagnostics(diffed.whole_text.then(quillmark_core::document::whole_text_diff).into_iter().collect()))?;
    Ok(out)
}

/// Enter at `at`: the change that splits the block holding `at`, the new
/// block `new_id` taking its kind and path while the text keeps its id. Raises
/// `QuillmarkError` (`edit::content_apply`) where `at` is past the text.
#[pyfunction]
pub fn split_block<'py>(py: Python<'py>, content: &Bound<'_, PyAny>, at: usize, new_id: &str) -> PyResult<Bound<'py, PyAny>> {
    let content = content_from_py(content, "split_block")?;
    let change = quillmark_content::gestures::split_block(&content, at, new_id).map_err(change_error)?;
    json_to_py(py, &change.to_value())
}

/// Backspace at the start of block `id`: the change that joins it into the
/// block before it, which keeps its id and kind. Raises `QuillmarkError`
/// (`edit::content_apply`) where no block holds `id`, and for the first block.
#[pyfunction]
pub fn join_blocks<'py>(py: Python<'py>, content: &Bound<'_, PyAny>, id: &str) -> PyResult<Bound<'py, PyAny>> {
    let content = content_from_py(content, "join_blocks")?;
    let change = quillmark_content::gestures::join_blocks(&content, id).map_err(change_error)?;
    json_to_py(py, &change.to_value())
}

/// `raw` made valid by the merge-repair rules, as `{"content", "repairs"}`.
/// Total over any value shaped as content, and deterministic: the same raw
/// value always repairs to the same content.
#[pyfunction]
pub fn repair<'py>(py: Python<'py>, raw: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let raw = quillmark_content::repair::Raw::from_value(&py_to_json(raw)?)
        .map_err(|e| PyValueError::new_err(format!("repair: not shaped as content: {e}")))?;
    let (content, repairs) = quillmark_content::repair::repair(raw);
    json_to_py(
        py,
        &serde_json::json!({
            "content": content.to_value(),
            "repairs": repairs.iter().map(|r| r.to_value()).collect::<Vec<_>>(),
        }),
    )
}
