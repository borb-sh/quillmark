//! Text at the edges, one import and one export per codec. The rich codec
//! imports with [`from_markdown`] and exports with [`to_markdown`], plain, or
//! [`to_markdown_annotated`], which spells every anchor for a revise to read
//! back. The plain codec imports with [`from_plaintext`] ([`plain`] under
//! [`Codec::Plain`]) and exports with [`to_plaintext`]. [`to_text`] and
//! [`to_text_annotated`] export under the value's own codec.

use crate::ids::prefix;
use crate::lines;
pub use crate::lines::import::{DroppedConstruct, ImportError, ImportWarning};
use crate::migrate::from_lines;
use crate::model::Scalar;
use crate::model::{Block, BlockKind, Codec, Content, HARD_BREAK, ISLAND_SLOT, MarkKind};

/// A markdown import: the content and what the markdown spelled that it
/// could not carry.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "carries the import's warnings; read `.warnings` or bind it"]
pub struct Imported {
    pub content: Content,
    pub warnings: Vec<ImportWarning>,
}

/// A [`to_markdown_annotated`] read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotated {
    /// [`to_markdown`]'s markdown with the anchor tags it can hold.
    pub markdown: String,
    /// Every anchor, spelled or not, in the prose and in each island's slots,
    /// in reading order: by the position in the text where each stands, a
    /// slot's at its island's, then prose before slots, by start and id.
    pub anchors: Vec<AnchorRead>,
}

/// One anchor of an [`Annotated`] read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorRead {
    pub id: String,
    /// The text of the line the anchor's start sits on, island slots
    /// removed, or of the table cell or footnote note that holds it: what
    /// locates the anchor where a writer lost its tag.
    pub line: String,
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
    /// A mark the markdown writes over other text, `now`, or not at all.
    Mark {
        kind: MarkKind,
        start: Scalar,
        end: Scalar,
        now: Option<(Scalar, Scalar)>,
    },
    /// An island that reads back with other props, or not at all.
    Island { id: String },
    /// The text or its blocks read back otherwise.
    Text,
    /// A slot whose text the line form spells otherwise: a hard break where
    /// markdown reads a paragraph break (two in a row, or one ending a
    /// paragraph another follows), or one at the slot's edge.
    SlotBreaks { island: String, slot: String },
    /// An `x-` block, which markdown writes as a paragraph.
    ExtBlock { id: String, name: String },
    /// An `x-` mark, which markdown does not write. Positions are the
    /// content's, or the slot's where `slot` names one.
    ExtMark {
        start: Scalar,
        end: Scalar,
        name: String,
        slot: Option<(String, String)>,
    },
}

/// The import: `markdown` read cold, its ids minted positionally and no
/// anchor in it, with an [`ImportWarning`] per construct it dropped: each
/// dropped opening tag, a `<pre>`, `<script>`, `<style>` or `<textarea>`
/// block's included, a block holding no other opening tag under its first
/// where it drops markdown with it, each footnote no reference reads and each
/// equation that stays as its markup's text, and each bidi control the text
/// drops. A comment, the content of a type 1–5 HTML block, and any other
/// closing tag or `qm-anchor` tag drop silently. What a footnote nothing reads
/// holds reports nothing of its own. A `qm-island` token, which only a revise
/// reads, drops reported under `qm-island`.
pub fn from_markdown(markdown: &str) -> Result<Imported, ImportError> {
    let (walked, warnings) = lines::import::walk(markdown, false)?;
    Ok(Imported {
        content: from_lines(&walked),
        warnings,
    })
}

/// [`from_markdown`] with each `qm-island` token dropped silently: the read
/// of a markdown whose tokens a revise of the same markdown resolves once the
/// stored value it revises is known.
pub fn from_markdown_unresolved(markdown: &str) -> Result<Imported, ImportError> {
    let (walked, warnings) = lines::import::walk(markdown, true)?;
    Ok(Imported {
        content: from_lines(&walked),
        warnings,
    })
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
    for c in s
        .chars()
        .filter(|&c| c != ISLAND_SLOT)
        .filter_map(lines::normalize::admit_char)
    {
        match c {
            '\n' => breaks += 1,
            c => {
                flush(&mut text, &mut breaks);
                text.push(c);
            }
        }
    }
    flush(&mut text, &mut breaks);
    let blocks = (0..blocks)
        .map(|n| Block::new(format!("{}{n}", prefix::BLOCK), BlockKind::Para))
        .collect();
    Content {
        text,
        blocks,
        ..Content::empty()
    }
}

/// [`from_plaintext`] under [`Codec::Plain`]: a `text` field's value.
pub fn plain(s: &str) -> Content {
    Content {
        codec: Codec::Plain,
        ..from_plaintext(s)
    }
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
    crate::export::to_markdown(c)
}

/// [`to_markdown`] and each part of `c` it does not spell as itself, read
/// back through [`from_markdown`]. Costs an import beyond the export.
pub fn to_markdown_checked(c: &Content) -> Exported {
    let (markdown, mut losses) = crate::export::to_markdown_checked(c);
    let ext_marks = |c: &Content, slot: Option<(String, String)>| {
        c.marks
            .iter()
            .filter_map(|m| match &m.kind {
                MarkKind::Ext { name, .. } => Some(Loss::ExtMark {
                    start: m.start,
                    end: m.end,
                    name: name.clone(),
                    slot: slot.clone(),
                }),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    losses.extend(c.blocks.iter().filter_map(|b| match &b.kind {
        BlockKind::Ext { name, .. } => Some(Loss::ExtBlock {
            id: b.id.clone(),
            name: name.clone(),
        }),
        _ => None,
    }));
    losses.extend(ext_marks(c, None));
    for island in &c.islands {
        let held = crate::export::held_slots(island);
        for (id, slot) in &island.slots {
            let kept = held.iter().find(|(at, _)| at == id).map(|(_, s)| &s.text);
            if kept != Some(&slot.text) {
                losses.push(Loss::SlotBreaks {
                    island: island.id.clone(),
                    slot: id.clone(),
                });
            }
            losses.extend(ext_marks(slot, Some((island.id.clone(), id.clone()))));
        }
    }
    Exported { markdown, losses }
}

/// The two-way read: [`to_markdown`] with each anchor's tag and each
/// unwritable island's token, and every anchor listed (markdown-spec §6.4),
/// a transient one aside: a revise keeps an anchor its markdown does not
/// spell, so the read need not carry one.
pub fn to_markdown_annotated(c: &Content) -> Annotated {
    crate::export::to_markdown_annotated(&c.without_transient_anchors())
}

/// The annotated read under the value's codec: [`to_markdown_annotated`], or
/// for a [`Codec::Plain`] value its literal text, no tag in it, and every
/// anchor listed but a transient one.
pub fn to_text_annotated(c: &Content) -> Annotated {
    match c.codec {
        Codec::Rich => to_markdown_annotated(c),
        Codec::Plain => Annotated {
            markdown: to_plaintext(c),
            anchors: anchors_where(c, |_| true),
        },
    }
}

/// The anchors [`to_markdown_annotated`] lists whose id `keep` holds.
pub fn anchors_where(c: &Content, keep: impl Fn(&str) -> bool) -> Vec<AnchorRead> {
    crate::export::anchors_where(c, |id| {
        !crate::model::is_transient_anchor_id(id) && keep(id)
    })
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
    use crate::model::{Anchor, Mark, MarkKind, NOTE_SLOT};

    /// Both codecs' reads, a slot's anchors included.
    #[test]
    fn an_annotated_read_leaves_out_a_transient_anchor() {
        let rich = from_markdown("Text with a note[^1].\n\n[^1]: The note.")
            .unwrap()
            .content;
        let mut held = rich.clone();
        held.anchors = vec![Anchor::new("c1", 0, 4), Anchor::new("~w1", 5, 9)];
        let mut kept = held.clone();
        kept.anchors.truncate(1);
        held.islands[0].slots.get_mut(NOTE_SLOT).unwrap().anchors = vec![Anchor::new("~w2", 0, 3)];
        let read = to_text_annotated(&held);
        assert_eq!(read, to_text_annotated(&kept));
        assert!(!read.markdown.contains("~w"), "{}", read.markdown);

        let mut flat = plain("plain words");
        flat.anchors = vec![Anchor::new("~w3", 0, 5)];
        assert_eq!(to_text_annotated(&flat).anchors, []);
    }

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
        let pipe = from_markdown("| h |\n| - |\n| one<br><br>two |")
            .unwrap()
            .content;
        let html = from_markdown(
            "<table><tr><td>h</td></tr><tr><td><p>one</p>\n<p>two</p></td></tr></table>",
        )
        .unwrap();
        assert_eq!(html.warnings, []);
        let joined = format!("one{HARD_BREAK}two");
        for c in [&pipe, &html.content] {
            let cell = &c.islands[0].slots[&c.islands[0].slot_order()[1]];
            assert_eq!(
                (cell.text.as_str(), cell.blocks.len()),
                (joined.as_str(), 1)
            );
            assert_eq!(from_markdown(&to_markdown(c)).unwrap().content, *c);
        }
        let mut edited = pipe.clone();
        edited.islands[0].slots.get_mut("s1").unwrap().text = format!("one{HARD_BREAK}three");
        let change = crate::diff::replace(&pipe, &edited).unwrap();
        assert_eq!(
            crate::change::apply(&pipe, &change).unwrap().content,
            edited
        );

        let mut stored = pipe.to_value();
        stored["islands"][0]["slots"]["s1"]["text"] = "one\ntwo".into();
        stored["islands"][0]["slots"]["s1"]["blocks"] =
            serde_json::json!([{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}]);
        let refused = Invariant::BreakInCell {
            island: "isl-0".into(),
            slot: "s1".into(),
        };
        assert_eq!(
            Content::from_value(&stored),
            Err(ParseError::Invalid(refused.clone()))
        );
        let read = Content::from_value_lenient(&stored).unwrap();
        assert_eq!((read.content.clone(), read.rewritten), (pipe.clone(), true));
        let (repaired, repairs, _) = repair(Raw::from_value(&stored).unwrap());
        assert_eq!(repaired, pipe);
        assert!(
            matches!(repairs.as_slice(), [r @ Repair::Invalid(inv)] if *inv == refused && r.inserts())
        );

        let note = from_markdown("a[^1]\n\n[^1]: one\n\n    two")
            .unwrap()
            .content;
        let mut noted = note.to_value();
        assert_eq!(note.islands[0].slots[NOTE_SLOT].text, joined);
        for md in [
            "a[^1]\n\n[^1]: one<br><br>two",
            "a[^1]\n\n[^1]: one<br>\n\n    two",
        ] {
            let c = from_markdown(md).unwrap().content;
            let slot = &c.islands[0].slots[NOTE_SLOT];
            assert_eq!(
                (slot.text.as_str(), slot.blocks.len()),
                (joined.as_str(), 1),
                "{md:?}"
            );
            assert_eq!(from_markdown(&to_markdown(&c)).unwrap().content, c);
        }
        noted["islands"][0]["slots"][NOTE_SLOT] = stored["islands"][0]["slots"]["s1"].clone();
        let refused = Invariant::BreakInCell {
            island: "isl-0".into(),
            slot: NOTE_SLOT.into(),
        };
        assert_eq!(
            Content::from_value(&noted),
            Err(ParseError::Invalid(refused.clone()))
        );
        assert_eq!(Content::from_value_lenient(&noted).unwrap().content, note);
        let (repaired, repairs, _) = repair(Raw::from_value(&noted).unwrap());
        assert_eq!(repaired, note);
        assert!(matches!(repairs.as_slice(), [Repair::Invalid(inv)] if *inv == refused));

        let mut c = pipe;
        let cell = c.islands[0].slots.get_mut("s1").unwrap();
        cell.text = format!("d{HARD_BREAK}{HARD_BREAK}e");
        let out = to_markdown_checked(&c);
        assert_eq!(
            out.losses,
            [Loss::SlotBreaks {
                island: "isl-0".into(),
                slot: "s1".into()
            }]
        );
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
                Loss::ExtBlock {
                    id: "b0".into(),
                    name: "x-call".into()
                },
                Loss::ExtMark {
                    start: 3,
                    end: 5,
                    name: "x-hi".into(),
                    slot: None
                },
            ]
        );
    }

    /// A `*`, `_` or `~~` mark reaching over whitespace writes over its text
    /// alone, which the receipt names; an underline's tags hold the whitespace.
    #[test]
    fn a_delimited_mark_drops_its_edge_whitespace_and_says_so() {
        let marked = |mark: Mark| {
            let mut c = from_markdown("foo bar baz").unwrap().content;
            c.marks.push(mark);
            c
        };
        for (start, end, kind) in [
            (4, 8, MarkKind::Strong),
            (3, 7, MarkKind::Emph),
            (4, 8, MarkKind::Strike),
        ] {
            let out = to_markdown_checked(&marked(Mark::new(start, end, kind.clone())));
            let back = from_markdown(&out.markdown).unwrap().content;
            assert_eq!(
                back.marks,
                [Mark::new(4, 7, kind.clone())],
                "{:?}",
                out.markdown
            );
            assert_eq!(
                out.losses,
                [Loss::Mark {
                    kind,
                    start,
                    end,
                    now: Some((4, 7))
                }]
            );
        }
        let underlined = marked(Mark::new(4, 8, MarkKind::Underline));
        assert_eq!(to_markdown_checked(&underlined).losses, []);
    }

    /// An import reads back as itself, so its receipt is empty, a table
    /// cell's anchor included.
    #[test]
    fn the_receipt_of_an_import_is_empty() {
        let mut c = from_markdown(
            "# Title\n\nSome **bold** and [a link](https://x.y).\n\n| a |\n| --- |\n| b |",
        )
        .unwrap()
        .content;
        assert_eq!(to_markdown_checked(&c).losses, []);

        let isl = &mut c.islands[0];
        let slot = isl.slots.keys().next().unwrap().clone();
        isl.slots
            .get_mut(&slot)
            .unwrap()
            .anchors
            .push(Anchor::new("c1", 0, 1));
        assert_eq!(c.validate(), Ok(()));
        assert_eq!(
            to_markdown_checked(&c).losses,
            [],
            "a cell's anchor is no loss"
        );
    }
}
