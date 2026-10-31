//! What a prose anchor is attached to, and the one exact rule that places a
//! dropped anchor again. The contract is `prose/canon/COMMENTS.md`.
//!
//! Both read the stored text as it is: an inline island is its [`ISLAND_SLOT`]
//! character and a line break its `\n`. A quote spanning an island holds its
//! slot, and is found again only where a slot sits.

use std::collections::{HashMap, HashSet};

use crate::delta::MIN_MOVE;
use crate::model::{Content, ISLAND_SLOT, Mark, MarkKind, Normalized, Usv};

/// Context read on each side of an anchor, in Unicode scalar values.
pub const ANCHOR_CONTEXT_USV: usize = 32;

/// Context that must agree on each side for a match. A context the field's
/// edge cut shorter agrees as far as it reaches, so text written past that
/// edge since the read does not orphan the anchor.
pub const ANCHOR_AGREE_USV: usize = 8;

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

/// The text surface an anchor sits on: the prose, or one cell of an island
/// with a cell model (a table's cell, a footnote's note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Surface {
    Prose,
    /// The `cell`th cell [`IslandType::cell_marks`](crate::island::IslandType::cell_marks)
    /// reads of the `island`th island.
    Cell {
        island: usize,
        cell: usize,
    },
}

/// One anchor of a content, on whichever surface holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorSite {
    pub id: String,
    pub surface: Surface,
    /// The range in the surface's own text.
    pub start: Usv,
    pub end: Usv,
}

impl Content {
    /// Every anchor of this content, in the prose and in each island's cells,
    /// in reading order: by the position in the text where each stands, a
    /// cell's at its island's slot, then by surface, start and id. One
    /// inverted or past its surface's text, which
    /// [`validate`](Content::validate) or the cell model refuses, is left out.
    pub fn anchor_sites(&self) -> Vec<AnchorSite> {
        let len = self.len_usv();
        let mut sites: Vec<(Usv, AnchorSite)> = self
            .marks
            .iter()
            .filter_map(|m| match &m.kind {
                MarkKind::Anchor { id } if m.start <= m.end && m.end <= len => Some((
                    m.start,
                    AnchorSite {
                        id: id.clone(),
                        surface: Surface::Prose,
                        start: m.start,
                        end: m.end,
                    },
                )),
                _ => None,
            })
            .collect();
        let slots = self
            .text
            .chars()
            .enumerate()
            .filter(|&(_, c)| c == ISLAND_SLOT)
            .map(|(at, _)| at);
        for (island, (isl, at)) in self.islands.iter().zip(slots).enumerate() {
            for (cell, (text, marks)) in isl
                .island_type
                .cell_marks(&isl.props)
                .into_iter()
                .enumerate()
            {
                let n = text.chars().count();
                sites.extend(marks.into_iter().filter_map(|m| match m.kind {
                    MarkKind::Anchor { id } if m.start <= m.end && m.end <= n => Some((
                        at,
                        AnchorSite {
                            id,
                            surface: Surface::Cell { island, cell },
                            start: m.start,
                            end: m.end,
                        },
                    )),
                    _ => None,
                }));
            }
        }
        sites.sort_by(|(a, x), (b, y)| {
            (a, x.surface, x.start, &x.id).cmp(&(b, y.surface, y.start, &y.id))
        });
        sites.into_iter().map(|(_, site)| site).collect()
    }

    /// Each anchor id more than one of [`anchor_sites`](Self::anchor_sites)
    /// holds, sorted: a collision a cell takes part in, which
    /// [`validate`](Content::validate) does not refuse, so a stored value
    /// holding one still loads.
    pub fn anchor_collisions(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut twice: Vec<String> = self
            .anchor_sites()
            .into_iter()
            .filter(|site| !seen.insert(site.id.clone()))
            .map(|site| site.id)
            .collect();
        twice.sort_unstable();
        twice.dedup();
        twice
    }

    /// The text of `surface`: the prose, or the cell's own text.
    pub(crate) fn surface_text(&self, surface: Surface) -> Option<String> {
        match surface {
            Surface::Prose => Some(self.text.clone()),
            Surface::Cell { island, cell } => {
                let isl = self.islands.get(island)?;
                isl.island_type
                    .cell_marks(&isl.props)
                    .into_iter()
                    .nth(cell)
                    .map(|(text, _)| text)
            }
        }
    }

    /// Every text surface: the prose, then each island's cells in order.
    fn surfaces(&self) -> Vec<(Surface, Vec<char>)> {
        let mut out = vec![(Surface::Prose, self.text.chars().collect())];
        for (island, isl) in self.islands.iter().enumerate() {
            for (cell, (text, _)) in isl
                .island_type
                .cell_marks(&isl.props)
                .into_iter()
                .enumerate()
            {
                out.push((Surface::Cell { island, cell }, text.chars().collect()));
            }
        }
        out
    }
}

/// Every anchor of `rt` with its context, in the order
/// [`anchor_sites`](Content::anchor_sites) reads them, the order
/// [`anchors_where`](crate::export::anchors_where) lists them. A cell anchor's
/// context is read off its cell's text and cut at the cell's edges. Read off
/// the stored text, so it is pure and moves nothing.
pub fn anchor_contexts(rt: &Normalized) -> Vec<AnchorContext> {
    contexts_where(rt, |_| true)
}

/// The [`anchor_contexts`] entry for `id`, reading no other anchor's context.
pub fn anchor_context(rt: &Normalized, id: &str) -> Option<AnchorContext> {
    contexts_where(rt, |held| held == id).into_iter().next()
}

fn contexts_where(rt: &Normalized, keep: impl Fn(&str) -> bool) -> Vec<AnchorContext> {
    let sites: Vec<AnchorSite> = rt
        .anchor_sites()
        .into_iter()
        .filter(|s| keep(&s.id))
        .collect();
    let mut texts: HashMap<Surface, Vec<char>> = HashMap::new();
    sites
        .into_iter()
        .filter_map(|site| {
            let chars = match texts.entry(site.surface) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(rt.surface_text(site.surface)?.chars().collect())
                }
            };
            let (start, end) = (site.start, site.end);
            Some(AnchorContext {
                id: site.id,
                quote: chars[start..end].iter().collect(),
                prefix: chars[start.saturating_sub(ANCHOR_CONTEXT_USV)..start]
                    .iter()
                    .collect(),
                suffix: chars[end..(end + ANCHOR_CONTEXT_USV).min(chars.len())]
                    .iter()
                    .collect(),
            })
        })
        .collect()
}

/// The one range `ctx` names in `rt`, or `None`. Never a nearest match:
///
/// - A quote shorter than [`MIN_ANCHOR_QUOTE`] names none, unless it is island
///   slots alone: an anchor over a table's slot is placed on its context, the
///   way a point is.
/// - Of every occurrence of the quote in the stored text, one counts where the
///   text before it ends with the last [`ANCHOR_AGREE_USV`] of `prefix` and the
///   text after it starts with the first [`ANCHOR_AGREE_USV`] of `suffix`. A
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
/// The prose range of [`locate_site`]: `None` where the one counting place is
/// in a cell.
pub fn locate(
    rt: &Normalized,
    ctx: &AnchorContext,
    inserted: Option<&[(Usv, Usv)]>,
) -> Option<(Usv, Usv)> {
    match locate_site(rt, ctx, inserted)? {
        (Surface::Prose, start, end) => Some((start, end)),
        _ => None,
    }
}

/// [`locate`] over every text surface: the prose and each island's cells,
/// each read as its own text, so a context is cut at a cell's edges. Exactly
/// one counting place over all of them names its surface and range. With
/// `inserted` given, which spans the prose alone, only the prose counts.
pub fn locate_site(
    rt: &Normalized,
    ctx: &AnchorContext,
    inserted: Option<&[(Usv, Usv)]>,
) -> Option<(Surface, Usv, Usv)> {
    locate_among(&searched(rt, inserted), ctx, inserted)
}

fn searched(rt: &Normalized, inserted: Option<&[(Usv, Usv)]>) -> Vec<(Surface, Vec<char>)> {
    match inserted {
        Some(_) => vec![(Surface::Prose, rt.text.chars().collect())],
        None => rt.surfaces(),
    }
}

fn locate_among(
    surfaces: &[(Surface, Vec<char>)],
    ctx: &AnchorContext,
    inserted: Option<&[(Usv, Usv)]>,
) -> Option<(Surface, Usv, Usv)> {
    let mut hits = surfaces.iter().filter_map(|(surface, text)| {
        locate_in(text, ctx, inserted).map(|(start, end)| (*surface, start, end))
    });
    let hit = hits.next()?;
    hits.next().is_none().then_some(hit)
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
    /// Re-add each context's anchor where [`locate_site`] finds it, as a mark
    /// of the prose or of its cell, and answer which landed. An id in `live`, or already an anchor
    /// here on any surface, is not written and is answered as reattached at
    /// its live range. Never mints an id, never moves a live anchor, and
    /// places each anchor whole or not at all.
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
        let ctxs: Vec<&AnchorContext> = ctxs
            .iter()
            .filter(|c| answered.insert(c.id.as_str()))
            .collect();
        let held: HashSet<String> = self
            .anchor_sites()
            .into_iter()
            .map(|site| site.id)
            .collect();
        let surfaces = searched(self, inserted);
        let mut placed = Vec::with_capacity(ctxs.len());
        let mut marks = Vec::new();
        for ctx in &ctxs {
            if live.contains(ctx.id.as_str()) || held.contains(&ctx.id) {
                placed.push(true);
                continue;
            }
            let site = locate_among(&surfaces, ctx, inserted).filter(|_| !ctx.id.is_empty());
            if let Some((surface, start, end)) = site {
                marks.push((
                    surface,
                    Mark::new(start, end, MarkKind::Anchor { id: ctx.id.clone() }),
                ));
            }
            placed.push(site.is_some());
        }
        if !marks.is_empty() {
            let mut content =
                std::mem::replace(self, Content::empty().into_normalized()).into_content();
            let mut cells: HashMap<(usize, usize), Vec<Mark>> = HashMap::new();
            for (surface, mark) in marks {
                match surface {
                    Surface::Prose => content.marks.push(mark),
                    Surface::Cell { island, cell } => {
                        cells.entry((island, cell)).or_default().push(mark)
                    }
                }
            }
            for (island, isl) in content.islands.iter_mut().enumerate() {
                for (cell, value) in isl
                    .island_type
                    .cell_values_mut(&mut isl.props)
                    .into_iter()
                    .enumerate()
                {
                    if let Some(added) = cells.remove(&(island, cell)) {
                        let (_, mut held) = crate::serial::parse_cell(value);
                        held.extend(added);
                        crate::serial::set_cell_marks(value, &held);
                    }
                }
            }
            *self = content.into_normalized();
        }
        let mut fresh: HashMap<String, AnchorContext> = anchor_contexts(self)
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();
        let mut out = Reattached::default();
        for (ctx, landed) in ctxs.into_iter().zip(placed) {
            if landed {
                out.reattached
                    .push(fresh.remove(&ctx.id).unwrap_or_else(|| ctx.clone()));
            } else {
                out.orphaned.push(ctx.clone());
            }
        }
        out
    }
}

fn locate_in(
    text: &[char],
    ctx: &AnchorContext,
    inserted: Option<&[(Usv, Usv)]>,
) -> Option<(Usv, Usv)> {
    let quote: Vec<char> = ctx.quote.chars().collect();
    let prefix: Vec<char> = ctx.prefix.chars().collect();
    let suffix: Vec<char> = ctx.suffix.chars().collect();
    let n = text.len();
    let touches = |(a, b): (Usv, Usv)| {
        inserted.is_none_or(|spans| spans.iter().any(|&(s, e)| a < e && b > s))
    };
    let agrees = |i: usize, j: usize| {
        agrees_before(&text[..i], &prefix) && agrees_after(&text[j..], &suffix)
    };
    let mut hits = Vec::new();
    if quote.is_empty() {
        let (before, after) = (
            prefix.len().min(ANCHOR_AGREE_USV),
            suffix.len().min(ANCHOR_AGREE_USV),
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
    before.ends_with(&prefix[prefix.len().saturating_sub(ANCHOR_AGREE_USV)..])
}

fn agrees_after(after: &[char], suffix: &[char]) -> bool {
    after.starts_with(&suffix[..suffix.len().min(ANCHOR_AGREE_USV)])
}

#[cfg(test)]
mod tests {
    mod properties;

    use super::*;
    use crate::delta::{diff_import, rebase_onto};
    use crate::import::from_markdown;
    use crate::model::Mark;
    use crate::ops::MarkOp;

    fn content(markdown: &str) -> Normalized {
        from_markdown(markdown).unwrap().content
    }

    fn with_anchor(rt: Normalized, start: Usv, end: Usv, id: &str) -> Normalized {
        let mut rt = rt.into_content();
        rt.marks
            .push(Mark::new(start, end, MarkKind::Anchor { id: id.into() }));
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
        let slots: Vec<Usv> = two
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
        let rt = content("and then gamma rises fast, and then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "and then ", " rises fast");
        assert_eq!(
            locate(&rt, &ctx, None),
            None,
            "two occurrences with equal context"
        );

        let rt = content("and then gamma rises fast, but then gamma rises fast.");
        let ctx = AnchorContext::new("c", "gamma", "but then ", " rises fast");
        let at = rt.text.rfind("gamma").unwrap();
        assert_eq!(locate(&rt, &ctx, None), Some((at, at + 5)));
    }

    #[test]
    fn a_delta_counts_only_a_match_overlapping_what_it_inserted() {
        let rt = content("Lorem ipsum dolor sit amet, consectetur adipiscing elit.");
        let at = rt.text.find("dolor sit").unwrap();
        let quote = AnchorContext::new(
            "c",
            "dolor sit",
            "Lorem ipsum ",
            " amet, consectetur adipiscing elit.",
        );
        assert_eq!(locate(&rt, &quote, None), Some((at, at + 9)));
        assert_eq!(
            locate(&rt, &quote, Some(&[(at + 8, at + 12)])),
            Some((at, at + 9))
        );
        assert_eq!(locate(&rt, &quote, Some(&[(at + 9, at + 12)])), None);

        let point = AnchorContext::new("p", "", "Lorem ipsum ", "dolor sit amet, consectetur");
        assert_eq!(locate(&rt, &point, None), Some((at, at)));
        assert_eq!(
            locate(&rt, &point, Some(&[(at + 7, at + 12)])),
            Some((at, at))
        );
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

        let (kept, _, _) =
            diff_import(&base, "Always keep the quarterly figures in view.").unwrap();
        assert_eq!(text(&kept, span(&kept, "c1").unwrap()), "quarterly figures");

        let (wide, _, _) = diff_import(&base, "Keep the quarterly sales figures in view.").unwrap();
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
        let (mut next, delta, _) = diff_import(&base, "keep the target two").unwrap();
        assert_eq!(span(&next, "c1"), None, "revise drops it");

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
        raw.marks
            .push(Mark::new(9, 3, MarkKind::Anchor { id: "bad".into() }));
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

    fn table_with_cell_anchor(id: &str) -> Normalized {
        let mut rt = content("Lead line here.\n\n| head | other |\n|---|---|\n| alpha beta | gamma |\n\nTail line here.").into_content();
        rt.islands[0].props["rows"][0][0]["marks"] =
            serde_json::json!([{ "start": 6, "end": 10, "type": "anchor", "attrs": { "id": id } }]);
        rt.into_normalized()
    }

    #[test]
    fn a_cell_anchor_is_a_site_read_with_its_cell_context() {
        let rt = with_anchor(table_with_cell_anchor("cell"), 0, 4, "lead");
        let sites = rt.anchor_sites();
        let ids: Vec<(&str, Surface)> = sites.iter().map(|s| (s.id.as_str(), s.surface)).collect();
        assert_eq!(
            ids,
            [
                ("lead", Surface::Prose),
                ("cell", Surface::Cell { island: 0, cell: 2 })
            ]
        );
        let ctx = anchor_context(&rt, "cell").unwrap();
        assert_eq!(
            (&*ctx.prefix, &*ctx.quote, &*ctx.suffix),
            ("alpha ", "beta", "")
        );
        assert!(rt.anchor_collisions().is_empty());

        let twice = with_anchor(table_with_cell_anchor("lead"), 0, 4, "lead");
        assert_eq!(
            twice.validate(),
            Ok(()),
            "a collision a cell takes part in loads"
        );
        assert_eq!(twice.anchor_collisions(), ["lead"]);
    }

    #[test]
    fn a_cell_anchor_reattaches_into_its_cell() {
        let rt = table_with_cell_anchor("cell");
        let ctxs = anchor_contexts(&rt);
        let mut bare = without_anchors(&rt).into_content();
        bare.islands[0].props["rows"][0][0]["marks"] = serde_json::json!([]);
        let mut bare = bare.into_normalized();
        assert_eq!(
            locate(&bare, &ctxs[0], None),
            None,
            "the one place is a cell"
        );
        assert_eq!(
            locate_site(&bare, &ctxs[0], None),
            Some((Surface::Cell { island: 0, cell: 2 }, 6, 10))
        );
        assert_eq!(
            locate_site(&bare, &ctxs[0], Some(&[(0, 100)])),
            None,
            "a delta binds the prose alone"
        );
        let out = bare.reattach(&ctxs, None, &none());
        assert_eq!((out.reattached, bare), (ctxs, rt));
    }

    #[test]
    fn the_ops_reach_cell_anchors_and_refuse_a_collision() {
        let mut rt = table_with_cell_anchor("cell");
        let add = MarkOp::Add {
            start: 0,
            end: 4,
            kind: MarkKind::Anchor { id: "cell".into() },
        };
        assert_eq!(
            rt.apply_mark_ops(&[add]),
            Err(crate::ops::ApplyError::AnchorIdCollision { id: "cell".into() })
        );

        let island = rt.islands[0].clone();
        let mut renamed = island.clone();
        renamed.id = "isl-9".into();
        let mut twice = rt.clone();
        let insert = crate::ops::IslandOp::Insert {
            at: twice.len_usv(),
            island: renamed,
        };
        assert_eq!(
            twice.apply_island_ops(&[insert]),
            Err(crate::ops::ApplyError::AnchorIdCollision { id: "cell".into() })
        );
        let set = crate::ops::IslandOp::Set {
            island: island.clone(),
        };
        assert_eq!(
            rt.clone().apply_island_ops(&[set]),
            Ok(()),
            "a set keeps the ids it replaces"
        );

        rt.apply_mark_ops(&[MarkOp::RemoveAnchor { id: "cell".into() }])
            .unwrap();
        assert!(rt.anchor_sites().is_empty());
    }

    #[test]
    fn a_revise_keeps_an_unchanged_cells_anchor_and_drops_an_edited_ones() {
        let rt = table_with_cell_anchor("cell");
        let md = crate::export::to_markdown(&rt);
        let (kept, _, _) = diff_import(&rt, &md).unwrap();
        assert_eq!(kept.anchor_sites(), rt.anchor_sites());

        let (dropped, _, _) = diff_import(&rt, &md.replace("alpha beta", "alpha delta")).unwrap();
        assert!(dropped.anchor_sites().is_empty());
        assert_eq!(
            dropped.islands[0].id, rt.islands[0].id,
            "the edited table keeps its id"
        );
    }
}
