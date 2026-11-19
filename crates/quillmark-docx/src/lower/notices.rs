use std::collections::HashSet;

use crate::layout::{BracketHint, ItemKind, ItemRef, LayoutDoc, NodeId, NodeKind, Notice};

use super::tables::tabbed_hint;

use super::Lower;
use super::paragraph::runs_script;
use crate::notice::Code;

impl<'a> Lower<'a> {
    /// What this lowering found, then what the tree as a whole shows.
    pub(super) fn notices(&mut self) -> Vec<Notice> {
        let doc = self.doc;
        let (shapes, ink) = dropped(doc, &self.consumed);
        let t = &mut self.tally;
        t.count(Code::LinkScript, "", script_links(doc));
        t.count(Code::DirectionDropped, "", right_to_left(doc));
        t.count(Code::HintMisused, "", misused_hints(doc));
        t.count(Code::ShapeDropped, "", shapes);
        t.count(Code::InkDropped, "", ink);
        t.notices()
    }
}

/// The links that run script.
fn script_links(doc: &LayoutDoc) -> usize {
    doc.nodes
        .iter()
        .filter(|n| matches!(&n.kind, NodeKind::Link { url } if runs_script(url)))
        .count()
}

/// The shapes, and the text and images, that lowering did not `consume`.
fn dropped(doc: &LayoutDoc, consumed: &HashSet<ItemRef>) -> (usize, usize) {
    let mut shapes = 0;
    let mut ink = 0;
    for (p, page) in doc.pages.iter().enumerate() {
        for (i, item) in page.items.iter().enumerate() {
            let r = ItemRef {
                page: p as u32,
                index: i as u32,
            };
            if consumed.contains(&r) {
                continue;
            }
            match item.kind {
                ItemKind::Text(_) | ItemKind::Image { .. } | ItemKind::Raster { .. } => ink += 1,
                ItemKind::Shape(_) => shapes += 1,
                ItemKind::Mark(_) => {}
            }
        }
    }
    (shapes, ink)
}

/// The paragraphs whose text holds a right-to-left letter.
fn right_to_left(doc: &LayoutDoc) -> usize {
    let rtl = |c: char| {
        matches!(c as u32,
            0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFC | 0x10800..=0x10FFF
                | 0x1E800..=0x1EFFF)
    };
    doc.nodes
        .iter()
        .filter(|n| {
            n.lines
                .iter()
                .flat_map(|l| &l.items)
                .any(|r| matches!(&doc.item(*r).kind, ItemKind::Text(t) if t.text.chars().any(rtl)))
        })
        .count()
}

/// The `label` hints outside any `list`, and the `layout` and `chrome` hints
/// that hold no grid or no place.
fn misused_hints(doc: &LayoutDoc) -> usize {
    let hint = |k: &NodeKind, of: fn(&BracketHint) -> bool| matches!(k, NodeKind::Hint(h) if of(h));
    let kind = |n: NodeId| &doc.nodes[n].kind;
    let tabbed: HashSet<NodeId> = (0..doc.nodes.len())
        .filter(|&n| matches!(kind(n), NodeKind::Table { .. }))
        .filter_map(|n| tabbed_hint(doc, n))
        .collect();
    (0..doc.nodes.len())
        .filter(|&n| match kind(n) {
            NodeKind::Hint(BracketHint::Label { .. }) => doc
                .ancestor(n, |k| hint(k, |h| matches!(h, BracketHint::List { .. })))
                .is_none(),
            NodeKind::Hint(BracketHint::Tabbed) => !tabbed.contains(&n),
            NodeKind::Hint(BracketHint::Chrome) => !doc
                .subtree(n)
                .iter()
                .any(|&m| matches!(kind(m), NodeKind::Place { .. })),
            _ => false,
        })
        .count()
}
