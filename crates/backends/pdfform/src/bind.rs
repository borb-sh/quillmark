//! Resolve every `form.json` field against the two static inputs (the quill
//! schema and the background geometry) into the session's value-free widget
//! layer. Pure in the quill, so it runs once at open: a widget bound to a
//! nonexistent field or an out-of-range page is a load error, not a silent blank.

use quillmark_core::quill::{
    FieldSchema, FieldType as SchemaType, QuillConfig, VARIANT_DISCRIMINANT_KEY,
};
use quillmark_pdf::FieldType as WidgetType;

use crate::form::{BoundField, FormSpec, Rect, UnboundWidget, WidgetKind};

/// A `form.json` field with its static parts resolved: `rect` is final
/// bottom-left PDF geometry, and the widget type is value-free.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundWidget {
    pub name: String,
    pub schema_field: Option<String>,
    pub page: usize,
    pub rect: [f32; 4],
    pub field_type: WidgetType,
    pub tooltip: Option<String>,
}

/// Why binding a `form.json` field failed. Every variant is a load error.
#[derive(Debug)]
pub enum BindError {
    /// A `schema_field` path does not resolve; names the failing segment.
    Dangling {
        name: String,
        path: String,
        segment: String,
    },
    /// A `schema_field` resolves to a schema type with no widget shape.
    Unbindable {
        name: String,
        path: String,
        ty: String,
    },
    PageOutOfRange {
        name: String,
        page: usize,
        page_count: usize,
    },
}

impl BindError {
    /// The stable diagnostic code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            BindError::Dangling { .. } => "pdfform::dangling_binding",
            BindError::Unbindable { .. } => "pdfform::unbindable_field",
            BindError::PageOutOfRange { .. } => "pdfform::field_page_out_of_range",
        }
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Dangling {
                name,
                path,
                segment,
            } => write!(
                f,
                "form.json field {name:?} binds schema_field {path:?}, but segment {segment:?} \
                 does not resolve against the quill schema"
            ),
            BindError::Unbindable { name, path, ty } => write!(
                f,
                "form.json field {name:?} binds schema_field {path:?}, which resolves to schema \
                 type `{ty}`: no widget can render it; bind a scalar, enum, boolean, or an array \
                 of those instead"
            ),
            BindError::PageOutOfRange {
                name,
                page,
                page_count,
            } => write!(
                f,
                "form.json field {name:?} targets page {page} but `form.pdf` has {page_count} page(s)"
            ),
        }
    }
}

/// Resolve and place every field in `spec` against the schema and the page
/// geometry, yielding the session's value-free widget layer.
pub fn bind_widgets(
    spec: &FormSpec,
    config: &QuillConfig,
    canvas_boxes: &[[f32; 4]],
) -> Result<Vec<BoundWidget>, BindError> {
    let mut bound = Vec::with_capacity(spec.fields.len() + spec.widgets.len());
    for field in &spec.fields {
        bound.push(bind_field(field, config, canvas_boxes)?);
    }
    for widget in &spec.widgets {
        bound.push(bind_unbound(widget, canvas_boxes)?);
    }
    Ok(bound)
}

/// An explicit `tooltip` overrides the schema `description`.
fn bind_field(
    field: &BoundField,
    config: &QuillConfig,
    canvas_boxes: &[[f32; 4]],
) -> Result<BoundWidget, BindError> {
    let schema = bind(config, &field.name, &field.schema_field)?;
    let field_type = project_kind(schema, &field.name, &field.schema_field)?;
    let tooltip = field
        .tooltip
        .clone()
        .or_else(|| schema.description.clone());
    Ok(BoundWidget {
        name: field.name.clone(),
        schema_field: Some(field.schema_field.clone()),
        page: field.page,
        rect: place(&field.name, field.page, field.rect, canvas_boxes)?,
        field_type,
        tooltip,
    })
}

fn bind_unbound(
    widget: &UnboundWidget,
    canvas_boxes: &[[f32; 4]],
) -> Result<BoundWidget, BindError> {
    Ok(BoundWidget {
        name: widget.name.clone(),
        schema_field: None,
        page: widget.page,
        rect: place(&widget.name, widget.page, widget.rect, canvas_boxes)?,
        field_type: widget_type(&widget.kind),
        tooltip: widget.tooltip.clone(),
    })
}

fn place(
    name: &str,
    page: usize,
    rect: Rect,
    canvas_boxes: &[[f32; 4]],
) -> Result<[f32; 4], BindError> {
    let canvas_box = canvas_boxes
        .get(page)
        .ok_or_else(|| BindError::PageOutOfRange {
            name: name.to_string(),
            page,
            page_count: canvas_boxes.len(),
        })?;
    Ok(flip_rect(rect, *canvas_box))
}

/// Page-relative top-left `{x,y,w,h}` → bottom-left `[x0, y0, x1, y1]` in PDF
/// user space. `x`/`y` measure from the corner of the page a viewer displays, so
/// they flip against the canvas box (`/CropBox` ∩ `/MediaBox`): on a page whose
/// canvas starts at `[10 20 622 812]`, a widget lands 10 pt right and 20 pt up of
/// where user-space `(0,0)` would put it.
fn flip_rect(r: Rect, canvas_box: [f32; 4]) -> [f32; 4] {
    let left = canvas_box[0];
    let top = canvas_box[3];
    [left + r.x, top - (r.y + r.h), left + r.x + r.w, top - r.y]
}

/// Resolve a `schema_field` path to the leaf [`FieldSchema`] it addresses.
///
/// The root segment resolves in `main.fields`, or is the reserved `$cards`:
/// `$cards.<kind>.<i>.<field>…` addresses a card field. An absolute index
/// (`$cards.<i>.…`) is rejected — only the kind names which schema field the
/// slot binds, and the widget's kind must be derivable at load.
///
/// A variant container is addressed through its resting shape rather than as a
/// whole: `classification.value` is the discriminant and `classification.<cell>`
/// a variant field, active world or not, since a form binds against the schema
/// once and the document selects its world later. The container itself resolves
/// but binds nothing — its value is the container object, which no widget
/// coerces — so a plate wanting the level binds `.value`.
pub fn bind<'a>(
    config: &'a QuillConfig,
    name: &str,
    path: &str,
) -> Result<&'a FieldSchema, BindError> {
    let dangling = |segment: &str| BindError::Dangling {
        name: name.to_string(),
        path: path.to_string(),
        segment: segment.to_string(),
    };

    let mut parts = path.split('.');
    let root = parts.next().unwrap_or("");

    let mut cur: &FieldSchema = if root == "$cards" {
        let kind = parts.next().ok_or_else(|| dangling(root))?;
        let card = config.card_kind(kind).ok_or_else(|| dangling(kind))?;
        let idx = parts.next().ok_or_else(|| dangling(kind))?;
        // All instances of a kind share one schema, so the index is validated
        // but does not descend.
        idx.parse::<usize>().map_err(|_| dangling(idx))?;
        let card_field = parts.next().ok_or_else(|| dangling(idx))?;
        card.fields
            .get(card_field)
            .ok_or_else(|| dangling(card_field))?
    } else {
        config.main.fields.get(root).ok_or_else(|| dangling(root))?
    };

    for seg in parts.by_ref() {
        // The discriminant is the container's own enum, so the step selects a
        // value rather than a child schema: the walk stops on it, and the
        // trailing check below rejects anything that follows.
        if cur.is_variant_bearing() && seg == VARIANT_DISCRIMINANT_KEY {
            break;
        }
        cur = descend(cur, seg).ok_or_else(|| dangling(seg))?;
    }
    match parts.next() {
        Some(seg) => Err(dangling(seg)),
        None => Ok(cur),
    }
}

fn descend<'a>(cur: &'a FieldSchema, seg: &str) -> Option<&'a FieldSchema> {
    match seg.parse::<usize>() {
        Ok(_) => match cur.r#type {
            SchemaType::Array => cur.items.as_deref(),
            _ => None,
        },
        Err(_) => match cur.r#type {
            SchemaType::Object => cur.properties.as_ref()?.get(seg).map(Box::as_ref),
            _ => cur.variant_field(seg),
        },
    }
}

/// Project a resolved [`FieldSchema`] to its widget kind, keyed on capability
/// rather than the `type` token: any field carrying `enum_values` is a dropdown.
pub fn project_kind(
    field: &FieldSchema,
    name: &str,
    path: &str,
) -> Result<WidgetType, BindError> {
    if let Some(values) = &field.enum_values {
        return Ok(WidgetType::Choice {
            options: blank_first(values),
        });
    }
    let unbindable = || BindError::Unbindable {
        name: name.to_string(),
        path: path.to_string(),
        ty: type_desc(field),
    };
    Ok(match &field.r#type {
        SchemaType::Boolean => WidgetType::Checkbox,
        SchemaType::String
        | SchemaType::Number
        | SchemaType::Integer
        | SchemaType::Date
        | SchemaType::DateTime
        | SchemaType::RichText { .. }
        | SchemaType::PlainText { .. } => WidgetType::Text {
            multiline: is_multiline(field),
        },
        // `resolve::coerce_text` joins an array's elements with newlines, so the
        // widget is multiline whatever `ui` says: a single-line one collapses
        // the value the flattened raster stacks.
        SchemaType::Array => match field.items.as_deref() {
            Some(items) if is_scalar_or_prose(items) => WidgetType::Text { multiline: true },
            _ => return Err(unbindable()),
        },
        // An `Enum` reaching here carries no `enum_values`, so it has no options
        // to offer; an `Object` has no widget shape at all.
        SchemaType::Enum | SchemaType::Object => return Err(unbindable()),
    })
}

/// An enum's options with its blank leading them: `resolve::coerce_choice`
/// keeps a value only when it matches a declared option, so without the blank a
/// blank cell binds to nothing.
fn blank_first(values: &[String]) -> Vec<String> {
    std::iter::once(String::new())
        .chain(values.iter().cloned())
        .collect()
}

fn is_multiline(field: &FieldSchema) -> bool {
    field
        .ui
        .as_ref()
        .and_then(|u| u.multiline)
        .unwrap_or(false)
}

fn is_scalar_or_prose(field: &FieldSchema) -> bool {
    !matches!(field.r#type, SchemaType::Array | SchemaType::Object)
}

fn type_desc(field: &FieldSchema) -> String {
    match &field.r#type {
        SchemaType::Array => format!(
            "array<{}>",
            field.items.as_ref().map_or("?", |i| i.r#type.as_str())
        ),
        other => other.as_str().to_string(),
    }
}

/// Map an unbound widget's declared kind to a [`WidgetType`].
fn widget_type(kind: &WidgetKind) -> WidgetType {
    match kind {
        WidgetKind::Text { multiline } => WidgetType::Text {
            multiline: *multiline,
        },
        WidgetKind::Checkbox => WidgetType::Checkbox,
        WidgetKind::Choice { options } => WidgetType::Choice {
            options: options.clone(),
        },
        WidgetKind::Signature => WidgetType::Signature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = r#"
quill:
  name: binder
  version: 0.1.0
  backend: pdfform
  description: bind-test schema
main:
  body:
    enabled: false
  fields:
    full_name:
      type: string
    comments:
      type: array
      items:
        type: string
      ui:
        multiline: true
    tags:
      type: array
      items:
        type: string
    agree:
      type: boolean
    favorite_color:
      type: enum
      values: [red, green, blue]
    count:
      type: integer
    when:
      type: datetime
    bio:
      type: richtext
    address:
      type: object
      properties:
        street: { type: string }
        city: { type: string }
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: string }
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      default: ""
      variants:
        CUI:
          poc: { type: string }
          urgent: { type: boolean }
card_kinds:
  indorsement:
    fields:
      from:
        type: string
      signed:
        type: boolean
"#;

    fn config() -> QuillConfig {
        QuillConfig::from_yaml_with_warnings(YAML).expect("schema parses").0
    }

    fn kind(path: &str) -> Result<WidgetType, BindError> {
        let c = config();
        let schema = bind(&c, "W", path)?;
        project_kind(schema, "W", path)
    }

    #[test]
    fn scalar_and_prose_project_to_text() {
        assert_eq!(kind("full_name").unwrap(), WidgetType::Text { multiline: false });
        assert_eq!(kind("count").unwrap(), WidgetType::Text { multiline: false });
        assert_eq!(kind("when").unwrap(), WidgetType::Text { multiline: false });
        assert_eq!(kind("bio").unwrap(), WidgetType::Text { multiline: false });
    }

    #[test]
    fn boolean_projects_to_checkbox() {
        assert_eq!(kind("agree").unwrap(), WidgetType::Checkbox);
    }

    /// The blank leads so a blank cell has an option to land on:
    /// `resolve::coerce_choice` keeps a value only when it matches a declared
    /// option.
    #[test]
    fn a_schema_bound_enum_leads_its_choices_with_the_blank() {
        assert_eq!(
            kind("favorite_color").unwrap(),
            WidgetType::Choice {
                options: vec!["".into(), "red".into(), "green".into(), "blue".into()]
            }
        );
    }

    /// With or without `ui.multiline`: the joined value carries newlines either
    /// way.
    #[test]
    fn a_scalar_array_projects_to_multiline_text() {
        assert_eq!(kind("comments").unwrap(), WidgetType::Text { multiline: true });
        assert_eq!(kind("tags").unwrap(), WidgetType::Text { multiline: true });
    }

    #[test]
    fn array_element_binds_to_scalar_text() {
        assert_eq!(kind("comments.0").unwrap(), WidgetType::Text { multiline: false });
    }

    #[test]
    fn object_property_binds() {
        assert_eq!(kind("address.street").unwrap(), WidgetType::Text { multiline: false });
    }

    /// A variant container is bound through its resting shape: the discriminant
    /// under `value`, each world's cells beside it. Binding is against the
    /// *schema*, so a cell binds whether or not today's document selects its
    /// world — the form is built once and the document picks a world later.
    #[test]
    fn variant_container_binds_through_its_resting_shape() {
        assert_eq!(
            kind("classification.value").unwrap(),
            WidgetType::Choice {
                options: vec!["".into(), "UNCLASSIFIED".into(), "CUI".into()]
            }
        );
        assert_eq!(
            kind("classification.poc").unwrap(),
            WidgetType::Text { multiline: false }
        );
        assert_eq!(kind("classification.urgent").unwrap(), WidgetType::Checkbox);
    }

    #[test]
    fn object_and_object_array_are_unbindable() {
        // A container is unbindable at every depth: no widget renders one.
        // Its leaves bind, which is what `descend` walks to.
        for (path, ty) in [("address", "object"), ("refs", "array<object>")] {
            match kind(path) {
                Err(e @ BindError::Unbindable { .. }) => {
                    assert_eq!(e.code(), "pdfform::unbindable_field");
                    assert!(e.to_string().contains(ty), "{path}: {e}");
                }
                other => panic!("{path}: expected Unbindable, got {other:?}"),
            }
        }
    }

    #[test]
    fn dangling_root_and_segment_error() {
        for (path, seg) in [
            ("nonesuch", "nonesuch"),
            ("full_name.0", "0"),
            ("address.zip", "zip"),
            ("comments.oops", "oops"),
            ("classification.nosuch", "nosuch"),
            // The discriminant is a value, not a schema to descend further.
            ("classification.value.oops", "oops"),
        ] {
            let c = config();
            match bind(&c, "W", path) {
                Err(e @ BindError::Dangling { .. }) => {
                    assert_eq!(e.code(), "pdfform::dangling_binding");
                    assert!(e.to_string().contains(seg), "{path}: {e}");
                }
                other => panic!("{path}: expected Dangling, got {other:?}"),
            }
        }
    }

    #[test]
    fn card_paths_bind_by_kind_and_index() {
        assert_eq!(
            kind("$cards.indorsement.0.from").unwrap(),
            WidgetType::Text { multiline: false }
        );
        assert_eq!(
            kind("$cards.indorsement.1.signed").unwrap(),
            WidgetType::Checkbox
        );
    }

    #[test]
    fn card_absolute_index_and_bad_kind_are_dangling() {
        let c = config();
        assert!(matches!(
            bind(&c, "W", "$cards.0.from"),
            Err(BindError::Dangling { .. })
        ));
        assert!(matches!(
            bind(&c, "W", "$cards.memo.0.author"),
            Err(BindError::Dangling { .. })
        ));
        assert!(matches!(
            bind(&c, "W", "$cards.indorsement.0"),
            Err(BindError::Dangling { .. })
        ));
        assert!(matches!(
            bind(&c, "W", "$cards.indorsement.x.from"),
            Err(BindError::Dangling { .. })
        ));
    }

    #[test]
    fn tooltip_inherits_description_unless_overridden() {
        let yaml = r#"
quill:
  name: t
  version: 0.1.0
  backend: pdfform
  description: t
main:
  body:
    enabled: false
  fields:
    a:
      type: string
      description: From the schema.
    b:
      type: string
      description: Also schema.
"#;
        let cfg = QuillConfig::from_yaml_with_warnings(yaml).unwrap().0;
        let spec = FormSpec::parse(
            br#"{
              "schema": "quillmark/form@0.2.0",
              "fields": [
                { "name": "A", "schema_field": "a", "page": 0,
                  "rect": { "x": 0, "y": 0, "w": 1, "h": 1 } },
                { "name": "B", "schema_field": "b", "page": 0,
                  "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "tooltip": "Override." }
              ]
            }"#,
        )
        .unwrap();
        let mb = [[0.0, 0.0, 612.0, 792.0]];
        let bound = bind_widgets(&spec, &cfg, &mb).unwrap();
        assert_eq!(bound[0].tooltip.as_deref(), Some("From the schema."));
        assert_eq!(bound[1].tooltip.as_deref(), Some("Override."));
    }

    #[test]
    fn unbound_widgets_pass_their_declared_kind_through() {
        let spec = FormSpec::parse(
            br#"{
              "schema": "quillmark/form@0.2.0",
              "widgets": [
                { "name": "T", "page": 0, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 },
                  "type": "text" },
                { "name": "M", "page": 0, "rect": { "x": 0, "y": 2, "w": 1, "h": 1 },
                  "type": "text", "multiline": true },
                { "name": "C", "page": 0, "rect": { "x": 0, "y": 4, "w": 1, "h": 1 },
                  "type": "checkbox" },
                { "name": "Ch", "page": 0, "rect": { "x": 0, "y": 6, "w": 1, "h": 1 },
                  "type": "choice", "options": ["a", "b"] },
                { "name": "S", "page": 0, "rect": { "x": 0, "y": 8, "w": 1, "h": 1 },
                  "type": "signature" }
              ]
            }"#,
        )
        .unwrap();
        let bound = bind_widgets(&spec, &config(), &[[0.0, 0.0, 612.0, 792.0]]).unwrap();

        let kinds: Vec<WidgetType> = bound.iter().map(|w| w.field_type.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                WidgetType::Text { multiline: false },
                WidgetType::Text { multiline: true },
                WidgetType::Checkbox,
                WidgetType::Choice {
                    options: vec!["a".into(), "b".into()]
                },
                WidgetType::Signature,
            ]
        );
        assert!(bound.iter().all(|w| w.schema_field.is_none()));
        assert_eq!(bound[0].rect, [0.0, 791.0, 1.0, 792.0]);
    }

    #[test]
    fn place_flips_to_bottom_left_and_honours_origin() {
        let r = Rect {
            x: 180.0,
            y: 90.0,
            w: 14.0,
            h: 14.0,
        };
        assert_eq!(
            place("W", 0, r, &[[0.0, 0.0, 600.0, 800.0]]).unwrap(),
            [180.0, 800.0 - 104.0, 194.0, 800.0 - 90.0]
        );
        // Translated canvas box: widgets land offset by the origin.
        assert_eq!(
            place("W", 0, r, &[[10.0, 20.0, 622.0, 812.0]]).unwrap(),
            [10.0 + 180.0, 812.0 - 104.0, 10.0 + 194.0, 812.0 - 90.0]
        );
    }

    #[test]
    fn place_rejects_out_of_range_page() {
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        };
        match place("W", 2, r, &[[0.0, 0.0, 612.0, 792.0]]) {
            Err(e @ BindError::PageOutOfRange { .. }) => {
                assert_eq!(e.code(), "pdfform::field_page_out_of_range");
            }
            other => panic!("expected PageOutOfRange, got {other:?}"),
        }
    }
}
