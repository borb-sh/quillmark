//! The line form's markdown export: each call lifts the value onto the
//! content model with [`crate::migrate::from_lines`] and exports it there
//! ([`crate::markdown`]). [`to_plaintext`], [`char_count`] and
//! [`line_segments`] read the line form itself.

use crate::lines::model::{Content, ISLAND_SLOT, Lines};
use crate::migrate::from_lines;

pub use crate::export::{clip_range_to_atomic, tex_is_writable, url_is_writable};
pub use crate::markdown::{AnchorRead, Annotated};

/// Render a content to markdown.
pub fn to_markdown(rt: &Lines) -> String {
    crate::export::to_markdown(&from_lines(rt))
}

/// The two-way read: [`crate::markdown::to_markdown_annotated`].
pub fn to_markdown_annotated(rt: &Lines) -> Annotated {
    crate::markdown::to_markdown_annotated(&from_lines(rt))
}

/// Render a content to plaintext: [`Content::text`] with island slots
/// ([`ISLAND_SLOT`]) removed. The lossy sibling of [`to_markdown`]: it drops
/// every mark and island, keeping only literal text.
///
/// Tables and images having no plaintext form is a **decided limitation**: the
/// acroform backend fills a form field from this projection, so a field bound to
/// a table-bearing content renders the surrounding text and omits the table,
/// under a `backend::declined_construct` warning, rather than emitting a
/// row/tab dump that would read as faithful.
pub fn to_plaintext(rt: &Content) -> String {
    rt.text.chars().filter(|&c| c != ISLAND_SLOT).collect()
}

/// The character count a text cap (`max:`) measures on a plaintext projection:
/// Unicode scalar values, trailing line breaks aside.
pub fn char_count(text: &str) -> usize {
    text.trim_end_matches(['\n', '\r']).chars().count()
}

/// One line's char range `[start, end)` into the content, with the matching byte
/// range and the count of island slots before the line, so a caller indexes the
/// content text and the island list in O(1) without rescanning. A derived view
/// [`line_segments`] recomputes whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// scalar index of the line's first char.
    pub start: usize,
    /// scalar index one past the line's last char (its `\n`, or the content end).
    pub end: usize,
    /// Byte offset of `start` in the content text.
    pub byte_start: usize,
    /// Byte offset of `end` in the content text.
    pub byte_end: usize,
    /// Count of [`ISLAND_SLOT`] chars before `start`.
    pub slots_before: usize,
}

/// Per-line char/byte ranges and slot prefixes over a content, in line order.
pub fn line_segments(rt: &Content) -> Vec<Segment> {
    let mut segs = Vec::with_capacity(rt.lines.len());
    let mut start = 0usize;
    let mut byte_start = 0usize;
    let mut slots_before = 0usize;
    let mut line_slots = 0usize;
    let mut pos = 0usize;
    for (b, c) in rt.text.char_indices() {
        if c == '\n' {
            segs.push(Segment {
                start,
                end: pos,
                byte_start,
                byte_end: b,
                slots_before,
            });
            start = pos + 1;
            byte_start = b + 1; // `\n` is one byte
            slots_before += line_slots;
            line_slots = 0;
        } else if c == ISLAND_SLOT {
            line_slots += 1;
        }
        pos += 1;
    }
    segs.push(Segment {
        start,
        end: pos,
        byte_start,
        byte_end: rt.text.len(),
        slots_before,
    });
    // Defensive: a malformed content (lines.len() != segments) still gets one
    // segment per line so indexing never panics.
    let total_slots = slots_before + line_slots;
    while segs.len() < rt.lines.len() {
        segs.push(Segment {
            start: pos,
            end: pos,
            byte_start: rt.text.len(),
            byte_end: rt.text.len(),
            slots_before: total_slots,
        });
    }
    segs
}
