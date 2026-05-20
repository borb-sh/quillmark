//! Card-yaml block metadata: the `$`-prefixed reserved keys.
//!
//! Every `~~~card-yaml` block's YAML payload may carry up to three reserved
//! keys — `$quill`, `$kind`, `$id`. These are **system metadata** drawn from
//! a closed set; any other `$`-prefixed key is a parse error. The keys are
//! ordinary YAML and are read by the same parser that handles the rest of
//! the payload; this module then **extracts** them from the user field set
//! into a typed [`CardMetadata`].
//!
//! `$quill` on the root block binds the document to a quill; `$kind` is
//! name-validated against `[a-z_][a-z0-9_]*` at parse time. `$id` is opaque
//! metadata: any scalar is accepted and stringified on extraction, then
//! carried through round-trip unchanged.

use std::str::FromStr;

use serde_json::Value as JsonValue;

use crate::error::ParseError;
use crate::version::QuillReference;

/// Typed `$`-metadata of a single card-yaml block.
///
/// The reserved keys are a **closed set** of three optional entries; an
/// unknown `$key` is rejected at parse time. `$quill` is parsed into a typed
/// [`QuillReference`] as the block is read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CardMetadata {
    /// The `$quill` reference. Required on the document's root block and
    /// rejected on composable cards (see `assemble`); `None` on every card
    /// in a successfully parsed [`crate::Document`].
    pub quill: Option<QuillReference>,
    /// The `$kind` card kind, if the block declares one. Validated against
    /// `[a-z_][a-z0-9_]*` at parse time.
    pub kind: Option<String>,
    /// The `$id` opaque identifier, if the block declares one.
    pub id: Option<String>,
}

/// Walk the parsed YAML payload, extracting `$`-prefixed reserved keys into
/// a typed [`CardMetadata`] and removing them from the user field set.
///
/// The accepted keys are the closed set `{$quill, $kind, $id}`. Any other
/// `$`-prefixed key is a parse error. Duplicate keys cannot arise here —
/// the YAML parser rejects them as duplicate mapping keys before this
/// function runs. A `$quill` value that fails to parse as a
/// [`QuillReference`], or a `$kind` value that fails [`is_valid_kind_name`],
/// is also a parse error.
///
/// `$quill` and `$kind` require string scalars (non-string YAML types are
/// rejected). `$id` accepts any scalar and stringifies it, matching the
/// "opaque identifier" semantics of the spec (§3.3).
pub(super) fn extract_meta_from_payload(
    payload: &mut JsonValue,
) -> Result<CardMetadata, ParseError> {
    let mut meta = CardMetadata::default();
    let map = match payload {
        JsonValue::Object(m) => m,
        _ => return Ok(meta),
    };

    let dollar_keys: Vec<String> = map
        .keys()
        .filter(|k| k.starts_with('$'))
        .cloned()
        .collect();

    for key in dollar_keys {
        let value = map
            .shift_remove(&key)
            .expect("key was just enumerated from the same map");
        match key.as_str() {
            "$quill" => {
                let s = require_string("$quill reference", value)?;
                let reference = QuillReference::from_str(&s).map_err(|e| {
                    ParseError::InvalidStructure(format!(
                        "Invalid $quill reference '{}': {}",
                        s, e
                    ))
                })?;
                meta.quill = Some(reference);
            }
            "$kind" => {
                let s = match value {
                    JsonValue::String(s) => s,
                    other => {
                        return Err(ParseError::InvalidStructure(format!(
                            "Invalid `$kind` value — a card kind must be a string \
                             matching `[a-z_][a-z0-9_]*` (got {})",
                            yaml_type_name(&other)
                        )));
                    }
                };
                if !is_valid_kind_name(&s) {
                    return Err(ParseError::InvalidStructure(format!(
                        "Invalid `$kind` value '{}' — a card kind must match \
                         `[a-z_][a-z0-9_]*`",
                        s
                    )));
                }
                meta.kind = Some(s);
            }
            "$id" => {
                meta.id = Some(scalar_to_string(&key, value)?);
            }
            other => {
                return Err(ParseError::InvalidStructure(format!(
                    "Unknown `{}` system-metadata key — the card-yaml block \
                     accepts only `$quill`, `$kind`, and `$id`",
                    other
                )));
            }
        }
    }

    Ok(meta)
}

/// Require a YAML string scalar. The `label` is interpolated into the error
/// message — pass `"$quill reference"` to get `"Invalid $quill reference …"`.
fn require_string(label: &str, value: JsonValue) -> Result<String, ParseError> {
    match value {
        JsonValue::String(s) => Ok(s),
        other => Err(ParseError::InvalidStructure(format!(
            "Invalid {} — expected a string scalar, got {}",
            label,
            yaml_type_name(&other)
        ))),
    }
}

/// Coerce any YAML scalar to its string form (for `$id`, the opaque
/// identifier). Mappings, sequences, and explicit null are rejected.
fn scalar_to_string(key: &str, value: JsonValue) -> Result<String, ParseError> {
    match value {
        JsonValue::String(s) => Ok(s),
        JsonValue::Bool(b) => Ok(b.to_string()),
        JsonValue::Number(n) => Ok(n.to_string()),
        JsonValue::Null => Err(ParseError::InvalidStructure(format!(
            "`{}` cannot be null — provide a scalar value",
            key
        ))),
        other => Err(ParseError::InvalidStructure(format!(
            "`{}` must be a scalar value, got {}",
            key,
            yaml_type_name(&other)
        ))),
    }
}

fn yaml_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "sequence",
        JsonValue::Object(_) => "mapping",
    }
}

/// Validate a card-yaml block's YAML payload (after `$` metadata extraction).
///
/// Rejects the reserved wire-format keys (`QUILL`, `CARD`, `BODY`, `CARDS`)
/// appearing as user-defined fields — they would collide with
/// [`crate::Document::to_plate_json`]'s output. The parsed value is returned
/// unchanged.
pub(super) fn validate_payload_yaml(
    parsed: serde_json::Value,
) -> Result<serde_json::Value, ParseError> {
    if let Some(mapping) = parsed.as_object() {
        for reserved in ["QUILL", "CARD", "BODY", "CARDS"] {
            if mapping.contains_key(reserved) {
                return Err(ParseError::InvalidStructure(format!(
                    "Reserved field name '{}' cannot be used in a card-yaml block",
                    reserved
                )));
            }
        }
    }
    Ok(parsed)
}

/// Validate a card kind name follows the pattern `[a-z_][a-z0-9_]*`.
pub(super) fn is_valid_kind_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();
    let first = chars.next().unwrap();

    if !first.is_ascii_lowercase() && first != '_' {
        return false;
    }

    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_quill_kind_and_leaves_data_intact() {
        let mut payload = json!({
            "$quill": "foo@0.1",
            "$kind": "main",
            "title": "Doc",
        });
        let meta = extract_meta_from_payload(&mut payload).unwrap();
        assert_eq!(meta.quill.unwrap().to_string(), "foo@0.1");
        assert_eq!(meta.kind.as_deref(), Some("main"));
        assert_eq!(payload, json!({"title": "Doc"}));
    }

    #[test]
    fn extracts_id_from_string() {
        let mut payload = json!({"$id": "rev-1"});
        let meta = extract_meta_from_payload(&mut payload).unwrap();
        assert_eq!(meta.id.as_deref(), Some("rev-1"));
    }

    #[test]
    fn extracts_id_from_number() {
        let mut payload = json!({"$id": 42});
        let meta = extract_meta_from_payload(&mut payload).unwrap();
        assert_eq!(meta.id.as_deref(), Some("42"));
    }

    #[test]
    fn extracts_id_from_bool() {
        let mut payload = json!({"$id": true});
        let meta = extract_meta_from_payload(&mut payload).unwrap();
        assert_eq!(meta.id.as_deref(), Some("true"));
    }

    #[test]
    fn rejects_unknown_dollar_key() {
        let mut payload = json!({"$unknown": "x"});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unknown `$unknown`"), "got: {msg}");
    }

    #[test]
    fn rejects_non_string_quill() {
        let mut payload = json!({"$quill": 42});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("$quill reference"), "got: {msg}");
    }

    #[test]
    fn rejects_non_string_kind() {
        let mut payload = json!({"$kind": ["main"]});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid `$kind`"), "got: {msg}");
    }

    #[test]
    fn null_kind_reports_invalid_kind() {
        let mut payload = json!({"$kind": JsonValue::Null});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid `$kind`"), "got: {msg}");
    }

    #[test]
    fn null_quill_reports_invalid_quill_reference() {
        let mut payload = json!({"$quill": JsonValue::Null});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid $quill reference"), "got: {msg}");
    }

    #[test]
    fn rejects_invalid_kind_pattern() {
        let mut payload = json!({"$kind": "Bad-Kind"});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid `$kind`"), "got: {msg}");
    }

    #[test]
    fn rejects_id_mapping() {
        let mut payload = json!({"$id": {"nested": "yes"}});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("`$id`"), "got: {msg}");
        assert!(msg.contains("scalar"), "got: {msg}");
    }

    #[test]
    fn non_object_payload_returns_empty_meta() {
        let mut payload = JsonValue::Null;
        let meta = extract_meta_from_payload(&mut payload).unwrap();
        assert_eq!(meta, CardMetadata::default());
    }
}
