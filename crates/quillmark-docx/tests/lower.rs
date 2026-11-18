//! Lowering from layout values built here: what each node becomes in Word,
//! and what the export says it left out.

use quillmark_docx::layout::{
    Child, FontFace, Item, ItemKind, ItemRef, LayoutDoc, Line, LineEnd, Node, NodeKind, Page,
    PageSetup, Rgb, Sides, TextRun,
};
use quillmark_docx::lower::lower;
use quillmark_docx::model::{Block, Inline, RunContent};

fn page() -> Page {
    Page {
        size: [612.0, 792.0],
        setup: PageSetup {
            margins: Sides {
                top: 72.0,
                right: 72.0,
                bottom: 72.0,
                left: 72.0,
            },
            columns: 1,
            gutter: 0.0,
            flipped: false,
        },
        number: 1,
        number_text: Some("1".into()),
        items: Vec::new(),
    }
}

/// A document of one paragraph whose lines hold `lines`, in `family`.
fn paragraph(family: &str, lines: &[(&str, LineEnd)]) -> LayoutDoc {
    let mut doc = LayoutDoc::default();
    doc.fonts.push(FontFace {
        family: family.into(),
        bold: false,
        italic: false,
        monospace: false,
    });
    doc.pages.push(page());
    let par = 1;
    doc.nodes.push(Node::new(
        Some(0),
        NodeKind::Par {
            justify: false,
            first_line: 0.0,
            hanging: 0.0,
            style: None,
        },
    ));
    doc.nodes[0].children.push(Child::Node(par));
    for (i, (text, end)) in lines.iter().enumerate() {
        let baseline = 100.0 + 14.0 * i as f64;
        let r = ItemRef {
            page: 0,
            index: i as u32,
        };
        doc.pages[0].items.push(Item {
            node: par,
            kind: ItemKind::Text(TextRun {
                origin: [72.0, baseline],
                text: text.to_string(),
                font: 0,
                size: 12.0,
                fill: Rgb(0, 0, 0),
                width: 6.0 * text.len() as f64,
            }),
        });
        doc.nodes[par].children.push(Child::Item(r));
        doc.nodes[par].lines.push(Line {
            page: 0,
            baseline,
            x0: 72.0,
            x1: 72.0 + 6.0 * text.len() as f64,
            items: vec![r],
            end: *end,
        });
    }
    doc
}

fn texts(blocks: &[Block], last: &quillmark_docx::model::Paragraph) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .chain(std::iter::once(last))
        .map(|p| {
            p.content
                .iter()
                .flat_map(|i| match i {
                    Inline::Run(r) => r.content.clone(),
                    _ => Vec::new(),
                })
                .map(|c| match c {
                    RunContent::Text(t) => t,
                    RunContent::Break(_) => "\n".into(),
                    _ => String::new(),
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_paragraphs_lines_join_into_one_run_and_only_a_forced_end_breaks() {
    let doc = paragraph(
        "TeX Gyre Termes",
        &[
            ("A wrapped ", LineEnd::Soft { ate_space: true }),
            ("hyphen\u{ad}", LineEnd::Hyphenated),
            ("ated line", LineEnd::Forced),
            ("after", LineEnd::Last),
        ],
    );
    let (word, notices) = lower(&doc);
    let body = &word.sections[0].body;
    assert_eq!(
        texts(&body.blocks, &body.last),
        ["A wrapped hyphenated line\nafter"]
    );
    assert_eq!(word.fonts.len(), 1);
    assert_eq!(word.fonts[0].name, "Times New Roman");
    assert!(notices.is_empty(), "{notices:?}");
    let spacing = body.last.props.spacing.expect("spacing");
    assert_eq!(
        spacing.line,
        Some(quillmark_docx::model::Line::Exact(280)),
        "the measured stride"
    );
}

#[test]
fn a_face_without_a_word_clone_is_named_and_reported() {
    let (word, notices) = lower(&paragraph("Quill Sans", &[("text", LineEnd::Last)]));
    assert_eq!(word.fonts[0].name, "Quill Sans");
    let codes: Vec<&str> = notices.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["docx::font_not_embedded"]);
}

#[test]
fn ink_no_node_places_is_counted_as_dropped() {
    let mut doc = paragraph("TeX Gyre Termes", &[("kept", LineEnd::Last)]);
    let stray = Node::new(Some(0), NodeKind::Strong);
    doc.nodes.push(stray);
    let id = doc.nodes.len() - 1;
    doc.nodes[0].children.push(Child::Node(id));
    doc.pages[0].items.push(Item {
        node: id,
        kind: ItemKind::Text(TextRun {
            origin: [72.0, 300.0],
            text: "lost".into(),
            font: 0,
            size: 12.0,
            fill: Rgb(0, 0, 0),
            width: 24.0,
        }),
    });
    doc.nodes[id]
        .children
        .push(Child::Item(ItemRef { page: 0, index: 1 }));
    let (_, notices) = lower(&doc);
    let codes: Vec<&str> = notices.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["docx::ink_dropped"]);
}
