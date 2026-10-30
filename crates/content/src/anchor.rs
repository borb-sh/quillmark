//! What a prose anchor is attached to, and the one exact rule that places a
//! dropped anchor again. The contract is `prose/canon/COMMENTS.md`.
//!
//! Both read the stored text through its slot-stripped projection: the text
//! with every [`ISLAND_SLOT`] removed, each character remembering its stored
//! position. A quote spanning an inline island reads as the text either side
//! of it, and is found again over the same island.

use std::collections::{HashMap, HashSet};

use crate::delta::MIN_MOVE;
use crate::model::{Content, MarkKind, Normalized, Usv, ISLAND_SLOT};
use crate::ops::MarkOp;

/// Context read on each side of an anchor, in Unicode scalar values.
pub const ANCHOR_CONTEXT_USV: usize = 32;

/// Context that must agree on each side for a match. A context the field's
/// edge cut shorter must agree whole, edge included.
pub const ANCHOR_AGREE_USV: usize = 8;

/// A quote shorter than this is too weak to place: the floor `revise` applies
/// to a moved anchor.
pub const MIN_ANCHOR_QUOTE: usize = MIN_MOVE;

/// What a prose anchor is attached to: a text-quote selector read off its
/// field.
///
/// No serde derive, like the rest of the model: the bindings build the wire
/// object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnchorContext {
    pub id: String,
    /// The anchored text, island slots removed and line breaks kept; empty for
    /// a point.
    pub quote: String,
    /// Up to [`ANCHOR_CONTEXT_USV`] scalar values before the anchor, cut at
    /// the field's start.
    pub prefix: String,
    /// Up to [`ANCHOR_CONTEXT_USV`] scalar values after the anchor, cut at the
    /// field's end.
    pub suffix: String,
}

impl AnchorContext {
    pub fn new(
        id: impl Into<String>,
        quote: impl Into<String>,
        prefix: impl Into<String>,
        suffix: impl Into<String>,
    ) -> Self {
        AnchorContext {
            id: id.into(),
            quote: quote.into(),
            prefix: prefix.into(),
            suffix: suffix.into(),
        }
    }
}

/// The receipt of a re-attachment, over whichever selector type the caller
/// gave: each selector in the order given, reattached with its refreshed
/// context or orphaned as it came.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[must_use = "names the anchors it could not place; read `.orphaned` or bind it"]
pub struct Reattached<T> {
    pub reattached: Vec<T>,
    pub orphaned: Vec<T>,
}

impl<T> Default for Reattached<T> {
    fn default() -> Self {
        Reattached {
            reattached: Vec::new(),
            orphaned: Vec::new(),
        }
    }
}

/// Every prose anchor of `rt` with its context, in `(start, id)` order: the
/// order [`anchors_where`](crate::export::anchors_where) lists them. Read off
/// the stored text, so it is pure and moves nothing. An anchor in a table cell
/// is outside the read.
pub fn anchor_contexts(rt: &Normalized) -> Vec<AnchorContext> {
    contexts(rt)
}

/// The one range `ctx` names in `rt`, or `None`. Never a nearest match:
///
/// - A quote shorter than [`MIN_ANCHOR_QUOTE`] names none.
/// - Of every occurrence of the quote in the slot-stripped text, one counts
///   where the text before it ends with the last [`ANCHOR_AGREE_USV`] of
///   `prefix` and the text after it starts with the first
///   [`ANCHOR_AGREE_USV`] of `suffix`. A shorter `prefix` or `suffix` was cut
///   at the field's edge, so it must be the whole text on that side.
/// - An empty quote is a point: a position where the prefix and the suffix
///   both agree.
/// - With `inserted` given, the stored spans a write inserted, a quote counts
///   only where it overlaps one, and a point only where the context it agrees
///   on does: the discipline `revise` keeps when it moves an anchor.
/// - Exactly one counting place names its range; none or several name none.
///
/// The range runs from the quote's first character to its last, over any
/// island slots inside it. A point's range covers the island slots where the
/// prefix meets the suffix, so an anchor over a table's slot alone comes back
/// over that slot, and is empty where none sit.
pub fn locate(
    rt: &Normalized,
    ctx: &AnchorContext,
    inserted: Option<&[(Usv, Usv)]>,
) -> Option<(Usv, Usv)> {
    Stripped::of(rt).locate(ctx, inserted)
}

impl Content {
    /// Re-add each context's anchor where [`locate`] finds it, through the
    /// [`MarkOp::Add`] arm, and answer which landed. An id in `live`, or
    /// already a prose anchor here, is not written and is answered as
    /// reattached at its live range. Never mints an id, never moves a live
    /// anchor, and places each anchor whole or not at all.
    ///
    /// A reattached entry carries its context as read after the write; an
    /// orphaned one, and one live outside this content, carries the context as
    /// given.
    pub fn reattach(
        &mut self,
        ctxs: &[AnchorContext],
        inserted: Option<&[(Usv, Usv)]>,
        live: &HashSet<&str>,
    ) -> Reattached<AnchorContext> {
        let stripped = Stripped::of(self);
        let mut held: HashSet<String> = self
            .marks
            .iter()
            .filter_map(|m| match &m.kind {
                MarkKind::Anchor { id } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let mut placed = Vec::with_capacity(ctxs.len());
        for ctx in ctxs {
            if live.contains(ctx.id.as_str()) || held.contains(&ctx.id) {
                placed.push(true);
                continue;
            }
            let landed = stripped.locate(ctx, inserted).is_some_and(|(start, end)| {
                let add = MarkOp::Add {
                    start,
                    end,
                    kind: MarkKind::Anchor { id: ctx.id.clone() },
                };
                self.apply_mark_ops(&[add]).is_ok()
            });
            if landed {
                held.insert(ctx.id.clone());
            }
            placed.push(landed);
        }
        let mut fresh: HashMap<String, AnchorContext> =
            contexts(self).into_iter().map(|c| (c.id.clone(), c)).collect();
        let mut out = Reattached::default();
        for (ctx, landed) in ctxs.iter().zip(placed) {
            if landed {
                out.reattached.push(fresh.remove(&ctx.id).unwrap_or_else(|| ctx.clone()));
            } else {
                out.orphaned.push(ctx.clone());
            }
        }
        out
    }
}

impl Normalized {
    /// [`Content::reattach`], which leaves the content canonical.
    pub fn reattach(
        &mut self,
        ctxs: &[AnchorContext],
        inserted: Option<&[(Usv, Usv)]>,
        live: &HashSet<&str>,
    ) -> Reattached<AnchorContext> {
        self.as_content_mut().reattach(ctxs, inserted, live)
    }
}

fn contexts(rt: &Content) -> Vec<AnchorContext> {
    let mut found: Vec<(Usv, &str, Usv)> = rt
        .marks
        .iter()
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } => Some((m.start, id.as_str(), m.end)),
            _ => None,
        })
        .collect();
    if found.is_empty() {
        return Vec::new();
    }
    found.sort_unstable();
    let s = Stripped::of(rt);
    found
        .into_iter()
        .map(|(start, id, end)| {
            let (i, j) = (s.index(start), s.index(end));
            AnchorContext {
                id: id.to_string(),
                quote: s.chars[i..j].iter().collect(),
                prefix: s.chars[i.saturating_sub(ANCHOR_CONTEXT_USV)..i].iter().collect(),
                suffix: s.chars[j..(j + ANCHOR_CONTEXT_USV).min(s.chars.len())].iter().collect(),
            }
        })
        .collect()
}

/// The stored text with its island slots removed, and the stored position of
/// each character left.
struct Stripped {
    chars: Vec<char>,
    at: Vec<Usv>,
    len: Usv,
}

impl Stripped {
    fn of(rt: &Content) -> Self {
        let mut chars = Vec::new();
        let mut at = Vec::new();
        let mut len = 0;
        for (pos, c) in rt.text.chars().enumerate() {
            if c != ISLAND_SLOT {
                chars.push(c);
                at.push(pos);
            }
            len = pos + 1;
        }
        Stripped { chars, at, len }
    }

    /// The stripped position of stored position `pos`: the characters before
    /// it.
    fn index(&self, pos: Usv) -> usize {
        self.at.partition_point(|&a| a < pos)
    }

    /// The stored range of stripped `[i, j)`: tight around its characters, or,
    /// empty, the island slots between `i - 1` and `i`.
    fn range(&self, i: usize, j: usize) -> (Usv, Usv) {
        if i < j {
            return (self.at[i], self.at[j - 1] + 1);
        }
        let start = if i == 0 { 0 } else { self.at[i - 1] + 1 };
        let end = self.at.get(i).copied().unwrap_or(self.len);
        (start, end)
    }

    fn locate(&self, ctx: &AnchorContext, inserted: Option<&[(Usv, Usv)]>) -> Option<(Usv, Usv)> {
        let quote: Vec<char> = ctx.quote.chars().collect();
        let prefix: Vec<char> = ctx.prefix.chars().collect();
        let suffix: Vec<char> = ctx.suffix.chars().collect();
        let n = self.chars.len();
        let touches = |(a, b): (Usv, Usv)| {
            inserted.is_none_or(|spans| spans.iter().any(|&(s, e)| a < e && b > s))
        };
        let agrees = |i: usize, j: usize| {
            agrees_before(&self.chars[..i], &prefix) && agrees_after(&self.chars[j..], &suffix)
        };
        let mut hits = Vec::new();
        if quote.is_empty() {
            let (before, after) = (prefix.len().min(ANCHOR_AGREE_USV), suffix.len().min(ANCHOR_AGREE_USV));
            for p in before..=n.saturating_sub(after) {
                if agrees(p, p) && touches(self.range(p - before, p + after)) {
                    hits.push(self.range(p, p));
                    if hits.len() > 1 {
                        return None;
                    }
                }
            }
        } else {
            if quote.len() < MIN_ANCHOR_QUOTE || quote.len() > n {
                return None;
            }
            for i in 0..=n - quote.len() {
                let j = i + quote.len();
                if self.chars[i..j] == quote[..] && agrees(i, j) && touches(self.range(i, j)) {
                    hits.push(self.range(i, j));
                    if hits.len() > 1 {
                        return None;
                    }
                }
            }
        }
        hits.pop()
    }
}

fn agrees_before(before: &[char], prefix: &[char]) -> bool {
    match prefix.len().checked_sub(ANCHOR_AGREE_USV) {
        Some(cut) => before.ends_with(&prefix[cut..]),
        None => before == prefix,
    }
}

fn agrees_after(after: &[char], suffix: &[char]) -> bool {
    if suffix.len() >= ANCHOR_AGREE_USV {
        after.starts_with(&suffix[..ANCHOR_AGREE_USV])
    } else {
        after == suffix
    }
}

#[cfg(test)]
mod tests {
    mod properties;

    use super::*;
    use crate::delta::{diff_import, rebase_onto};
    use crate::import::from_markdown;
    use crate::model::Mark;

    fn content(markdown: &str) -> Normalized {
        from_markdown(markdown).unwrap().content
    }

    fn with_anchor(rt: Normalized, start: Usv, end: Usv, id: &str) -> Normalized {
        let mut rt = rt.into_content();
        rt.marks.push(Mark::new(start, end, MarkKind::Anchor { id: id.into() }));
        rt.into_normalized()
    }

    fn span(rt: &Content, id: &str) -> Option<(Usv, Usv)> {
        rt.marks.iter().find_map(|m| match &m.kind {
            MarkKind::Anchor { id: i } if i == id => Some((m.start, m.end)),
            _ => None,
        })
    }

    fn text(rt: &Content, (start, end): (Usv, Usv)) -> String {
        rt.text.chars().skip(start).take(end - start).collect()
    }

    fn without_anchors(rt: &Normalized) -> Normalized {
        let mut rt = rt.clone().into_content();
        rt.marks.retain(|m| m.kind.is_formatting());
        rt.into_normalized()
    }

    fn none() -> HashSet<&'static str> {
        HashSet::new()
    }

    #[test]
    fn contexts_then_reattach_restore_every_anchor_where_it_was() {
        let base = content("The tax rate and the tax base rose in Q3.\n\nA second paragraph follows.");
        let base = with_anchor(base, 4, 12, "range");
        let base = with_anchor(base, 21, 21, "point");
        let ctxs = anchor_contexts(&base);
        assert_eq!(ctxs[0].quote, "tax rate");
        assert_eq!(ctxs[1].quote, "");

        let mut held = base.clone();
        let out = held.reattach(&ctxs, None, &none());
        assert_eq!(held, base, "a live anchor is not written");
        assert_eq!(out.reattached, ctxs);

        let mut bare = without_anchors(&base);
        let out = bare.reattach(&ctxs, None, &none());
        assert!(out.orphaned.is_empty(), "{out:?}");
        assert_eq!(bare, base);
        assert_eq!(out.reattached, ctxs);
    }

    #[test]
    fn a_quote_across_an_inline_island_is_read_and_found() {
        let base = content("Pay the fee[^1] by Friday, or else.\n\n[^1]: Net of tax.");
        let fee = base.text.find("fee").unwrap();
        let end = base.text.chars().position(|c| c == ',').unwrap();
        let base = with_anchor(base, fee, end, "c1");
        let ctx = &anchor_contexts(&base)[0];
        assert_eq!(ctx.quote, "fee by Friday");
        assert_eq!(locate(&base, ctx, None), Some((fee, end)));
    }

    #[test]
    fn an_anchor_over_a_table_slot_alone_comes_back_over_it() {
        let base = content("Intro line here.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nOutro line here.");
        let slot = base.text.chars().position(|c| c == ISLAND_SLOT).unwrap();
        let base = with_anchor(base, slot, slot + 1, "table");
        let ctx = &anchor_contexts(&base)[0];
        assert_eq!(ctx.quote, "");
        assert_eq!(locate(&base, ctx, None), Some((slot, slot + 1)));
    }

    #[test]
    fn a_short_or_repeated_quote_places_nothing() {
        let rt = content("one two one two one two");
        assert_eq!(locate(&rt, &AnchorContext::new("c", "two", "", ""), None), None);

        let rt = content("and then gamma rises fast, and then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "and then ", " rises fast");
        assert_eq!(locate(&rt, &ctx, None), None, "two occurrences with equal context");

        let rt = content("and then gamma rises fast, but then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "but then ", " rises fast");
        let at = rt.text.rfind("gamma").unwrap();
        assert_eq!(locate(&rt, &ctx, None), Some((at, at + 5)));
    }

    #[test]
    fn a_point_places_where_its_context_meets_once() {
        let base = content("Lorem ipsum dolor sit amet, consectetur adipiscing elit.");
        let at = base.text.find("sit").unwrap();
        let base = with_anchor(base, at, at, "p");
        let ctx = anchor_contexts(&base).remove(0);

        let edited = content("Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do.");
        assert_eq!(locate(&edited, &ctx, None), Some((at, at)));

        let rewritten = content("Lorem ipsum dolxr sXt amet, consectetur adipiscing elit.");
        assert_eq!(locate(&rewritten, &ctx, None), None, "edited within the agreement on both sides");
    }

    #[test]
    fn a_range_keeps_its_extent_through_revise_and_widens_over_an_inserted_word() {
        let base = content("Keep the quarterly figures in view.");
        let at = base.text.find("quarterly figures").unwrap();
        let base = with_anchor(base, at, at + "quarterly figures".len(), "c1");

        let (kept, _, _) = diff_import(&base, "Always keep the quarterly figures in view.").unwrap();
        assert_eq!(text(&kept, span(&kept, "c1").unwrap()), "quarterly figures");

        let (wide, _, _) = diff_import(&base, "Keep the quarterly sales figures in view.").unwrap();
        assert_eq!(text(&wide, span(&wide, "c1").unwrap()), "quarterly sales figures");
    }

    #[test]
    fn an_anchor_revise_refuses_onto_a_surviving_twin_stays_orphaned() {
        let base = with_anchor(content("target one to drop\n\nkeep the target two"), 0, 6, "c1");
        let ctxs = anchor_contexts(&base);
        let (mut next, delta, _) = diff_import(&base, "keep the target two").unwrap();
        assert_eq!(span(&next, "c1"), None, "revise drops it");

        let inserted = delta.inserted_spans();
        let out = next.reattach(&ctxs, Some(&inserted), &none());
        assert_eq!(out.orphaned, ctxs);
        let out = next.reattach(&ctxs, None, &none());
        assert_eq!(out.orphaned, ctxs, "the context disagrees even without a delta");
        assert_eq!(span(&next, "c1"), None);
    }

    #[test]
    fn a_line_rewritten_past_recognition_keeps_its_quote_over_the_same_words() {
        let base = content("Intro.\n\nOver the fiscal year we approved the budget for roads.");
        let at = base.text.find("approved the budget").unwrap();
        let base = with_anchor(base, at, at + "approved the budget".len(), "c1");
        let ctxs = anchor_contexts(&base);
        let filler = "one two three four five six seven eight nine ten eleven twelve thirteen";
        let new_md = format!("Intro.\n\n{filler} fourteen fifteen sixteen fiscal year we approved the budget for roads.");

        let (revised, _, _) = diff_import(&base, &new_md).unwrap();
        let moved = span(&revised, "c1").expect("revise relocates it into the inserted line");
        assert_eq!(text(&revised, moved), "approved the budget");

        let (mut next, delta) = rebase_onto(&without_anchors(&base), content(&new_md));
        let out = next.reattach(&ctxs, Some(&delta.inserted_spans()), &none());
        assert!(out.orphaned.is_empty(), "{out:?}");
        assert_eq!(span(&next, "c1"), Some(moved));
        assert_eq!(out.reattached[0].quote, "approved the budget");
    }

    #[test]
    fn an_id_live_elsewhere_is_answered_and_not_written() {
        let rt = content("some words to anchor here");
        let ctx = AnchorContext::new("c1", "words to anchor", "some ", " here");
        let mut held = rt.clone();
        let out = held.reattach(std::slice::from_ref(&ctx), None, &HashSet::from(["c1"]));
        assert_eq!(held, rt);
        assert_eq!(out.reattached, vec![ctx]);
    }
}
