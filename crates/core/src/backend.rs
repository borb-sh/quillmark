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

/// The diagnostic code a backend's own declined construct rides.
pub const DECLINED_CONSTRUCT: &str = "backend::declined_construct";

/// The warning a backend owes a content field holding a construct it typesets
/// nothing for: `count` of `construct` in the field `path` anchors, from
/// `backend`.
///
/// The observed twin of quill-declared
/// [`plate::unsupported_construct`](crate::quill::UNSUPPORTED_CONSTRUCT), which
/// exists because core cannot see a plate drop a construct. A backend declining
/// one outright *is* the observer, so it says so itself, per field rather than
/// per body and at the compile that dropped it rather than at the pre-render
/// walk. One diagnostic per (field, construct): a producer that sees every
/// occurrence at once collapses them into `count`.
///
/// Non-fatal by construction, for the same reason as its twin: the content
/// stores and round-trips, and it is the page that will not carry it.
pub fn declined_construct(
    backend: &str,
    construct: crate::quill::BlockConstruct,
    count: usize,
    path: &crate::path::DocPath,
) -> crate::Diagnostic {
    let mut args = std::collections::BTreeMap::new();
    args.insert("backend".to_string(), backend.into());
    args.insert("construct".to_string(), construct.as_str().into());
    args.insert("count".to_string(), count.into());
    crate::Diagnostic::new(
        crate::Severity::Warning,
        format!(
            "the {backend} backend does not typeset {}: {count} in this field \
             will not reach the page",
            crate::quill::support::plural(construct, count)
        ),
    )
    .with_code(DECLINED_CONSTRUCT.to_string())
    .with_path(path.to_string())
    .with_args(args)
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
    fn formats_support_canvas_keys_off_visual_formats() {
        use OutputFormat::*;
        assert!(formats_support_canvas(&[Pdf, Svg, Png]));
        assert!(formats_support_canvas(&[Pdf, Svg]));
        assert!(formats_support_canvas(&[Png]));
        assert!(!formats_support_canvas(&[Pdf]));
        assert!(!formats_support_canvas(&[]));
    }
}
