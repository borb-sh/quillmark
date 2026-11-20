use std::collections::{BTreeMap, HashSet};

use crate::layout::{BracketHint, Child, HAlign, ItemKind, LayoutDoc, NodeId, NodeKind, Rgb};
use crate::model::{
    Block, Blocks, Border, Borders, BreakKind, Cell, CellMargins, CellProps, FirstLine, Indent,
    Inline, Paragraph, Row, RowProps, Run, RunContent, RunProps, TabKind, TabLeader, TabStop,
    Table, TableProps, Twips, VMerge,
};

use super::paragraph::style_of;
use super::space::{self, Entry, Rows, line_props};
use super::styles::style_id;
use super::{Ctx, Lower, hex, spacer, tw};

/// The height of the paragraph that closes a table cell ending in a table,
/// which Word requires and draws a line tall unless told otherwise.
pub(super) const CLOSING: Twips = 20;

/// The `layout(form: "tabbed")` hint that holds the grid `n`, through probe
/// brackets only.
pub(super) fn tabbed_hint(doc: &LayoutDoc, n: NodeId) -> Option<NodeId> {
    let mut at = doc.nodes[n].parent;
    while let Some(m) = at {
        match doc.nodes[m].kind {
            NodeKind::Hint(BracketHint::Tabbed) => return Some(m),
            NodeKind::Block { .. } | NodeKind::Align { .. } => at = doc.nodes[m].parent,
            _ => return None,
        }
    }
    None
}

impl<'a> Lower<'a> {
    /// How many twips wider than Typst's kerned ink each content-sized
    /// column of table `n` must be for its widest line in the face's own
    /// advances, as Word sets it, with a twip for each character after the
    /// first, which covers a layout that rounds each advance up to a whole
    /// twip. Room the column already has past a line, such as a gutter it
    /// took in, counts against it.
    pub(super) fn slack(&self, n: NodeId) -> Vec<Twips> {
        let NodeKind::Table {
            columns,
            content_sized,
            ..
        } = self.kind(n)
        else {
            return Vec::new();
        };
        let mut out = vec![0.0f64; columns.len().saturating_sub(1)];
        for m in self.doc.cells_of(n) {
            let NodeKind::Cell {
                col,
                colspan,
                inset,
                ..
            } = *self.kind(m)
            else {
                continue;
            };
            let col = col as usize;
            if colspan != 1 || col >= out.len() || !content_sized.get(col).copied().unwrap_or(false)
            {
                continue;
            }
            for p in self.doc.subtree(m) {
                for line in &self.doc.nodes[p].lines {
                    let (extra, glyphs) = line
                        .items
                        .iter()
                        .filter_map(|r| match &self.doc.item(*r).kind {
                            ItemKind::Text(t) => {
                                Some((t.natural - t.width, t.text.trim_end().chars().count()))
                            }
                            _ => None,
                        })
                        .fold((0.0, 0), |(e, g), (de, dg)| (e + de, g + dg));
                    let spare = (columns[col + 1] - inset.right - line.x1).max(0.0);
                    let need = extra + glyphs.saturating_sub(1) as f64 / 20.0 - spare;
                    out[col] = out[col].max(need);
                }
            }
        }
        out.into_iter()
            .map(|s| (s * 20.0).ceil() as Twips)
            .collect()
    }

    /// The paragraph-forming nodes of `cell`, in reading order.
    pub(super) fn cell_paragraphs(&self, cell: NodeId) -> Vec<NodeId> {
        self.doc
            .subtree(cell)
            .into_iter()
            .filter(|&m| self.kind(m).forms_paragraph() && !self.doc.nodes[m].lines.is_empty())
            .collect()
    }

    /// A tabbed grid as one paragraph per row: the cells' text joined by a
    /// tab each, a tab stop at each column, and the last column's text
    /// hanging at its x; `None` when a cell before the last wraps or the
    /// last holds a table.
    pub(super) fn tabbed(&mut self, n: NodeId, ctx: &Ctx) -> Option<Vec<Entry>> {
        let NodeKind::Table { columns, .. } = self.kind(n).clone() else {
            return None;
        };
        let mut rows: BTreeMap<u32, Vec<(u32, NodeId)>> = BTreeMap::new();
        for m in self.doc.cells_of(n) {
            if let NodeKind::Cell { col, row, .. } = *self.kind(m) {
                rows.entry(row).or_default().push((col, m));
            }
        }
        for cells in rows.values_mut() {
            cells.sort();
            let (_, last) = *cells.last()?;
            for &(_, c) in &cells[..cells.len() - 1] {
                let lines: usize = self
                    .cell_paragraphs(c)
                    .iter()
                    .map(|&p| self.doc.nodes[p].lines.len())
                    .sum();
                if lines > 1 {
                    return None;
                }
            }
            if self
                .doc
                .subtree(last)
                .iter()
                .any(|&m| m != last && matches!(self.kind(m), NodeKind::Table { .. }))
            {
                return None;
            }
        }
        for m in self.doc.subtree(n) {
            for c in self.doc.nodes[m].children.clone() {
                if let Child::Item(r) = c {
                    if matches!(self.doc.item(r).kind, ItemKind::Shape(_)) {
                        self.consumed.insert(r);
                    }
                }
            }
        }
        let role = self.role_of(n);
        let slack = self.slack(n);
        let mut out = Vec::new();
        for cells in rows.values() {
            let (_, last) = *cells.last().expect("a cell");
            let pars: Vec<NodeId> = cells
                .iter()
                .flat_map(|&(_, c)| self.cell_paragraphs(c))
                .collect();
            let Some(&lead) = self.cell_paragraphs(last).first().or(pars.first()) else {
                continue;
            };
            let first = pars
                .iter()
                .filter_map(|&p| self.doc.nodes[p].lines.first())
                .min_by(|a, b| {
                    (a.page, a.baseline)
                        .partial_cmp(&(b.page, b.baseline))
                        .unwrap()
                })?
                .clone();
            let style = style_of(self.doc, lead);
            let lead_lines = self.doc.nodes[lead].lines.clone();
            let pitch = self.strides.pitch(lead, &lead_lines, style);
            let mut props = line_props(pitch, false);
            let x = |l: &Self, c: NodeId, col: u32| {
                let inset = match l.kind(c) {
                    NodeKind::Cell { inset, .. } => inset.left,
                    _ => 0.0,
                };
                let wider: Twips = slack.iter().take(col as usize).sum();
                columns.get(col as usize).copied().unwrap_or(ctx.left) + inset - ctx.left
                    + wider as f64 / 20.0
            };
            let end = style.is_some_and(|s| s.align == HAlign::End)
                || self
                    .doc
                    .ancestor(lead, |k| matches!(k, NodeKind::Align { h: HAlign::End }))
                    .is_some_and(|a| {
                        self.doc.ancestor(a, |k| matches!(k, NodeKind::Cell { .. })) == Some(last)
                    });
            let mut content = Vec::new();
            let mut tabs = Vec::new();
            let mut stop: Twips = 0;
            for (i, &(col, c)) in cells.iter().enumerate() {
                if i > 0 {
                    let pos = (((x(self, c, col) * 20.0).ceil()) as Twips).max(stop + 1);
                    stop = pos;
                    let last_cell = i + 1 == cells.len();
                    tabs.push(if last_cell && end {
                        TabStop {
                            kind: TabKind::Right,
                            pos: tw(columns.last().copied().unwrap_or(0.0) - ctx.left),
                            leader: TabLeader::None,
                        }
                    } else {
                        TabStop {
                            kind: TabKind::Left,
                            pos,
                            leader: TabLeader::None,
                        }
                    });
                    content.push(Inline::Run(Run {
                        props: RunProps::default(),
                        content: vec![RunContent::Tab],
                    }));
                }
                for (j, p) in self.cell_paragraphs(c).into_iter().enumerate() {
                    if j > 0 {
                        content.push(Inline::Run(Run {
                            props: RunProps::default(),
                            content: vec![RunContent::Break(BreakKind::Line)],
                        }));
                    }
                    let lines = self.doc.nodes[p].lines.clone();
                    content.extend(self.runs(p, &lines, ctx));
                }
            }
            if !end && cells.len() > 1 {
                props.indent = Some(Indent {
                    left: Some(stop),
                    right: None,
                    first: Some(FirstLine::Hanging(stop)),
                });
            }
            props.tabs = tabs;
            if let Some((name, next)) = role.clone() {
                let id = style_id(&name);
                self.roles.entry(id.clone()).or_insert((name, next));
                props.style = Some(id);
            }
            let last_line = self
                .cell_paragraphs(last)
                .last()
                .and_then(|&p| self.doc.nodes[p].lines.last().cloned())
                .unwrap_or_else(|| first.clone());
            let at = self.placed(lead, ctx, style, &first, &last_line, pitch);
            out.push(Entry::Par(Paragraph { props, content }, at));
        }
        Some(out)
    }

    /// The paragraph whose first line is the highest of `row`'s cells on
    /// `page`, with its cell's top inset: where Word's row starts from.
    pub(super) fn first_line_in_row(
        &self,
        cells: &[NodeId],
        row: u32,
        page: u32,
    ) -> Option<(NodeId, f64)> {
        cells
            .iter()
            .filter_map(|&c| match self.kind(c) {
                NodeKind::Cell { row: r, inset, .. } if *r == row => Some((c, inset.top)),
                _ => None,
            })
            .flat_map(|(c, inset)| {
                self.doc
                    .subtree(c)
                    .into_iter()
                    .filter(|&m| {
                        self.doc.nodes[m]
                            .lines
                            .first()
                            .is_some_and(|l| l.page == page)
                    })
                    .map(move |m| (m, inset))
            })
            .min_by(|a, b| {
                let y = |m: NodeId| self.doc.nodes[m].lines[0].baseline;
                y(a.0).total_cmp(&y(b.0))
            })
    }

    /// Where the first line box of `row` starts on `page`, inside its cell's
    /// inset, with that inset.
    fn row_top(&self, cells: &[NodeId], row: u32, page: u32) -> Option<(f64, f64)> {
        let (m, inset) = self.first_line_in_row(cells, row, page)?;
        let lines = &self.doc.nodes[m].lines;
        let pitch = self.strides.pitch(m, lines, self.par_style(m));
        Some((lines[0].baseline - 0.8 * pitch, inset))
    }

    /// Where Word ends a table's last row on `page`: below the last line its
    /// cells hold there at that line's pitch, plus the cell's bottom inset and
    /// the closing paragraph of a cell that ends in a table.
    pub(super) fn table_bottom(
        &self,
        cells: &[NodeId],
        nrows: u32,
        page: u32,
        closed: &HashSet<NodeId>,
    ) -> Option<f64> {
        cells
            .iter()
            .filter_map(|&c| match self.kind(c) {
                NodeKind::Cell {
                    row,
                    rowspan,
                    inset,
                    ..
                } if row + (*rowspan).max(1) >= nrows => {
                    let text = self
                        .doc
                        .subtree(c)
                        .into_iter()
                        .filter_map(|m| {
                            let lines = &self.doc.nodes[m].lines;
                            let last = lines.last().filter(|l| l.page == page)?;
                            let pitch = self.strides.pitch(m, lines, self.par_style(m));
                            Some(last.baseline + 0.2 * pitch)
                        })
                        .reduce(f64::max)?;
                    let closing = if closed.contains(&c) {
                        CLOSING as f64 / 20.0
                    } else {
                        0.0
                    };
                    Some(text + inset.bottom + closing)
                }
                _ => None,
            })
            .reduce(f64::max)
    }

    /// Table `n`, then the drawings in its cells that no paragraph there
    /// takes.
    pub(super) fn table(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Entry> {
        let NodeKind::Table {
            columns,
            content_sized,
            rows: tops,
            header_rows,
            ..
        } = self.kind(n).clone()
        else {
            unreachable!("a table node")
        };
        let cells = self.doc.cells_of(n);
        for m in self.doc.subtree(n) {
            let owner = self.doc.ancestor(m, |k| {
                matches!(k, NodeKind::Table { .. } | NodeKind::Cell { .. })
            });
            if owner != Some(n) {
                continue;
            }
            for c in self.doc.nodes[m].children.clone() {
                if let Child::Item(r) = c {
                    if matches!(self.doc.item(r).kind, ItemKind::Shape(_)) {
                        self.consumed.insert(r);
                    }
                }
            }
        }
        let ncols = columns.len().saturating_sub(1).max(1);
        let mut grid: Vec<Twips> = if columns.len() >= 2 {
            columns
                .windows(2)
                .map(|w| tw((w[1] - w[0]).max(1.0)))
                .collect()
        } else {
            vec![tw(ctx.width.max(72.0))]
        };
        for (c, add) in self.slack(n).into_iter().enumerate() {
            if add == 0 || c >= grid.len() {
                continue;
            }
            grid[c] += add;
            let flexible = (c + 1..grid.len())
                .chain((0..c).rev())
                .find(|&d| !content_sized.get(d).copied().unwrap_or(false));
            if let Some(d) = flexible {
                grid[d] = (grid[d] - add).max(20);
            }
        }
        let nrows = cells
            .iter()
            .filter_map(|&c| match self.kind(c) {
                NodeKind::Cell { row, rowspan, .. } => Some(row + rowspan),
                _ => None,
            })
            .max()
            .unwrap_or(0) as usize;
        let top = tops.first().map(|&(page, y)| {
            let first = self
                .row_top(&cells, 0, page)
                .map_or(y, |(top, inset)| top - inset);
            (page, first)
        });
        let mut opens = false;
        let mut drawings = Vec::new();
        let mut closed = HashSet::new();
        let border = |b: Option<(f64, Rgb)>| {
            b.map(|(w, c)| Border {
                size: ((w * 8.0).round() as u32).clamp(2, 96),
                color: hex(c),
            })
        };
        let mut rows = Vec::new();
        for r in 0..nrows {
            let mut row_cells = Vec::new();
            let mut c = 0;
            while c < ncols {
                let here = cells.iter().copied().find(|&m| {
                    matches!(self.kind(m), NodeKind::Cell { col, row, .. } if *col as usize == c && *row as usize == r)
                });
                let covering = cells.iter().copied().find(|&m| {
                    matches!(self.kind(m), NodeKind::Cell { col, row, rowspan, .. }
                        if *col as usize == c && (*row as usize) < r && (*row + *rowspan) as usize > r)
                });
                let Some(m) = here.or(covering) else {
                    row_cells.push(Cell {
                        props: CellProps {
                            width: grid.get(c).copied(),
                            span: 1,
                            ..CellProps::default()
                        },
                        body: Blocks::default(),
                    });
                    c += 1;
                    continue;
                };
                let NodeKind::Cell {
                    colspan,
                    rowspan,
                    fill,
                    inset,
                    borders,
                    ..
                } = self.kind(m).clone()
                else {
                    unreachable!()
                };
                let span = colspan.max(1).min((ncols - c) as u32);
                let (body, merge) = if here.is_some() {
                    let inner = Ctx {
                        left: columns.get(c).copied().unwrap_or(ctx.left) + inset.left,
                        width: grid.iter().skip(c).take(span as usize).sum::<i32>() as f64 / 20.0,
                        ..Ctx::default()
                    };
                    let top = self
                        .doc
                        .subtree(m)
                        .into_iter()
                        .find_map(|d| self.doc.nodes[d].lines.first().map(|l| l.page))
                        .and_then(|page| {
                            let (top, _) = self.row_top(&cells, r as u32, page)?;
                            Some((page, top))
                        });
                    let (mut blocks, left) = space::nested(self.children(m, &inner), top);
                    drawings.extend(left);
                    if matches!(blocks.last(), Some(Block::Table(_))) {
                        blocks.push(Block::Paragraph(spacer(CLOSING)));
                        closed.insert(m);
                    }
                    let header = (r as u32) < header_rows;
                    for b in &mut blocks {
                        if let Block::Paragraph(p) = b {
                            p.props.keep_next |= header;
                        }
                    }
                    opens |=
                        r == 0 && c == 0 && matches!(blocks.first(), Some(Block::Paragraph(_)));
                    (
                        Blocks::from_blocks(blocks),
                        (rowspan > 1).then_some(VMerge::Restart),
                    )
                } else {
                    (Blocks::default(), Some(VMerge::Continue))
                };
                let [t, rt, b, l] = borders;
                row_cells.push(Cell {
                    props: CellProps {
                        width: Some(grid.iter().skip(c).take(span as usize).sum()),
                        span,
                        merge,
                        borders: Some(Borders {
                            top: border(t),
                            left: border(l),
                            bottom: border(b),
                            right: border(rt),
                            inside_h: None,
                            inside_v: None,
                        }),
                        fill: fill.map(hex),
                        no_wrap: span == 1 && content_sized.get(c).copied().unwrap_or(false),
                        margins: Some(CellMargins {
                            top: tw(inset.top),
                            left: tw(inset.left),
                            bottom: tw(inset.bottom),
                            right: tw(inset.right),
                        }),
                        valign: None,
                    },
                    body,
                });
                c += span as usize;
            }
            let height = match (tops.get(r), tops.get(r + 1)) {
                (Some((p, a)), Some((q, b))) if p == q => Some(tw(b - a)),
                _ => None,
            };
            rows.push(Row {
                props: RowProps {
                    cant_split: true,
                    header: (r as u32) < header_rows,
                    min_height: height,
                },
                cells: row_cells,
            });
        }
        let end = self.doc.nodes[n].frames.last().map(|&(p, f)| {
            let bottom = self
                .table_bottom(&cells, nrows as u32, p, &closed)
                .unwrap_or(f.y1);
            (p, bottom)
        });
        let table = Table {
            props: TableProps {
                style: None,
                width: Some(grid.iter().sum()),
                indent: columns.first().map(|x| tw(x - ctx.left)),
                borders: None,
                fixed: true,
                cell_margins: None,
            },
            grid,
            rows,
        };
        let mut out = vec![Entry::Table(table, Rows { top, opens, end })];
        out.extend(
            drawings
                .into_iter()
                .map(|(page, d)| Entry::Drawing(page, d)),
        );
        out
    }
}
