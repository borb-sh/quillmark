//! Write a fresh `/AcroForm` (and an `/Info` `/Producer` stamp) onto a base PDF
//! via one incremental-update append.
//!
//! Technique A: style the real AcroForm fields and set `/NeedAppearances`, never
//! baking `/AP` appearance streams. Appearance synthesis is the viewer's job, so
//! flat rasterizers render the fields blank and values reach non-interactive
//! output only via the [`RenderedRegion`] sidecar.
//!
//! The background owns all visual chrome, so a widget is a transparent input
//! over it: no borders, no fills, and black as the only text color. What a
//! widget does choose is the type its value is *set in*, since that has to
//! match the background it sits on rather than decorate it: each field names
//! its own face, size, and justification, and every face so named is registered
//! in the form `/DR`. The form is built fresh from the spec; a foreign AcroForm
//! is never reconciled.

use pdf_writer::types::{AnnotationFlags, FieldFlags, FieldType as PwFieldType, Quadding, SigFlags};
use pdf_writer::writers::{Field, Form};
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, Str, TextStr};

use quillmark_core::RenderedRegion;

use crate::error::PdfError;
use crate::reader::{err, find_dict_value, parse_indirect_ref, ObjectIndex, UpdatedObject};
use crate::update::PdfUpdate;
use crate::writer::{
    alloc_id, append_refs_to_array_key, dict_object, to_ref, type1_font_object, OnNonArray,
};
use crate::{FieldSpec, FieldType, FormFont, TextAlign};

const CODE_PARSE: &str = "pdf::stamp_parse";
const CODE_EXISTING_ACROFORM: &str = "pdf::existing_acroform";

/// The fixed checkbox on-state export name. A checkbox [`FieldSpec`] carries
/// this as its `value` when checked, `None` when not.
pub const CHECKBOX_ON_STATE: &str = "Yes";

/// House-style default appearance: Helvetica, `0 Tf` (auto-size), black fill.
/// `Helv` is registered in the form `/DR` `/Font`.
///
/// The form-level `/DA` fallback only; a widget carries its own, built by
/// [`field_appearance`].
const DEFAULT_APPEARANCE: &[u8] = b"/Helv 0 Tf 0 g";

/// One widget's `/DA`: its face and size over the house black fill. `f32`'s
/// `Display` drops the trailing `.0`, so a whole-point size writes `12`, and an
/// absent size writes the `0 Tf` that defers to the viewer's auto-size.
///
/// A size that is not positive and finite falls back to that same `0 Tf`:
/// `font_size` is public, so the Typst helper's call-site asserts do not cover
/// every caller, and `NaN`/`inf` reach the `/DA` as a token no PDF number
/// grammar admits.
fn field_appearance(spec: &FieldSpec) -> Vec<u8> {
    let size = spec
        .font_size
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(0.0);
    let mut da = b"/".to_vec();
    da.extend_from_slice(spec.font.resource_name().as_bytes());
    da.extend_from_slice(format!(" {size} Tf 0 g").as_bytes());
    da
}

/// Every face named by a `/DA` this stamp writes: the variable-text widgets'
/// own, plus the Helvetica of the form-level [`DEFAULT_APPEARANCE`]. A checkbox
/// or signature writes no `/DA`, so its `font` names nothing.
fn fonts_used(fields: &[FieldSpec]) -> Vec<FormFont> {
    let mut fonts: Vec<FormFont> = fields
        .iter()
        .filter(|f| {
            matches!(
                f.field_type,
                FieldType::Text { .. } | FieldType::Choice { .. }
            )
        })
        .map(|f| f.font)
        .collect();
    fonts.push(FormFont::Helvetica);
    fonts.sort_by_key(|f| f.resource_name());
    fonts.dedup();
    fonts
}

/// Options for [`stamp`](crate::stamp).
#[derive(Debug, Clone)]
pub struct StampOptions {
    /// The `/Info` `/Producer` this stamp writes over whatever the base carries.
    pub producer: String,
}

impl Default for StampOptions {
    fn default() -> Self {
        Self {
            producer: format!("Quillmark {}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Stamp `fields` onto `base` as a fresh AcroForm via one incremental update,
/// and `/Info` `/Producer` with it. Geometry is not produced here; it is a
/// session-level query (see [`regions_of`]).
///
/// `base` must satisfy the reader's input contract (traditional-xref,
/// unencrypted, inline-annots, bounded-tree) and carry no `/AcroForm` of its
/// own, and each `rect` must already be final, finite, bottom-left PDF-point
/// geometry.
pub fn stamp(
    base: Vec<u8>,
    fields: &[FieldSpec],
    opts: &StampOptions,
) -> Result<Vec<u8>, PdfError> {
    for spec in fields {
        spec.assert_finite_rect()?;
    }

    let pdf = base;
    let idx = ObjectIndex::new(&pdf);
    let mut up = PdfUpdate::begin(&idx, Some(opts.producer.as_str()))?;

    if !fields.is_empty() {
        // Before any allocation: a second `/AcroForm` on the catalog is a dict
        // the spec does not define, and the base's own widgets stay live in the
        // page `/Annots` this update preserves.
        if find_dict_value(idx.dict(up.catalog_id, CODE_PARSE, "catalog")?, "AcroForm").is_some() {
            return Err(err(
                CODE_EXISTING_ACROFORM,
                "base PDF already carries an /AcroForm; strip its form before stamping",
            ));
        }

        let pages = up.resolve_pages(&idx, fields)?;
        let page_count = pages.len();

        let fonts = fonts_used(fields);
        let font_ids: Vec<u32> = fonts
            .iter()
            .map(|_| alloc_id(&mut up.next_id))
            .collect::<Result<_, _>>()?;
        let widget_ids: Vec<u32> = fields
            .iter()
            .map(|_| alloc_id(&mut up.next_id))
            .collect::<Result<_, _>>()?;
        let acroform_id = alloc_id(&mut up.next_id)?;

        // Grouped by page for the page-side `/Annots`.
        let mut widgets_by_page: Vec<Vec<u32>> = vec![Vec::new(); page_count];
        for (spec, &wid) in fields.iter().zip(&widget_ids) {
            widgets_by_page[spec.page].push(wid);
            let page_ref = to_ref(pages[spec.page].id)?;
            up.objects.push(UpdatedObject {
                id: wid,
                bytes: write_widget_object(spec, to_ref(wid)?, page_ref),
            });
        }

        for (font, &fid) in fonts.iter().zip(&font_ids) {
            up.objects
                .push(type1_font_object(fid, font.base_font(), None)?);
        }

        let has_signature = fields
            .iter()
            .any(|f| matches!(f.field_type, FieldType::Signature));
        let field_refs: Vec<Ref> = widget_ids
            .iter()
            .map(|&id| to_ref(id))
            .collect::<Result<_, _>>()?;
        let mut achunk = Chunk::new();
        {
            let mut form: Form<'_> = achunk.indirect(to_ref(acroform_id)?).start::<Form>();
            form.fields(field_refs);
            if has_signature {
                form.sig_flags(SigFlags::SIGNATURES_EXIST);
            }
            form.pair(Name(b"NeedAppearances"), true);
            form.default_appearance(Str(DEFAULT_APPEARANCE));
            // /DR << /Font << /Helv <font> .. >> >>
            {
                let mut dr = form.insert(Name(b"DR")).dict();
                let mut font_dict = dr.insert(Name(b"Font")).dict();
                for (font, &fid) in fonts.iter().zip(&font_ids) {
                    font_dict.pair(Name(font.resource_name().as_bytes()), to_ref(fid)?);
                }
            }
            form.finish();
        }
        up.objects.push(UpdatedObject {
            id: acroform_id,
            bytes: achunk.as_bytes().to_vec(),
        });

        // A widget is fillable only if reachable both ways: the catalog's
        // `/AcroForm /Fields` (added here) and the page's `/Annots` (below).
        let cat_dict = idx.dict(up.catalog_id, CODE_PARSE, "catalog")?;
        let mut cat_inner = cat_dict.to_vec();
        cat_inner.extend_from_slice(format!(" /AcroForm {acroform_id} 0 R").as_bytes());
        up.objects.push(dict_object(up.catalog_id, &cat_inner));

        for (page_idx, widget_refs) in widgets_by_page.iter().enumerate() {
            if widget_refs.is_empty() {
                continue;
            }
            let page_obj_id = pages[page_idx].id;
            let what = format!("page node {page_obj_id}");
            let pg_dict = idx.dict(page_obj_id, CODE_PARSE, &what)?;
            up.objects.push(dict_object(
                page_obj_id,
                &rewrite_page_with_annots(pg_dict, widget_refs)?,
            ));
        }
    }

    up.finish(pdf)
}

/// One [`RenderedRegion`] per field carrying a schema address, keyed on that
/// address. A widget with no schema field is a backend-only artifact and emits
/// nothing. Shared by `stamp` and the no-stamp render paths, so region geometry
/// always matches the widget.
pub fn regions_of(fields: &[FieldSpec]) -> Vec<RenderedRegion> {
    fields
        .iter()
        .filter_map(|f| {
            Some(RenderedRegion::new(
                f.schema_field.clone()?,
                f.page,
                f.rect,
            ))
        })
        .collect()
}

/// Left is the PDF's own `/Q` default, so it is left unwritten: a field that
/// never touches this dial stamps exactly as it did before the dial existed.
fn write_quadding(field: &mut Field<'_>, align: TextAlign) {
    let q = match align {
        TextAlign::Left => return,
        TextAlign::Center => Quadding::Center,
        TextAlign::Right => Quadding::Right,
    };
    field.vartext_quadding(q);
}

/// Serialize one field as a merged field+widget indirect object.
fn write_widget_object(spec: &FieldSpec, wid: Ref, page_ref: Ref) -> Vec<u8> {
    let mut chunk = Chunk::new();
    {
        let mut field = chunk.form_field(wid);
        field.partial_name(TextStr(&spec.name));
        if let Some(tt) = spec.tooltip.as_deref() {
            field.alternate_name(TextStr(tt));
        }

        // Captured to also set the annotation `/AS` below.
        let mut checkbox_on: Option<bool> = None;
        let da = field_appearance(spec);

        match &spec.field_type {
            FieldType::Text { multiline } => {
                field.field_type(PwFieldType::Text);
                field.vartext_default_appearance(Str(&da));
                write_quadding(&mut field, spec.align);
                if *multiline {
                    field.field_flags(FieldFlags::MULTILINE);
                }
                if let Some(v) = spec.value.as_deref() {
                    field.text_value(TextStr(v));
                }
            }
            FieldType::Checkbox => {
                field.field_type(PwFieldType::Button);
                let on = spec.value.is_some();
                checkbox_on = Some(on);
                field.pair(
                    Name(b"V"),
                    if on {
                        Name(CHECKBOX_ON_STATE.as_bytes())
                    } else {
                        Name(b"Off")
                    },
                );
                // /MK /CA (4): the ZapfDingbats check glyph the viewer
                // synthesizes under NeedAppearances.
                {
                    let mut mk = field.insert(Name(b"MK")).dict();
                    mk.pair(Name(b"CA"), Str(b"4"));
                }
            }
            FieldType::Choice { options } => {
                field.field_type(PwFieldType::Choice);
                field.field_flags(FieldFlags::COMBO);
                field.vartext_default_appearance(Str(&da));
                write_quadding(&mut field, spec.align);
                {
                    let mut opts = field.choice_options();
                    for o in options {
                        opts.option(TextStr(o));
                    }
                }
                if let Some(v) = spec.value.as_deref() {
                    field.choice_value(Some(TextStr(v)));
                }
            }
            FieldType::Signature => {
                field.field_type(PwFieldType::Signature);
            }
        }

        // `into_annotation` writes `/Type /Annot` + `/Subtype /Widget` exactly
        // once; do not call `.subtype()` again or it duplicates the key.
        let mut ann = field.into_annotation();
        ann.rect(Rect::new(
            spec.rect[0],
            spec.rect[1],
            spec.rect[2],
            spec.rect[3],
        ))
        .page(page_ref)
        .flags(AnnotationFlags::PRINT);
        if let Some(on) = checkbox_on {
            ann.appearance_state(if on {
                Name(CHECKBOX_ON_STATE.as_bytes())
            } else {
                Name(b"Off")
            });
        }
        ann.finish();
    }
    chunk.as_bytes().to_vec()
}

/// Three cases for the existing `/Annots`: absent (write a fresh array);
/// inline array (splice widget refs before `]`); indirect reference (hard
/// error, the input contract requires inline annots).
fn rewrite_page_with_annots(pg_dict: &[u8], widget_refs: &[u32]) -> Result<Vec<u8>, PdfError> {
    append_refs_to_array_key(
        pg_dict,
        "Annots",
        widget_refs,
        CODE_PARSE,
        OnNonArray::Reject(non_array_annots),
    )
}

fn non_array_annots(existing: &[u8]) -> PdfError {
    if parse_indirect_ref(existing).is_some() {
        err(
            "pdf::indirect_annots",
            "/Annots is an indirect reference; only inline arrays are supported",
        )
    } else {
        err(CODE_PARSE, "/Annots is neither array nor indirect ref")
    }
}
