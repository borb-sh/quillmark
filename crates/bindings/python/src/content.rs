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
use quillmark_core::document::{CardMut, Document, Edit, EditError, LoadError, Receipt, SyncMessage, SyncState, Target, WriteOptions, check_change, check_content_value};
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
    opts: WriteOptions<'_>,
) -> PyResult<Bound<'py, PyDict>> {
    taking(py, doc, card, base_path, field, |card, target| card.write(target, edit, opts))
}

/// Run `take` on the value at `field` on `card` (`None` the main card) of
/// `doc`, its receipt as it crosses.
pub(crate) fn taking<'py>(
    py: Python<'py>,
    doc: &mut Document,
    card: Option<usize>,
    base_path: &DocPath,
    field: Option<&str>,
    take: impl FnOnce(&mut CardMut<'_>, Target<'_>) -> Result<Receipt, EditError>,
) -> PyResult<Bound<'py, PyDict>> {
    let path = match field {
        None => base_path.body(),
        Some(f) => base_path.field(f),
    };
    let mut card = card_mut(doc, card);
    let receipt = take(&mut card, Target::from(field)).map_err(|e| convert_edit_error(e, base_path))?;
    let card_id = card.id().map(str::to_string);
    receipt_to_py(py, receipt, &path, card_id)
}

/// The card `card` names (`None` the main card), its index resolved.
pub(crate) fn card_mut(doc: &mut Document, card: Option<usize>) -> CardMut<'_> {
    match card {
        None => doc.main_mut(),
        Some(i) => doc.card_mut(i).expect("the card index is resolved"),
    }
}

/// What one peer of the Automerge sync protocol is known to hold, for one
/// value: `SyncState()` makes it, and `generate_sync_message` and
/// `receive_sync_message` read and advance it. One serves one value and one
/// peer; `encode` keeps it across a restart.
#[pyclass(name = "SyncState")]
pub struct PySyncState {
    pub(crate) inner: SyncState,
}

#[pymethods]
impl PySyncState {
    /// A peer of whom nothing is known yet.
    #[new]
    fn new() -> Self {
        PySyncState { inner: SyncState::new() }
    }

    /// The state as bytes `decode` reads back.
    fn encode<'py>(&self, py: Python<'py>) -> Bound<'py, pyo3::types::PyBytes> {
        pyo3::types::PyBytes::new(py, &self.inner.encode())
    }

    /// The state `encode` gave as `data`. Bytes that do not decode raise
    /// `edit::bad_live_bytes` with `reason` `decode`.
    #[staticmethod]
    fn decode(data: &[u8]) -> PyResult<Self> {
        SyncState::decode(data)
            .map(|inner| PySyncState { inner })
            .map_err(|e| convert_edit_error(EditError::bad_live_bytes(LoadError::Decode(e.to_string())), &DocPath::new()))
    }
}

/// The `edit::bad_live_bytes` refusal of a sync message that does not decode.
pub(crate) fn sync_message(bytes: &[u8]) -> Result<SyncMessage, EditError> {
    SyncMessage::decode(bytes).map_err(|e| EditError::bad_live_bytes(LoadError::Decode(e.to_string())))
}

/// `receipt` as it crosses, its dropped anchors' selectors at `path` on the
/// card `card_id` names.
pub(crate) fn receipt_to_py<'py>(
    py: Python<'py>,
    receipt: Receipt,
    path: &DocPath,
    card_id: Option<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let warnings = py_diagnostics(receipt.diagnostics(Some(path)));
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
    out.set_item("minted", receipt.minted)?;
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

/// A value's export as a binding answers it: the string cold, and with
/// `annotated` a dict of the annotated `markdown` and its `anchors`, each
/// `{id, line}`.
pub(crate) fn exported<'py>(py: Python<'py>, content: &Content, annotated: bool) -> PyResult<Bound<'py, PyAny>> {
    if !annotated {
        return Ok(quillmark_content::markdown::to_text(content).into_pyobject(py)?.into_any());
    }
    let read = quillmark_content::markdown::to_text_annotated(content);
    let anchors: Vec<_> = read.anchors.into_iter().map(|a| serde_json::json!({ "id": a.id, "line": a.line })).collect();
    json_to_py(py, &serde_json::json!({ "markdown": read.markdown, "anchors": anchors }))
}

/// Import markdown: the canonical `content` and a `parse::dropped_construct`
/// warning per construct the import dropped, `{"content", "warnings"}`.
/// `replace(import_markdown(md)["content"], ...)` is the cold write; prefer
/// `revise`. Raises `QuillmarkError` on an over-nested input or one past
/// 10 MiB.
#[pyfunction]
pub fn import_markdown<'py>(py: Python<'py>, markdown: &str) -> PyResult<Bound<'py, PyDict>> {
    let imported = quillmark_content::markdown::from_markdown(markdown)
        .map_err(|e| convert_edit_error(EditError::import(e), &DocPath::new()))?;
    let out = PyDict::new(py);
    out.set_item("content", content_to_py(py, &imported.content)?)?;
    out.set_item(
        "warnings",
        py_diagnostics(quillmark_core::document::dropped_constructs(imported.warnings, None)),
    )?;
    Ok(out)
}

/// The one content export: `content`'s markdown, a `codec: "plain"` value's
/// literal text. With `annotated`, a dict of the `markdown` spelling each
/// anchor and each island markdown cannot hold, which a revise reads back,
/// and every anchor as `{id, line}`. Raises where `content` is not canonical.
#[pyfunction]
#[pyo3(signature = (content, *, annotated=false))]
pub fn export_markdown<'py>(py: Python<'py>, content: &Bound<'_, PyAny>, annotated: bool) -> PyResult<Bound<'py, PyAny>> {
    exported(py, &content_from_py(content, "export_markdown")?, annotated)
}

/// The canonical form of `content`, read without a document: the one
/// spelling `replace` and every other content input takes. Raises
/// `ValueError` where `content` is not a valid value once normalized.
#[pyfunction]
pub fn normalize<'py>(py: Python<'py>, content: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let json = py_to_json(content)?;
    if !json.is_object() {
        return Err(PyValueError::new_err("normalize: expected a Content dict"));
    }
    check_content_value(&json).map_err(|e| convert_edit_error(e, &DocPath::new()))?;
    let read = Content::from_value_lenient(&json).map_err(|e| match &e {
        ParseError::Invalid(inv) if EditError::bound(inv).is_some() => {
            convert_edit_error(EditError::bound(inv).expect("a bound"), &DocPath::new())
        }
        _ => PyValueError::new_err(format!("normalize: not a Content: {e}")),
    })?;
    content_to_py(py, &read.content)
}

/// Whether `content` holds to the one-line constraint: one paragraph, in no
/// container, with no islands.
#[pyfunction]
pub fn is_inline(content: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(content_from_py(content, "is_inline")?.is_inline())
}

/// Whether `content` holds to the `text` constraint: no marks, no islands,
/// every block a paragraph in no container.
#[pyfunction]
pub fn is_plain(content: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(content_from_py(content, "is_plain")?.is_plain())
}
