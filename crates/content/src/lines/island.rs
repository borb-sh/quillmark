//! The line form's readers of an island's props, by
//! [`Island::island_type`](crate::lines::model::Island::island_type).

use crate::lines::model::Mark;
use crate::model::IslandType;
use serde_json::Value;

impl IslandType {
    /// This type's `(text, marks)` cells: the set that participates in mark
    /// normalization and cell-mark validation. Empty for a type with no cell
    /// model.
    pub fn cell_marks(&self, props: &Value) -> Vec<(String, Vec<Mark>)> {
        match self {
            Self::Table => crate::lines::serial::table_cells(props),
            Self::Footnote => vec![crate::lines::serial::parse_cell(props)],
            Self::Ext(_) => ext_cells(props)
                .map(crate::lines::serial::parse_cell)
                .collect(),
            Self::Image | Self::Math => Vec::new(),
        }
    }

    /// Refuse a cell mark whose `type` is outside the mark vocabulary. The
    /// decoder's arm of [`cell_marks`](Self::cell_marks): those read leniently,
    /// so this is the one place the row can be refused rather than silently
    /// re-encoded without the mark.
    pub fn reject_unknown_cell_mark(
        &self,
        props: &Value,
    ) -> Result<(), crate::lines::serial::ParseError> {
        match self {
            Self::Table => crate::lines::serial::reject_unknown_cell_mark_name(
                crate::lines::serial::table_cell_values(props),
            ),
            Self::Footnote => crate::lines::serial::reject_unknown_cell_mark_name([props]),
            Self::Ext(_) => crate::lines::serial::reject_unknown_cell_mark_name(ext_cells(props)),
            Self::Image | Self::Math => Ok(()),
        }
    }

    /// Repair this type's props to canonical shape in place; a no-op for a type
    /// with no shape invariants.
    pub fn normalize_props(&self, props: &mut Value) {
        match self {
            Self::Table => crate::lines::serial::normalize_table_props(props),
            Self::Footnote => crate::lines::serial::normalize_note_props(props),
            Self::Math => crate::model::normalize_math_props(props),
            Self::Image | Self::Ext(_) => {}
        }
    }
}

/// An [`Ext`](IslandType::Ext) island's cells, in slot id order.
fn ext_cells(props: &Value) -> impl Iterator<Item = &Value> {
    props
        .get("slots")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|cells| cells.values())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{TableLayout, cell_alignment, table_spans};

    #[test]
    fn known_types_round_trip() {
        for k in IslandType::ALL {
            assert_eq!(IslandType::parse(k.as_str()).as_ref(), Some(k));
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
        use serde_json::{Value, json};
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
            (
                json!([9007199254740992u64, 1]),
                "widths",
                json!([9007199254740992u64, 1]),
            ),
            (
                json!([9007199254740991u64, 1]),
                "widths",
                json!([9007199254740991u64, 1, null]),
            ),
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
            (json!(true), "headless", json!(true)),
            (json!(false), "headless", absent.clone()),
            (json!("true"), "headless", json!("true")),
            (json!(1), "headless", json!(1)),
        ];
        for (value, key, settled) in cases {
            let mut props = json!({"header": ["a", "b", "c"], "rows": [["1", "2", "3"]]});
            props[*key] = value.clone();
            IslandType::Table.normalize_props(&mut props);
            assert_eq!(
                props.get(*key).unwrap_or(&absent),
                settled,
                "{key}: {value}"
            );
            assert_eq!(
                props["header"].as_array().unwrap().len(),
                3,
                "{key}: {value}"
            );

            let once = props.clone();
            IslandType::Table.normalize_props(&mut props);
            assert_eq!(props, once, "{key}: {value} is not a fixed point");
        }
    }

    #[test]
    fn the_layout_read_takes_each_key_only_in_its_set() {
        use serde_json::json;
        let rows = json!([["1"], ["2"]]);
        let read = TableLayout::of(
            &json!({"widths": [2, null], "align": "center", "head": 2, "rows": rows}),
        );
        let layout = TableLayout {
            widths: Some(vec![Some(2), None]),
            align: Some("center"),
            head: 2,
        };
        assert_eq!(read, layout);
        let outside = json!({"widths": [2, 0], "align": "middle", "head": "2", "rows": rows});
        assert_eq!(TableLayout::of(&outside), TableLayout::default());
        let heads = [(json!({"head": 0}), 0), (json!({"head": 7}), 3)];
        for (keys, head) in heads {
            let mut props = json!({"header": ["h"], "rows": rows});
            props
                .as_object_mut()
                .unwrap()
                .extend(keys.as_object().unwrap().clone());
            assert_eq!(TableLayout::of(&props).head, head, "{keys}");
        }
    }

    /// `head: 0` carries `headless: true` beside it, the key a 0.130 reader
    /// draws no header row by, and a stored `headless: true` settles to
    /// `head: 0` where `head` holds no whole number.
    #[test]
    fn headless_rides_beside_no_header_row() {
        use serde_json::json;
        let cases = [
            (json!({"headless": true}), (json!(0), json!(true))),
            (json!({"head": 0}), (json!(0), json!(true))),
            (
                json!({"head": 0, "headless": "yes"}),
                (json!(0), json!(true)),
            ),
            (
                json!({"headless": true, "head": "x"}),
                (json!(0), json!(true)),
            ),
            (
                json!({"headless": true, "head": 2}),
                (json!(2), Value::Null),
            ),
            (
                json!({"headless": true, "head": 1}),
                (Value::Null, Value::Null),
            ),
            (
                json!({"headless": false, "head": 0}),
                (json!(0), json!(true)),
            ),
            (json!({"headless": "yes"}), (Value::Null, json!("yes"))),
        ];
        for (keys, (head, headless)) in cases {
            let mut props = json!({"header": ["a"], "rows": [["1"], ["2"]]});
            props
                .as_object_mut()
                .unwrap()
                .extend(keys.as_object().unwrap().clone());
            IslandType::Table.normalize_props(&mut props);
            let got = (
                props.get("head").cloned().unwrap_or(Value::Null),
                props.get("headless").cloned().unwrap_or(Value::Null),
            );
            assert_eq!(got, (head.clone(), headless.clone()), "{keys}");
            let once = props.clone();
            IslandType::Table.normalize_props(&mut props);
            assert_eq!(props, once, "{keys} is not a fixed point");
        }
    }

    fn spanned(cells: &[&[(&str, usize, usize)]], head: usize) -> serde_json::Value {
        let cell = |&(text, colspan, rowspan): &(&str, usize, usize)| serde_json::json!({"text": text, "marks": [], "colspan": colspan, "rowspan": rowspan});
        let mut rows = cells
            .iter()
            .map(|row| row.iter().map(cell).collect::<Vec<_>>());
        let header = rows.next().unwrap();
        serde_json::json!({"header": header, "rows": rows.collect::<Vec<_>>(), "head": head})
    }

    /// A span claims the empty cells to its right, then below, inside the grid
    /// and its section; one another span already covers, or one holding text,
    /// stops it, and the earlier cell in reading order claims first.
    #[test]
    fn a_span_covers_only_empty_cells_no_other_span_covers() {
        let cases: &[(
            &[&[(&str, usize, usize)]],
            usize,
            Vec<Vec<Option<(usize, usize)>>>,
        )] = &[
            (
                &[&[("a", 2, 1), ("", 1, 1)], &[("b", 1, 1), ("c", 1, 1)]],
                1,
                vec![vec![Some((2, 1)), None], vec![Some((1, 1)), Some((1, 1))]],
            ),
            (
                &[&[("a", 3, 1), ("x", 1, 1), ("", 1, 1)]],
                1,
                vec![vec![Some((1, 1)), Some((1, 1)), Some((1, 1))]],
            ),
            (
                &[&[("a", 9, 9), ("", 1, 1)], &[("", 1, 1), ("", 1, 1)]],
                1,
                vec![vec![Some((2, 1)), None], vec![Some((1, 1)), Some((1, 1))]],
            ),
            (
                &[&[("a", 9, 9), ("", 1, 1)], &[("", 1, 1), ("", 1, 1)]],
                0,
                vec![vec![Some((2, 2)), None], vec![None, None]],
            ),
            (
                &[&[("a", 1, 2), ("b", 1, 1)], &[("", 2, 1), ("c", 1, 1)]],
                0,
                vec![vec![Some((1, 2)), Some((1, 1))], vec![None, Some((1, 1))]],
            ),
            (
                &[&[("a", 1, 1), ("b", 1, 2)], &[("c", 2, 1), ("", 1, 1)]],
                0,
                vec![vec![Some((1, 1)), Some((1, 2))], vec![Some((1, 1)), None]],
            ),
            (
                &[&[("a", 2, 2), ("", 1, 1)], &[("", 1, 1), ("d", 1, 1)]],
                0,
                vec![vec![Some((2, 1)), None], vec![Some((1, 1)), Some((1, 1))]],
            ),
        ];
        for (cells, head, spans) in cases {
            assert_eq!(
                &table_spans(&spanned(cells, *head)),
                spans,
                "{cells:?} head {head}"
            );
        }
    }

    /// Normalization writes each span as the cell can cover it, absent at 1,
    /// and drops the span keys of a covered cell, which keeps its alignment; a
    /// value outside its set rides as written.
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
        assert_eq!(
            props["header"][0],
            json!({"text": "a", "marks": [], "colspan": 2})
        );
        assert_eq!(
            props["header"][1],
            json!({"text": "", "marks": [], "align": "right", "x_host": 1})
        );
        assert_eq!(
            props["header"][2],
            json!({"text": "c", "marks": [], "rowspan": "2"})
        );
        assert_eq!(
            props["rows"][0][0],
            json!({"text": "d", "marks": [], "rowspan": 0})
        );
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
            (
                json!({"align": "right", "valign": "middle"}),
                (Some("right"), Some("middle")),
            ),
            (
                json!({"align": "middle", "valign": "horizon"}),
                (None, None),
            ),
            (json!({"align": ["left"], "valign": 1}), (None, None)),
        ];
        for (keys, read) in keys {
            let mut cell = json!({"text": "c", "marks": []});
            cell.as_object_mut()
                .unwrap()
                .extend(keys.as_object().unwrap().clone());
            let mut props =
                json!({"aligns": ["right"], "header": [cell.clone()], "rows": [[cell.clone()]]});
            IslandType::Table.normalize_props(&mut props);
            for at in ["/header/0", "/rows/0/0"] {
                assert_eq!(props.pointer(at), Some(&cell), "{keys} at {at}");
                assert_eq!(
                    cell_alignment(props.pointer(at).unwrap()),
                    read,
                    "{keys} at {at}"
                );
            }
        }
    }

    /// A covered cell's alignment comes back with it once the span over it
    /// shrinks.
    #[test]
    fn a_covered_cells_alignment_survives_the_span_shrinking() {
        use serde_json::json;
        let mut props = json!({
            "header": [
                {"text": "a", "marks": [], "colspan": 2},
                {"text": "", "marks": [], "align": "right", "valign": "top"},
            ],
            "rows": [],
        });
        IslandType::Table.normalize_props(&mut props);
        assert_eq!(table_spans(&props), vec![vec![Some((2, 1)), None]]);
        props["header"][1]["text"] = json!("b");
        IslandType::Table.normalize_props(&mut props);
        assert_eq!(table_spans(&props), vec![vec![Some((1, 1)), Some((1, 1))]]);
        assert_eq!(
            cell_alignment(&props["header"][1]),
            (Some("right"), Some("top"))
        );
    }

    /// A cell holding a mark, a comment's zero-width anchor included, is no
    /// placeholder a span can cover.
    #[test]
    fn a_span_stops_at_a_cell_holding_an_anchor() {
        use serde_json::json;
        let anchored =
            json!({"text": "", "marks": [{"start": 0, "end": 0, "type": "anchor", "id": "c1"}]});
        let mut props =
            json!({"header": [{"text": "a", "marks": [], "colspan": 2}, anchored], "rows": []});
        IslandType::Table.normalize_props(&mut props);
        assert_eq!(table_spans(&props), vec![vec![Some((1, 1)), Some((1, 1))]]);
        assert_eq!(props["header"][0].get("colspan"), None);
    }
}
