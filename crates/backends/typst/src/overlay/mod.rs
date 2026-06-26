//! The Typst backend's adapter onto the shared `quillmark-pdf` stamping layer.
//!
//! [`extract`] walks the compiled Typst document for `signature-field` calls,
//! yielding [`SigPlacement`]s in Typst (top-left origin) points. [`inject`]
//! converts those to `quillmark_pdf::FieldSpec`s in PDF (bottom-left origin)
//! space — the one thing the Typst backend contributes that `pdfform` doesn't —
//! and hands them to `quillmark_pdf::stamp`, which writes the AcroForm and the
//! `/Producer` stamp in one incremental update.

use quillmark_core::{Diagnostic, RenderError, RenderedRegion, Severity};
use quillmark_pdf::{stamp, FieldSpec, FieldType, StampOptions};
use typst_layout::PagedDocument;

mod extract;

pub(crate) use quillmark_pdf::default_producer;

/// One signature field's name + page + rect in Typst (top-left origin) points.
/// [`inject`] converts to PDF bottom-left before stamping.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SigPlacement {
    pub name: String,
    pub page: usize,
    pub rect_typst_pt: [f32; 4],
}

/// Build a single-`Diagnostic` `RenderError` with `code`, used by the extract
/// walk for the handful of internal-invariant failures it can hit.
pub(crate) fn err(code: &'static str, msg: impl Into<String>) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, msg.into()).with_code(code.into())],
    }
}

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<SigPlacement>, RenderError> {
    extract::extract(doc)
}

/// Stamp the `/Producer` metadata plus one signature widget per placement onto
/// `pdf`, returning the stamped bytes and the phase-1 regions sidecar (one
/// region per signature field). `producer` is the already-resolved `/Producer`
/// string (default or a caller override). Placements convert top-left Typst
/// points to bottom-left PDF points using each page's height from the compiled
/// document.
pub(crate) fn inject(
    pdf: Vec<u8>,
    doc: &PagedDocument,
    placements: &[SigPlacement],
    producer: &str,
) -> Result<(Vec<u8>, Vec<RenderedRegion>), RenderError> {
    let page_heights_pt: Vec<f32> = doc
        .pages()
        .iter()
        .map(|p| p.frame.size().y.to_pt() as f32)
        .collect();

    let fields: Vec<FieldSpec> = placements
        .iter()
        .map(|p| {
            let [x0, y0, x1, y1] = p.rect_typst_pt;
            // Typst top-left → PDF bottom-left. `p.page` is in range by
            // construction (extract reads it from the document's introspector).
            let page_h = page_heights_pt[p.page];
            FieldSpec::new(
                p.name.clone(),
                p.page,
                [x0, page_h - y1, x1, page_h - y0],
                FieldType::Signature,
            )
        })
        .collect();

    let result = stamp(
        pdf,
        &fields,
        &StampOptions {
            producer: Some(producer.to_string()),
        },
    )?;
    Ok((result.pdf, result.regions))
}
