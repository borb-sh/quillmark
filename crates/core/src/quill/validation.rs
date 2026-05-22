use indexmap::IndexMap;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::document::Document;
use crate::error::{Diagnostic, Severity};
use crate::quill::formats::DATE_FORMAT;
use crate::quill::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::value::QuillValue;

/// Literal sentinel string the blueprint emitter writes into the value
/// cell of every Must Fill field. Validation detects unreplaced sentinels
/// and reports `validation::unfilled_placeholder`.
pub const MUST_FILL_SENTINEL: &str = "<must-fill>";

/// Validation error with a structured field path.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error(
        "field `{path}` has no value and the schema declares no default; supply a value before shipping"
    )]
    RequiredFieldAbsent { path: String },

    #[error(
        "field `{path}` still carries the `<must-fill>` blueprint sentinel; replace it with a value of the declared type"
    )]
    UnfilledPlaceholder { path: String },

    #[error("field `{path}` has type `{actual}`, expected `{expected}`")]
    TypeMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("field `{path}` value `{value}` not in allowed set {allowed:?}")]
    EnumViolation {
        path: String,
        value: String,
        allowed: Vec<String>,
    },

    #[error("field `{path}` does not match expected format `{format}`")]
    FormatViolation { path: String, format: String },

    #[error("unknown card kind `{card}` at `{path}`")]
    UnknownCard { path: String, card: String },

    #[error(
        "card `{card}` at `{path}` has body content but the card kind declares `body.enabled: false` — remove the body content or set `body.enabled: true` on the card kind"
    )]
    BodyDisabled { path: String, card: String },
}

impl ValidationError {
    /// Document-model path anchor for this error.
    ///
    /// See [`crate::error`] module docs for the path grammar and conventions.
    pub fn path(&self) -> &str {
        match self {
            ValidationError::RequiredFieldAbsent { path }
            | ValidationError::UnfilledPlaceholder { path }
            | ValidationError::TypeMismatch { path, .. }
            | ValidationError::EnumViolation { path, .. }
            | ValidationError::FormatViolation { path, .. }
            | ValidationError::UnknownCard { path, .. }
            | ValidationError::BodyDisabled { path, .. } => path,
        }
    }

    /// Stable diagnostic code for this error variant. Pattern-match on this
    /// instead of the message text.
    pub fn code(&self) -> &'static str {
        match self {
            ValidationError::RequiredFieldAbsent { .. } => "validation::required_field_absent",
            ValidationError::UnfilledPlaceholder { .. } => "validation::unfilled_placeholder",
            ValidationError::TypeMismatch { .. } => "validation::type_mismatch",
            ValidationError::EnumViolation { .. } => "validation::enum_violation",
            ValidationError::FormatViolation { .. } => "validation::format_violation",
            ValidationError::UnknownCard { .. } => "validation::unknown_card",
            ValidationError::BodyDisabled { .. } => "validation::body_disabled",
        }
    }

    /// Convert this error into a structured [`Diagnostic`] carrying the
    /// stable code, the document-model `path`, and an optional hint.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let mut diag = Diagnostic::new(Severity::Error, self.to_string())
            .with_code(self.code().to_string())
            .with_path(self.path().to_string());

        if let Some(hint) = self.hint() {
            diag = diag.with_hint(hint);
        }
        diag
    }

    fn hint(&self) -> Option<String> {
        match self {
            ValidationError::RequiredFieldAbsent { .. } => {
                Some("Add this field to the document.".to_string())
            }
            ValidationError::UnfilledPlaceholder { .. } => Some(format!(
                "Replace the `{}` sentinel with a value of the declared type.",
                MUST_FILL_SENTINEL
            )),
            ValidationError::TypeMismatch { expected, .. } => {
                Some(format!("Provide a value of type `{}`.", expected))
            }
            ValidationError::EnumViolation { allowed, .. } => {
                Some(format!("Use one of: {}.", allowed.join(", ")))
            }
            ValidationError::FormatViolation { format, .. } => {
                Some(format!("Use the `{}` format.", format))
            }
            _ => None,
        }
    }
}

/// Validate a typed [`Document`] (with `IndexMap` payload + typed `Card` list).
///
/// This is the typed entry point used by `QuillConfig::validate_document`.
pub fn validate_typed_document(
    config: &QuillConfig,
    doc: &Document,
) -> Result<(), Vec<ValidationError>> {
    let main_fields = doc.main().payload().to_index_map();
    let mut errors = validate_fields_for_card_indexmap(&config.main, &main_fields, "");

    // Enforce body.enabled on the main card. Whitespace-only bodies are
    // treated as empty — only meaningful prose triggers the diagnostic.
    if !config.main.body_enabled() && !doc.main().body().trim().is_empty() {
        errors.push(ValidationError::BodyDisabled {
            path: "main.body".to_string(),
            card: "main".to_string(),
        });
    }

    for (index, card) in doc.cards().iter().enumerate() {
        let card_name = card.kind().unwrap_or("").to_string();
        let item_path = format!("cards[{index}]");
        // NOTE: `cards[N]` is the document-instance-side path (the cards
        // array on a Document). Card-kind definitions live under
        // `card_kinds:` in Quill.yaml, but instances on a document are
        // still a `cards` list.

        let Some(card_schema) = config.card_kind(card_name.as_str()) else {
            errors.push(ValidationError::UnknownCard {
                path: item_path,
                card: card_name,
            });
            continue;
        };

        let card_path = format!("cards.{card_name}[{index}]");
        let card_fields = card.payload().to_index_map();
        errors.extend(validate_fields_for_card_indexmap(
            card_schema,
            &card_fields,
            &card_path,
        ));

        if !card_schema.body_enabled() && !card.body().trim().is_empty() {
            errors.push(ValidationError::BodyDisabled {
                path: format!("{card_path}.body"),
                card: card_name,
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_fields_for_card_indexmap(
    card: &CardSchema,
    fields: &IndexMap<String, QuillValue>,
    base_path: &str,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut field_names: Vec<&String> = card.fields.keys().collect();
    field_names.sort();

    for field_name in field_names {
        let schema = &card.fields[field_name];
        let path = child_path(base_path, field_name);
        match fields.get(field_name) {
            Some(value) => errors.extend(validate_field(schema, value, &path)),
            None if schema.default.is_none() => {
                errors.push(ValidationError::RequiredFieldAbsent { path })
            }
            None => {}
        }
    }

    errors
}

/// Validate a single value against a field schema at the given path.
/// Used internally; exposed for testing.
pub(crate) fn validate_field(
    field: &FieldSchema,
    value: &QuillValue,
    path: &str,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Sentinel detection runs before per-type coercion / validation.
    // Scalar fields carry the sentinel in the value cell; markdown carries
    // it as the trimmed content of the block scalar. On match, emit
    // `UnfilledPlaceholder` and skip per-type checks for this field.
    if let Some(text) = value.as_str() {
        let candidate = if matches!(field.r#type, FieldType::Markdown) {
            text.trim()
        } else {
            text
        };
        if candidate == MUST_FILL_SENTINEL {
            errors.push(ValidationError::UnfilledPlaceholder {
                path: path.to_string(),
            });
            return errors;
        }
    }

    let type_valid = match field.r#type {
        FieldType::String | FieldType::Markdown => value.as_str().is_some(),
        FieldType::Integer => {
            let json = value.as_json();
            json.is_i64() || json.is_u64()
        }
        FieldType::Number => value.as_json().is_number(),
        FieldType::Boolean => value.as_bool().is_some(),
        FieldType::Date => {
            if value.as_json().is_null() {
                true
            } else {
                match value.as_str() {
                    Some("") => true,
                    Some(text) => {
                        if is_valid_date(text) {
                            true
                        } else {
                            errors.push(ValidationError::FormatViolation {
                                path: path.to_string(),
                                format: "date".to_string(),
                            });
                            false
                        }
                    }
                    None => false,
                }
            }
        }
        FieldType::DateTime => {
            if value.as_json().is_null() {
                true
            } else {
                match value.as_str() {
                    Some("") => true,
                    Some(text) => {
                        if is_valid_datetime(text) {
                            true
                        } else {
                            errors.push(ValidationError::FormatViolation {
                                path: path.to_string(),
                                format: "date-time".to_string(),
                            });
                            false
                        }
                    }
                    None => false,
                }
            }
        }
        FieldType::Array => match value.as_array() {
            Some(items) => {
                if let Some(properties) = &field.properties {
                    for (idx, item) in items.iter().enumerate() {
                        let obj = item.as_object();
                        for (prop_name, prop_schema) in properties {
                            let prop_path = format!("{}[{}].{}", path, idx, prop_name);
                            match obj.and_then(|o| o.get(prop_name)) {
                                Some(v) => errors.extend(validate_field(
                                    prop_schema,
                                    &QuillValue::from_json(v.clone()),
                                    &prop_path,
                                )),
                                None if prop_schema.default.is_none() => errors.push(
                                    ValidationError::RequiredFieldAbsent { path: prop_path },
                                ),
                                None => {}
                            }
                        }
                    }
                }
                true
            }
            None => false,
        },
        FieldType::Object => match value.as_object() {
            Some(object) => {
                if let Some(properties) = &field.properties {
                    let mut property_names: Vec<&String> = properties.keys().collect();
                    property_names.sort();
                    for property_name in property_names {
                        let property_schema = &properties[property_name];
                        let property_path = child_path(path, property_name);
                        match object.get(property_name) {
                            Some(property_value) => errors.extend(validate_field(
                                property_schema,
                                &QuillValue::from_json(property_value.clone()),
                                &property_path,
                            )),
                            None if property_schema.default.is_none() => {
                                errors.push(ValidationError::RequiredFieldAbsent {
                                    path: property_path,
                                })
                            }
                            None => {}
                        }
                    }
                }
                true
            }
            None => false,
        },
    };

    // A Date/DateTime with a string value already emitted a FormatViolation;
    // skip the redundant TypeMismatch in that case.
    let format_error_already_reported =
        matches!(field.r#type, FieldType::Date | FieldType::DateTime) && value.as_str().is_some();

    if !type_valid && !format_error_already_reported {
        errors.push(ValidationError::TypeMismatch {
            path: path.to_string(),
            expected: expected_type_name(&field.r#type).to_string(),
            actual: json_type_name(value.as_json()).to_string(),
        });
    }

    if type_valid {
        if let (Some(allowed), Some(actual)) = (&field.enum_values, value.as_str()) {
            if !allowed.contains(&actual.to_string()) {
                errors.push(ValidationError::EnumViolation {
                    path: path.to_string(),
                    value: actual.to_string(),
                    allowed: allowed.clone(),
                });
            }
        }
    }

    errors
}

fn is_valid_date(value: &str) -> bool {
    Date::parse(value, &DATE_FORMAT).is_ok()
}

fn is_valid_datetime(value: &str) -> bool {
    OffsetDateTime::parse(value, &Rfc3339).is_ok()
}

fn expected_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::String | FieldType::Markdown | FieldType::Date | FieldType::DateTime => "string",
        FieldType::Integer => "integer",
        FieldType::Number => "number",
        FieldType::Boolean => "boolean",
        FieldType::Array => "array",
        FieldType::Object => "object",
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn child_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}.{child}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Card, Document};
    use serde_json::json;

    fn config_with(main_fields: &str, cards: &str) -> QuillConfig {
        let yaml = format!(
            r#"
quill:
  name: native_validation
  backend: typst
  description: Native validator tests
  version: 1.0.0
main:
  fields:
{main_fields}
{cards}
"#
        );
        let (config, warnings) = QuillConfig::from_yaml_with_warnings(&yaml).unwrap();
        assert!(
            warnings.is_empty(),
            "config_with produced warnings (test schema is unsupported): {:?}",
            warnings
        );
        config
    }

    fn doc_from_fm(entries: &[(&str, serde_json::Value)]) -> Document {
        doc_with_typed_cards(entries, vec![])
    }

    fn doc_with_typed_cards(fm: &[(&str, serde_json::Value)], cards: Vec<Card>) -> Document {
        use crate::document::Payload;
        let mut payload = IndexMap::new();
        for (k, v) in fm {
            payload.insert(k.to_string(), QuillValue::from_json(v.clone()));
        }
        let mut p = Payload::from_index_map(payload);
        p.set_quill("test_quill".parse().unwrap());
        p.set_kind("main");
        let main = Card::from_parts(p, String::new());
        Document::from_main_and_cards(main, cards, vec![])
    }

    fn typed_card(tag: &str, fields: &[(&str, serde_json::Value)]) -> Card {
        let mut card = Card::new(tag).unwrap();
        for (k, v) in fields {
            card.set_field(k, QuillValue::from_json(v.clone())).unwrap();
        }
        card
    }

    fn has_error<F>(errors: &[ValidationError], predicate: F) -> bool
    where
        F: Fn(&ValidationError) -> bool,
    {
        errors.iter().any(predicate)
    }

    #[test]
    fn validates_simple_string_field() {
        let config = config_with("    title:\n      type: string", "");
        let doc = doc_from_fm(&[("title", json!("Memo"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_simple_string_type_mismatch() {
        let config = config_with("    title:\n      type: string\n      default: \"\"", "");
        let doc = doc_from_fm(&[("title", json!(9))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual }
            if path == "title" && expected == "string" && actual == "number"
        )));
    }

    #[test]
    fn validates_integer_field_with_integer_value() {
        let config = config_with("    count:\n      type: integer\n      default: 0", "");
        let doc = doc_from_fm(&[("count", json!(9))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_integer_field_with_decimal_value() {
        let config = config_with("    count:\n      type: integer\n      default: 0", "");
        let doc = doc_from_fm(&[("count", json!(9.5))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual }
            if path == "count" && expected == "integer" && actual == "number"
        )));
    }

    #[test]
    fn reports_absent_must_fill_field() {
        // A field with no `default:` is Must Fill. Missing from the document
        // → `required_field_absent`.
        let config = config_with("    memo_for:\n      type: string", "");
        let doc = doc_from_fm(&[]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::RequiredFieldAbsent { path } if path == "memo_for")
        }));
    }

    #[test]
    fn missing_field_with_default_is_ok() {
        // Endorsed field absent from document → no error; default applies.
        let config = config_with(
            "    memo_for:\n      type: string\n      default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn detects_unfilled_placeholder_sentinel() {
        let config = config_with("    memo_for:\n      type: string", "");
        let doc = doc_from_fm(&[("memo_for", json!(MUST_FILL_SENTINEL))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::UnfilledPlaceholder { path } if path == "memo_for")
        }));
    }

    #[test]
    fn detects_unfilled_placeholder_in_markdown_block() {
        // Markdown block scalars carry the sentinel inside the block; the
        // detector trims whitespace before comparing.
        let config = config_with("    body:\n      type: markdown", "");
        let doc = doc_from_fm(&[("body", json!(format!("  {}\n", MUST_FILL_SENTINEL)))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::UnfilledPlaceholder { path } if path == "body")
        }));
    }

    #[test]
    fn quoted_must_fill_string_is_content_not_sentinel() {
        // Once parsed YAML decodes both forms to the same string, exact
        // string equality is the detector. Document this expectation by
        // confirming the literal sentinel always fires the placeholder
        // error (no escape hatch besides quoting at the YAML layer, which
        // produces the same decoded string and therefore still fires).
        // The behavior is documented in BLUEPRINT.md authoring guidance.
        let config = config_with("    name:\n      type: string", "");
        let doc = doc_from_fm(&[("name", json!(MUST_FILL_SENTINEL))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::UnfilledPlaceholder { .. }
        )));
    }

    #[test]
    fn reports_wrong_type_on_must_fill_field() {
        let config = config_with("    memo_for:\n      type: string", "");
        let doc = doc_from_fm(&[("memo_for", json!(true))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, .. } if path == "memo_for"
        )));
    }

    #[test]
    fn validates_enum_value() {
        let config = config_with(
            "    status:\n      type: string\n      default: draft\n      enum:\n        - draft\n        - final",
            "",
        );
        let doc = doc_from_fm(&[("status", json!("draft"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_invalid_enum_value() {
        let config = config_with(
            "    status:\n      type: string\n      default: draft\n      enum:\n        - draft\n        - final",
            "",
        );
        let doc = doc_from_fm(&[("status", json!("invalid"))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::EnumViolation { path, value, .. }
            if path == "status" && value == "invalid"
        )));
    }

    #[test]
    fn validates_date_format() {
        let config = config_with("    signed_on:\n      type: date\n      default: \"\"", "");
        let doc = doc_from_fm(&[("signed_on", json!("2026-04-13"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_invalid_date_format() {
        let config = config_with("    signed_on:\n      type: date\n      default: \"\"", "");
        let doc = doc_from_fm(&[("signed_on", json!("13-04-2026"))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::FormatViolation { path, format } if path == "signed_on" && format == "date")
        }));
    }

    #[test]
    fn validates_datetime_format() {
        let config = config_with(
            "    created_at:\n      type: datetime\n      default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("created_at", json!("2026-04-13T19:24:55Z"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_invalid_datetime_format() {
        let config = config_with(
            "    created_at:\n      type: datetime\n      default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("created_at", json!("2026-04-13 19:24:55"))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::FormatViolation { path, format }
            if path == "created_at" && format == "date-time"
        )));
    }

    #[test]
    fn markdown_accepts_any_string() {
        let config = config_with("    body:\n      type: markdown\n      default: \"\"", "");
        let doc = doc_from_fm(&[("body", json!("# Heading\n\nBody text"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn validates_array_of_objects() {
        let config = config_with(
            "    recipients:\n      type: array\n      default: []\n      properties:\n        name:\n          type: string\n        org:\n          type: string\n          default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([{ "name": "Sam", "org": "HQ" }]))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn reports_must_fill_property_absent_in_array_object() {
        // Property `name` is Must Fill (no default); `org` is Endorsed.
        // Missing `name` in a row → `required_field_absent`.
        let config = config_with(
            "    recipients:\n      type: array\n      default: []\n      properties:\n        name:\n          type: string\n        org:\n          type: string\n          default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([{ "org": "HQ" }]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::RequiredFieldAbsent { path } if path == "recipients[0].name")
        }));
    }

    // NOTE: top-level typed-dictionary fields (`type: object` with `properties`)
    // are supported. Coverage lives in `validates_array_of_objects` (typed
    // tables) and the blueprint tests. Freeform objects without properties are
    // rejected at config parse time.

    #[test]
    fn accumulates_multiple_absent_must_fill_errors() {
        let config = config_with(
            "    memo_for:\n      type: string\n    memo_from:\n      type: string",
            "",
        );
        let doc = doc_from_fm(&[]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let missing_paths: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                ValidationError::RequiredFieldAbsent { path } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert!(missing_paths.contains(&"memo_for"));
        assert!(missing_paths.contains(&"memo_from"));
    }

    #[test]
    fn validates_card_with_valid_discriminator() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![typed_card(
                "indorsement",
                &[("signature_block", json!("Signed"))],
            )],
        );
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_unknown_card_discriminator() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(&[], vec![typed_card("unknown", &[])]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::UnknownCard { path, card } if path == "cards[0]" && card == "unknown")
        }));
    }

    #[test]
    fn validates_multiple_card_instances_same_type() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![
                typed_card("indorsement", &[("signature_block", json!("A"))]),
                typed_card("indorsement", &[("signature_block", json!("B"))]),
            ],
        );
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn validates_multiple_card_kinds_mixed() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string\n  routing:\n    fields:\n      office:\n        type: string",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![
                typed_card("indorsement", &[("signature_block", json!("A"))]),
                typed_card("routing", &[("office", json!("HQ"))]),
            ],
        );
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn reports_card_field_paths_with_card_name_and_index() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(&[], vec![typed_card("indorsement", &[])]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::RequiredFieldAbsent { path } if path == "cards.indorsement[0].signature_block")
        }));
    }

    #[test]
    fn body_disabled_card_enforces_trim_boundary() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  skills:\n    body:\n      enabled: false\n    fields:\n      items:\n        type: array\n        default: []",
        );
        // Prose triggers the error; whitespace-only does not.
        let mut prose_card = typed_card("skills", &[("items", json!(["Rust"]))]);
        prose_card.replace_body("Should not be here.");
        let doc = doc_with_typed_cards(&[], vec![prose_card]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "skills" && path == "cards.skills[0].body"
        )));

        let mut ws_card = typed_card("skills", &[("items", json!(["Rust"]))]);
        ws_card.replace_body("\n   \n");
        let ok_doc = doc_with_typed_cards(&[], vec![ws_card]);
        assert!(validate_typed_document(&config, &ok_doc).is_ok());
    }

    #[test]
    fn to_diagnostic_carries_path_and_code() {
        let err = ValidationError::RequiredFieldAbsent {
            path: "cards.indorsement[0].signature_block".to_string(),
        };
        let diag = err.to_diagnostic();
        assert_eq!(
            diag.code.as_deref(),
            Some("validation::required_field_absent")
        );
        assert_eq!(
            diag.path.as_deref(),
            Some("cards.indorsement[0].signature_block")
        );
        assert_eq!(diag.severity, Severity::Error);
    }

    #[test]
    fn main_body_disabled_with_body_content_is_an_error() {
        let config = QuillConfig::from_yaml(
            r#"
quill:
  name: native_validation
  backend: typst
  description: Native validator tests
  version: 1.0.0
main:
  body:
    enabled: false
  fields:
    title:
      type: string
"#,
        )
        .unwrap();
        use crate::document::Payload;
        let mut p = Payload::from_index_map(IndexMap::new());
        p.set_quill("test_quill".parse().unwrap());
        p.set_kind("main");
        let main = Card::from_parts(p, "Body content that should not be here.".to_string());
        let doc = Document::from_main_and_cards(main, vec![], vec![]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "main" && path == "main.body"
        )));
    }
}
