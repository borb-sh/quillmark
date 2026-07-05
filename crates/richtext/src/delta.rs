//! The per-field edit surface: a [`Delta`] (Quill-Delta semantics) over the USV
//! corpus, plus the **stale-text writer** path — cold-parse a full new markdown
//! document, char-diff it against the base, and rebase the base's identity marks
//! (anchors/comments) through the diff so annotations survive an LLM
//! full-document rewrite with no preservation contract on the LLM.
//!
//! Phase 1 delivers the diff + rebase + a **move detector**; the live delta
//! transport (revision, bounded change log) is phase 3. Position mapping follows
//! CodeMirror's `ChangeDesc.mapPos` / ProseMirror mapping semantics.
//!
//! ## The move weak spot (documented limit)
//!
//! A paragraph reorder is delete-here + insert-there to any char differ, so a
//! naive rebase collapses an anchor in the moved text to the deletion point. The
//! detector re-homes an anchor onto a **single, verbatim block move** by locating
//! the moved text in the new corpus. Text both *moved and rewritten* in one round
//! (the match is lost) drops the anchor — the accepted residual, stated not
//! hidden. Tightening verbatim → fuzzy (longest-common-substring) is a hardening
//! follow-up.

use crate::model::{Mark, MarkKind, RichText};

/// A per-field edit against a base corpus. Ops apply left-to-right, consuming
/// base positions; `Retain`/`Delete` advance the base cursor, `Insert` adds new
/// text. USV throughout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    pub ops: Vec<Op>,
}

/// One delta operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Keep `n` chars of the base unchanged.
    Retain(usize),
    /// Insert this text at the cursor.
    Insert(String),
    /// Drop `n` chars of the base.
    Delete(usize),
}

/// Which side of a same-position insertion a mapped point lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    /// Stay before inserted text.
    Before,
    /// Move after inserted text.
    After,
}

impl Delta {
    /// Apply to `base`, producing the new text. Ignores over-long
    /// `Retain`/`Delete` gracefully (clamps), so a mismatched delta cannot
    /// panic.
    pub fn apply(&self, base: &str) -> String {
        let chars: Vec<char> = base.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    let end = (i + n).min(chars.len());
                    out.extend(&chars[i..end]);
                    i = end;
                }
                Op::Delete(n) => {
                    i = (i + n).min(chars.len());
                }
                Op::Insert(s) => out.push_str(s),
            }
        }
        out.extend(&chars[i.min(chars.len())..]);
        out
    }

    /// Map a base char position to its new position. `assoc` decides the side of
    /// a same-position insertion (`After` moves past it).
    pub fn map_pos(&self, pos: usize, assoc: Assoc) -> usize {
        let mut old = 0usize;
        let mut new = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    if pos <= old + n {
                        return new + (pos - old);
                    }
                    old += n;
                    new += n;
                }
                Op::Delete(n) => {
                    if pos < old + n {
                        // Inside (or at the start of) the deletion — collapse to
                        // the deletion point.
                        return new;
                    }
                    old += n;
                }
                Op::Insert(s) => {
                    let len = s.chars().count();
                    if pos == old {
                        match assoc {
                            Assoc::Before => return new,
                            Assoc::After => new += len, // fall through past insert
                        }
                    } else {
                        new += len;
                    }
                }
            }
        }
        new
    }

    /// Whether base position `pos` sits strictly inside a deleted span (used to
    /// detect a collapsed zero-width anchor).
    fn is_deleted(&self, pos: usize) -> bool {
        let mut old = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => old += n,
                Op::Delete(n) => {
                    if pos >= old && pos < old + n {
                        return true;
                    }
                    old += n;
                }
                Op::Insert(_) => {}
            }
        }
        false
    }
}

/// Char-level diff via common-prefix / common-suffix trim: the change is one
/// `Delete` of the differing base middle and one `Insert` of the new middle.
/// Coarse but valid — the move detector recovers relocations from the new text
/// directly, so a finer diff is not needed for phase-1 anchor rebase.
pub fn diff(base: &str, new: &str) -> Delta {
    let a: Vec<char> = base.chars().collect();
    let b: Vec<char> = new.chars().collect();

    let mut p = 0usize;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0usize;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }

    let mut ops = Vec::new();
    if p > 0 {
        ops.push(Op::Retain(p));
    }
    let del = a.len() - p - s;
    if del > 0 {
        ops.push(Op::Delete(del));
    }
    let ins: String = b[p..b.len() - s].iter().collect();
    if !ins.is_empty() {
        ops.push(Op::Insert(ins));
    }
    if s > 0 {
        ops.push(Op::Retain(s));
    }
    Delta { ops }
}

/// The stale-text writer path: cold-parse `new_markdown`, char-diff it against
/// `base`, and carry `base`'s identity marks (anchors) forward, rebased through
/// the diff (re-homing verbatim block moves). The returned corpus is `new_rt`
/// (structure/marks/islands from the fresh import) plus the surviving anchors.
///
/// Returns the new corpus and the [`Delta`] used (the change log entry a phase-3
/// revision would record).
pub fn diff_import(
    base: &RichText,
    new_markdown: &str,
) -> Result<(RichText, Delta), crate::import::ImportError> {
    let mut new_rt = crate::import::from_markdown(new_markdown)?;
    let delta = diff(&base.text, &new_rt.text);

    let new_chars: Vec<char> = new_rt.text.chars().collect();
    for m in &base.marks {
        // Only identity marks live in the corpus but not in markdown; formatting
        // marks are re-derived by the fresh import, so we do not carry them.
        let MarkKind::Anchor { .. } = &m.kind else {
            continue;
        };
        if let Some((ns, ne)) = rebase_anchor(&base.text, &delta, &new_chars, m) {
            new_rt.marks.push(Mark {
                start: ns,
                end: ne,
                kind: m.kind.clone(),
            });
        }
        // else: detached — the accepted residual drop.
    }
    new_rt.normalize();
    Ok((new_rt, delta))
}

/// Rebase one anchor through the delta. Returns its new range, or `None` if it
/// detaches (its text was deleted and no verbatim move re-homes it).
fn rebase_anchor(
    base_text: &str,
    delta: &Delta,
    new_chars: &[char],
    m: &Mark,
) -> Option<(usize, usize)> {
    if m.start == m.end {
        // Zero-width point anchor.
        if !delta.is_deleted(m.start) {
            return Some((
                delta.map_pos(m.start, Assoc::Before),
                delta.map_pos(m.start, Assoc::Before),
            ));
        }
        // Context relocation: place it at the same offset inside its
        // surrounding text if that context survived the move.
        return relocate_point(base_text, new_chars, m.start);
    }

    let ns = delta.map_pos(m.start, Assoc::After);
    let ne = delta.map_pos(m.end, Assoc::Before);
    if ns < ne {
        return Some((ns, ne)); // survived a surrounding edit
    }
    // Collapsed — try a verbatim block move: find the annotated span in the new
    // corpus.
    relocate_span(base_text, new_chars, m.start, m.end)
}

fn relocate_span(
    base_text: &str,
    new_chars: &[char],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let base_chars: Vec<char> = base_text.chars().collect();
    if end > base_chars.len() {
        return None;
    }
    let needle = &base_chars[start..end];
    find_subslice(new_chars, needle).map(|pos| (pos, pos + needle.len()))
}

fn relocate_point(base_text: &str, new_chars: &[char], pos: usize) -> Option<(usize, usize)> {
    const K: usize = 24;
    let base_chars: Vec<char> = base_text.chars().collect();
    // Prefer left context (text immediately before the point); fall back to
    // right context. The point lands at the boundary between them.
    let l0 = pos.saturating_sub(K);
    let left = &base_chars[l0..pos];
    if !left.is_empty() {
        if let Some(p) = find_subslice(new_chars, left) {
            return Some((p + left.len(), p + left.len()));
        }
    }
    let r1 = (pos + K).min(base_chars.len());
    let right = &base_chars[pos..r1];
    if !right.is_empty() {
        if let Some(p) = find_subslice(new_chars, right) {
            return Some((p, p));
        }
    }
    None
}

/// First index where `needle` occurs in `hay` (char slices). `None` if absent or
/// empty needle.
fn find_subslice(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::from_markdown;
    use crate::model::MarkKind;

    #[test]
    fn diff_apply_round_trips() {
        let d = diff("the quick brown fox", "the slow brown fox");
        assert_eq!(d.apply("the quick brown fox"), "the slow brown fox");
    }

    #[test]
    fn map_pos_insertion() {
        // Insert "XY" at position 3 of "abcdef".
        let d = diff("abcdef", "abcXYdef");
        assert_eq!(d.apply("abcdef"), "abcXYdef");
        // A point before the insert is unmoved; after it shifts by 2.
        assert_eq!(d.map_pos(2, Assoc::After), 2);
        assert_eq!(d.map_pos(4, Assoc::Before), 6);
    }

    #[test]
    fn anchor_survives_edit_elsewhere() {
        // Anchor on "target" (chars 7..13); edit happens before it.
        let mut base = from_markdown("hello, target word").unwrap();
        base.marks.push(Mark {
            start: 7,
            end: 13,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        let (new_rt, _) = diff_import(&base, "why hello, target word").unwrap();
        let anchor = new_rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("anchor preserved");
        assert_eq!(
            new_rt.text[byte(&new_rt.text, anchor.start)..byte(&new_rt.text, anchor.end)].to_string(),
            "target"
        );
    }

    #[test]
    fn anchor_rehomed_on_block_move() {
        // Two paragraphs; anchor on the first; the rewrite swaps their order.
        let mut base = from_markdown("first para here\n\nsecond para here").unwrap();
        // "first para here" is chars 0..15
        base.marks.push(Mark {
            start: 0,
            end: 15,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        let (new_rt, _) = diff_import(&base, "second para here\n\nfirst para here").unwrap();
        let anchor = new_rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("anchor re-homed onto moved block");
        assert_eq!(
            new_rt.text[byte(&new_rt.text, anchor.start)..byte(&new_rt.text, anchor.end)].to_string(),
            "first para here"
        );
    }

    #[test]
    fn anchor_dropped_when_text_deleted() {
        let mut base = from_markdown("keep this and drop that").unwrap();
        // Anchor on "drop that" (14..23).
        base.marks.push(Mark {
            start: 14,
            end: 23,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        let (new_rt, _) = diff_import(&base, "keep this").unwrap();
        assert!(
            !new_rt
                .marks
                .iter()
                .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")),
            "anchor on deleted text detaches (accepted residual)"
        );
    }

    fn byte(s: &str, char_idx: usize) -> usize {
        crate::usv::char_to_byte(s, char_idx)
    }
}
