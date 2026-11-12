//! What an anchor is attached to, read off the stored text, and the one
//! exact rule that places a dropped anchor again. Why the rule is exact or
//! nothing: `prose/INVARIANTS.md` § "Comments".
//!
//! Both read the stored text as it is: an island is its [`ISLAND_SLOT`]
//! character, a hard break its [`HARD_BREAK`] and a block break its `\n`. A
//! quote spanning an island holds its slot, and is found again only where a
//! slot sits.

use std::collections::HashSet;

use crate::delta::MIN_MOVE;
use crate::model::{Anchor, Content, HARD_BREAK, ISLAND_SLOT, Scalar};

/// Context read on each side of an anchor, in Unicode scalar values.
pub const ANCHOR_CONTEXT_SCALARS: usize = 32;

/// Context that must agree on each side for a match. A context the field's
/// edge cut shorter agrees as far as it reaches, so text written past that
/// edge since the read does not orphan the anchor.
pub const ANCHOR_AGREE_SCALARS: usize = 8;

/// A quote shorter than this is too weak to place: the floor `revise` applies
/// to a moved anchor. A quote made of island slots alone is under it and is
/// placed on its context, like a point.
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
    /// Up to [`ANCHOR_CONTEXT_SCALARS`] scalar values of stored text before the
    /// anchor, cut at the field's start.
    pub prefix: String,
    /// Up to [`ANCHOR_CONTEXT_SCALARS`] scalar values of stored text after the
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

/// A text surface of a value: its prose, or one slot of one of its islands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    Prose,
    Slot { island: String, slot: String },
}

/// Every anchor of `c` with its context, in reading order: by the position
/// where each stands, a slot's at its island's slot, then by slot order and
/// start. `quote` is the anchored text as stored, a hard break its `U+2028`
/// and a block break its `\n`; `prefix` and `suffix` are up to
/// [`ANCHOR_CONTEXT_SCALARS`] scalar values of stored text either side, cut at
/// the surface's edges. Read off the stored text, so it is pure and moves
/// nothing.
pub fn anchor_contexts(c: &Content) -> Vec<AnchorContext> {
    contexts_where(c, |_| true)
}

/// The [`anchor_contexts`] entry for `id`, reading no other anchor's context.
pub fn anchor_context(c: &Content, id: &str) -> Option<AnchorContext> {
    contexts_where(c, |held| held == id).into_iter().next()
}

fn contexts_where(c: &Content, keep: impl Fn(&str) -> bool) -> Vec<AnchorContext> {
    let mut sites: Vec<(Scalar, usize, Scalar, AnchorContext)> = Vec::new();
    let prose: Vec<char> = c.text.chars().collect();
    for a in c.anchors.iter().filter(|a| keep(&a.id)) {
        sites.push((a.start, 0, a.start, context_of(&prose, a)));
    }
    let slots = c
        .text
        .chars()
        .enumerate()
        .filter(|&(_, ch)| ch == ISLAND_SLOT)
        .map(|(at, _)| at);
    for (island, at) in c.islands.iter().zip(slots) {
        for (n, slot) in island.slot_order().iter().enumerate() {
            let Some(inner) = island.slots.get(slot) else {
                continue;
            };
            let text: Vec<char> = inner.text.chars().collect();
            for a in inner.anchors.iter().filter(|a| keep(&a.id)) {
                sites.push((at, n + 1, a.start, context_of(&text, a)));
            }
        }
    }
    sites.sort_by(|a, b| (a.0, a.1, a.2, &a.3.id).cmp(&(b.0, b.1, b.2, &b.3.id)));
    sites.into_iter().map(|(_, _, _, ctx)| ctx).collect()
}

fn context_of(text: &[char], a: &Anchor) -> AnchorContext {
    let (start, end) = (a.start.min(text.len()), a.end.min(text.len()));
    AnchorContext {
        id: a.id.clone(),
        quote: text[start..end].iter().collect(),
        prefix: text[start.saturating_sub(ANCHOR_CONTEXT_SCALARS)..start]
            .iter()
            .collect(),
        suffix: text[end..(end + ANCHOR_CONTEXT_SCALARS).min(text.len())]
            .iter()
            .collect(),
    }
}

/// A surface's text as the line form spelled it, with each of its positions
/// mapped to the stored one: a hard break as `\n`, and in a slot a block
/// break as two.
fn line_spelling(text: &str, slot: bool) -> (Vec<char>, Vec<Scalar>) {
    let mut chars = Vec::with_capacity(text.len());
    let mut at = Vec::with_capacity(text.len() + 1);
    for (i, ch) in text.chars().enumerate() {
        at.push(i);
        match ch {
            HARD_BREAK => chars.push('\n'),
            '\n' if slot => {
                chars.push('\n');
                at.push(i);
                chars.push('\n');
            }
            ch => chars.push(ch),
        }
    }
    at.push(text.chars().count());
    (chars, at)
}

/// The one surface and range `ctx` names in `c`, or `None`. Never a nearest
/// match. Each surface, the prose and each island's slots, is read as its own
/// text, so a context is cut at a slot's edges:
///
/// - A quote shorter than [`MIN_ANCHOR_QUOTE`] names none, unless it is island
///   slots alone: an anchor over a table's slot is placed on its context, the
///   way a point is.
/// - Of every occurrence of the quote in the stored text, one counts where the
///   text before it ends with the last [`ANCHOR_AGREE_SCALARS`] of `prefix` and the
///   text after it starts with the first [`ANCHOR_AGREE_SCALARS`] of `suffix`. A
///   shorter `prefix` or `suffix` was cut at the field's edge and agrees
///   whole, wherever the field's edge now sits.
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
///
/// The rule reads the stored spelling, or, where that names no place, the
/// line form's spelling of each surface, which the selectors stored before
/// 0.135 use. With `inserted` given, which spans the prose alone, only the
/// prose counts.
pub fn locate_site(
    c: &Content,
    ctx: &AnchorContext,
    inserted: Option<&[(Scalar, Scalar)]>,
) -> Option<(Surface, Scalar, Scalar)> {
    let surfaces: Vec<(Surface, &str)> = match inserted {
        Some(_) => vec![(Surface::Prose, c.text.as_str())],
        None => std::iter::once((Surface::Prose, c.text.as_str()))
            .chain(c.islands.iter().flat_map(|i| {
                i.slot_order().into_iter().filter_map(move |s| {
                    i.slots.get(&s).map(|inner| {
                        (
                            Surface::Slot {
                                island: i.id.clone(),
                                slot: s.clone(),
                            },
                            inner.text.as_str(),
                        )
                    })
                })
            }))
            .collect(),
    };
    let one = |mut hits: Box<dyn Iterator<Item = (Surface, Scalar, Scalar)> + '_>| {
        let hit = hits.next();
        hit.filter(|_| hits.next().is_none())
    };
    let mut stored = surfaces.iter().filter_map(|(surface, text)| {
        let chars: Vec<char> = text.chars().collect();
        locate_in(&chars, ctx, inserted).map(|(s, e)| (surface.clone(), s, e))
    });
    match stored.next() {
        Some(hit) => return stored.next().is_none().then_some(hit),
        None => {}
    }
    one(Box::new(surfaces.iter().filter_map(|(surface, text)| {
        let (chars, at) = line_spelling(text, *surface != Surface::Prose);
        locate_in(&chars, ctx, inserted).map(|(s, e)| (surface.clone(), at[s], at[e]))
    })))
}

fn locate_in(
    text: &[char],
    ctx: &AnchorContext,
    inserted: Option<&[(Scalar, Scalar)]>,
) -> Option<(Scalar, Scalar)> {
    let quote: Vec<char> = ctx.quote.chars().collect();
    let prefix: Vec<char> = ctx.prefix.chars().collect();
    let suffix: Vec<char> = ctx.suffix.chars().collect();
    let n = text.len();
    let touches = |(a, b): (Scalar, Scalar)| {
        inserted.is_none_or(|spans| spans.iter().any(|&(s, e)| a < e && b > s))
    };
    let agrees = |i: usize, j: usize| {
        agrees_before(&text[..i], &prefix) && agrees_after(&text[j..], &suffix)
    };
    let mut hits = Vec::new();
    if quote.is_empty() {
        let (before, after) = (
            prefix.len().min(ANCHOR_AGREE_SCALARS),
            suffix.len().min(ANCHOR_AGREE_SCALARS),
        );
        for p in before..=n.saturating_sub(after) {
            if agrees(p, p) && touches((p - before, p + after)) {
                hits.push((p, p));
                if hits.len() > 1 {
                    return None;
                }
            }
        }
    } else {
        let slots_alone = quote.iter().all(|&c| c == ISLAND_SLOT);
        if (quote.len() < MIN_ANCHOR_QUOTE && !slots_alone) || quote.len() > n {
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
    before.ends_with(&prefix[prefix.len().saturating_sub(ANCHOR_AGREE_SCALARS)..])
}

fn agrees_after(after: &[char], suffix: &[char]) -> bool {
    after.starts_with(&suffix[..suffix.len().min(ANCHOR_AGREE_SCALARS)])
}

impl Content {
    /// Re-add each context's anchor where [`locate_site`] finds it, in the
    /// prose or a slot, and answer which landed. An id in `live`, or already
    /// an anchor here on any surface, is not written and is answered as
    /// reattached at its live range. Never mints an id, never moves a live
    /// anchor, and places each anchor whole or not at all. Each id is
    /// answered once: of contexts sharing an id, the first is answered and
    /// the rest are left out of the receipt. A reattached entry carries its
    /// context as read after the write; an orphaned one, and one live outside
    /// this value, carries the context as given.
    pub fn reattach(
        &mut self,
        ctxs: &[AnchorContext],
        inserted: Option<&[(Scalar, Scalar)]>,
        live: &HashSet<&str>,
    ) -> Reattached<AnchorContext> {
        let mut answered = HashSet::new();
        let ctxs: Vec<&AnchorContext> = ctxs
            .iter()
            .filter(|c| answered.insert(c.id.as_str()))
            .collect();
        let held: HashSet<String> = self.anchor_ids().into_iter().collect();
        let mut placed = Vec::with_capacity(ctxs.len());
        for ctx in &ctxs {
            if live.contains(ctx.id.as_str()) || held.contains(&ctx.id) {
                placed.push(true);
                continue;
            }
            let site = locate_site(self, ctx, inserted).filter(|_| !ctx.id.is_empty());
            if let Some((surface, start, end)) = &site {
                let anchor = Anchor::new(ctx.id.clone(), *start, *end);
                match surface {
                    Surface::Prose => self.anchors.push(anchor),
                    Surface::Slot { island, slot } => {
                        if let Some(inner) = self
                            .islands
                            .iter_mut()
                            .find(|i| &i.id == island)
                            .and_then(|i| i.slots.get_mut(slot))
                        {
                            inner.anchors.push(anchor);
                        }
                    }
                }
            }
            placed.push(site.is_some());
        }
        self.normalize();
        let mut out = Reattached::default();
        for (ctx, placed) in ctxs.into_iter().zip(placed) {
            match placed {
                true => out
                    .reattached
                    .push(anchor_context(self, &ctx.id).unwrap_or_else(|| ctx.clone())),
                false => out.orphaned.push(ctx.clone()),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    mod properties;

    use super::*;
    use crate::carry::carry_onto;
    use crate::diff::revise;
    use crate::markdown::{from_markdown, to_markdown};
    use crate::table::Table;
    use serde_json::json;

    fn value(v: serde_json::Value) -> Content {
        Content::from_value(&v).unwrap()
    }

    fn content(markdown: &str) -> Content {
        from_markdown(markdown).unwrap().content
    }

    fn with_anchor(mut c: Content, start: Scalar, end: Scalar, id: &str) -> Content {
        c.anchors.push(Anchor::new(id, start, end));
        c.normalize();
        c
    }

    fn span(c: &Content, id: &str) -> Option<(Scalar, Scalar)> {
        c.anchor(id).map(|a| (a.start, a.end))
    }

    fn text(c: &Content, (start, end): (Scalar, Scalar)) -> String {
        c.text.chars().skip(start).take(end - start).collect()
    }

    fn without_anchors(c: &Content) -> Content {
        let mut c = c.clone();
        c.anchors.clear();
        for slot in c.islands.iter_mut().flat_map(|i| i.slots.values_mut()) {
            slot.anchors.clear();
        }
        c
    }

    fn none() -> HashSet<&'static str> {
        HashSet::new()
    }

    fn locate(
        c: &Content,
        ctx: &AnchorContext,
        inserted: Option<&[(Scalar, Scalar)]>,
    ) -> Option<(Scalar, Scalar)> {
        match locate_site(c, ctx, inserted)? {
            (Surface::Prose, start, end) => Some((start, end)),
            _ => None,
        }
    }

    /// The slot of the first body row's first cell.
    fn first_cell(c: &Content) -> String {
        let table = Table::read(&c.islands[0].props).unwrap();
        table.slot(1, 0).unwrap().to_string()
    }

    fn table_with_cell_anchor(id: &str) -> Content {
        let mut c = content(
            "Lead line here.\n\n| head | other |\n|---|---|\n| alpha beta | gamma |\n\nTail line here.",
        );
        let slot = first_cell(&c);
        let cell = c.islands[0].slots.get_mut(&slot).unwrap();
        cell.anchors.push(Anchor::new(id, 6, 10));
        c
    }

    /// A context the field's edge cut short agrees as far as it reaches, so a
    /// sentence written ahead of the anchor since the read leaves it placeable.
    #[test]
    fn a_context_cut_at_the_edge_agrees_past_new_text_there() {
        let read = with_anchor(content("Hello world, and more."), 6, 11, "w");
        let ctx = anchor_context(&read, "w").unwrap();
        assert_eq!(ctx.prefix, "Hello ");
        let now = content("A lead sentence. Hello world, and more.");
        assert_eq!(locate(&now, &ctx, None), Some((23, 28)));
    }

    #[test]
    fn contexts_then_reattach_restore_every_anchor_where_it_was() {
        let base =
            content("The tax rate and the tax base rose in Q3.\n\nA second paragraph follows.");
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
    fn an_anchor_over_a_slot_alone_quotes_the_slot_and_is_placed_on_its_context() {
        let base =
            content("Intro line here.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nOutro line here.");
        let slot = base.text.chars().position(|c| c == ISLAND_SLOT).unwrap();
        let base = with_anchor(base, slot, slot + 1, "table");
        let ctxs = anchor_contexts(&base);
        assert_eq!(ctxs[0].quote, "\u{FFFC}");

        let mut bare = without_anchors(&base);
        let out = bare.reattach(&ctxs, None, &none());
        assert_eq!((bare, out.reattached), (base, ctxs));
    }

    #[test]
    fn a_slot_alone_is_not_placed_where_its_context_disagrees_or_twice() {
        let two = content(
            "Intro line here.\n\n| a |\n|---|\n| 1 |\n\nMiddle.\n\n| a |\n|---|\n| 2 |\n\nOutro line here.",
        );
        let slots: Vec<Scalar> = two
            .text
            .char_indices()
            .filter(|&(_, c)| c == ISLAND_SLOT)
            .map(|(i, _)| i)
            .collect();
        let first = with_anchor(two.clone(), slots[0], slots[0] + 1, "t");
        let ctx = anchor_contexts(&first).remove(0);
        assert_eq!(locate(&two, &ctx, None), Some((slots[0], slots[0] + 1)));
        let moved = AnchorContext::new("t", "\u{FFFC}", &ctx.prefix, "\n\nElsewhere.");
        assert_eq!(locate(&two, &moved, None), None);
        let same_context = content(
            "Intro line here.\n\n| a |\n|---|\n| 1 |\n\nMiddle.\n\nIntro line here.\n\n| a |\n|---|\n| 2 |\n\nMiddle.\n\nOutro line here.",
        );
        assert_eq!(locate(&same_context, &ctx, None), None);
    }

    #[test]
    fn the_quote_floor_alone_refuses_a_three_character_quote() {
        let whole =
            |text: &str| locate(&content(text), &AnchorContext::new("c", text, "", ""), None);
        assert_eq!(whole("abc"), None);
        assert_eq!(whole("abcd"), Some((0, 4)));
    }

    #[test]
    fn a_repeated_quote_places_nothing_and_a_disagreeing_twin_does_not_count() {
        let c = content("and then gamma rises fast, and then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "and then ", " rises fast");
        assert_eq!(
            locate(&c, &ctx, None),
            None,
            "two occurrences with equal context"
        );

        let c = content("and then gamma rises fast, but then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "but then ", " rises fast");
        let at = c.text.rfind("gamma").unwrap();
        assert_eq!(locate(&c, &ctx, None), Some((at, at + 5)));
    }

    #[test]
    fn a_delta_counts_only_a_match_overlapping_what_it_inserted() {
        let c = content("Lorem ipsum dolor sit amet, consectetur adipiscing elit.");
        let at = c.text.find("dolor sit").unwrap();
        let quote = AnchorContext::new(
            "c",
            "dolor sit",
            "Lorem ipsum ",
            " amet, consectetur adipiscing elit.",
        );
        assert_eq!(locate(&c, &quote, None), Some((at, at + 9)));
        assert_eq!(
            locate(&c, &quote, Some(&[(at + 8, at + 12)])),
            Some((at, at + 9))
        );
        assert_eq!(locate(&c, &quote, Some(&[(at + 9, at + 12)])), None);

        let point = AnchorContext::new("p", "", "Lorem ipsum ", "dolor sit amet, consectetur");
        assert_eq!(locate(&c, &point, None), Some((at, at)));
        assert_eq!(
            locate(&c, &point, Some(&[(at + 7, at + 12)])),
            Some((at, at))
        );
        assert_eq!(locate(&c, &point, Some(&[(at + 8, at + 12)])), None);
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
        assert_eq!(
            locate(&rewritten, &ctx, None),
            None,
            "edited within the agreement on both sides"
        );
    }

    #[test]
    fn a_range_keeps_its_extent_through_revise_and_widens_over_an_inserted_word() {
        let base = content("Keep the quarterly figures in view.");
        let at = base.text.find("quarterly figures").unwrap();
        let base = with_anchor(base, at, at + "quarterly figures".len(), "c1");

        let kept = revise(&base, "Always keep the quarterly figures in view.")
            .unwrap()
            .content;
        assert_eq!(text(&kept, span(&kept, "c1").unwrap()), "quarterly figures");

        let wide = revise(&base, "Keep the quarterly sales figures in view.")
            .unwrap()
            .content;
        assert_eq!(
            text(&wide, span(&wide, "c1").unwrap()),
            "quarterly sales figures"
        );
    }

    #[test]
    fn an_anchor_revise_refuses_onto_a_surviving_twin_stays_orphaned() {
        let base = with_anchor(
            content("target one to drop\n\nkeep the target two"),
            0,
            6,
            "c1",
        );
        let ctxs = anchor_contexts(&base);
        let (mut next, delta) = carry_onto(&base, content("keep the target two"));
        assert_eq!(span(&next, "c1"), None, "the carry drops it");

        let inserted = delta.inserted_spans();
        let out = next.reattach(&ctxs, Some(&inserted), &none());
        assert_eq!(out.orphaned, ctxs);
        let out = next.reattach(&ctxs, None, &none());
        assert_eq!(
            out.orphaned, ctxs,
            "the context disagrees even without a delta"
        );
        assert_eq!(span(&next, "c1"), None);
    }

    #[test]
    fn a_line_rewritten_past_recognition_keeps_its_quote_over_the_same_words() {
        let base = content("Intro.\n\nOver the fiscal year we approved the budget for roads.");
        let at = base.text.find("approved the budget").unwrap();
        let base = with_anchor(base, at, at + "approved the budget".len(), "c1");
        let ctxs = anchor_contexts(&base);
        let filler = "one two three four five six seven eight nine ten eleven twelve thirteen";
        let new_md = format!(
            "Intro.\n\n{filler} fourteen fifteen sixteen fiscal year we approved the budget for roads."
        );

        let revised = revise(&base, &new_md).unwrap().content;
        let moved = span(&revised, "c1").expect("revise relocates it into the inserted line");
        assert_eq!(text(&revised, moved), "approved the budget");

        let (mut next, delta) = carry_onto(&without_anchors(&base), content(&new_md));
        let out = next.reattach(&ctxs, Some(&delta.inserted_spans()), &none());
        assert!(out.orphaned.is_empty(), "{out:?}");
        assert_eq!(span(&next, "c1"), Some(moved));
        assert_eq!(out.reattached[0].quote, "approved the budget");
    }

    #[test]
    fn each_id_is_answered_once() {
        let mut c = content("some words to anchor here");
        let ctx = AnchorContext::new("c1", "words to anchor", "some ", " here");
        let twin = AnchorContext::new("c1", "nothing here at all", "", "");
        let out = c.reattach(std::slice::from_ref(&twin), None, &none());
        assert_eq!(out.orphaned, std::slice::from_ref(&twin));
        let out = c.reattach(&[ctx.clone(), twin], None, &none());
        assert_eq!((out.reattached, out.orphaned), (vec![ctx], vec![]));
        assert_eq!(span(&c, "c1"), Some((5, 20)));
    }

    #[test]
    fn an_id_live_elsewhere_is_answered_and_not_written() {
        let c = content("some words to anchor here");
        let ctx = AnchorContext::new("c1", "words to anchor", "some ", " here");
        let mut held = c.clone();
        let out = held.reattach(std::slice::from_ref(&ctx), None, &HashSet::from(["c1"]));
        assert_eq!(held, c);
        assert_eq!(out.reattached, vec![ctx]);
    }

    #[test]
    fn a_cell_anchor_is_read_with_its_cell_context() {
        let c = with_anchor(table_with_cell_anchor("cell"), 0, 4, "lead");
        let ids: Vec<String> = anchor_contexts(&c).into_iter().map(|x| x.id).collect();
        assert_eq!(ids, ["lead", "cell"]);
        let ctx = anchor_context(&c, "cell").unwrap();
        assert_eq!(
            (&*ctx.prefix, &*ctx.quote, &*ctx.suffix),
            ("alpha ", "beta", "")
        );
    }

    #[test]
    fn a_cell_anchor_reattaches_into_its_cell() {
        let c = table_with_cell_anchor("cell");
        let ctxs = anchor_contexts(&c);
        let mut bare = without_anchors(&c);
        assert_eq!(
            locate(&bare, &ctxs[0], None),
            None,
            "the one place is a cell"
        );
        let cell = Surface::Slot {
            island: c.islands[0].id.clone(),
            slot: first_cell(&c),
        };
        assert_eq!(locate_site(&bare, &ctxs[0], None), Some((cell, 6, 10)));
        assert_eq!(
            locate_site(&bare, &ctxs[0], Some(&[(0, 100)])),
            None,
            "a delta binds the prose alone"
        );
        let out = bare.reattach(&ctxs, None, &none());
        assert_eq!((out.reattached, bare), (ctxs, c));
    }

    #[test]
    fn a_revise_keeps_an_unchanged_cells_anchor_and_drops_an_edited_ones() {
        let c = table_with_cell_anchor("cell");
        let md = to_markdown(&c);
        let kept = revise(&c, &md).unwrap().content;
        assert_eq!(anchor_contexts(&kept), anchor_contexts(&c));

        let dropped = revise(&c, &md.replace("alpha beta", "alpha delta"))
            .unwrap()
            .content;
        assert!(anchor_contexts(&dropped).is_empty());
        assert_eq!(
            dropped.islands[0].id, c.islands[0].id,
            "the edited table keeps its id"
        );
    }

    /// A context spells its quote as stored, a hard break and a block break
    /// each as its own character, in the prose and in a slot alike.
    #[test]
    fn a_context_reads_the_stored_spelling() {
        let c = value(json!({
            "text": "one\u{2028}two\nthree \u{FFFC}", "blocks": [{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}],
            "anchors": [{"id": "a", "start": 0, "end": 7}, {"id": "p", "start": 10, "end": 10}],
            "islands": [{"id": "f0", "type": "x-box", "props": {}, "slots": {"note": {
                "text": "a\u{2028}\nb", "blocks": [{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}],
                "anchors": [{"id": "n", "start": 0, "end": 4}]}}}],
        }));
        let ctxs = anchor_contexts(&c);
        assert_eq!(
            ctxs.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["a", "p", "n"]
        );
        assert_eq!(ctxs[0].quote, "one\u{2028}two");
        assert_eq!(
            (ctxs[1].prefix.as_str(), ctxs[1].suffix.as_str()),
            ("one\u{2028}two\nth", "ree \u{FFFC}")
        );
        assert_eq!(ctxs[2].quote, "a\u{2028}\nb");
    }

    /// A selector spelled either way re-attaches: as stored, or as the line
    /// form spelled it, a hard break as `\n` and a slot's block break as two.
    #[test]
    fn a_selector_in_either_spelling_reattaches() {
        let stored = value(json!({
            "text": "one\u{2028}two\nthree \u{FFFC}", "blocks": [{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}],
            "islands": [{"id": "f0", "type": "x-box", "props": {}, "slots": {"note": {
                "text": "ab\u{2028}\ncd", "blocks": [{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}]}}}],
        }));
        for (quote, note) in [
            ("one\u{2028}two", "ab\u{2028}\ncd"),
            ("one\ntwo", "ab\n\n\ncd"),
        ] {
            let mut c = stored.clone();
            let ctxs = [
                AnchorContext::new("a", quote, "", "\nthree"),
                AnchorContext::new("n", note, "", ""),
            ];
            let out = c.reattach(&ctxs, None, &HashSet::new());
            assert_eq!(out.orphaned, [], "{quote:?}");
            assert_eq!(c.anchors, [Anchor::new("a", 0, 7)], "{quote:?}");
            assert_eq!(
                c.islands[0].slots["note"].anchors,
                [Anchor::new("n", 0, 6)],
                "{note:?}"
            );
            assert_eq!(out.reattached[1].quote, "ab\u{2028}\ncd");
        }
    }
}
