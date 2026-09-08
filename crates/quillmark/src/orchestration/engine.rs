use quillmark_core::{
    Backend, Diagnostic, Document, LiveSession, OutputFormat, Quill, RenderError, RenderOptions,
    RenderResult, Severity,
};
use std::collections::HashMap;
use std::sync::Arc;

/// A backend registry and render dispatcher: the sole home of
/// backend-dependent surface, resolving a [`Quill`]'s *declared* backend at
/// render time. Quill loading needs no engine — see [`Quill::from_tree`] or
/// [`quill_from_path`](crate::quill_from_path).
pub struct Quillmark {
    backends: HashMap<String, Arc<dyn Backend>>,
}

impl Quillmark {
    /// An engine with a backend registered per enabled cargo feature.
    pub fn new() -> Self {
        // `mut` is unused when no backend features are enabled (e.g. a
        // Typst-less core build).
        #[allow(unused_mut)]
        let mut engine = Self {
            backends: HashMap::new(),
        };

        #[cfg(feature = "typst")]
        {
            engine.register_backend(Box::new(quillmark_typst::TypstBackend));
        }

        #[cfg(feature = "pdfform")]
        {
            engine.register_backend(Box::new(quillmark_pdfform::PdfformBackend));
        }

        engine
    }

    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn register_backend(&mut self, backend: Box<dyn Backend>) {
        let id = backend.id().to_string();
        self.backends.insert(id, Arc::from(backend));
    }

    /// The registered backend ids.
    pub fn registered_backends(&self) -> Vec<&str> {
        self.backends.keys().map(|s| s.as_str()).collect()
    }

    /// Errors with `engine::backend_not_found` when none is registered. The
    /// check lives at render time, not load time, so a backend-less build can
    /// still load and validate quills.
    fn resolve_backend(&self, quill: &Quill) -> Result<&Arc<dyn Backend>, RenderError> {
        let backend_id = quill.backend_id();
        self.backends.get(backend_id).ok_or_else(|| {
            RenderError::from_diag(
                Diagnostic::new(
                    Severity::Error,
                    format!("Backend '{}' not registered or not enabled", backend_id),
                )
                .with_code("engine::backend_not_found".to_string())
                .with_hint(format!(
                    "Available backends: {}",
                    self.backends.keys().cloned().collect::<Vec<_>>().join(", ")
                )),
            )
        })
    }

    /// Open a live render session for `doc` against `quill`'s backend.
    pub fn open(&self, quill: &Quill, doc: &Document) -> Result<LiveSession, RenderError> {
        let backend = self.resolve_backend(quill)?;
        let json_data = quill.compile_checked(doc)?;
        backend.open(quill, &json_data)
    }

    /// Render `doc` against `quill` in one shot. Convenience over
    /// [`open`](Self::open) + [`LiveSession::render`]: an unset
    /// `output_format` falls back to the backend's first supported format.
    pub fn render(
        &self,
        quill: &Quill,
        doc: &Document,
        opts: &RenderOptions,
    ) -> Result<RenderResult, RenderError> {
        let backend = self.resolve_backend(quill)?;
        let default_format = backend.supported_formats().first().copied();
        let session = backend.open(quill, &quill.compile_checked(doc)?)?;
        // Clone-and-narrow so a new RenderOptions field is carried through by
        // default; only `output_format` gets the backend-default fallback.
        let mut resolved = opts.clone();
        resolved.output_format = opts.output_format.or(default_format);
        session.render(&resolved)
    }

    /// The output formats `quill`'s backend can emit; compiles nothing.
    pub fn supported_formats(&self, quill: &Quill) -> Result<&'static [OutputFormat], RenderError> {
        Ok(self.resolve_backend(quill)?.supported_formats())
    }

    /// Pre-session hint for whether `quill`'s backend can paint sessions to a
    /// canvas, derived from its output formats; `false` when the backend is
    /// unregistered. Compiles nothing. Once a session exists,
    /// [`LiveSession::supports_canvas`](quillmark_core::LiveSession::supports_canvas)
    /// is authoritative.
    pub fn supports_canvas(&self, quill: &Quill) -> bool {
        self.resolve_backend(quill)
            .map(|b| quillmark_core::formats_support_canvas(b.supported_formats()))
            .unwrap_or(false)
    }
}

impl Default for Quillmark {
    fn default() -> Self {
        Self::new()
    }
}
