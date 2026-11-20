//! What a Word export carries less of: one code per kind of loss, with its
//! message, and the tally the reader and lowering add to.

use std::collections::BTreeMap;

use crate::layout::Notice;

/// A kind of notice. Two kinds can share a diagnostic code and differ in
/// their message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    HintUnknown,
    HintUnclosed,
    BulletReplaced,
    NumbersPrinted,
    MarginsAssumed,
    Rasterized,
    PictureFailed,
    EquationText,
    LinkInternal,
    TocFrozen,
    LabelMismatch,
    PageNumberFrozen,
    LinkScript,
    UnstructuredText,
    GridUntabbed,
    ColumnsFlattened,
    DirectionDropped,
    HintMisused,
    FontNotEmbedded,
    PlaceAnchored,
    ShapeAnchored,
    ShapeDropped,
    InkDropped,
    ProbesDisabled,
    FormFieldDeclined,
}

impl Code {
    /// The diagnostic code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::HintUnknown => "docx::hint_unknown",
            Code::HintUnclosed => "docx::hint_unclosed",
            Code::BulletReplaced | Code::NumbersPrinted => "docx::list_marker_replaced",
            Code::MarginsAssumed => "docx::margins_assumed",
            Code::Rasterized => "docx::rasterized",
            Code::PictureFailed => "docx::picture_failed",
            Code::EquationText => "docx::equation_text",
            Code::LinkInternal => "docx::link_internal",
            Code::TocFrozen => "docx::toc_frozen",
            Code::LabelMismatch => "docx::label_mismatch",
            Code::PageNumberFrozen => "docx::page_number_frozen",
            Code::LinkScript => "docx::link_script",
            Code::UnstructuredText => "docx::unstructured_text",
            Code::GridUntabbed | Code::HintMisused => "docx::hint_unchecked",
            Code::ColumnsFlattened => "docx::columns_flattened",
            Code::DirectionDropped => "docx::direction_dropped",
            Code::FontNotEmbedded => "docx::font_not_embedded",
            Code::PlaceAnchored | Code::ShapeAnchored => "docx::place_anchored",
            Code::ShapeDropped => "docx::shape_dropped",
            Code::InkDropped => "docx::ink_dropped",
            Code::ProbesDisabled => "docx::probes_disabled",
            Code::FormFieldDeclined => "docx::form_field_declined",
        }
    }

    /// The message for `n` occurrences, about `name` for a kind that names
    /// what it is about.
    pub fn message(self, name: &str, n: usize) -> String {
        match self {
            Code::HintUnknown => {
                format!("the hint {name:?} is not one the Word export knows, so it is ignored")
            }
            Code::HintUnclosed => {
                "a hint bracket is still open where its element ends, so it closes there".into()
            }
            Code::BulletReplaced => {
                "a list's marker is not text, so its items carry a bullet".into()
            }
            Code::NumbersPrinted => "a numbered list's numbering is not a pattern Word can count, \
                                     so its numbers are kept as printed"
                .into(),
            Code::MarginsAssumed => {
                format!("{n} page(s) carry no page probe, so their margins are taken as 1 in")
            }
            Code::Rasterized => format!(
                "{n} equation(s), curve(s), gradient(s), vector image(s), or rotated, mirrored or \
                 clipped group(s) are carried as pictures"
            ),
            Code::PictureFailed => format!(
                "{n} rotated, clipped or drawn group(s) on the page could not be carried as \
                 pictures, so their ink is read as it is laid out, unrotated and unclipped"
            ),
            Code::EquationText => format!(
                "{n} equation(s) that wrap across lines or frames are carried as their glyphs' \
                 text, not as pictures"
            ),
            Code::LinkInternal => {
                format!("{n} link(s) to a place in the document are carried as plain text")
            }
            Code::TocFrozen => format!(
                "{n} table(s) of contents print page numbers Word's PAGE field cannot, so they \
                 are locked against an update"
            ),
            Code::LabelMismatch => format!(
                "{n} paragraph number(s) are not the number Word would count, so they stay as \
                 printed text"
            ),
            Code::PageNumberFrozen => format!(
                "{n} printed page number(s) are not literal text around a number Word can count, \
                 so they stay as printed text"
            ),
            Code::LinkScript => format!("{n} link(s) that run script are carried as plain text"),
            Code::UnstructuredText => format!(
                "{n} line(s) of text that no element tags are set as a paragraph each, as printed"
            ),
            Code::GridUntabbed => format!(
                "{n} grid(s) marked as tabbed lines wrap a cell before the last or hold a table, \
                 so they stay tables"
            ),
            Code::ColumnsFlattened => {
                format!("{n} block(s) set in columns within the flow are written as one column")
            }
            Code::DirectionDropped => format!(
                "{n} paragraph(s) hold right-to-left text, which the Word file sets with no text \
                 direction"
            ),
            Code::HintMisused => format!(
                "{n} hint(s) mark what they cannot apply to: a label outside any list, a layout \
                 around no grid, or chrome around no place, so the export ignores them"
            ),
            Code::FontNotEmbedded => format!(
                "the Word file names the font {name} without embedding it, so Word substitutes \
                 another where it is not installed"
            ),
            Code::PlaceAnchored => format!(
                "{n} placed object(s) are anchored to the page beside the paragraph that follows \
                 them, so they do not move with the text"
            ),
            Code::ShapeAnchored => format!(
                "{n} drawn line(s) or box(es) are anchored to the page beside the paragraph that \
                 follows them, so they do not move with the text"
            ),
            Code::ShapeDropped => format!(
                "{n} drawn line(s) or box(es) that no table or decoration explains are left out \
                 of the Word file"
            ),
            Code::InkDropped => {
                format!("{n} piece(s) of text or image are left out of the Word file")
            }
            Code::ProbesDisabled => "the probed compile did not lay out the document's ink, so \
                                     the Word file is read without its probes and carries less \
                                     structure"
                .into(),
            Code::FormFieldDeclined => format!(
                "{n} form or signature field(s) the PDF carries as widgets have no Word \
                 counterpart and are left out"
            ),
        }
    }
}

/// The notices of one export. A raised notice is reported once, in the
/// order notices are first raised; a counted one is reported with its count,
/// in the order of [`Code`] and then of what it names, after every raised
/// one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tally {
    raised: Vec<(Code, String)>,
    counted: BTreeMap<(Code, String), usize>,
}

impl Tally {
    pub fn raise(&mut self, code: Code, name: &str) {
        if !self.raised.iter().any(|(c, n)| *c == code && n == name) {
            self.raised.push((code, name.to_string()));
        }
    }

    /// Adds `n` to the count of `code` about `name`; a count of none is not
    /// reported.
    pub fn count(&mut self, code: Code, name: &str, n: usize) {
        if n > 0 {
            *self.counted.entry((code, name.to_string())).or_default() += n;
        }
    }

    pub fn notices(&self) -> Vec<Notice> {
        self.raised
            .iter()
            .map(|(code, name)| (code, name, 1))
            .chain(
                self.counted
                    .iter()
                    .map(|((code, name), n)| (code, name, *n)),
            )
            .map(|(code, name, n)| Notice {
                code: code.as_str().into(),
                message: code.message(name, n),
            })
            .collect()
    }
}
