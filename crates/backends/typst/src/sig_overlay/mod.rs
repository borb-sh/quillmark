//! Adds AcroForm SigField widgets to a typst_pdf-produced PDF via an
//! incremental update. Public entry points called from `compile.rs`:
//! [`extract`] walks the Typst document and returns placements; [`inject`]
//! applies them to the PDF.

use quillmark_core::RenderError;
use typst::layout::PagedDocument;

mod extract;
mod inject;

// The byte-level PDF scanner and `err` helper moved to the crate-level
// `pdf_scan` module when `meta_overlay` became a second consumer. Re-export
// them under the original names so the submodules' `super::scanner::…` and
// `super::err` paths keep resolving.
pub(super) use crate::pdf_scan as scanner;
pub(super) use crate::pdf_scan::err;

/// One signature field's name + page + rect in Typst (top-left origin) points.
/// The PDF inject pass converts to bottom-left.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SigPlacement {
    pub name: String,
    pub page: usize,
    pub rect_typst_pt: [f32; 4],
}

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<SigPlacement>, RenderError> {
    extract::extract(doc)
}

pub(crate) fn inject(
    pdf: Vec<u8>,
    doc: &PagedDocument,
    placements: &[SigPlacement],
) -> Result<Vec<u8>, RenderError> {
    inject::inject(pdf, doc, placements)
}
