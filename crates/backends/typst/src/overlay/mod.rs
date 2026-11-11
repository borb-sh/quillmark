//! The form-field adapter: a thin introspection→[`FieldSpec`] bridge onto the
//! shared `quillmark-pdf` stamping spine. Typst→PDF coordinate ownership lives
//! here so the spine never imports `typst_layout`.

use quillmark_core::error::RenderError;
use quillmark_core::quill::{
    FieldSchema, FieldType as SchemaType, QuillConfig, VARIANT_DISCRIMINANT_KEY,
};
use quillmark_pdf::{FieldSpec, FieldType, FormFont, TextAlign};
use typst_layout::PagedDocument;

mod extract;
mod span_scan;

pub(crate) use extract::extract;
pub(crate) use span_scan::{FieldWindow, Scan, unclosed_claims};

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
    /// The `tooltip:` argument; `None` inherits the schema description at
    /// `schema_field` in [`build_field_specs`].
    pub tooltip: Option<String>,
}

/// Flips each rect from Typst's top-left origin to the PDF bottom-left origin
/// the spine consumes. The two backends meet only at the `&[FieldSpec]` seam.
/// A widget's tooltip is its `tooltip:` argument, else the `description` of
/// the schema field at its `field:` address, as an acroform widget's is.
pub(crate) fn build_field_specs(
    doc: &PagedDocument,
    placements: &[FieldPlacement],
    config: &QuillConfig,
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
            spec.tooltip = p.tooltip.clone().or_else(|| {
                p.schema_field
                    .as_deref()
                    .and_then(|path| description_at(config, path))
                    .map(str::to_string)
            });
            Ok(spec)
        })
        .collect()
}

/// The `description` declared at a schema address, walked the way the
/// helper's `_qm-node` admits it: a card address resolves through its kind,
/// `$body` to the card's body, and a variant container's discriminant to the
/// container itself. The helper has already rejected an unknown address, so a
/// step that fails here just yields no description.
fn description_at<'a>(config: &'a QuillConfig, path: &str) -> Option<&'a str> {
    let mut parts = path.split('.');
    let root = parts.next()?;
    let (card, head) = if root == "$cards" {
        let card = config.card_kind(parts.next()?)?;
        parts.next()?;
        (card, parts.next()?)
    } else {
        (&config.main, root)
    };
    if head == "$body" {
        return card.body.as_ref()?.description.as_deref();
    }
    let mut cur: &FieldSchema = card.fields.get(head)?;
    for seg in parts {
        if cur.is_variant_bearing() && seg == VARIANT_DISCRIMINANT_KEY {
            break;
        }
        cur = match (&cur.r#type, seg.parse::<usize>()) {
            (SchemaType::Array, Ok(_)) => cur.items.as_deref()?,
            (SchemaType::Object, Err(_)) => cur.namespace_props()?.get(seg)?,
            (SchemaType::Matrix { .. }, Err(_)) => cur.matrix_member_at(seg)?,
            (_, Err(_)) if cur.is_variant_bearing() => cur.variant_field(seg)?,
            _ => return None,
        };
    }
    cur.description.as_deref()
}
