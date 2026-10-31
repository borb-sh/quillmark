//! A table's two spellings over generated grids: header rows, spans,
//! alignments and layout keys over cells holding markup characters, written
//! by the export and read back by the import as the grid normalization
//! settles, but for a covered cell's alignment, which no spelling holds, in
//! pipes exactly where nothing needs HTML. Separately, generated
//! HTML around the subset's tags imports without a panic to a content that
//! is the fixed point of a re-import.

use proptest::prelude::*;
use serde_json::{json, Value};

use crate::lines::export::to_markdown;
use crate::lines::html_table::writes_html;
use crate::lines::import::from_markdown;
use crate::lines::island::IslandType;
use crate::lines::model::{Content, Island, Line, LineKind, ISLAND_SLOT};

/// A cell: literal text over characters the HTML and pipe spellings both
/// escape, or a word under one mark.
fn cell() -> impl Strategy<Value = Value> {
    let literal = r"[a-z0-9 <>/&|`*_\\$\[\]#!:;=\-\n]{0,8}".prop_map(|t| json!({"text": t, "marks": []}));
    let marked = ("[a-z]{1,5}", 0..5u8).prop_map(|(w, k)| {
        let mark = match k {
            0 => json!({"type": "strong"}),
            1 => json!({"type": "emph"}),
            2 => json!({"type": "code"}),
            3 => json!({"type": "underline"}),
            _ => json!({"type": "link", "attrs": {"url": format!("td {w}")}}),
        };
        let mut mark = mark;
        mark["start"] = 2.into();
        mark["end"] = (2 + w.len()).into();
        json!({"text": format!("a {w} b"), "marks": [mark]})
    });
    let keys = (
        prop::option::weighted(0.3, 2usize..4),
        prop::option::weighted(0.3, 2usize..4),
        prop::option::weighted(0.2, prop::sample::select(vec!["left", "center", "right"])),
        prop::option::weighted(0.2, prop::sample::select(vec!["top", "middle", "bottom"])),
    );
    (prop_oneof![2 => literal, 1 => marked, 1 => Just(json!({"text": "", "marks": []}))], keys).prop_map(
        |(mut cell, (colspan, rowspan, align, valign))| {
            let o = cell.as_object_mut().unwrap();
            for (key, v) in [("colspan", colspan.map(Value::from)), ("rowspan", rowspan.map(Value::from))] {
                if let Some(v) = v {
                    o.insert(key.into(), v);
                }
            }
            for (key, v) in [("align", align), ("valign", valign)] {
                if let Some(v) = v {
                    o.insert(key.into(), v.into());
                }
            }
            cell
        },
    )
}

fn table() -> impl Strategy<Value = Content> {
    (1usize..4, 1usize..5).prop_flat_map(|(cols, rows)| {
        (
            prop::collection::vec(prop::collection::vec(cell(), cols), rows),
            prop::collection::vec(prop::sample::select(vec!["none", "left", "center", "right"]), cols),
            prop::option::weighted(0.4, 0..=rows),
            prop::option::weighted(0.2, prop::collection::vec(prop::option::of(1u64..9), cols)),
            prop::option::weighted(0.2, prop::sample::select(vec!["left", "center", "right"])),
        )
            .prop_map(|(grid, aligns, head, widths, align)| {
                let mut grid = grid.into_iter();
                let mut props = json!({"header": grid.next().unwrap(), "rows": grid.collect::<Vec<_>>(), "aligns": aligns});
                if let Some(head) = head {
                    props["head"] = head.into();
                }
                if let Some(widths) = widths {
                    props["widths"] = widths.into_iter().map(|w| w.map_or(Value::Null, Value::from)).collect();
                }
                if let Some(align) = align {
                    props["align"] = align.into();
                }
                let island = Island::new("isl-0".into(), IslandType::Table).with_props(props);
                Content::new(ISLAND_SLOT.to_string(), vec![Line::new(LineKind::Para)]).with_islands(vec![island])
            })
    })
}

/// Text around the subset's tags, as a writer or a model might put it.
fn soup() -> impl Strategy<Value = String> {
    let token = prop::sample::select(vec![
        "<table>", "<table widths=\"1 2\" aligns=\"left right\">", "</table>", "<thead>", "</thead>", "<tbody>",
        "</tbody>", "<tfoot>", "<tr>", "</tr>", "<td>", "<td colspan=2>", "<td rowspan=\"3\" align=right>",
        "<th>", "<th colspan=9>", "</td>", "</th>", "<caption>", "<col>", "x", "yz", " ", "\n", "`",
        "\\", "<!--", "-->", "](<", ">", "<br>", "<u>", "</u>", "|", "&amp;", "<qm-keep>", "$", "[^1]",
    ]);
    prop::collection::vec(token, 0..40).prop_map(|t| format!("<table>{}", t.concat()))
}

/// `rt` with each covered cell's `align` and `valign` gone.
fn without_covered_alignment(rt: &crate::lines::model::Normalized) -> crate::lines::model::Normalized {
    let mut content = rt.clone().into_content();
    for island in &mut content.islands {
        let spans = crate::lines::island::table_spans(&island.props);
        for (r, row) in spans.iter().enumerate() {
            for c in (0..row.len()).filter(|c| row[*c].is_none()) {
                let at = if r == 0 { format!("/header/{c}") } else { format!("/rows/{}/{c}", r - 1) };
                if let Some(cell) = island.props.pointer_mut(&at).and_then(Value::as_object_mut) {
                    for (key, _) in crate::lines::island::CELL_KEYS {
                        cell.remove(key);
                    }
                }
            }
        }
    }
    content.into_normalized()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_table_reads_back_from_the_spelling_its_storage_picks(table in table()) {
        let rt = table.into_normalized();
        prop_assert_eq!(rt.validate(), Ok(()));
        let md = to_markdown(&rt);
        prop_assert_eq!(md.starts_with("<table"), writes_html(&rt.islands[0].props), "{}", md);
        let back = from_markdown(&md).unwrap();
        prop_assert!(back.warnings.is_empty(), "{:?}\n{}", back.warnings, md);
        prop_assert_eq!(&back.content, &without_covered_alignment(&rt), "{}", md);
    }

    /// The prose a table's block sets apart after its `</table>` reads as
    /// markdown, where a mark across a hard break re-exports split in two, so
    /// the tables and the text are what read back.
    #[test]
    fn html_soup_imports_to_a_fixed_point(md in soup()) {
        let imported = from_markdown(&md).unwrap();
        prop_assert_eq!(imported.content.validate(), Ok(()), "{:?}", md);
        let out = to_markdown(&imported.content);
        let back = from_markdown(&out).unwrap();
        prop_assert!(back.warnings.is_empty(), "{:?}\n{:?}\n{}", back.warnings, md, out);
        prop_assert_eq!(&back.content.islands, &imported.content.islands, "{:?}\n{}", md, out);
        prop_assert_eq!(&back.content.text, &imported.content.text, "{:?}\n{}", md, out);
    }
}
