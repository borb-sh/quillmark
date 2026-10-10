//! `qm-cell`: a table cell's alignment spelled as the attributes of the
//! inline pair around the cell's whole content.

use super::Element;
use crate::island::{cell_alignment, CELL_VALIGNS, TABLE_ALIGNS};
use serde_json::Value;
use std::collections::BTreeMap;

/// The cell value attribute `name` spells with `value`, or `None` for a name
/// the engine does not name or a value outside its set: `align` is `left`,
/// `center` or `right`; `valign` is `top`, `middle` or `bottom`.
pub(crate) fn key(name: &str, value: &str) -> Option<Value> {
    let set: &[&str] = match name {
        "align" => &TABLE_ALIGNS,
        "valign" => &CELL_VALIGNS,
        _ => return None,
    };
    set.contains(&value).then(|| value.into())
}

/// The pair spelling `cell`'s alignment, or `None` when it holds none in its
/// set.
pub(crate) fn pair(cell: &Value) -> Option<Element> {
    let (align, valign) = cell_alignment(cell);
    let attrs: BTreeMap<String, String> = [("align", align), ("valign", valign)]
        .into_iter()
        .filter_map(|(k, v)| Some((k.to_string(), v?.to_string())))
        .collect();
    (!attrs.is_empty()).then(|| Element::new(super::CELL, attrs).expect("cell attribute names are in the grammar"))
}
