//! Each anchor's context, read off the stored text, and the re-attachment of
//! a dropped one where its context is found exactly once.

use std::collections::HashSet;

use crate::content::lines::anchor::{ANCHOR_CONTEXT_SCALARS, locate_in};
pub use crate::content::lines::anchor::{AnchorContext, Reattached};
use crate::content::model::{Anchor, Content, ISLAND_SLOT, Scalar};

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
    let slots = c.text.chars().enumerate().filter(|&(_, ch)| ch == ISLAND_SLOT).map(|(at, _)| at);
    for (island, at) in c.islands.iter().zip(slots) {
        for (n, slot) in island.slot_order().iter().enumerate() {
            let Some(inner) = island.slots.get(slot) else { continue };
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
        prefix: text[start.saturating_sub(ANCHOR_CONTEXT_SCALARS)..start].iter().collect(),
        suffix: text[end..(end + ANCHOR_CONTEXT_SCALARS).min(text.len())].iter().collect(),
    }
}

/// The one surface and range `ctx` names in `c`, under
/// [`locate_site`](crate::content::lines::anchor::locate_site)'s rule over the stored
/// spelling. With `inserted` given, the prose alone counts.
pub fn locate_site(c: &Content, ctx: &AnchorContext, inserted: Option<&[(Scalar, Scalar)]>) -> Option<(Surface, Scalar, Scalar)> {
    let surfaces: Vec<(Surface, &str)> = match inserted {
        Some(_) => vec![(Surface::Prose, c.text.as_str())],
        None => std::iter::once((Surface::Prose, c.text.as_str()))
            .chain(c.islands.iter().flat_map(|i| {
                i.slot_order().into_iter().filter_map(move |s| {
                    i.slots.get(&s).map(|inner| (Surface::Slot { island: i.id.clone(), slot: s.clone() }, inner.text.as_str()))
                })
            }))
            .collect(),
    };
    let mut hits = surfaces.iter().filter_map(|(surface, text)| {
        let chars: Vec<char> = text.chars().collect();
        locate_in(&chars, ctx, inserted).map(|(s, e)| (surface.clone(), s, e))
    });
    let hit = hits.next();
    hit.filter(|_| hits.next().is_none())
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
        let ctxs: Vec<&AnchorContext> = ctxs.iter().filter(|c| answered.insert(c.id.as_str())).collect();
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
                        if let Some(inner) = self.islands.iter_mut().find(|i| &i.id == island).and_then(|i| i.slots.get_mut(slot)) {
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
                true => out.reattached.push(anchor_context(self, &ctx.id).unwrap_or_else(|| ctx.clone())),
                false => out.orphaned.push(ctx.clone()),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn value(v: serde_json::Value) -> Content {
        Content::from_value(&v).unwrap()
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
        assert_eq!(ctxs.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["a", "p", "n"]);
        assert_eq!(ctxs[0].quote, "one\u{2028}two");
        assert_eq!((ctxs[1].prefix.as_str(), ctxs[1].suffix.as_str()), ("one\u{2028}two\nth", "ree \u{FFFC}"));
        assert_eq!(ctxs[2].quote, "a\u{2028}\nb");
    }

    /// A selector spelled as stored reattaches, in the prose and in a slot.
    #[test]
    fn a_selector_in_the_stored_spelling_reattaches() {
        let mut c = value(json!({
            "text": "one\u{2028}two\nthree \u{FFFC}", "blocks": [{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}],
            "islands": [{"id": "f0", "type": "x-box", "props": {}, "slots": {"note": {
                "text": "ab\u{2028}\ncd", "blocks": [{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}]}}}],
        }));
        let ctxs = [AnchorContext::new("a", "one\u{2028}two", "", "\nthree"), AnchorContext::new("n", "ab\u{2028}\ncd", "", "")];
        let out = c.reattach(&ctxs, None, &HashSet::new());
        assert_eq!(out.orphaned, []);
        assert_eq!(c.anchors, [Anchor::new("a", 0, 7)]);
        assert_eq!(c.islands[0].slots["note"].anchors, [Anchor::new("n", 0, 6)]);
    }
}
