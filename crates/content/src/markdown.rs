//! Markdown at the edges: [`from_markdown`] is the one import and
//! [`to_markdown`] with [`to_markdown_annotated`] the one export, plain or
//! annotated; the plain export drops every anchor and the annotated one spells
//! them for a revise to read back.

use crate::lines;
use crate::lines::export::ExportLoss;
pub use crate::lines::export::{AnchorRead, Annotated};
pub use crate::lines::import::{DroppedConstruct, ImportError, ImportWarning};
use crate::migrate::{from_lines, to_lines};
use crate::ids::prefix;
use crate::model::{Block, BlockKind, Codec, Content, HARD_BREAK, ISLAND_SLOT, MarkKind};
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

/// The import: `markdown` read cold, its ids minted positionally and no
/// anchor in it. Its `qm-anchor` tags and `qm-island` tokens are what a
/// revise reads back; read cold, they hold nothing.
pub fn from_markdown(markdown: &str) -> Result<Imported, ImportError> {
    let lines::import::Imported { content, warnings } = lines::import::from_markdown(markdown)?;
    Ok(Imported { content: from_lines(&content), warnings })
}

/// The literal codec: each blank line a paragraph break and each other `\n`
/// a hard break, so a run of `\n` past two that counts odd is a hard break
/// then a paragraph break per pair. Every character is text, never syntax.
pub fn from_plaintext(s: &str) -> Content {
    let mut text = String::with_capacity(s.len());
    let mut breaks = 0usize;
    let mut blocks = 1usize;
    let mut flush = |text: &mut String, breaks: &mut usize| {
        if *breaks % 2 == 1 {
            text.push(HARD_BREAK);
        }
        for _ in 0..*breaks / 2 {
            text.push('\n');
            blocks += 1;
        }
        *breaks = 0;
    };
    for c in s.chars().filter(|&c| c != ISLAND_SLOT).filter_map(lines::normalize::admit_char) {
        match c {
            '\n' => breaks += 1,
            c => {
                flush(&mut text, &mut breaks);
                text.push(c);
            }
        }
    }
    flush(&mut text, &mut breaks);
    let blocks = (0..blocks).map(|n| Block::new(format!("{}{n}", prefix::BLOCK), BlockKind::Para)).collect();
    Content { text, blocks, ..Content::empty() }
}

/// [`from_plaintext`] under [`Codec::Plain`]: a `text` field's value.
pub fn plain(s: &str) -> Content {
    Content { codec: Codec::Plain, ..from_plaintext(s) }
}

/// The text a value's codec projects it to: a [`Codec::Plain`] value its
/// literal text, any other its markdown.
pub fn to_text(c: &Content) -> String {
    match c.codec {
        Codec::Plain => to_plaintext(c),
        Codec::Rich => to_markdown(c),
    }
}

/// The plain export: what the content's markdown spells, every anchor and
/// every `x-` construct dropped ([`to_markdown_checked`] names them).
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

/// The annotated read under the value's codec: [`to_markdown_annotated`], or
/// for a [`Codec::Plain`] value its literal text, no tag in it, and every
/// anchor listed.
pub fn to_text_annotated(c: &Content) -> Annotated {
    match c.codec {
        Codec::Rich => to_markdown_annotated(c),
        Codec::Plain => Annotated { markdown: to_plaintext(c), anchors: anchors_where(c, |_| true) },
    }
}

/// The anchors [`to_markdown_annotated`] lists whose id `keep` holds.
pub fn anchors_where(c: &Content, keep: impl Fn(&str) -> bool) -> Vec<AnchorRead> {
    lines::export::anchors_where(&to_lines(c), keep)
}

/// The text with every block break a blank line and every hard break a `\n`,
/// island slots removed: what [`from_plaintext`] reads back, so
/// `plain(to_plaintext(v)) == v` for a plain value `v` whose every hard break
/// stands between two characters of its paragraph.
pub fn to_plaintext(c: &Content) -> String {
    let mut out = String::with_capacity(c.text.len());
    for ch in c.text.chars() {
        match ch {
            '\n' => out.push_str("\n\n"),
            HARD_BREAK => out.push('\n'),
            ISLAND_SLOT => {}
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NOTE_SLOT;

    #[test]
    fn a_hard_break_is_one_block() {
        let c = from_markdown("one\\\ntwo\n\nthree").unwrap().content;
        assert_eq!(c.text, format!("one{HARD_BREAK}two\nthree"));
        assert_eq!(c.blocks.len(), 2);
        assert_eq!(from_markdown(&to_markdown(&c)).unwrap().content, c);
    }

    /// A table cell is one paragraph: a paragraph break in one reads as a hard
    /// break from either table spelling, a value holding one is refused at a
    /// write, joined on a stored read and by repair. A footnote's note is one
    /// paragraph the same way.
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
        let (repaired, repairs, _) = repair(Raw::from_value(&stored).unwrap());
        assert_eq!(repaired, pipe);
        assert!(matches!(repairs.as_slice(), [r @ Repair::Invalid(inv)] if *inv == refused && r.inserts()));

        let note = from_markdown("a[^1]\n\n[^1]: one\n\n    two").unwrap().content;
        let mut noted = note.to_value();
        assert_eq!(note.islands[0].slots[NOTE_SLOT].text, joined);
        for md in ["a[^1]\n\n[^1]: one<br><br>two", "a[^1]\n\n[^1]: one<br>\n\n    two"] {
            let c = from_markdown(md).unwrap().content;
            let slot = &c.islands[0].slots[NOTE_SLOT];
            assert_eq!((slot.text.as_str(), slot.blocks.len()), (joined.as_str(), 1), "{md:?}");
            assert_eq!(from_markdown(&to_markdown(&c)).unwrap().content, c);
        }
        noted["islands"][0]["slots"][NOTE_SLOT] = stored["islands"][0]["slots"]["s1"].clone();
        let refused = Invariant::BreakInCell { island: "isl-0".into(), slot: NOTE_SLOT.into() };
        assert_eq!(Content::from_value(&noted), Err(ParseError::Invalid(refused.clone())));
        assert_eq!(Content::from_value_lenient(&noted).unwrap().content, note);
        let (repaired, repairs, _) = repair(Raw::from_value(&noted).unwrap());
        assert_eq!(repaired, note);
        assert!(matches!(repairs.as_slice(), [Repair::Invalid(inv)] if *inv == refused));

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
