//! Value type for unified representation of YAML/JSON values.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::ops::Deref;
use std::sync::OnceLock;

/// Unified value type: an annotated tree of JSON-shaped data where every
/// node additionally records whether it was tagged `!must_fill`.
///
/// The tree is authoritative; the data API (`as_json`, `Deref`, …) reads a
/// lazily materialized, **fill-free** [`serde_json::Value`] projection of it.
/// Construction leaves `fill = false`; the document layer applies the markers.
/// No data-mutating method exists (only `set_fill_at`, which the projection
/// ignores), so the cache never goes stale.
pub struct QuillValue {
    node: Node,
    json: OnceLock<JsonValue>,
}

#[derive(Debug, Clone, PartialEq)]
struct Node {
    fill: bool,
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Node>),
    Object(IndexMap<String, Node>),
}

/// One step of a path into a value tree: an object key or an array index. The
/// canonical path-segment type for the whole crate, covering nested-comment and
/// nested-fill paths alike.
///
/// Serializes **untagged** (a key as a JSON string, an index as a JSON number),
/// so a path crosses the binding wire as a plain JS array like
/// `["addr", "street"]` or `["recipients", 0, "name"]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

fn collect_fill_paths(node: &Node, prefix: &mut Vec<PathSegment>, out: &mut Vec<Vec<PathSegment>>) {
    if node.fill {
        out.push(prefix.clone());
    }
    match &node.kind {
        Kind::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                prefix.push(PathSegment::Index(i));
                collect_fill_paths(child, prefix, out);
                prefix.pop();
            }
        }
        Kind::Object(entries) => {
            for (k, child) in entries {
                prefix.push(PathSegment::Key(k.clone()));
                collect_fill_paths(child, prefix, out);
                prefix.pop();
            }
        }
        _ => {}
    }
}

fn node_at_mut<'a>(node: &'a mut Node, path: &[PathSegment]) -> Option<&'a mut Node> {
    let mut cur = node;
    for seg in path {
        cur = match (&mut cur.kind, seg) {
            (Kind::Object(entries), PathSegment::Key(k)) => entries.get_mut(k)?,
            (Kind::Array(items), PathSegment::Index(i)) => items.get_mut(*i)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn node_is_object(node: &Node, path: &[PathSegment]) -> bool {
    fn at<'a>(node: &'a Node, path: &[PathSegment]) -> Option<&'a Node> {
        let mut cur = node;
        for seg in path {
            cur = match (&cur.kind, seg) {
                (Kind::Object(entries), PathSegment::Key(k)) => entries.get(k)?,
                (Kind::Array(items), PathSegment::Index(i)) => items.get(*i)?,
                _ => return None,
            };
        }
        Some(cur)
    }
    matches!(at(node, path).map(|n| &n.kind), Some(Kind::Object(_)))
}

impl Node {
    fn from_json(value: &JsonValue) -> Node {
        let kind = match value {
            JsonValue::Null => Kind::Null,
            JsonValue::Bool(b) => Kind::Bool(*b),
            JsonValue::Number(n) => Kind::Number(n.clone()),
            JsonValue::String(s) => Kind::String(s.clone()),
            JsonValue::Array(items) => Kind::Array(items.iter().map(Node::from_json).collect()),
            JsonValue::Object(map) => Kind::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), Node::from_json(v)))
                    .collect(),
            ),
        };
        Node { fill: false, kind }
    }

    fn to_json(&self) -> JsonValue {
        match &self.kind {
            Kind::Null => JsonValue::Null,
            Kind::Bool(b) => JsonValue::Bool(*b),
            Kind::Number(n) => JsonValue::Number(n.clone()),
            Kind::String(s) => JsonValue::String(s.clone()),
            Kind::Array(items) => JsonValue::Array(items.iter().map(Node::to_json).collect()),
            Kind::Object(entries) => JsonValue::Object(
                entries
                    .iter()
                    .map(|(k, n)| (k.clone(), n.to_json()))
                    .collect(),
            ),
        }
    }
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
    fn from_node(node: Node) -> Self {
        QuillValue {
            node,
            json: OnceLock::new(),
        }
    }

    /// Parse a YAML string under the parser's own budget, so an over-deep
    /// document errors rather than overflowing its stack.
    pub fn from_yaml_str(yaml_str: &str) -> Result<Self, crate::error::YamlError> {
        let json_val: serde_json::Value = serde_saphyr::from_str(yaml_str)
            .map_err(|e| crate::error::YamlError::from_de(e, yaml_str))?;
        Ok(Self::from_json(json_val))
    }

    /// The value's JSON projection, materialized on first use and cached.
    /// Data only: `!must_fill` markers are not represented in JSON.
    pub fn as_json(&self) -> &serde_json::Value {
        self.json.get_or_init(|| self.node.to_json())
    }

    /// Convert into the underlying JSON value, dropping fill markers.
    pub fn into_json(self) -> serde_json::Value {
        match self.json.into_inner() {
            Some(json) => json,
            None => self.node.to_json(),
        }
    }

    /// Create a QuillValue from a JSON value, with every node `fill = false`.
    pub fn from_json(json_val: serde_json::Value) -> Self {
        let node = Node::from_json(&json_val);
        let json = OnceLock::new();
        // Seed the projection so the render path doesn't re-lower it, at the
        // cost of holding the data twice.
        let _ = json.set(json_val);
        QuillValue { node, json }
    }

    /// Whether this value's root node carries the `!must_fill` marker.
    pub fn fill(&self) -> bool {
        self.node.fill
    }

    /// Paths (relative to this value's root) of every node carrying the
    /// `!must_fill` marker. The root, if filled, is reported as the empty
    /// path. The JSON projection carries no fill, so this is the only way to
    /// observe nested fill markers.
    pub fn fill_paths(&self) -> Vec<Vec<PathSegment>> {
        let mut out = Vec::new();
        let mut prefix = Vec::new();
        collect_fill_paths(&self.node, &mut prefix, &mut out);
        out
    }

    /// Every [`fill_paths`](Self::fill_paths) entry except the empty (root)
    /// path. A root fill rides the owning field's own `fill` flag, so the wire
    /// and storage DTOs record only the nested ones.
    pub fn nonroot_fill_paths(&self) -> impl Iterator<Item = Vec<PathSegment>> {
        self.fill_paths().into_iter().filter(|p| !p.is_empty())
    }

    /// Set `fill = true` on the node at `path` (relative to the root).
    /// Returns `false` if the path does not resolve to a node.
    pub fn set_fill_at(&mut self, path: &[PathSegment]) -> bool {
        match node_at_mut(&mut self.node, path) {
            Some(n) => {
                n.fill = true;
                true
            }
            None => false,
        }
    }

    /// Clear `fill` on the root node. A stored field's root marker rides the
    /// owning [`PayloadItem`](crate::PayloadItem)'s flag, so the tree beneath
    /// it carries the nested markers alone.
    pub(crate) fn clear_root_fill(&mut self) {
        self.node.fill = false;
    }

    /// Whether the node at `path` (relative to the root) is a mapping.
    /// Used to reject `!must_fill` on object-valued nodes.
    pub fn is_object_at(&self, path: &[PathSegment]) -> bool {
        node_is_object(&self.node, path)
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

impl PartialEq for QuillValue {
    /// Two values are equal when their annotated trees (data **and** fill)
    /// are equal. The cached JSON projection is derived and not compared.
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl Clone for QuillValue {
    fn clone(&self) -> Self {
        let json = OnceLock::new();
        if let Some(cached) = self.json.get() {
            let _ = json.set(cached.clone());
        }
        QuillValue {
            node: self.node.clone(),
            json,
        }
    }
}

impl std::fmt::Debug for QuillValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.node.fill {
            write!(f, "QuillValue(!must_fill {:?})", self.as_json())
        } else {
            write!(f, "QuillValue({:?})", self.as_json())
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
        match &self.node.kind {
            Kind::Object(entries) => entries.get(key).map(|n| QuillValue::from_node(n.clone())),
            _ => None,
        }
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
    fn json_round_trips_through_the_tree() {
        // Identity, object key order (serde_json `preserve_order`) included.
        let original = serde_json::json!({
            "z": 1,
            "a": [true, "x", 3.5, null],
            "nested": { "k": 42 }
        });
        let qv = QuillValue::from_json(original.clone());
        assert_eq!(qv.as_json(), &original);

        // Re-lowered from the tree rather than read off the seeded cache.
        let relowered = QuillValue::from_node(qv.node.clone()).into_json();
        assert_eq!(relowered, original);
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
    fn fill_marker_rides_on_the_node_not_the_json() {
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
}
