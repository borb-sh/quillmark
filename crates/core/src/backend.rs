//! Backend trait for output backends.

use crate::error::RenderError;
use crate::quill::Quill;
use crate::{OutputFormat, RenderSession};

/// Backend trait for rendering different output formats.
pub trait Backend: Send + Sync + std::fmt::Debug {
    /// Get the backend identifier (e.g., "typst", "latex").
    fn id(&self) -> &'static str;

    /// Get supported output formats.
    fn supported_formats(&self) -> &'static [OutputFormat];

    /// Open an iterative render session from plate + compiled JSON data.
    fn open(
        &self,
        plate_content: &str,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<RenderSession, RenderError>;
}

/// Pre-session hint for whether a backend with these `formats` can paint pages
/// to a canvas, used before a session exists (e.g. a GUI deciding whether to
/// mount a canvas preview without first paying to open one).
///
/// Canvas paint needs a per-page *visual image* of the laid-out page, so the
/// predicate keys off the visual-page output formats — [`OutputFormat::Png`]
/// (raster) and [`OutputFormat::Svg`] (vector) — as opposed to
/// [`OutputFormat::Pdf`] (a document) or [`OutputFormat::Txt`]. A backend that
/// can rasterize a page advertises one of these.
///
/// This is only a hint. The **authoritative** answer is
/// [`RenderSession::supports_canvas`], which is derived from the session's
/// actual canvas seam ([`SessionHandle::page_size_pt`](crate::session::SessionHandle::page_size_pt))
/// and so cannot disagree with what `paint` will do — there is no separately
/// maintained capability flag to drift from the implementation.
pub fn formats_support_canvas(formats: &[OutputFormat]) -> bool {
    formats
        .iter()
        .any(|f| matches!(f, OutputFormat::Png | OutputFormat::Svg))
}
