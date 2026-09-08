use pyo3::conversion::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::pycell::{PyRef, PyRefMut};
use pyo3::types::{PyBytes, PyDict, PyList};
use pyo3::Bound;

use quillmark::{
    quill_from_path, Diagnostic, Document, Location, OutputFormat, Quill, Quillmark, RenderResult,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::enums::{PyOutputFormat, PySeverity};
use crate::errors::{
    card_index, convert_edit_error, convert_edit_errors, convert_edit_errors_at,
    convert_render_error, convert_wire_error, page_indices, raise_with_diagnostics,
};

#[pyclass(name = "Quillmark")]
pub struct PyQuillmark {
    inner: Quillmark,
}

#[pymethods]
impl PyQuillmark {
    #[new]
    fn new() -> Self {
        Self {
            inner: Quillmark::new(),
        }
    }

    /// Render `doc` against `quill` in one shot, resolving `quill`'s backend on
    /// this engine. The default `output_format` falls back to the backend's
    /// first supported format. `ppi` (raster formats only, default 144) must be
    /// finite, above 0, and small enough to keep every rendered page under
    /// [`MAX_RASTER_PIXELS`](quillmark_core::MAX_RASTER_PIXELS). Raises
    /// `QuillmarkError` (`engine::backend_not_found`) when the backend is not
    /// registered, or `backend::invalid_raster_scale` for a `ppi` outside that
    /// range.
    ///
    /// `pages` selects 0-based page indices counting from the first page; a
    /// negative one selects no page and raises like an index past the last.
    #[pyo3(signature = (quill, doc, format=None, ppi=None, pages=None, regions=false))]
    fn render(
        &self,
        quill: &PyQuill,
        doc: PyRef<'_, PyDocument>,
        format: Option<PyOutputFormat>,
        ppi: Option<f32>,
        pages: Option<Vec<isize>>,
        regions: bool,
    ) -> PyResult<PyRenderResult> {
        let mut opts = quillmark_core::RenderOptions::default();
        opts.output_format = format.map(OutputFormat::from);
        opts.ppi = ppi;
        opts.pages = pages.map(page_indices).transpose()?;
        opts.regions = regions;
        let start = Instant::now();
        let mut result = self
            .inner
            .render(&quill.inner, &doc.inner, &opts)
            .map_err(convert_render_error)?;
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let kinds: Vec<Option<&str>> = doc.inner.cards().iter().map(|c| c.kind()).collect();
        result.regions = quillmark_core::regions_to_doc_path(result.regions, &kinds);
        result
            .warnings
            .splice(0..0, doc.parse_warnings.iter().cloned());
        PyRenderResult::new(doc.py(), result, elapsed_ms)
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

#[pyclass(name = "Quill", from_py_object)]
#[derive(Clone)]
pub struct PyQuill {
    pub(crate) inner: Quill,
}

#[pymethods]
impl PyQuill {
    /// Load a quill from a filesystem directory: a pure config load, the
    /// declared backend resolved at render time by a `Quillmark` engine.
    #[staticmethod]
    fn from_path(path: PathBuf) -> PyResult<PyQuill> {
        let quill = quill_from_path(&path).map_err(convert_render_error)?;
        Ok(PyQuill { inner: quill })
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
        self.inner
            .warnings()
            .iter()
            .map(|d| PyDiagnostic { inner: d.clone() })
            .collect()
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
    /// `Quillmark.supported_formats(quill)`.
    ///
    /// The five standard keys come first in their declared order, then the
    /// extra keys sorted by name, so the dict's key order is a function of the
    /// quill alone.
    #[getter]
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let source = &self.inner;
        let config = source.config();

        let dict = PyDict::new(py);
        dict.set_item("name", &config.name)?;
        dict.set_item("version", &config.version)?;
        dict.set_item("backend", &config.backend)?;
        dict.set_item("author", &config.author)?;
        dict.set_item("description", &config.description)?;

        let extras: BTreeMap<&str, &quillmark_core::QuillValue> = source
            .metadata()
            .iter()
            .filter(|(key, _)| !quillmark_core::STANDARD_METADATA_KEYS.contains(&key.as_str()))
            .map(|(key, value)| (key.as_str(), value))
            .collect();
        for (key, value) in extras {
            dict.set_item(key, quillvalue_to_py(py, value)?)?;
        }

        Ok(dict)
    }

    /// Document schema as a structured dict (matches the wasm `schema` shape).
    #[getter]
    fn schema<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let value = self.inner.config().schema();
        json_to_py(py, &value)
    }

    #[getter]
    fn blueprint(&self) -> String {
        self.inner.config().blueprint()
    }

    /// Validate `doc` against this quill's schema, returning a list of diagnostic
    /// dicts (empty when the document is valid). Forwards the canonical
    /// `validation::*` diagnostics the engine emits, including the non-fatal
    /// `validation::must_fill` warning per `!must_fill` marker left behind.
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
    /// path, and the bound twin of the schema-free `Document.from_markdown`. The
    /// returned document rests at its canonical form: a `richtext` field as a
    /// content object, a `plaintext` field as its literal string.
    ///
    /// Parse warnings and the `conform::*` diagnostics both land on
    /// `doc.warnings`. Raises `QuillmarkError` on a parse failure, or when
    /// `markdown` declares a `$quill` this quill does not answer to.
    fn parse(&self, markdown: &str) -> PyResult<PyDocument> {
        let parsed = self.inner.parse(markdown).map_err(|e| {
            let diags = e.to_diagnostics();
            let message = quillmark_core::RenderError::summary_message(&diags);
            raise_with_diagnostics(diags, message)
        })?;
        Ok(PyDocument {
            inner: parsed.document,
            parse_warnings: parsed.warnings,
        })
    }

    /// Land `doc`'s declared content fields at their canonical rest **in
    /// place**, returning the `conform::*` diagnostic dicts for values that would
    /// not commit. The read-repair verb for a document that arrived through the
    /// transport door (`from_markdown`, `from_stored`, a stored row).
    ///
    /// Idempotent and a byte no-op on an already-canonical document, comments
    /// included. A `!must_fill` marker anywhere in a field's value skips that
    /// field; a value the strict write refuses stays as authored with a
    /// diagnostic. Raises `QuillmarkError` when `doc` declares a different
    /// `$quill`, before any mutation.
    fn conform<'py>(
        &self,
        py: Python<'py>,
        mut doc: PyRefMut<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyList>> {
        let diags = self.inner.conform(&mut doc.inner).map_err(|e| {
            let diags = e.into_diagnostics();
            let message = quillmark_core::RenderError::summary_message(&diags);
            raise_with_diagnostics(diags, message)
        })?;
        let json_value = serde_json::to_value(&diags)
            .map_err(|e| PyValueError::new_err(format!("conform: serialization failed: {e}")))?;
        let py_obj = json_to_py(py, &json_value)?;
        let list = py_obj
            .cast::<PyList>()
            .map_err(|_| PyValueError::new_err("conform: expected a list at top level"))?;
        Ok(list.clone())
    }

    /// Seed a starter `Document` from the schema: the main card plus one instance
    /// of each composable card kind, each committing its fields' `example` values
    /// and leaving every other field absent (interpolated at render). A field
    /// with both an `example` and a `default` renders its example.
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
    /// layering an optional per-kind seed `overlay` over the schema-example base
    /// (`overlay › example › absent`); `None` if `card_kind` is not declared.
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
            Some(value) => quillmark_core::SeedOverlay::from_json(&py_to_json(&value)?),
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
    pub(crate) parse_warnings: Vec<quillmark_core::Diagnostic>,
}

#[pymethods]
impl PyDocument {
    /// A blank document: a main card carrying only `$quill`, an empty body, and
    /// no composable cards. Absent fields resolve at render time (`default`, else
    /// the field's blank), so nothing the caller did not set reaches the output.
    /// For an example-filled starter use `Quill.seed_document()`. Raises
    /// `ValueError` on an invalid quill reference.
    #[new]
    fn new(quill_ref: &str) -> PyResult<Self> {
        let qr: quillmark_core::QuillReference = quill_ref.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", quill_ref, e))
        })?;
        Ok(PyDocument {
            inner: Document::new(qr),
            parse_warnings: Vec::new(),
        })
    }

    #[staticmethod]
    fn from_markdown(markdown: &str) -> PyResult<Self> {
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

    /// Reconstruct a `Document` from its versioned storage DTO string.
    /// Raises `QuillmarkError` on malformed JSON, unknown `schema`, missing fields,
    /// or unparseable quill reference.
    #[staticmethod]
    fn from_stored(json: &str) -> PyResult<Self> {
        let inner: Document = serde_json::from_str(json).map_err(|e| {
            let msg = format!("invalid storage DTO: {e}");
            raise_with_diagnostics(
                vec![quillmark_core::Diagnostic::new(
                    quillmark_core::Severity::Error,
                    msg.clone(),
                )],
                msg,
            )
        })?;
        Ok(PyDocument {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Like [`from_stored`] but returns `None` instead of raising.
    #[staticmethod]
    fn try_from_stored(json: &str) -> Option<Self> {
        let inner: Document = serde_json::from_str(json).ok()?;
        Some(PyDocument {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Read the storage version tag from a raw DTO string without a full parse, or `None`.
    /// The **storage** version, not a field schema; the wire key stays `"schema"`,
    /// the DTO's serde tag.
    #[staticmethod]
    fn storage_version_of(json: &str) -> Option<String> {
        quillmark_core::document::peek_storage_version(json)
    }

    /// Storage version this build writes.
    #[staticmethod]
    fn current_storage_version() -> &'static str {
        quillmark_core::document::STORAGE_V0_112_0
    }

    /// Canonical card-yaml authoring rules.
    #[staticmethod]
    fn format_rules() -> &'static str {
        quillmark_core::document::FORMAT_RULES
    }

    /// Authoring-ergonomics header introducing a blueprint to an LLM/MCP
    /// consumer for `quill_name`.
    #[staticmethod]
    fn blueprint_instruction(quill_name: &str) -> String {
        quillmark_core::document::blueprint_instruction(quill_name)
    }

    /// The canonical `$quill` reference grammar as author-facing text: the same
    /// text the `parse::invalid_quill_reference` hint carries.
    #[staticmethod]
    fn quill_ref_hint() -> &'static str {
        quillmark_core::quill_ref_hint()
    }

    /// Emit canonical Quillmark Markdown. Round-trip safe.
    fn to_markdown(&self) -> String {
        self.inner.to_markdown()
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
        self.parse_warnings
            .iter()
            .map(|d| PyDiagnostic { inner: d.clone() })
            .collect()
    }

    /// The main card's body as canonical Content-JSON (`{text, lines, marks,
    /// islands}`). For the markdown projection use
    /// `quill.reader(doc).body_markdown()`.
    #[getter]
    fn body<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let wire = quillmark_core::CardWire::from(self.inner.main());
        json_to_py(py, &wire.body)
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
        let card = self.inner.card(index).ok_or_else(|| index_error(index, len))?;
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
    /// an index the composable card at that position (out of range raises).
    #[pyo3(signature = (name, card=None))]
    fn remove_field<'py>(
        &mut self,
        py: Python<'py>,
        name: &str,
        card: Option<isize>,
    ) -> PyResult<Bound<'py, PyAny>> {
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
    fn store_ext(&mut self, value: Bound<'_, PyAny>, card: Option<isize>) -> PyResult<()> {
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
        card: Option<isize>,
    ) -> PyResult<Bound<'py, PyAny>> {
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
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::new()))?;
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
        let qr: quillmark_core::QuillReference = ref_str.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", ref_str, e))
        })?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    /// Build a fresh `Card` dict from a kind and a flat field mapping: the
    /// ergonomic constructor for `insert_card`, which also takes any card dict
    /// directly. Each `fields` entry becomes a card field in insertion order;
    /// `body` defaults to `""`.
    ///
    /// Checks only what a detached card can decide alone: field-name grammar and
    /// value depth. Kind validity is positional, so `insert_card` is its gate and
    /// any kind string is accepted here.
    #[staticmethod]
    #[pyo3(signature = (kind, fields=None, body=None))]
    fn make_card<'py>(
        py: Python<'py>,
        kind: String,
        fields: Option<Bound<'_, PyDict>>,
        body: Option<String>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let mut payload_items = Vec::new();
        if let Some(fields) = fields {
            for (k, v) in fields.iter() {
                let key: String = k.extract()?;
                payload_items.push(quillmark_core::PayloadItemWire::Field {
                    key,
                    value: py_to_json(&v)?,
                    fill: false,
                    nested_fills: Vec::new(),
                });
            }
        }
        let mut wire = quillmark_core::CardWire::new(
            kind,
            serde_json::Value::String(body.unwrap_or_default()),
        );
        wire.payload_items = payload_items;
        let card = quillmark_core::Card::try_from(wire).map_err(convert_wire_error)?;
        card_to_pydict(py, &card)
    }

    /// Place a composable card. `at` picks the position: `None` appends, `Some(i)`
    /// inserts at index `i` (`0..=card_count`; out of range raises
    /// `IndexOutOfRange`). `card` is a `Card` dict, as `make_card`, `cards`,
    /// `remove_card`, and `seed_card` return.
    #[pyo3(signature = (card, at=None))]
    fn insert_card(&mut self, card: Bound<'_, PyAny>, at: Option<isize>) -> PyResult<()> {
        let core_card = py_dict_to_card(&card)?;
        let at = at
            .map(|at| card_index(at, self.inner.cards().len()))
            .transpose()?;
        // A kind error anchors at the target slot; an append has no slot yet, so
        // its kind error carries no anchor.
        let base = at.map_or_else(quillmark_core::DocPath::new, |i| {
            quillmark_core::DocPath::card(None, i)
        });
        match at {
            None => self.inner.push_card(core_card),
            Some(index) => self.inner.insert_card(index, core_card),
        }
        .map_err(|e| convert_edit_error(e, &base))
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
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::new()))
    }

    fn set_card_kind(&mut self, index: isize, new_kind: &str) -> PyResult<()> {
        let index = card_index(index, self.inner.cards().len())?;
        self.inner
            .set_card_kind(index, new_kind)
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::card(None, index)))
    }

}

impl PyDocument {
    fn card_mut_or_raise(&mut self, index: isize) -> PyResult<&mut quillmark_core::Card> {
        let len = self.inner.cards().len();
        let index = card_index(index, len)?;
        self.inner.card_mut(index).ok_or_else(|| index_error(index, len))
    }

    fn addr_card_mut(&mut self, card: Option<isize>) -> PyResult<&mut quillmark_core::Card> {
        match card {
            None => Ok(self.inner.main_mut()),
            Some(index) => self.card_mut_or_raise(index),
        }
    }

    /// The anchor root the `card` selector names, main for `None`. An index that
    /// addresses no card anchors nowhere, the call it precedes raising for it.
    fn addr_base(&self, card: Option<isize>) -> quillmark_core::DocPath {
        match card {
            None => quillmark_core::DocPath::main(),
            Some(index) => usize::try_from(index).map_or_else(
                |_| quillmark_core::DocPath::new(),
                |index| card_base(&self.inner, index),
            ),
        }
    }
}

/// The card root at `index`, kind-qualified from the stored `$kind`.
fn card_base(doc: &Document, index: usize) -> quillmark_core::DocPath {
    quillmark_core::DocPath::card(doc.cards().get(index).and_then(|c| c.kind()), index)
}

fn index_error(index: usize, len: usize) -> PyErr {
    convert_edit_error(
        quillmark_core::EditError::IndexOutOfRange { index, len },
        &quillmark_core::DocPath::new(),
    )
}

/// A `Document` bound to its `Quill` for typed writes, from `Quill.writer(doc)`.
/// It re-borrows both objects per call, pyo3 objects carrying no lifetime to hold
/// the borrow, so it is ephemeral by convention: bind, write, discard.
#[pyclass(name = "Writer")]
pub struct PyWriter {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
}

#[pymethods]
impl PyWriter {
    /// The bound document: the same object passed in, mutated in place.
    #[getter]
    fn document(&self, py: Python<'_>) -> Py<PyDocument> {
        self.doc.clone_ref(py)
    }

    /// Typed-commit one main-card field (strict coerce, mismatch raises now).
    /// Raises `edit::unknown_field` for a name the schema does not declare.
    fn set(&self, py: Python<'_>, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .set(name, qv)
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::main()))
    }

    /// Typed-commit several main-card fields atomically: nothing is applied on
    /// error, and the raised `QuillmarkError` carries one diagnostic per offending
    /// field (an `edit::unknown_field` per undeclared name).
    fn set_all(&self, py: Python<'_>, fields: Bound<'_, PyDict>) -> PyResult<()> {
        let batch = pydict_to_field_batch(&fields)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .set_all(batch)
            .map_err(|errs| convert_edit_errors(errs, &quillmark_core::DocPath::main()))
    }

    /// Write the document in the values form: the write twin of
    /// `reader.values()`. An absent key is untouched; a present one replaces its
    /// axis, so `fields` is the whole truth for declared names and an unnamed one
    /// is removed. All-or-nothing: nothing is applied on error, and each refused
    /// cell is one diagnostic under its own `path` (`main.qty`,
    /// `cards.line_item[0].desc`); a `values` this binding cannot read as the
    /// shape raises `ValueError` before any of it is applied.
    ///
    /// A cell equal to its projection is not written, so handing back an unedited
    /// `values()` read changes no bytes. A changed content cell is a cold import
    /// (`revise_field` per cell is what keeps its anchors), and cards match by
    /// position and kind, so deleting or reordering one rewrites every card after
    /// it.
    fn set_values(&self, py: Python<'_>, values: &Bound<'_, PyAny>) -> PyResult<()> {
        let json = py_to_json(values)?;
        let values: quillmark_core::DocumentValues = serde_json::from_value(json)
            .map_err(|e| PyValueError::new_err(format!("set_values: invalid values shape: {e}")))?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .set_values(&values)
            .map_err(convert_edit_errors_at)
    }

    /// Revise the main body from markdown; anchors rebase. The `Delta` receipt is
    /// discarded, as on `revise_field`.
    fn revise_body(&self, py: Python<'_>, markdown: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .revise_body(markdown)
            .map(|_| ())
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::main()))
    }

    /// Revise the content main-card field `name` from authored text: typed *and*
    /// anchor-preserving, where `set` cold-imports. The codec comes from the
    /// declared type: `richtext` diffs markdown, `plaintext` the literal text.
    /// Surviving anchors rebase, then the diffed result is schema-conformed, so a
    /// `richtext(inline)` field rejects a multi-block result with
    /// `edit::field_not_inline`. Raises `edit::unknown_field` for an undeclared
    /// name. The text `Delta` is discarded.
    fn revise_field(&self, py: Python<'_>, name: &str, text: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .revise_field(name, text)
            .map(|_| ())
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::main()))
    }

    /// Build a composable card of `kind`, typed-commit `fields` onto it, set its
    /// body from optional markdown, and place it. `at` picks the position: `None`
    /// appends, `Some(i)` inserts at index `i`, and a position out of range
    /// raises. Transactional: a rejected field (raising a per-field diagnostic
    /// bundle) or an invalid kind, body, or position leaves the document
    /// untouched.
    #[pyo3(signature = (kind, fields=None, body=None, at=None))]
    fn add_card(
        &self,
        py: Python<'_>,
        kind: &str,
        fields: Option<Bound<'_, PyDict>>,
        body: Option<String>,
        at: Option<isize>,
    ) -> PyResult<()> {
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
        quill
            .inner
            .writer(&mut doc.inner)
            .add_card(kind, batch, body.as_deref(), at)
            .map_err(|errs| convert_edit_errors(errs, &quillmark_core::DocPath::new()))
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

    /// A `CardWriter` for the composable card at `index`. The index is checked
    /// lazily at the write, so this never raises. The cursor is ephemeral: a
    /// `remove_card`/`add_card` between binding and writing silently retargets
    /// it; re-resolve the index at write time when cards may move.
    fn card(&self, py: Python<'_>, index: isize) -> PyCardWriter {
        PyCardWriter {
            quill: self.quill.clone_ref(py),
            doc: self.doc.clone_ref(py),
            index,
        }
    }
}

/// A composable card bound to its `Quill` for typed writes, from `Writer.card`.
/// Same verbs as `Writer`, targeting the card at its bound index; each write
/// raises `edit::index_out_of_range` for an index out of range.
#[pyclass(name = "CardWriter")]
pub struct PyCardWriter {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
    index: isize,
}

impl PyCardWriter {
    fn bound_index(&self, doc: &PyDocument) -> PyResult<usize> {
        card_index(self.index, doc.inner.cards().len())
    }
}

#[pymethods]
impl PyCardWriter {
    /// The bound card index.
    #[getter]
    fn index(&self) -> isize {
        self.index
    }

    /// The bound card's `$kind`, or `None` when it carries none. Raises
    /// `edit::index_out_of_range` for a bad bound index.
    #[getter]
    fn kind(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let mut writer = quill.inner.writer(&mut doc.inner);
        let card = writer
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?;
        Ok(card.kind().map(|k| k.to_string()))
    }

    /// Typed-commit one field on this card, resolving its type from the card's
    /// `$kind` schema. Raises `edit::unknown_field` for an undeclared name
    /// and `edit::index_out_of_range` for a bad index.
    fn set(&self, py: Python<'_>, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        quill
            .inner
            .writer(&mut doc.inner)
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .set(name, qv)
            .map_err(|e| convert_edit_error(e, &base))
    }

    /// Typed-commit several fields on this card atomically: same per-field
    /// diagnostic bundle as `Writer.set_all`.
    fn set_all(&self, py: Python<'_>, fields: Bound<'_, PyDict>) -> PyResult<()> {
        let batch = pydict_to_field_batch(&fields)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let mut writer = quill.inner.writer(&mut doc.inner);
        writer
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .set_all(batch)
            .map_err(|errs| convert_edit_errors(errs, &base))
    }

    /// Revise this card's body from markdown (edit semantics), discarding the
    /// `Delta` receipt as `Writer.revise_body` does.
    fn revise_body(&self, py: Python<'_>, markdown: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        quill
            .inner
            .writer(&mut doc.inner)
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .revise_body(markdown)
            .map(|_| ())
            .map_err(|e| convert_edit_error(e, &base))
    }

    /// The card twin of `Writer.revise_field`. Raises `edit::unknown_field` for
    /// an undeclared name and `edit::index_out_of_range` for a bad index.
    fn revise_field(&self, py: Python<'_>, name: &str, text: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let mut writer = quill.inner.writer(&mut doc.inner);
        writer
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .revise_field(name, text)
            .map(|_| ())
            .map_err(|e| convert_edit_error(e, &base))
    }

    /// Write this card in the values form: `Writer.set_values` restricted to
    /// one slot, under the same per-axis rule. An absent `kind` keeps the
    /// card's; a differing one rebuilds the slot. Refusals anchor at
    /// `cards.<kind>[<index>]`; raises `edit::index_out_of_range` for a bad
    /// bound index and `ValueError` for a `values` this binding cannot read as
    /// the shape.
    fn set_values(&self, py: Python<'_>, values: &Bound<'_, PyAny>) -> PyResult<()> {
        let json = py_to_json(values)?;
        let values: quillmark_core::CardValues = serde_json::from_value(json)
            .map_err(|e| PyValueError::new_err(format!("set_values: invalid values shape: {e}")))?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let mut writer = quill.inner.writer(&mut doc.inner);
        writer
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .set_values(&values)
            .map_err(convert_edit_errors_at)
    }
}

/// A `Document` bound to its `Quill` for interpreted reads, from
/// `Quill.reader(doc)`: the read twin of `Writer`. Re-borrows both objects per
/// call, so it is ephemeral by convention: bind, read, discard.
#[pyclass(name = "Reader")]
pub struct PyReader {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
}

#[pymethods]
impl PyReader {
    /// The bound document: the same object passed in.
    #[getter]
    fn document(&self, py: Python<'_>) -> Py<PyDocument> {
        self.doc.clone_ref(py)
    }

    /// Read a main-card field, interpreted by its declared type: a richtext field
    /// to its markdown projection, every other type its canonical value, or
    /// `None` when the field is absent. Raises `edit::unknown_field` for an
    /// undeclared name and `edit::field_decode` for a richtext value that does
    /// not decode.
    fn get<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let read = quill
            .inner
            .reader(&doc.inner)
            .get(name)
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::main()))?;
        read_value_to_py(py, read)
    }

    /// Read a main-card content field as canonical Content-JSON (`{text, lines,
    /// marks, islands}`): the `Content` twin of `get`, which projects. Decodes
    /// through the codec the declared type names, so a field the writer committed
    /// and one a markdown parse left as an authored string read back the same.
    ///
    /// `None` when the field is absent. Raises `edit::unknown_field` for an
    /// undeclared name, `edit::field_not_content` for a declared type that is not
    /// a content leaf, and `edit::field_decode` for a value that decodes under
    /// neither encoding.
    fn get_content<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let read = quill
            .inner
            .reader(&doc.inner)
            .get_content(name)
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::main()))?;
        content_to_py(py, read)
    }

    /// Read the Content nested inside a composite field at `path`: `[0]` an
    /// element of an `array<richtext>`, `[1, "notes"]` a leaf under an array and
    /// an object both. The codec is the leaf's declared type's, resolved through
    /// the field schema's `items` / `properties` / `variants`. An empty `path` is
    /// `get_content`.
    ///
    /// `None` when the field is absent and when `path` names nothing in the stored
    /// value: an editor's row index goes stale between derive and read, so absence
    /// there is a read, not a fault. Raises `edit::unknown_field` for an
    /// undeclared name at any depth, `edit::field_not_content` when `path`
    /// resolves to no content leaf, and `edit::field_decode`, anchored at the
    /// addressed path, for a value that decodes under neither encoding.
    fn get_content_at<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        path: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let at = path_from_py(path, "get_content_at")?;
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let read = quill
            .inner
            .reader(&doc.inner)
            .get_content_at(name, &at)
            .map_err(|e| convert_edit_error(e, &quillmark_core::DocPath::main()))?;
        content_to_py(py, read)
    }

    /// The main body's markdown: quill-free, since a body's type is a format fact
    /// rather than a schema fact. Never raises.
    fn body_markdown(&self, py: Python<'_>) -> String {
        let doc = self.doc.borrow(py);
        doc.inner.main().body_markdown()
    }

    /// The whole document in the values form: `{"fields", "body", "cards",
    /// "ext"}`, the main card's fields with every content leaf as its codec's
    /// text (`richtext` markdown, `plaintext` literal) and everything else as
    /// stored, its body as markdown, its `$ext`, and every composable card as
    /// `{"kind", "fields", "body", "ext"}`. A present-null field is `None`.
    ///
    /// Every key is present, so the dict is a valid `writer.set_values` input and
    /// handing it back unedited changes no bytes. Sparse: an absent field is an
    /// absent key, never its `default`. Never raises: a content leaf that decodes
    /// under neither encoding rides out as stored where `get` would raise. A
    /// projection, never a storage format: persist with `to_stored`.
    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let values = quill.inner.reader(&doc.inner).values();
        let json = serde_json::to_value(&values)
            .map_err(|e| PyValueError::new_err(format!("values: serialization failed: {e}")))?;
        json_to_py(py, &json)
    }

    /// A `CardReader` for the composable card at `index`. The index is checked
    /// lazily at the read, so this never raises. The cursor is ephemeral: a
    /// `remove_card`/`add_card` between binding and reading silently retargets it.
    fn card(&self, py: Python<'_>, index: isize) -> PyCardReader {
        PyCardReader {
            quill: self.quill.clone_ref(py),
            doc: self.doc.clone_ref(py),
            index,
        }
    }
}

/// A composable card bound to its `Quill` for interpreted reads, from
/// `Reader.card`. Same verbs as `Reader`, reading the card at its bound index;
/// each read raises `edit::index_out_of_range` for an index out of range.
#[pyclass(name = "CardReader")]
pub struct PyCardReader {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
    index: isize,
}

impl PyCardReader {
    fn bound_index(&self, doc: &PyDocument) -> PyResult<usize> {
        card_index(self.index, doc.inner.cards().len())
    }
}

#[pymethods]
impl PyCardReader {
    /// The bound card index.
    #[getter]
    fn index(&self) -> isize {
        self.index
    }

    /// The bound card's `$kind`, or `None` when it carries none. Raises
    /// `edit::index_out_of_range` if the bound index is out of range.
    #[getter]
    fn kind(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let reader = quill.inner.reader(&doc.inner);
        let card = reader
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?;
        Ok(card.kind().map(|k| k.to_string()))
    }

    /// Read a field on this card, interpreted by its declared type: the
    /// card-indexed twin of `Reader.get`. Raises `edit::unknown_field` for an
    /// undeclared name and `edit::index_out_of_range` for a bad index.
    fn get<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let reader = quill.inner.reader(&doc.inner);
        let read = reader
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .get(name)
            .map_err(|e| convert_edit_error(e, &base))?;
        read_value_to_py(py, read)
    }

    /// The card-indexed twin of `Reader.get_content`, with the same outcomes plus
    /// `edit::index_out_of_range` for a bad index.
    fn get_content<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let reader = quill.inner.reader(&doc.inner);
        let read = reader
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .get_content(name)
            .map_err(|e| convert_edit_error(e, &base))?;
        content_to_py(py, read)
    }

    /// The card-indexed twin of `Reader.get_content_at`, with the same outcomes
    /// plus `edit::index_out_of_range` for a bad index.
    fn get_content_at<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        path: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let at = path_from_py(path, "get_content_at")?;
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let reader = quill.inner.reader(&doc.inner);
        let read = reader
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .get_content_at(name, &at)
            .map_err(|e| convert_edit_error(e, &base))?;
        content_to_py(py, read)
    }

    /// This card in the values form: `Reader.values` restricted to one slot.
    /// Raises `edit::index_out_of_range` for a bad bound index.
    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let index = self.bound_index(&doc)?;
        let base = card_base(&doc.inner, index);
        let reader = quill.inner.reader(&doc.inner);
        let values = reader
            .card(index)
            .map_err(|e| convert_edit_error(e, &base))?
            .values();
        let json = serde_json::to_value(&values)
            .map_err(|e| PyValueError::new_err(format!("values: serialization failed: {e}")))?;
        json_to_py(py, &json)
    }

    /// The card twin of `Reader.body_markdown`. Raises `edit::index_out_of_range`
    /// for a bad bound index.
    fn body_markdown(&self, py: Python<'_>) -> PyResult<String> {
        let doc = self.doc.borrow(py);
        let index = self.bound_index(&doc)?;
        let card = doc
            .inner
            .card(index)
            .ok_or_else(|| index_error(index, doc.inner.cards().len()))?;
        Ok(card.body_markdown())
    }
}

#[pyclass(name = "RenderResult")]
pub struct PyRenderResult {
    artifacts: Vec<Py<PyArtifact>>,
    warnings: Vec<Py<PyDiagnostic>>,
    output_format: OutputFormat,
    regions: Vec<quillmark_core::RenderedRegion>,
    render_time_ms: f64,
}

impl PyRenderResult {
    fn new(py: Python<'_>, result: RenderResult, render_time_ms: f64) -> PyResult<Self> {
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
            render_time_ms,
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

    /// Wall-clock time spent inside `render`, in milliseconds.
    #[getter]
    fn render_time_ms(&self) -> f64 {
        self.render_time_ms
    }

    /// Schema-field geometry, populated only when `render(..., regions=True)`
    /// asked for it. One dict per entry: `{"field": str, "page": int, "rect":
    /// [x0, y0, x1, y1], "span": [start, end] | None}`, rect in PDF points with a
    /// bottom-left origin and page indices document-space. `field` is a `DocPath`
    /// address, as the document APIs use. Content fields carry one entry per
    /// segment (paragraph, heading, code fence) and page, each `span` the covered
    /// USV range; widgets and scalar reference sites carry `span: None`. Group by
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
                vec![quillmark_core::Diagnostic::new(
                    quillmark_core::Severity::Error,
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

#[pymethods]
impl PyDiagnostic {
    /// Canonical pretty-printed diagnostic text: the same rendering the CLI and
    /// WASM (`Document.formatDiagnostic`) emit.
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

    #[getter]
    fn source_chain(&self) -> Vec<String> {
        self.inner.source_chain.clone()
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
    value: &quillmark_core::QuillValue,
) -> PyResult<Bound<'py, PyAny>> {
    json_to_py(py, value.as_json())
}

/// Read an in-field path: a `str` is an object key, a non-negative `int` an
/// array index. A malformed step raises rather than being dropped — a skipped
/// step reads a different address and never says so.
fn path_from_py(path: &Bound<'_, PyAny>, ctx: &str) -> PyResult<Vec<quillmark::PathSegment>> {
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
    content: Option<quillmark_content::Normalized>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    match content {
        None => Ok(None),
        Some(content) => Ok(Some(json_to_py(
            py,
            &quillmark_content::serial::to_seam_value(&content),
        )?)),
    }
}

fn read_value_to_py<'py>(
    py: Python<'py>,
    read: Option<quillmark_core::QuillValue>,
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
    card: &quillmark_core::Card,
) -> PyResult<Bound<'py, PyDict>> {
    let wire = quillmark_core::CardWire::from(card);
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

fn json_to_py<'py>(py: Python<'py>, value: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
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

fn py_to_quillvalue(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::QuillValue> {
    let json = py_to_json(value)?;
    Ok(quillmark_core::QuillValue::from_json(json))
}

/// Value-conversion failures are collected rather than fail-fast, into one
/// `QuillmarkError` with a per-field `path`, matching the writer's own batch
/// contract. A non-string key raises `ValueError` directly.
fn pydict_to_field_batch(
    fields: &Bound<'_, PyDict>,
) -> PyResult<Vec<(String, quillmark_core::QuillValue)>> {
    let mut batch = Vec::new();
    let mut diags = Vec::new();
    for (key, value) in fields.iter() {
        let name: String = key
            .extract()
            .map_err(|_| PyValueError::new_err("field names must be strings"))?;
        match py_to_quillvalue(&value) {
            Ok(qv) => batch.push((name, qv)),
            Err(e) => diags.push(
                quillmark_core::Diagnostic::new(
                    quillmark_core::Severity::Error,
                    format!("invalid value: {e}"),
                )
                .with_path(name),
            ),
        }
    }
    if !diags.is_empty() {
        let message = quillmark_core::RenderError::summary_message(&diags);
        return Err(raise_with_diagnostics(diags, message));
    }
    Ok(batch)
}

fn py_to_json(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    py_to_json_at(value, 0)
}

/// Depth-bounded so an adversarially deep Python object cannot overflow the
/// native stack, and is refused at the shape core's
/// [`json_depth_exceeds`](quillmark_core::json_depth_exceeds) refuses. The cutoff
/// counts container levels, `depth` being the 0-based depth of the current node,
/// so a scalar leaf is never charged one.
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
fn py_dict_to_card(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::Card> {
    let json = py_to_json(value)?;
    let wire: quillmark_core::CardWire = serde_json::from_value(json).map_err(|e| {
        PyValueError::new_err(format!(
            "card must be a Card dict {{ kind, payload_items?, body? }}: {e}"
        ))
    })?;
    quillmark_core::Card::try_from(wire).map_err(convert_wire_error)
}

