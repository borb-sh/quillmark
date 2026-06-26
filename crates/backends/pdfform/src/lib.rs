//! # quillmark-pdfform
//!
//! A greenfield Quillmark backend dedicated to filling government PDF forms.
//!
//! A `pdfform` quill ships two assets the qualification ("quillifying") layer
//! produced upstream:
//!
//! - **`form.pdf`** — the *stripped background*: the normalized gov form with
//!   its `/AcroForm`, widget annotations and page `/Annots` removed (pure pages
//!   + content streams); and
//! - **`form.json`** — the complete field reconstruction spec.
//!
//! The backend reads `form.json`, binds each field to the document's resolved
//! value, and hands the resulting `&[FieldSpec]` to `quillmark_pdf::stamp`,
//! which writes the AcroForm **fresh** onto the background. It never reads or
//! reconciles a foreign AcroForm — both backends do the same single
//! "stamp from spec" operation, differing only in where geometry comes from
//! (here: `form.json`; for Typst: introspection).
//!
//! See issue #744 for the surrounding architecture.

mod form;
#[cfg(feature = "preview")]
mod preview;

pub use form::{FormField, FormFieldKind, FormSpec};

use std::any::Any;

use quillmark_core::session::SessionHandle;
use quillmark_core::{
    Artifact, Backend, Diagnostic, OutputFormat, Quill, RenderError, RenderOptions, RenderResult,
    RenderSession, RenderedRegion, Severity,
};
use quillmark_pdf::{stamp, FieldSpec, StampOptions};

/// Without the `preview` feature pdfform produces only the filled PDF. With it,
/// hayro adds SVG output and the raster-preview capability (canvas paint).
#[cfg(not(feature = "preview"))]
const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Pdf];
#[cfg(feature = "preview")]
const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Pdf, OutputFormat::Svg];

/// The `pdfform` backend.
#[derive(Debug)]
pub struct PdfformBackend;

/// A prepared form: the stripped background plus the field specs (geometry +
/// bound values) ready to stamp, and the regions sidecar reported on every
/// render. Built once in [`Backend::open`]; cheap to re-render across requests.
#[derive(Debug)]
pub struct PdfformSession {
    background: Vec<u8>,
    fields: Vec<FieldSpec>,
    regions: Vec<RenderedRegion>,
    page_count: usize,
}

impl SessionHandle for PdfformSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);
        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(RenderError::FormatNotSupported {
                diags: vec![Diagnostic::new(
                    Severity::Error,
                    format!("pdfform backend does not support {format:?} output"),
                )
                .with_code("pdfform::format_not_supported".to_string())],
            });
        }

        let result = match format {
            OutputFormat::Pdf => {
                let stamped = stamp(
                    self.background.clone(),
                    &self.fields,
                    &StampOptions {
                        producer: opts.producer.clone(),
                    },
                )?;
                RenderResult::new(
                    vec![Artifact {
                        bytes: stamped.pdf,
                        output_format: OutputFormat::Pdf,
                    }],
                    OutputFormat::Pdf,
                )
            }
            #[cfg(feature = "preview")]
            OutputFormat::Svg => {
                // SVG is the *stripped background* (vector). Field values are
                // not painted — hayro doesn't render NeedAppearances fields —
                // so the GUI composites them from the regions sidecar.
                let artifacts = preview::background_svgs(&self.background, opts.pages.as_deref())?;
                RenderResult::new(artifacts, OutputFormat::Svg)
            }
            // Unreachable: SUPPORTED_FORMATS gates `format` above.
            other => {
                return Err(RenderError::FormatNotSupported {
                    diags: vec![Diagnostic::new(
                        Severity::Error,
                        format!("pdfform backend does not support {other:?} output"),
                    )
                    .with_code("pdfform::format_not_supported".to_string())],
                })
            }
        };

        // The regions sidecar rides on every render, regardless of format — the
        // GUI overlay needs field geometry whether it shows the PDF or the raster.
        Ok(result.with_regions(self.regions.clone()))
    }

    fn page_count(&self) -> usize {
        self.page_count
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Render the stripped background (not the stamped PDF) to a
    /// non-premultiplied RGBA8 pixmap via hayro. Field values are composited by
    /// the GUI from the regions sidecar.
    #[cfg(feature = "preview")]
    fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        preview::render_rgba(&self.background, page, scale)
    }

    #[cfg(feature = "preview")]
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        preview::page_size_pt(&self.background, page)
    }
}

impl Backend for PdfformBackend {
    fn id(&self) -> &'static str {
        "pdfform"
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        SUPPORTED_FORMATS
    }

    /// With `preview`, the session implements the raster capability
    /// (`render_rgba`), so the canvas paint path is reachable; without it the
    /// background can't be rasterized and canvas is unavailable.
    #[cfg(feature = "preview")]
    fn supports_canvas(&self) -> bool {
        true
    }

    fn open(
        &self,
        _plate_content: &str,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<RenderSession, RenderError> {
        let background = source
            .files()
            .get_file("form.pdf")
            .ok_or_else(|| {
                err(
                    "pdfform::missing_background",
                    "pdfform quill is missing its stripped background 'form.pdf'",
                )
            })?
            .to_vec();

        let form_json = source.files().get_file("form.json").ok_or_else(|| {
            err(
                "pdfform::missing_form_json",
                "pdfform quill is missing 'form.json'",
            )
        })?;
        let spec: FormSpec = serde_json::from_slice(form_json)
            .map_err(|e| err("pdfform::form_json_parse", format!("form.json: {e}")))?;

        let fields: Vec<FieldSpec> = spec
            .fields
            .iter()
            .map(|f| {
                let value = f
                    .schema_field
                    .as_deref()
                    .and_then(|key| lookup(json_data, key));
                f.to_field_spec(value)
            })
            .collect();

        let regions = fields.iter().map(FieldSpec::to_region).collect();
        let page_count = quillmark_pdf::page_count(&background)?;

        Ok(RenderSession::new(Box::new(PdfformSession {
            background,
            fields,
            regions,
            page_count,
        })))
    }
}

/// Resolve a schema field's value from the compiled plate JSON. Fields live at
/// the top level of the object; a missing key yields `None`.
fn lookup<'a>(json_data: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    json_data.get(key).filter(|v| !v.is_null())
}

fn err(code: &'static str, msg: impl Into<String>) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, msg.into()).with_code(code.into())],
    }
}
