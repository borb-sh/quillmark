//! Value type for unified representation of YAML/JSON values.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::ops::Deref;

/// Unified value type for JSON-shaped data.
#[derive(Clone, PartialEq)]
pub struct QuillValue {
    json: JsonValue,
}

/// One step of a path into a value tree: an object key or an array index. The
/// canonical path-segment type for the whole crate.
///
/// Serializes **untagged** (a key as a JSON string, an index as a JSON number),
/// so a path crosses the binding wire as a plain JS array like
/// `["addr", "street"]` or `["recipients", 0, "name"]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

/// `true` when a value nests deeper than `max_depth` container levels: the
/// content crate's guard, re-exported so this crate's boundaries and the content
/// model reject the identical shape.
///
/// Every path that stores a value into a `Document` bounds nesting at
/// [`MAX_JSON_DEPTH`](quillmark_content::MAX_JSON_DEPTH), so the recursive consumers
/// (emit, plate-JSON serialization, DTO conversion) are bounded by
/// construction. The Python binding's `py_to_json_at` charges levels the same
/// way.
pub use quillmark_content::model::json_depth_exceeds;

/// Depth-bound an owned `$ext` / `$seed` map against
/// [`MAX_JSON_DEPTH`](quillmark_content::MAX_JSON_DEPTH), returning it
/// unchanged when within bounds. On overflow, `on_too_deep` builds the caller's
/// boundary error from the limit, so each write surface keeps its own error
/// type while the wrap-check-rebuild lives here once.
pub(crate) fn depth_check_meta_map<E>(
    map: serde_json::Map<String, serde_json::Value>,
    on_too_deep: impl FnOnce(usize) -> E,
) -> Result<serde_json::Map<String, serde_json::Value>, E> {
    let max = quillmark_content::MAX_JSON_DEPTH;
    let as_value = serde_json::Value::Object(map);
    if json_depth_exceeds(&as_value, max) {
        return Err(on_too_deep(max));
    }
    let serde_json::Value::Object(map) = as_value else {
        unreachable!("constructed as Object above")
    };
    Ok(map)
}

impl QuillValue {
    /// Parse a YAML string under the parser's own budget, so an over-deep
    /// document errors rather than overflowing its stack.
    pub fn from_yaml_str(yaml_str: &str) -> Result<Self, crate::error::YamlError> {
        let json_val: serde_json::Value = serde_saphyr::from_str(yaml_str)
            .map_err(|e| crate::error::YamlError::from_de(e, yaml_str))?;
        Ok(Self::from_json(json_val))
    }

    /// The value's data.
    pub fn as_json(&self) -> &serde_json::Value {
        &self.json
    }

    /// Convert into the underlying JSON value.
    pub fn into_json(self) -> serde_json::Value {
        self.json
    }

    /// Create a QuillValue from a JSON value.
    pub fn from_json(json_val: serde_json::Value) -> Self {
        QuillValue { json: json_val }
    }
}

/// Scalar conversions mirror [`serde_json::Value`]'s, so a non-finite `f64`
/// maps to null. These back the `impl Into<QuillValue>` mutator parameters.
macro_rules! impl_from_scalar {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for QuillValue {
            fn from(v: $ty) -> Self {
                QuillValue::from_json(serde_json::Value::from(v))
            }
        }
    )*};
}
impl_from_scalar!(&str, String, bool, i32, i64, u32, u64, f64);

impl From<serde_json::Value> for QuillValue {
    fn from(v: serde_json::Value) -> Self {
        QuillValue::from_json(v)
    }
}

impl Deref for QuillValue {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        self.as_json()
    }
}

impl std::fmt::Debug for QuillValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "QuillValue({:?})", self.json)
    }
}

impl Serialize for QuillValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QuillValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let json = serde_json::Value::deserialize(deserializer)?;
        Ok(QuillValue::from_json(json))
    }
}

impl QuillValue {
    /// Get a field from an object by key.
    pub fn get(&self, key: &str) -> Option<QuillValue> {
        let child = self.json.as_object()?.get(key)?;
        Some(QuillValue::from_json(child.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_error_locates_and_sanitizes() {
        let err = QuillValue::from_yaml_str("a: 1\nb: [unclosed\n")
            .expect_err("malformed YAML must not parse");
        let diag = err.to_diagnostic("quill::yaml_parse_error", "Quill.yaml");
        let loc = diag.location.expect("a located error carries a Location");
        assert_eq!(loc.file, "Quill.yaml");
        assert!(
            loc.line >= 2 && loc.column >= 1,
            "1-indexed position at or past the unclosed sequence: {loc:?}"
        );
        assert_eq!(diag.code.as_deref(), Some("quill::yaml_parse_error"));
    }

    /// `YamlError` promises the engine is invisible, text included.
    #[test]
    fn yaml_error_strips_the_engine_api_names() {
        let err = QuillValue::from_yaml_str("a: 1\na: 2\n")
            .expect_err("a duplicate key must not parse");
        assert!(
            !err.message().contains("DuplicateKeyPolicy") && !err.message().contains("Options"),
            "engine API names reached the message: {}",
            err.message()
        );
        assert!(err.message().contains("duplicate"), "{}", err.message());
    }

    #[test]
    fn json_round_trips_declaration_order_included() {
        // Identity, object key order (serde_json `preserve_order`) included.
        let original = serde_json::json!({
            "z": 1,
            "a": [true, "x", 3.5, null],
            "nested": { "k": 42 }
        });
        let qv = QuillValue::from_json(original.clone());
        assert_eq!(qv.as_json(), &original);
        assert_eq!(qv.into_json(), original);
    }

    #[test]
    fn depth_check_counts_empty_containers() {
        use serde_json::json;

        // A container occupies a level even when empty.
        assert!(!json_depth_exceeds(&json!([]), 1));
        assert!(!json_depth_exceeds(&json!({}), 1));
        assert!(json_depth_exceeds(&json!([[]]), 1));
        assert!(json_depth_exceeds(&json!({ "a": {} }), 1));

        // `[[[…[]…]]]`, built iteratively so the test stays stack-safe.
        let deep_empty = |levels: usize| {
            let mut v = serde_json::Value::Array(Vec::new());
            for _ in 1..levels {
                v = serde_json::Value::Array(vec![v]);
            }
            v
        };
        assert!(!json_depth_exceeds(&deep_empty(100), 100));
        assert!(json_depth_exceeds(&deep_empty(101), 100));
    }

    #[test]
    fn depth_check_counts_container_levels_not_the_scalar_leaf() {
        // The Python binding's `py_to_json_at` pins the same boundary.
        let scalar_terminated = |levels: usize| {
            let mut v = serde_json::json!(1);
            for _ in 0..levels {
                v = serde_json::json!({ "a": v });
            }
            v
        };
        assert!(!json_depth_exceeds(&scalar_terminated(100), 100));
        assert!(json_depth_exceeds(&scalar_terminated(101), 100));

        // The deepest container, not its contents, occupies the last level.
        let container_terminated = |levels: usize| {
            let mut v = serde_json::json!([1, 2, 3]);
            for _ in 1..levels {
                v = serde_json::json!({ "a": v });
            }
            v
        };
        assert!(!json_depth_exceeds(&container_terminated(100), 100));
        assert!(json_depth_exceeds(&container_terminated(101), 100));
    }
}
