//! The line form's own properties: its mint's fixed point, its canonical
//! serialization, its decoder refusing rather than panicking, and the
//! migration between it and the content model.

mod generate;

use generate::{alignment, cell_content, decode_json, document, fixture_body};
use proptest::prelude::*;
use quillmark_content::lines::model::{Content, Island, Line, LineKind, Lines, Mark, MarkKind};
use quillmark_content::markdown;
use quillmark_content::migrate::{from_lines, to_lines};
use quillmark_content::model::{Content as V2, IslandType};
use serde_json::{Value, json};

fn from_markdown(md: &str) -> Lines {
    to_lines(&markdown::from_markdown(md).unwrap().content)
}

/// The mint's fixed point: normalizing again changes nothing.
///
/// Every repair [`Content::normalize`] performs is a strict rewrite — a kind
/// demoted, a flag cleared, a mark split at a line break, a zero-width mark
/// dropped, a row padded — so one
/// equality states the whole canonical form. Paired with `validate`, which
/// reports what the mint cannot repair, it is the oracle over an accepted op:
/// the two together cover the content's invariants.
fn renormalized(rt: &Lines) -> Content {
    let mut again = rt.clone().into_content();
    again.normalize();
    again
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Property 2a: the *byte* fixed point and the *value* fixed point. Bytes
    /// can hold while a value that encodes to some other value's bytes loses
    /// the second.
    #[test]
    fn canonical_json_fixed_point(md in document()) {
        let rt = from_markdown(&md);
        let json = rt.to_canonical_json();
        let back = Content::from_canonical_json(&json).unwrap();
        prop_assert_eq!(back.to_canonical_json(), json);
        prop_assert_eq!(back, rt);
    }

    /// Property 2b: canonical bytes are insensitive to mark *discovery* order.
    /// Islands are ordered by slot position, so only mark order is free.
    #[test]
    fn canonical_json_order_insensitive(md in document()) {
        let rt = from_markdown(&md);
        let mut shuffled = rt.clone().into_content();
        shuffled.marks.reverse();
        prop_assert_eq!(
            rt.to_canonical_json(),
            shuffled.into_lines().to_canonical_json()
        );
    }
}

/// The canonical `{text, marks}` cells for a row of markdown contents, obtained
/// by importing a header-only table, so each cell is representable and its
/// marks are canonical by construction. `contents` must be non-empty.
fn import_row(contents: &[String]) -> Vec<Value> {
    let cols = contents.len();
    let header = contents.join(" | ");
    let delim = vec!["---"; cols].join(" | ");
    let body = vec!["x"; cols].join(" | ");
    let md = format!("| {header} |\n| {delim} |\n| {body} |");
    let rt = from_markdown(&md);
    rt.islands[0].props["header"].as_array().unwrap().clone()
}

/// A single-table content with the given (possibly ill-shaped) props. `id` is the
/// first-island id import mints (`isl-0`), so a re-imported table compares equal.
fn table_content(aligns: Vec<&str>, header: Vec<Value>, rows: Vec<Vec<Value>>) -> Content {
    Content::new("\u{FFFC}".into(), vec![Line::new(LineKind::Para)]).with_islands(vec![
        Island::new("isl-0".into(), IslandType::Table)
            .with_props(json!({ "aligns": aligns, "header": header, "rows": rows })),
    ])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// An ill-shaped table island normalizes to a valid content that is a fixed
    /// point of export∘import.
    #[test]
    fn table_island_normalizes_and_round_trips(
        header in prop::collection::vec(cell_content(), 0..4),
        rows in prop::collection::vec(prop::collection::vec(cell_content(), 0..4), 0..4),
        aligns in prop::collection::vec(alignment(), 0..5),
    ) {
        let header_cells = if header.is_empty() { vec![] } else { import_row(&header) };
        let row_cells: Vec<Vec<Value>> = rows
            .iter()
            .map(|r| if r.is_empty() { vec![] } else { import_row(r) })
            .collect();

        // The widest column count drives normalization. A fully empty table has
        // no markdown projection, so it is outside the fixed-point contract.
        let cols = header_cells.len()
            .max(aligns.len())
            .max(row_cells.iter().map(Vec::len).max().unwrap_or(0));
        prop_assume!(cols >= 1);

        let rt = table_content(aligns.clone(), header_cells, row_cells).into_lines();
        prop_assert_eq!(rt.validate(), Ok(()), "normalized table invalid");
        prop_assert_eq!(&renormalized(&rt), &*rt, "the table mint is not a fixed point");

        let props = &rt.islands[0].props;
        prop_assert_eq!(props["header"].as_array().unwrap().len(), cols);
        prop_assert_eq!(props["aligns"].as_array().unwrap().len(), cols);
        for row in props["rows"].as_array().unwrap() {
            prop_assert_eq!(row.as_array().unwrap().len(), cols);
        }

        let c = quillmark_content::migrate::from_lines(&rt);
        let md = quillmark_content::markdown::to_markdown(&c);
        let back = markdown::from_markdown(&md).unwrap().content;
        prop_assert_eq!(&c, &back, "table not a fixed point.\n  md: {:?}", md);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// The `install` lane.
    #[test]
    fn canonical_decode_never_panics(v in decode_json()) {
        let _ = quillmark_content::lines::serial::from_canonical_value(&v);
    }

    /// A line's containers inside an envelope that decodes up to them.
    #[test]
    fn container_decode_never_panics(containers in prop::collection::vec(decode_json(), 0..4)) {
        let content = serde_json::json!({
            "text": "a",
            "lines": [{"kind": "para", "containers": containers}],
            "marks": [],
            "islands": [],
        });
        let _ = quillmark_content::lines::serial::from_canonical_value(&content);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn migration_lowers_back_to_its_source(md in document()) {
        let rt = from_markdown(&md);
        let v2 = from_lines(&rt);
        prop_assert_eq!(v2.validate(), Ok(()), "invariants for {:?}", md);
        prop_assert_eq!(&from_lines(&rt), &v2);
        prop_assert_eq!(&to_lines(&v2), &rt, "lowering differs for {:?}", md);
        prop_assert_eq!(from_lines(&to_lines(&v2)), v2);
    }

    #[test]
    fn a_v1_value_decodes_as_its_migration(md in document()) {
        let rt = from_markdown(&md);
        let v1 = rt.to_canonical_json();
            prop_assert_eq!(V2::from_json(&v1).unwrap(), from_lines(&rt));
    }
}

/// Line-form shapes the model refuses migrate to the nearest it holds: a
/// table sharing its line takes a block of its own, and an anchor id held
/// twice keeps its first holder.
#[test]
fn a_shape_the_model_refuses_migrates_valid() {
    let mut rt = from_markdown("before\n\n| a |\n|---|\n| b |\n\nafter").into_content();
    rt.text = "before\u{FFFC}after".into();
    rt.lines.truncate(1);
    rt.marks
        .push(Mark::new(0, 6, MarkKind::Anchor { id: "c1".into() }));
    rt.marks
        .push(Mark::new(7, 12, MarkKind::Anchor { id: "c1".into() }));
    let v2 = from_lines(&rt.into_lines());
    assert_eq!(v2.validate(), Ok(()));
    assert_eq!(v2.text, "before\n\u{FFFC}\nafter");
    assert_eq!(v2.anchors.len(), 1);
    assert_eq!((v2.anchors[0].start, v2.anchors[0].end), (0, 6));
}

#[test]
fn fixture_bodies_migrate_and_lower_back() {
    for name in [
        "sample.md",
        "card_yaml_demo.md",
        "extended_metadata_demo.md",
    ] {
        let rt = from_markdown(&fixture_body(name));
        let v2 = from_lines(&rt);
        assert_eq!(v2.validate(), Ok(()), "{name}");
        assert_eq!(to_lines(&v2), rt, "{name}");
    }
}
