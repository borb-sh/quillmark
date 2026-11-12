//! The char diff a markdown revise carries anchors through: a [`Delta`] of
//! text splices from the stored text to the rewrite's, which
//! [`carry`](crate::carry) maps each stored anchor across, so a writer
//! rewriting a value whole keeps its comments with no contract to keep them.
//!
//! ## Text splices, not attributed ops
//!
//! [`Delta`] is `retain` / `insert` / `delete` over the character sequence:
//! CodeMirror `ChangeSet` / OT text semantics, **not** Quill-Delta. It carries
//! no formatting attributes: marks and islands are separate `(range, kind)` data
//! that *map through* a delta ([`Delta::map_pos`]). An attribute map is a
//! per-character property map and cannot represent overlapping same-kind marks
//! or two distinct identity anchors over one range, the algebra the content
//! model keeps.
//!
//! [`diff`] computes a Myers/LCS minimal edit script and pairs it with a
//! **move detector** that re-homes an anchor across a verbatim block move.
//! Position mapping follows CodeMirror's `ChangeDesc.mapPos` semantics.
//! The delta is internal to the revise and the markdown export; the content
//! write surface speaks `Change`.
//!
//! ## The move weak spot
//!
//! A paragraph reorder is delete-here + insert-there to any char differ, so a
//! naive carry collapses an anchor in the moved text to the deletion point. The
//! detector re-homes an anchor onto a **single, verbatim block move** by
//! locating the moved text in the new content. Text both *moved and rewritten*
//! in one round loses the match, and the anchor with it: the accepted
//! residual.

use similar::{Algorithm, ChangeTag, DiffOp, TextDiff, capture_diff_slices};
use std::collections::HashSet;

/// A per-field edit against a base content. Ops apply left-to-right, consuming
/// base positions; `Retain`/`Delete` advance the base cursor, `Insert` adds new
/// text. scalar throughout.
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
    /// Map a base char position to its new position. `assoc` decides the side of
    /// a same-position insertion (`After` moves past it).
    ///
    /// The base cursor saturates: an op run summing past `usize` describes a
    /// base longer than any content, so a position lands inside it as it would
    /// in a bounded one.
    pub fn map_pos(&self, pos: usize, assoc: Assoc) -> usize {
        let mut old = 0usize;
        let mut new = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    // Strictly inside the retain resolves here; the right
                    // boundary (pos == old + n) falls through, so a following
                    // Insert can apply its `assoc`.
                    if pos < old.saturating_add(*n) {
                        return new.saturating_add(pos - old);
                    }
                    old = old.saturating_add(*n);
                    new = new.saturating_add(*n);
                }
                Op::Delete(n) => {
                    if pos < old.saturating_add(*n) {
                        // Inside (or at the start of) the deletion: collapse to
                        // the deletion point.
                        return new;
                    }
                    old = old.saturating_add(*n);
                }
                Op::Insert(s) => {
                    let len = s.chars().count();
                    if pos == old {
                        match assoc {
                            Assoc::Before => return new,
                            Assoc::After => new = new.saturating_add(len), // fall through past insert
                        }
                    } else {
                        new = new.saturating_add(len);
                    }
                }
            }
        }
        new.saturating_add(pos.saturating_sub(old))
    }

    /// Whether base position `pos` sits strictly inside a deleted span. The
    /// deletion's left edge (`pos == old`) survives (a point anchor there stays
    /// put) so only `old < pos < old + n` counts as deleted.
    fn is_deleted(&self, pos: usize) -> bool {
        let mut old = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => old = old.saturating_add(*n),
                Op::Delete(n) => {
                    if pos > old && pos < old.saturating_add(*n) {
                        return true;
                    }
                    old = old.saturating_add(*n);
                }
                Op::Insert(_) => {}
            }
        }
        false
    }

    /// New-text char ranges covered by `Insert` ops: the only regions an anchor
    /// may be re-homed into (moved text must have been *inserted*, not merely
    /// present in surviving text elsewhere), and the hint
    /// [`locate_site`](crate::anchors::locate_site) takes for the same discipline.
    pub fn inserted_spans(&self) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut new = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => new = new.saturating_add(*n),
                Op::Insert(s) => {
                    let len = s.chars().count();
                    if len > 0 {
                        spans.push((new, new.saturating_add(len)));
                    }
                    new = new.saturating_add(len);
                }
                Op::Delete(_) => {}
            }
        }
        spans
    }
}

/// A relocation match shorter than this many chars is too weak to trust: the
/// verbatim-move detector's length floor.
pub(crate) const MIN_MOVE: usize = 4;

/// Above this many scalar chars, a char diff skips `similar`'s
/// char-level Myers diff and falls back to [`coarse_replace`].
/// `TextDiff::from_chars` is O(N·D) with no deadline; on two long, unrelated
/// single-line strings D grows with N, so cost is effectively quadratic (two
/// unrelated 30,000-char lines measured 86s in a debug build). This threshold
/// keeps 6x headroom under that while still covering a real single-paragraph
/// field. A fixed char budget rather than `TextDiffConfig::timeout`, so the
/// cutoff is deterministic in CI.
const CHAR_DIFF_LIMIT: usize = 5_000;

/// Char-level Myers/LCS diff over scalar: a minimal `Retain` / `Delete` / `Insert`
/// script. Disjoint edits stay separate ops rather than collapsing the span
/// between them into one delete+insert, so anchors sitting in unchanged middle
/// text survive the carry without relying on the move detector.
///
/// Single-line text diffs at char granularity; multi-line text diffs at line
/// granularity so a paragraph reorder surfaces as whole-line insert spans the
/// move detector can match (char Myers fragments reordered blocks). Within a
/// run of replaced lines, a line paired with a rewrite of itself
/// diffs by char, so an edit to one word of a paragraph
/// leaves the rest of it retained. Above `CHAR_DIFF_LIMIT` chars, a char
/// diff skips Myers entirely and uses `coarse_replace` instead.
pub fn diff(base: &str, new: &str) -> Delta {
    diff_at(base, new, Grain::Char)
}

/// [`diff`] with a rewritten line, or a text of one line, diffed by word
/// rather than by char: a word either side keeps whole or changes whole, so a
/// range over a rewritten word does not ride its scattered letters.
pub fn diff_words(base: &str, new: &str) -> Delta {
    diff_at(base, new, Grain::Word)
}

#[derive(Clone, Copy)]
enum Grain {
    Char,
    Word,
}

fn diff_at(base: &str, new: &str, grain: Grain) -> Delta {
    let mut ops = Vec::new();
    if base.contains('\n') || new.contains('\n') {
        let old_lines: Vec<&str> = base.split_inclusive('\n').collect();
        let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
        let old_set: HashSet<&str> = old_lines.iter().map(|l| line_text(l)).collect();
        let new_set: HashSet<&str> = new_lines.iter().map(|l| line_text(l)).collect();
        for op in capture_diff_slices(Algorithm::Myers, &old_lines, &new_lines) {
            let (old, new) = (op.old_range(), op.new_range());
            match op {
                DiffOp::Equal { .. } => push_retain(
                    &mut ops,
                    old_lines[old].iter().map(|l| l.chars().count()).sum(),
                ),
                _ => refine_replace(
                    &mut ops,
                    &old_lines[old],
                    &new_lines[new],
                    &old_set,
                    &new_set,
                    grain,
                ),
            }
        }
    } else {
        push_grain_diff(&mut ops, base, new, grain);
    }
    Delta { ops }
}

/// Emit one run of replaced lines. Lines pair in order, the pairing with the
/// greatest total similarity ([`pair_lines`]), where each pair is a rewrite:
/// neither is longer than [`CHAR_DIFF_LIMIT`] chars, the two clear
/// [`MIN_WORD_SIMILARITY`], and neither occurs whole on the other side, as a
/// moved line does. A paired line diffs by its grain, and an unpaired line is
/// deleted or inserted whole.
fn refine_replace(
    ops: &mut Vec<Op>,
    old: &[&str],
    new: &[&str],
    old_set: &HashSet<&str>,
    new_set: &HashSet<&str>,
    grain: Grain,
) {
    let similarity = |o: &str, n: &str| {
        let fits = o.chars().count() <= CHAR_DIFF_LIMIT
            && n.chars().count() <= CHAR_DIFF_LIMIT
            && !new_set.contains(line_text(o))
            && !old_set.contains(line_text(n));
        let s = if fits {
            word_similarity(&words(o), &words(n))
        } else {
            0.0
        };
        (s >= MIN_WORD_SIMILARITY).then_some(s)
    };
    let (mut i, mut j) = (0, 0);
    for (oi, nj) in pair_lines(old.len(), new.len(), similarity, old, new) {
        for line in &old[i..oi] {
            push_delete(ops, line.chars().count());
        }
        for line in &new[j..nj] {
            push_insert(ops, line);
        }
        push_grain_diff(ops, old[oi], new[nj], grain);
        (i, j) = (oi + 1, nj + 1);
    }
    for line in &old[i..] {
        push_delete(ops, line.chars().count());
    }
    for line in &new[j..] {
        push_insert(ops, line);
    }
}

/// Past this many candidate pairs, a run's lines pair from the front and the
/// back only, as the pairing's cost is one word diff per candidate.
const PAIRING_LIMIT: usize = 4_096;

/// The pairs `(old, new)` of a replaced run, in order, that maximise the
/// total similarity of the pairs, each clearing the threshold; the greedy
/// front-and-back pairing above [`PAIRING_LIMIT`] candidates.
fn pair_lines(
    n: usize,
    m: usize,
    similarity: impl Fn(&str, &str) -> Option<f32>,
    old: &[&str],
    new: &[&str],
) -> Vec<(usize, usize)> {
    if n * m > PAIRING_LIMIT {
        let mut head = 0;
        while head < n.min(m) && similarity(old[head], new[head]).is_some() {
            head += 1;
        }
        let mut tail = 0;
        while tail < (n - head).min(m - head)
            && similarity(old[n - 1 - tail], new[m - 1 - tail]).is_some()
        {
            tail += 1;
        }
        return (0..head)
            .map(|k| (k, k))
            .chain((0..tail).map(|k| (n - tail + k, m - tail + k)))
            .collect();
    }
    // best[i][j]: the greatest total over old[..i] and new[..j].
    let mut best = vec![vec![0.0f32; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            let mut here = best[i - 1][j].max(best[i][j - 1]);
            if let Some(s) = similarity(old[i - 1], new[j - 1]) {
                here = here.max(best[i - 1][j - 1] + s);
            }
            best[i][j] = here;
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        if best[i][j] == best[i - 1][j] {
            i -= 1;
        } else if best[i][j] == best[i][j - 1] {
            j -= 1;
        } else {
            pairs.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        }
    }
    pairs.reverse();
    pairs
}

fn line_text(line: &str) -> &str {
    line.strip_suffix('\n').unwrap_or(line)
}

/// The words a pairing compares: the paired diff's own tokens that hold a
/// letter or digit, so punctuation beside a word never makes it another word.
fn words(text: &str) -> Vec<&str> {
    word_tokens(text)
        .into_iter()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .collect()
}

/// Below this [`word_similarity`], two texts are not a rewrite of one another
/// but different text.
pub const MIN_WORD_SIMILARITY: f32 = 0.5;

/// The share of `a`'s and `b`'s words the two hold in common, in order: twice
/// the common words over both lengths, 1 for two empty texts. A word diff,
/// quadratic in the words at worst.
pub fn word_similarity(a: &[&str], b: &[&str]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let common: usize = capture_diff_slices(Algorithm::Myers, a, b)
        .iter()
        .map(|op| match op {
            DiffOp::Equal { len, .. } => *len,
            _ => 0,
        })
        .sum();
    (2 * common) as f32 / (a.len() + b.len()) as f32
}

fn push_grain_diff(ops: &mut Vec<Op>, base: &str, new: &str, grain: Grain) {
    if base.chars().count() > CHAR_DIFF_LIMIT || new.chars().count() > CHAR_DIFF_LIMIT {
        return coarse_replace(ops, base, new);
    }
    let tokens;
    let diff = match grain {
        Grain::Char => TextDiff::from_chars(base, new),
        Grain::Word => {
            tokens = (word_tokens(base), word_tokens(new));
            TextDiff::configure().diff_slices(&tokens.0, &tokens.1)
        }
    };
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => push_retain(ops, change.value().chars().count()),
            ChangeTag::Delete => push_delete(ops, change.value().chars().count()),
            ChangeTag::Insert => push_insert(ops, change.value()),
        }
    }
}

/// `text` as the word diff's tokens: UAX #29 word boundaries, so each
/// punctuation mark is a token of its own and a word changes without the
/// mark beside it, and each island slot alone, so a word beside an island
/// changes without taking the island with it.
fn word_tokens(text: &str) -> Vec<&str> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut out = Vec::new();
    for word in text.split_word_bounds() {
        if word.contains(crate::model::ISLAND_SLOT) {
            let mut start = 0;
            for (at, ch) in word.char_indices() {
                if ch == crate::model::ISLAND_SLOT {
                    if start < at {
                        out.push(&word[start..at]);
                    }
                    out.push(&word[at..at + ch.len_utf8()]);
                    start = at + ch.len_utf8();
                }
            }
            if start < word.len() {
                out.push(&word[start..]);
            }
        } else {
            out.push(word);
        }
    }
    out
}

/// Linear-time fallback for a char diff above [`CHAR_DIFF_LIMIT`]: trims the
/// longest common prefix and suffix and replaces only the middle. Not minimal,
/// but an anchor in the untouched prefix or suffix still maps through a real
/// `Retain`; only one inside the replaced middle depends on the move detector.
fn coarse_replace(ops: &mut Vec<Op>, base: &str, new: &str) {
    let base_chars: Vec<char> = base.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let max_common = base_chars.len().min(new_chars.len());

    let mut prefix = 0;
    while prefix < max_common && base_chars[prefix] == new_chars[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < max_common - prefix
        && base_chars[base_chars.len() - 1 - suffix] == new_chars[new_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }

    push_retain(ops, prefix);
    push_delete(ops, base_chars.len() - prefix - suffix);
    let inserted: String = new_chars[prefix..new_chars.len() - suffix].iter().collect();
    push_insert(ops, &inserted);
    push_retain(ops, suffix);
}

/// `ops` from a base to `new`, each text ending in a newline the caller
/// appended, as the ops between the texts without it: the appended newlines
/// drop, and a char either kept against one is inserted or deleted.
pub(crate) fn without_sentinel(ops: Vec<Op>, new: &str) -> Vec<Op> {
    enum Ev {
        Keep,
        Del,
        Ins(char),
    }
    let new: Vec<char> = new.chars().collect();
    let old_len: usize = ops
        .iter()
        .map(|op| match op {
            Op::Retain(n) | Op::Delete(n) => *n,
            Op::Insert(_) => 0,
        })
        .sum();
    let (mut i, mut j) = (0usize, 0usize);
    let mut evs = Vec::new();
    for op in ops {
        match op {
            Op::Retain(n) => {
                for _ in 0..n {
                    let ev = match (i + 1 == old_len, j + 1 == new.len()) {
                        (true, true) => None,
                        (true, false) => Some(Ev::Ins(new[j])),
                        (false, true) => Some(Ev::Del),
                        (false, false) => Some(Ev::Keep),
                    };
                    evs.extend(ev);
                    i += 1;
                    j += 1;
                }
            }
            Op::Delete(n) => {
                for _ in 0..n {
                    if i + 1 != old_len {
                        evs.push(Ev::Del);
                    }
                    i += 1;
                }
            }
            Op::Insert(text) => {
                for ch in text.chars() {
                    if j + 1 != new.len() {
                        evs.push(Ev::Ins(ch));
                    }
                    j += 1;
                }
            }
        }
    }
    let mut out = Vec::new();
    for ev in evs {
        match ev {
            Ev::Keep => push_retain(&mut out, 1),
            Ev::Del => push_delete(&mut out, 1),
            Ev::Ins(ch) => push_insert(&mut out, ch.encode_utf8(&mut [0; 4])),
        }
    }
    out
}

fn push_retain(ops: &mut Vec<Op>, n: usize) {
    if n == 0 {
        return;
    }
    if let Some(Op::Retain(last)) = ops.last_mut() {
        *last += n;
    } else {
        ops.push(Op::Retain(n));
    }
}

fn push_delete(ops: &mut Vec<Op>, n: usize) {
    if n == 0 {
        return;
    }
    if let Some(Op::Delete(last)) = ops.last_mut() {
        *last += n;
    } else {
        ops.push(Op::Delete(n));
    }
}

fn push_insert(ops: &mut Vec<Op>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Op::Insert(last)) = ops.last_mut() {
        last.push_str(s);
    } else {
        ops.push(Op::Insert(s.to_owned()));
    }
}

/// Where the delta carries `[start, end)` of the base: a point where its
/// position survives, a range while any of it does, and either re-homed where
/// the text it stood in moved verbatim ([`relocate`]). `None` where it
/// detaches: its text was deleted and no move re-homes it.
pub(crate) fn carry_range(
    delta: &Delta,
    base_chars: &[char],
    new_chars: &[char],
    inserted: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    if start == end {
        // Zero-width point mark.
        if !delta.is_deleted(start) {
            let p = delta.map_pos(start, Assoc::Before);
            return Some((p, p));
        }
        // Its surrounding text was deleted: relocate only if that text was
        // re-inserted verbatim elsewhere (a move).
        return relocate_point(base_chars, new_chars, inserted, start);
    }

    let ns = delta.map_pos(start, Assoc::After);
    let ne = delta.map_pos(end, Assoc::Before);
    if ns < ne {
        return Some((ns, ne)); // survived a surrounding edit
    }
    // Collapsed, try a verbatim block move: the annotated span must reappear
    // inside inserted text (not merely somewhere in the surviving content).
    relocate_span(base_chars, new_chars, inserted, start, end)
}

/// Where `new` re-homes `[start, end)` of `base` that the change to `new`
/// deleted: a span or a point moved verbatim into `inserted`, the ranges of
/// `new` the change inserted, at least [`MIN_MOVE`] characters of it, as
/// [`carry_range`] relocates one. `None` where nothing moved it.
pub(crate) fn relocate(
    base: &str,
    new: &str,
    inserted: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let base_chars: Vec<char> = base.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    match start == end {
        true => relocate_point(&base_chars, &new_chars, inserted, start),
        false => relocate_span(&base_chars, &new_chars, inserted, start, end),
    }
}

/// Find the annotated span `base[start..end]` inside an inserted region of the
/// new text. Requires a length floor and containment in inserted text, so an
/// unrelated surviving occurrence of the same words cannot capture the anchor.
fn relocate_span(
    base_chars: &[char],
    new_chars: &[char],
    inserted: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    if end > base_chars.len() {
        return None;
    }
    let needle = &base_chars[start..end];
    find_in_spans(new_chars, needle, inserted).map(|pos| (pos, pos + needle.len()))
}

/// Relocate a point anchor by its left context (text immediately before it),
/// but only if that context reappears inside inserted text: the same
/// move-only, length-floored discipline as [`relocate_span`].
fn relocate_point(
    base_chars: &[char],
    new_chars: &[char],
    inserted: &[(usize, usize)],
    pos: usize,
) -> Option<(usize, usize)> {
    const K: usize = 24;
    let l0 = pos.saturating_sub(K);
    let left = &base_chars[l0..pos];
    if let Some(p) = find_in_spans(new_chars, left, inserted) {
        return Some((p + left.len(), p + left.len()));
    }
    let r1 = (pos + K).min(base_chars.len());
    let right = &base_chars[pos..r1];
    if let Some(p) = find_in_spans(new_chars, right, inserted) {
        return Some((p, p));
    }
    None
}

/// First index where `needle` occurs in `hay` while *overlapping* an inserted
/// span, so the match touches text the rewrite actually inserted. Overlap
/// rather than containment, because a diff can split a moved block across an
/// inserted region and the retained suffix; overlap still rejects an unrelated
/// occurrence sitting entirely in retained text. Enforces [`MIN_MOVE`].
/// O(hay × needle) naive scan.
fn find_in_spans(hay: &[char], needle: &[char], spans: &[(usize, usize)]) -> Option<usize> {
    if needle.len() < MIN_MOVE || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| {
        &hay[i..i + needle.len()] == needle
            && spans.iter().any(|&(s, e)| i < e && i + needle.len() > s)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(d: &Delta, base: &str) -> String {
        let chars: Vec<char> = base.chars().collect();
        let (mut out, mut i) = (String::new(), 0);
        for op in &d.ops {
            match op {
                Op::Retain(n) => {
                    out.extend(&chars[i..i + n]);
                    i += n;
                }
                Op::Delete(n) => i += n,
                Op::Insert(s) => out.push_str(s),
            }
        }
        out.extend(&chars[i..]);
        out
    }

    #[test]
    fn a_line_within_the_char_limit_pairs_by_its_word_diff_at_any_word_count() {
        let middle = vec!["w"; 2_400].join(" ");
        let base = format!("s {middle} e\nend");
        let new = format!("t {middle} f\nend");
        let retained: usize = diff(&base, &new)
            .ops
            .iter()
            .map(|op| match op {
                Op::Retain(n) => *n,
                _ => 0,
            })
            .sum();
        assert!(
            retained > middle.len(),
            "the shared middle is kept, not rewritten"
        );
    }

    #[test]
    fn map_pos_across_an_insertion() {
        let d = diff("abcdef", "abcXYdef");
        assert_eq!(d.map_pos(2, Assoc::After), 2);
        assert_eq!(d.map_pos(4, Assoc::Before), 6);
        assert_eq!(d.map_pos(3, Assoc::After), 5);
        assert_eq!(d.map_pos(3, Assoc::Before), 3);
    }

    /// `mapPos` hands a wire delta here with no length check ahead of it, and
    /// `usize` is 32-bit on wasm32, where a release build wraps and a checked
    /// one aborts the instance. Spelled against `usize::MAX` so the same run
    /// overflows on every width.
    #[test]
    fn map_pos_saturates_on_counts_past_usize() {
        let over = Delta {
            ops: vec![
                Op::Retain(usize::MAX),
                Op::Retain(2),
                Op::Insert("x".into()),
                Op::Delete(3),
            ],
        };
        // A position the first retain covers maps through it.
        assert_eq!(over.map_pos(5, Assoc::After), 5);
        // One at the saturated cursor walks the rest of the run without
        // overflowing, on either side of the insertion.
        assert_eq!(over.map_pos(usize::MAX, Assoc::Before), usize::MAX);
        assert_eq!(over.map_pos(usize::MAX, Assoc::After), usize::MAX);
        assert!(!over.is_deleted(usize::MAX));
        assert_eq!(over.inserted_spans(), vec![(usize::MAX, usize::MAX)]);
    }

    #[test]
    fn point_anchor_at_deletion_left_edge_survives() {
        let d = diff("abcdef", "abef"); // delete "cd" (span [2,4))
        assert!(!d.is_deleted(2), "left edge of deletion survives");
        assert!(d.is_deleted(3), "interior of deletion is deleted");
    }

    #[test]
    fn disjoint_edits_are_separate_ops() {
        let d = diff("aaaMIDDLEbbb", "AAAMIDDLEZZZ");
        assert_eq!(apply(&d, "aaaMIDDLEbbb"), "AAAMIDDLEZZZ");
        let retained: usize = d
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Retain(n) => Some(*n),
                _ => None,
            })
            .sum();
        assert!(
            retained >= 6,
            "unchanged middle span retained ({retained} scalar): {ops:?}",
            ops = d.ops
        );
        assert!(
            !matches!(d.ops.as_slice(), [Op::Delete(_), Op::Insert(_)]),
            "coarse single replace: {ops:?}",
            ops = d.ops
        );
    }

    #[test]
    fn a_long_multi_paragraph_rewrite_stays_fast() {
        let para = |offset: u8| {
            (0..40)
                .map(|k| filler(120, offset.wrapping_add(k)))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let base: String = (0..20).map(|i| para(i)).collect::<Vec<_>>().join("\n");
        let new: String = (0..20).map(|i| para(i + 3)).collect::<Vec<_>>().join("\n");
        let start = std::time::Instant::now();
        let d = diff(&base, &new);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            start.elapsed()
        );
        assert_eq!(apply(&d, &base), new);
    }

    #[test]
    fn a_long_paragraph_of_shared_words_stays_fast() {
        let words = |seed: usize| {
            (0..160_000)
                .map(|k| format!("w{}", (k * 7919 + seed * 104_729) % 1000))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let base = format!("h\n{}\n", words(1));
        let new = format!("h\n{}\n", words(2));
        let start = std::time::Instant::now();
        let d = diff(&base, &new);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            start.elapsed()
        );
        assert_eq!(apply(&d, &base), new);
    }

    /// Deterministic filler with no long common substring between the two
    /// variants: worst case for a char-level Myers diff.
    fn filler(n: usize, offset: u8) -> String {
        (0..n)
            .map(|i| char::from(b'a' + ((i as u8).wrapping_mul(7).wrapping_add(offset)) % 26))
            .collect()
    }

    #[test]
    fn large_single_line_diff_stays_fast() {
        // The shape `similar::TextDiff::from_chars` chokes on with no cutoff.
        let base = format!("PREFIX-{}-BASE-SUFFIX", filler(25_000, 0));
        let new = format!("PREFIX-{}-NEW-SUFFIX", filler(25_000, 13));

        let start = std::time::Instant::now();
        let d = diff(&base, &new);
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "large single-line diff took {elapsed:?}, expected well under the 2s budget"
        );

        assert_eq!(apply(&d, &base), new);
    }

    #[test]
    fn large_single_line_diff_retains_common_prefix_and_suffix() {
        // A shared prefix/suffix around a large rewritten middle must come back
        // as real Retain ops, not one whole-field Delete+Insert.
        let base = format!("shared-prefix-{}-shared-suffix", filler(20_000, 0));
        let new = format!("shared-prefix-{}-shared-suffix", filler(20_000, 5));
        let d = diff(&base, &new);
        assert_eq!(apply(&d, &base), new);

        let Some(Op::Retain(prefix_len)) = d.ops.first() else {
            panic!(
                "expected a leading Retain for the shared prefix: {:?}",
                d.ops
            );
        };
        assert!(
            *prefix_len >= "shared-prefix-".len(),
            "shared prefix should be retained, got Retain({prefix_len})"
        );
        let Some(Op::Retain(suffix_len)) = d.ops.last() else {
            panic!(
                "expected a trailing Retain for the shared suffix: {:?}",
                d.ops
            );
        };
        assert!(
            *suffix_len >= "shared-suffix".len(),
            "shared suffix should be retained, got Retain({suffix_len})"
        );
    }
}
