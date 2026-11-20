use std::collections::HashMap;

use crate::layout::{ItemKind, LayoutDoc, NodeId, NodeKind, ParStyle};
use crate::model::{
    Block, Drawing, Inline, Line, ParProps, Paragraph, Run, RunContent, RunProps, Spacing, Table,
    Toc, Twips,
};

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

type StrideKey = (usize, i64, i64);

/// What makes paragraphs alike in the space Typst sets above them: the list
/// level they are numbered at, or else their heading level, their style's
/// size, leading and spacing, and their block's space above and below, in
/// hundredths of a point.
pub(super) type SpaceKey = (Option<(u32, u8)>, Option<u8>, [i64; 5]);

/// The face, size and leading a paragraph's first text is set in.
fn stride_key(doc: &LayoutDoc, n: NodeId) -> Option<StrideKey> {
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
fn line_strides(lines: &[crate::layout::Line], size: f64) -> impl Iterator<Item = f64> + '_ {
    lines
        .windows(2)
        .filter(|w| w[0].page == w[1].page)
        .map(|w| w[1].baseline - w[0].baseline)
        .filter(move |s| *s >= 0.5 * size)
}

/// The usual baseline stride of paragraphs of one face, size and leading,
/// which sets the pitch of a paragraph that has no stride of its own.
pub(super) struct Strides<'a> {
    doc: &'a LayoutDoc,
    usual: HashMap<StrideKey, f64>,
}

impl<'a> Strides<'a> {
    /// The median stride per face, size and leading, over the paragraphs
    /// with two or more lines on one page.
    pub(super) fn new(doc: &'a LayoutDoc) -> Self {
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
        let usual = all
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, mut v)| {
                v.sort_by(f64::total_cmp);
                (k, v[v.len() / 2])
            })
            .collect();
        Strides { doc, usual }
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
            .and_then(|k| self.usual.get(&k).copied())
            .or_else(|| {
                style
                    .filter(|s| s.top_edge + s.bottom_edge > 0.0)
                    .map(|s| s.leading + s.top_edge + s.bottom_edge)
            })
            .unwrap_or(1.2 * size)
    }
}

/// What lowering yields, in reading order: the blocks of a scope and what
/// they ask of their neighbours, which [`Flow`] settles in one pass.
#[derive(Clone)]
pub(super) enum Entry {
    /// A paragraph and where its lines sit.
    Par(Paragraph, Placed),
    /// A table and where its rows sit.
    Table(Table, Rows),
    /// A table of contents whose entries are the paragraphs these set.
    Toc(Toc, Vec<Entry>),
    /// No page break between the block before and the one after.
    Keep,
    /// A page break before the next paragraph or table on a later page.
    Break,
    /// An object anchored to a page, waiting for a paragraph there.
    Drawing(u32, Drawing),
    /// A child of the flow begins, on this page when it has one.
    Child(Option<u32>),
    /// A node's children begin.
    Open,
    /// They end, with the keeps the node puts on their paragraphs.
    Close(Hold),
    /// The entries after it belong to this section of the body.
    Section(usize),
}

/// Where a paragraph's lines sit.
#[derive(Clone)]
pub(super) struct Placed {
    pub(super) page: u32,
    pub(super) baseline: f64,
    pub(super) pitch: f64,
    pub(super) key: Option<SpaceKey>,
    /// Its style's space above, for a paragraph with no measure of its own.
    pub(super) spacing: f64,
    /// A picture, whose line Word sizes to it.
    pub(super) object: bool,
    pub(super) end: Prev,
    /// Where its last line box ends, which the gap above a table after it
    /// is measured from.
    pub(super) bottom: Option<(u32, f64)>,
}

/// Where a table's rows sit.
#[derive(Clone)]
pub(super) struct Rows {
    /// The page of its first row and where the first line box in it starts.
    pub(super) top: Option<(u32, f64)>,
    /// Its first cell opens with a paragraph, which takes a page break.
    pub(super) opens: bool,
    /// The page of its last row and where Word ends it.
    pub(super) end: Option<(u32, f64)>,
}

/// The keeps a node puts on the paragraphs of its children.
#[derive(Clone, Copy)]
pub(super) enum Hold {
    None,
    /// A block that does not break keeps each paragraph's lines together
    /// and with the next; a sticky one keeps its last with what follows.
    Block {
        sticky: bool,
        breakable: bool,
    },
    /// A figure keeps all but its last with the next.
    Figure,
}

impl Hold {
    fn apply(self, blocks: &mut [Block]) {
        let count = blocks.len();
        for (i, b) in blocks.iter_mut().enumerate() {
            let Block::Paragraph(p) = b else {
                continue;
            };
            let last = i + 1 == count;
            match self {
                Hold::None => {}
                Hold::Block { sticky, breakable } => {
                    if !breakable {
                        p.props.keep_lines = true;
                        p.props.keep_next |= !last;
                    }
                    p.props.keep_next |= sticky && last;
                }
                Hold::Figure => p.props.keep_next |= !last,
            }
        }
    }
}

/// Paragraph properties for lines at `pitch`: an exact line height, none for
/// a picture, and the space above, which [`Flow`] sets.
pub(super) fn line_props(pitch: f64, object: bool) -> ParProps {
    ParProps {
        spacing: Some(Spacing {
            before: None,
            after: None,
            line: (!object).then_some(Line::Exact(tw(pitch))),
        }),
        ..ParProps::default()
    }
}

/// The section of `groups` that holds `page`.
pub(super) fn section_of(groups: &[Vec<usize>], page: u32) -> usize {
    groups
        .iter()
        .position(|g| g.contains(&(page as usize)))
        .unwrap_or(0)
}

/// The document body's pages and the space above a paragraph Typst put at a
/// page top, by what paragraphs are alike in.
#[derive(Clone, Copy)]
struct Body<'a> {
    doc: &'a LayoutDoc,
    groups: &'a [Vec<usize>],
    usual: &'a HashMap<SpaceKey, f64>,
}

/// One pass over a scope's entries that decides what depends on a
/// neighbour: the space above each paragraph, the gap above a table, keeps,
/// page breaks, and the paragraph each drawing is anchored in. A cell, text
/// box or footnote has no body: space above only from what stands above on
/// its page, no page break, and its drawings left to the scope around it.
struct Flow<'a> {
    body: Option<Body<'a>>,
    bodies: Vec<Vec<Block>>,
    section: usize,
    /// Where the blocks of each open node begin in the section's body.
    scopes: Vec<usize>,
    prev: Option<Prev>,
    /// Where the last paragraph or table ends, for the gap above a table.
    bottom: Option<(u32, f64)>,
    /// A page break waits for the next paragraph or table.
    broken: bool,
    pending: Vec<(u32, Drawing)>,
    /// The space above each paragraph that follows a paragraph on its page.
    measured: HashMap<SpaceKey, Vec<f64>>,
}

/// The blocks of each section's body. The entries are spaced twice: a
/// paragraph Typst put at a page top takes the median space above the
/// paragraphs alike in it that the first pass measured.
pub(super) fn body(doc: &LayoutDoc, groups: &[Vec<usize>], entries: Vec<Entry>) -> Vec<Vec<Block>> {
    let none = HashMap::new();
    let usual: HashMap<SpaceKey, f64> = Flow::new(
        Some(Body {
            doc,
            groups,
            usual: &none,
        }),
        groups.len(),
    )
    .run(entries.clone())
    .measured
    .into_iter()
    .map(|(k, mut v)| {
        v.sort_by(f64::total_cmp);
        (k, v[v.len() / 2])
    })
    .collect();
    let body = Body {
        doc,
        groups,
        usual: &usual,
    };
    Flow::new(Some(body), groups.len()).run(entries).bodies
}

/// The blocks of a cell, text box or footnote, and the drawings no
/// paragraph in it takes, with their pages. A paragraph takes the space
/// above it from the one before it on its page, the first from where its
/// scope's text starts when `top` gives that.
pub(super) fn nested(
    entries: Vec<Entry>,
    top: Option<(u32, f64)>,
) -> (Vec<Block>, Vec<(u32, Drawing)>) {
    let mut flow = Flow::new(None, 1);
    flow.prev = top.map(|(page, y)| Prev::Bottom(page, y));
    let mut flow = flow.run(entries);
    (flow.bodies.pop().unwrap_or_default(), flow.pending)
}

fn drawing_run(drawings: Vec<(u32, Drawing)>) -> Inline {
    Inline::Run(Run {
        props: RunProps::default(),
        content: drawings
            .into_iter()
            .map(|(_, d)| RunContent::Drawing(d))
            .collect(),
    })
}

impl<'a> Flow<'a> {
    fn new(body: Option<Body<'a>>, sections: usize) -> Self {
        Flow {
            body,
            bodies: vec![Vec::new(); sections.max(1)],
            section: 0,
            scopes: Vec::new(),
            prev: None,
            bottom: None,
            broken: false,
            pending: Vec::new(),
            measured: HashMap::new(),
        }
    }

    fn run(mut self, entries: Vec<Entry>) -> Self {
        for e in entries {
            self.entry(e);
        }
        if self.body.is_some() {
            self.flush(None);
        }
        self
    }

    fn out(&mut self) -> &mut Vec<Block> {
        &mut self.bodies[self.section]
    }

    fn entry(&mut self, e: Entry) {
        match e {
            Entry::Par(p, at) => self.paragraph(p, at),
            Entry::Table(t, rows) => self.table(t, rows),
            Entry::Toc(mut toc, entries) => {
                let start = self.out().len();
                for e in entries {
                    self.entry(e);
                }
                toc.entries = self
                    .out()
                    .drain(start..)
                    .filter_map(|b| match b {
                        Block::Paragraph(p) => Some(p),
                        _ => None,
                    })
                    .collect();
                if !toc.entries.is_empty() {
                    self.out().push(Block::Toc(toc));
                }
            }
            Entry::Keep => match self.out().last_mut() {
                Some(Block::Paragraph(p)) => p.props.keep_next = true,
                Some(Block::Table(t)) => {
                    for row in &mut t.rows {
                        row.props.cant_split = true;
                    }
                }
                _ => {}
            },
            Entry::Break => self.broken |= self.body.is_some(),
            Entry::Drawing(page, d) => self.pending.push((page, d)),
            Entry::Child(page) => {
                if self.body.is_some() {
                    self.flush(page);
                }
            }
            Entry::Open => {
                let start = self.out().len();
                self.scopes.push(start);
            }
            Entry::Close(hold) => {
                let start = self.scopes.pop().unwrap_or(0);
                hold.apply(&mut self.out()[start..]);
            }
            Entry::Section(s) => self.section = s,
        }
    }

    /// A spacer the block after it needs, which belongs to the node around
    /// the open nodes that hold nothing yet.
    fn separate(&mut self, height: Twips) {
        let len = self.out().len();
        for s in self.scopes.iter_mut().rev().take_while(|s| **s == len) {
            *s += 1;
        }
        self.out().push(Block::Paragraph(spacer(height)));
    }

    /// The page break and the waiting drawings a paragraph on `page` opens
    /// with.
    fn open(&mut self, p: &mut Paragraph, page: u32) {
        if self.broken && self.prev.is_some_and(|q| q.page() < page) {
            p.props.page_break_before = true;
        }
        self.broken = false;
        if !self.pending.is_empty() {
            p.content
                .insert(0, drawing_run(std::mem::take(&mut self.pending)));
        }
    }

    fn before(&mut self, at: &Placed) -> f64 {
        match self.prev {
            Some(Prev::Baseline(p, y)) if p == at.page => {
                let gap = at.baseline - y - at.pitch;
                if let Some(key) = at.key {
                    self.measured.entry(key).or_default().push(gap);
                }
                gap
            }
            Some(Prev::Bottom(p, y)) if p == at.page => at.baseline - y - 0.8 * at.pitch,
            _ => match self.body {
                Some(body) => self.across(body, at),
                None => 0.0,
            },
        }
    }

    /// The space above a paragraph with nothing above it on its page.
    fn across(&self, body: Body, at: &Placed) -> f64 {
        match self.prev {
            Some(_) if !self.broken => at
                .key
                .and_then(|k| body.usual.get(&k).copied())
                .unwrap_or(at.spacing),
            _ => {
                let top = body.doc.pages[at.page as usize].setup.margins.top;
                at.baseline - top - 0.8 * at.pitch
            }
        }
    }

    fn paragraph(&mut self, mut p: Paragraph, at: Placed) {
        let before = self.before(&at);
        let before = if at.object {
            before.max(0.0) - 0.2 * at.pitch
        } else {
            before
        };
        p.props.spacing.get_or_insert_with(Spacing::default).before = Some(tw(before.max(0.0)));
        if self.body.is_some() {
            self.open(&mut p, at.page);
            self.bottom = at.bottom.or(self.bottom);
        }
        self.prev = Some(at.end);
        self.out().push(Block::Paragraph(p));
    }

    /// A table: the paragraph before it takes the gap above it as space
    /// after, or a spacer of that height stands for it after a table or
    /// contents, and a spacer separates two tables Word would join.
    fn table(&mut self, mut t: Table, rows: Rows) {
        if self.body.is_some() {
            let gap = match (rows.top, self.bottom) {
                (Some((page, first)), Some((p, bottom))) if p == page => {
                    Some(tw((first - bottom).max(0.0)))
                }
                _ => None,
            };
            let forced = self.broken
                && match (self.prev, rows.top) {
                    (Some(p), Some((q, _))) => p.page() < q,
                    _ => false,
                };
            self.broken = false;
            if forced && rows.opens {
                if let Some(cell) = t.rows.first_mut().and_then(|r| r.cells.first_mut()) {
                    let body = &mut cell.body;
                    match body.blocks.first_mut() {
                        Some(Block::Paragraph(p)) => p.props.page_break_before = true,
                        _ => body.last.props.page_break_before = true,
                    }
                }
            }
            if let Some(gap) = gap {
                match self.out().last_mut() {
                    Some(Block::Paragraph(p)) => {
                        p.props.spacing.get_or_insert_with(Spacing::default).after = Some(gap);
                    }
                    Some(_) => self.separate(gap),
                    None => {}
                }
            }
        }
        if matches!(self.out().last(), Some(Block::Table(_))) {
            self.separate(0);
        }
        self.out().push(Block::Table(t));
        if let Some((page, bottom)) = rows.end {
            self.prev = Some(Prev::Bottom(page, bottom));
            if self.body.is_some() {
                self.bottom = rows.end;
            }
        }
    }

    /// The drawings waiting for a paragraph on an earlier page than
    /// `before` (or any, with none), set on that page: on the last paragraph
    /// of the node being lowered, or of the section at the top level, when
    /// it ends there, else on a 1 pt paragraph of their own, which the next
    /// page breaks after when it stands for a page with no flow.
    fn flush(&mut self, before: Option<u32>) {
        let Some(&(page, _)) = self.pending.first() else {
            return;
        };
        if before.is_some_and(|q| q <= page) {
            return;
        }
        let (section, start) = match (self.scopes.last(), self.body) {
            (Some(&start), _) => (self.section, start),
            (None, Some(body)) => (section_of(body.groups, page), 0),
            (None, None) => (self.section, 0),
        };
        let prev = self.prev.map(Prev::page);
        let out = &mut self.bodies[section];
        if prev == Some(page) && out.len() > start {
            if let Some(Block::Paragraph(p)) = out.last_mut() {
                p.content
                    .push(drawing_run(std::mem::take(&mut self.pending)));
                return;
            }
        }
        let mut anchor = spacer(20);
        self.broken |= prev.is_some_and(|p| p < page);
        self.open(&mut anchor, page);
        self.prev = Some(Prev::Bottom(page, 0.0));
        self.broken = prev != Some(page);
        self.bodies[section].push(Block::Paragraph(anchor));
    }
}

impl<'a> Lower<'a> {
    /// Where lines from `first` to `last` of paragraph `n` sit, set at
    /// `pitch` in `style`.
    pub(super) fn placed(
        &self,
        n: NodeId,
        ctx: &Ctx,
        style: Option<ParStyle>,
        first: &crate::layout::Line,
        last: &crate::layout::Line,
        pitch: f64,
    ) -> Placed {
        Placed {
            page: first.page,
            baseline: first.baseline,
            pitch,
            key: Some(self.space_key(n, ctx)),
            spacing: style.map_or(0.0, |s| s.spacing),
            object: false,
            end: Prev::Baseline(last.page, last.baseline),
            bottom: Some((last.page, last.baseline + 0.2 * pitch)),
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
}
