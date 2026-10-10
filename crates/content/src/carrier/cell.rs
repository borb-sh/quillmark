//! `qm-cell`: a table cell's alignment spelled as the attributes of the
//! inline pair around the cell's whole content.

use super::Element;
use crate::island::{cell_alignment, CELL_KEYS};
use serde_json::Value;
use std::collections::BTreeMap;

/// The cell value attribute `name` spells with `value`, or `None` for a name
/// outside [`CELL_KEYS`] or a value outside its set.
pub(crate) fn key(name: &str, value: &str) -> Option<Value> {
    let (_, set) = CELL_KEYS.iter().find(|(key, _)| *key == name)?;
    set.contains(&value).then(|| value.into())
}

/// The pair spelling `cell`'s alignment, or `None` when it holds none in its
/// set.
pub(crate) fn pair(cell: &Value) -> Option<Element> {
    let (align, valign) = cell_alignment(cell);
    let attrs: BTreeMap<String, String> = CELL_KEYS
        .iter()
        .zip([align, valign])
        .filter_map(|((k, _), v)| Some((k.to_string(), v?.to_string())))
        .collect();
    (!attrs.is_empty()).then(|| Element::new(super::CELL, attrs).expect("cell attribute names are in the grammar"))
}
