use pyo3::prelude::*;

mod content;
mod enums;
mod errors;
mod types;

pub use enums::{PyOutputFormat, PySeverity};
pub use errors::QuillmarkError;
pub use types::{
    PyArtifact, PyDiagnostic, PyDocument, PyEngine, PyLocation, PyQuill, PyReader, PyRenderResult,
    PyWriter,
};

#[pymodule]
fn _quillmark(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyEngine>()?;
    m.add_class::<PyQuill>()?;
    m.add_class::<PyDocument>()?;
    m.add_class::<PyWriter>()?;
    m.add_class::<PyReader>()?;
    m.add_class::<PyRenderResult>()?;
    m.add_class::<PyArtifact>()?;
    m.add_class::<PyDiagnostic>()?;
    m.add_class::<PyLocation>()?;

    m.add_class::<PyOutputFormat>()?;
    m.add_class::<PySeverity>()?;

    m.add_function(wrap_pyfunction!(content::invert, m)?)?;
    m.add_function(wrap_pyfunction!(content::compose, m)?)?;
    m.add_function(wrap_pyfunction!(content::map_pos, m)?)?;
    m.add_function(wrap_pyfunction!(content::diff, m)?)?;
    m.add_function(wrap_pyfunction!(content::repair, m)?)?;

    m.add("QuillmarkError", m.py().get_type::<QuillmarkError>())?;

    Ok(())
}
