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
    /// cell's text being a line break, and the optional [layout](TableLayout)
    /// keys `widths`, `align` and `head`. A cell's optional `align` and
    /// `valign` are its [alignment](cell_alignment), and its `colspan` and
    /// `rowspan` its [span](cell_span), each cell another's span covers an
    /// empty placeholder ([`table_spans`]).
    /// Mark-carrying, shape-normalized (one column count, `\n` the only
    /// line-break char a cell keeps, each knob key absent at its default,
    /// `widths` settled to the column count and each span to what it can
    /// cover). A knob key holding a value outside its set rides as a key
    /// Quillmark does not name does, and reads as absent.
    Table,
    /// `{url, alt}`. No cell model, no shape invariants.
    Image,
    /// A footnote: `{text, marks}`, the note, shaped as one table cell and
    /// normalized as one, with no line break at either edge. Its slot is the
    /// reference, so the note sits where it is cited.
    Footnote,
    /// An equation: `{tex}`, its TeX source, and `display: true` for one set
    /// apart from the line. `tex` holds no line break and no whitespace at
    /// either edge, which TeX reads as nothing; `display` is absent at
    /// `false`. No cell model.
    Math,
}

/// The values of a table's `align` key, its placement, and of a cell's, its
/// horizontal alignment.
pub const TABLE_ALIGNS: [&str; 3] = ["left", "center", "right"];

/// The values of a table cell's `valign` key, its vertical alignment.
pub const CELL_VALIGNS: [&str; 3] = ["top", "middle", "bottom"];

/// A table's layout keys.
pub const TABLE_KEYS: [&str; 3] = ["widths", "align", "head"];

/// A table cell's alignment keys, each with its values.
pub const CELL_KEYS: [(&str, &[&str]); 2] = [("align", &TABLE_ALIGNS), ("valign", &CELL_VALIGNS)];

/// A table cell's span keys: the columns and the rows it covers, each a whole
/// number from 1, absent at 1.
pub const CELL_SPANS: [&str; 2] = ["colspan", "rowspan"];

/// The largest column weight, JavaScript's `Number.MAX_SAFE_INTEGER`: the
/// WASM binding hands no larger integer to JavaScript.
pub const MAX_WEIGHT: u64 = (1 << 53) - 1;

/// Whether `n` is a column weight, `1..=`[`MAX_WEIGHT`].
pub fn is_weight(n: u64) -> bool {
    (1..=MAX_WEIGHT).contains(&n)
}

/// A table's layout as its props hold it, each key at its default where
/// absent or outside its set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLayout {
    /// A [weight](is_weight), or `None` for an auto-fit column, per column.
    pub widths: Option<Vec<Option<u64>>>,
    /// The table's placement, one of [`TABLE_ALIGNS`].
    pub align: Option<&'static str>,
    /// How many rows from the top of `header` then `rows` are header rows,
    /// at most every row: `head` where it holds a whole number, else `1`.
    pub head: usize,
}

impl Default for TableLayout {
    fn default() -> Self {
        TableLayout { widths: None, align: None, head: 1 }
    }
}

impl TableLayout {
    pub fn of(props: &Value) -> Self {
        let widths = props.get("widths").and_then(Value::as_array).and_then(|ws| {
            ws.iter()
                .map(|w| match w {
                    Value::Null => Some(None),
                    w => w.as_u64().filter(|n| is_weight(*n)).map(Some),
                })
                .collect()
        });
        let head = props.get("head").and_then(Value::as_u64).map_or(1, saturate);
        TableLayout {
            widths,
            align: one_of(props.get("align"), &TABLE_ALIGNS),
            head: head.min(table_rows(props).len()),
        }
    }
}

fn saturate(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// A table cell's `colspan` and `rowspan` as it holds them, each `1` where the
/// cell holds no whole number from 1.
pub fn cell_span(cell: &Value) -> (usize, usize) {
    let read = |key: &str| cell.get(key).and_then(Value::as_u64).filter(|n| *n >= 1).map_or(1, saturate);
    let [colspan, rowspan] = CELL_SPANS;
    (read(colspan), read(rowspan))
}

/// A table's rows, `header` first, each a row's cells; a row that is no array
/// holds none.
pub fn table_rows(props: &Value) -> Vec<&[Value]> {
    fn row(v: &Value) -> &[Value] {
        v.as_array().map_or(&[], Vec::as_slice)
    }
    let header = props.get("header").map_or(&[][..], row);
    let rows = props.get("rows").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    std::iter::once(header).chain(rows.iter().map(row)).collect()
}

/// Per row of [`table_rows`] and per cell, the columns and rows a drawn cell
/// spans, or `None` for a cell another's span covers.
///
/// Cells claim slots in reading order, so an earlier span wins. A span takes
/// the columns to its right, then the rows below, while each slot it would
/// cover holds a cell with no text that no other span covers, stopping at the
/// grid's edge and at the end of its own section: the [`TableLayout::head`]
/// rows or the body. Its cell's own `colspan` and `rowspan` bound it.
pub fn table_spans(props: &Value) -> Vec<Vec<Option<(usize, usize)>>> {
    let rows = table_rows(props);
    let head = TableLayout::of(props).head;
    let mut covered: Vec<Vec<bool>> = rows.iter().map(|r| vec![false; r.len()]).collect();
    let mut spans: Vec<Vec<Option<(usize, usize)>>> = rows.iter().map(|r| vec![None; r.len()]).collect();
    let free = |covered: &[Vec<bool>], r: usize, c: usize| {
        let empty = rows[r].get(c).is_some_and(|cell| cell.get("text").and_then(Value::as_str).unwrap_or("").is_empty());
        empty && !covered[r][c]
    };
    for r in 0..rows.len() {
        let end = if r < head { head } else { rows.len() };
        for c in 0..rows[r].len() {
            if covered[r][c] {
                continue;
            }
            let (colspan, rowspan) = cell_span(&rows[r][c]);
            let mut w = 1;
            while w < colspan && free(&covered, r, c + w) {
                w += 1;
            }
            let mut h = 1;
            while h < rowspan && r + h < end && (c..c + w).all(|x| free(&covered, r + h, x)) {
                h += 1;
            }
            for row in &mut covered[r..r + h] {
                row[c..c + w].iter_mut().for_each(|slot| *slot = true);
            }
            spans[r][c] = Some((w, h));
        }
    }
    spans
}

/// A table cell's `align` and `valign`, each `None` where the cell holds no
/// value in its set.
pub fn cell_alignment(cell: &Value) -> (Option<&'static str>, Option<&'static str>) {
    let [(align, aligns), (valign, valigns)] = CELL_KEYS;
    (one_of(cell.get(align), aligns), one_of(cell.get(valign), valigns))
}

fn one_of(value: Option<&Value>, set: &[&'static str]) -> Option<&'static str> {
    let value = value?.as_str()?;
    set.iter().copied().find(|v| *v == value)
}

impl IslandType {
    /// Every known type, for a reader that needs the closed set whole.
    pub const ALL: &'static [IslandType] =
        &[IslandType::Table, IslandType::Image, IslandType::Footnote, IslandType::Math];

    /// The wire discriminator; `parse(k.as_str()) == Some(k)` for every variant.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Image => "image",
            Self::Footnote => "footnote",
            Self::Math => "math",
        }
    }

    /// Parse a wire discriminator; `parse(k.as_str()) == Some(k)` for every
    /// variant.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "table" => Some(Self::Table),
            "image" => Some(Self::Image),
            "footnote" => Some(Self::Footnote),
            "math" => Some(Self::Math),
            _ => None,
        }
    }

    /// Whether this type's markdown projection is a **block**: markup no
    /// paragraph line can hold, so its slot has to sit alone on its line. A
    /// pipe table is one; an image (`![alt](url)`), a footnote reference
    /// (`[^1]`) and an equation (`$x$`, `$$x$$`) are inline.
    pub fn block_only(self) -> bool {
        match self {
            Self::Table => true,
            Self::Image | Self::Footnote | Self::Math => false,
        }
    }

    /// This type's `(text, marks)` cells: the set that participates in mark
    /// normalization and cell-mark validation. Empty for a type with no cell
    /// model.
    pub fn cell_marks(self, props: &Value) -> Vec<(String, Vec<Mark>)> {
        match self {
            Self::Table => crate::serial::table_cells(props),
            Self::Footnote => vec![crate::serial::parse_cell(props)],
            Self::Image | Self::Math => Vec::new(),
        }
    }

    /// Refuse a cell mark whose `type` is outside the mark vocabulary. The
    /// decoder's arm of [`cell_marks`](Self::cell_marks): those read leniently,
    /// so this is the one place the row can be refused rather than silently
    /// re-encoded without the mark.
    pub fn reject_unknown_cell_mark(self, props: &Value) -> Result<(), crate::serial::ParseError> {
        match self {
            Self::Table => crate::serial::reject_unknown_cell_mark_name(crate::serial::table_cell_values(props)),
            Self::Footnote => crate::serial::reject_unknown_cell_mark_name([props]),
            Self::Image | Self::Math => Ok(()),
        }
    }

    /// Repair this type's props to canonical shape in place; a no-op for a type
    /// with no shape invariants.
    pub fn normalize_props(self, props: &mut Value) {
        match self {
            Self::Table => crate::serial::normalize_table_props(props),
            Self::Footnote => crate::serial::normalize_note_props(props),
            Self::Math => crate::serial::normalize_math_props(props),
            Self::Image => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_round_trip() {
        for &k in IslandType::ALL {
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
    /// never widens the table; each layout key is absent at its default, and
    /// one outside its set stays as written and reads as absent.
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
            (json!([1, 0, 1]), "widths", json!([1, 0, 1])),
            (json!([1, -1, 1]), "widths", json!([1, -1, 1])),
            (json!([1, 1.5, 1]), "widths", json!([1, 1.5, 1])),
            (json!([1, "2", 1]), "widths", json!([1, "2", 1])),
            (json!([9007199254740992u64, 1]), "widths", json!([9007199254740992u64, 1])),
            (json!([9007199254740991u64, 1]), "widths", json!([9007199254740991u64, 1, null])),
            (json!("1 2 3"), "widths", json!("1 2 3")),
            (json!(2), "widths", json!(2)),
            (json!("center"), "align", json!("center")),
            (json!("left"), "align", json!("left")),
            (json!("right"), "align", json!("right")),
            (json!("middle"), "align", json!("middle")),
            (json!(["center"]), "align", json!(["center"])),
            (json!(0), "head", json!(0)),
            (json!(1), "head", absent.clone()),
            (json!(2), "head", json!(2)),
            (json!(9), "head", json!(2)),
            (json!(-1), "head", json!(-1)),
            (json!("2"), "head", json!("2")),
            (json!(true), "headless", absent.clone()),
            (json!(false), "headless", absent.clone()),
            (json!("true"), "headless", json!("true")),
            (json!(1), "headless", json!(1)),
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

    #[test]
    fn the_layout_read_takes_each_key_only_in_its_set() {
        use serde_json::json;
        let rows = json!([["1"], ["2"]]);
        let read = TableLayout::of(&json!({"widths": [2, null], "align": "center", "head": 2, "rows": rows}));
        let layout = TableLayout { widths: Some(vec![Some(2), None]), align: Some("center"), head: 2 };
        assert_eq!(read, layout);
        let outside = json!({"widths": [2, 0], "align": "middle", "head": "2", "rows": rows});
        assert_eq!(TableLayout::of(&outside), TableLayout::default());
        let heads = [(json!({"head": 0}), 0), (json!({"head": 7}), 3)];
        for (keys, head) in heads {
            let mut props = json!({"header": ["h"], "rows": rows});
            props.as_object_mut().unwrap().extend(keys.as_object().unwrap().clone());
            assert_eq!(TableLayout::of(&props).head, head, "{keys}");
        }
    }

    /// A stored `headless: true` settles to `head: 0`, where no `head` is
    /// stored beside it.
    #[test]
    fn headless_settles_to_no_header_row() {
        use serde_json::json;
        let mut props = json!({"header": ["a"], "rows": [["1"]], "headless": true});
        IslandType::Table.normalize_props(&mut props);
        assert_eq!(props.get("head"), Some(&json!(0)));
        assert_eq!(props.get("headless"), None);
        let mut props = json!({"header": ["a"], "rows": [["1"]], "headless": true, "head": 2});
        IslandType::Table.normalize_props(&mut props);
        assert_eq!((props.get("head"), props.get("headless")), (Some(&json!(2)), None));
    }

    fn spanned(cells: &[&[(&str, usize, usize)]], head: usize) -> serde_json::Value {
        let cell = |&(text, colspan, rowspan): &(&str, usize, usize)| {
            serde_json::json!({"text": text, "marks": [], "colspan": colspan, "rowspan": rowspan})
        };
        let mut rows = cells.iter().map(|row| row.iter().map(cell).collect::<Vec<_>>());
        let header = rows.next().unwrap();
        serde_json::json!({"header": header, "rows": rows.collect::<Vec<_>>(), "head": head})
    }

    /// A span claims the empty cells to its right, then below, inside the grid
    /// and its section; one another span already covers, or one holding text,
    /// stops it, and the earlier cell in reading order claims first.
    #[test]
    fn a_span_covers_only_empty_cells_no_other_span_covers() {
        let cases: &[(&[&[(&str, usize, usize)]], usize, Vec<Vec<Option<(usize, usize)>>>)] = &[
            (&[&[("a", 2, 1), ("", 1, 1)], &[("b", 1, 1), ("c", 1, 1)]], 1, vec![vec![Some((2, 1)), None], vec![Some((1, 1)), Some((1, 1))]]),
            (&[&[("a", 3, 1), ("x", 1, 1), ("", 1, 1)]], 1, vec![vec![Some((1, 1)), Some((1, 1)), Some((1, 1))]]),
            (&[&[("a", 9, 9), ("", 1, 1)], &[("", 1, 1), ("", 1, 1)]], 1, vec![vec![Some((2, 1)), None], vec![Some((1, 1)), Some((1, 1))]]),
            (&[&[("a", 9, 9), ("", 1, 1)], &[("", 1, 1), ("", 1, 1)]], 0, vec![vec![Some((2, 2)), None], vec![None, None]]),
            (&[&[("a", 1, 2), ("b", 1, 1)], &[("", 2, 1), ("c", 1, 1)]], 0, vec![vec![Some((1, 2)), Some((1, 1))], vec![None, Some((1, 1))]]),
            (&[&[("a", 1, 1), ("b", 1, 2)], &[("c", 2, 1), ("", 1, 1)]], 0, vec![vec![Some((1, 1)), Some((1, 2))], vec![Some((1, 1)), None]]),
            (&[&[("a", 2, 2), ("", 1, 1)], &[("", 1, 1), ("d", 1, 1)]], 0, vec![vec![Some((2, 1)), None], vec![Some((1, 1)), Some((1, 1))]]),
        ];
        for (cells, head, spans) in cases {
            assert_eq!(&table_spans(&spanned(cells, *head)), spans, "{cells:?} head {head}");
        }
    }

    /// Normalization writes each span as the cell can cover it, absent at 1,
    /// and drops the span and alignment keys of a covered cell; a value outside
    /// its set rides as written.
    #[test]
    fn normalize_props_settles_each_span() {
        use serde_json::json;
        let mut props = json!({
            "header": [
                {"text": "a", "marks": [], "colspan": 5},
                {"text": "", "marks": [], "colspan": 2, "align": "right", "x_host": 1},
                {"text": "c", "marks": [], "colspan": 1, "rowspan": "2"},
            ],
            "rows": [[{"text": "d", "marks": [], "rowspan": 0}]],
        });
        IslandType::Table.normalize_props(&mut props);
        assert_eq!(props["header"][0], json!({"text": "a", "marks": [], "colspan": 2}));
        assert_eq!(props["header"][1], json!({"text": "", "marks": [], "x_host": 1}));
        assert_eq!(props["header"][2], json!({"text": "c", "marks": [], "rowspan": "2"}));
        assert_eq!(props["rows"][0][0], json!({"text": "d", "marks": [], "rowspan": 0}));
        let once = props.clone();
        IslandType::Table.normalize_props(&mut props);
        assert_eq!(props, once);
    }

    /// Normalization leaves a cell's `align` and `valign` as stored, an
    /// alignment equal to its column's and a value outside its set included,
    /// and the read takes only a value in its set.
    #[test]
    fn a_cells_alignment_rides_normalization_as_stored() {
        use serde_json::json;
        let keys = [
            (json!({"align": "right", "valign": "middle"}), (Some("right"), Some("middle"))),
            (json!({"align": "middle", "valign": "horizon"}), (None, None)),
            (json!({"align": ["left"], "valign": 1}), (None, None)),
        ];
        for (keys, read) in keys {
            let mut cell = json!({"text": "c", "marks": []});
            cell.as_object_mut().unwrap().extend(keys.as_object().unwrap().clone());
            let mut props = json!({"aligns": ["right"], "header": [cell.clone()], "rows": [[cell.clone()]]});
            IslandType::Table.normalize_props(&mut props);
            for at in ["/header/0", "/rows/0/0"] {
                assert_eq!(props.pointer(at), Some(&cell), "{keys} at {at}");
                assert_eq!(cell_alignment(props.pointer(at).unwrap()), read, "{keys} at {at}");
            }
        }
    }
}
