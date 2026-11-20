//! Lowering from layout values built here: what each node becomes in Word,
//! and what the export says it left out.

use quillmark_docx::layout::{
    Child, FontFace, Item, ItemKind, ItemRef, LayoutDoc, Line, LineEnd, Node, NodeKind, Page,
    PageSetup, Rgb, Sides, TextRun,
};
use quillmark_docx::lower::lower;
use quillmark_docx::model::{Block, Inline, Paragraph, RunContent, Twips};

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
            header_ascent: 21.6,
            footer_descent: 21.6,
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
            left: None,
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
                natural: 6.0 * text.len() as f64,
                lang: None,
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

fn texts(blocks: &[Block], last: &Paragraph) -> Vec<String> {
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
fn lines_closer_than_half_their_size_give_no_pitch() {
    let mut doc = paragraph(
        "TeX Gyre Termes",
        &[
            ("one line ", LineEnd::Soft { ate_space: true }),
            ("read as two", LineEnd::Last),
        ],
    );
    doc.nodes[1].lines[1].baseline = 96.0;
    let (word, _) = lower(&doc);
    let spacing = word.sections[0].body.last.props.spacing.expect("spacing");
    assert_eq!(
        spacing.line,
        Some(quillmark_docx::model::Line::Exact(288)),
        "1.2 of the size, not the stride"
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
            natural: 24.0,
            lang: None,
        }),
    });
    doc.nodes[id]
        .children
        .push(Child::Item(ItemRef { page: 0, index: 1 }));
    let (_, notices) = lower(&doc);
    let codes: Vec<&str> = notices.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["docx::ink_dropped"]);
}

#[test]
fn text_no_element_tags_is_a_paragraph_per_line_and_reported() {
    let mut doc = paragraph(
        "TeX Gyre Termes",
        &[
            ("Two lines ", LineEnd::Soft { ate_space: true }),
            ("of loose text", LineEnd::Last),
        ],
    );
    doc.nodes[1].kind = NodeKind::Unstructured;
    let (word, notices) = lower(&doc);
    let body = &word.sections[0].body;
    assert_eq!(
        texts(&body.blocks, &body.last),
        ["Two lines ", "of loose text"]
    );
    let codes: Vec<&str> = notices.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["docx::unstructured_text"]);
}

/// A one-line paragraph of `text` under `parent`, on the first page with its
/// baseline at `baseline`.
fn line_under(doc: &mut LayoutDoc, parent: usize, text: &str, baseline: f64) -> usize {
    let par = push_node(
        doc,
        parent,
        NodeKind::Par {
            justify: false,
            first_line: 0.0,
            hanging: 0.0,
            style: None,
            left: None,
        },
    );
    let r = ItemRef {
        page: 0,
        index: doc.pages[0].items.len() as u32,
    };
    let width = 6.0 * text.len() as f64;
    doc.pages[0].items.push(Item {
        node: par,
        kind: ItemKind::Text(TextRun {
            origin: [72.0, baseline],
            text: text.into(),
            font: 0,
            size: 12.0,
            fill: Rgb(0, 0, 0),
            width,
            natural: width,
            lang: None,
        }),
    });
    doc.nodes[par].children.push(Child::Item(r));
    doc.nodes[par].lines.push(Line {
        page: 0,
        baseline,
        x0: 72.0,
        x1: 72.0 + width,
        items: vec![r],
        end: LineEnd::Last,
    });
    par
}

fn push_node(doc: &mut LayoutDoc, parent: usize, kind: NodeKind) -> usize {
    doc.nodes.push(Node::new(Some(parent), kind));
    let id = doc.nodes.len() - 1;
    doc.nodes[parent].children.push(Child::Node(id));
    id
}

#[test]
fn a_contents_entry_is_spaced_from_the_entry_before_it_past_what_sits_between() {
    let mut doc = paragraph("TeX Gyre Termes", &[("Before", LineEnd::Last)]);
    let toc = push_node(&mut doc, 0, NodeKind::Toc { depth: None });
    let a = push_node(&mut doc, toc, NodeKind::TocEntry { level: 1 });
    line_under(&mut doc, a, "Alpha", 130.0);
    line_under(&mut doc, toc, "Between", 150.0);
    let b = push_node(&mut doc, toc, NodeKind::TocEntry { level: 1 });
    line_under(&mut doc, b, "Beta", 170.0);
    let (word, _) = lower(&doc);
    let body = &word.sections[0].body;
    let Some(Block::Toc(contents)) = body.blocks.get(1) else {
        panic!("contents second: {:?}", body.blocks);
    };
    let entries: Vec<String> = contents
        .entries
        .iter()
        .map(|p| texts(&[], p).remove(0))
        .collect();
    assert_eq!(entries, ["Alpha", "Beta"]);
    let before = |p: &Paragraph| p.props.spacing.and_then(|s| s.before);
    assert_eq!(
        before(&contents.entries[1]),
        Some(((170.0 - 130.0 - 14.4) * 20.0f64).round() as Twips),
        "from Alpha, which Word sets it after"
    );
    assert_eq!(texts(&[], &body.last), ["Between"]);
    assert_eq!(
        before(&body.last),
        Some(0),
        "Word sets it after Beta, which is below it"
    );
}
