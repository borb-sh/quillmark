//! Maps the [`FieldSchema`] / [`FieldType`] model to a JSON-Schema-shaped
//! [`QuillValue`]. Backend-agnostic: backends consume it to drive per-field
//! transforms such as markdown to backend markup.

use super::{FieldSchema, FieldType, QuillConfig, VARIANT_DISCRIMINANT_KEY};
use crate::value::QuillValue;

/// The `contentMediaType` marking a richtext field in the transform schema. The
/// value crossing the seam for such a field is canonical Content-JSON (an
/// object), not a string: backends classify on this media type to lower the
/// content rather than a scalar.
pub const CONTENT_MEDIA_TYPE: &str = "application/quillmark-content+json";

/// Transform-schema keyword marking a single-`Para` richtext field (`inline: true`
/// in Quill.yaml). The blueprint spells the same fact `richtext(inline)<markdown>`;
/// this key is the JSON Schema–shaped wire for editor and backend consumers.
pub const QUILLMARK_INLINE_KEY: &str = "quillmark:inline";

/// Transform-schema keyword marking a `plaintext` field: the literal-codec
/// sibling of richtext. It rides the same [`CONTENT_MEDIA_TYPE`], so backends
/// lower it identically; this annotation only tells editors to mount a
/// formatting-free surface and to author/project through the literal codec.
pub const QUILLMARK_PLAIN_KEY: &str = "quillmark:plain";

/// Transform-schema keyword carrying an `enum` blank's label (`ui.blank_title`).
/// Emitted only when the author names one; absent, a consumer supplies its own
/// conventional label.
pub const QUILLMARK_BLANK_TITLE_KEY: &str = "quillmark:blank_title";

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
    // standard JSON-Schema validator rejects a value the engine accepts.
    schema.insert(
        "enum".to_string(),
        serde_json::Value::Array(
            std::iter::once(String::new())
                .chain(field.enum_values.iter().flatten().cloned())
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

/// Build a JSON-Schema-shaped descriptor of a [`QuillConfig`]'s main + card fields.
///
/// The descriptor marks richtext fields with `contentMediaType:
/// application/quillmark-content+json` (see [`CONTENT_MEDIA_TYPE`]) and
/// date/date-time fields with the corresponding JSON Schema `format`.
///
/// `$body` is injected into a kind's `properties` only when that kind's
/// `body.enabled` is not `false`. A body-disabled kind's `$body` is absent,
/// not present-and-empty, so `form-field(field:)` rejects `$body` addresses on
/// that kind at compile time, matching `Quill::validate`'s hard error on
/// authored body content for the same kind.
pub fn build_transform_schema(config: &QuillConfig) -> QuillValue {
    fn field_to_schema(field: &FieldSchema) -> serde_json::Value {
        let mut schema = serde_json::Map::new();
        // A variant-bearing enum crosses as the container it rests as, every
        // world's fields flattened beside the discriminant: at schema time there
        // is no live world, so the union is the only projection available
        // (`SCHEMAS.md` § "Schema emission").
        if let Some(variants) = &field.variants {
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
                    properties.insert(name.clone(), field_to_schema(variant_field));
                }
            }
            schema.insert(
                "type".to_string(),
                serde_json::Value::String("object".to_string()),
            );
            schema.insert("properties".to_string(), properties.into());
            return serde_json::Value::Object(schema);
        }
        if field.enum_values.is_some() {
            return discriminant_schema(field);
        }
        match field.r#type {
            FieldType::String => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
            }
            FieldType::RichText { inline } => {
                // The content crosses the seam as a JSON object (canonical
                // Content-JSON), not a string; `type: object` + the richtext
                // media type is how a backend classifies it to lower the content.
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("object".to_string()),
                );
                schema.insert(
                    "contentMediaType".to_string(),
                    serde_json::Value::String(CONTENT_MEDIA_TYPE.to_string()),
                );
                if inline {
                    schema.insert(
                        QUILLMARK_INLINE_KEY.to_string(),
                        serde_json::Value::Bool(true),
                    );
                }
            }
            FieldType::PlainText { inline } => {
                // Plaintext rides the *same* content and media type as richtext, so
                // a backend classifies and lowers it identically: no backend edit.
                // The distinction (literal codec, no formatting) is carried by the
                // `quillmark:plain` annotation, which only editors consult.
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("object".to_string()),
                );
                schema.insert(
                    "contentMediaType".to_string(),
                    serde_json::Value::String(CONTENT_MEDIA_TYPE.to_string()),
                );
                schema.insert(QUILLMARK_PLAIN_KEY.to_string(), serde_json::Value::Bool(true));
                if inline {
                    schema.insert(
                        QUILLMARK_INLINE_KEY.to_string(),
                        serde_json::Value::Bool(true),
                    );
                }
            }
            // A loaded `enum` always carries `values:`, so the domain branch
            // above claims it; this arm is the domain-less residue.
            FieldType::Enum => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
            }
            FieldType::Number => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("number".to_string()),
                );
            }
            FieldType::Integer => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("integer".to_string()),
                );
            }
            FieldType::Boolean => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("boolean".to_string()),
                );
            }
            FieldType::Array => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("array".to_string()),
                );
                // The element schema is emitted recursively, so a scalar
                // element yields `items: {type: string}` (and a richtext element
                // carries its `contentMediaType`), while an object element yields
                // `items: {type: object, properties: …}`.
                if let Some(items) = &field.items {
                    schema.insert("items".to_string(), field_to_schema(items));
                }
            }
            FieldType::Object => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("object".to_string()),
                );
                if let Some(properties) = &field.properties {
                    let mut props = serde_json::Map::new();
                    for (name, prop) in properties {
                        props.insert(name.clone(), field_to_schema(prop));
                    }
                    schema.insert("properties".to_string(), serde_json::Value::Object(props));
                }
            }
            // Distinct markers for the two date types drive the Typst backend's
            // per-type lowering (3-component vs 6-component `datetime(..)`). This
            // is the internal transform schema; the marker precedent is
            // `quillmark:inline`.
            FieldType::Date => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
                schema.insert(
                    "format".to_string(),
                    serde_json::Value::String("date".to_string()),
                );
            }
            FieldType::DateTime => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
                schema.insert(
                    "format".to_string(),
                    serde_json::Value::String("date-time".to_string()),
                );
            }
        }
        serde_json::Value::Object(schema)
    }

    let mut properties = serde_json::Map::new();
    for (name, field) in &config.main.fields {
        properties.insert(name.clone(), field_to_schema(field));
    }
    if config.main.body_enabled() {
        properties.insert(
            "$body".to_string(),
            serde_json::json!({ "type": "object", "contentMediaType": CONTENT_MEDIA_TYPE }),
        );
    }

    let mut defs = serde_json::Map::new();
    for card in &config.card_kinds {
        let mut card_properties = serde_json::Map::new();
        for (name, field) in &card.fields {
            card_properties.insert(name.clone(), field_to_schema(field));
        }
        if card.body_enabled() {
            card_properties.insert(
                "$body".to_string(),
                serde_json::json!({ "type": "object", "contentMediaType": CONTENT_MEDIA_TYPE }),
            );
        }
        defs.insert(
            format!("{}_card", card.name),
            serde_json::json!({
                "type": "object",
                "properties": card_properties,
            }),
        );
    }

    QuillValue::from_json(serde_json::json!({
        "type": "object",
        "properties": properties,
        "$defs": defs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_from_yaml(yaml: &str) -> QuillValue {
        let config = QuillConfig::from_yaml(yaml).expect("yaml parses");
        build_transform_schema(&config)
    }

    /// The projection is the wire *validity* contract, and obligation is not a
    /// validity fact: an unauthored must-fill cell is wire-valid by design. A
    /// `required`-shaped flag here would read as the gate `SCHEMAS.md` forbids.
    #[test]
    fn obligation_does_not_cross_into_the_transform_schema() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    severity:   { type: enum, values: [low, high] }
    status:     { type: string, default: draft }
"#;
        let json = build_from_yaml(yaml).as_json().clone();

        assert!(!json.to_string().contains("must_fill"), "{json}");
        assert_eq!(json["properties"]["status"], serde_json::json!({"type": "string"}));
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

    #[test]
    fn typed_table_emits_items_with_object_and_properties() {
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
          org: { type: string }
          year: { type: integer }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let refs = &json["properties"]["refs"];
        assert_eq!(refs["type"], "array");
        assert_eq!(refs["items"]["type"], "object");
        assert_eq!(refs["items"]["properties"]["org"]["type"], "string");
        assert_eq!(refs["items"]["properties"]["year"]["type"], "integer");
    }

    #[test]
    fn scalar_array_emits_items_with_element_type() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    counts:
      type: array
      items: { type: integer }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let counts = &json["properties"]["counts"];
        assert_eq!(counts["type"], "array");
        assert_eq!(counts["items"]["type"], "integer");
    }

    #[test]
    fn markdown_array_emits_items_with_content_media_type() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    sections:
      type: array
      items: { type: richtext }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let sections = &json["properties"]["sections"];
        assert_eq!(sections["type"], "array");
        assert_eq!(sections["items"]["type"], "object");
        assert_eq!(sections["items"]["contentMediaType"], CONTENT_MEDIA_TYPE);
    }

    #[test]
    fn typed_dict_emits_object_with_properties() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string }
        city: { type: string }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let address = &json["properties"]["address"];
        assert_eq!(address["type"], "object");
        assert_eq!(address["properties"]["street"]["type"], "string");
        assert_eq!(address["properties"]["city"]["type"], "string");
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
      type: string

card_kinds:
  indorsement:
    fields:
      signature_block:
        type: string
  note:
    fields:
      author:
        type: string
"#;

        let schema = build_from_yaml(yaml);
        let json = schema.as_json();

        let main_body = &json["properties"]["$body"];
        assert_eq!(main_body["type"], "object");
        assert_eq!(main_body["contentMediaType"], CONTENT_MEDIA_TYPE);

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
        }
    }

    #[test]
    fn inline_richtext_emits_quillmark_inline() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    subject:
      type: richtext
      inline: true
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let subject = &json["properties"]["subject"];
        assert_eq!(subject["type"], "object");
        assert_eq!(subject["contentMediaType"], CONTENT_MEDIA_TYPE);
        assert_eq!(subject[QUILLMARK_INLINE_KEY], true);
    }

    #[test]
    fn inline_richtext_array_items_emit_quillmark_inline() {
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
        type: richtext
        inline: true
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let items = &json["properties"]["refs"]["items"];
        assert_eq!(items[QUILLMARK_INLINE_KEY], true);
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
      type: string

card_kinds:
  indorsement:
    body:
      enabled: false
    fields:
      signature_block:
        type: string
  note:
    fields:
      author:
        type: string
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
