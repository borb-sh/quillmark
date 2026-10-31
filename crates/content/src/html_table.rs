//! A table's HTML spelling (markdown-spec §6.8): the subset of HTML a table
//! is written in where a GFM pipe table cannot carry it, read off an HTML block
//! into the grid a table island holds, and written back from one.

use crate::carrier;
use crate::html;
use crate::import::Dropped;
use crate::island::{
    CELL_VALIGNS, TABLE_ALIGNS, TableLayout, cell_alignment, is_weight, table_rows, table_spans,
};
use pulldown_cmark::{CowStr, Event, Parser, Tag as PTag, TagEnd};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::ops::Range;

/// The subset's elements.
const TAGS: [&str; 7] = ["table", "thead", "tbody", "tfoot", "tr", "th", "td"];

/// The values of an `aligns` entry, a column's alignment.
const COLUMN_ALIGNS: [&str; 4] = ["none", "left", "center", "right"];

/// The `widths` token for an auto-fit column, a `null` entry.
const AUTO: &str = "auto";

/// Attributes that style a table and carry none of its content, dropped
/// without a report.
const PRESENTATIONAL: [&str; 4] = ["border", "class", "scope", "style"];

/// The widest `colspan` read, the bound HTML reads one at.
const MAX_COLSPAN: usize = 1000;

/// The subset's element `name` names, read ASCII-case-insensitively.
fn subset_tag(name: &str) -> Option<&'static str> {
    TAGS.into_iter().find(|t| t.eq_ignore_ascii_case(name))
}

/// Whether a tag named `name` is one of the subset's elements.
pub(crate) fn is_tag(name: &str) -> bool {
    subset_tag(name).is_some()
}

/// Whether an HTML block's text opens a table: its first tag, at its start, is
/// a complete `<table>` open tag.
pub(crate) fn opens(text: &str) -> bool {
    let at = text.len() - text.trim_start_matches([' ', '\t', '\n']).len();
    html::tag_at(text, at).is_some_and(|t| !t.closing && t.name.eq_ignore_ascii_case("table"))
}

/// One table an HTML block spells, resolved into the rectangular grid a table
/// island holds.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    /// The layout keys the `<table>` tag and its `thead` set, each absent at
    /// its default.
    pub(crate) layout: Map<String, Value>,
    /// One entry per column.
    pub(crate) aligns: Vec<&'static str>,
    /// The head rows first, then the body's, each one slot per column.
    pub(crate) rows: Vec<Vec<Slot>>,
}

/// A slot of the grid: a cell the source writes, or an empty placeholder for
/// one a span covers or a short row leaves out.
#[derive(Debug, Clone)]
pub(crate) enum Slot {
    Cell(Cell),
    Empty,
}

#[derive(Debug, Clone)]
pub(crate) struct Cell {
    /// The cell's content as written, inline markdown.
    pub(crate) content: String,
    /// The content's offset in the source.
    pub(crate) at: usize,
    /// The cell keys its attributes set: `colspan` and `rowspan` as the grid
    /// resolves them, `align` and `valign`, each absent at its default.
    pub(crate) keys: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Head,
    Body,
    Foot,
}

struct RawCell {
    tag: &'static str,
    /// The content's start in the block text.
    start: usize,
    at: usize,
    content: String,
    colspan: usize,
    rowspan: usize,
    keys: Map<String, Value>,
}

struct RawRow {
    at: usize,
    cells: Vec<RawCell>,
    /// Whether the row's cells, those the scan leaves out among them, are all
    /// `<th>`, and the longest `rowspan` among them, 0 for a row of none.
    th: bool,
    rowspan: usize,
    /// Whether the scan left out a cell past the table's slot budget.
    cut: bool,
}

impl RawRow {
    fn new(at: usize) -> Self {
        Self {
            at,
            cells: Vec::new(),
            th: true,
            rowspan: 0,
            cut: false,
        }
    }

    fn heading(&self) -> bool {
        self.rowspan > 0 && self.th
    }
}

struct RawGroup {
    kind: Group,
    rows: Vec<RawRow>,
}

struct RawTable {
    at: usize,
    layout: Map<String, Value>,
    aligns: Vec<&'static str>,
    groups: Vec<RawGroup>,
    /// The `colspan` totals of the head, body and foot cells kept so far.
    slots: [usize; 3],
}

/// Each table an HTML block's `text` spells and what it drops, at the source
/// offset `at` maps a block offset to, the tables' slots spent from `budget`.
///
/// The subset's tags are structure wherever they stand outside a code span, a
/// comment, an angle-bracketed link destination or a backslash escape in a
/// cell; HTML's own omissions read as HTML reads them. Any other tag outside a
/// cell drops alone, and so does text outside every cell. A `<table>` the
/// block ends before its `</table>`, or that another `<table>` interrupts,
/// closes there.
pub(crate) fn scan(
    text: &str,
    at: impl Fn(usize) -> usize,
    budget: &mut usize,
) -> (Vec<Table>, Vec<(Dropped, usize)>) {
    let s = run(text, at, *budget);
    *budget = s.budget;
    (s.tables, s.drops)
}

/// The offset just past each `</table>` closing a table in an HTML block's
/// `text` that anything but whitespace follows.
pub(crate) fn tails(text: &str) -> Vec<usize> {
    let mut ends = run(text, |i| i, 0).ends;
    ends.retain(|&end| !text[end..].trim().is_empty());
    ends
}

fn run<F: Fn(usize) -> usize>(text: &str, at: F, budget: usize) -> Scan<'_, F> {
    let mut s = Scan {
        text,
        at,
        budget,
        tables: Vec::new(),
        drops: Vec::new(),
        ends: Vec::new(),
        table: None,
        group_open: false,
        row: None,
        cell: None,
    };
    let b = text.as_bytes();
    let last_close = text.rfind("-->");
    let mut runs: Option<HashMap<usize, Vec<usize>>> = None;
    let mut tags: Option<Vec<usize>> = None;
    let mut i = 0;
    let mut stray = false;
    while i < b.len() {
        if s.cell.is_some() {
            match b[i] {
                b'\\' if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) => {
                    i += 2;
                    continue;
                }
                b'`' => {
                    let runs = runs.get_or_insert_with(|| backtick_runs(b));
                    let tags =
                        tags.get_or_insert_with(|| tag_spans(text).map(|t| t.start).collect());
                    i = code_span_end(b, i, runs, tags);
                    continue;
                }
                b']' if text[i..].starts_with("](<") => {
                    i = destination_end(b, i + 2).unwrap_or(i + 1);
                    continue;
                }
                b'<' => {}
                _ => {
                    i += 1;
                    continue;
                }
            }
        }
        if b[i] == b'<' {
            if let Some(end) = comment_end(text, i, last_close) {
                i = end;
                continue;
            }
            if let Some(tag) = html::tag_at(text, i) {
                if let Some(name) = subset_tag(tag.name) {
                    s.structural(name, &tag, i);
                    stray = false;
                } else if s.cell.is_none() {
                    if let Some(dropped) =
                        crate::import::dropped_tag(tag.name).filter(|_| !tag.closing)
                    {
                        s.drops.push((dropped, (s.at)(i)));
                    }
                    stray = false;
                }
                i = tag.span.end;
                continue;
            }
        }
        if s.cell.is_none() && !b[i].is_ascii_whitespace() && !std::mem::replace(&mut stray, true) {
            s.drops.push((Dropped::TableText, (s.at)(i)));
        }
        i += 1;
    }
    s.close_table(text.len(), true);
    s
}

struct Scan<'a, F> {
    text: &'a str,
    at: F,
    /// The slots the block's tables may still take.
    budget: usize,
    tables: Vec<Table>,
    drops: Vec<(Dropped, usize)>,
    /// The offset past each `</table>` that closes a table.
    ends: Vec<usize>,
    table: Option<RawTable>,
    /// Whether the table's last row group takes the next row: one a group tag
    /// opened, or the one a row standing in none opened.
    group_open: bool,
    row: Option<RawRow>,
    cell: Option<RawCell>,
}

impl<F: Fn(usize) -> usize> Scan<'_, F> {
    /// One of the subset's tags, named `name`, at block offset `i`.
    fn structural(&mut self, name: &'static str, tag: &html::Tag, i: usize) {
        let at = (self.at)(i);
        match (name, tag.closing) {
            ("table", false) => {
                self.close_table(i, true);
                let mut layout = Map::new();
                let mut aligns = Vec::new();
                self.attrs(name, tag, at, |attr, value| match attr {
                    "widths" => widths(value)
                        .map(|v| layout.insert(attr.into(), v))
                        .is_some(),
                    "aligns" => column_aligns(value).map(|a| aligns = a).is_some(),
                    "align" => one_of(value, &TABLE_ALIGNS)
                        .map(|a| layout.insert(attr.into(), a.into()))
                        .is_some(),
                    _ => false,
                });
                self.table = Some(RawTable {
                    at,
                    layout,
                    aligns,
                    groups: Vec::new(),
                    slots: [0; 3],
                });
            }
            ("table", true) => {
                if self.table.is_some() {
                    self.ends.push(tag.span.end);
                }
                self.close_table(i, false);
            }
            (_, false) if self.table.is_none() => self.drops.push((Dropped::Tag(name.into()), at)),
            (_, true) if self.table.is_none() => {}
            ("thead" | "tbody" | "tfoot", closing) => {
                self.close_row(i);
                self.group_open = !closing;
                if !closing {
                    self.attrs(name, tag, at, |_, _| false);
                    let group = match name {
                        "thead" => Group::Head,
                        "tfoot" => Group::Foot,
                        _ => Group::Body,
                    };
                    if let Some(table) = self.table.as_mut() {
                        table.groups.push(RawGroup {
                            kind: group,
                            rows: Vec::new(),
                        });
                    }
                }
            }
            ("tr", false) => {
                self.close_row(i);
                self.attrs(name, tag, at, |_, _| false);
                self.row = Some(RawRow::new(at));
            }
            ("tr", true) => self.close_row(i),
            (_, false) => {
                self.close_cell(i);
                let (mut colspan, mut rowspan) = (1, 1);
                let mut keys = Map::new();
                self.attrs(name, tag, at, |attr, value| match attr {
                    "colspan" => span(value).map(|n| colspan = n.min(MAX_COLSPAN)).is_some(),
                    "rowspan" => span(value).map(|n| rowspan = n).is_some(),
                    "align" => one_of(value, &TABLE_ALIGNS)
                        .map(|a| keys.insert(attr.into(), a.into()))
                        .is_some(),
                    "valign" => one_of(value, &CELL_VALIGNS)
                        .map(|v| keys.insert(attr.into(), v.into()))
                        .is_some(),
                    _ => false,
                });
                self.row.get_or_insert_with(|| RawRow::new(at));
                let start = tag.span.end;
                let content = String::new();
                let at = (self.at)(start);
                self.cell = Some(RawCell {
                    tag: name,
                    start,
                    at,
                    content,
                    colspan,
                    rowspan,
                    keys,
                });
            }
            (_, true) => self.close_cell(i),
        }
    }

    /// Read a tag's attributes, each `read` takes kept; any other drops alone.
    fn attrs(
        &mut self,
        tag_name: &str,
        tag: &html::Tag,
        at: usize,
        mut read: impl FnMut(&str, &str) -> bool,
    ) {
        let attrs = carrier::decode_attrs(&tag.attrs);
        let mut dropped = attrs.refused;
        for (attr, value) in attrs.values {
            if !read(&attr, &value) {
                dropped.push(attr);
            }
        }
        for attr in dropped
            .into_iter()
            .filter(|a| !PRESENTATIONAL.contains(&a.as_str()))
        {
            self.drops.push((
                Dropped::TableAttr {
                    tag: tag_name.into(),
                    attr,
                },
                at,
            ));
        }
    }

    /// Close the open cell at block offset `end`. A cell placed past every
    /// slot the budget leaves, by the `colspan` totals of the cells it follows
    /// in grid order, is left out, its row marked cut: no grid holds it, and
    /// the scan holds no more cells than a grid could.
    fn close_cell(&mut self, end: usize) {
        let (Some(mut cell), Some(row), Some(table)) =
            (self.cell.take(), self.row.as_mut(), self.table.as_mut())
        else {
            return;
        };
        row.th &= cell.tag == "th";
        row.rowspan = row.rowspan.max(cell.rowspan);
        let kind = match table.groups.last() {
            Some(group) if self.group_open => group.kind,
            _ => Group::Body,
        } as usize;
        table.slots[kind] = table.slots[kind].saturating_add(cell.colspan);
        if table.slots[..=kind]
            .iter()
            .fold(0, |sum: usize, &n| sum.saturating_add(n))
            > self.budget
        {
            row.cut = true;
            return;
        }
        cell.content = self.text[cell.start..end.max(cell.start)].to_string();
        row.cells.push(cell);
    }

    fn close_row(&mut self, end: usize) {
        self.close_cell(end);
        let (Some(row), Some(table)) = (self.row.take(), self.table.as_mut()) else {
            return;
        };
        if !self.group_open || table.groups.is_empty() {
            table.groups.push(RawGroup {
                kind: Group::Body,
                rows: Vec::new(),
            });
            self.group_open = true;
        }
        if let Some(group) = table.groups.last_mut() {
            group.rows.push(row);
        }
    }

    /// Close the open table at block offset `end`, reported where `cut`.
    fn close_table(&mut self, end: usize, cut: bool) {
        self.close_row(end);
        self.group_open = false;
        let Some(raw) = self.table.take() else {
            return;
        };
        if cut {
            self.drops.push((Dropped::Table, raw.at));
        }
        if let Some(table) = resolve(raw, self.budget, &mut self.drops) {
            self.budget -= table.rows.len() * table.aligns.len();
            self.tables.push(table);
        }
    }
}

/// The grid a table's rows resolve into, as HTML's table model forms one: the
/// `thead` rows first and the `tfoot` rows last, each cell in the next slot of
/// its row no span covers. In a table with no `thead`, the rows opening its
/// first body group, its `tbody` tag written or not, whose cells are all
/// `<th>` are its head rows, as many as no `rowspan` among them reaches past.
/// The grid is as wide as its widest row. A `rowspan` stops at the end of its
/// row group, and a span over a slot an earlier span covers is left for
/// normalization to settle. A row too short pads, reported. The rows from the
/// first that takes the grid past `budget` slots drop, reported. `None` for a
/// table holding no cell within that.
fn resolve(raw: RawTable, budget: usize, drops: &mut Vec<(Dropped, usize)>) -> Option<Table> {
    let mut groups = raw.groups;
    if !groups.iter().any(|g| g.kind == Group::Head)
        && let Some(first) = groups.first_mut().filter(|g| g.kind == Group::Body)
    {
        let (mut n, mut reach) = (0, 0);
        for (r, row) in first
            .rows
            .iter()
            .take_while(|row| row.heading())
            .enumerate()
        {
            reach = reach.max(r.saturating_add(row.rowspan));
            if reach <= r + 1 {
                n = r + 1;
            }
        }
        let rows: Vec<RawRow> = first.rows.drain(..n).collect();
        groups.insert(
            0,
            RawGroup {
                kind: Group::Head,
                rows,
            },
        );
    }
    groups.sort_by_key(|group| group.kind as u8);
    let head: usize = groups
        .iter()
        .filter(|g| g.kind == Group::Head)
        .map(|g| g.rows.len())
        .sum();
    let mut rows: Vec<(RawRow, usize)> = Vec::new();
    for group in groups {
        let end = rows.len() + group.rows.len();
        rows.extend(group.rows.into_iter().map(|row| (row, end)));
    }
    let mut covers: Vec<usize> = Vec::new();
    let mut placed = Vec::with_capacity(rows.len());
    let mut cols = 0;
    for (r, (row, end)) in rows.into_iter().enumerate() {
        let mut cells = Vec::with_capacity(row.cells.len());
        let (mut x, mut past) = (0, false);
        for cell in row.cells {
            while covers.get(x).is_some_and(|&n| n > 0) {
                x += 1;
            }
            if x + cell.colspan > budget / (r + 1) {
                past = true;
                break;
            }
            let h = cell.rowspan.min(end - r);
            if covers.len() < x + cell.colspan {
                covers.resize(x + cell.colspan, 0);
            }
            for n in &mut covers[x..x + cell.colspan] {
                *n = (*n).max(h);
            }
            let mut keys = cell.keys;
            for (key, n) in [("colspan", cell.colspan), ("rowspan", h)] {
                if n > 1 {
                    keys.insert(key.into(), n.into());
                }
            }
            cells.push((
                x,
                Cell {
                    content: cell.content,
                    at: cell.at,
                    keys,
                },
            ));
            x += cell.colspan;
        }
        let wide = covers
            .iter()
            .rposition(|&n| n > 0)
            .map_or(cols, |k| cols.max(k + 1));
        if past || row.cut || (r + 1).saturating_mul(wide) > budget {
            drops.push((Dropped::TableSize, row.at));
            break;
        }
        cols = wide;
        placed.push((row.at, cells, covers.iter().filter(|&&n| n > 0).count()));
        for n in &mut covers {
            *n = n.saturating_sub(1);
        }
    }
    if cols == 0 {
        return None;
    }
    let grid: Vec<Vec<Slot>> = placed
        .into_iter()
        .map(|(at, cells, reach)| {
            if reach < cols {
                drops.push((Dropped::TableRow, at));
            }
            let mut slots = vec![Slot::Empty; cols];
            for (x, cell) in cells {
                slots[x] = Slot::Cell(cell);
            }
            slots
        })
        .collect();
    let mut layout = raw.layout;
    let head = head.min(grid.len());
    if head != 1 {
        layout.insert("head".into(), head.into());
    }
    let mut aligns = raw.aligns;
    aligns.resize(cols, "none");
    Some(Table {
        layout,
        aligns,
        rows: grid,
    })
}

/// The offset just past the code span a backtick run at `i` opens: through
/// the next run of its length in `runs`, or past the run where none closes it
/// before the next of the subset's `tags`, which a code span in a cell never
/// holds.
fn code_span_end(b: &[u8], i: usize, runs: &HashMap<usize, Vec<usize>>, tags: &[usize]) -> usize {
    let n = b[i..].iter().take_while(|&&c| c == b'`').count();
    let starts = runs.get(&n).map_or(&[][..], Vec::as_slice);
    let tag = tags
        .get(tags.partition_point(|&at| at < i + n))
        .copied()
        .unwrap_or(usize::MAX);
    match starts.get(starts.partition_point(|&at| at < i + n)) {
        Some(&at) if at < tag => at + n,
        _ => i + n,
    }
}

/// The span of each of the subset's tags in `text`, wherever it stands.
pub(crate) fn tag_spans(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    text.match_indices('<').filter_map(|(i, _)| {
        html::tag_at(text, i)
            .filter(|t| is_tag(t.name))
            .map(|t| t.span)
    })
}

/// `text` cut at the subset's tags, in order: each piece's span, and whether
/// it is a tag.
pub(crate) fn tag_pieces(text: &str) -> Vec<(Range<usize>, bool)> {
    let mut out = Vec::new();
    let mut taken = 0;
    for tag in tag_spans(text) {
        if tag.start < taken {
            continue;
        }
        if taken < tag.start {
            out.push((taken..tag.start, false));
        }
        taken = tag.end;
        out.push((tag, true));
    }
    if taken < text.len() {
        out.push((taken..text.len(), false));
    }
    out
}

/// Where each maximal run of backticks in `b` starts, by its length.
fn backtick_runs(b: &[u8]) -> HashMap<usize, Vec<usize>> {
    let mut runs: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut i = 0;
    while let Some(off) = b[i..].iter().position(|&c| c == b'`') {
        let at = i + off;
        let n = b[at..].iter().take_while(|&&c| c == b'`').count();
        runs.entry(n).or_default().push(at);
        i = at + n;
    }
    runs
}

/// The offset just past an angle-bracketed link destination opening at `i`,
/// or `None` where none closes on its line.
fn destination_end(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while let Some(&c) = b.get(j) {
        match c {
            b'\\' if b.get(j + 1).is_some_and(u8::is_ascii_punctuation) => j += 2,
            b'>' => return Some(j + 1),
            b'<' | b'\n' => return None,
            _ => j += 1,
        }
    }
    None
}

/// The offset just past an HTML comment opening at `i`, as CommonMark reads
/// one: `<!-->`, `<!--->`, or `<!--` through the next `-->`, which none
/// past `last_close`, the text's last `-->`, holds.
fn comment_end(text: &str, i: usize, last_close: Option<usize>) -> Option<usize> {
    let rest = text[i..].strip_prefix("<!--")?;
    if rest.starts_with('>') {
        return Some(i + 5);
    }
    if rest.starts_with("->") {
        return Some(i + 6);
    }
    last_close.filter(|&at| at >= i + 4)?;
    rest.find("-->").map(|at| i + 4 + at + 3)
}

/// A `widths` value: whitespace-separated tokens, each a decimal
/// [weight](is_weight) or `auto`.
fn widths(value: &str) -> Option<Value> {
    value
        .split_ascii_whitespace()
        .map(|t| match t.eq_ignore_ascii_case(AUTO) {
            true => Some(Value::Null),
            false => whole(t).filter(|n| is_weight(*n)).map(Value::from),
        })
        .collect::<Option<Vec<_>>>()
        .map(Value::Array)
}

/// An `aligns` value: whitespace-separated tokens, each one of
/// [`COLUMN_ALIGNS`].
fn column_aligns(value: &str) -> Option<Vec<&'static str>> {
    value
        .split_ascii_whitespace()
        .map(|t| one_of(t, &COLUMN_ALIGNS))
        .collect()
}

/// The member of `set` a value names, ASCII-case-insensitively.
fn one_of(value: &str, set: &[&'static str]) -> Option<&'static str> {
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    set.iter().copied().find(|v| v.eq_ignore_ascii_case(value))
}

/// A `colspan` or `rowspan` value: a whole number from 1, one too large for a
/// `u64` read as the largest.
fn span(value: &str) -> Option<usize> {
    let n = whole(value.trim_matches(|c: char| c.is_ascii_whitespace()))?;
    (n >= 1).then(|| usize::try_from(n).unwrap_or(usize::MAX))
}

/// A run of ASCII digits read as a whole number, saturating.
fn whole(token: &str) -> Option<u64> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(token.parse().unwrap_or(u64::MAX))
}

/// A cell's content as the one line of inline markdown the import reads it
/// as: its edge whitespace dropped and each line break, with the spaces and
/// tabs beside it, a space, as HTML reads one, between `,` sentinels that keep
/// any block construct from opening. A trailing backslash, which would escape
/// the sentinel, is doubled into the one backslash it reads as.
pub(crate) fn cell_source(content: &str) -> String {
    let content = content.trim_matches([' ', '\t', '\n']);
    let mut src = String::with_capacity(content.len() + 3);
    src.push(',');
    for (k, line) in content.split('\n').enumerate() {
        if k > 0 {
            src.push(' ');
        }
        src.push_str(line.trim_matches([' ', '\t']));
    }
    if (src.len() - src.trim_end_matches('\\').len()) % 2 == 1 {
        src.push('\\');
    }
    src.push(',');
    src
}

/// The inline events of `src`, a [`cell_source`], with its sentinels taken off
/// the text events that hold them, and each code span split around the
/// subset's tags it holds, each tag its text, as a span the scan reads never
/// holds one.
pub(crate) fn cell_events(src: &str) -> Vec<(Event<'_>, Range<usize>)> {
    let mut events: Vec<(Event, Range<usize>)> = Vec::new();
    for (event, range) in Parser::new_ext(src, crate::import::options()).into_offset_iter() {
        match event {
            Event::Start(PTag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            Event::Code(code) if tag_spans(&code).next().is_some() => {
                for (piece, tag) in tag_pieces(&code) {
                    let piece = code[piece].to_string().into();
                    events.push((
                        if tag {
                            Event::Text(piece)
                        } else {
                            Event::Code(piece)
                        },
                        range.clone(),
                    ));
                }
            }
            event => events.push((event, range)),
        }
    }
    let first = events.iter().position(|(e, _)| matches!(e, Event::Text(_)));
    let last = events
        .iter()
        .rposition(|(e, _)| matches!(e, Event::Text(_)));
    for (k, lead) in [(first, true), (last, false)] {
        if let Some((Event::Text(t), _)) = k.and_then(|k| events.get_mut(k)) {
            let kept = match lead {
                true => t.strip_prefix(','),
                false => t.strip_suffix(','),
            };
            if let Some(kept) = kept {
                *t = CowStr::from(kept.to_string());
            }
        }
    }
    events
}

/// Whether a table writes as HTML: one holding more or fewer header rows than
/// one, `widths`, a placement, or a cell's alignment or span, which a GFM pipe
/// table has no spelling for.
pub(crate) fn writes_html(props: &Value) -> bool {
    let layout = TableLayout::of(props);
    layout.head != 1
        || layout.widths.is_some()
        || layout.align.is_some()
        || crate::serial::table_cell_values(props).any(|cell| cell_alignment(cell) != (None, None))
        || table_spans(props)
            .iter()
            .flatten()
            .flatten()
            .any(|span| *span != (1, 1))
}

/// The table's HTML spelling: the `<table>` tag's layout attributes, its head
/// rows in a `thead` and the rest in a `tbody`, each row on its own line and
/// each drawn cell's content as `cell` writes it. A covered cell is left out,
/// as HTML leaves one out.
pub(crate) fn write(props: &Value, cell: impl Fn(&Value) -> String) -> String {
    let layout = TableLayout::of(props);
    let rows = table_rows(props);
    let spans = table_spans(props);
    let cols = rows[0].len();
    let mut out = String::from("<table");
    if let Some(widths) = &layout.widths {
        let tokens: Vec<String> = (0..cols)
            .map(|k| {
                widths
                    .get(k)
                    .copied()
                    .flatten()
                    .map_or_else(|| AUTO.to_string(), |n| n.to_string())
            })
            .collect();
        out.push_str(&format!(" widths=\"{}\"", tokens.join(" ")));
    }
    let aligns: Vec<&str> = (0..cols)
        .map(|k| {
            let entry = props
                .get("aligns")
                .and_then(|a| a.get(k))
                .and_then(Value::as_str);
            entry
                .and_then(|a| COLUMN_ALIGNS.into_iter().find(|v| *v == a))
                .unwrap_or("none")
        })
        .collect();
    if aligns.iter().any(|a| *a != "none") {
        out.push_str(&format!(" aligns=\"{}\"", aligns.join(" ")));
    }
    if let Some(align) = layout.align {
        out.push_str(&format!(" align=\"{align}\""));
    }
    out.push('>');
    for (r, row) in rows.iter().enumerate() {
        if r == 0 && layout.head > 0 {
            out.push_str("\n<thead>");
        }
        if r == layout.head {
            if r > 0 {
                out.push_str("\n</thead>");
            }
            out.push_str("\n<tbody>");
        }
        let tag = if r < layout.head { "th" } else { "td" };
        out.push_str("\n<tr>");
        for (c, value) in row.iter().enumerate() {
            let Some((w, h)) = spans[r][c] else { continue };
            out.push('<');
            out.push_str(tag);
            for (key, n) in [("colspan", w), ("rowspan", h)] {
                if n > 1 {
                    out.push_str(&format!(" {key}=\"{n}\""));
                }
            }
            let (align, valign) = cell_alignment(value);
            for (key, v) in [("align", align), ("valign", valign)] {
                if let Some(v) = v {
                    out.push_str(&format!(" {key}=\"{v}\""));
                }
            }
            out.push('>');
            out.push_str(&cell(value));
            out.push_str(&format!("</{tag}>"));
        }
        out.push_str("</tr>");
    }
    out.push_str(if layout.head == rows.len() {
        "\n</thead>"
    } else {
        "\n</tbody>"
    });
    out.push_str("\n</table>");
    out
}

#[cfg(test)]
mod tests {
    mod properties;
}
