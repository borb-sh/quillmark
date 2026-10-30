//! The comment-anchor handle over a whole document: each prose anchor's
//! selector at its path, and the exact re-attachment of the ones a write
//! dropped. The contract is `prose/canon/COMMENTS.md`.

use std::collections::{HashMap, HashSet};

use quillmark_content::anchor::{anchor_contexts, AnchorContext, Reattached};
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
        });
        out
    }

    /// The selector of the prose anchor `id`, or `None` where no body or
    /// content value holds one.
    pub fn anchor_context(&self, id: &str) -> Option<AnchorSelector> {
        self.anchor_contexts().into_iter().find(|s| s.id == id)
    }

    /// Re-add dropped anchors, each at the path its selector names, where the
    /// quote is found under the exact rule of
    /// [`locate`](quillmark_content::anchor::locate). Answers each selector in
    /// the order given, reattached with its selector as read after the write,
    /// or orphaned as given.
    ///
    /// - Path-strict: a path naming no body or content value orphans its
    ///   selector, and nothing here remaps a card index. A card path's kind
    ///   must be the card's.
    /// - An id live anywhere in the document, a table cell's anchor included,
    ///   is not written: it is answered as reattached, with the selector it
    ///   now reads at, or as given for a cell's.
    /// - `deltas` holds a write's [`Delta`] per path where the caller has one,
    ///   and a match at that path must then overlap text the write inserted.
    pub fn reattach(
        &mut self,
        selectors: &[AnchorSelector],
        deltas: &HashMap<DocPath, Delta>,
    ) -> Reattached<AnchorSelector> {
        let mut known: HashMap<String, AnchorSelector> = HashMap::new();
        for selector in self.anchor_contexts() {
            known.entry(selector.id.clone()).or_insert(selector);
        }
        let mut live = self.cell_anchor_ids();
        live.extend(known.keys().cloned());

        let mut out = Reattached::default();
        for selector in selectors {
            if live.contains(&selector.id) {
                out.reattached.push(known.get(&selector.id).unwrap_or(selector).clone());
                continue;
            }
            let inserted = deltas.get(&selector.path).map(Delta::inserted_spans);
            let ctx = selector.context();
            let placed = self
                .edit_content_at(&selector.path, |content| {
                    content.reattach(std::slice::from_ref(&ctx), inserted.as_deref(), &HashSet::new())
                })
                .and_then(|mut r| r.reattached.pop());
            match placed {
                Some(ctx) => {
                    let fresh = AnchorSelector::at(ctx, &selector.path);
                    live.insert(fresh.id.clone());
                    known.insert(fresh.id.clone(), fresh.clone());
                    out.reattached.push(fresh);
                }
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
    fn each_content(&self, visit: &mut dyn FnMut(&DocPath, &Normalized)) {
        let roots = std::iter::once((DocPath::main(), &self.main))
            .chain(self.cards.iter().enumerate().map(|(i, card)| (DocPath::card(card.kind(), i), card)));
        for (at, card) in roots {
            each_in_card(card, &at, visit);
        }
    }

    /// The ids of every anchor a table cell holds, in any body or content
    /// value.
    fn cell_anchor_ids(&self) -> HashSet<String> {
        let mut ids = HashSet::new();
        self.each_content(&mut |_, content| ids.extend(cell_anchors(content)));
        ids
    }
}

fn each_in_card(card: &Card, at: &DocPath, visit: &mut dyn FnMut(&DocPath, &Normalized)) {
    for (name, value) in card.payload().iter() {
        each_in_value(value.as_json(), &at.field(name), visit);
    }
    visit(&at.body(), card.body());
}

fn each_in_value(value: &JsonValue, at: &DocPath, visit: &mut dyn FnMut(&DocPath, &Normalized)) {
    if let Some(content) = content_of(value) {
        return visit(at, &content);
    }
    match value {
        JsonValue::Object(map) => {
            for (key, value) in map {
                each_in_value(value, &at.field(key), visit);
            }
        }
        JsonValue::Array(items) => {
            for (i, value) in items.iter().enumerate() {
                each_in_value(value, &at.index(i), visit);
            }
        }
        _ => {}
    }
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
