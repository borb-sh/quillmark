use std::collections::HashSet;

use crate::layout::{BracketHint, ItemKind, ItemRef, LayoutDoc, NodeId, NodeKind, Notice};

use super::tables::tabbed_hint;

use super::paragraph::runs_script;
use super::{
    CODE_COLUMNS_FLATTENED, CODE_DIRECTION_DROPPED, CODE_FONT_NOT_EMBEDDED, CODE_HINT_UNCHECKED,
    CODE_INK_DROPPED, CODE_LABEL_MISMATCH, CODE_LINK_SCRIPT, CODE_PAGE_NUMBER_FROZEN,
    CODE_PLACE_ANCHORED, CODE_SHAPE_DROPPED, CODE_TOC_FROZEN, CODE_UNSTRUCTURED_TEXT, Lower,
};

impl<'a> Lower<'a> {
    pub(super) fn notices(&self) -> Vec<Notice> {
        let mut out = Vec::new();
        if self.frozen > 0 {
            out.push(Notice {
                code: CODE_TOC_FROZEN.into(),
                message: format!(
                    "{} table(s) of contents print page numbers Word's PAGE field cannot, so \
                     they are locked against an update",
                    self.frozen
                ),
            });
        }
        if self.mismatches > 0 {
            out.push(Notice {
                code: CODE_LABEL_MISMATCH.into(),
                message: format!(
                    "{} paragraph number(s) are not the number Word would count, so they stay as \
                     printed text",
                    self.mismatches
                ),
            });
        }
        if self.frozen_numbers > 0 {
            out.push(Notice {
                code: CODE_PAGE_NUMBER_FROZEN.into(),
                message: format!(
                    "{} printed page number(s) are not literal text around a number Word can \
                     count, so they stay as printed text",
                    self.frozen_numbers
                ),
            });
        }
        let scripts = script_links(self.doc);
        if scripts > 0 {
            out.push(Notice {
                code: CODE_LINK_SCRIPT.into(),
                message: format!("{scripts} link(s) that run script are carried as plain text"),
            });
        }
        if self.unstructured > 0 {
            out.push(Notice {
                code: CODE_UNSTRUCTURED_TEXT.into(),
                message: format!(
                    "{} line(s) of text that no element tags are set as a paragraph each, as \
                     printed",
                    self.unstructured
                ),
            });
        }
        if self.untabbed > 0 {
            out.push(Notice {
                code: CODE_HINT_UNCHECKED.into(),
                message: format!(
                    "{} grid(s) marked as tabbed lines wrap a cell before the last or hold a \
                     table, so they stay tables",
                    self.untabbed
                ),
            });
        }
        if self.flattened > 0 {
            out.push(Notice {
                code: CODE_COLUMNS_FLATTENED.into(),
                message: format!(
                    "{} block(s) set in columns within the flow are written as one column",
                    self.flattened
                ),
            });
        }
        let rtl = right_to_left(self.doc);
        if rtl > 0 {
            out.push(Notice {
                code: CODE_DIRECTION_DROPPED.into(),
                message: format!(
                    "{rtl} paragraph(s) hold right-to-left text, which the Word file sets with \
                     no text direction"
                ),
            });
        }
        let misused = misused_hints(self.doc);
        if misused > 0 {
            out.push(Notice {
                code: CODE_HINT_UNCHECKED.into(),
                message: format!(
                    "{misused} hint(s) mark what they cannot apply to: a label outside any list, \
                     a layout around no grid, or chrome around no place, so the export ignores \
                     them"
                ),
            });
        }
        for family in &self.unmapped {
            out.push(Notice {
                code: CODE_FONT_NOT_EMBEDDED.into(),
                message: format!(
                    "the Word file names the font {family} without embedding it, so Word \
                     substitutes another where it is not installed"
                ),
            });
        }
        if self.placed > 0 {
            out.push(Notice {
                code: CODE_PLACE_ANCHORED.into(),
                message: format!(
                    "{} placed object(s) are anchored to the page beside the paragraph that \
                     follows them, so they do not move with the text",
                    self.placed
                ),
            });
        }
        let (shapes, ink) = dropped(self.doc, &self.consumed);
        if shapes > 0 {
            out.push(Notice {
                code: CODE_SHAPE_DROPPED.into(),
                message: format!(
                    "{shapes} drawn line(s) or box(es) that no table or decoration explains \
                     are left out of the Word file"
                ),
            });
        }
        if ink > 0 {
            out.push(Notice {
                code: CODE_INK_DROPPED.into(),
                message: format!("{ink} piece(s) of text or image are left out of the Word file"),
            });
        }
        out
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
