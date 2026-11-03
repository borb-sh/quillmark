use pyo3::prelude::*;

mod content;
mod enums;
mod errors;
mod types;

pub use enums::{PyOutputFormat, PySeverity};
pub use content::PySyncState;
pub use errors::QuillmarkError;
pub use types::{
    PyArtifact, PyDiagnostic, PyDocument, PyEngine, PyLocation, PyQuill, PyReader, PyRenderResult,
    PyUnchecked, PyWriter,
};

#[pymodule]
fn _quillmark(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyEngine>()?;
    m.add_class::<PyQuill>()?;
    m.add_class::<PyDocument>()?;
    m.add_class::<PyUnchecked>()?;
    m.add_class::<PyWriter>()?;
    m.add_class::<PyReader>()?;
    m.add_class::<PyRenderResult>()?;
    m.add_class::<PyArtifact>()?;
    m.add_class::<PyDiagnostic>()?;
    m.add_class::<PyLocation>()?;
    m.add_class::<content::PySyncState>()?;

    m.add_class::<PyOutputFormat>()?;
    m.add_class::<PySeverity>()?;

    m.add_function(wrap_pyfunction!(content::invert, m)?)?;
    m.add_function(wrap_pyfunction!(content::diff, m)?)?;
    m.add_function(wrap_pyfunction!(content::repair, m)?)?;
    m.add_function(wrap_pyfunction!(content::import_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(content::export_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(content::normalize, m)?)?;
    m.add_function(wrap_pyfunction!(content::is_inline, m)?)?;
    m.add_function(wrap_pyfunction!(content::is_plain, m)?)?;

    m.add("QuillmarkError", m.py().get_type::<QuillmarkError>())?;

    Ok(())
}
