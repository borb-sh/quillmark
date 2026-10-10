//! `qm-table`: a table island's layout props spelled as the attributes of
//! the block wrapper around its pipe table.

use super::Element;
use crate::island::{is_weight, TableLayout, TABLE_ALIGNS};
use serde_json::Value;
use std::collections::BTreeMap;

/// The `widths` token for an auto-fit column, a `null` entry.
const AUTO: &str = "auto";

/// The props value attribute `name` spells with `value`, or `None` for a name
/// the engine does not name or a value outside its spelling: `widths` is
/// whitespace-separated tokens, each a decimal [weight](is_weight) or `auto`;
/// `align` is one of [`TABLE_ALIGNS`]; `headless` is bare.
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
        "align" => TABLE_ALIGNS.contains(&value).then(|| value.into()),
        "headless" => value.is_empty().then_some(Value::Bool(true)),
        _ => None,
    }
}

fn weight(token: &str) -> Option<u64> {
    if !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok().filter(|n| is_weight(*n))
}

/// The wrapper spelling `props`' layout, or `None` where every key is at its
/// default.
pub(crate) fn wrapper(props: &Value) -> Option<Element> {
    let layout = TableLayout::of(props);
    let mut attrs = BTreeMap::new();
    if let Some(widths) = layout.widths {
        let tokens: Vec<String> = widths.iter().map(|w| w.map_or_else(|| AUTO.to_string(), |n| n.to_string())).collect();
        attrs.insert("widths".to_string(), tokens.join(" "));
    }
    if let Some(align) = layout.align {
        attrs.insert("align".to_string(), align.to_string());
    }
    if layout.headless {
        attrs.insert("headless".to_string(), String::new());
    }
    if attrs.is_empty() {
        return None;
    }
    Some(Element::spelling(super::TABLE, attrs).expect("layout attribute names are in the grammar"))
}
