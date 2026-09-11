//! The form-field adapter: a thin introspection→[`FieldSpec`] bridge onto the
//! shared `quillmark-pdf` stamping spine. Typst→PDF coordinate ownership lives
//! here so the spine never imports `typst_layout`.

use quillmark_core::RenderError;
use quillmark_pdf::{FieldSpec, FieldType, FormFont, TextAlign};
use typst_layout::PagedDocument;

mod extract;
mod span_scan;

pub(crate) use extract::extract;
pub(crate) use span_scan::{scalar_windows, unclosed_claims, FieldWindow, Scan};

/// One form field's geometry in Typst (top-left origin) points, plus its
/// type/value payload.
#[derive(Debug)]
pub(crate) struct FieldPlacement {
    pub name: String,
    /// The `field:` argument; `None` when the plate omits it, and the widget
    /// then exposes no region.
    pub schema_field: Option<String>,
    pub page: usize,
    pub rect_typst_pt: [f32; 4],
    pub field_type: FieldType,
    pub value: Option<String>,
    pub font: FormFont,
    pub font_size: Option<f32>,
    pub align: TextAlign,
}

/// Flips each rect from Typst's top-left origin to the PDF bottom-left origin
/// the spine consumes. The two backends meet only at the `&[FieldSpec]` seam.
pub(crate) fn build_field_specs(
    doc: &PagedDocument,
    placements: &[FieldPlacement],
) -> Result<Vec<FieldSpec>, RenderError> {
    let page_heights: Vec<f32> = doc
        .pages()
        .iter()
        .map(|p| p.frame.size().y.to_pt() as f32)
        .collect();

    placements
        .iter()
        .map(|p| {
            let page_h = *page_heights.get(p.page).ok_or_else(|| {
                RenderError::coded(
                    "typst::form_field_page_out_of_range",
                    format!(
                        "form-field {:?} targets page {} but the document has {} page(s)",
                        p.name,
                        p.page,
                        page_heights.len()
                    ),
                )
            })?;
            let [x0, y0, x1, y1] = p.rect_typst_pt;
            // Typst top-left → PDF bottom-left.
            let mut spec = FieldSpec::new(
                p.name.clone(),
                p.page,
                [x0, page_h - y1, x1, page_h - y0],
                p.field_type.clone(),
            );
            spec.schema_field = p.schema_field.clone();
            spec.value = p.value.clone();
            spec.font = p.font;
            spec.font_size = p.font_size;
            spec.align = p.align;
            Ok(spec)
        })
        .collect()
}
