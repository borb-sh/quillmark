//! Markdown at the edges: an import reads markdown through the line form and
//! migrates it, an export lowers the content to the line form and writes it.

use crate::lines;
use crate::lines::export::{AnchorRead, Annotated, ExportLoss};
use crate::lines::import::{ImportError, ImportWarning};
use crate::migrate::{from_lines, to_lines};
use crate::model::{BlockKind, Content, MarkKind};
use crate::model::Usv;

/// A markdown import: the content and what the markdown spelled that it
/// could not carry.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "carries the import's warnings; read `.warnings` or bind it"]
pub struct Imported {
    pub content: Content,
    pub warnings: Vec<ImportWarning>,
}

/// An export and what of the content it does not carry.
#[derive(Debug, Clone, PartialEq)]
pub struct Exported {
    pub markdown: String,
    pub losses: Vec<Loss>,
}

/// One part of a content its markdown does not carry as itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Loss {
    /// What the line form reports, in its positions, which are the content's.
    Line(ExportLoss),
    /// A slot whose text the line form spells otherwise: a hard break where
    /// markdown reads a paragraph break (two in a row, or one ending a
    /// paragraph another follows), or one at the slot's edge.
    SlotBreaks { island: String, slot: String },
    /// An `x-` block, which markdown writes as a paragraph.
    ExtBlock { id: String, name: String },
    /// An `x-` mark, which markdown does not write. Positions are the
    /// content's, or the slot's where `slot` names one.
    ExtMark { start: Usv, end: Usv, name: String, slot: Option<(String, String)> },
}

pub fn from_markdown(markdown: &str) -> Result<Imported, ImportError> {
    let lines::import::Imported { content, warnings } = lines::import::from_markdown(markdown)?;
    Ok(Imported { content: from_lines(&content), warnings })
}

/// The literal codec: each blank-line-separated run a paragraph, each lone
/// `\n` within one a hard break.
pub fn from_plaintext(s: &str) -> Content {
    from_lines(&lines::import::from_plaintext(s))
}

pub fn to_markdown(c: &Content) -> String {
    lines::export::to_markdown(&to_lines(c))
}

pub fn to_markdown_checked(c: &Content) -> Exported {
    let lines::export::Exported { markdown, losses } = lines::export::to_markdown_checked(&to_lines(c));
    let mut losses: Vec<Loss> = losses.into_iter().map(Loss::Line).collect();
    let ext_marks = |c: &Content, slot: Option<(String, String)>| {
        c.marks
            .iter()
            .filter_map(|m| match &m.kind {
                MarkKind::Ext { name, .. } => Some(Loss::ExtMark { start: m.start, end: m.end, name: name.clone(), slot: slot.clone() }),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    losses.extend(c.blocks.iter().filter_map(|b| match &b.kind {
        BlockKind::Ext { name, .. } => Some(Loss::ExtBlock { id: b.id.clone(), name: name.clone() }),
        _ => None,
    }));
    losses.extend(ext_marks(c, None));
    let lifted = crate::diff::lift(c, &to_lines(c));
    for island in &c.islands {
        for (id, slot) in &island.slots {
            let held = lifted.island(&island.id).and_then(|i| i.slots.get(id));
            if held.map(|s| &s.text) != Some(&slot.text) {
                losses.push(Loss::SlotBreaks { island: island.id.clone(), slot: id.clone() });
            }
            losses.extend(ext_marks(slot, Some((island.id.clone(), id.clone()))));
        }
    }
    Exported { markdown, losses }
}

/// The two-way read: [`to_markdown`] with each anchor's tag and each
/// unwritable island's token, and every anchor listed (markdown-spec §6.4).
pub fn to_markdown_annotated(c: &Content) -> Annotated {
    lines::export::to_markdown_annotated(&to_lines(c))
}

/// The anchors [`to_markdown_annotated`] lists whose id `keep` holds.
pub fn anchors_where(c: &Content, keep: impl Fn(&str) -> bool) -> Vec<AnchorRead> {
    lines::export::anchors_where(&to_lines(c), keep)
}

/// The text with every block break a blank line and every hard break a `\n`,
/// island slots removed: what the literal codec reads back.
pub fn to_plaintext(c: &Content) -> String {
    lines::export::to_plaintext(&to_lines(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HARD_BREAK, NOTE_SLOT};

    #[test]
    fn a_hard_break_is_one_block() {
        let c = from_markdown("one\\\ntwo\n\nthree").unwrap().content;
        assert_eq!(c.text, format!("one{HARD_BREAK}two\nthree"));
        assert_eq!(c.blocks.len(), 2);
        assert_eq!(from_markdown(&to_markdown(&c)).unwrap().content, c);
    }

    /// A table cell is one paragraph: a paragraph break in one reads as a hard
    /// break from either table spelling, a value holding one is refused at a
    /// write, joined on a stored read and by repair, and a footnote's note
    /// keeps its paragraphs.
    #[test]
    fn a_table_cell_is_one_paragraph() {
        use crate::model::Invariant;
        use crate::repair::{Raw, Repair, repair};
        use crate::wire::ParseError;
        let pipe = from_markdown("| h |\n| - |\n| one<br><br>two |").unwrap().content;
        let html = from_markdown("<table><tr><td>h</td></tr><tr><td><p>one</p>\n<p>two</p></td></tr></table>").unwrap();
        assert_eq!(html.warnings, []);
        let joined = format!("one{HARD_BREAK}two");
        for c in [&pipe, &html.content] {
            let cell = &c.islands[0].slots[&c.islands[0].slot_order()[1]];
            assert_eq!((cell.text.as_str(), cell.blocks.len()), (joined.as_str(), 1));
            assert_eq!(from_markdown(&to_markdown(c)).unwrap().content, *c);
        }
        let mut edited = pipe.clone();
        edited.islands[0].slots.get_mut("s1").unwrap().text = format!("one{HARD_BREAK}three");
        let change = crate::diff::replace(&pipe, &edited).unwrap();
        assert_eq!(crate::change::apply(&pipe, &change).unwrap().content, edited);

        let mut stored = pipe.to_value();
        stored["islands"][0]["slots"]["s1"]["text"] = "one\ntwo".into();
        stored["islands"][0]["slots"]["s1"]["blocks"] = serde_json::json!([{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}]);
        let refused = Invariant::BreakInCell { island: "isl-0".into(), slot: "s1".into() };
        assert_eq!(Content::from_value(&stored), Err(ParseError::Invalid(refused.clone())));
        let read = Content::from_value_lenient(&stored).unwrap();
        assert_eq!((read.content.clone(), read.rewritten), (pipe.clone(), true));
        let (repaired, repairs) = repair(Raw::from_value(&stored).unwrap());
        assert_eq!(repaired, pipe);
        assert!(matches!(repairs.as_slice(), [r @ Repair::Invalid(inv)] if *inv == refused && r.inserts()));

        let mut noted = from_markdown("a[^1]\n\n[^1]: one").unwrap().content.to_value();
        noted["islands"][0]["slots"][NOTE_SLOT] = stored["islands"][0]["slots"]["s1"].clone();
        assert_eq!(Content::from_value(&noted).unwrap().to_value(), noted);

        let mut c = pipe;
        let cell = c.islands[0].slots.get_mut("s1").unwrap();
        cell.text = format!("d{HARD_BREAK}{HARD_BREAK}e");
        let out = to_markdown_checked(&c);
        assert_eq!(out.losses, [Loss::SlotBreaks { island: "isl-0".into(), slot: "s1".into() }]);
    }

    #[test]
    fn an_x_block_and_an_x_mark_are_losses_the_line_form_hides() {
        let c = Content::from_value(&serde_json::json!({
            "text": "ab\ncd", "blocks": [{"id": "b0", "kind": "x-call"}, {"id": "b1", "kind": "para"}],
            "marks": [{"start": 3, "end": 5, "type": "x-hi", "attrs": {"k": 1}}],
        }))
        .unwrap();
        assert_eq!(
            to_markdown_checked(&c).losses,
            [
                Loss::ExtBlock { id: "b0".into(), name: "x-call".into() },
                Loss::ExtMark { start: 3, end: 5, name: "x-hi".into(), slot: None },
            ]
        );
    }
}
