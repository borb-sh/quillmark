use pyo3::Bound;
use pyo3::conversion::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::pycell::{PyRef, PyRefMut};
use pyo3::types::{PyBytes, PyDate, PyDict, PyList, PyString};

use quillmark::{
    Diagnostic, Document, Engine, Location, OutputFormat, Quill, RenderResult, quill_from_path,
};
use std::path::PathBuf;
use std::sync::Arc;

use crate::enums::{PyOutputFormat, PySeverity};
use crate::errors::{
    card_index, convert_edit_error, convert_edit_errors, convert_render_error, convert_wire_error,
    page_indices, raise_summarized, raise_with_diagnostics,
};

/// The engine: a backend registry and render dispatcher. Renders a `Document`
/// against a `Quill`; holds no quills.
#[pyclass(name = "Engine")]
pub struct PyEngine {
    inner: Engine,
}

#[pymethods]
impl PyEngine {
    /// `fonts` is the host's font library: each a `.ttf`, `.otf`, `.ttc` or
    /// `.otc` file's bytes, whose faces set only text a `font_family` mark
    /// sets.
    /// Raises `QuillmarkError` (`engine::font_unsupported`,
    /// `engine::font_unreadable`) for a file it refuses.
    #[new]
    #[pyo3(signature = (fonts=None))]
    fn new(fonts: Option<Vec<Vec<u8>>>) -> PyResult<Self> {
        Ok(Self {
            inner: Engine::with_fonts(fonts.unwrap_or_default()).map_err(convert_render_error)?,
        })
    }

    /// The text faces a `font_family` mark may set in a session of `quill`:
    /// the host library's, in library order. Each is a dict of `family`,
    /// `weight`, `style`,
    /// `index` and `data` (the font file's bytes).
    fn font_faces<'py>(
        &self,
        py: Python<'py>,
        quill: &PyQuill,
    ) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let faces = self
            .inner
            .font_faces(&quill.inner)
            .map_err(convert_render_error)?;
        faces
            .into_iter()
            .map(|face| {
                let dict = PyDict::new(py);
                dict.set_item("family", face.family)?;
                dict.set_item("weight", face.weight)?;
                dict.set_item("style", face.style)?;
                dict.set_item("index", face.index)?;
                dict.set_item("data", PyBytes::new(py, &face.data))?;
                Ok(dict)
            })
            .collect()
    }

    /// Render `doc` against `quill` in one shot, resolving `quill`'s backend on
    /// this engine. The default `output_format` falls back to the backend's
    /// first supported format. `ppi` (raster formats only, default 144) must be
    /// finite, above 0, and small enough to keep both sides of every rendered
    /// page within [`MAX_RASTER_SIDE`](quillmark_core::backend::MAX_RASTER_SIDE).
    /// Raises `QuillmarkError` (`engine::backend_not_found`) when the backend
    /// is not registered, or `backend::invalid_raster_scale` for a `ppi`
    /// outside that range.
    ///
    /// `pages` selects 0-based page indices counting from the first page; a
    /// negative one selects no page and raises like an index past the last.
    ///
    /// `title` is the PDF document title, over the plate's own and then the
    /// quill's `description`; a blank one counts as absent, and PNG and SVG
    /// ignore it.
    ///
    /// `today` is the render date, a `datetime.date` (default
    /// `datetime.date.today()`): a `today` date field renders as it, and so does
    /// a plate's `datetime.today()`.
    ///
    /// The result's `warnings` are every `quill.validate(doc)` warning but
    /// `backend::declined_construct`, then the compile's, a decline among them;
    /// the load's stay on `doc.warnings`.
    ///
    /// The GIL is released while the backend compiles.
    #[pyo3(signature = (quill, doc, format=None, ppi=None, pages=None, regions=false, today=None, title=None))]
    #[allow(clippy::too_many_arguments)]
    fn render(
        &self,
        py: Python<'_>,
        quill: &PyQuill,
        doc: PyRef<'_, PyDocument>,
        format: Option<PyOutputFormat>,
        ppi: Option<f32>,
        pages: Option<Vec<isize>>,
        regions: bool,
        today: Option<Bound<'_, PyDate>>,
        title: Option<String>,
    ) -> PyResult<PyRenderResult> {
        let today = render_date(py, today.map(Bound::into_any))?;
        let mut opts = quillmark_core::types::RenderOptions::default();
        opts.output_format = format.map(OutputFormat::from);
        opts.ppi = ppi;
        opts.pages = pages.map(page_indices).transpose()?;
        opts.regions = regions;
        opts.title = title;
        // A clone, so the `Document` stays editable from other threads while
        // the GIL is released.
        let document = doc.inner.clone();
        drop(doc);
        let (engine, quill) = (&self.inner, &quill.inner);
        let mut result = py
            .detach(|| engine.render(quill, &document, today, &opts))
            .map_err(convert_render_error)?;
        let kinds: Vec<Option<&str>> = document.cards().iter().map(|c| c.kind()).collect();
        result.regions = quillmark_core::region::regions_to_doc_path(result.regions, &kinds);
        PyRenderResult::new(py, result)
    }

    /// The output formats `quill`'s backend can emit. Raises `QuillmarkError`
    /// (`engine::backend_not_found`) for an unregistered backend.
    fn supported_formats(&self, quill: &PyQuill) -> PyResult<Vec<PyOutputFormat>> {
        Ok(self
            .inner
            .supported_formats(&quill.inner)
            .map_err(convert_render_error)?
            .iter()
            .map(|f| (*f).into())
            .collect())
    }

    /// The backend ids this build compiled in, in no guaranteed order: the
    /// engine's roster, as opposed to the per-quill probe `supported_formats`.
    fn registered_backends(&self) -> Vec<String> {
        self.inner
            .registered_backends()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

/// The render date: `today`'s `year`/`month`/`day`, else Python's local
/// `datetime.date.today()`.
fn render_date(
    py: Python<'_>,
    today: Option<Bound<'_, PyAny>>,
) -> PyResult<quillmark_core::quill::CalendarDate> {
    let date = match today {
        Some(date) => date,
        None => py
            .import("datetime")?
            .getattr("date")?
            .call_method0("today")?,
    };
    let (year, month, day) = (
        date.getattr("year")?.extract()?,
        date.getattr("month")?.extract()?,
        date.getattr("day")?.extract()?,
    );
    quillmark_core::quill::CalendarDate::new(year, month, day).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "today is not a calendar date: {year}-{month}-{day}"
        ))
    })
}

#[pyclass(name = "Quill", from_py_object)]
#[derive(Clone)]
pub struct PyQuill {
    pub(crate) inner: Quill,
}

#[pymethods]
impl PyQuill {
    /// Load a quill from a filesystem directory: a pure config load, the
    /// declared backend resolved at render time by an `Engine`.
    #[staticmethod]
    fn from_path(path: PathBuf) -> PyResult<PyQuill> {
        let quill = quill_from_path(&path).map_err(convert_render_error)?;
        Ok(PyQuill { inner: quill })
    }

    /// Read a `.quill` bundle, the zip archive holding one quill's file tree.
    /// A bundle outside the format raises one `quill::bundle_*` diagnostic.
    #[staticmethod]
    fn from_bundle(bytes: &[u8]) -> PyResult<PyQuill> {
        let quill = Quill::from_bundle(bytes).map_err(raise_summarized)?;
        Ok(PyQuill { inner: quill })
    }

    /// This quill's file tree as a `.quill` bundle, the bytes `from_bundle`
    /// reads.
    fn to_bundle<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner.to_bundle().map_err(raise_summarized)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// The declared backend identifier (e.g. `"typst"`).
    #[getter]
    fn backend_id(&self) -> String {
        self.inner.backend_id().to_string()
    }

    /// The advisory diagnostics of the load that produced this quill: what is
    /// wrong with it short of refusing it. A quill that loads clean answers
    /// `[]`.
    #[getter]
    fn warnings(&self) -> Vec<PyDiagnostic> {
        py_diagnostics(self.inner.warnings().to_vec())
    }

    /// Bind this quill's schema to `doc` for typed writes. See [`PyWriter`] for
    /// the re-borrow/ephemerality contract.
    fn writer(slf: Py<Self>, doc: Py<PyDocument>) -> PyWriter {
        PyWriter { quill: slf, doc }
    }

    /// Bind this quill's schema to `doc` for interpreted reads: each field by its
    /// declared type, a richtext field to markdown and every other type verbatim.
    /// See [`PyReader`] for the re-borrow/ephemerality contract.
    fn reader(slf: Py<Self>, doc: Py<PyDocument>) -> PyReader {
        PyReader { quill: slf, doc }
    }

    #[getter]
    fn quill_ref(&self) -> String {
        format!("{}@{}", self.inner.name(), self.inner.config().version)
    }

    /// Identity snapshot mirroring the `quill:` section of `Quill.yaml`. A pure
    /// config read: capability lives on the engine, as
    /// `Engine.supported_formats(quill)`.
    #[getter]
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let config = self.inner.config();

        let dict = PyDict::new(py);
        dict.set_item("name", &config.name)?;
        dict.set_item("version", &config.version)?;
        dict.set_item("backend", &config.backend)?;
        dict.set_item("author", &config.author)?;
        dict.set_item("description", &config.description)?;
        if let Some(license) = &config.license {
            dict.set_item("license", license)?;
        }
        if let Some(homepage) = &config.homepage {
            dict.set_item("homepage", homepage)?;
        }

        Ok(dict)
    }

    /// Document schema as a structured dict (matches the wasm `schema` shape).
    #[getter]
    fn schema<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let value = self.inner.config().schema();
        json_to_py(py, &value)
    }

    /// JSON Schema (draft 2020-12) of the quill's authoring document: the
    /// shape `from_authoring` takes and `to_authoring` returns.
    #[getter]
    fn authoring_schema<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        json_to_py(py, &self.inner.config().authoring_schema())
    }

    /// `doc` as an authoring document: every field in the values form (each
    /// content leaf its codec's text), every body as Markdown, each card
    /// `{kind, fields, body}`. Sparse: an absent or `null` field stays absent. A content
    /// leaf that decodes under neither encoding exports as stored.
    fn to_authoring<'py>(
        &self,
        py: Python<'py>,
        doc: PyRef<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyAny>> {
        json_to_py(py, &self.inner.to_authoring(&doc.inner))
    }

    /// A document from an authoring document, conformed against this quill:
    /// the JSON twin of `parse`. `None` is absent; the document carries this
    /// quill's `$quill`. A value the schema refuses lands with a `conform::*`
    /// warning on `doc.warnings`, and `validate` reports the rest at document
    /// paths. Raises `QuillmarkError`, with every error found, on a value that
    /// is not the authoring document's shape (`authoring::invalid_shape`) or a
    /// name or kind no card can store (`edit::*`).
    fn from_authoring(&self, value: &Bound<'_, PyAny>) -> PyResult<PyDocument> {
        let value = py_to_json(value)?;
        let parsed = self
            .inner
            .from_authoring(&value)
            .map_err(raise_summarized)?;
        Ok(PyDocument {
            inner: parsed.document,
            parse_warnings: parsed.warnings,
        })
    }

    /// Validate `doc` against this quill's schema, returning a list of diagnostic
    /// dicts: an error blocks a render, a warning does not. Returns the
    /// schema's `validation::*`, a markdown string's `parse::dropped_construct`
    /// and the backend's `backend::declined_construct`.
    fn validate<'py>(
        &self,
        py: Python<'py>,
        doc: PyRef<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyList>> {
        let diags = self.inner.validate(&doc.inner);
        let json_value = serde_json::to_value(&diags)
            .map_err(|e| PyValueError::new_err(format!("validate: serialization failed: {e}")))?;
        let py_obj = json_to_py(py, &json_value)?;
        let list = py_obj
            .cast::<PyList>()
            .map_err(|_| PyValueError::new_err("validate: expected a list at top level"))?;
        Ok(list.clone())
    }

    /// Parse `markdown` and conform it against this quill: the primary ingestion
    /// path, and the bound twin of the schema-free `Document.parse`. The
    /// returned document is in its stored form: a content field as a
    /// content object, a `text` field's under `codec: "plain"`.
    ///
    /// Parse warnings and the `conform::*` diagnostics both land on
    /// `doc.warnings`. Raises `QuillmarkError` on a parse failure, or when
    /// `markdown` declares a `$quill` this quill does not answer to.
    fn parse(&self, markdown: &str) -> PyResult<PyDocument> {
        let parsed = self
            .inner
            .parse(markdown)
            .map_err(|e| raise_summarized(e.to_diagnostics()))?;
        Ok(PyDocument {
            inner: parsed.document,
            parse_warnings: parsed.warnings,
        })
    }

    /// Land `doc`'s declared content fields at their canonical rest **in
    /// place**, returning the `conform::*` diagnostic dicts for values that would
    /// not commit. The read-repair verb for a document that arrived without
    /// `parse` (`from_markdown`, `from_stored`, a stored row).
    ///
    /// Idempotent and a byte no-op on an already-canonical document, comments
    /// included. A value the strict write refuses stays as authored with a
    /// diagnostic. Raises `QuillmarkError` when `doc` declares a different
    /// `$quill`, before any mutation.
    fn conform<'py>(
        &self,
        py: Python<'py>,
        mut doc: PyRefMut<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyList>> {
        let diags = self
            .inner
            .conform(&mut doc.inner)
            .map_err(|e| raise_summarized(e.into_diagnostics()))?;
        let json_value = serde_json::to_value(&diags)
            .map_err(|e| PyValueError::new_err(format!("conform: serialization failed: {e}")))?;
        let py_obj = json_to_py(py, &json_value)?;
        let list = py_obj
            .cast::<PyList>()
            .map_err(|_| PyValueError::new_err("conform: expected a list at top level"))?;
        Ok(list.clone())
    }

    /// The empty document: a main card carrying this quill's `$quill` and
    /// `$kind: main`, no field committed, no composable card, no body. The
    /// type-minimal valid input, and the document a plate is bound to render
    /// (the plate contract). `Document(ref)` is the same document
    /// reached by spelling the reference.
    fn empty_document(&self) -> PyDocument {
        PyDocument {
            inner: self.inner.empty_document(),
            parse_warnings: Vec::new(),
        }
    }

    /// The quill's example document, `None` when no slot or body declares an
    /// example: the seeded document with every declared `example` committed as
    /// its cell's value and each kind's `body.example` as its body. A
    /// filled-in page whose values are made up to show the quill in use, never
    /// kept.
    fn example_document(&self) -> Option<PyDocument> {
        self.inner.example_document().map(|inner| PyDocument {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Seed a starter `Document` from the schema: the main card plus one instance
    /// of each composable card kind, each carrying its kind's `seed` and every
    /// other field absent (interpolated at render as `default`, else the field's
    /// blank).
    fn seed_document(&self) -> PyDocument {
        PyDocument {
            inner: self.inner.seed_document(),
            parse_warnings: Vec::new(),
        }
    }

    /// Seed a starter main card (carries `$quill`) from the schema: the
    /// `$kind: main` card of `seed_document()` alone, as a `Document.main` dict.
    fn seed_main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        card_to_pydict(py, &self.inner.seed_main())
    }

    /// Seed a starter composable card of the given kind (carries `$kind`),
    /// committing an optional per-kind seed `overlay`'s fields and `$body` in
    /// place of the kind's own `seed`, else that seed's; `None` if `card_kind`
    /// is not declared.
    /// Pass `document.seed_overlay(card_kind)` as `overlay` so a card added to a
    /// template-derived document inherits its curated starting values.
    #[pyo3(signature = (card_kind, overlay=None))]
    fn seed_card<'py>(
        &self,
        py: Python<'py>,
        card_kind: &str,
        overlay: Option<Bound<'_, PyAny>>,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let overlay = match overlay {
            Some(value) => quillmark_core::document::SeedOverlay::from_json(&py_to_json(&value)?),
            None => None,
        };
        match self.inner.seed_card(card_kind, overlay.as_ref()) {
            Some(card) => Ok(Some(card_to_pydict(py, &card)?)),
            None => Ok(None),
        }
    }
}

#[pyclass(name = "Document")]
pub struct PyDocument {
    pub(crate) inner: Document,
    pub(crate) parse_warnings: Vec<quillmark_core::error::Diagnostic>,
}

#[pymethods]
impl PyDocument {
    /// A blank document: a main card carrying only `$quill`, an empty body, and
    /// no composable cards. Absent fields resolve at render time (`default`, else
    /// the field's blank), so nothing the caller did not set reaches the output.
    /// For one instance of every card kind use `Quill.seed_document()`. Raises
    /// `ValueError` on an invalid quill reference.
    #[new]
    fn new(quill_ref: &str) -> PyResult<Self> {
        let qr: quillmark_core::version::QuillReference = quill_ref.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", quill_ref, e))
        })?;
        Ok(PyDocument {
            inner: Document::new(qr),
            parse_warnings: Vec::new(),
        })
    }

    /// Parse a whole document's markdown, quill-free: the document-level
    /// import, its content read cold, no anchor in it. `Quill.parse` is the
    /// same parse conformed to a quill; `revise` is the import that keeps
    /// anchors.
    #[staticmethod]
    fn parse(markdown: &str) -> PyResult<Self> {
        let output = Document::parse(markdown).map_err(|e| {
            let diag = e.to_diagnostic();
            let message = diag.message.clone();
            raise_with_diagnostics(vec![diag], message)
        })?;
        Ok(PyDocument {
            inner: output.document,
            parse_warnings: output.warnings,
        })
    }

    /// Reconstruct a `Document` from its versioned storage DTO string. Its
    /// `warnings` hold a `content::noncanonical_read` at each body or field
    /// whose content value the read rewrote to its canonical form, which
    /// storing the document again writes.
    /// Raises `QuillmarkError` on malformed JSON, unknown `schema`, missing fields,
    /// or unparseable quill reference.
    #[staticmethod]
    fn from_stored(json: &str) -> PyResult<Self> {
        let parsed = Document::from_stored(json).map_err(|e| {
            let msg = format!("invalid storage DTO: {e}");
            raise_with_diagnostics(
                vec![quillmark_core::error::Diagnostic::new(
                    quillmark_core::error::Severity::Error,
                    msg.clone(),
                )],
                msg,
            )
        })?;
        Ok(PyDocument {
            inner: parsed.document,
            parse_warnings: parsed.warnings,
        })
    }

    /// Read the storage version tag from a raw DTO string without a full parse, or `None`.
    /// The **storage** version, not a field schema; the wire key stays `"schema"`,
    /// the DTO's serde tag.
    #[staticmethod]
    fn storage_version_of(json: &str) -> Option<String> {
        quillmark_core::document::peek_storage_version(json)
    }

    /// The storage version every save this build makes writes. The tag
    /// advances only when the wire format changes, not on every release.
    #[staticmethod]
    fn current_storage_version() -> &'static str {
        quillmark_core::document::STORAGE_V0_135_0
    }

    /// The Quillmark Markdown rules.
    #[staticmethod]
    fn markdown_rules() -> &'static str {
        quillmark_core::document::MARKDOWN_RULES
    }

    /// The canonical `$quill` reference grammar as author-facing text: the same
    /// text the `parse::invalid_quill_reference` hint carries.
    #[staticmethod]
    fn quill_ref_hint() -> &'static str {
        quillmark_core::version::quill_ref_hint()
    }

    /// The one markdown export of the document. Cold, round-trip safe but for
    /// content: a body or content field holds what its markdown spells, so its
    /// anchors drop and its island ids re-mint; `to_stored` keeps both.
    /// With `annotated`, a dict: `markdown` spelling each anchor and each
    /// island markdown cannot hold, which `revise` and
    /// `writer.revise_document` read back, and `anchors`, every anchor of
    /// every body and content field as `{id, path, card_id, line}`, a cell's
    /// or a note's listed unspelled.
    #[pyo3(signature = (*, annotated=false))]
    fn to_markdown<'py>(&self, py: Python<'py>, annotated: bool) -> PyResult<Bound<'py, PyAny>> {
        if !annotated {
            return Ok(self.inner.to_markdown().into_pyobject(py)?.into_any());
        }
        let read = self.inner.to_markdown_annotated();
        let anchors: Vec<_> = read
            .anchors
            .into_iter()
            .map(|a| serde_json::json!({ "id": a.id, "path": a.path.to_string(), "card_id": a.card_id, "line": a.line }))
            .collect();
        json_to_py(py, &serde_json::json!({ "markdown": read.markdown, "anchors": anchors }))
    }

    /// The hash of the document's stored form, the quill-free twin of
    /// `writer.hash()`. Equal documents hash alike, whatever order a store kept
    /// their keys in.
    fn hash(&self) -> String {
        self.inner.hash()
    }

    /// The hash of the body (`field=None`) or richtext field on `card` (`None`
    /// for the main card): the field's content however it is stored, an absent
    /// field the empty content. Richtext only; `writer.hash_at` reads a field
    /// of any type through its schema, and is the `base_hash` the writer's
    /// verbs check. Raises `QuillmarkError` on a bad card index, a malformed
    /// field name, or a present non-content field value.
    #[pyo3(signature = (field=None, card=None))]
    fn hash_at(&self, field: Option<&str>, card: Option<CardSel>) -> PyResult<String> {
        let target = Target::resolve(&self.inner, card)?;
        let card = match target.index {
            None => self.inner.main(),
            Some(i) => self
                .inner
                .card(i)
                .ok_or_else(|| index_error(i, self.inner.cards().len()))?,
        };
        card.hash_of(quillmark_core::document::Target::from(field))
            .map_err(|e| convert_edit_error(e, &target.base))
    }

    /// Serialize to a versioned storage DTO string. Byte-deterministic per schema version.
    fn to_stored(&self) -> String {
        serde_json::to_string(&self.inner).expect("Document serialization is infallible")
    }

    #[getter]
    fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// Return a fresh `Document` handle with the same parsed state.
    fn clone(&self) -> Self {
        PyDocument {
            inner: self.inner.clone(),
            parse_warnings: self.parse_warnings.clone(),
        }
    }

    fn __copy__(&self) -> Self {
        self.clone()
    }

    fn __deepcopy__(&self, _memo: Bound<'_, PyAny>) -> Self {
        self.clone()
    }

    fn equals(&self, other: PyRef<'_, PyDocument>) -> bool {
        self.inner == other.inner
    }

    fn __eq__(&self, other: Bound<'_, PyAny>) -> bool {
        match other.extract::<PyRef<'_, PyDocument>>() {
            Ok(other) => self.inner == other.inner,
            Err(_) => false,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Document(quill_ref={:?}, cards={})",
            self.inner.quill_reference().to_string(),
            self.inner.cards().len()
        )
    }

    #[getter]
    fn card_count(&self) -> usize {
        self.inner.cards().len()
    }

    #[getter]
    fn warnings(&self) -> Vec<PyDiagnostic> {
        py_diagnostics(self.parse_warnings.clone())
    }

    /// Main (entry) card as a dict with `kind`, `quill`, `payload_items`,
    /// `ext`, `seed`, and `body`.
    #[getter]
    fn main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        card_to_pydict(py, self.inner.main())
    }

    /// Ordered list of composable card blocks.
    #[getter]
    fn cards<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let mut result = Vec::new();
        for card in self.inner.cards() {
            result.push(card_to_pydict(py, card)?);
        }
        Ok(result)
    }

    /// One composable card by index, same dict shape as `main`. An out-of-range
    /// `index` raises `IndexOutOfRange`.
    fn card<'py>(&self, py: Python<'py>, index: isize) -> PyResult<Bound<'py, PyDict>> {
        let len = self.inner.cards().len();
        let index = card_index(index, len)?;
        let card = self
            .inner
            .card(index)
            .ok_or_else(|| index_error(index, len))?;
        card_to_pydict(py, card)
    }

    /// The main card's `$seed[kind]` overlay dict, or `None`. Feeds
    /// `quill.seed_card(kind, overlay)` without projecting the whole main card,
    /// and keeps `seed_card` pure: the quill never reads the document.
    fn seed_overlay<'py>(&self, py: Python<'py>, kind: &str) -> PyResult<Bound<'py, PyAny>> {
        match self.inner.main().seed().and_then(|seed| seed.get(kind)) {
            Some(overlay) => json_to_py(py, overlay),
            None => py.None().into_bound_py_any(py),
        }
    }

    /// Remove a payload field, returning its previous value or `None`. `card`
    /// selects the target, here as on every `card=` verb: `None` the main card,
    /// a `str` the composable card holding that `$id` (`edit::unknown_card_id`
    /// where none does), an `int` the card at that position (out of range
    /// raises). The content writes, `apply`, `revise` and `replace`,
    /// take a card by `$id` alone, so a card moved or inserted
    /// since the writer's read leaves the write on the card it read.
    #[pyo3(signature = (name, card=None))]
    fn remove_field<'py>(
        &mut self,
        py: Python<'py>,
        name: &str,
        card: Option<CardSel>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let card = card_position(&self.inner, card)?;
        let base = self.addr_base(card);
        match self
            .addr_card_mut(card)?
            .remove_field(name)
            .map_err(|e| convert_edit_error(e, &base))?
        {
            Some(v) => quillvalue_to_py(py, &v),
            None => py.None().into_bound_py_any(py),
        }
    }

    /// Replace the opaque `$ext` map on a card. `value` must be a dict; raises
    /// `ValueError` otherwise. `$ext` carries out-of-band consumer state and never
    /// reaches the rendered output; pass `{}` for an explicit empty `$ext`. The
    /// whole map is the write, so a consumer holding one namespace merges the
    /// rest: `{**doc.main["ext"], ns: v}`.
    #[pyo3(signature = (value, card=None))]
    fn store_ext(&mut self, value: Bound<'_, PyAny>, card: Option<CardSel>) -> PyResult<()> {
        let card = card_position(&self.inner, card)?;
        let map = py_to_object(&value, "store_ext")?;
        let base = self.addr_base(card);
        self.addr_card_mut(card)?
            .store_ext(map)
            .map_err(|e| convert_edit_error(e, &base))?;
        Ok(())
    }

    /// Remove the `$ext` map from a card entirely, returning the previous map or
    /// `None`. Discards every namespace at once.
    #[pyo3(signature = (card=None))]
    fn remove_ext<'py>(
        &mut self,
        py: Python<'py>,
        card: Option<CardSel>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let card = card_position(&self.inner, card)?;
        let prev = self.addr_card_mut(card)?.remove_ext();
        ext_map_to_py(py, prev)
    }

    /// Merge a card-kind's seed `overlay` into the main card's `$seed` map
    /// under `card_kind`, preserving sibling kinds. Sets the starting values
    /// new cards of that kind spawn with.
    fn store_seed_overlay(&mut self, card_kind: &str, overlay: Bound<'_, PyAny>) -> PyResult<()> {
        let json = py_to_json(&overlay)?;
        // `$seed` is config-space, so an error here carries no field anchor.
        self.inner
            .main_mut()
            .store_seed_overlay(card_kind, json)
            .map_err(|e| convert_edit_error(e, &quillmark_core::path::DocPath::new()))?;
        Ok(())
    }

    /// Remove `card_kind` from the main card's `$seed` map, returning its
    /// overlay or `None`; drops `$seed` entirely once empty. Sibling kinds
    /// survive.
    fn remove_seed_overlay<'py>(
        &mut self,
        py: Python<'py>,
        card_kind: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        ext_value_to_py(py, self.inner.main_mut().remove_seed_overlay(card_kind))
    }

    fn set_quill_ref(&mut self, ref_str: &str) -> PyResult<()> {
        let qr: quillmark_core::version::QuillReference = ref_str.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", ref_str, e))
        })?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    /// The writes no quill schema checks: an [`PyUnchecked`] over this
    /// document.
    #[getter]
    fn unchecked(slf: Py<Self>) -> PyUnchecked {
        PyUnchecked { doc: slf }
    }

    /// Remove the composable card at `index`, returning it as a dict or `None`
    /// when the index addresses no card.
    fn remove_card<'py>(
        &mut self,
        py: Python<'py>,
        index: isize,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Ok(index) = usize::try_from(index) else {
            return Ok(None);
        };
        match self.inner.remove_card(index) {
            Some(card) => Ok(Some(card_to_pydict(py, &card)?)),
            None => Ok(None),
        }
    }

    fn move_card(&mut self, from_idx: isize, to_idx: isize) -> PyResult<()> {
        let len = self.inner.cards().len();
        let from_idx = card_index(from_idx, len)?;
        let to_idx = card_index(to_idx, len)?;
        self.inner
            .move_card(from_idx, to_idx)
            .map_err(|e| convert_edit_error(e, &quillmark_core::path::DocPath::new()))
    }

    /// Every anchor of every body and content value with its context, as
    /// dicts `{id, path, card_id, quote, prefix, suffix}`: the selector a host
    /// stores beside a comment thread and hands back to `reattach`. `path` is
    /// the value's `DocPath` string; `card_id` the `$id` of the card it names,
    /// `None` on the main card; `quote` the anchored text as stored, an inline
    /// island its `U+FFFC` slot, empty for a point; `prefix` and `suffix` up to
    /// 32 characters either side.
    /// Each card's fields come before its body. A table cell's or a note's
    /// anchor is read too, its context cut at the cell's edges.
    fn anchor_contexts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let selectors = self
            .inner
            .anchor_contexts()
            .into_iter()
            .map(selector_json)
            .collect();
        json_to_py(py, &serde_json::Value::Array(selectors))
    }

    /// The context of the anchor `id`, as `anchor_contexts` reads it, or
    /// `None` where no body or content value holds one.
    fn anchor_context<'py>(
        &self,
        py: Python<'py>,
        id: &str,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.inner
            .anchor_context(id)
            .map(|selector| json_to_py(py, &selector_json(selector)))
            .transpose()
    }

    /// Re-add anchors a write dropped, each at the `path` its selector names,
    /// where its `quote` is found exactly once with its context agreeing: four
    /// characters of quote or more, or a table's slot alone, eight of context
    /// on each side or the whole context where the value's edge cut it, an
    /// empty quote a point where the two meet. Never a nearest match. Returns `{reattached, orphaned}`, each
    /// selector given either placed with its context as read after the write or
    /// orphaned as given.
    ///
    /// A selector with a `card_id` finds its card by that id wherever it now
    /// sits, and is orphaned where no card of its path's kind holds it; one
    /// without is path-strict. A path naming no body or content value orphans
    /// its selector. An id live anywhere in the document, a table cell's
    /// included, is answered as reattached and not written. Each id is answered
    /// once, by its first selector. `changes` maps a path to the `applied` of the
    /// write that dropped its anchors, and a match there must overlap text
    /// that write inserted. Raises `ValueError` on a selector or change that
    /// is not one, or a path that does not parse.
    #[pyo3(signature = (selectors, changes=None))]
    fn reattach<'py>(
        &mut self,
        py: Python<'py>,
        selectors: &Bound<'_, PyAny>,
        changes: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let invalid = |e: String| PyValueError::new_err(format!("reattach: {e}"));
        let parse_path = |path: &str| {
            path.parse::<quillmark_core::path::DocPath>()
                .map_err(|e| invalid(format!("invalid path {path:?}: {e}")))
        };
        let serde_json::Value::Array(selectors) = py_to_json(selectors)? else {
            return Err(invalid(
                "selectors must be a list of anchor contexts".into(),
            ));
        };
        let selectors = selectors
            .iter()
            .map(|s| {
                let key = |name: &str| {
                    s.get(name)
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .ok_or_else(|| {
                            invalid(format!("an anchor context needs a string `{name}`"))
                        })
                };
                let card_id = match s.get("card_id") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(serde_json::Value::String(id)) => Some(id.clone()),
                    Some(_) => {
                        return Err(invalid("an anchor context's `card_id` is a string".into()));
                    }
                };
                Ok(quillmark_core::document::AnchorSelector::new(
                    key("id")?,
                    parse_path(&key("path")?)?,
                    key("quote")?,
                    key("prefix")?,
                    key("suffix")?,
                )
                .with_card_id(card_id))
            })
            .collect::<PyResult<Vec<_>>>()?;
        let mut by_path = std::collections::HashMap::new();
        if let Some(changes) = changes.filter(|c| !c.is_none()) {
            let serde_json::Value::Object(changes) = py_to_json(changes)? else {
                return Err(invalid("changes must map a path to a change".into()));
            };
            for (path, change) in changes {
                let change = quillmark_content::change::Change::from_value(&change)
                    .map_err(|e| invalid(format!("changes[{path:?}] is not a change: {e}")))?;
                by_path.insert(parse_path(&path)?, change);
            }
        }
        let out = self.inner.reattach(&selectors, &by_path);
        let receipt = serde_json::json!({
            "reattached": out.reattached.into_iter().map(selector_json).collect::<Vec<_>>(),
            "orphaned": out.orphaned.into_iter().map(selector_json).collect::<Vec<_>>(),
        });
        json_to_py(py, &receipt)
    }

    /// The `Content` at `field` (`None` the body) on `card` (`None` the main
    /// card), an absent field the empty content. Raises `QuillmarkError` on a
    /// bad card index, a malformed field name, or a present field value that is
    /// not content.
    #[pyo3(signature = (field=None, card=None))]
    fn read<'py>(&self, py: Python<'py>, field: Option<&str>, card: Option<CardSel>) -> PyResult<Bound<'py, PyAny>> {
        let target = Target::resolve(&self.inner, card)?;
        let content = self
            .card_at(target.index)
            .read(quillmark_core::document::Target::from(field))
            .map_err(|e| convert_edit_error(e, &target.base))?;
        crate::content::content_to_py(py, &content)
    }

    /// `to_markdown` of the value at `field` (`None` the body) on `card`
    /// (`None` the main card) alone, a `codec: "plain"` field its literal
    /// text: with `annotated`, a dict of the `markdown` `revise` there reads
    /// back and its `anchors`, each `{id, line}`. Raises as `read`.
    #[pyo3(signature = (field=None, card=None, *, annotated=false))]
    fn to_markdown_at<'py>(
        &self,
        py: Python<'py>,
        field: Option<&str>,
        card: Option<CardSel>,
        annotated: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let target = Target::resolve(&self.inner, card)?;
        let content = self
            .card_at(target.index)
            .read(quillmark_core::document::Target::from(field))
            .map_err(|e| convert_edit_error(e, &target.base))?;
        crate::content::exported(py, &content, annotated)
    }

    /// **Apply** `change` to the value at `field` on `card`, all its steps or
    /// none, and return the receipt: `applied` (the change in its normalized
    /// form, which `invert` undoes), `hash` (the next write's `base`),
    /// `dropped_anchors`, `moved_anchors`, `warnings`, `rebased` and
    /// `minted`, each placeholder id the change gave a record mapped to the id
    /// the write minted. `actor` is the Automerge actor the write writes
    /// under, each id it mints an object id under it. `base` is the hash the writer read
    /// (`hash_at`); pass it. One the value has
    /// moved on from merges the change with the change since as Automerge
    /// merges (CONTENT.md § "Live state and merge"), raising
    /// `edit::stale_base` where the document's history does not reach the hash. A refused change raises `edit::content_apply` and writes
    /// nothing.
    #[pyo3(signature = (change, field=None, card=None, base=None, actor=None))]
    fn apply<'py>(
        &mut self,
        py: Python<'py>,
        change: &Bound<'_, PyAny>,
        field: Option<&str>,
        card: Option<CardSel>,
        base: Option<&str>,
        actor: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        require_card_id(&card, "apply")?;
        let target = Target::resolve(&self.inner, card)?;
        let change = crate::content::change_from_py(change, "apply")?;
        let edit = quillmark_core::document::Edit::Apply(change);
        crate::content::write(py, &mut self.inner, target.index, &target.base, field, edit, quillmark_core::document::WriteOptions { base: base.map(quillmark_core::document::Base::Hash), actor })
    }

    /// Rewrite the value at `field` on `card` as `markdown`: the anchors and
    /// islands whose `qm-*` tags it keeps (`annotated` spells them) stay put,
    /// and the rest keeps what a word-aligned diff keeps. Returns the receipt,
    /// its `warnings` the import's. Raises as `apply`.
    #[pyo3(signature = (markdown, field=None, card=None, base=None, actor=None))]
    fn revise<'py>(
        &mut self,
        py: Python<'py>,
        markdown: &str,
        field: Option<&str>,
        card: Option<CardSel>,
        base: Option<&str>,
        actor: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        require_card_id(&card, "revise")?;
        let target = Target::resolve(&self.inner, card)?;
        let edit = quillmark_core::document::Edit::Revise(markdown);
        crate::content::write(py, &mut self.inner, target.index, &target.base, field, edit, quillmark_core::document::WriteOptions { base: base.map(quillmark_core::document::Base::Hash), actor })
    }

    /// Write `content` over the value at `field` on `card`, carrying each
    /// anchor it does not name where a diff keeps its text and reporting the
    /// rest in `dropped_anchors`. Raises as `apply`.
    #[pyo3(signature = (content, field=None, card=None, base=None, actor=None))]
    fn replace<'py>(
        &mut self,
        py: Python<'py>,
        content: &Bound<'_, PyAny>,
        field: Option<&str>,
        card: Option<CardSel>,
        base: Option<&str>,
        actor: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        require_card_id(&card, "replace")?;
        let target = Target::resolve(&self.inner, card)?;
        let content = crate::content::content_from_py(content, "replace")?;
        let edit = quillmark_core::document::Edit::Replace(content);
        crate::content::write(py, &mut self.inner, target.index, &target.base, field, edit, quillmark_core::document::WriteOptions { base: base.map(quillmark_core::document::Base::Hash), actor })
    }

    /// The document as Automerge bytes, its whole history: the row a host
    /// stores, which `load` reads back.
    fn save<'py>(&mut self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.save())
    }

    /// The document `data` holds, as `save` gave them or a merge of several.
    /// Bytes that do not decode, or hold no Quillmark document of a schema
    /// this build reads, raise `edit::bad_live_bytes`.
    #[staticmethod]
    fn load(data: &[u8]) -> PyResult<Self> {
        let inner = quillmark_core::document::Document::load(data).map_err(crate::content::refused)?;
        Ok(PyDocument { inner, parse_warnings: Vec::new() })
    }

    /// The document's revision: the hashes of its Automerge heads.
    fn heads(&mut self) -> Vec<String> {
        self.inner.heads().iter().map(ToString::to_string).collect()
    }

    /// The changes since `heads`, as bytes `merge` takes.
    fn save_since<'py>(&mut self, py: Python<'py>, heads: Vec<String>) -> PyResult<Bound<'py, PyBytes>> {
        let heads = crate::content::heads_from(&heads)?;
        Ok(PyBytes::new(py, &self.inner.save_since(&heads)))
    }

    /// Take the changes `data` holds, another peer's `save` or `save_since`,
    /// and merge them as Automerge merges. Bytes that do not decode, or leave
    /// the document outside its schema, raise `edit::bad_live_bytes` and
    /// change nothing.
    fn merge(&mut self, data: &[u8]) -> PyResult<()> {
        self.inner.merge(data).map_err(crate::content::refused)
    }

    /// The next Automerge sync message to the peer `state` tracks, `None`
    /// where the peer holds everything.
    fn generate_sync_message<'py>(&mut self, py: Python<'py>, state: &mut crate::content::PySyncState) -> Option<Bound<'py, PyBytes>> {
        self.inner.generate_sync_message(&mut state.inner).map(|m| PyBytes::new(py, &m.encode()))
    }

    /// Take an Automerge sync message from the peer `state` tracks. A message
    /// that does not decode, or would leave the document outside its schema,
    /// raises `edit::bad_live_bytes` and changes nothing.
    fn receive_sync_message(&mut self, state: &mut crate::content::PySyncState, message: &[u8]) -> PyResult<()> {
        let message = crate::content::sync_message(message).map_err(|e| convert_edit_error(e, &quillmark_core::path::DocPath::new()))?;
        self.inner.receive_sync_message(&mut state.inner, message).map_err(crate::content::refused)
    }
}

impl PyDocument {
    fn card_at(&self, index: Option<usize>) -> &quillmark_core::document::Card {
        match index {
            None => self.inner.main(),
            Some(i) => &self.inner.cards()[i],
        }
    }
}

/// An `AnchorSelector` as it crosses: a flat dict, its `path` a `DocPath`
/// string.
fn selector_json(s: quillmark_core::document::AnchorSelector) -> serde_json::Value {
    serde_json::json!({
        "id": s.id,
        "path": s.path.to_string(),
        "card_id": s.card_id,
        "quote": s.quote,
        "prefix": s.prefix,
        "suffix": s.suffix,
    })
}

impl PyDocument {
    fn card_mut_or_raise(
        &mut self,
        index: isize,
    ) -> PyResult<quillmark_core::document::CardMut<'_>> {
        let len = self.inner.cards().len();
        let index = card_index(index, len)?;
        self.inner
            .card_mut(index)
            .ok_or_else(|| index_error(index, len))
    }

    fn addr_card_mut(
        &mut self,
        card: Option<isize>,
    ) -> PyResult<quillmark_core::document::CardMut<'_>> {
        match card {
            None => Ok(self.inner.main_mut()),
            Some(index) => self.card_mut_or_raise(index),
        }
    }

    /// The anchor root the `card` selector names, main for `None`. An index that
    /// addresses no card anchors nowhere, the call it precedes raising for it.
    fn addr_base(&self, card: Option<isize>) -> quillmark_core::path::DocPath {
        match card {
            None => quillmark_core::path::DocPath::main(),
            Some(index) => usize::try_from(index).map_or_else(
                |_| quillmark_core::path::DocPath::new(),
                |index| card_base(&self.inner, index),
            ),
        }
    }
}

/// The card root at `index`, kind-qualified from the stored `$kind`.
fn card_base(doc: &Document, index: usize) -> quillmark_core::path::DocPath {
    quillmark_core::path::DocPath::card(doc.cards().get(index).and_then(|c| c.kind()), index)
}

fn index_error(index: usize, len: usize) -> PyErr {
    convert_edit_error(
        quillmark_core::document::EditError::IndexOutOfRange { index, len },
        &quillmark_core::path::DocPath::new(),
    )
}

/// What a revise's writer read, off its `base_hash` / `base_content`
/// keywords: one or neither.
enum ReadBase {
    Hash(String),
    Content(quillmark_core::Content),
}

impl ReadBase {
    fn from_py(base_hash: Option<&str>, base_content: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Self>> {
        match (base_hash, base_content) {
            (None, None) => Ok(None),
            (Some(hash), None) => Ok(Some(ReadBase::Hash(hash.to_string()))),
            (None, Some(content)) => Ok(Some(ReadBase::Content(crate::content::content_from_py(content, "base_content")?))),
            (Some(_), Some(_)) => Err(PyValueError::new_err("name base_hash or base_content, not both")),
        }
    }

    fn as_base(&self) -> quillmark_core::document::Base<'_> {
        match self {
            ReadBase::Hash(hash) => quillmark_core::document::Base::Hash(hash),
            ReadBase::Content(content) => quillmark_core::document::Base::Content(content),
        }
    }
}

/// How a `card=` selector names a composable card: by index, or by `$id`.
#[derive(FromPyObject)]
pub(crate) enum CardSel {
    Index(isize),
    Id(String),
}

/// The index `card` names, a card id resolved to its card's, `None` the main
/// card. An id no card holds raises `edit::unknown_card_id`.
fn card_position(doc: &Document, card: Option<CardSel>) -> PyResult<Option<isize>> {
    match card {
        None => Ok(None),
        Some(CardSel::Index(index)) => Ok(Some(index)),
        Some(CardSel::Id(id)) => match doc.card_index(&id) {
            Some(index) => Ok(Some(index as isize)),
            None => Err(convert_edit_error(
                quillmark_core::document::EditError::UnknownCardId(id),
                &quillmark_core::path::DocPath::new(),
            )),
        },
    }
}

/// A content write or a live-document verb names a composable card by its
/// `$id` alone: an index another writer's move or insert can shift would
/// land it on another card.
fn require_card_id(card: &Option<CardSel>, verb: &str) -> PyResult<()> {
    match card {
        Some(CardSel::Index(_)) => Err(PyValueError::new_err(format!(
            "{verb}: a card is named by its id (card=card[\"id\"]), not by index"
        ))),
        _ => Ok(()),
    }
}

/// Which card a `Writer` / `Reader` verb addresses, and the `DocPath` its
/// diagnostics anchor at: `None` is the main card.
struct Target {
    index: Option<usize>,
    base: quillmark_core::path::DocPath,
}

impl Target {
    fn resolve(doc: &quillmark_core::document::Document, card: Option<CardSel>) -> PyResult<Self> {
        match card_position(doc, card)? {
            None => Ok(Target {
                index: None,
                base: quillmark_core::path::DocPath::main(),
            }),
            Some(i) => {
                let index = card_index(i, doc.cards().len())?;
                Ok(Target {
                    index: Some(index),
                    base: card_base(doc, index),
                })
            }
        }
    }
}

/// The writes on a `Document` no quill schema checks, from
/// `Document.unchecked`. Each still holds every document invariant but the
/// schema: names, the bounds, card ids, and each content value decoded as a
/// body's is and stored in its canonical form, so `equals`, `to_stored` and
/// `to_markdown` agree for a field as for a body. The schema-checked writes
/// are `quill.writer(doc)`'s.
#[pyclass(name = "Unchecked")]
pub struct PyUnchecked {
    doc: Py<PyDocument>,
}

#[pymethods]
impl PyUnchecked {
    /// Store `value` as the field `name` on `card` (`None` the main card, a
    /// `str` the card holding that `$id`, an `int` the card at that index)
    /// with no schema coercion, which a render applies later. Each content
    /// value in `value`, at any depth, decodes as a body write's does, raising
    /// `edit::field_decode` where one does not.
    #[pyo3(signature = (name, value, card=None))]
    fn store_field(&self, py: Python<'_>, name: &str, value: Bound<'_, PyAny>, card: Option<CardSel>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        let mut doc = self.doc.borrow_mut(py);
        let card = card_position(&doc.inner, card)?;
        let base = doc.addr_base(card);
        let index = card.map(|i| card_index(i, doc.inner.cards().len())).transpose()?;
        doc.inner
            .unchecked()
            .store_field(index, name, qv)
            .map_err(|e| convert_edit_error(e, &base))
    }

    /// Place a composable card and return its `$id`. `at` picks the position:
    /// `None` appends, `i` inserts at index `i` (`0..=card_count`; out of range
    /// raises `IndexOutOfRange`, and a document holding 1,000 cards raises
    /// `edit::too_many_cards`). `card` is a `Card` dict, as `cards`,
    /// `remove_card`, and `seed_card` return. A card spelling no `id` mints
    /// one, under `actor` where one is given (`{actor}-{token}`), so two live
    /// clients placing cards never mint one id.
    #[pyo3(signature = (card, at=None, actor=None))]
    fn insert_card(&self, py: Python<'_>, card: Bound<'_, PyAny>, at: Option<isize>, actor: Option<&str>) -> PyResult<String> {
        let core_card = py_dict_to_card(&card)?;
        let mut doc = self.doc.borrow_mut(py);
        let at = at
            .map(|at| card_index(at, doc.inner.cards().len()))
            .transpose()?;
        // A kind error anchors at the target slot; an append has no slot yet, so
        // its kind error carries no anchor.
        let base = at.map_or_else(quillmark_core::path::DocPath::new, |i| {
            quillmark_core::path::DocPath::card(None, i)
        });
        doc.inner
            .unchecked()
            .insert_card(at, core_card, actor)
            .map_err(|e| convert_edit_error(e, &base))
    }
}

/// A `Document` bound to its `Quill` for typed writes, from `Quill.writer(doc)`.
/// Re-borrows both objects per call, so it is ephemeral by convention: bind,
/// write, discard.
///
/// Every verb takes `card`: `None` writes the main card, a `str` the
/// composable card holding that `$id`, an `int` the card at that index,
/// raising `edit::unknown_card_id` or `edit::index_out_of_range` when it
/// addresses none.
#[pyclass(name = "Writer")]
pub struct PyWriter {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
}

impl PyWriter {
    fn target(doc: &PyDocument, card: Option<CardSel>) -> PyResult<Target> {
        Target::resolve(&doc.inner, card)
    }

    fn card_id(doc: &PyDocument, index: Option<usize>) -> Option<String> {
        match index {
            None => doc.inner.main().id(),
            Some(i) => doc.inner.cards().get(i).and_then(|c| c.id()),
        }
        .map(str::to_string)
    }
}

#[pymethods]
impl PyWriter {
    /// The bound document: the same object passed in, mutated in place.
    #[getter]
    fn document(&self, py: Python<'_>) -> Py<PyDocument> {
        self.doc.clone_ref(py)
    }

    /// The hash of the document as this quill conforms it: the `base_hash`
    /// `revise_document` checks. A document loaded without its quill hashes as
    /// its conformed self, so a conform between the read and the write does
    /// not stale the base. Raises on a `$quill` this quill does not answer to.
    fn hash(&self, py: Python<'_>) -> PyResult<String> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .hash()
            .map_err(|e| raise_summarized(e.to_diagnostics()))
    }

    /// The hash of the body (`field=None`) or of `field` through its schema on
    /// `card` (`None` for the main card): the `base_hash` `revise` checks. A `richtext` or `text` field hashes its content
    /// however it is stored, any other type its stored value, an absent field as
    /// `None`. Raises `edit::unknown_field` for an
    /// undeclared name.
    #[pyo3(signature = (field=None, card=None))]
    fn hash_at(
        &self,
        py: Python<'_>,
        field: Option<&str>,
        card: Option<CardSel>,
    ) -> PyResult<String> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let target = Self::target(&doc, card)?;
        let mut writer = quill.inner.writer(&mut doc.inner);
        match target.index {
            None => writer.hash_of(field),
            Some(i) => writer
                .card(i)
                .map_err(|e| convert_edit_error(e, &target.base))?
                .hash_of(field),
        }
        .map_err(|e| convert_edit_error(e, &target.base))
    }

    /// Typed-commit one field (strict coerce, mismatch raises now), resolving
    /// its type from the addressed card's schema. Raises `edit::unknown_field`
    /// for a name that schema does not declare. Returns a
    /// `parse::dropped_construct` warning per construct a `richtext` markdown
    /// string in the value dropped, at the string's path.
    #[pyo3(signature = (name, value, card=None))]
    fn set(
        &self,
        py: Python<'_>,
        name: &str,
        value: Bound<'_, PyAny>,
        card: Option<CardSel>,
    ) -> PyResult<Vec<PyDiagnostic>> {
        let qv = py_to_quillvalue(&value)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let target = Self::target(&doc, card)?;
        let mut writer = quill.inner.writer(&mut doc.inner);
        match target.index {
            None => writer.set(name, qv),
            Some(i) => writer
                .card(i)
                .map_err(|e| convert_edit_error(e, &target.base))?
                .set(name, qv),
        }
        .map(py_diagnostics)
        .map_err(|e| convert_edit_error(e, &target.base))
    }

    /// Typed-commit several fields atomically: nothing is applied on error, and
    /// the raised `QuillmarkError` carries one diagnostic per offending field
    /// (an `edit::unknown_field` per undeclared name). Returns `set`'s warnings
    /// for every field, in batch order.
    #[pyo3(signature = (fields, card=None))]
    fn set_all(
        &self,
        py: Python<'_>,
        fields: Bound<'_, PyDict>,
        card: Option<CardSel>,
    ) -> PyResult<Vec<PyDiagnostic>> {
        let batch = pydict_to_field_batch(&fields)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let target = Self::target(&doc, card)?;
        let mut writer = quill.inner.writer(&mut doc.inner);
        match target.index {
            None => writer.set_all(batch),
            Some(i) => writer
                .card(i)
                .map_err(|e| convert_edit_error(e, &target.base))?
                .set_all(batch),
        }
        .map(py_diagnostics)
        .map_err(|errs| convert_edit_errors(errs, &target.base))
    }

    /// Revise the body (`field=None`) or the content field `field` on `card`
    /// (`None` for the main card) from authored text; anchors ride their text. A field
    /// is typed as well: the codec comes from its declared type (`richtext`
    /// diffs markdown, `text` the literal text, the body being `richtext`),
    /// the diffed result is schema-conformed, so a one-paragraph `richtext`
    /// field rejects a multi-block result with `edit::field_not_single_line`,
    /// and an undeclared name raises `edit::unknown_field`.
    ///
    /// Returns the receipt a content write answers, as `Document.revise` does:
    /// `applied`, the change from the value before (a `text` field's is one
    /// splice over its literal text); `hash`; `dropped_anchors` and
    /// `moved_anchors`; `warnings`, the import's `parse::dropped_construct` as
    /// `Diagnostic` objects anchored at the value; and `rebased`, set where the
    /// write merged with a change since its base.
    ///
    /// `base_hash` or `base_content` names what the writer read: its hash
    /// (`hash_at`), or the value itself as canonical Content-JSON (`read`). A
    /// value that has moved on merges the write with the change since as
    /// Automerge merges (CONTENT.md § "Live state and merge"); a base the
    /// document's history does not reach, as any read before it was loaded
    /// from its stored row, raises `edit::stale_base`, its `args["current"]`
    /// the hash now.
    #[pyo3(signature = (text, field=None, card=None, *, base_hash=None, base_content=None))]
    fn revise<'py>(
        &self,
        py: Python<'py>,
        text: &str,
        field: Option<&str>,
        card: Option<CardSel>,
        base_hash: Option<&str>,
        base_content: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let read = ReadBase::from_py(base_hash, base_content.as_ref())?;
        let read = read.as_ref().map(ReadBase::as_base);
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let target = Self::target(&doc, card)?;
        let card_id = Self::card_id(&doc, target.index);
        let mut writer = quill.inner.writer(&mut doc.inner);
        match target.index {
            None => writer.revise(field, text, read),
            Some(i) => writer
                .card(i)
                .map_err(|e| convert_edit_error(e, &target.base))?
                .revise(field, text, read),
        }
        .map_err(|e| convert_edit_error(e, &target.base))
        .and_then(|receipt| {
            let at = match field {
                None => target.base.body(),
                Some(name) => target.base.field(name),
            };
            crate::content::receipt_to_py(py, receipt, &at, card_id)
        })
    }

    /// Replace the bound document with `markdown`, then conform it: a
    /// composable card spelling a stored card's `$id` revises it, the rest
    /// align to the stored ones by `$kind` and text, and each aligned
    /// body and richtext field revises as `revise` does.
    /// Everything else lands as written; an omitted `$ext` keeps the stored one
    /// on the main card, on a card that aligned by id or text, and on one that aligned
    /// by position only when the `$kind` sequence is unchanged. Returns the
    /// receipt as a dict: `dropped_anchors`, each anchor the write dropped as
    /// the selector `reattach` takes, read off the stored document; `warnings`,
    /// `Diagnostic` objects, the `conform::*` ones last; and
    /// `from`, for each composable card of the revised document the index of
    /// the stored card it revises, or `None` for an inserted card. Clears the
    /// document's load `warnings`. Raises `QuillmarkError` on a parse failure
    /// or a `$quill` this quill does not answer to, leaving the document
    /// unchanged.
    ///
    /// `base_hash` (`hash()`) or `base_stored` (the `save()` bytes the writer
    /// read, or a `to_stored()` row) guard the write. A stale hash raises
    /// before any mutation. Under `base_stored` bytes of this document's
    /// history, the writer's change lands at the heads they were saved at and
    /// merges with every change since as Automerge merges; a row or bytes of
    /// another history hold only where they read as the document now.
    #[pyo3(signature = (markdown, *, base_hash=None, base_stored=None))]
    fn revise_document<'py>(
        &self,
        py: Python<'py>,
        markdown: &str,
        base_hash: Option<&str>,
        base_stored: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        use quillmark_core::document::DocumentBase;
        let stored = match (base_hash, base_stored) {
            (Some(_), Some(_)) => return Err(PyValueError::new_err("name base_hash or base_stored, not both")),
            (_, Some(stored)) => Some(match stored.cast::<PyBytes>() {
                Ok(bytes) => PyDocument::load(bytes.as_bytes())?.inner,
                Err(_) => PyDocument::from_stored(stored.extract()?)?.inner,
            }),
            _ => None,
        };
        let read = match (base_hash, &stored) {
            (Some(hash), _) => Some(DocumentBase::Hash(hash)),
            (None, Some(stored)) => Some(DocumentBase::Stored(stored)),
            (None, None) => None,
        };
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let mut writer = quill.inner.writer(&mut doc.inner);
        let revised = match read {
            None => writer.revise_document(markdown),
            Some(read) => writer.revise_document_against(read, markdown),
        }
        .map_err(|e| raise_summarized(e.to_diagnostics()))?;
        doc.parse_warnings.clear();
        let dropped: Vec<_> = revised.dropped_anchors.into_iter().map(selector_json).collect();
        let receipt = PyDict::new(py);
        receipt.set_item(
            "dropped_anchors",
            json_to_py(py, &serde_json::Value::Array(dropped))?,
        )?;
        receipt.set_item("warnings", py_diagnostics(revised.warnings))?;
        receipt.set_item("from", revised.from)?;
        Ok(receipt)
    }

    /// Build a composable card of `kind`, typed-commit `fields` onto it, set its
    /// body from optional markdown, and place it. `at` picks the position: `None`
    /// appends, `Some(i)` inserts at index `i`, and a position out of range
    /// raises. Transactional: a rejected field (raising a per-field diagnostic
    /// bundle) or an invalid kind, body, or position leaves the document
    /// untouched. The card's `$id` mints as `unchecked.insert_card` mints one,
    /// under `actor` where one is given. Returns a dict: `id`, that id, and
    /// `warnings`, the fields' `parse::dropped_construct` warnings then the
    /// body import's, anchored under the placed card.
    #[pyo3(signature = (kind, fields=None, body=None, at=None, actor=None))]
    fn add_card<'py>(
        &self,
        py: Python<'py>,
        kind: &str,
        fields: Option<Bound<'py, PyDict>>,
        body: Option<String>,
        at: Option<isize>,
        actor: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let batch = match fields {
            Some(f) => pydict_to_field_batch(&f)?,
            None => Vec::new(),
        };
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let at = at
            .map(|at| card_index(at, doc.inner.cards().len()))
            .transpose()?;
        // The card is built before it joins the document, so field errors anchor
        // at bare names (empty base).
        let added = quill
            .inner
            .writer(&mut doc.inner)
            .add_card(kind, batch, body.as_deref(), at, actor)
            .map_err(|errs| convert_edit_errors(errs, &quillmark_core::path::DocPath::new()))?;
        let out = PyDict::new(py);
        out.set_item("id", added.id)?;
        out.set_item("warnings", py_diagnostics(added.warnings))?;
        Ok(out)
    }

    /// Remove the composable card at `index`, returning it as a dict or `None`
    /// when the index is out of range.
    fn remove_card<'py>(
        &self,
        py: Python<'py>,
        index: isize,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Ok(index) = usize::try_from(index) else {
            return Ok(None);
        };
        let mut doc = self.doc.borrow_mut(py);
        match doc.inner.remove_card(index) {
            Some(card) => Ok(Some(card_to_pydict(py, &card)?)),
            None => Ok(None),
        }
    }
}

/// A `Document` bound to its `Quill` for interpreted reads, from
/// `Quill.reader(doc)`: the read twin of `Writer`. Re-borrows both objects per
/// call, so it is ephemeral by convention: bind, read, discard.
///
/// Every verb takes `card` on `Writer`'s terms: `None` reads the main card, an
/// `int` the composable card at that index.
#[pyclass(name = "Reader")]
pub struct PyReader {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
}

impl PyReader {
    fn target(doc: &PyDocument, card: Option<CardSel>) -> PyResult<Target> {
        Target::resolve(&doc.inner, card)
    }
}

#[pymethods]
impl PyReader {
    /// The bound document: the same object passed in.
    #[getter]
    fn document(&self, py: Python<'_>) -> Py<PyDocument> {
        self.doc.clone_ref(py)
    }

    /// Read the body (`field=None`) or a field, interpreted by its declared
    /// type: a richtext field to its markdown projection, every other type its
    /// canonical value, or `None` when the field is absent. The body reads its
    /// markdown. Raises `edit::unknown_field` for an undeclared name and
    /// `edit::field_decode` for a richtext value that does not decode.
    #[pyo3(signature = (field=None, card=None))]
    fn get<'py>(
        &self,
        py: Python<'py>,
        field: Option<&str>,
        card: Option<CardSel>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let target = Self::target(&doc, card)?;
        let reader = quill.inner.reader(&doc.inner);
        let read = match target.index {
            None => reader.get(field),
            Some(i) => reader
                .card(i)
                .map_err(|e| convert_edit_error(e, &target.base))?
                .get(field),
        }
        .map_err(|e| convert_edit_error(e, &target.base))?;
        read_value_to_py(py, read)
    }

    /// Read the body (`field=None`) or a content field as canonical Content-JSON (`{text, blocks, marks,
    /// islands}`): the `Content` twin of `get`, which projects. Decodes through
    /// the codec the declared type names, so a field the writer committed and one
    /// a markdown parse left as an authored string read back the same.
    ///
    /// `None` when the field is absent. Raises `edit::unknown_field` for an
    /// undeclared name, `edit::field_not_content` for a declared type that is not
    /// a content leaf, and `edit::field_decode` for a value that decodes under
    /// neither encoding.
    #[pyo3(signature = (field=None, card=None))]
    fn get_content<'py>(
        &self,
        py: Python<'py>,
        field: Option<&str>,
        card: Option<CardSel>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let target = Self::target(&doc, card)?;
        let reader = quill.inner.reader(&doc.inner);
        let read = match target.index {
            None => reader.get_content(field),
            Some(i) => reader
                .card(i)
                .map_err(|e| convert_edit_error(e, &target.base))?
                .get_content(field),
        }
        .map_err(|e| convert_edit_error(e, &target.base))?;
        content_to_py(py, read)
    }

    /// Read the Content nested inside a composite field at `path`: `[0]` an
    /// element of an `array<richtext>`, `["motto"]` an `object`'s content
    /// property, `[1, "notes"]` a leaf under both, `["controlled_by"]` a
    /// variant's cell. The codec is the leaf's declared type's, resolved through
    /// the field schema's `items` / `properties` / `variants`. An empty `path` is
    /// `get_content`.
    ///
    /// The Content is a write input, so this is the read a round-trip takes: an
    /// anchor and an island `id` have no markdown projection, and survive an edit
    /// only by riding this read back through `writer.set` of the field.
    ///
    /// `None` when the field is absent and when `path` names nothing in the stored
    /// value: an editor's row index goes stale between derive and read, so absence
    /// there is a read, not a fault. Raises `edit::unknown_field` for an
    /// undeclared name at any depth, `edit::field_not_content` when `path`
    /// resolves to no content leaf, `edit::field_decode`, anchored at the
    /// addressed path, for a value that decodes under neither encoding, and
    /// `edit::index_out_of_range` for a `card` addressing none.
    #[pyo3(signature = (name, path, card=None))]
    fn get_content_at<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        path: &Bound<'py, PyAny>,
        card: Option<CardSel>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let at = path_from_py(path, "get_content_at")?;
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let target = Self::target(&doc, card)?;
        let reader = quill.inner.reader(&doc.inner);
        let read = match target.index {
            None => reader.get_content_at(name, &at),
            Some(i) => reader
                .card(i)
                .map_err(|e| convert_edit_error(e, &target.base))?
                .get_content_at(name, &at),
        }
        .map_err(|e| convert_edit_error(e, &target.base))?;
        content_to_py(py, read)
    }
}

#[pyclass(name = "RenderResult")]
pub struct PyRenderResult {
    artifacts: Vec<Py<PyArtifact>>,
    warnings: Vec<Py<PyDiagnostic>>,
    output_format: OutputFormat,
    regions: Vec<quillmark_core::region::RenderedRegion>,
}

impl PyRenderResult {
    fn new(py: Python<'_>, result: RenderResult) -> PyResult<Self> {
        Ok(Self {
            artifacts: result
                .artifacts
                .into_iter()
                .map(|a| {
                    Py::new(
                        py,
                        PyArtifact {
                            inner: Arc::from(a.bytes),
                            format: a.output_format,
                        },
                    )
                })
                .collect::<PyResult<_>>()?,
            warnings: result
                .warnings
                .into_iter()
                .map(|inner| Py::new(py, PyDiagnostic { inner }))
                .collect::<PyResult<_>>()?,
            output_format: result.output_format,
            regions: result.regions,
        })
    }
}

#[pymethods]
impl PyRenderResult {
    /// Every read hands back the same `Artifact` objects in a new list,
    /// copying no artifact bytes.
    #[getter]
    fn artifacts(&self, py: Python<'_>) -> Vec<Py<PyArtifact>> {
        self.artifacts.iter().map(|a| a.clone_ref(py)).collect()
    }

    #[getter]
    fn warnings(&self, py: Python<'_>) -> Vec<Py<PyDiagnostic>> {
        self.warnings.iter().map(|d| d.clone_ref(py)).collect()
    }

    #[getter]
    fn format(&self) -> PyOutputFormat {
        self.output_format.into()
    }

    /// Schema-field geometry, populated only when `render(..., regions=True)`
    /// asked for it. One dict per entry: `{"field": str, "page": int, "rect":
    /// [x0, y0, x1, y1], "span": [start, end] | None}`, rect in PDF points with a
    /// bottom-left origin and page indices document-space. `field` is a `DocPath`
    /// address, as the document APIs use. Content fields carry one entry per
    /// segment (paragraph, heading, code fence) and page, each `span` the covered
    /// scalar range; scalar ink, claims and widgets carry `span: None`. Group by
    /// `field` and union the segment rects for the whole-field box.
    #[getter]
    fn regions<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.regions
            .iter()
            .map(|r| {
                let d = PyDict::new(py);
                d.set_item("field", &r.field)?;
                d.set_item("page", r.page)?;
                d.set_item("rect", r.rect.to_vec())?;
                d.set_item("span", r.span.map(|s| s.to_vec()))?;
                Ok(d)
            })
            .collect()
    }
}

#[pyclass(name = "Artifact", from_py_object)]
#[derive(Clone)]
pub struct PyArtifact {
    pub(crate) inner: Arc<[u8]>,
    pub(crate) format: OutputFormat,
}

#[pymethods]
impl PyArtifact {
    /// Copies the rendered buffer into a new `bytes` on each read; `save`
    /// writes it without a copy.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner)
    }

    #[getter]
    fn format(&self) -> PyOutputFormat {
        self.format.into()
    }

    fn save(&self, path: String) -> PyResult<()> {
        std::fs::write(&path, &self.inner).map_err(|e| {
            let msg = format!("Failed to save artifact to {}: {}", path, e);
            raise_with_diagnostics(
                vec![quillmark_core::error::Diagnostic::new(
                    quillmark_core::error::Severity::Error,
                    msg.clone(),
                )],
                msg,
            )
        })
    }

    #[getter]
    fn mime_type(&self) -> &'static str {
        self.format.mime_type()
    }
}

#[pyclass(name = "Diagnostic", from_py_object)]
#[derive(Clone)]
pub struct PyDiagnostic {
    pub(crate) inner: Diagnostic,
}

pub(crate) fn py_diagnostics(diags: Vec<Diagnostic>) -> Vec<PyDiagnostic> {
    diags
        .into_iter()
        .map(|inner| PyDiagnostic { inner })
        .collect()
}

#[pymethods]
impl PyDiagnostic {
    /// Canonical pretty-printed diagnostic text: the same rendering the CLI emits.
    fn __str__(&self) -> String {
        self.inner.fmt_pretty()
    }

    fn __repr__(&self) -> String {
        format!(
            "Diagnostic(severity={:?}, code={:?}, message={:?})",
            self.inner.severity, self.inner.code, self.inner.message,
        )
    }

    #[getter]
    fn severity(&self) -> PySeverity {
        self.inner.severity.into()
    }

    #[getter]
    fn message(&self) -> &str {
        &self.inner.message
    }

    #[getter]
    fn code(&self) -> Option<&str> {
        self.inner.code.as_deref()
    }

    #[getter]
    fn location(&self) -> Option<PyLocation> {
        self.inner
            .location
            .as_ref()
            .map(|l| PyLocation { inner: l.clone() })
    }

    #[getter]
    fn hint(&self) -> Option<&str> {
        self.inner.hint.as_deref()
    }

    #[getter]
    fn path(&self) -> Option<&str> {
        self.inner.path.as_deref()
    }

    /// The facts `message` interpolates, keyed by name. With `code`, enough to
    /// word this diagnostic in another language.
    #[getter]
    fn args<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let map: serde_json::Map<String, serde_json::Value> =
            self.inner.args.clone().into_iter().collect();
        json_to_py(py, &serde_json::Value::Object(map))
    }
}

#[pyclass(name = "Location", from_py_object)]
#[derive(Clone)]
pub struct PyLocation {
    pub(crate) inner: Location,
}

#[pymethods]
impl PyLocation {
    #[getter]
    fn file(&self) -> &str {
        &self.inner.file
    }

    #[getter]
    fn line(&self) -> usize {
        self.inner.line as usize
    }

    #[getter]
    fn column(&self) -> usize {
        self.inner.column as usize
    }
}

fn quillvalue_to_py<'py>(
    py: Python<'py>,
    value: &quillmark_core::value::QuillValue,
) -> PyResult<Bound<'py, PyAny>> {
    json_to_py(py, value.as_json())
}

/// Read an in-field path: a `str` is an object key, a non-negative `int` an
/// array index. A malformed step raises rather than being dropped — a skipped
/// step reads a different address and never says so.
///
/// A bare `str` or `bytes` is refused as the path itself for the same reason:
/// both iterate, so `path="motto"` would read `motto.m.o.t.t.o` and say
/// nothing. `["motto"]` is the one-key path.
fn path_from_py(path: &Bound<'_, PyAny>, ctx: &str) -> PyResult<Vec<quillmark::PathSegment>> {
    if path.is_instance_of::<PyString>() || path.is_instance_of::<PyBytes>() {
        return Err(PyValueError::new_err(format!(
            "{ctx}: `path` must be a sequence of str keys and non-negative int indices, \
             not a bare str; wrap a single key as `[key]`"
        )));
    }
    path.try_iter()
        .map_err(|_| {
            PyValueError::new_err(format!(
                "{ctx}: `path` must be a sequence of str keys and non-negative int indices"
            ))
        })?
        .enumerate()
        .map(|(i, step)| {
            let step = step?;
            if let Ok(key) = step.extract::<String>() {
                return Ok(quillmark::PathSegment::Key(key));
            }
            match step.extract::<usize>() {
                Ok(index) => Ok(quillmark::PathSegment::Index(index)),
                Err(_) => Err(PyValueError::new_err(format!(
                    "{ctx}: `path[{i}]` must be a str key or a non-negative int index"
                ))),
            }
        })
        .collect()
}

fn content_to_py<'py>(
    py: Python<'py>,
    content: Option<quillmark_content::model::Content>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    content.map(|c| crate::content::content_to_py(py, &c)).transpose()
}

fn read_value_to_py<'py>(
    py: Python<'py>,
    read: Option<quillmark_core::value::QuillValue>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    match read {
        None => Ok(None),
        Some(v) => Ok(Some(quillvalue_to_py(py, &v)?)),
    }
}

/// `CardWire`'s serde shape with the two divergences Python's surface owns: the
/// top-level key is snake_case `payload_items`, and an absent `quill` / `ext` /
/// `seed` is an explicit `None` rather than a missing key. Item entries match
/// the WASM `Card` shape verbatim.
fn card_to_pydict<'py>(
    py: Python<'py>,
    card: &quillmark_core::document::Card,
) -> PyResult<Bound<'py, PyDict>> {
    let wire = quillmark_core::document::CardWire::from(card);
    let json = serde_json::to_value(&wire).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let serde_json::Value::Object(mut map) = json else {
        return Err(PyValueError::new_err("card wire is not a JSON object"));
    };

    // `CardWire` declares the three right after `kind`, so filling them left to
    // right lands each at its own index and keeps one key order for every card.
    for (offset, key) in ["quill", "ext", "seed"].into_iter().enumerate() {
        if !map.contains_key(key) {
            map.shift_insert(offset + 1, key.to_string(), serde_json::Value::Null);
        }
    }

    let d = PyDict::new(py);
    for (key, value) in &map {
        let key = if key == "payloadItems" {
            "payload_items"
        } else {
            key.as_str()
        };
        d.set_item(key, json_to_py(py, value)?)?;
    }
    Ok(d)
}

pub(crate) fn json_to_py<'py>(py: Python<'py>, value: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    match value {
        serde_json::Value::Null => py.None().into_bound_py_any(py),
        serde_json::Value::Bool(b) => b.into_bound_py_any(py),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_bound_py_any(py)
            } else if let Some(u) = n.as_u64() {
                u.into_bound_py_any(py)
            } else if let Some(f) = n.as_f64() {
                f.into_bound_py_any(py)
            } else {
                py.None().into_bound_py_any(py)
            }
        }
        serde_json::Value::String(s) => s.as_str().into_bound_py_any(py),
        serde_json::Value::Array(arr) => {
            let list = pyo3::types::PyList::empty(py);
            for item in arr {
                let val = json_to_py(py, item)?;
                list.append(val)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(map) => {
            let dict = pyo3::types::PyDict::new(py);
            for (key, val) in map {
                let py_val = json_to_py(py, val)?;
                dict.set_item(key, py_val)?;
            }
            Ok(dict.into_any())
        }
    }
}

fn py_to_quillvalue(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::value::QuillValue> {
    let json = py_to_json(value)?;
    Ok(quillmark_core::value::QuillValue::from_json(json))
}

/// Value-conversion failures are collected rather than fail-fast, into one
/// `QuillmarkError` with a per-field `path`, matching the writer's own batch
/// contract. A non-string key raises `ValueError` directly.
fn pydict_to_field_batch(
    fields: &Bound<'_, PyDict>,
) -> PyResult<Vec<(String, quillmark_core::value::QuillValue)>> {
    let mut batch = Vec::new();
    let mut diags = Vec::new();
    for (key, value) in fields.iter() {
        let name: String = key
            .extract()
            .map_err(|_| PyValueError::new_err("field names must be strings"))?;
        match py_to_quillvalue(&value) {
            Ok(qv) => batch.push((name, qv)),
            Err(e) => diags.push(
                quillmark_core::error::Diagnostic::new(
                    quillmark_core::error::Severity::Error,
                    format!("invalid value: {e}"),
                )
                .with_path(name),
            ),
        }
    }
    if !diags.is_empty() {
        return Err(raise_summarized(diags));
    }
    Ok(batch)
}

pub(crate) fn py_to_json(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    py_to_json_at(value, 0)
}

/// Depth-bounded so an adversarially deep Python object cannot overflow the
/// native stack, and is refused at the shape core's
/// [`json_depth_exceeds`](quillmark_core::value::json_depth_exceeds) refuses.
/// The cutoff counts container levels, `depth` being the 0-based depth of the
/// current node, so a scalar leaf is never charged one.
fn py_to_json_at(value: &Bound<'_, PyAny>, depth: usize) -> PyResult<serde_json::Value> {
    use pyo3::types::{PyBool, PyDate, PyFloat, PyInt, PyList, PyString, PyTime};

    let reject_too_deep = || {
        Err(PyValueError::new_err(format!(
            "value nests deeper than the maximum of {} levels",
            quillmark_core::error::MAX_JSON_DEPTH
        )))
    };

    if value.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if value.is_instance_of::<PyBool>() {
        let b: bool = value.extract()?;
        return Ok(serde_json::Value::Bool(b));
    }
    if value.is_instance_of::<PyInt>() {
        // Python ints are unbounded: try i64, then u64, so large positive values
        // still convert losslessly, and report overflow as a `ValueError` rather
        // than leaking PyO3's raw `OverflowError`.
        if let Ok(i) = value.extract::<i64>() {
            return Ok(serde_json::json!(i));
        }
        if let Ok(u) = value.extract::<u64>() {
            return Ok(serde_json::json!(u));
        }
        return Err(pyo3::exceptions::PyValueError::new_err(
            "integer value is out of range for JSON conversion (exceeds 64-bit)",
        ));
    }
    if value.is_instance_of::<PyFloat>() {
        let f: f64 = value.extract()?;
        if !f.is_finite() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "non-finite float value '{}' cannot be represented in JSON",
                f
            )));
        }
        return Ok(serde_json::json!(f));
    }
    if value.is_instance_of::<PyString>() {
        let s: String = value.extract()?;
        return Ok(serde_json::Value::String(s));
    }
    if value.is_instance_of::<PyList>() {
        if depth >= quillmark_core::error::MAX_JSON_DEPTH {
            return reject_too_deep();
        }
        let list = value.cast::<PyList>()?;
        let arr: PyResult<Vec<serde_json::Value>> = list
            .iter()
            .map(|item| py_to_json_at(&item, depth + 1))
            .collect();
        return Ok(serde_json::Value::Array(arr?));
    }
    if value.is_instance_of::<PyDict>() {
        if depth >= quillmark_core::error::MAX_JSON_DEPTH {
            return reject_too_deep();
        }
        let dict = value.cast::<PyDict>()?;
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json_at(&v, depth + 1)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    // `datetime.date` (and its `datetime.datetime` subclass) and `datetime.time`
    // stringify to the spellings a `date` / `datetime` field reads. For anything
    // else `str()` stores a repr, which surfaces as garbage at render or
    // read-back rather than at the call that wrote it.
    if value.is_instance_of::<PyDate>() || value.is_instance_of::<PyTime>() {
        return Ok(serde_json::Value::String(value.str()?.to_string()));
    }
    Err(PyValueError::new_err(format!(
        "value of type '{}' has no JSON form; pass a str, int, float, bool, None, \
         list, dict, or a datetime.date / datetime.time",
        value.get_type().name()?
    )))
}

fn py_to_object(
    value: &Bound<'_, PyAny>,
    ctx: &str,
) -> PyResult<serde_json::Map<String, serde_json::Value>> {
    match py_to_json(value)? {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(PyValueError::new_err(format!(
            "{}: $ext must be a dict",
            ctx
        ))),
    }
}

fn ext_value_to_py<'py>(
    py: Python<'py>,
    value: Option<serde_json::Value>,
) -> PyResult<Bound<'py, PyAny>> {
    match value {
        Some(v) => json_to_py(py, &v),
        None => py.None().into_bound_py_any(py),
    }
}

fn ext_map_to_py<'py>(
    py: Python<'py>,
    map: Option<serde_json::Map<String, serde_json::Value>>,
) -> PyResult<Bound<'py, PyAny>> {
    ext_value_to_py(py, map.map(serde_json::Value::Object))
}

/// Accepts the snake_case `payload_items` key; a flat `{ kind, fields }` dict
/// fails loudly (`deny_unknown_fields`) rather than yielding an empty card.
fn py_dict_to_card(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::document::Card> {
    let json = py_to_json(value)?;
    let wire: quillmark_core::document::CardWire = serde_json::from_value(json).map_err(|e| {
        PyValueError::new_err(format!(
            "card must be a Card dict {{ kind, payload_items?, body? }}: {e}"
        ))
    })?;
    quillmark_core::document::Card::try_from(wire).map_err(convert_wire_error)
}
