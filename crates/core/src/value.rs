//! Value type for unified representation of YAML/JSON values.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::ops::Deref;

/// Unified value type: JSON-shaped data beside the paths of the nodes tagged
/// `!must_fill`.
///
/// The JSON is the data in full, fill-free, and every reader of it (`as_json`,
/// `Deref`, `Serialize`, …) hands it out directly. Construction records no
/// marker; the document layer applies them with
/// [`set_fill_at`](Self::set_fill_at), which is the only mutator — the data
/// itself is never edited in place, so a recorded path cannot come to address a
/// node that has gone.
///
/// `fills` is kept sorted and duplicate-free, so equality over it is set
/// equality and the order a caller marks in is not observable.
#[derive(Clone, PartialEq)]
pub struct QuillValue {
    json: JsonValue,
    fills: Vec<Vec<PathSegment>>,
}

/// One step of a path into a value tree: an object key or an array index. The
/// canonical path-segment type for the whole crate, covering nested-comment and
/// nested-fill paths alike.
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

fn json_at<'a>(value: &'a JsonValue, path: &[PathSegment]) -> Option<&'a JsonValue> {
    let mut cur = value;
    for seg in path {
        cur = match (cur, seg) {
            (JsonValue::Object(map), PathSegment::Key(k)) => map.get(k)?,
            (JsonValue::Array(items), PathSegment::Index(i)) => items.get(*i)?,
            _ => return None,
        };
    }
    Some(cur)
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

    /// The value's data. `!must_fill` markers are not represented in JSON.
    pub fn as_json(&self) -> &serde_json::Value {
        &self.json
    }

    /// Convert into the underlying JSON value, dropping fill markers.
    pub fn into_json(self) -> serde_json::Value {
        self.json
    }

    /// Create a QuillValue from a JSON value, carrying no fill marker.
    pub fn from_json(json_val: serde_json::Value) -> Self {
        QuillValue {
            json: json_val,
            fills: Vec::new(),
        }
    }

    /// Whether this value's root carries the `!must_fill` marker.
    pub fn fill(&self) -> bool {
        self.fills.iter().any(Vec::is_empty)
    }

    /// Paths (relative to this value's root) of every node carrying the
    /// `!must_fill` marker. The root, if filled, is reported as the empty
    /// path. The JSON carries no fill, so this is the only way to observe
    /// nested fill markers.
    pub fn fill_paths(&self) -> Vec<Vec<PathSegment>> {
        self.fills.clone()
    }

    /// Every [`fill_paths`](Self::fill_paths) entry except the empty (root)
    /// path. A root fill rides the owning field's own `fill` flag, so the wire
    /// and storage DTOs record only the nested ones.
    pub fn nonroot_fill_paths(&self) -> impl Iterator<Item = Vec<PathSegment>> {
        self.fill_paths().into_iter().filter(|p| !p.is_empty())
    }

    /// Mark the node at `path` (relative to the root) `!must_fill`. Returns
    /// `false`, recording nothing, if the path does not resolve to a node: a
    /// marker never outlives what it addresses.
    pub fn set_fill_at(&mut self, path: &[PathSegment]) -> bool {
        if json_at(&self.json, path).is_none() {
            return false;
        }
        if let Err(i) = self.fills.binary_search_by(|p| p.as_slice().cmp(path)) {
            self.fills.insert(i, path.to_vec());
        }
        true
    }

    /// Clear the root marker. A stored field's root marker rides the owning
    /// [`PayloadItem`](crate::PayloadItem)'s flag, so the value beneath it
    /// carries the nested markers alone.
    pub(crate) fn clear_root_fill(&mut self) {
        self.fills.retain(|p| !p.is_empty());
    }

    /// Whether the node at `path` (relative to the root) is a mapping.
    /// Used to reject `!must_fill` on object-valued nodes.
    pub fn is_object_at(&self, path: &[PathSegment]) -> bool {
        json_at(&self.json, path).is_some_and(JsonValue::is_object)
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
        if self.fill() {
            write!(f, "QuillValue(!must_fill {:?})", self.json)
        } else {
            write!(f, "QuillValue({:?})", self.json)
        }
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
    /// Get a field from an object by key, preserving the child's fill markers.
    /// The scalar reads come from the [`Deref`] target instead, where fill is
    /// not observable anyway.
    pub fn get(&self, key: &str) -> Option<QuillValue> {
        let child = self.json.as_object()?.get(key)?;
        let head = PathSegment::Key(key.to_string());
        Some(QuillValue {
            json: child.clone(),
            // Stripping a shared head preserves the sort, so the child's list
            // is normalized by the parent's being so.
            fills: self
                .fills
                .iter()
                .filter_map(|p| p.split_first())
                .filter(|(first, _)| **first == head)
                .map(|(_, rest)| rest.to_vec())
                .collect(),
        })
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
    fn test_yaml_custom_tags_ignored_at_value_level() {
        // The tag is recovered a layer up, by `document::prescan`.
        let yaml_str = "memo_from: !must_fill 2d lt example";
        let quill_val = QuillValue::from_yaml_str(yaml_str).unwrap();

        assert_eq!(
            quill_val.get("memo_from").as_ref().and_then(|v| v.as_str()),
            Some("2d lt example")
        );
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

    #[test]
    fn fill_marker_rides_beside_the_json_not_in_it() {
        let filled = || {
            let mut qv = QuillValue::from("draft");
            assert!(qv.set_fill_at(&[]));
            qv
        };
        let qv = filled();
        assert!(qv.fill());
        assert_eq!(qv.as_json(), &serde_json::json!("draft"));
        assert_ne!(qv, QuillValue::from("draft"), "equality is fill-sensitive");
        assert_eq!(qv, filled());
    }

    /// The marking order and repetition a caller happens to use are not
    /// observable: the same set of markers is the same value.
    #[test]
    fn fill_markers_normalize_to_a_set() {
        let key = |k: &str| vec![PathSegment::Key(k.to_string())];
        let mark = |order: [&str; 3]| {
            let mut qv = QuillValue::from_json(serde_json::json!({"a": 1, "b": 2, "c": 3}));
            for k in order {
                assert!(qv.set_fill_at(&key(k)));
            }
            qv
        };
        let forward = mark(["a", "b", "c"]);
        assert_eq!(forward, mark(["c", "a", "b"]));
        assert_eq!(forward.fill_paths().len(), 3);

        let mut twice = mark(["a", "b", "c"]);
        assert!(twice.set_fill_at(&key("a")));
        assert_eq!(twice, forward, "re-marking a node changes nothing");
    }

    /// A marker addresses a node or is refused, so the recorded paths always
    /// resolve against the data beside them.
    #[test]
    fn fill_marker_on_an_absent_node_is_refused() {
        let mut qv = QuillValue::from_json(serde_json::json!({"a": [1]}));
        assert!(!qv.set_fill_at(&[PathSegment::Key("nope".to_string())]));
        assert!(!qv.set_fill_at(&[
            PathSegment::Key("a".to_string()),
            PathSegment::Index(9),
        ]));
        assert!(qv.fill_paths().is_empty());
    }

    #[test]
    fn get_carries_the_child_markers_and_drops_its_siblings() {
        let mut qv = QuillValue::from_json(serde_json::json!({
            "addr": { "street": "", "city": "" },
            "name": "",
        }));
        let street = vec![
            PathSegment::Key("addr".to_string()),
            PathSegment::Key("street".to_string()),
        ];
        assert!(qv.set_fill_at(&street));
        assert!(qv.set_fill_at(&[PathSegment::Key("name".to_string())]));

        let addr = qv.get("addr").expect("addr is a member");
        assert_eq!(
            addr.fill_paths(),
            vec![vec![PathSegment::Key("street".to_string())]],
            "rebased onto the child, and `name` left behind"
        );
        assert!(!addr.fill(), "the child root carries no marker of its own");
    }
}
