//! Walk a compiled Typst document and return one `FieldPlacement` per
//! `form-field` call. The helper wraps a `<__qm_field__>`-labelled `metadata`
//! inside an invisible box of the widget's size, where the tag is the body
//! flow's first item and so lands at the body origin. That origin is the box's
//! top-left in every layout context, so `introspector.position()` is the
//! widget's top-left: no frame walk.

use std::collections::HashMap;

use typst::foundations::{Dict, FromValue, Label, Selector, Str, Value};
use typst::introspection::{Introspector, Location};
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

use quillmark_core::{Diagnostic, RenderError, Severity};

use quillmark_pdf::{FieldType, FormFont, TextAlign};

use super::FieldPlacement;

const FIELD_LABEL: &str = "__qm_field__";
const CODE_INTERNAL: &str = "typst::overlay_internal";

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<FieldPlacement>, RenderError> {
    let intro = doc.introspector();
    let label = Label::new(PicoStr::intern(FIELD_LABEL)).ok_or_else(|| {
        RenderError::coded(
            CODE_INTERNAL,
            "FIELD_LABEL must be a non-empty interned string",
        )
    })?;
    let elems = intro.query(&Selector::Label(label));
    if elems.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_name: HashMap<String, Location> = HashMap::new();
    let mut placements: Vec<FieldPlacement> = Vec::with_capacity(elems.len());

    for c in elems.iter() {
        let dict = match c.get_by_name("value") {
            Ok(Value::Dict(d)) => d,
            Ok(other) => {
                return Err(RenderError::coded(
                    CODE_INTERNAL,
                    format!("expected metadata value to be a dict, got {}", other.ty()),
                ))
            }
            Err(e) => {
                return Err(RenderError::coded(
                    CODE_INTERNAL,
                    format!("metadata.value missing: {e:?}"),
                ))
            }
        };
        if get::<Str>(&dict, "kind")?.as_str() != FIELD_LABEL {
            // User attached <__qm_field__> to unrelated metadata; ignore it.
            continue;
        }
        let name = get::<Str>(&dict, "name")?.to_string();
        let schema_field = get::<Option<Str>>(&dict, "field")?.map(|s| s.to_string());
        let field_type = get::<Str>(&dict, "field-type")?;
        let width = get::<f64>(&dict, "width")?;
        let height = get::<f64>(&dict, "height")?;
        let (field_type, value) = read_field_kind(&dict, field_type.as_str())?;
        let font = read_font(&dict)?;
        let font_size = get::<Option<f64>>(&dict, "size")?.map(|s| s as f32);
        let align = read_align(&dict)?;
        let loc = c
            .location()
            .ok_or_else(|| {
                RenderError::coded(CODE_INTERNAL, "form-field metadata is not located")
            })?;

        if let Some(&prior) = by_name.get(&name) {
            return Err(duplicate_field_error(&name, prior, loc));
        }
        by_name.insert(name.clone(), loc);

        let pos = intro
            .position(loc)
            .ok_or_else(|| {
                RenderError::coded(CODE_INTERNAL, "form-field metadata has no position")
            })?;
        placements.push(FieldPlacement {
            name,
            schema_field,
            page: pos.page.get().saturating_sub(1),
            rect_typst_pt: [
                pos.point.x.to_pt() as f32,
                pos.point.y.to_pt() as f32,
                (pos.point.x.to_pt() + width) as f32,
                (pos.point.y.to_pt() + height) as f32,
            ],
            field_type,
            value,
            font,
            font_size,
            align,
        });
    }

    // Stable sort keeps the introspector's document order (= paint order)
    // within a page; `field_at` resolves overlapping widgets by later-painted-wins.
    placements.sort_by_key(|p| p.page);
    Ok(placements)
}

fn read_field_kind(
    d: &Dict,
    field_type: &str,
) -> Result<(FieldType, Option<String>), RenderError> {
    match field_type {
        "text" => Ok((
            FieldType::Text {
                multiline: get::<Option<bool>>(d, "multiline")?.unwrap_or(false),
            },
            read_value_str(d, "value")?,
        )),
        "signature" => Ok((FieldType::Signature, None)),
        other => Err(RenderError::coded(
            CODE_INTERNAL,
            format!("unknown form-field type {other:?}"),
        )),
    }
}

/// The helper's asserts already reject anything outside these sets, so an
/// unknown string here means the metadata was hand-forged, not mistyped.
fn read_font(d: &Dict) -> Result<FormFont, RenderError> {
    match get::<Str>(d, "font")?.as_str() {
        "helvetica" => Ok(FormFont::Helvetica),
        "times" => Ok(FormFont::Times),
        "courier" => Ok(FormFont::Courier),
        other => Err(RenderError::coded(
            CODE_INTERNAL,
            format!("unknown form-field font {other:?}"),
        )),
    }
}

fn read_align(d: &Dict) -> Result<TextAlign, RenderError> {
    match get::<Str>(d, "align")?.as_str() {
        "left" => Ok(TextAlign::Left),
        "center" => Ok(TextAlign::Center),
        "right" => Ok(TextAlign::Right),
        other => Err(RenderError::coded(
            CODE_INTERNAL,
            format!("unknown form-field align {other:?}"),
        )),
    }
}

/// A missing key reads as `none`: an optional target then yields `None`, a
/// required one fails the cast.
fn get<T: FromValue>(d: &Dict, key: &str) -> Result<T, RenderError> {
    d.get(key)
        .cloned()
        .unwrap_or(Value::None)
        .cast()
        .map_err(|e| {
            RenderError::coded(CODE_INTERNAL, format!("metadata.{key}: {}", e.message()))
        })
}

/// An empty string yields `None` so the widget carries no `/V` (mirrors
/// acroform's `coerce_text`).
fn read_value_str(d: &Dict, key: &str) -> Result<Option<String>, RenderError> {
    let s = match d.get(key) {
        Ok(Value::Str(s)) => s.to_string(),
        Ok(Value::Int(i)) => i.to_string(),
        Ok(Value::Float(f)) => f.to_string(),
        Ok(Value::Bool(b)) => b.to_string(),
        Ok(Value::None) | Err(_) => return Ok(None),
        Ok(other) => {
            return Err(RenderError::coded(
                CODE_INTERNAL,
                format!(
                    "expected metadata.{key} to be str/int/float/bool/none, got {}",
                    other.ty()
                ),
            ))
        }
    };
    Ok((!s.is_empty()).then_some(s))
}

/// Quote the name first so downstream parsers can extract it with a stable
/// first-quoted-token convention.
fn duplicate_field_error(name: &str, first: Location, second: Location) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(
            Severity::Error,
            format!("{name:?} is defined twice: each form-field name must be unique"),
        )
        .with_code("typst::duplicate_form_field".to_string())
        .with_hint(format!(
            "Rename one of the calls. Conflicting Typst location ids: {first:?}, {second:?}"
        )),
    )
}
