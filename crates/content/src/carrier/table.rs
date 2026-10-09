//! `quill-table`: a table island's layout props spelled as the attributes of
//! the block wrapper around its pipe table.

use super::Element;
use serde_json::Value;
use std::collections::BTreeMap;

/// The `widths` token for an auto-fit column, a `null` entry.
const AUTO: &str = "auto";

/// The largest column weight, JavaScript's `Number.MAX_SAFE_INTEGER`: the
/// WASM binding hands no larger integer to JavaScript.
pub(crate) const MAX_WEIGHT: u64 = (1 << 53) - 1;

/// The props value attribute `name` spells with `value`, or `None` for a name
/// the engine does not name or a value outside its spelling: `widths` is
/// whitespace-separated tokens, each a decimal weight in `1..=`[`MAX_WEIGHT`]
/// or `auto`; `align` is `left`, `center` or `right`.
pub(crate) fn prop(name: &str, value: &str) -> Option<Value> {
    match name {
        "widths" => value
            .split_ascii_whitespace()
            .map(|t| match t {
                AUTO => Some(Value::Null),
                t => weight(t).map(Value::from),
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        "align" => crate::island::TABLE_ALIGNS.contains(&value).then(|| value.into()),
        _ => None,
    }
}

fn weight(token: &str) -> Option<u64> {
    if !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok().filter(|n| (1..=MAX_WEIGHT).contains(n))
}

/// The wrapper spelling normalized `props`' layout keys, or `None` when none
/// is present.
pub(crate) fn wrapper(props: &Value) -> Option<Element> {
    let mut attrs = BTreeMap::new();
    if let Some(widths) = props.get("widths").and_then(Value::as_array) {
        let tokens: Vec<String> = widths
            .iter()
            .map(|w| w.as_u64().map_or_else(|| AUTO.to_string(), |n| n.to_string()))
            .collect();
        attrs.insert("widths".to_string(), tokens.join(" "));
    }
    if let Some(align) = props.get("align").and_then(Value::as_str) {
        attrs.insert("align".to_string(), align.to_string());
    }
    if attrs.is_empty() {
        return None;
    }
    Some(Element::new("table", attrs).expect("layout attribute names are in the grammar"))
}
