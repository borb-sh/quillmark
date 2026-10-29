//! Maps the [`FieldSchema`] / [`FieldType`] model to a JSON-Schema-shaped
//! [`QuillValue`]. Backend-agnostic: backends consume it to drive per-field
//! transforms such as markdown to backend markup.

use indexmap::IndexMap;

use super::{CardSchema, FieldSchema, FieldType, QuillConfig, VARIANT_DISCRIMINANT_KEY};
use crate::value::QuillValue;

/// The `contentMediaType` marking a richtext field in the transform schema. The
/// value crossing the seam for such a field is canonical Content-JSON (an
/// object), not a string: backends classify on this media type to lower the
/// content rather than a scalar.
pub const CONTENT_MEDIA_TYPE: &str = "application/quillmark-content+json";

/// Transform-schema keyword marking a `text` or `richtext` field that admits
/// more than one line (`multiline: true` in Quill.yaml); absent, the field is
/// one line (`text`) or one paragraph (`richtext`).
pub const QUILLMARK_MULTILINE_KEY: &str = "quillmark:multiline";

/// Transform-schema keywords carrying a field's `max:` in the unit its type
/// counts: elements on an array, characters on a text field. A cap warns and
/// never refuses, so it rides a `quillmark:` annotation rather than JSON
/// Schema's asserting `maxItems` / `maxLength`, which a standard validator
/// would enforce against a document Quillmark renders.
pub const QUILLMARK_MAX_ITEMS_KEY: &str = "quillmark:maxItems";
pub const QUILLMARK_MAX_LENGTH_KEY: &str = "quillmark:maxLength";

/// Transform-schema keyword marking a `text` field: the literal-codec
/// sibling of richtext. It rides the same [`CONTENT_MEDIA_TYPE`], so backends
/// lower it identically; this annotation only tells editors to mount a
/// formatting-free surface and to author/project through the literal codec.
pub const QUILLMARK_PLAIN_KEY: &str = "quillmark:plain";

/// Transform-schema keyword carrying an `enum` blank's label (`ui.blank_title`).
/// Emitted only when the author names one; absent, a consumer supplies its own
/// conventional label.
pub const QUILLMARK_BLANK_TITLE_KEY: &str = "quillmark:blank_title";

/// Transform-schema keyword carrying a `matrix`'s roster, its `[id, title]`
/// pairs in declaration order: the vocabulary a plate prints, held or not.
///
/// It also fixes the key order a held value lowers in. The backend's dictionary
/// keys sort, so a reorder-only update rebuilds byte-identical source; this
/// order is a property of the schema rather than of the data, so honoring it
/// costs that nothing. The keys it does not name, an open matrix's added items,
/// trail it sorted.
pub const QUILLMARK_ROSTER_KEY: &str = "quillmark:roster";

/// The `{type: string, enum: ["", …]}` an enum projects to: a plain enum's own
/// cell, and the discriminant cell of a variant-bearing one. That container is a
/// mapping and therefore not a cell, so this is where its choice lands.
fn discriminant_schema(field: &FieldSchema) -> serde_json::Value {
    let mut schema = serde_json::Map::new();
    schema.insert(
        "type".to_string(),
        serde_json::Value::String("string".to_string()),
    );
    // The model layer keeps `""` out of `values:` (it is not a choice), but this
    // projection describes what is *wire-valid*, and the blank is: without it a
    // standard JSON-Schema validator rejects a value Quillmark accepts.
    schema.insert(
        "enum".to_string(),
        serde_json::Value::Array(
            std::iter::once(String::new())
                .chain(field.domain().iter().cloned())
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    if let Some(blank_title) = field.ui.as_ref().and_then(|u| u.blank_title.as_ref()) {
        schema.insert(
            QUILLMARK_BLANK_TITLE_KEY.to_string(),
            serde_json::Value::String(blank_title.clone()),
        );
    }
    serde_json::Value::Object(schema)
}

/// The form a schema's content leaves take: the one switch between the two
/// documents [`build_transform_schema`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaCodec {
    /// Canonical Content-JSON objects marked [`CONTENT_MEDIA_TYPE`]: the plate
    /// seam backends lower, with `$body` among a card's properties and each
    /// kind under `$defs` as `<kind>_card`.
    Content,
    /// Each leaf as its codec's text, `richtext` Markdown marked
    /// [`MARKDOWN_MEDIA_TYPE`] and `text` its literal string: the authoring
    /// document (`prose/canon/AUTHORING.md`). Every cell admits
    /// `null` (null ≡ absent), every declared object refuses an undeclared
    /// key, a variant-bearing enum is a `oneOf` over its
    /// worlds, and a slot carries its `title`, `description`, `default` and
    /// `example` as JSON Schema annotations.
    Text,
}

/// The `contentMediaType` marking a `richtext` leaf or a body in the authoring
/// schema: the model writes Markdown there.
pub const MARKDOWN_MEDIA_TYPE: &str = "text/markdown";

/// The authoring document's card discriminator. The wire spells it `$kind`;
/// here it sits beside `fields`, so no field name collides with it.
pub const AUTHORING_KIND_KEY: &str = "kind";

/// Build a JSON-Schema-shaped descriptor of a [`QuillConfig`]'s main and card
/// fields, its content leaves in the form `codec` names.
///
/// Under [`SchemaCodec::Content`] the descriptor marks richtext fields with
/// `contentMediaType: application/quillmark-content+json` (see
/// [`CONTENT_MEDIA_TYPE`]) and date/date-time fields with the corresponding
/// JSON Schema `format`. A `max:` crosses as [`QUILLMARK_MAX_ITEMS_KEY`] or
/// [`QUILLMARK_MAX_LENGTH_KEY`]. `$body` is injected into a kind's
/// `properties` only when that kind's `body.enabled` is not `false`. A
/// body-disabled kind's `$body` is absent, not present-and-empty, so
/// `form-field(field:)` rejects `$body` addresses on that kind at compile
/// time, as authored body content on the same kind drops under
/// `Quill::validate`'s `validation::body_disabled` warning.
///
/// Under [`SchemaCodec::Text`] it describes the authoring document instead:
/// `{fields, body, cards: [{kind, fields, body}]}`, each card kind a `$defs`
/// entry the `cards` items select by `kind`.
pub fn build_transform_schema(config: &QuillConfig, codec: SchemaCodec) -> QuillValue {
    QuillValue::from_json(match codec {
        SchemaCodec::Content => content_envelope(config),
        SchemaCodec::Text => authoring_envelope(config),
    })
}

fn content_envelope(config: &QuillConfig) -> serde_json::Value {
    let body = || {
        serde_json::json!({
            "type": "object",
            "contentMediaType": CONTENT_MEDIA_TYPE,
            QUILLMARK_MULTILINE_KEY: true,
        })
    };
    let properties = |card: &CardSchema| {
        let mut properties: serde_json::Map<String, serde_json::Value> = card
            .fields
            .iter()
            .map(|(name, field)| (name.clone(), field_to_schema(field, SchemaCodec::Content)))
            .collect();
        if card.body_enabled() {
            properties.insert("$body".to_string(), body());
        }
        properties
    };
    let defs: serde_json::Map<String, serde_json::Value> = config
        .card_kinds
        .iter()
        .map(|card| {
            (
                format!("{}_card", card.name),
                serde_json::json!({ "type": "object", "properties": properties(card) }),
            )
        })
        .collect();
    serde_json::json!({
        "type": "object",
        "properties": properties(&config.main),
        "$defs": defs,
    })
}

fn authoring_envelope(config: &QuillConfig) -> serde_json::Value {
    let mut root = serde_json::Map::new();
    root.insert(
        "$schema".to_string(),
        "https://json-schema.org/draft/2020-12/schema".into(),
    );
    label(&mut root, config.main.title.as_deref(), config.main.description.as_deref());
    root.insert("type".to_string(), "object".into());
    let mut properties = authoring_card_properties(&config.main);
    let kinds: Vec<serde_json::Value> = config
        .card_kinds
        .iter()
        .map(|card| serde_json::json!({ "$ref": format!("#/$defs/{}", card.name) }))
        .collect();
    let cards = if kinds.is_empty() {
        serde_json::json!({ "type": ["array", "null"], "maxItems": 0 })
    } else {
        serde_json::json!({ "type": ["array", "null"], "items": { "oneOf": kinds } })
    };
    properties.insert("cards".to_string(), cards);
    root.insert("properties".to_string(), properties.into());
    root.insert("additionalProperties".to_string(), false.into());
    if !config.card_kinds.is_empty() {
        let defs: serde_json::Map<String, serde_json::Value> = config
            .card_kinds
            .iter()
            .map(|card| (card.name.clone(), authoring_kind(card)))
            .collect();
        root.insert("$defs".to_string(), defs.into());
    }
    serde_json::Value::Object(root)
}

/// One card kind's envelope: `kind` fixed to its name, then the `fields` and
/// `body` a main card carries too.
fn authoring_kind(card: &CardSchema) -> serde_json::Value {
    let mut schema = serde_json::Map::new();
    label(&mut schema, card.title.as_deref(), card.description.as_deref());
    schema.insert("type".to_string(), "object".into());
    let mut properties = serde_json::Map::new();
    properties.insert(
        AUTHORING_KIND_KEY.to_string(),
        serde_json::json!({ "const": card.name }),
    );
    properties.extend(authoring_card_properties(card));
    schema.insert("properties".to_string(), properties.into());
    schema.insert("required".to_string(), serde_json::json!([AUTHORING_KIND_KEY]));
    schema.insert("additionalProperties".to_string(), false.into());
    serde_json::Value::Object(schema)
}

/// A card's `fields` and, where the kind enables one, its `body`.
fn authoring_card_properties(card: &CardSchema) -> serde_json::Map<String, serde_json::Value> {
    let fields: serde_json::Map<String, serde_json::Value> = card
        .fields
        .iter()
        .map(|(name, field)| (name.clone(), field_to_schema(field, SchemaCodec::Text)))
        .collect();
    let mut properties = serde_json::Map::new();
    properties.insert(
        "fields".to_string(),
        serde_json::json!({
            "type": ["object", "null"],
            "properties": fields,
            "additionalProperties": false,
        }),
    );
    if card.body_enabled() {
        let mut body = serde_json::Map::new();
        let declared = card.body.as_ref();
        label(&mut body, None, declared.and_then(|b| b.description.as_deref()));
        body.insert("type".to_string(), serde_json::json!(["string", "null"]));
        body.insert("contentMediaType".to_string(), MARKDOWN_MEDIA_TYPE.into());
        body.insert(QUILLMARK_MULTILINE_KEY.to_string(), true.into());
        if let Some(example) = declared.and_then(|b| b.example.as_deref()) {
            body.insert("examples".to_string(), serde_json::json!([example]));
        }
        properties.insert("body".to_string(), body.into());
    } else {
        properties.insert(
            "body".to_string(),
            serde_json::json!({ "type": ["string", "null"], "maxLength": 0 }),
        );
    }
    properties
}

fn label(schema: &mut serde_json::Map<String, serde_json::Value>, title: Option<&str>, description: Option<&str>) {
    if let Some(title) = title {
        schema.insert("title".to_string(), title.into());
    }
    if let Some(description) = description {
        schema.insert("description".to_string(), description.into());
    }
}

/// An optional cell's wire admits `null` beside its type, as the JSON-Schema
/// `type: [<type>, "null"]` union, and an enum's list gains `null` beside its
/// blank. The authoring document admits it on every cell.
fn field_to_schema(field: &FieldSchema, codec: SchemaCodec) -> serde_json::Value {
    let mut schema = declared_schema(field, codec);
    if field.optional || codec == SchemaCodec::Text {
        admit_null(&mut schema);
    }
    if codec == SchemaCodec::Content {
        return schema;
    }
    let mut annotated = serde_json::Map::new();
    label(&mut annotated, field.title.as_deref(), field.description.as_deref());
    if let serde_json::Value::Object(schema) = schema {
        annotated.extend(schema);
    }
    let literal = |value: &QuillValue| {
        let value = value.as_json();
        if value.is_null() {
            None
        } else if field.is_variant_bearing() {
            Some(serde_json::json!({ VARIANT_DISCRIMINANT_KEY: value }))
        } else {
            Some(value.clone())
        }
    };
    if let Some(default) = field.default.as_ref().and_then(literal) {
        annotated.insert("default".to_string(), default);
    }
    if let Some(example) = field.example.as_ref().and_then(literal) {
        annotated.insert("examples".to_string(), serde_json::json!([example]));
    }
    serde_json::Value::Object(annotated)
}

fn admit_null(schema: &mut serde_json::Value) {
    if let Some(serde_json::Value::Array(branches)) = schema.get_mut("oneOf") {
        branches.push(serde_json::json!({ "type": "null" }));
        return;
    }
    admit_type(schema, "null");
    if let Some(serde_json::Value::Array(members)) = schema.get_mut("enum") {
        members.insert(0, serde_json::Value::Null);
    }
}

fn admit_type(schema: &mut serde_json::Value, extra: &str) {
    match schema.get_mut("type") {
        Some(serde_json::Value::Array(types)) => types.push(extra.into()),
        Some(ty) => *ty = serde_json::json!([ty.clone(), extra]),
        None => {}
    }
}

/// A variant-bearing enum as the authoring document spells it: one branch per
/// world, its `value` a `const`, one branch for the members that open no world
/// and the blank, and the bare discriminant, the shorthand coercion accepts.
fn variant_union(field: &FieldSchema, variants: &IndexMap<String, super::VariantFields>) -> serde_json::Value {
    let world = |value: serde_json::Value, cells: serde_json::Map<String, serde_json::Value>| {
        let mut properties = serde_json::Map::new();
        properties.insert(VARIANT_DISCRIMINANT_KEY.to_string(), value);
        properties.extend(cells);
        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": [VARIANT_DISCRIMINANT_KEY],
            "additionalProperties": false,
        })
    };
    let mut branches: Vec<serde_json::Value> = variants
        .iter()
        .map(|(member, fields)| {
            let cells = fields
                .iter()
                .map(|(name, cell)| (name.clone(), field_to_schema(cell, SchemaCodec::Text)))
                .collect();
            world(serde_json::json!({ "const": member }), cells)
        })
        .collect();
    let worldless: Vec<&str> = std::iter::once("")
        .chain(field.domain().iter().map(String::as_str))
        .filter(|member| !variants.contains_key(*member))
        .collect();
    branches.push(world(
        serde_json::json!({ "type": "string", "enum": worldless }),
        serde_json::Map::new(),
    ));
    branches.push(discriminant_schema(field));
    serde_json::json!({ "oneOf": branches })
}

/// Under [`SchemaCodec::Text`], refuse a key the quill does not declare: the
/// schema binds a model to the declared shape, though an import still stores
/// an undeclared key and `validate` warns on it.
fn close(schema: &mut serde_json::Map<String, serde_json::Value>, codec: SchemaCodec) {
    if codec == SchemaCodec::Text {
        schema.insert("additionalProperties".to_string(), false.into());
    }
}

fn declared_schema(field: &FieldSchema, codec: SchemaCodec) -> serde_json::Value {
    let mut schema = serde_json::Map::new();
    if let Some(variants) = &field.variants {
        if codec == SchemaCodec::Text {
            return variant_union(field, variants);
        }
        // A variant-bearing enum crosses the plate seam as the container it
        // rests as, every world's fields flattened beside the discriminant: at
        // schema time there is no live world, so the union is the only
        // projection available (`SCHEMAS.md` § "Schema emission").
        let mut properties = serde_json::Map::new();
        properties.insert(
            VARIANT_DISCRIMINANT_KEY.to_string(),
            discriminant_schema(field),
        );
        for fields in variants.values() {
            for (name, variant_field) in fields {
                // Every repetition is the same declaration
                // (`quill::variant_field_collision`).
                if properties.contains_key(name) {
                    continue;
                }
                properties.insert(name.clone(), field_to_schema(variant_field, codec));
            }
        }
        schema.insert(
            "type".to_string(),
            serde_json::Value::String("object".to_string()),
        );
        schema.insert("properties".to_string(), properties.into());
        return serde_json::Value::Object(schema);
    }
    match field.r#type {
        FieldType::Enum { .. } => return discriminant_schema(field),
        FieldType::RichText { multiline } | FieldType::Text { multiline } => {
            let plain = matches!(field.r#type, FieldType::Text { .. });
            match codec {
                // The content crosses the seam as a JSON object (canonical
                // Content-JSON), not a string; `type: object` + the content
                // media type is how a backend classifies it to lower the
                // content. Text rides the same type, so a backend lowers it
                // identically; `quillmark:plain` tells editors to mount a
                // formatting-free surface.
                SchemaCodec::Content => {
                    schema.insert("type".to_string(), "object".into());
                    schema.insert("contentMediaType".to_string(), CONTENT_MEDIA_TYPE.into());
                    if plain {
                        schema.insert(QUILLMARK_PLAIN_KEY.to_string(), true.into());
                    }
                }
                SchemaCodec::Text => {
                    schema.insert("type".to_string(), "string".into());
                    if !plain {
                        schema.insert("contentMediaType".to_string(), MARKDOWN_MEDIA_TYPE.into());
                    }
                }
            }
            if multiline {
                schema.insert(QUILLMARK_MULTILINE_KEY.to_string(), true.into());
            }
            if let Some(max) = field.max {
                schema.insert(QUILLMARK_MAX_LENGTH_KEY.to_string(), max.into());
            }
        }
        FieldType::Number => {
            schema.insert("type".to_string(), "number".into());
        }
        FieldType::Integer => {
            schema.insert("type".to_string(), "integer".into());
        }
        FieldType::Boolean => {
            schema.insert("type".to_string(), "boolean".into());
        }
        FieldType::Array => {
            schema.insert("type".to_string(), "array".into());
            // The element schema is emitted recursively, so a scalar
            // element yields `items: { type: text }` (and a richtext element
            // carries its `contentMediaType`), while an object element yields
            // `items: {type: object, properties: …}`.
            if let Some(items) = &field.items {
                schema.insert("items".to_string(), field_to_schema(items, codec));
            }
            if let Some(max) = field.max {
                schema.insert(QUILLMARK_MAX_ITEMS_KEY.to_string(), max.into());
            }
        }
        FieldType::Object => {
            schema.insert("type".to_string(), "object".into());
            if let Some(properties) = &field.properties {
                let props: serde_json::Map<String, serde_json::Value> = properties
                    .iter()
                    .map(|(name, prop)| (name.clone(), field_to_schema(prop, codec)))
                    .collect();
                schema.insert("properties".to_string(), serde_json::Value::Object(props));
            }
            close(&mut schema, codec);
        }
        // Each roster id's node is the member schema, so
        // `qualifications.flight_cc.detail` resolves as a typed dictionary's
        // leaf does; an open matrix's added items are `additionalProperties`,
        // where `title` is a cell. None is required: a value holds only the
        // members it ticks.
        FieldType::Matrix { ref roster, .. } => {
            schema.insert("type".to_string(), "object".into());
            schema.insert(
                QUILLMARK_ROSTER_KEY.to_string(),
                serde_json::Value::Array(
                    roster
                        .iter()
                        .map(|(id, title)| serde_json::json!([id, title]))
                        .collect(),
                ),
            );
            if let Some(member) = &field.member {
                let mut node = field_to_schema(member, codec);
                // The values form holds a ticked member with no columns as
                // `true`, and an unticked one as `false`.
                if codec == SchemaCodec::Text {
                    admit_type(&mut node, "boolean");
                }
                let props: serde_json::Map<String, serde_json::Value> =
                    roster.keys().map(|id| (id.clone(), node.clone())).collect();
                schema.insert("properties".to_string(), serde_json::Value::Object(props));
            }
            match &field.added_item {
                Some(item) => {
                    schema.insert("additionalProperties".to_string(), field_to_schema(item, codec));
                }
                None => close(&mut schema, codec),
            }
        }
        // Distinct markers for the two date types drive the Typst backend's
        // per-type lowering (3-component vs 6-component `datetime(..)`).
        FieldType::Date => {
            schema.insert("type".to_string(), "string".into());
            schema.insert("format".to_string(), "date".into());
        }
        FieldType::DateTime => {
            schema.insert("type".to_string(), "string".into());
            schema.insert("format".to_string(), "date-time".into());
        }
    }
    serde_json::Value::Object(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_from_yaml(yaml: &str) -> QuillValue {
        let config = QuillConfig::from_yaml(yaml).expect("yaml parses");
        build_transform_schema(&config, SchemaCodec::Content)
    }

    #[test]
    fn enum_carries_its_domain_at_every_depth() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
    endorsements:
      type: array
      items:
        type: object
        properties:
          action:
            type: enum
            values: [approve, disapprove]
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        // The blank leads the domain: the projection describes what is
        // wire-valid, and every enum accepts its blank.
        let domain = serde_json::json!({
            "type": "string",
            "enum": ["", "UNCLASSIFIED", "CUI"],
        });
        assert_eq!(json["properties"]["classification"], domain);
        // The domain survives the recursion into an array's element object: a
        // consumer building a validator sees it, blank and all, at the leaf too.
        assert_eq!(
            json["properties"]["endorsements"]["items"]["properties"]["action"],
            serde_json::json!({
                "type": "string",
                "enum": ["", "approve", "disapprove"],
            })
        );
    }

    const BLANK_TITLE_YAML: &str = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      default: ""
      ui:
        blank_title: "(no marking)"
    action:
      type: enum
      values: [approve, disapprove]
"#;

    /// One `max:` in Quill.yaml, two keywords on the wire: the unit a reader
    /// of the declaration takes from the sibling `type:` is in the keyword's
    /// name here, and no asserting JSON Schema keyword refuses what Quillmark
    /// renders.
    #[test]
    fn max_crosses_as_the_keyword_that_names_its_unit() {
        let json = build_from_yaml(
            r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    citation: { type: richtext, multiline: true, max: 1350 }
    ssn: { type: text, max: 9 }
    statements:
      type: array
      max: 4
      items: { type: text, max: 350 }
    notes:
      type: array
      items: { type: text }
"#,
        )
        .as_json()
        .clone();
        let fields = &json["properties"];
        assert_eq!(fields["citation"]["quillmark:maxLength"], 1350);
        assert_eq!(fields["ssn"]["quillmark:maxLength"], 9);
        assert_eq!(fields["statements"]["quillmark:maxItems"], 4);
        assert_eq!(fields["statements"]["items"]["quillmark:maxLength"], 350);
        for (field, keyword) in [("citation", "quillmark:maxItems"), ("statements", "quillmark:maxLength")] {
            assert!(fields[field].get(keyword).is_none(), "{field} carries no {keyword}");
        }
        assert!(fields["notes"].get("quillmark:maxItems").is_none(), "an uncapped array");
        assert!(fields["notes"]["items"].get("quillmark:maxLength").is_none(), "uncapped text");
        for keyword in ["maxItems", "maxLength"] {
            assert!(fields["statements"].get(keyword).is_none(), "a cap never asserts");
        }
    }

    #[test]
    fn blank_title_labels_the_blank_only_where_the_author_names_one() {
        let json = build_from_yaml(BLANK_TITLE_YAML).as_json().clone();

        assert_eq!(
            json["properties"]["classification"],
            serde_json::json!({
                "type": "string",
                "enum": ["", "UNCLASSIFIED", "CUI"],
                "quillmark:blank_title": "(no marking)",
            })
        );
        assert_eq!(
            json["properties"]["action"],
            serde_json::json!({
                "type": "string",
                "enum": ["", "approve", "disapprove"],
            })
        );
    }

    /// The declaration view emits what the author wrote, so a quill round-tripping
    /// through it still loads: injecting the blank there would emit `values: ["", …]`,
    /// which `quill::enum_blank_member` rejects.
    #[test]
    fn blank_title_does_not_cross_into_the_declaration_view() {
        let config = QuillConfig::from_yaml(BLANK_TITLE_YAML).expect("yaml parses");
        let emitted = config.schema();

        assert_eq!(
            emitted["main"]["fields"]["classification"]["values"],
            serde_json::json!(["UNCLASSIFIED", "CUI"])
        );
        assert!(!emitted.to_string().contains("quillmark:"), "{emitted}");
    }

    #[test]
    fn each_field_shape_projects_its_schema() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: text }
          year: { type: integer }
    counts:
      type: array
      items: { type: integer }
    sections:
      type: array
      items: { type: richtext, multiline: true }
    address:
      type: object
      properties:
        street: { type: text }
        city: { type: text }
    subject:
      type: richtext
      multiline: false
    lines:
      type: array
      items:
        type: richtext
        multiline: false
    label:
      type: text
"#;
        let content = serde_json::json!({ "type": "object", "contentMediaType": CONTENT_MEDIA_TYPE });
        let multiline = serde_json::json!({
            "type": "object",
            "contentMediaType": CONTENT_MEDIA_TYPE,
            QUILLMARK_MULTILINE_KEY: true,
        });
        let text = serde_json::json!({
            "type": "object",
            "contentMediaType": CONTENT_MEDIA_TYPE,
            QUILLMARK_PLAIN_KEY: true,
        });
        let cases = [
            (
                "refs",
                serde_json::json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "org": text,
                            "year": { "type": "integer" },
                        },
                    },
                }),
            ),
            (
                "counts",
                serde_json::json!({ "type": "array", "items": { "type": "integer" } }),
            ),
            (
                "sections",
                serde_json::json!({ "type": "array", "items": multiline }),
            ),
            (
                "address",
                serde_json::json!({
                    "type": "object",
                    "properties": { "street": text, "city": text },
                }),
            ),
            ("subject", content.clone()),
            (
                "lines",
                serde_json::json!({ "type": "array", "items": content }),
            ),
            ("label", text),
        ];
        let schema = build_from_yaml(yaml);
        for (field, expected) in cases {
            assert_eq!(schema.as_json()["properties"][field], expected, "{field}");
        }
    }

    #[test]
    fn injects_body_as_markdown_for_main_and_each_card_kind() {
        let yaml = r#"
quill:
  name: example
  version: 0.1.0
  backend: typst
  description: example

main:
  fields:
    title:
      type: text

card_kinds:
  indorsement:
    fields:
      signature_block:
        type: text
  note:
    fields:
      author:
        type: text
"#;

        let schema = build_from_yaml(yaml);
        let json = schema.as_json();

        let main_body = &json["properties"]["$body"];
        assert_eq!(main_body["type"], "object");
        assert_eq!(main_body["contentMediaType"], CONTENT_MEDIA_TYPE);
        assert_eq!(main_body[QUILLMARK_MULTILINE_KEY], true);

        for def_name in ["indorsement_card", "note_card"] {
            let card_body = &json["$defs"][def_name]["properties"]["$body"];
            assert_eq!(
                card_body["type"], "object",
                "{def_name} $body type should be object"
            );
            assert_eq!(
                card_body["contentMediaType"], CONTENT_MEDIA_TYPE,
                "{def_name} $body should be richtext"
            );
            assert_eq!(
                card_body[QUILLMARK_MULTILINE_KEY], true,
                "{def_name} $body should lower as a block"
            );
        }
    }

    #[test]
    fn body_disabled_kind_omits_body_from_schema() {
        let yaml = r#"
quill:
  name: example
  version: 0.1.0
  backend: typst
  description: example

main:
  body:
    enabled: false
  fields:
    title:
      type: text

card_kinds:
  indorsement:
    body:
      enabled: false
    fields:
      signature_block:
        type: text
  note:
    fields:
      author:
        type: text
"#;

        let schema = build_from_yaml(yaml);
        let json = schema.as_json();

        assert!(
            json["properties"].get("$body").is_none(),
            "body-disabled main should not carry $body"
        );
        assert!(
            json["$defs"]["indorsement_card"]["properties"]
                .get("$body")
                .is_none(),
            "body-disabled card kind should not carry $body"
        );
        assert!(
            json["$defs"]["note_card"]["properties"]
                .get("$body")
                .is_some(),
            "body-enabled card kind should still carry $body"
        );
    }
}
