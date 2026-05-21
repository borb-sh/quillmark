//! Validation helpers for card-yaml `$`-prefixed system metadata.
//!
//! The closed set of `$` keys (`$quill`, `$kind`, `$id`) and their typed
//! values are now stored as variants of [`super::PayloadItem`] inside a
//! card's unified [`super::Payload`] item list — they sit alongside user
//! fields and comments in source order, which is what makes inline-comment
//! preservation symmetric across the `$`/non-`$` boundary.
//!
//! This module retains only the validation primitives shared between the
//! parser, the editor surface, and the storage DTO:
//!
//! - [`extract_meta_from_payload`] — strip `$` keys from a parsed YAML
//!   mapping and validate each into a typed [`MetaValue`].
//! - [`is_valid_kind_name`] / [`validate_composable_kind`] — name checks
//!   for `$kind`.

use std::str::FromStr;

use serde_json::Value as JsonValue;

use crate::error::ParseError;
use crate::version::QuillReference;

/// A typed `$`-prefixed metadata value, decoupled from its position in the
/// containing card's item list.
///
/// `extract_meta_from_payload` returns these in source order; the assembler
/// then interleaves them with comments and user fields when building the
/// final [`super::Payload`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MetaValue {
    Quill(QuillReference),
    Kind(String),
    Id(String),
}

impl MetaValue {
    /// The `$key` string this value corresponds to.
    pub(super) fn key(&self) -> &'static str {
        match self {
            MetaValue::Quill(_) => "$quill",
            MetaValue::Kind(_) => "$kind",
            MetaValue::Id(_) => "$id",
        }
    }
}

/// Walk the parsed YAML payload, extracting `$`-prefixed reserved keys into
/// typed [`MetaValue`]s in source order. The keys are removed from `payload`
/// so the caller can build the user-field portion from what remains.
///
/// The accepted keys are the closed set `{$quill, $kind, $id}`. Any other
/// `$`-prefixed key is a parse error. Duplicate keys cannot arise here —
/// the YAML parser rejects them as duplicate mapping keys before this
/// function runs.
///
/// `$quill` and `$kind` require string scalars (non-string YAML types are
/// rejected). `$id` accepts any scalar and stringifies it.
pub(super) fn extract_meta_from_payload(
    payload: &mut JsonValue,
) -> Result<Vec<MetaValue>, ParseError> {
    let map = match payload {
        JsonValue::Object(m) => m,
        _ => return Ok(Vec::new()),
    };

    let dollar_keys: Vec<String> = map
        .keys()
        .filter(|k| k.starts_with('$'))
        .cloned()
        .collect();

    let mut out = Vec::with_capacity(dollar_keys.len());
    for key in dollar_keys {
        let value = map
            .shift_remove(&key)
            .expect("key was just enumerated from the same map");
        let meta = match key.as_str() {
            "$quill" => {
                let s = require_string("$quill reference", value)?;
                let reference = QuillReference::from_str(&s).map_err(|e| {
                    ParseError::InvalidStructure(format!(
                        "Invalid $quill reference '{}': {}",
                        s, e
                    ))
                })?;
                MetaValue::Quill(reference)
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
                MetaValue::Kind(s)
            }
            "$id" => MetaValue::Id(scalar_to_string(&key, value)?),
            other => {
                return Err(ParseError::InvalidStructure(format!(
                    "Unknown `{}` system-metadata key — the card-yaml block \
                     accepts only `$quill`, `$kind`, and `$id`",
                    other
                )));
            }
        };
        out.push(meta);
    }

    Ok(out)
}

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

/// Reject the reserved wire-format keys (`QUILL`, `CARD`, `BODY`, `CARDS`)
/// appearing as user-defined fields — they would collide with
/// [`crate::Document::to_plate_json`]'s output.
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

/// `true` when `name` matches `[a-z_][a-z0-9_]*`.
pub fn is_valid_kind_name(name: &str) -> bool {
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

/// Validate a composable card kind: must match `[a-z_][a-z0-9_]*` and must
/// not be the reserved root kind `"main"`.
///
/// Single source of truth for the composable-kind rule, used by
/// [`crate::Card::new`], [`crate::Document::set_card_kind`], and the storage
/// DTO conversion so the rule cannot drift between editor and reader paths.
pub fn validate_composable_kind(kind: &str) -> Result<(), CardKindError> {
    if !is_valid_kind_name(kind) {
        return Err(CardKindError::InvalidName);
    }
    if kind == "main" {
        return Err(CardKindError::Reserved);
    }
    Ok(())
}

/// Reason [`validate_composable_kind`] rejected a kind string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKindError {
    /// Kind did not match `[a-z_][a-z0-9_]*`.
    InvalidName,
    /// Kind was `"main"`, reserved for the document root.
    Reserved,
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
        assert_eq!(meta.len(), 2);
        assert!(matches!(meta[0], MetaValue::Quill(_)));
        assert!(matches!(meta[1], MetaValue::Kind(_)));
        assert_eq!(payload, json!({"title": "Doc"}));
    }

    #[test]
    fn extracts_id_from_number() {
        let mut payload = json!({"$id": 42});
        let meta = extract_meta_from_payload(&mut payload).unwrap();
        assert!(matches!(meta[0], MetaValue::Id(ref s) if s == "42"));
    }

    #[test]
    fn rejects_unknown_dollar_key() {
        let mut payload = json!({"$unknown": "x"});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        assert!(err.to_string().contains("Unknown `$unknown`"));
    }

    #[test]
    fn rejects_non_string_quill() {
        let mut payload = json!({"$quill": 42});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        assert!(err.to_string().contains("$quill reference"));
    }

    #[test]
    fn rejects_invalid_kind_pattern() {
        let mut payload = json!({"$kind": "Bad-Kind"});
        let err = extract_meta_from_payload(&mut payload).unwrap_err();
        assert!(err.to_string().contains("Invalid `$kind`"));
    }

    #[test]
    fn validate_composable_kind_rejects_main() {
        assert_eq!(
            validate_composable_kind("main"),
            Err(CardKindError::Reserved)
        );
    }

    #[test]
    fn validate_composable_kind_rejects_bad_name() {
        assert_eq!(
            validate_composable_kind("Bad-Name"),
            Err(CardKindError::InvalidName)
        );
    }

    #[test]
    fn validate_composable_kind_accepts_valid() {
        assert!(validate_composable_kind("indorsement").is_ok());
    }
}
