//! Every table knob lowers and compiles, and placing a table moves no
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

const PLAIN: &str = "| Item | Amount |\n| --- | --- |\n| Total | 42 |";

/// Each knob alone compiles and moves the table's text.
#[test]
fn every_knob_compiles_and_moves_the_table() {
    let quill = quill();
    let plain = origins(&svg(&quill, PLAIN));
    for attrs in ["align=\"center\"", "widths=\"2 1\"", "align=\"center\" widths=\"2 1\""] {
        let knobs = format!("<qm-table {attrs}>\n\n{PLAIN}\n\n</qm-table>");
        let moved = origins(&svg(&quill, &knobs));
        assert_eq!(moved.len(), plain.len(), "{attrs}: {moved:?}");
        assert_ne!(moved, plain, "{attrs} moves the table");
    }
}

/// A centered table's short cell keeps its offset from the header above it: the
/// placement aligns the table, not the text in its cells, under the plate's own
/// cell alignment or none.
#[test]
fn placing_a_table_keeps_its_cells_aligned_as_they_were() {
    let table = "| A wide header cell |\n| --- |\n| x |";
    let placed = format!("<qm-table align=\"center\">\n\n{table}\n\n</qm-table>");
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


/// A plate styling the header row through `table.header` styles a headed
/// table's first row and leaves a headless table's alone.
#[test]
fn a_headless_table_has_no_header_row_to_style() {
    let plate = PLATE.replace(
        "#data",
        "#show table: it => if it.children.any(c => c.func() == table.header) {\n  \
         show table.cell.where(y: 0): set text(fill: rgb(\"#ff0000\"))\n  it\n} else { it }\n#data",
    );
    let quill = common::quill_with_plate(&common::yaml("main:\n  fields: {}\n"), &plate);
    let styled = |md: &str| svg(&quill, md).matches("#ff0000").count();
    assert!(styled(PLAIN) > 0);
    assert_eq!(styled(&format!("<qm-table headless>\n\n{PLAIN}\n\n</qm-table>")), 0);
}
