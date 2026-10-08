//! `quill-cell`: a table cell's alignment keys spelled as the attributes of
//! the inline pair around the cell's whole content.

use super::Element;
use serde_json::Value;
use std::collections::BTreeMap;

/// Each cell key and the values it reads.
const KEYS: [(&str, &[&str]); 2] = [
    ("align", &["left", "center", "right"]),
    ("valign", &["top", "horizon", "bottom"]),
];

/// The cell value attribute `name` spells with `value`, or `None` for a name
/// the engine does not name or a value outside its set: `align` is `left`,
/// `center` or `right`; `valign` is `top`, `horizon` or `bottom`.
pub(crate) fn key(name: &str, value: &str) -> Option<Value> {
    KEYS.iter()
        .any(|(k, set)| *k == name && set.contains(&value))
        .then(|| value.into())
}

/// The pair spelling a normalized cell's keys, or `None` when it holds none.
pub(crate) fn pair(cell: &Value) -> Option<Element> {
    let attrs: BTreeMap<String, String> = KEYS
        .iter()
        .filter_map(|(k, _)| Some((k.to_string(), cell.get(k)?.as_str()?.to_string())))
        .collect();
    if attrs.is_empty() {
        return None;
    }
    Some(Element::new("cell", attrs).expect("cell attribute names are in the grammar"))
}
