//! Canonical key order and the depth bound of an opaque JSON payload: an
//! island's `props`, an `x-` member's `attrs`.

use serde_json::Value as JsonValue;

/// A `serde_json::Value` rendered to a string with object keys recursively
/// sorted: order-insensitive, so it is a stable comparison/grouping key.
pub(crate) fn canonical_json_string(v: &JsonValue) -> String {
    if is_value_key_sorted(v) {
        return serde_json::to_string(v).unwrap_or_default();
    }
    serde_json::to_string(&sort_keys_owned(v.clone())).unwrap_or_default()
}

/// Whether every object in `v` already has its keys in ascending order,
/// recursively: the allocation-free check that lets a re-normalize skip
/// rebuilding an already-canonical tree.
pub(crate) fn is_value_key_sorted(v: &JsonValue) -> bool {
    match v {
        JsonValue::Array(items) => items.iter().all(is_value_key_sorted),
        JsonValue::Object(map) => {
            map.keys().zip(map.keys().skip(1)).all(|(a, b)| a <= b)
                && map.values().all(is_value_key_sorted)
        }
        _ => true,
    }
}

/// `true` when `v` nests deeper than `max` container levels: the guard that
/// keeps the recursive walkers here and `Value`'s own `Drop` inside a bounded
/// frame count. The walk is iterative, so the check itself cannot overflow on
/// the adversarially deep input it exists to detect.
///
/// The unit is **container levels**, not nodes: only arrays/objects are charged
/// a level and a scalar leaf is never checked, so an empty container at level
/// `max + 1` is rejected exactly like a full one. `quillmark_core` re-exports
/// this as its own depth guard, so every boundary rejects the identical shape.
pub fn json_depth_exceeds(v: &JsonValue, max: usize) -> bool {
    // (value, depth) pairs; depth counts container levels entered.
    let mut stack: Vec<(&JsonValue, usize)> = vec![(v, 0)];
    while let Some((v, depth)) = stack.pop() {
        match v {
            JsonValue::Array(items) => {
                if depth + 1 > max {
                    return true;
                }
                stack.extend(items.iter().map(|c| (c, depth + 1)));
            }
            JsonValue::Object(map) => {
                if depth + 1 > max {
                    return true;
                }
                stack.extend(map.values().map(|c| (c, depth + 1)));
            }
            _ => {}
        }
    }
    false
}

/// Put `v` in canonical key order, rebuilding it only when a key is out of
/// order, so an untouched tree pays the scan and skips the deep clone.
///
/// Both walks below recurse, where the container walks do not: a container path
/// is flat memory at any depth, so nothing but the walk bounds it, while a
/// [`JsonValue`] deep enough to overflow these overflows its own `Drop` in the
/// frame that built it. The bound belongs where such a value enters: the
/// decode, before its clone, and [`Content::validate`](crate::model::Content::validate).
pub(crate) fn canonicalize_keys(v: &mut JsonValue) {
    if !is_value_key_sorted(v) {
        *v = sort_keys_owned(std::mem::take(v));
    }
}

/// Reorder every object's keys by **moving** each entry into a freshly
/// key-sorted map, recursively. Pins the canonical bytes against
/// `serde_json`'s `preserve_order` leaking insertion order; rebuilding the map
/// (rather than sorting in place) keeps that independent of whether the feature
/// is on in the crate graph.
pub(crate) fn sort_keys_owned(v: JsonValue) -> JsonValue {
    match v {
        JsonValue::Array(items) => {
            JsonValue::Array(items.into_iter().map(sort_keys_owned).collect())
        }
        JsonValue::Object(map) => {
            let mut entries: Vec<(String, JsonValue)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::with_capacity(entries.len());
            for (k, child) in entries {
                out.insert(k, sort_keys_owned(child));
            }
            JsonValue::Object(out)
        }
        other => other,
    }
}
