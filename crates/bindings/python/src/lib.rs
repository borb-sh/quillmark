use pyo3::prelude::*;

mod enums;
mod errors;
mod types;

pub use enums::{PyOutputFormat, PySeverity};
pub use errors::QuillmarkError;
pub use types::{
    PyArtifact, PyDiagnostic, PyDocument, PyLocation, PyQuill,
    PyQuillmark, PyRenderResult, PyReader, PyWriter,
};

#[pymodule]
fn _quillmark(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyQuillmark>()?;
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

    m.add("QuillmarkError", m.py().get_type::<QuillmarkError>())?;

    Ok(())
}
