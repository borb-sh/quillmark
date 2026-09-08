//! The per-field blank: the field's spelling of "explicitly nothing", and the
//! render floor, shared by blueprint emission and the blank-filled render.

use serde_json::json;

use super::{FieldSchema, FieldType, VARIANT_DISCRIMINANT_KEY};
use crate::value::QuillValue;

/// The **blank** for `field`: the leanest value satisfying its declared type,
/// and the value a reader recognizes as "nobody said anything". The per-type
/// table is `SCHEMAS.md` § "Blank-filled render".
///
/// The `enum` blank is `""` unconditionally: the loader rejects a declared `""`
/// (`quill::enum_blank_member`), but a [`QuillConfig`](super::QuillConfig) built
/// through serde bypasses loader validation, so this does not lean on that.
pub fn blank(field: &FieldSchema) -> QuillValue {
    // The blank activates no variant, so the container carries the blank
    // discriminant and nothing else.
    if field.is_variant_bearing() {
        let mut obj = serde_json::Map::new();
        obj.insert(VARIANT_DISCRIMINANT_KEY.to_string(), json!(""));
        return QuillValue::from_json(serde_json::Value::Object(obj));
    }
    // Keyed on the carrier rather than the `Enum` token, as every consumer of a
    // finite domain is, so a serde-built schema whose type and carrier disagree
    // still blanks to the reserved `""`.
    if field.enum_values.is_some() {
        return QuillValue::from_json(json!(""));
    }
    let json = match field.r#type {
        FieldType::Array => json!([]),
        FieldType::Object => match &field.properties {
            Some(properties) => serde_json::Value::Object(
                properties
                    .iter()
                    .map(|(name, schema)| (name.clone(), blank(schema).into_json()))
                    .collect(),
            ),
            // A property-less object is schema-invalid; `{}` is its only
            // type-correct blank.
            None => json!({}),
        },
        FieldType::Integer | FieldType::Number => json!(0),
        FieldType::Boolean => json!(false),
        // The empty content, not `""`: the seam carries canonical Content-JSON.
        // It is single-`Para`, so it satisfies `inline` and is `plain`.
        FieldType::RichText { .. } | FieldType::PlainText { .. } => {
            quillmark_content::serial::to_canonical_value(&quillmark_content::Normalized::empty())
        }
        // String, Date and DateTime: a date's `""` lowers to Typst `none`.
        _ => json!(""),
    };
    QuillValue::from_json(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(yaml: &str) -> FieldSchema {
        let value = QuillValue::from_yaml_str(yaml).unwrap();
        FieldSchema::from_quill_value("field".to_string(), &value).unwrap()
    }

    #[test]
    fn object_with_properties_blanks_each_scalar_leaf() {
        let schema = field(
            r#"
type: object
properties:
  street: { type: string }
  zip: { type: integer }
  active: { type: boolean }
"#,
        );

        assert_eq!(
            blank(&schema).into_json(),
            json!({ "street": "", "zip": 0, "active": false })
        );
    }

    #[test]
    fn nested_object_recurses_to_blank_leaves() {
        let schema = field(
            r#"
type: object
properties:
  name: { type: string }
  address:
    type: object
    properties:
      city: { type: string }
      tags: { type: array, items: { type: string } }
"#,
        );

        assert_eq!(
            blank(&schema).into_json(),
            json!({
                "name": "",
                "address": { "city": "", "tags": [] }
            })
        );
    }

    /// The loader rejects `values:` on a non-enum type; serde builds it anyway.
    #[test]
    fn enum_values_on_a_non_enum_type_blanks_to_the_empty_string() {
        let mut schema = FieldSchema::new("clearance".to_string(), FieldType::Integer, None);
        schema.enum_values = Some(vec!["1".to_string(), "2".to_string()]);
        assert_eq!(blank(&schema).into_json(), json!(""));
    }

    #[test]
    fn property_less_object_degrades_to_empty_object() {
        let schema = field("type: object\n");
        assert_eq!(blank(&schema).into_json(), json!({}));
    }
}
