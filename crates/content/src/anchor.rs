//! What a prose anchor is attached to, and the one exact rule that places a
//! dropped anchor again. The contract is `prose/canon/COMMENTS.md`.
//!
//! Both read the stored text as it is: an inline island is its
//! [`ISLAND_SLOT`](crate::model::ISLAND_SLOT) character and a line break its
//! `\n`. A quote spanning an island holds its slot, and is found again only
//! where a slot sits.

use std::collections::{HashMap, HashSet};

use crate::delta::MIN_MOVE;
use crate::model::{Content, MarkKind, Normalized, Usv};
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
    /// The anchored text as stored, island slots and line breaks included;
    /// empty for a point.
    pub quote: String,
    /// Up to [`ANCHOR_CONTEXT_USV`] scalar values of stored text before the
    /// anchor, cut at the field's start.
    pub prefix: String,
    /// Up to [`ANCHOR_CONTEXT_USV`] scalar values of stored text after the
    /// anchor, cut at the field's end.
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
/// is outside the read, and so is one inverted or past the text, which
/// [`validate`](Content::validate) refuses.
pub fn anchor_contexts(rt: &Normalized) -> Vec<AnchorContext> {
    if !rt.marks.iter().any(|m| matches!(m.kind, MarkKind::Anchor { .. })) {
        return Vec::new();
    }
    let chars: Vec<char> = rt.text.chars().collect();
    let mut found: Vec<(Usv, &str, Usv)> = rt
        .marks
        .iter()
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } if m.start <= m.end && m.end <= chars.len() => {
                Some((m.start, id.as_str(), m.end))
            }
            _ => None,
        })
        .collect();
    found.sort_unstable();
    found
        .into_iter()
        .map(|(start, id, end)| AnchorContext {
            id: id.to_string(),
            quote: chars[start..end].iter().collect(),
            prefix: chars[start.saturating_sub(ANCHOR_CONTEXT_USV)..start].iter().collect(),
            suffix: chars[end..(end + ANCHOR_CONTEXT_USV).min(chars.len())].iter().collect(),
        })
        .collect()
}

/// The one range `ctx` names in `rt`, or `None`. Never a nearest match:
///
/// - A quote shorter than [`MIN_ANCHOR_QUOTE`] names none.
/// - Of every occurrence of the quote in the stored text, one counts where the
///   text before it ends with the last [`ANCHOR_AGREE_USV`] of `prefix` and the
///   text after it starts with the first [`ANCHOR_AGREE_USV`] of `suffix`. A
///   shorter `prefix` or `suffix` was cut at the field's edge, so it must be
///   the whole text on that side.
/// - An empty quote is a point: a position where the prefix and the suffix
///   both agree.
/// - With `inserted` given, the stored spans a write inserted, a quote counts
///   only where it overlaps one, and a point only where the context it agrees
///   on does: the discipline `revise` keeps when it moves an anchor.
/// - Exactly one counting place names its range; none or several name none.
///
/// A range is exactly the quote's characters, and a point is the empty range
/// where its prefix meets its suffix. An island slot is a character like any
/// other, so a quote holding one matches only where a slot sits.
pub fn locate(
    rt: &Normalized,
    ctx: &AnchorContext,
    inserted: Option<&[(Usv, Usv)]>,
) -> Option<(Usv, Usv)> {
    let text: Vec<char> = rt.text.chars().collect();
    locate_in(&text, ctx, inserted)
}

impl Content {
    /// [`Normalized::reattach`] on this content, normalized first.
    pub fn reattach(
        &mut self,
        ctxs: &[AnchorContext],
        inserted: Option<&[(Usv, Usv)]>,
        live: &HashSet<&str>,
    ) -> Reattached<AnchorContext> {
        let mut rt = std::mem::replace(self, Content::empty()).into_normalized();
        let out = rt.reattach(ctxs, inserted, live);
        *self = rt.into_content();
        out
    }
}

impl Normalized {
    /// Re-add each context's anchor where [`locate`] finds it, through the
    /// [`MarkOp::Add`] arm, and answer which landed. An id in `live`, or
    /// already a prose anchor here, is not written and is answered as
    /// reattached at its live range. Never mints an id, never moves a live
    /// anchor, and places each anchor whole or not at all.
    ///
    /// Each id is answered once: of contexts sharing an id, the first is
    /// answered and the rest are left out of the receipt.
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
        let mut answered = HashSet::new();
        let ctxs: Vec<&AnchorContext> = ctxs.iter().filter(|c| answered.insert(c.id.as_str())).collect();
        let held: HashSet<String> = self
            .marks
            .iter()
            .filter_map(|m| match &m.kind {
                MarkKind::Anchor { id } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let text: Vec<char> = self.text.chars().collect();
        let mut placed = Vec::with_capacity(ctxs.len());
        for ctx in &ctxs {
            if live.contains(ctx.id.as_str()) || held.contains(&ctx.id) {
                placed.push(true);
                continue;
            }
            let landed = locate_in(&text, ctx, inserted).is_some_and(|(start, end)| {
                let add = MarkOp::Add {
                    start,
                    end,
                    kind: MarkKind::Anchor { id: ctx.id.clone() },
                };
                self.apply_mark_ops(&[add]).is_ok()
            });
            placed.push(landed);
        }
        let mut fresh: HashMap<String, AnchorContext> =
            anchor_contexts(self).into_iter().map(|c| (c.id.clone(), c)).collect();
        let mut out = Reattached::default();
        for (ctx, landed) in ctxs.into_iter().zip(placed) {
            if landed {
                out.reattached.push(fresh.remove(&ctx.id).unwrap_or_else(|| ctx.clone()));
            } else {
                out.orphaned.push(ctx.clone());
            }
        }
        out
    }
}

fn locate_in(text: &[char], ctx: &AnchorContext, inserted: Option<&[(Usv, Usv)]>) -> Option<(Usv, Usv)> {
    let quote: Vec<char> = ctx.quote.chars().collect();
    let prefix: Vec<char> = ctx.prefix.chars().collect();
    let suffix: Vec<char> = ctx.suffix.chars().collect();
    let n = text.len();
    let touches = |(a, b): (Usv, Usv)| inserted.is_none_or(|spans| spans.iter().any(|&(s, e)| a < e && b > s));
    let agrees = |i: usize, j: usize| agrees_before(&text[..i], &prefix) && agrees_after(&text[j..], &suffix);
    let mut hits = Vec::new();
    if quote.is_empty() {
        let (before, after) = (prefix.len().min(ANCHOR_AGREE_USV), suffix.len().min(ANCHOR_AGREE_USV));
        for p in before..=n.saturating_sub(after) {
            if agrees(p, p) && touches((p - before, p + after)) {
                hits.push((p, p));
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
            if text[i..j] == quote[..] && agrees(i, j) && touches((i, j)) {
                hits.push((i, j));
                if hits.len() > 1 {
                    return None;
                }
            }
        }
    }
    hits.pop()
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
    use crate::model::{Mark, ISLAND_SLOT};

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
    fn a_quote_across_an_inline_island_holds_its_slot() {
        let base = content("Pay the fee[^1] by Friday, or else.\n\n[^1]: Net of tax.");
        let fee = base.text.find("fee").unwrap();
        let end = base.text.chars().position(|c| c == ',').unwrap();
        let base = with_anchor(base, fee, end, "c1");
        let ctx = &anchor_contexts(&base)[0];
        assert_eq!(ctx.quote, "fee\u{FFFC} by Friday");
        assert_eq!(locate(&base, ctx, None), Some((fee, end)));
        let unslotted = AnchorContext::new("c1", "fee by Friday", &*ctx.prefix, &*ctx.suffix);
        assert_eq!(locate(&base, &unslotted, None), None);
    }

    #[test]
    fn an_anchor_beside_an_inline_island_comes_back_where_it_was() {
        let base = content("Pay the fee[^1] today please.\n\n[^1]: Net of tax.");
        let slot = base.text.chars().position(|c| c == ISLAND_SLOT).unwrap();
        let the = base.text.find("the").unwrap();
        for (start, end) in [(slot, slot), (slot + 1, slot + 1), (the, slot + 1)] {
            let anchored = with_anchor(base.clone(), start, end, "c1");
            let ctxs = anchor_contexts(&anchored);
            let mut bare = without_anchors(&anchored);
            let out = bare.reattach(&ctxs, None, &none());
            assert_eq!(span(&bare, "c1"), Some((start, end)), "{ctxs:?}");
            assert_eq!(out.reattached, ctxs);
        }
    }

    #[test]
    fn an_anchor_over_a_slot_alone_quotes_the_slot_and_is_under_the_floor() {
        let base = content("Intro line here.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nOutro line here.");
        let slot = base.text.chars().position(|c| c == ISLAND_SLOT).unwrap();
        let base = with_anchor(base, slot, slot + 1, "table");
        let ctxs = anchor_contexts(&base);
        assert_eq!(ctxs[0].quote, "\u{FFFC}");

        let mut held = base.clone();
        let out = held.reattach(&ctxs, None, &none());
        assert_eq!((held, out.reattached), (base.clone(), ctxs.clone()));

        let mut bare = without_anchors(&base);
        let out = bare.reattach(&ctxs, None, &none());
        assert_eq!(out.orphaned, ctxs);
        assert_eq!(bare, without_anchors(&base));
    }

    #[test]
    fn the_quote_floor_alone_refuses_a_three_character_quote() {
        let whole = |text: &str| locate(&content(text), &AnchorContext::new("c", text, "", ""), None);
        assert_eq!(whole("abc"), None);
        assert_eq!(whole("abcd"), Some((0, 4)));
    }

    #[test]
    fn a_repeated_quote_places_nothing_and_a_disagreeing_twin_does_not_count() {
        let rt = content("and then gamma rises fast, and then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "and then ", " rises fast");
        assert_eq!(locate(&rt, &ctx, None), None, "two occurrences with equal context");

        let rt = content("and then gamma rises fast, but then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "but then ", " rises fast");
        let at = rt.text.rfind("gamma").unwrap();
        assert_eq!(locate(&rt, &ctx, None), Some((at, at + 5)));
    }

    #[test]
    fn a_delta_counts_only_a_match_overlapping_what_it_inserted() {
        let rt = content("Lorem ipsum dolor sit amet, consectetur adipiscing elit.");
        let at = rt.text.find("dolor sit").unwrap();
        let quote = AnchorContext::new("c", "dolor sit", "Lorem ipsum ", " amet, consectetur adipiscing elit.");
        assert_eq!(locate(&rt, &quote, None), Some((at, at + 9)));
        assert_eq!(locate(&rt, &quote, Some(&[(at + 8, at + 12)])), Some((at, at + 9)));
        assert_eq!(locate(&rt, &quote, Some(&[(at + 9, at + 12)])), None);

        let point = AnchorContext::new("p", "", "Lorem ipsum ", "dolor sit amet, consectetur");
        assert_eq!(locate(&rt, &point, None), Some((at, at)));
        assert_eq!(locate(&rt, &point, Some(&[(at + 7, at + 12)])), Some((at, at)));
        assert_eq!(locate(&rt, &point, Some(&[(at + 8, at + 12)])), None);
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
    fn raw_content_is_normalized_first_and_each_id_answered_once() {
        let mut raw = content("some words to anchor here").into_content();
        raw.marks.push(Mark::new(9, 3, MarkKind::Anchor { id: "bad".into() }));
        let ctx = AnchorContext::new("c1", "words to anchor", "some ", " here");
        let twin = AnchorContext::new("c1", "nothing here at all", "", "");
        let out = raw.reattach(std::slice::from_ref(&twin), None, &none());
        assert_eq!(out.orphaned, std::slice::from_ref(&twin));
        let out = raw.reattach(&[ctx.clone(), twin], None, &none());
        assert_eq!((out.reattached, out.orphaned), (vec![ctx], vec![]));
        assert_eq!(span(&raw, "c1"), Some((5, 20)));
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
