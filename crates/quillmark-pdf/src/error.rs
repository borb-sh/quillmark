/// An error from the stamp spine. Carries a stable `code` (a `pdf::*` string a
/// consumer can match on) and a human-readable `message`. The [`From`] impl
/// forwards the code intact across a backend boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PdfError {
    /// Stable error code, e.g. `pdf::xref_stream`, `pdf::encrypted`.
    pub code: &'static str,
    pub message: String,
}

impl PdfError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<PdfError> for quillmark_core::RenderError {
    fn from(e: PdfError) -> Self {
        quillmark_core::RenderError::coded(e.code, e.message)
    }
}
