//! The Word export's reader: what the Layout IR says about small synthetic
//! documents, each read through the probed compile.

use quillmark_docx::layout::{
    ArtifactKind, Child, HAlign, ItemKind, LayoutDoc, LineEnd, Marker, NodeId, NodeKind,
    NumberFormat, PointHint,
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
    assert_eq!(first[0].end, LineEnd::Soft);
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
    for (bullet, items) in [(true, ["one", "two"]), (false, ["first", "second"])] {
        let list = find(
            &doc,
            |k| matches!(k, NodeKind::List { marker: m, .. } if matches!(m, Marker::Bullet(_)) == bullet),
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
        ..
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
        .filter(|&n| {
            doc.ancestor(n, |k| matches!(k, NodeKind::Artifact { .. }))
                .is_none()
        })
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
    for (name, quill, data) in common::typst_fixture_documents() {
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

#[test]
fn a_list_number_format_comes_from_the_enum_and_not_its_labels() {
    let doc = read(&format!(
        "{PAGE}#enum(numbering: \"(a)\", start: 9, [nine], [ten])\n#list(marker: [--], [dash])\n"
    ));
    let markers: Vec<Marker> = find(&doc, |k| matches!(k, NodeKind::List { .. }))
        .into_iter()
        .map(|n| match &doc.nodes[n].kind {
            NodeKind::List { marker, .. } => marker.clone(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        markers,
        [
            Marker::Numbered {
                counters: vec![("(".into(), NumberFormat::LowerLetter)],
                suffix: ")".into(),
                start: 9,
            },
            Marker::Bullet("\u{2013}".into()),
        ]
    );
}

#[test]
fn a_newline_a_string_spells_inside_a_grid_cell_is_a_forced_line_end() {
    let doc = read(&format!(
        "{PAGE}#grid(columns: 2, [#\"123 Street\\nSpringfield\"], [right])\n"
    ));
    let cell = paragraphs(&doc)
        .into_iter()
        .find(|&p| text(&doc, p).starts_with("123"))
        .expect("the cell's paragraph");
    assert_eq!(lines(&doc, cell), ["123 Street", "Springfield"]);
    assert_eq!(doc.nodes[cell].lines[0].end, LineEnd::Forced);
}

#[test]
fn ink_the_tree_cannot_hold_is_a_picture_and_a_highlight_keeps_its_fill() {
    let doc = read(&format!(
        "{PAGE}Before #highlight(fill: blue)[blue] after.\n#circle(radius: 10pt, fill: red)\n\
         #rotate(30deg)[Turned]\n#rect(width: 40pt, height: 10pt, fill: gradient.linear(red, blue))\n\
         #line(length: 100%)\n#box(clip: true, width: 20pt, height: 8pt)[clipped text that is long]\n\
         #box(clip: true, width: 200pt)[fits]\n#text(weight: \"bold\")[Heavy]\n"
    ));
    let items: Vec<&ItemKind> = doc
        .pages
        .iter()
        .flat_map(|p| &p.items)
        .map(|i| &i.kind)
        .collect();
    let rasters = items
        .iter()
        .filter(|k| matches!(k, ItemKind::Raster { .. }))
        .count();
    assert_eq!(
        rasters, 4,
        "circle, rotation, gradient and the cutting clip"
    );
    assert!(
        items.iter().any(|k| matches!(k, ItemKind::Shape(_))),
        "the rule stays a line"
    );
    assert!(doc.notices.iter().any(|n| n.code == "docx::rasterized"));
    let fills: Vec<_> = find(&doc, |k| matches!(k, NodeKind::Highlight { .. }))
        .into_iter()
        .map(|n| doc.nodes[n].kind.clone())
        .collect();
    assert_eq!(
        fills,
        [NodeKind::Highlight {
            fill: quillmark_docx::layout::Rgb(0, 116, 217)
        }]
    );
    let heavy = paragraphs(&doc)
        .into_iter()
        .flat_map(|p| doc.nodes[p].lines.clone())
        .flat_map(|l| l.items)
        .find_map(|r| match &doc.item(r).kind {
            ItemKind::Text(t) if t.text == "Heavy" => Some(t.font),
            _ => None,
        })
        .expect("the bold run");
    assert!(doc.fonts[heavy].bold, "a weight on a variable face is bold");
}

#[test]
fn a_tables_columns_close_at_their_own_size_and_cells_carry_their_fields() {
    let doc = read(&format!(
        "{PAGE}Above.\n#table(columns: (60pt, 1fr), inset: 4pt, fill: (x, y) => if y == 0 {{ gray }},\n\
         stroke: 0.5pt + red, [a], [b], [c], [d])\n#table(columns: (auto, auto), [wide cell], [x])\n"
    ));
    let tables = find(&doc, |k| matches!(k, NodeKind::Table { .. }));
    let NodeKind::Table {
        columns,
        content_sized,
        rows,
        ..
    } = &doc.nodes[tables[0]].kind
    else {
        unreachable!()
    };
    assert_eq!(
        columns,
        &[30.0, 90.0, 270.0],
        "a fixed column and a fraction to the margin"
    );
    assert_eq!(content_sized, &[false, false]);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].1 < rows[1].1);
    let cells: Vec<NodeKind> = doc
        .subtree(tables[0])
        .into_iter()
        .filter(|&n| matches!(doc.nodes[n].kind, NodeKind::Cell { .. }))
        .map(|n| doc.nodes[n].kind.clone())
        .collect();
    let NodeKind::Cell {
        fill,
        inset,
        borders,
        ..
    } = &cells[0]
    else {
        unreachable!()
    };
    assert_eq!(*fill, Some(quillmark_docx::layout::Rgb(170, 170, 170)));
    assert_eq!((inset.top, inset.left), (4.0, 4.0));
    assert_eq!(
        borders[0],
        Some((0.5, quillmark_docx::layout::Rgb(255, 65, 54)))
    );
    assert!(matches!(&cells[2], NodeKind::Cell { fill: None, .. }));
    let NodeKind::Table {
        columns,
        content_sized,
        ..
    } = &doc.nodes[tables[1]].kind
    else {
        unreachable!()
    };
    assert_eq!(content_sized, &[true, true]);
    let wide = paragraphs(&doc)
        .into_iter()
        .find(|&p| text(&doc, p) == "wide cell")
        .unwrap();
    let ink = doc.nodes[wide].frames[0].1.x1;
    assert!((columns[1] - (ink + 5.0)).abs() < 0.5, "{columns:?} {ink}");
}

#[test]
fn a_ligatures_natural_width_is_its_characters_own_advances() {
    let width = |plate: &str| {
        let doc = read(&format!("{PAGE}{plate}\n"));
        doc.pages
            .iter()
            .flat_map(|p| &p.items)
            .find_map(|i| match &i.kind {
                ItemKind::Text(t) if t.text.starts_with("ffi") => Some((t.width, t.natural)),
                _ => None,
            })
            .expect("the run")
    };
    let (joined, natural) = width("ffi");
    let (apart, _) = width("#text(ligatures: false)[ffi]");
    assert!(joined < apart - 0.1, "{joined} {apart}");
    assert!((natural - apart).abs() < 0.01, "{natural} {apart}");
}
