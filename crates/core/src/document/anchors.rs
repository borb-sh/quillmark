//! The comment-anchor handle over a whole document: each prose anchor's
//! selector at its path, and the exact re-attachment of the ones a write
//! dropped. The contract is `prose/canon/COMMENTS.md`.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use quillmark_content::anchor::{anchor_context, anchor_contexts, AnchorContext, Reattached};
use quillmark_content::delta::Delta;
use quillmark_content::model::{Content, Mark, MarkKind, Normalized};
use quillmark_content::serial::{from_canonical_value, to_canonical_value};
use serde_json::Value as JsonValue;

use super::{Card, Document};
use crate::path::{DocPath, DocSeg};

/// A prose anchor's context at its address: the selector a host stores beside
/// a comment thread, flat as [`DocumentAnchor`](super::DocumentAnchor) is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnchorSelector {
    pub id: String,
    /// The body or content value holding the anchor.
    pub path: DocPath,
    /// The anchored text as stored, island slots and line breaks included;
    /// empty for a point.
    pub quote: String,
    /// Up to [`ANCHOR_CONTEXT_USV`](quillmark_content::anchor::ANCHOR_CONTEXT_USV)
    /// scalar values before the anchor, cut at the value's start.
    pub prefix: String,
    /// Up to [`ANCHOR_CONTEXT_USV`](quillmark_content::anchor::ANCHOR_CONTEXT_USV)
    /// scalar values after the anchor, cut at the value's end.
    pub suffix: String,
}

impl AnchorSelector {
    pub fn new(
        id: impl Into<String>,
        path: DocPath,
        quote: impl Into<String>,
        prefix: impl Into<String>,
        suffix: impl Into<String>,
    ) -> Self {
        AnchorSelector {
            id: id.into(),
            path,
            quote: quote.into(),
            prefix: prefix.into(),
            suffix: suffix.into(),
        }
    }

    fn at(ctx: AnchorContext, path: &DocPath) -> Self {
        AnchorSelector::new(ctx.id, path.clone(), ctx.quote, ctx.prefix, ctx.suffix)
    }

    fn context(&self) -> AnchorContext {
        AnchorContext::new(&*self.id, &*self.quote, &*self.prefix, &*self.suffix)
    }
}

impl Document {
    /// Every prose anchor of every body and content value, at any depth of a
    /// field, with its context: each card's fields, then its body, as
    /// [`to_markdown_annotated`](Self::to_markdown_annotated) lists them. A pure
    /// read. An anchor in a table cell is outside it.
    pub fn anchor_contexts(&self) -> Vec<AnchorSelector> {
        let mut out = Vec::new();
        self.each_content(&mut |path, content| {
            out.extend(anchor_contexts(content).into_iter().map(|ctx| AnchorSelector::at(ctx, path)));
            ControlFlow::Continue(())
        });
        out
    }

    /// The selector of the prose anchor `id`, or `None` where no body or
    /// content value holds one. Ids are unique per value, not per document:
    /// the first value holding `id`, in [`anchor_contexts`](Self::anchor_contexts)
    /// order, answers.
    pub fn anchor_context(&self, id: &str) -> Option<AnchorSelector> {
        let mut found = None;
        self.each_content(&mut |path, content| match anchor_context(content, id) {
            Some(ctx) => {
                found = Some(AnchorSelector::at(ctx, path));
                ControlFlow::Break(())
            }
            None => ControlFlow::Continue(()),
        });
        found
    }

    /// Re-add dropped anchors, each at the path its selector names, where the
    /// quote is found under the exact rule of
    /// [`locate`](quillmark_content::anchor::locate). Answers each selector in
    /// the order given, reattached with its selector as read after the write,
    /// or orphaned as given. Each value a selector names is read and written
    /// once, however many selectors name it.
    ///
    /// - Each id is answered once: of selectors sharing an id, the first is
    ///   answered and the rest are left out of the receipt.
    /// - Path-strict: a path naming no body or content value orphans its
    ///   selector, and nothing here remaps a card index. A card path's kind
    ///   must be the card's.
    /// - An id live anywhere in the document, a table cell's anchor included,
    ///   is not written: it is answered as reattached, with the selector the
    ///   first value holding it reads, or as given for an id only a cell holds.
    /// - `deltas` holds a write's [`Delta`] per path where the caller has one,
    ///   and a match at that path must then overlap text the write inserted.
    pub fn reattach(
        &mut self,
        selectors: &[AnchorSelector],
        deltas: &HashMap<DocPath, Delta>,
    ) -> Reattached<AnchorSelector> {
        let mut answered = HashSet::new();
        let selectors: Vec<&AnchorSelector> =
            selectors.iter().filter(|s| answered.insert(s.id.as_str())).collect();

        let mut known: HashMap<String, AnchorSelector> = HashMap::new();
        let mut cells: HashSet<String> = HashSet::new();
        self.each_content(&mut |path, content| {
            for ctx in anchor_contexts(content) {
                known.entry(ctx.id.clone()).or_insert_with(|| AnchorSelector::at(ctx, path));
            }
            cells.extend(cell_anchors(content));
            ControlFlow::Continue(())
        });

        let mut answer: Vec<Option<AnchorSelector>> = Vec::with_capacity(selectors.len());
        let mut pending: Vec<(&DocPath, Vec<usize>)> = Vec::new();
        for (i, selector) in selectors.iter().enumerate() {
            if let Some(live) = known.get(&selector.id) {
                answer.push(Some(live.clone()));
            } else if cells.contains(&selector.id) {
                answer.push(Some((*selector).clone()));
            } else {
                answer.push(None);
                match pending.iter_mut().find(|(path, _)| **path == selector.path) {
                    Some((_, at)) => at.push(i),
                    None => pending.push((&selector.path, vec![i])),
                }
            }
        }

        for (path, at) in pending {
            let ctxs: Vec<AnchorContext> = at.iter().map(|&i| selectors[i].context()).collect();
            let inserted = deltas.get(path).map(Delta::inserted_spans);
            let Some(placed) = self.edit_content_at(path, |content| {
                content.reattach(&ctxs, inserted.as_deref(), &HashSet::new()).reattached
            }) else {
                continue;
            };
            let mut placed: HashMap<String, AnchorContext> =
                placed.into_iter().map(|ctx| (ctx.id.clone(), ctx)).collect();
            for i in at {
                answer[i] = placed.remove(&selectors[i].id).map(|ctx| AnchorSelector::at(ctx, path));
            }
        }

        let mut out = Reattached::default();
        for (selector, answer) in selectors.into_iter().zip(answer) {
            match answer {
                Some(fresh) => out.reattached.push(fresh),
                None => out.orphaned.push(selector.clone()),
            }
        }
        out
    }

    /// Run `f` on the body or content value `path` names, writing the value
    /// back where `f` changed it, or `None` where `path` names neither. A field
    /// value counts where it is an object
    /// [`from_canonical_value`] accepts, the test the revise's drop report
    /// applies, and lands back in canonical form in place, so the comments
    /// around it stay.
    pub(crate) fn edit_content_at<R>(
        &mut self,
        path: &DocPath,
        f: impl FnOnce(&mut Normalized) -> R,
    ) -> Option<R> {
        let (card, rest) = match path.segs() {
            [DocSeg::Main, rest @ ..] => (&mut self.main, rest),
            [DocSeg::Card { kind, index }, rest @ ..] => {
                let card = self.cards.get_mut(*index)?;
                if card.kind() != kind.as_deref() {
                    return None;
                }
                (card, rest)
            }
            _ => return None,
        };
        match rest {
            [DocSeg::Body] => Some(f(card.body_mut())),
            [DocSeg::Field { name }, tail @ ..] => {
                let (_, value) = card.payload_mut().fields_mut().find(|(key, _)| *key == name)?;
                let leaf = tail.iter().try_fold(value.as_json_mut(), |value, seg| match seg {
                    DocSeg::Field { name } => value.as_object_mut()?.get_mut(name),
                    DocSeg::Index { index } => value.as_array_mut()?.get_mut(*index),
                    _ => None,
                })?;
                let mut content = content_of(leaf)?;
                let before = content.clone();
                let out = f(&mut content);
                if content != before {
                    *leaf = to_canonical_value(&content);
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// `visit` each body and content value with its path, in the order
    /// [`anchor_contexts`](Self::anchor_contexts) lists them.
    fn each_content(&self, visit: &mut Visit<'_>) {
        let roots = std::iter::once((DocPath::main(), &self.main))
            .chain(self.cards.iter().enumerate().map(|(i, card)| (DocPath::card(card.kind(), i), card)));
        for (at, card) in roots {
            if each_in_card(card, &at, visit).is_break() {
                return;
            }
        }
    }
}

type Visit<'a> = dyn FnMut(&DocPath, &Normalized) -> ControlFlow<()> + 'a;

fn each_in_card(card: &Card, at: &DocPath, visit: &mut Visit<'_>) -> ControlFlow<()> {
    for (name, value) in card.payload().iter() {
        each_in_value(value.as_json(), &at.field(name), visit)?;
    }
    visit(&at.body(), card.body())
}

fn each_in_value(value: &JsonValue, at: &DocPath, visit: &mut Visit<'_>) -> ControlFlow<()> {
    if let Some(content) = content_of(value) {
        return visit(at, &content);
    }
    match value {
        JsonValue::Object(map) => {
            for (key, value) in map {
                each_in_value(value, &at.field(key), visit)?;
            }
        }
        JsonValue::Array(items) => {
            for (i, value) in items.iter().enumerate() {
                each_in_value(value, &at.index(i), visit)?;
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn content_of(value: &JsonValue) -> Option<Normalized> {
    value.is_object().then(|| from_canonical_value(value).ok()).flatten()
}

fn cell_anchors(content: &Content) -> impl Iterator<Item = String> + '_ {
    content
        .islands
        .iter()
        .flat_map(|i| i.island_type.cell_marks(&i.props))
        .flat_map(|(_, marks)| marks)
        .filter_map(|m: Mark| match m.kind {
            MarkKind::Anchor { id } => Some(id),
            _ => None,
        })
}
