use std::collections::HashMap;

use crate::layout::{ItemKind, LayoutDoc, NodeId, NodeKind, ParStyle};
use crate::model::{Block, Inline, Line, ParProps, Paragraph, Run, RunContent, RunProps, Spacing};

use super::paragraph::style_of;
use super::{Ctx, Lower, spacer, tw};

/// Clears the page break before the first paragraph `blocks` set, in a
/// table's first cell when they open with a table, and whether they open
/// with anything.
pub(super) fn drop_opening_break(blocks: &mut [Block]) -> bool {
    match blocks.first_mut() {
        Some(Block::Paragraph(p)) => p.props.page_break_before = false,
        Some(Block::Table(t)) => {
            if let Some(cell) = t.rows.first_mut().and_then(|r| r.cells.first_mut())
                && !drop_opening_break(&mut cell.body.blocks)
            {
                cell.body.last.props.page_break_before = false;
            }
        }
        Some(Block::Toc(_)) => {}
        None => return false,
    }
    true
}

#[derive(Clone, Copy)]
pub(super) enum Prev {
    Baseline(u32, f64),
    Bottom(u32, f64),
}

impl Prev {
    pub(super) fn page(self) -> u32 {
        match self {
            Prev::Baseline(p, _) | Prev::Bottom(p, _) => p,
        }
    }
}

pub(super) type StrideKey = (usize, i64, i64);

/// What makes paragraphs alike in the space Typst sets above them: the list
/// level they are numbered at, or else their heading level, their style's
/// size, leading and spacing, and their block's space above and below, in
/// hundredths of a point.
pub(super) type SpaceKey = (Option<(u32, u8)>, Option<u8>, [i64; 5]);

/// The face, size and leading a paragraph's first text is set in.
pub(super) fn stride_key(doc: &LayoutDoc, n: NodeId) -> Option<StrideKey> {
    let t = doc.nodes[n]
        .lines
        .iter()
        .flat_map(|l| &l.items)
        .find_map(|r| match &doc.item(*r).kind {
            ItemKind::Text(t) => Some(t),
            _ => None,
        })?;
    let leading = style_of(doc, n).map_or(0.0, |s| s.leading);
    Some((
        t.font,
        (t.size * 100.0).round() as i64,
        (leading * 100.0).round() as i64,
    ))
}

/// The baseline strides between consecutive `lines` on one page that can be
/// a line pitch for text `size` tall: a stride under half the size is one
/// line read as two.
pub(super) fn line_strides(
    lines: &[crate::layout::Line],
    size: f64,
) -> impl Iterator<Item = f64> + '_ {
    lines
        .windows(2)
        .filter(|w| w[0].page == w[1].page)
        .map(|w| w[1].baseline - w[0].baseline)
        .filter(move |s| *s >= 0.5 * size)
}

/// The median baseline stride per face, size and leading, over the
/// paragraphs with two or more lines on one page.
pub(super) fn strides(doc: &LayoutDoc) -> HashMap<StrideKey, f64> {
    let mut all: HashMap<StrideKey, Vec<f64>> = HashMap::new();
    for n in 0..doc.nodes.len() {
        if !doc.nodes[n].kind.forms_paragraph() {
            continue;
        }
        let Some(key) = stride_key(doc, n) else {
            continue;
        };
        let size = key.1 as f64 / 100.0;
        all.entry(key)
            .or_default()
            .extend(line_strides(&doc.nodes[n].lines, size));
    }
    all.into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, mut v)| {
            v.sort_by(f64::total_cmp);
            (k, v[v.len() / 2])
        })
        .collect()
}

impl<'a> Lower<'a> {
    /// What the child just lowered asks of the paragraph before it: the
    /// space a table needs above it, as that paragraph's space after, or as
    /// a spacer paragraph of that height after a table or contents. When
    /// nothing comes before it at this level, the request is `late` and
    /// belongs to the level above, and the children after it settle their
    /// own.
    pub(super) fn settle(&mut self, before: &mut Vec<Block>, late: &mut bool) {
        if *late || (self.table_gap.is_none() && !self.keep) {
            return;
        }
        if self.keep {
            match before.last_mut() {
                Some(Block::Paragraph(p)) => {
                    p.props.keep_next = true;
                    self.keep = false;
                }
                Some(Block::Table(t)) => {
                    for row in &mut t.rows {
                        row.props.cant_split = true;
                    }
                    self.keep = false;
                }
                Some(_) => self.keep = false,
                None => {
                    *late = true;
                    return;
                }
            }
        }
        if self.table_gap.is_none() {
            return;
        }
        match before.last_mut() {
            Some(Block::Paragraph(p)) => {
                let gap = self.table_gap.take();
                p.props.spacing.get_or_insert_with(Spacing::default).after = gap;
            }
            Some(_) => {
                let gap = self.table_gap.take().unwrap_or(0);
                before.push(Block::Paragraph(spacer(gap)));
            }
            None => *late = true,
        }
    }

    /// `blocks` after `out`, with a spacer between two tables that nothing
    /// else separates, since Word joins adjacent tables into one.
    pub(super) fn append(out: &mut Vec<Block>, blocks: Vec<Block>) {
        if matches!(out.last(), Some(Block::Table(_)))
            && matches!(blocks.first(), Some(Block::Table(_)))
        {
            out.push(Block::Paragraph(spacer(0)));
        }
        out.extend(blocks);
    }

    pub(super) fn apply_break(&mut self, p: &mut Paragraph, page: u32, ctx: &Ctx) {
        if ctx.nested {
            return;
        }
        if self.break_pending && self.prev.is_some_and(|q| q.page() < page) {
            p.props.page_break_before = true;
        }
        self.break_pending = false;
        if !self.pending.is_empty() {
            let drawings = std::mem::take(&mut self.pending);
            p.content.insert(
                0,
                Inline::Run(Run {
                    props: RunProps::default(),
                    content: drawings
                        .into_iter()
                        .map(|(_, d)| RunContent::Drawing(d))
                        .collect(),
                }),
            );
        }
    }

    /// The objects waiting for a paragraph on an earlier page than `before`
    /// (or any, with none), set on that page: on its last paragraph when
    /// `out` ends with one there, else on a 1 pt paragraph of their own,
    /// which the next page breaks after when it stands for a page with no
    /// flow.
    pub(super) fn flush_pending(&mut self, before: Option<u32>, out: &mut Vec<Block>, ctx: &Ctx) {
        let Some(&(page, _)) = self.pending.first() else {
            return;
        };
        if ctx.nested || before.is_some_and(|q| q <= page) {
            return;
        }
        let prev = self.prev.map(Prev::page);
        if prev == Some(page) && matches!(out.last(), Some(Block::Paragraph(_))) {
            self.attach_pending(out);
            return;
        }
        let mut anchor = spacer(20);
        self.break_pending |= prev.is_some_and(|p| p < page);
        self.apply_break(&mut anchor, page, ctx);
        self.prev = Some(Prev::Bottom(page, 0.0));
        self.break_pending = prev != Some(page);
        out.push(Block::Paragraph(anchor));
    }

    pub(super) fn attach_pending(&mut self, blocks: &mut [Block]) {
        if let Some(Block::Paragraph(p)) = blocks.last_mut() {
            let drawings = std::mem::take(&mut self.pending);
            p.content.push(Inline::Run(Run {
                props: RunProps::default(),
                content: drawings
                    .into_iter()
                    .map(|(_, d)| RunContent::Drawing(d))
                    .collect(),
            }));
        }
    }

    /// Paragraph properties from where a block of `height` whose first
    /// baseline is `baseline` sits.
    pub(super) fn par_frame(
        &mut self,
        ctx: &Ctx,
        n: Option<NodeId>,
        style: Option<ParStyle>,
        page: u32,
        baseline: f64,
        pitch: f64,
        object: bool,
    ) -> ParProps {
        let key = n.map(|n| self.space_key(n, ctx));
        let before = if ctx.nested {
            0.0
        } else {
            match self.prev {
                Some(Prev::Baseline(p, y)) if p == page => {
                    let gap = baseline - y - pitch;
                    if let Some(key) = key {
                        self.measured.entry(key).or_default().push(gap);
                    }
                    gap
                }
                Some(Prev::Bottom(p, y)) if p == page => baseline - y - 0.8 * pitch,
                Some(_) if !self.break_pending => key
                    .and_then(|k| self.usual.get(&k).copied())
                    .unwrap_or_else(|| style.map_or(0.0, |s| s.spacing)),
                _ => {
                    let top = self.doc.pages[page as usize].setup.margins.top;
                    baseline - top - 0.8 * pitch
                }
            }
        };
        let before = if object {
            before.max(0.0) - 0.2 * pitch
        } else {
            before
        };
        ParProps {
            spacing: Some(Spacing {
                before: Some(tw(before.max(0.0))),
                after: None,
                line: (!object).then_some(Line::Exact(tw(pitch))),
            }),
            ..ParProps::default()
        }
    }

    pub(super) fn space_key(&self, n: NodeId, ctx: &Ctx) -> SpaceKey {
        if ctx.list.is_some() {
            return (ctx.list, None, [0; 5]);
        }
        let heading = match self.kind(n) {
            NodeKind::Heading { level, .. } => Some(*level),
            _ => None,
        };
        let c = |v: f64| (v * 100.0).round() as i64;
        let style = self.par_style(n);
        let block = self
            .doc
            .ancestor(n, |k| matches!(k, NodeKind::Block { .. }))
            .and_then(|b| match self.doc.nodes[b].kind {
                NodeKind::Block { above, below, .. } => Some((above, below)),
                _ => None,
            });
        (
            None,
            heading,
            [
                style.map_or(0, |s| c(s.size)),
                style.map_or(0, |s| c(s.leading)),
                style.map_or(0, |s| c(s.spacing)),
                block.map_or(-1, |b| c(b.0)),
                block.map_or(-1, |b| c(b.1)),
            ],
        )
    }

    /// A paragraph's exact line height: its own median stride, else the
    /// stride of paragraphs of its face, size and leading, else its style's
    /// leading plus its line box.
    pub(super) fn pitch(
        &self,
        n: NodeId,
        lines: &[crate::layout::Line],
        style: Option<ParStyle>,
    ) -> f64 {
        let size = lines
            .iter()
            .flat_map(|l| &l.items)
            .find_map(|r| match &self.doc.item(*r).kind {
                ItemKind::Text(t) => Some(t.size),
                _ => None,
            })
            .unwrap_or(11.0);
        let mut strides: Vec<f64> = line_strides(lines, size).collect();
        if !strides.is_empty() {
            strides.sort_by(f64::total_cmp);
            return strides[strides.len() / 2];
        }
        stride_key(self.doc, n)
            .and_then(|k| self.strides.get(&k).copied())
            .or_else(|| {
                style
                    .filter(|s| s.top_edge + s.bottom_edge > 0.0)
                    .map(|s| s.leading + s.top_edge + s.bottom_edge)
            })
            .unwrap_or(1.2 * size)
    }
}
