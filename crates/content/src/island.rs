//! Island types: the dispatch authority over
//! [`Island::island_type`](crate::model::Island::island_type).

use crate::model::Mark;
use serde_json::Value;

/// The island types. Closed: a wire `type` outside this set is
/// [`ParseError::UnknownName`](crate::serial::ParseError::UnknownName).
///
/// Every emitter dispatches over the whole set: an island type wired into some
/// and not others projects the island away silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandType {
    /// `{header, rows, aligns}` with inline `{text, marks}` cells, a `\n` in a
    /// cell's text being a line break, and the optional layout keys `widths`
    /// (a weight or `null` per column) and `align` (one of [`TABLE_ALIGNS`]).
    /// Mark-carrying, shape-normalized (one column count, `\n` the only
    /// line-break char a cell keeps, each layout key absent at its default).
    Table,
    /// `{url, alt}`. No cell model, no shape invariants.
    Image,
}

/// The values of a table's `align` key, its placement.
pub const TABLE_ALIGNS: [&str; 3] = ["left", "center", "right"];

impl IslandType {
    /// Every known type, for a reader that needs the closed set whole.
    pub const ALL: &'static [IslandType] = &[IslandType::Table, IslandType::Image];

    /// The wire discriminator; `parse(k.as_str()) == Some(k)` for every variant.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Image => "image",
        }
    }

    /// Parse a wire discriminator; `parse(k.as_str()) == Some(k)` for every
    /// variant.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "table" => Some(Self::Table),
            "image" => Some(Self::Image),
            _ => None,
        }
    }

    /// Whether this type's markdown projection is a **block**: markup no
    /// paragraph line can hold, so its slot has to sit alone on its line. A
    /// pipe table is one; an image is inline (`![alt](url)`).
    pub fn block_only(self) -> bool {
        match self {
            Self::Table => true,
            Self::Image => false,
        }
    }

    /// This type's `(text, marks)` cells: the set that participates in mark
    /// normalization and cell-mark validation. Empty for a type with no cell
    /// model.
    pub fn cell_marks(self, props: &Value) -> Vec<(String, Vec<Mark>)> {
        match self {
            Self::Table => crate::serial::table_cells(props),
            Self::Image => Vec::new(),
        }
    }

    /// Refuse a cell mark whose `type` is outside the mark vocabulary. The
    /// decoder's arm of [`cell_marks`](Self::cell_marks): those read leniently,
    /// so this is the one place the row can be refused rather than silently
    /// re-encoded without the mark.
    pub fn reject_unknown_cell_mark(self, props: &Value) -> Result<(), crate::serial::ParseError> {
        match self {
            Self::Table => crate::serial::reject_unknown_cell_mark_name(props),
            Self::Image => Ok(()),
        }
    }

    /// Repair this type's props to canonical shape in place; a no-op for a type
    /// with no shape invariants.
    pub fn normalize_props(self, props: &mut Value) {
        match self {
            Self::Table => crate::serial::normalize_table_props(props),
            Self::Image => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_round_trip() {
        for k in [IslandType::Table, IslandType::Image] {
            assert_eq!(IslandType::parse(k.as_str()), Some(k));
        }
    }

    /// An island op's props are untyped at the wire, so a scalar where an array
    /// belongs reaches the mint, which answers with one column count across
    /// `header`, `aligns` and every row, and is a fixed point on what it wrote.
    #[test]
    fn normalize_props_settles_every_props_shape_on_one_column_count() {
        for props in [
            serde_json::json!({"header": ["h"], "aligns": "bogus", "rows": [["a"]]}),
            serde_json::json!({"header": "bogus", "aligns": ["left"], "rows": [["a"]]}),
            serde_json::json!({"header": ["h"], "aligns": ["left"], "rows": ["bogus"]}),
            serde_json::json!({"header": ["h"], "aligns": 7, "rows": [[], null]}),
        ] {
            let mut props = props;
            IslandType::Table.normalize_props(&mut props);

            let len = |k: &str| props[k].as_array().expect("an array").len();
            let cols = len("header");
            assert_eq!(len("aligns"), cols, "aligns off the column count: {props}");
            for row in props["rows"].as_array().expect("an array") {
                let width = row.as_array().expect("an array").len();
                assert_eq!(width, cols, "row off the column count: {props}");
            }

            let once = props.clone();
            IslandType::Table.normalize_props(&mut props);
            assert_eq!(props, once, "normalize_props is not a fixed point");
        }
    }

    /// `widths` settles to the column count, keeps its weights as written and
    /// never widens the table; each layout key is absent at its default or
    /// when invalid.
    #[test]
    fn normalize_props_settles_the_layout_keys() {
        use serde_json::{json, Value};
        let absent = Value::Null;
        let cases: &[(Value, &str, Value)] = &[
            (json!([2, 4, null]), "widths", json!([2, 4, null])),
            (json!([3, null]), "widths", json!([3, null, null])),
            (json!([6, 9, 12, 15]), "widths", json!([6, 9, 12])),
            (json!([null, null, 5]), "widths", json!([null, null, 5])),
            (json!([null, null, null, 4]), "widths", absent.clone()),
            (json!([null, null]), "widths", absent.clone()),
            (json!([]), "widths", absent.clone()),
            (json!([1, 0, 1]), "widths", absent.clone()),
            (json!([1, -1, 1]), "widths", absent.clone()),
            (json!([1, 1.5, 1]), "widths", absent.clone()),
            (json!([1, "2", 1]), "widths", absent.clone()),
            (json!([9007199254740992u64, 1]), "widths", absent.clone()),
            (json!([9007199254740991u64, 1]), "widths", json!([9007199254740991u64, 1, null])),
            (json!("1 2 3"), "widths", absent.clone()),
            (json!(2), "widths", absent.clone()),
            (json!("center"), "align", json!("center")),
            (json!("left"), "align", json!("left")),
            (json!("right"), "align", json!("right")),
            (json!("middle"), "align", absent.clone()),
            (json!(["center"]), "align", absent.clone()),
        ];
        for (value, key, settled) in cases {
            let mut props = json!({"header": ["a", "b", "c"], "rows": [["1", "2", "3"]]});
            props[*key] = value.clone();
            IslandType::Table.normalize_props(&mut props);
            assert_eq!(props.get(*key).unwrap_or(&absent), settled, "{key}: {value}");
            assert_eq!(props["header"].as_array().unwrap().len(), 3, "{key}: {value}");

            let once = props.clone();
            IslandType::Table.normalize_props(&mut props);
            assert_eq!(props, once, "{key}: {value} is not a fixed point");
        }
    }
}
