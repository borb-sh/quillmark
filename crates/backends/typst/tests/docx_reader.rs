//! The Word export's reader: what the Layout IR says about small synthetic
//! documents, each read through the probed compile.

use quillmark_docx::layout::{
    ArtifactKind, Child, HAlign, ItemKind, LayoutDoc, LineEnd, Marker, NodeId, NodeKind, PointHint,
};
use quillmark_typst::testing::layout;

mod common;

fn read(plate: &str) -> LayoutDoc {
    let plate = format!("#import \"@local/quillmark-helper:0.2.0\" as qm\n{plate}");
    let quill = common::quill_with_plate(&common::yaml(""), &plate);
    layout(&quill, &serde_json::json!({}), common::test_date()).expect("reads")
}

fn find(doc: &LayoutDoc, pred: impl Fn(&NodeKind) -> bool) -> Vec<NodeId> {
    (0..doc.nodes.len())
        .filter(|&n| pred(&doc.nodes[n].kind))
        .collect()
}

/// The text of `id`'s lines, each line's runs joined in x order.
fn lines(doc: &LayoutDoc, id: NodeId) -> Vec<String> {
    doc.nodes[id]
        .lines
        .iter()
        .map(|l| {
            l.items
                .iter()
                .filter_map(|r| match &doc.item(*r).kind {
                    ItemKind::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

fn text(doc: &LayoutDoc, id: NodeId) -> String {
    doc.subtree(id)
        .into_iter()
        .flat_map(|n| doc.nodes[n].children.clone())
        .filter_map(|c| match c {
            Child::Item(r) => match &doc.item(r).kind {
                ItemKind::Text(t) => Some(t.text.clone()),
                _ => None,
            },
            Child::Node(_) => None,
        })
        .collect()
}

fn paragraphs(doc: &LayoutDoc) -> Vec<NodeId> {
    find(doc, |k| k.forms_paragraph())
        .into_iter()
        .filter(|&n| !doc.nodes[n].lines.is_empty())
        .collect()
}

const PAGE: &str = "#set page(width: 300pt, height: 300pt, margin: (x: 30pt, y: 20pt), numbering: \"i\", header: [Head])\n";

#[test]
fn a_wrapped_paragraph_is_one_node_whose_lines_record_how_they_end() {
    let doc = read(&format!(
        "{PAGE}#set par(justify: true, first-line-indent: (amount: 12pt, all: true))\n\
         A paragraph with _emphasis_ and words enough that it wraps onto a second line of \
         text here.\n\nLine one \\\nline two\n"
    ));
    let pars = find(&doc, |k| matches!(k, NodeKind::Par { .. }));
    assert_eq!(pars.len(), 2);
    let NodeKind::Par {
        justify,
        first_line,
        style,
        ..
    } = &doc.nodes[pars[0]].kind
    else {
        unreachable!()
    };
    assert!(*justify);
    assert_eq!(*first_line, 12.0);
    assert!(style.is_some_and(|s| s.size == 11.0), "{style:?}");
    let first = &doc.nodes[pars[0]].lines;
    assert!(first.len() >= 2);
    assert_eq!(first[0].end, LineEnd::Soft { ate_space: true });
    assert_eq!(first.last().unwrap().end, LineEnd::Last);
    let emph = find(&doc, |k| matches!(k, NodeKind::Emph))[0];
    assert_eq!(
        doc.ancestor(emph, |k| matches!(k, NodeKind::Par { .. })),
        Some(pars[0])
    );
    assert_eq!(text(&doc, emph), "emphasis");
    assert_eq!(lines(&doc, pars[1]), ["Line one", "line two"]);
    assert_eq!(doc.nodes[pars[1]].lines[0].end, LineEnd::Forced);
}

#[test]
fn headings_lists_and_tables_keep_their_structure() {
    let doc = read(&format!(
        "{PAGE}= Title\n- one\n- two\n\n+ first\n+ second\n\n\
         #table(columns: 2, table.header[H1][H2], [a], [b])\n"
    ));
    let heading = find(&doc, |k| matches!(k, NodeKind::Heading { level: 1, .. }));
    assert_eq!(heading.len(), 1);
    assert_eq!(lines(&doc, heading[0]), ["Title"]);
    let block = find(&doc, |k| matches!(k, NodeKind::Block { sticky: true, .. }));
    assert!(
        block.iter().any(|&b| doc
            .ancestor(b, |k| matches!(k, NodeKind::Heading { .. }))
            .is_some()),
        "the heading's block is sticky"
    );
    for (marker, items) in [
        (Marker::Bullet, ["one", "two"]),
        (Marker::Numbered, ["first", "second"]),
    ] {
        let list = find(
            &doc,
            |k| matches!(k, NodeKind::List { marker: m } if *m == marker),
        );
        assert_eq!(list.len(), 1);
        let bodies: Vec<String> = doc
            .subtree(list[0])
            .into_iter()
            .filter(|&n| matches!(doc.nodes[n].kind, NodeKind::ListBody))
            .map(|n| text(&doc, n))
            .collect();
        assert_eq!(bodies, items);
    }
    let table = find(&doc, |k| matches!(k, NodeKind::Table { .. }))[0];
    let NodeKind::Table {
        columns,
        header_rows,
        grid,
    } = &doc.nodes[table].kind
    else {
        unreachable!()
    };
    assert!(!grid);
    assert_eq!(*header_rows, 1);
    assert_eq!(columns.len(), 3);
    assert!(columns.windows(2).all(|w| w[0] < w[1]), "{columns:?}");
    let cells: Vec<String> = doc
        .subtree(table)
        .into_iter()
        .filter(|&n| matches!(doc.nodes[n].kind, NodeKind::Cell { .. }))
        .map(|n| text(&doc, n))
        .collect();
    assert_eq!(cells, ["H1", "H2", "a", "b"]);
}

#[test]
fn page_setup_number_and_chrome_come_from_the_probes_and_artifacts() {
    let doc = read(&format!("{PAGE}one\n#pagebreak()\ntwo\n"));
    assert_eq!(doc.pages.len(), 2);
    let m = doc.pages[0].setup.margins;
    assert_eq!((m.top, m.right, m.bottom, m.left), (20.0, 30.0, 20.0, 30.0));
    let numbers: Vec<_> = doc.pages.iter().map(|p| p.number_text.clone()).collect();
    assert_eq!(numbers, [Some("i".into()), Some("ii".into())]);
    let headers = find(&doc, |k| {
        matches!(
            k,
            NodeKind::Artifact {
                kind: ArtifactKind::Header
            }
        )
    });
    assert_eq!(headers.len(), 2);
    assert!(headers.iter().all(|&h| text(&doc, h) == "Head"));
    let breaks = doc
        .pages
        .iter()
        .flat_map(|p| &p.items)
        .filter(|i| matches!(i.kind, ItemKind::Mark(PointHint::Break { weak: false, .. })))
        .count();
    assert_eq!(breaks, 1);
    assert!(doc.notices.is_empty(), "{:?}", doc.notices);
}

#[test]
fn a_paragraph_across_a_page_break_lends_nothing_to_the_chrome() {
    let words = "word ".repeat(140);
    let doc = read(&format!("{PAGE}#underline[{words}]\n"));
    assert!(doc.pages.len() >= 2);
    let underline = find(&doc, |k| matches!(k, NodeKind::Underline))[0];
    for n in find(&doc, |k| matches!(k, NodeKind::Artifact { .. })) {
        assert!(
            doc.subtree(n)
                .iter()
                .all(|&m| doc.ancestor(m, |k| matches!(k, NodeKind::Underline)) != Some(underline))
        );
    }
    let par = doc
        .ancestor(underline, |k| matches!(k, NodeKind::Par { .. }))
        .unwrap();
    let pages: Vec<u32> = doc.nodes[par].frames.iter().map(|f| f.0).collect();
    assert_eq!(pages, [0, 1]);
}

#[test]
fn a_repeated_header_row_is_read_once() {
    let rows = "[x], ".repeat(60);
    let doc = read(&format!(
        "{PAGE}#table(columns: 1, table.header[Top], {rows})\n"
    ));
    assert!(doc.pages.len() >= 2);
    let tops = paragraphs(&doc)
        .into_iter()
        .filter(|&p| lines(&doc, p) == ["Top"])
        .count();
    assert_eq!(tops, 1);
}

#[test]
fn the_inline_run_of_a_block_without_a_paragraph_becomes_one() {
    let doc = read(&format!(
        "{PAGE}#block[first run #block[inner] second run]\n#align(center)[centred]\n#box[no wrap here]\n"
    ));
    let runs: Vec<Vec<String>> = find(&doc, |k| matches!(k, NodeKind::InlineRun { .. }))
        .into_iter()
        .map(|n| lines(&doc, n))
        .collect();
    assert_eq!(runs, [["inner"]]);
    let texts: Vec<String> = paragraphs(&doc)
        .into_iter()
        .map(|p| text(&doc, p))
        .collect();
    for t in ["first run", "inner", "second run"] {
        assert!(texts.iter().any(|x| x == t), "{texts:?}");
    }
    let centred = find(&doc, |k| matches!(k, NodeKind::Align { h: HAlign::Center }));
    assert_eq!(centred.len(), 1);
    assert_eq!(text(&doc, centred[0]), "centred");
    let nobreak = find(&doc, |k| matches!(k, NodeKind::NoBreak));
    assert_eq!(text(&doc, nobreak[0]), "no wrap here");
}

#[test]
fn a_footnote_is_its_own_node_and_its_reference_a_mark() {
    let doc = read(&format!("{PAGE}Text#footnote[The note.] after.\n"));
    let notes = find(&doc, |k| matches!(k, NodeKind::Footnote));
    assert_eq!(notes.len(), 1);
    assert!(text(&doc, notes[0]).contains("The note."));
    let par = find(&doc, |k| matches!(k, NodeKind::Par { .. }))[0];
    assert_eq!(text(&doc, par), "Text after.");
    let marks = doc.nodes[par].lines[0]
        .items
        .iter()
        .filter(|r| matches!(doc.item(**r).kind, ItemKind::Mark(PointHint::FootnoteRef { note }) if note == notes[0]))
        .count();
    assert_eq!(marks, 1);
}

#[test]
fn every_fixture_reads_into_a_tree_that_holds_each_item_once() {
    for name in quillmark_fixtures::quill_names() {
        let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path(&name))
            .unwrap_or_else(|e| panic!("{name} loads: {e:?}"));
        if quill.backend_id() != "typst" {
            continue;
        }
        for doc in std::iter::once(quill.seed_document()).chain(quill.example_document()) {
            let data = quill
                .compile_checked(&doc, common::test_date())
                .expect("data");
            let ir = layout(&quill, &data, common::test_date()).expect("reads");
            let mut seen = std::collections::HashSet::new();
            for n in ir.subtree(0) {
                assert_eq!(ir.nodes[n].parent.is_none(), n == 0, "{name}");
                for c in &ir.nodes[n].children {
                    if let Child::Item(r) = c {
                        assert_eq!(ir.item(*r).node, n, "{name}");
                        assert!(seen.insert(*r), "{name}: an item is read twice");
                    }
                    if let Child::Node(m) = c {
                        assert_eq!(ir.nodes[*m].parent, Some(n), "{name}");
                    }
                }
            }
            let items: usize = ir.pages.iter().map(|p| p.items.len()).sum();
            assert_eq!(seen.len(), items, "{name}: an item is outside the tree");
            assert!(ir.pages.iter().all(|p| p.number_text.is_some()), "{name}");
        }
    }
}
