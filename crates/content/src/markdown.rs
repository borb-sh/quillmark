//! Markdown at the edges: an import reads markdown through the line form and
//! migrates it, an export lowers the content to the line form and writes it.

use crate::lines;
use crate::lines::export::{AnchorRead, Annotated, ExportLoss};
use crate::lines::import::{ImportError, ImportWarning};
use crate::migrate::{from_lines, to_lines};
use crate::model::Content;

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
    /// A slot whose hard breaks sit where markdown reads a paragraph break:
    /// two in a row, or one ending a paragraph another follows.
    SlotBreaks { island: String, slot: String },
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
    for island in &c.islands {
        for (id, slot) in &island.slots {
            let (text, marks) = crate::table::slot_to_cell(slot);
            if crate::table::cell_to_slot(&text, &marks).text != slot.text {
                losses.push(Loss::SlotBreaks { island: island.id.clone(), slot: id.clone() });
            }
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
    use crate::model::{Block, BlockKind, HARD_BREAK};

    #[test]
    fn a_hard_break_is_one_block() {
        let c = from_markdown("one\\\ntwo\n\nthree").unwrap().content;
        assert_eq!(c.text, format!("one{HARD_BREAK}two\nthree"));
        assert_eq!(c.blocks.len(), 2);
        assert_eq!(from_markdown(&to_markdown(&c)).unwrap().content, c);
    }

    #[test]
    fn a_slot_of_two_paragraphs_writes_an_html_table_and_reads_back() {
        let mut c = from_markdown("| a | b |\n| - | - |\n| c | d |").unwrap().content;
        let island = &mut c.islands[0];
        let slot = island.slot_order()[3].clone();
        let cell = island.slots.get_mut(&slot).unwrap();
        cell.text = "d\ne".into();
        cell.blocks.push(Block::new("b1", BlockKind::Para));
        cell.blocks[0].id = "b2".into();
        assert_eq!(c.validate(), Ok(()));
        let out = to_markdown_checked(&c);
        assert!(out.markdown.contains("<table"), "{}", out.markdown);
        assert_eq!(out.losses, []);
        let back = from_markdown(&out.markdown).unwrap().content;
        assert_eq!(back.islands[0].slots[&slot].text, "d\ne");

        let cell = c.islands[0].slots.get_mut(&slot).unwrap();
        cell.text = format!("d{HARD_BREAK}{HARD_BREAK}e");
        cell.blocks.truncate(1);
        let out = to_markdown_checked(&c);
        assert_eq!(out.losses, [Loss::SlotBreaks { island: c.islands[0].id.clone(), slot }]);
    }
}
