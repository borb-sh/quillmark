//! Word export of a compiled document.
//!
//! The export reads a second compile of the session's sources with
//! [`probes`] in the library, which record what Typst lays out without
//! tagging it. Their ink must equal the live compile's; when it does not, the
//! live document is read instead, without probes, under
//! `docx::probes_disabled`.

pub(crate) mod probes;

use quillmark_docx::model::{
    Blocks, Document, Info, Paragraph, ParProps, RunProps, Section, SectionProps,
};
use quillmark_core::error::{Diagnostic, Severity};
use typst::layout::{Frame, FrameItem, Transform};
use typst_layout::PagedDocument;

use crate::world::QuillWorld;

const CODE_PROBES_DISABLED: &str = "docx::probes_disabled";

/// The probed compile of `world` when its ink equals `live`'s, else `None`
/// and the `docx::probes_disabled` warning.
pub(crate) fn probed(
    live: &PagedDocument,
    world: &QuillWorld,
) -> Result<PagedDocument, Diagnostic> {
    world
        .probed()
        .and_then(|w| crate::compile::compile_document(&w).ok())
        .map(|(doc, _)| doc)
        .filter(|doc| crate::ink_hashes(doc) == crate::ink_hashes(live))
        .ok_or_else(|| {
            Diagnostic::new(
                Severity::Warning,
                "the probed compile did not lay out the document's ink, so the Word file is \
                 read without its probes and carries less structure"
                    .to_string(),
            )
            .with_code(CODE_PROBES_DISABLED.to_string())
        })
}

/// The `.docx` bytes for `live` and the warnings the export raises.
pub(crate) fn export(
    live: &PagedDocument,
    world: &QuillWorld,
    title: Option<String>,
) -> (Vec<u8>, Vec<Diagnostic>) {
    match probed(live, world) {
        Ok(doc) => (plain_text(&doc, title), Vec::new()),
        Err(warning) => (plain_text(live, title), vec![warning]),
    }
}

/// `document`'s text, one paragraph per line, each page starting a new one.
fn plain_text(document: &PagedDocument, title: Option<String>) -> Vec<u8> {
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
            title,
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
