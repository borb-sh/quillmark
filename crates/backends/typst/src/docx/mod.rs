//! Word export of a compiled document.

use quillmark_docx::model::{
    Blocks, Document, Info, Paragraph, ParProps, RunProps, Section, SectionProps,
};
use typst::model::Document as _;
use typst::layout::{Frame, FrameItem, Transform};
use typst_layout::PagedDocument;

/// The `.docx` bytes for `document`: its text, one paragraph per line, each
/// page starting a new one.
pub(crate) fn export(document: &PagedDocument) -> Vec<u8> {
    let mut paragraphs = Vec::new();
    for page in document.pages() {
        let mut lines: Vec<(f64, String)> = Vec::new();
        collect_lines(&page.frame, Transform::identity(), &mut lines);
        for (i, (_, text)) in lines.into_iter().enumerate() {
            let props = ParProps {
                page_break_before: i == 0 && !paragraphs.is_empty(),
                ..ParProps::default()
            };
            paragraphs.push(Paragraph::text(props, RunProps::default(), text));
        }
    }
    let doc = Document {
        sections: vec![Section {
            body: Blocks::from_paragraphs(paragraphs),
            props: SectionProps::default(),
        }],
        info: Info {
            title: document.info().title.as_ref().map(|t| t.to_string()),
            language: None,
        },
        ..Document::default()
    };
    quillmark_docx::write_docx(&doc)
}

/// Text items in paint order, joined into one line while their baselines agree.
fn collect_lines(frame: &Frame, ts: Transform, lines: &mut Vec<(f64, String)>) {
    for (pos, item) in frame.items() {
        match item {
            FrameItem::Group(group) => {
                let ts = ts
                    .pre_concat(Transform::translate(pos.x, pos.y))
                    .pre_concat(group.transform);
                collect_lines(&group.frame, ts, lines);
            }
            FrameItem::Text(text) => {
                let y = pos.transform(ts).y.to_pt();
                match lines.last_mut() {
                    Some((baseline, line)) if (*baseline - y).abs() < 0.5 => {
                        line.push_str(text.text.as_str());
                    }
                    _ => lines.push((y, text.text.to_string())),
                }
            }
            _ => {}
        }
    }
}
