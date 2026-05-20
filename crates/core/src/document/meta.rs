//! Card-yaml block metadata: the `$`-prefixed reserved keys.
//!
//! Every `~~~card-yaml` block's YAML payload may carry up to three reserved
//! keys — `$quill`, `$kind`, `$id`. These are **system metadata** drawn from
//! a closed set; any other `$`-prefixed key is a parse error.
//!
//! ## Data shape
//!
//! [`CardMetadata`] is an ordered list of [`MetaItem`]s — the typed `$`
//! entries interleaved with any YAML comments that sit on or among them.
//! This mirrors how [`super::Payload`] preserves comments inside the user
//! payload: the round-trip is symmetric on both sides of the `$`/non-`$`
//! boundary.
//!
//! Typed accessors ([`CardMetadata::quill`] etc.) return the value of each
//! entry; mutators ([`CardMetadata::set_quill`] etc.) replace it in place
//! when present, or insert it at canonical position (`$quill` → `$kind` →
//! `$id`) when absent. Comments are untouched by typed mutators.

use std::str::FromStr;

use serde_json::Value as JsonValue;

use crate::error::ParseError;
use crate::version::QuillReference;

/// One entry in a [`CardMetadata`] block — a typed `$` system-metadata
/// field or a YAML comment.
///
/// `Comment.inline` distinguishes own-line comments (`# text` on a line by
/// itself) from inline trailing comments (`$kind: value # note`). Inline
/// comments attach to the `$` entry that immediately precedes them in the
/// items vector; if no such entry exists at emit time they degrade to
/// own-line comments. This mirrors [`super::payload::PayloadItem`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaItem {
    /// The `$quill` reference. Required on the document root; rejected on
    /// composable cards.
    Quill(QuillReference),
    /// The `$kind` card kind. `$kind: main` is reserved for the root.
    Kind(String),
    /// The `$id` opaque identifier.
    Id(String),
    /// A YAML comment line sitting on or among the `$` entries.
    Comment {
        text: String,
        inline: bool,
    },
}

impl MetaItem {
    /// Canonical sort key for `$` entries. Returns `None` for comments,
    /// which are positioned by source order and never reshuffled by typed
    /// mutators.
    fn canonical_rank(&self) -> Option<u8> {
        match self {
            MetaItem::Quill(_) => Some(0),
            MetaItem::Kind(_) => Some(1),
            MetaItem::Id(_) => Some(2),
            MetaItem::Comment { .. } => None,
        }
    }
}

/// Ordered system-metadata of a single card-yaml block.
///
/// Holds the block's `$`-prefixed entries (closed set: `$quill`, `$kind`,
/// `$id`) plus any YAML comments interleaved among them, in source order.
/// Typed accessors look up entries by kind; the [`items`](Self::items)
/// iterator walks the full ordered list (used by emit and the storage DTO).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CardMetadata {
    items: Vec<MetaItem>,
}

impl CardMetadata {
    /// Empty metadata — no `$` entries, no comments.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a pre-computed item list (parser and DTO entry point).
    ///
    /// The caller is responsible for ordering and validity: at most one
    /// `Quill`, `Kind`, or `Id` entry; kind values matching
    /// `[a-z_][a-z0-9_]*`; etc. Use [`set_quill`](Self::set_quill) and
    /// friends for safe interactive construction.
    pub fn from_items(items: Vec<MetaItem>) -> Self {
        Self { items }
    }

    /// Ordered iterator over the underlying item list.
    pub fn items(&self) -> &[MetaItem] {
        &self.items
    }

    /// The `$quill` reference, if declared.
    pub fn quill(&self) -> Option<&QuillReference> {
        self.items.iter().find_map(|i| match i {
            MetaItem::Quill(q) => Some(q),
            _ => None,
        })
    }

    /// The `$kind` value, if declared.
    pub fn kind(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            MetaItem::Kind(k) => Some(k.as_str()),
            _ => None,
        })
    }

    /// The `$id` value, if declared.
    pub fn id(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            MetaItem::Id(id) => Some(id.as_str()),
            _ => None,
        })
    }

    /// Set the `$quill` reference. Replaces the existing entry in place if
    /// present, or inserts at canonical position (before any `$kind` /
    /// `$id`) otherwise. Comments are not moved.
    pub fn set_quill(&mut self, reference: QuillReference) {
        self.upsert_canonical(MetaItem::Quill(reference));
    }

    /// Set the `$kind` value. Same insertion rules as
    /// [`set_quill`](Self::set_quill).
    pub fn set_kind(&mut self, kind: impl Into<String>) {
        self.upsert_canonical(MetaItem::Kind(kind.into()));
    }

    /// Set the `$id` value. Same insertion rules as
    /// [`set_quill`](Self::set_quill).
    pub fn set_id(&mut self, id: impl Into<String>) {
        self.upsert_canonical(MetaItem::Id(id.into()));
    }

    /// Remove the `$quill` entry, returning the previous value if any.
    pub fn take_quill(&mut self) -> Option<QuillReference> {
        let pos = self
            .items
            .iter()
            .position(|i| matches!(i, MetaItem::Quill(_)))?;
        match self.items.remove(pos) {
            MetaItem::Quill(q) => Some(q),
            _ => unreachable!(),
        }
    }

    fn upsert_canonical(&mut self, new: MetaItem) {
        let new_rank = new
            .canonical_rank()
            .expect("upsert_canonical only accepts typed $ entries");
        for slot in self.items.iter_mut() {
            if slot.canonical_rank() == Some(new_rank) {
                *slot = new;
                return;
            }
        }
        let insert_at = self
            .items
            .iter()
            .position(|i| matches!(i.canonical_rank(), Some(r) if r > new_rank))
            .unwrap_or(self.items.len());
        self.items.insert(insert_at, new);
    }
}

/// Walk the parsed YAML payload, extracting `$`-prefixed reserved keys
/// from the user field set and validating each into a typed
/// [`MetaItem`]. Returns the items in source order (saphyr preserves
/// mapping insertion order via the workspace's `serde_json/preserve_order`
/// feature).
///
/// Comments are not handled here — the caller interleaves them with these
/// typed items using the prescan's source-order [`super::prescan::PreItem`]
/// list. See `assemble::build_meta_and_payload`.
///
/// The accepted keys are the closed set `{$quill, $kind, $id}`. Any other
/// `$`-prefixed key is a parse error. Duplicate keys cannot arise — the
/// YAML parser rejects them as duplicate mapping keys before this function
/// runs. A `$quill` value that fails to parse as a [`QuillReference`], or a
/// `$kind` value that fails [`is_valid_kind_name`], is also a parse error.
///
/// `$quill` and `$kind` require string scalars (non-string YAML types are
/// rejected). `$id` accepts any scalar and stringifies it.
pub(super) fn extract_meta_from_payload(
    payload: &mut JsonValue,
) -> Result<Vec<MetaItem>, ParseError> {
    let map = match payload {
        JsonValue::Object(m) => m,
        _ => return Ok(Vec::new()),
    };

    let dollar_keys: Vec<String> = map
        .keys()
        .filter(|k| k.starts_with('$'))
        .cloned()
        .collect();

    let mut items = Vec::with_capacity(dollar_keys.len());
    for key in dollar_keys {
        let value = map
            .shift_remove(&key)
            .expect("key was just enumerated from the same map");
        let item = match key.as_str() {
            "$quill" => {
                let s = require_string("$quill reference", value)?;
                let reference = QuillReference::from_str(&s).map_err(|e| {
                    ParseError::InvalidStructure(format!(
                        "Invalid $quill reference '{}': {}",
                        s, e
                    ))
                })?;
                MetaItem::Quill(reference)
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
                MetaItem::Kind(s)
            }
            "$id" => MetaItem::Id(scalar_to_string(&key, value)?),
            other => {
                return Err(ParseError::InvalidStructure(format!(
                    "Unknown `{}` system-metadata key — the card-yaml block \
                     accepts only `$quill`, `$kind`, and `$id`",
                    other
                )));
            }
        };
        items.push(item);
    }

    Ok(items)
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
/// Single source of truth for the composable-kind rule. Used by
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

    fn extract(payload: &mut JsonValue) -> CardMetadata {
        CardMetadata::from_items(extract_meta_from_payload(payload).unwrap())
    }

    #[test]
    fn extracts_quill_kind_and_leaves_data_intact() {
        let mut payload = json!({
            "$quill": "foo@0.1",
            "$kind": "main",
            "title": "Doc",
        });
        let meta = extract(&mut payload);
        assert_eq!(meta.quill().unwrap().to_string(), "foo@0.1");
        assert_eq!(meta.kind(), Some("main"));
        assert_eq!(payload, json!({"title": "Doc"}));
    }

    #[test]
    fn extracts_id_from_string() {
        let mut payload = json!({"$id": "rev-1"});
        let meta = extract(&mut payload);
        assert_eq!(meta.id(), Some("rev-1"));
    }

    #[test]
    fn extracts_id_from_number() {
        let mut payload = json!({"$id": 42});
        let meta = extract(&mut payload);
        assert_eq!(meta.id(), Some("42"));
    }

    #[test]
    fn extracts_id_from_bool() {
        let mut payload = json!({"$id": true});
        let meta = extract(&mut payload);
        assert_eq!(meta.id(), Some("true"));
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
        let items = extract_meta_from_payload(&mut payload).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn set_quill_inserts_at_position_zero() {
        let mut meta = CardMetadata::new();
        meta.set_kind("main");
        meta.set_quill("foo@0.1".parse().unwrap());
        assert!(matches!(meta.items()[0], MetaItem::Quill(_)));
        assert!(matches!(meta.items()[1], MetaItem::Kind(_)));
    }

    #[test]
    fn set_id_inserts_after_quill_and_kind() {
        let mut meta = CardMetadata::new();
        meta.set_quill("foo@0.1".parse().unwrap());
        meta.set_kind("main");
        meta.set_id("rev-1");
        assert_eq!(meta.items().len(), 3);
        assert!(matches!(meta.items()[2], MetaItem::Id(_)));
    }

    #[test]
    fn set_replaces_in_place_preserving_position() {
        let mut meta = CardMetadata::from_items(vec![
            MetaItem::Quill("foo@0.1".parse().unwrap()),
            MetaItem::Comment {
                text: "trailing".into(),
                inline: true,
            },
            MetaItem::Kind("main".into()),
        ]);
        meta.set_quill("bar@0.2".parse().unwrap());
        assert_eq!(meta.quill().unwrap().to_string(), "bar@0.2");
        // Item count unchanged, comment still between quill and kind.
        assert_eq!(meta.items().len(), 3);
        assert!(matches!(meta.items()[1], MetaItem::Comment { .. }));
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
