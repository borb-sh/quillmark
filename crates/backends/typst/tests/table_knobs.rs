//! Every table and cell knob lowers and compiles, and placing a table moves no
//! text inside it.

use quillmark_core::{
    backend::Backend,
    quill::Quill,
    types::{OutputFormat, RenderOptions},
};
use quillmark_typst::TypstBackend;

mod common;
use common::content;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 300pt, margin: 20pt)
#data.at("$body", default: [])
"#;

fn quill() -> Quill {
    common::quill_with_plate(&common::yaml("main:\n  fields: {}\n"), PLATE)
}

fn svg(quill: &Quill, markdown: &str) -> String {
    let data = serde_json::json!({ "$body": content(markdown) });
    let session = TypstBackend
        .open(quill, &data, common::test_date())
        .unwrap_or_else(|e| panic!("{markdown:?} compiles: {e}"));
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        .expect("render");
    String::from_utf8(result.artifacts[0].bytes.clone()).expect("svg is text")
}

/// Each text run's origin, its group's `matrix(1 0 0 -1 x y)`, in document
/// order.
fn origins(svg: &str) -> Vec<(f64, f64)> {
    svg.match_indices("<g transform=\"matrix(1 0 0 -1 ")
        .filter_map(|(at, m)| {
            let rest = &svg[at + m.len()..];
            let (x, rest) = rest.split_once(' ')?;
            let (y, _) = rest.split_once(')')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        })
        .collect()
}

const KNOBS: &str = "<quill-table align=\"center\" breakable=\"false\" widths=\"2 1\">\n\n\
                     | Item | Amount |\n| --- | --- |\n\
                     | Total | <quill-cell align=\"right\" valign=\"bottom\">42</quill-cell> |\n\n\
                     </quill-table>";

const PLAIN: &str = "| Item | Amount |\n| --- | --- |\n| Total | 42 |";

#[test]
fn every_knob_compiles_and_moves_the_table() {
    let quill = quill();
    assert_ne!(svg(&quill, KNOBS), svg(&quill, PLAIN), "the knobs move the table");
}

/// A centered table's short cell keeps its offset from the header above it: the
/// placement aligns the table, not the text in its cells, under the plate's own
/// cell alignment or none.
#[test]
fn placing_a_table_keeps_its_cells_aligned_as_they_were() {
    let table = "| A wide header cell |\n| --- |\n| x |";
    let placed = format!("<quill-table align=\"center\">\n\n{table}\n\n</quill-table>");
    for set in ["", "#set table(align: right)\n", "#set align(right)\n"] {
        let plate = PLATE.replace("#data", &format!("{set}#data"));
        let quill = common::quill_with_plate(
            &common::yaml("main:\n  fields: {}\n"),
            &plate,
        );
        let at = origins(&svg(&quill, &placed));
        let unplaced = origins(&svg(&quill, table));
        assert_eq!(at.len(), 2, "{at:?}");
        let offset = |o: &[(f64, f64)]| o[1].0 - o[0].0;
        assert!((offset(&at) - offset(&unplaced)).abs() < 1e-6, "under {set:?}: {at:?} {unplaced:?}");
        if set.is_empty() {
            assert!(at[0].0 > unplaced[0].0, "centered right of {unplaced:?}: {at:?}");
        }
    }
}

