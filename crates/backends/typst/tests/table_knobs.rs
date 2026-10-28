//! Every table knob lowers and compiles, placing a table moves no text
//! inside it, and a merged cell draws once across the slots it covers.

use quillmark_core::{
    backend::Backend,
    quill::Quill,
    types::{OutputFormat, RenderOptions},
};
use quillmark_typst::TypstBackend;

mod common;
use common::content;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 300pt, margin: 20pt)
#qm.data.at("$body", default: [])
"#;

fn quill() -> Quill {
    common::quill_with_plate(&common::yaml("main:\n  fields: {}\n"), PLATE)
}

fn svg(quill: &Quill, markdown: &str) -> String {
    common::body_svgs(quill, markdown).0.swap_remove(0)
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

/// [`PLAIN`] as an HTML table, its `<table>` tag carrying `attrs`.
fn html(attrs: &str) -> String {
    format!("<table{attrs}>\n<thead>\n<tr><th>Item</th><th>Amount</th></tr>\n</thead>\n<tbody>\n<tr><td>Total</td><td>42</td></tr>\n</tbody>\n</table>")
}

/// Each knob alone compiles and moves the table's text.
#[test]
fn every_knob_compiles_and_moves_the_table() {
    let quill = quill();
    let plain = origins(&svg(&quill, PLAIN));
    assert_eq!(origins(&svg(&quill, &html(""))), plain);
    for attrs in [" align=\"center\"", " widths=\"2 1\"", " align=\"center\" widths=\"2 1\""] {
        let knobs = html(attrs);
        let moved = origins(&svg(&quill, &knobs));
        assert_eq!(moved.len(), plain.len(), "{attrs}: {moved:?}");
        assert_ne!(moved, plain, "{attrs} moves the table");
    }
}

/// A centered table's cells keep their offsets from the header's first cell:
/// the placement aligns the table, not the text in its cells, under the
/// plate's table alignment, its vertical cell alignment, its own alignment or
/// none, with a column aligned or none.
#[test]
fn placing_a_table_keeps_its_cells_aligned_as_they_were() {
    let tables = [
        ("| A wide header cell |\n| --- |\n| x |", "<tr><th>A wide header cell</th></tr>\n<tr><td>x</td></tr>", ""),
        (
            "| A wide header cell | b |\n| --- | ---: |\n| x | y |",
            "<tr><th>A wide header cell</th><th>b</th></tr>\n<tr><td>x</td><td>y</td></tr>",
            " aligns=\"none right\"",
        ),
        (
            "| A wide header cell | b |\n| --- | ---: |\n| two<br>lines | y |",
            "<tr><th>A wide header cell</th><th>b</th></tr>\n<tr><td>two<br>lines</td><td>y</td></tr>",
            " aligns=\"none right\"",
        ),
    ];
    let sets = ["", "#set table(align: right)\n", "#set align(right)\n", "#show table.cell: set align(horizon)\n"];
    for (table, rows, aligns) in tables {
        let placed = format!("<table align=\"center\"{aligns}>\n{rows}\n</table>");
        for set in sets {
            let plate = PLATE.replace("#qm.data", &format!("{set}#qm.data"));
            let quill = common::quill_with_plate(&common::yaml("main:\n  fields: {}\n"), &plate);
            let at = origins(&svg(&quill, &placed));
            let unplaced = origins(&svg(&quill, table));
            let offsets = |o: &[(f64, f64)]| -> Vec<(f64, f64)> { o.iter().map(|p| (p.0 - o[0].0, p.1 - o[0].1)).collect() };
            let kept = at.len() == unplaced.len()
                && offsets(&at)
                    .iter()
                    .zip(offsets(&unplaced))
                    .all(|(a, b)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
            assert!(kept, "{table:?} under {set:?}: {at:?} {unplaced:?}");
            if set.is_empty() {
                assert!(at[0].0 > unplaced[0].0, "centered right of {unplaced:?}: {at:?}");
            }
        }
    }
}

/// A plate styling the header row through `table.header` styles a headed
/// table's first row and leaves a headless table's alone.
#[test]
fn a_headless_table_has_no_header_row_to_style() {
    let plate = PLATE.replace(
        "#qm.data",
        "#show table: it => if it.children.any(c => c.func() == table.header) {\n  \
         show table.cell.where(y: 0): set text(fill: rgb(\"#ff0000\"))\n  it\n} else { it }\n#qm.data",
    );
    let quill = common::quill_with_plate(&common::yaml("main:\n  fields: {}\n"), &plate);
    let styled = |md: &str| svg(&quill, md).matches("#ff0000").count();
    assert!(styled(PLAIN) > 0);
    let headless = "<table>\n<tbody>\n<tr><td>Item</td><td>Amount</td></tr>\n<tr><td>Total</td><td>42</td></tr>\n</tbody>\n</table>";
    assert_eq!(styled(headless), 0);
}

/// A cell spanning two columns draws its text once, centered across both, and
/// one spanning two rows once, between them; the covered slots draw nothing.
#[test]
fn a_merged_cell_draws_once_across_what_it_covers() {
    let quill = quill();
    let spanned = |md: &str| origins(&svg(&quill, md));
    let row = spanned("<table>\n<tr><td>left</td><td>right</td></tr>\n<tr><td colspan=\"2\" align=\"center\">both</td></tr>\n</table>");
    assert_eq!(row.len(), 3, "{row:?}");
    assert!(row[2].0 > row[0].0 && row[2].0 < row[1].0, "{row:?}");
    let mut column = spanned("<table>\n<tr><td rowspan=\"2\" valign=\"middle\">both</td><td>top</td></tr>\n<tr><td>bottom</td></tr>\n</table>");
    assert_eq!(column.len(), 3, "{column:?}");
    column.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let [both, top, bottom] = column[..] else { unreachable!() };
    assert!(both.0 < top.0 && top.1 < both.1 && both.1 < bottom.1, "{column:?}");
}

/// Two `thead` rows, one cell spanning both, lower as one `table.header`,
/// which repeats on each page the table runs onto.
#[test]
fn every_head_row_repeats_with_the_header() {
    let quill = quill();
    let body: String = (0..60).map(|i| format!("<tr><td>row {i}</td><td>x</td></tr>\n")).collect();
    let md = format!(
        "<table>\n<thead>\n<tr><th rowspan=\"2\">first</th><th>head</th></tr>\n<tr><th>second</th></tr>\n</thead>\n{body}</table>"
    );
    let data = serde_json::json!({ "$body": content(&md) });
    let session = TypstBackend::default().open(&quill, &data, common::test_date()).expect("compiles");
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        .expect("render");
    assert!(result.artifacts.len() > 1, "the table fits one page");
    let heads = |page: &[u8]| String::from_utf8_lossy(page).matches("<g transform=\"matrix(1 0 0 -1 ").count();
    let pages: Vec<usize> = result.artifacts.iter().map(|page| heads(&page.bytes)).collect();
    let cells: usize = pages.iter().sum::<usize>() - 3 * pages.len();
    assert_eq!(cells, 120, "each page draws the three head cells above its body rows: {pages:?}");
}

/// A table sharing its line with prose, the shape a merge of concurrent edits
/// leaves, compiles and draws what the line broken around it draws: Typst sets
/// a table as a block wherever the markup places it.
#[test]
fn a_table_sharing_its_line_draws_as_the_broken_line_does() {
    let quill = quill();
    let draw = |text: &str, lines: usize| {
        let mut value = content(&format!("before\n\n{PLAIN}\n\nafter"));
        value["text"] = text.into();
        value["lines"] = serde_json::Value::Array(vec![serde_json::json!({"containers": [], "kind": "para"}); lines]);
        let session = TypstBackend::default()
            .open(&quill, &serde_json::json!({ "$body": value }), common::test_date())
            .unwrap_or_else(|e| panic!("{text:?} compiles: {e}"));
        let result = session
            .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
            .expect("render");
        origins(&String::from_utf8(result.artifacts[0].bytes.clone()).expect("svg is text"))
    };
    let broken = draw("before\n\u{FFFC}\nafter", 3);
    let shared = draw("before\u{FFFC}after", 1);
    assert_eq!(shared.len(), broken.len(), "{shared:?}");
    assert_eq!(shared.iter().map(|o| o.0).collect::<Vec<_>>(), broken.iter().map(|o| o.0).collect::<Vec<_>>());
}
