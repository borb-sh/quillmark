//! Backend trait for output backends.

use crate::error::RenderError;
use crate::quill::Quill;
use crate::{LiveSession, OutputFormat};

#[doc(hidden)]
pub mod sealed {
    /// The seal on [`Backend`](super::Backend), implemented by the workspace's
    /// own backends. Naming it from elsewhere names a hidden item, which is the
    /// declaration that the seam behind it moves without notice.
    pub trait Sealed {}
}

/// Backend trait for rendering different output formats.
///
/// Implementing it outside the workspace is unsupported: [`Backend::open`]
/// returns a [`LiveSession`], which only a `#[doc(hidden)]` `SessionHandle`
/// implementation can build. The [`sealed::Sealed`] supertrait states that in
/// the type system — a declaration, not a barrier — so adding a method here
/// stays a minor release.
pub trait Backend: sealed::Sealed + Send + Sync + std::fmt::Debug {
    /// The backend identifier, e.g. `"typst"`.
    fn id(&self) -> &'static str;

    fn supported_formats(&self) -> &'static [OutputFormat];

    /// Open a live render session from a quill and compiled JSON data.
    ///
    /// The backend pulls whatever static inputs it needs straight from
    /// `source`. There is no universal "template" input: a plate is one
    /// backend's private notion, read by that backend from its own files.
    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<LiveSession, RenderError>;
}

/// The refusal every backend owes a format outside its
/// [`Backend::supported_formats`], under `backend::format_not_supported`.
/// `backend` names the backend in the message; `supported` becomes the hint.
pub fn unsupported_format(format: OutputFormat, backend: &str, supported: &[OutputFormat]) -> RenderError {
    RenderError::from_diag(
        crate::Diagnostic::new(
            crate::Severity::Error,
            format!("{format:?} not supported by the {backend} backend"),
        )
        .with_code("backend::format_not_supported".to_string())
        .with_hint(format!("Supported formats: {supported:?}")),
    )
}

/// The pages a render covers: `pages` as given, or every page of `page_count`
/// when it is `None`. An index at or past `page_count` fails under
/// `backend::page_index_out_of_bounds`, naming every offending index.
///
/// The indices come back as given: order and repeats are the caller's.
pub fn selected_pages(
    pages: Option<&[usize]>,
    page_count: usize,
) -> Result<Vec<usize>, RenderError> {
    let Some(requested) = pages else {
        return Ok((0..page_count).collect());
    };

    let out_of_bounds: Vec<usize> = requested
        .iter()
        .copied()
        .filter(|&i| i >= page_count)
        .collect();
    if !out_of_bounds.is_empty() {
        return Err(RenderError::from_diag(
            crate::Diagnostic::new(
                crate::Severity::Error,
                format!(
                    "Page index out of bounds (page_count={page_count}); offending indices: {out_of_bounds:?}"
                ),
            )
            .with_code("backend::page_index_out_of_bounds".to_string())
            .with_hint("Read the session's page count before requesting pages.".to_string()),
        ));
    }

    Ok(requested.to_vec())
}

/// The refusal a backend owes a `pages` selection on a format it emits whole,
/// under `backend::page_selection_not_supported`. `format` names it in the
/// message.
pub fn page_selection_not_supported(format: OutputFormat) -> RenderError {
    RenderError::from_diag(
        crate::Diagnostic::new(
            crate::Severity::Error,
            format!("{format:?} output does not support page selection"),
        )
        .with_code("backend::page_selection_not_supported".to_string())
        .with_hint(
            "Drop the page selection to render the whole document, or ask for a per-page format."
                .to_string(),
        ),
    )
}

/// Pre-session hint for whether a backend with these `formats` can paint pages
/// to a canvas, used before a session exists (e.g. a GUI deciding whether to
/// mount a canvas preview without first paying to open one).
///
/// Canvas paint needs a per-page visual image of the laid-out page, so the
/// predicate keys off the visual-page formats ([`OutputFormat::Png`],
/// [`OutputFormat::Svg`]) rather than [`OutputFormat::Pdf`].
///
/// Only a hint: the authoritative answer is
/// [`LiveSession::supports_canvas`](crate::LiveSession::supports_canvas),
/// derived from the session's own canvas seam.
pub fn formats_support_canvas(formats: &[OutputFormat]) -> bool {
    formats
        .iter()
        .any(|f| matches!(f, OutputFormat::Png | OutputFormat::Svg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_selection_covers_every_page_and_a_selection_is_taken_verbatim() {
        assert_eq!(selected_pages(None, 3).unwrap(), [0, 1, 2]);
        assert_eq!(selected_pages(None, 0).unwrap(), [] as [usize; 0]);
        assert_eq!(selected_pages(Some(&[2, 0, 0]), 3).unwrap(), [2, 0, 0]);
    }

    #[test]
    fn a_page_past_the_document_is_refused() {
        let err = selected_pages(Some(&[0, 3]), 3).unwrap_err();
        assert_eq!(
            err.diagnostics()[0].code.as_deref(),
            Some("backend::page_index_out_of_bounds")
        );
    }

    #[test]
    fn formats_support_canvas_keys_off_visual_formats() {
        use OutputFormat::*;
        assert!(formats_support_canvas(&[Pdf, Svg, Png]));
        assert!(formats_support_canvas(&[Pdf, Svg]));
        assert!(formats_support_canvas(&[Png]));
        assert!(!formats_support_canvas(&[Pdf]));
        assert!(!formats_support_canvas(&[]));
    }
}
