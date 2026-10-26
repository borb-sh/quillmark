//! A table's HTML spelling (markdown-spec §6.4): the subset of HTML a table
//! is written in where a GFM pipe table cannot carry it, read off an HTML block
//! into the grid a table island holds, and written back from one.

use crate::carrier;
use crate::html;
use crate::import::Dropped;
use crate::island::{cell_alignment, is_weight, table_rows, table_spans, TableLayout, CELL_VALIGNS, TABLE_ALIGNS};
use pulldown_cmark::{CowStr, Event, Parser, Tag as PTag, TagEnd};
use serde_json::{Map, Value};
use std::ops::Range;

/// The subset's elements.
const TAGS: [&str; 7] = ["table", "thead", "tbody", "tfoot", "tr", "th", "td"];

/// The values of an `aligns` entry, a column's alignment.
const COLUMN_ALIGNS: [&str; 4] = ["none", "left", "center", "right"];

/// The `widths` token for an auto-fit column, a `null` entry.
const AUTO: &str = "auto";

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
}

struct RawGroup {
    kind: Group,
    /// Opened by a row standing in no group tag.
    implicit: bool,
    rows: Vec<RawRow>,
}

struct RawTable {
    at: usize,
    layout: Map<String, Value>,
    aligns: Vec<&'static str>,
    groups: Vec<RawGroup>,
}

/// Each table an HTML block's `text` spells and what it drops, at the source
/// offset `at` maps a block offset to.
///
/// The subset's tags are structure wherever they stand outside a code span, a
/// comment, an angle-bracketed link destination or a backslash escape in a
/// cell; HTML's own omissions read as HTML reads them. Any other tag outside a
/// cell drops alone, and so does text outside every cell. A `<table>` the
/// block ends before its `</table>`, or that another `<table>` interrupts,
/// closes there.
pub(crate) fn scan(text: &str, at: impl Fn(usize) -> usize) -> (Vec<Table>, Vec<(Dropped, usize)>) {
    let s = run(text, at);
    (s.tables, s.drops)
}

/// The offset just past the first `</table>` closing a table in an HTML
/// block's `text` that anything but whitespace follows.
pub(crate) fn tail(text: &str) -> Option<usize> {
    run(text, |i| i).ends.into_iter().find(|&end| !text[end..].trim().is_empty())
}

fn run<F: Fn(usize) -> usize>(text: &str, at: F) -> Scan<'_, F> {
    let mut s = Scan {
        text,
        at,
        tables: Vec::new(),
        drops: Vec::new(),
        ends: Vec::new(),
        table: None,
        group_open: false,
        row: None,
        cell: None,
    };
    let b = text.as_bytes();
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
                    i = code_span_end(b, i);
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
            if let Some(end) = comment_end(text, i) {
                i = end;
                continue;
            }
            if let Some(tag) = html::tag_at(text, i) {
                if let Some(name) = subset_tag(tag.name) {
                    s.structural(name, &tag, i);
                    stray = false;
                } else if s.cell.is_none() {
                    if let Some(dropped) = crate::import::dropped_tag(tag.name).filter(|_| !tag.closing) {
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
                    "widths" => widths(value).map(|v| layout.insert(attr.into(), v)).is_some(),
                    "aligns" => column_aligns(value).map(|a| aligns = a).is_some(),
                    "align" => one_of(value, &TABLE_ALIGNS).map(|a| layout.insert(attr.into(), a.into())).is_some(),
                    _ => false,
                });
                self.table = Some(RawTable { at, layout, aligns, groups: Vec::new() });
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
                        table.groups.push(RawGroup { kind: group, implicit: false, rows: Vec::new() });
                    }
                }
            }
            ("tr", false) => {
                self.close_row(i);
                self.attrs(name, tag, at, |_, _| false);
                self.row = Some(RawRow { at, cells: Vec::new() });
            }
            ("tr", true) => self.close_row(i),
            (_, false) => {
                self.close_cell(i);
                let (mut colspan, mut rowspan) = (1, 1);
                let mut keys = Map::new();
                self.attrs(name, tag, at, |attr, value| match attr {
                    "colspan" => span(value).map(|n| colspan = n.min(MAX_COLSPAN)).is_some(),
                    "rowspan" => span(value).map(|n| rowspan = n).is_some(),
                    "align" => one_of(value, &TABLE_ALIGNS).map(|a| keys.insert(attr.into(), a.into())).is_some(),
                    "valign" => one_of(value, &CELL_VALIGNS).map(|v| keys.insert(attr.into(), v.into())).is_some(),
                    _ => false,
                });
                self.row.get_or_insert_with(|| RawRow { at, cells: Vec::new() });
                let start = tag.span.end;
                let content = String::new();
                let at = (self.at)(start);
                self.cell = Some(RawCell { tag: name, start, at, content, colspan, rowspan, keys });
            }
            (_, true) => self.close_cell(i),
        }
    }

    /// Read a tag's attributes, each `read` takes kept; any other drops alone.
    fn attrs(&mut self, tag_name: &str, tag: &html::Tag, at: usize, mut read: impl FnMut(&str, &str) -> bool) {
        let attrs = carrier::decode_attrs(&tag.attrs);
        let mut dropped = attrs.refused;
        for (attr, value) in attrs.values {
            if !read(&attr, &value) {
                dropped.push(attr);
            }
        }
        for attr in dropped {
            self.drops.push((Dropped::TableAttr { tag: tag_name.into(), attr }, at));
        }
    }

    fn close_cell(&mut self, end: usize) {
        if let Some(mut cell) = self.cell.take() {
            cell.content = self.text[cell.start..end.max(cell.start)].to_string();
            if let Some(row) = self.row.as_mut() {
                row.cells.push(cell);
            }
        }
    }

    fn close_row(&mut self, end: usize) {
        self.close_cell(end);
        let (Some(row), Some(table)) = (self.row.take(), self.table.as_mut()) else {
            return;
        };
        if !self.group_open || table.groups.is_empty() {
            table.groups.push(RawGroup { kind: Group::Body, implicit: true, rows: Vec::new() });
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
        if let Some(table) = resolve(raw, &mut self.drops) {
            self.tables.push(table);
        }
    }
}

/// The grid a table's rows resolve into, as HTML's table model forms one: the
/// `thead` rows first and the `tfoot` rows last, each cell in the next slot of
/// its row no span covers. In a table with no `thead`, the rows opening it
/// outside any group tag whose cells are all `<th>` are its head rows, as many
/// as no `rowspan` among them reaches past. The
/// grid is as wide as its first row holding a cell. A `colspan` stops at the
/// grid's edge, and short of a slot another span covers, reported; a
/// `rowspan` stops at the end of its row group. A row too short pads and one
/// too long drops its cells past the edge, reported. Rows past
/// [`MAX_TABLE_CELLS`](crate::MAX_TABLE_CELLS) slots drop, reported. `None` for
/// a table holding no cell, or whose first row alone is wider than that.
fn resolve(raw: RawTable, drops: &mut Vec<(Dropped, usize)>) -> Option<Table> {
    let mut groups = raw.groups;
    if !groups.iter().any(|g| g.kind == Group::Head)
        && let Some(first) = groups.first_mut().filter(|g| g.implicit)
    {
        let heading = |row: &RawRow| !row.cells.is_empty() && row.cells.iter().all(|c| c.tag == "th");
        let lead = first.rows.iter().take_while(|row| heading(row)).count();
        let contained = |n: usize| {
            let mut spans = first.rows[..n].iter().enumerate().flat_map(|(r, row)| row.cells.iter().map(move |c| (r, c)));
            spans.all(|(r, cell)| r.saturating_add(cell.rowspan) <= n)
        };
        let n = (1..=lead).rev().find(|&n| contained(n)).unwrap_or(0);
        let rows: Vec<RawRow> = first.rows.drain(..n).collect();
        groups.insert(0, RawGroup { kind: Group::Head, implicit: true, rows });
    }
    groups.sort_by_key(|group| group.kind as u8);
    let head: usize = groups.iter().filter(|g| g.kind == Group::Head).map(|g| g.rows.len()).sum();
    let mut rows: Vec<(RawRow, usize)> = Vec::new();
    for group in groups {
        let end = rows.len() + group.rows.len();
        rows.extend(group.rows.into_iter().map(|row| (row, end)));
    }
    let first = rows.iter().find(|(row, _)| !row.cells.is_empty())?;
    let cols = first.0.cells.iter().fold(0usize, |n, cell| n.saturating_add(cell.colspan));
    let fit = crate::MAX_TABLE_CELLS / cols;
    if fit < rows.len() {
        drops.push((Dropped::TableSize, rows[fit.min(rows.len() - 1)].0.at));
        if fit == 0 {
            return None;
        }
        rows.truncate(fit);
    }
    let mut covered = vec![vec![false; cols]; rows.len()];
    let mut grid: Vec<Vec<Slot>> = vec![vec![Slot::Empty; cols]; rows.len()];
    for (r, (row, end)) in rows.into_iter().enumerate() {
        let mut x = 0;
        let mut misfit = false;
        for cell in row.cells {
            while x < cols && covered[r][x] {
                x += 1;
            }
            if x == cols {
                misfit = true;
                continue;
            }
            let want = cell.colspan.min(cols - x);
            let w = 1 + (1..want).take_while(|&k| !covered[r][x + k]).count();
            if w < want {
                drops.push((Dropped::TableAttr { tag: cell.tag.into(), attr: "colspan".into() }, cell.at));
            }
            let h = cell.rowspan.min(end.min(fit) - r);
            for slots in &mut covered[r..r + h] {
                slots[x..x + w].fill(true);
            }
            let mut keys = cell.keys;
            for (key, n) in [("colspan", w), ("rowspan", h)] {
                if n > 1 {
                    keys.insert(key.into(), n.into());
                }
            }
            grid[r][x] = Slot::Cell(Cell { content: cell.content, at: cell.at, keys });
            x += w;
        }
        if misfit || covered[r].contains(&false) {
            drops.push((Dropped::TableRow, row.at));
        }
    }
    let mut layout = raw.layout;
    let head = head.min(grid.len());
    if head != 1 {
        layout.insert("head".into(), head.into());
    }
    let mut aligns = raw.aligns;
    aligns.resize(cols, "none");
    Some(Table { layout, aligns, rows: grid })
}

/// The offset just past the code span a backtick run at `i` opens, or past the
/// run where no run of its length closes one.
fn code_span_end(b: &[u8], i: usize) -> usize {
    let run = |at: usize| b[at..].iter().take_while(|&&c| c == b'`').count();
    let n = run(i);
    let mut j = i + n;
    while let Some(off) = b[j..].iter().position(|&c| c == b'`') {
        let m = run(j + off);
        if m == n {
            return j + off + m;
        }
        j += off + m;
    }
    i + n
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
/// one: `<!-->`, `<!--->`, or `<!--` through the next `-->`.
fn comment_end(text: &str, i: usize) -> Option<usize> {
    let rest = text[i..].strip_prefix("<!--")?;
    if rest.starts_with('>') {
        return Some(i + 5);
    }
    if rest.starts_with("->") {
        return Some(i + 6);
    }
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
    value.split_ascii_whitespace().map(|t| one_of(t, &COLUMN_ALIGNS)).collect()
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
/// the text events that hold them.
pub(crate) fn cell_events(src: &str) -> Vec<(Event<'_>, Range<usize>)> {
    let mut events: Vec<(Event, Range<usize>)> = Parser::new_ext(src, crate::import::options())
        .into_offset_iter()
        .filter(|(e, _)| !matches!(e, Event::Start(PTag::Paragraph) | Event::End(TagEnd::Paragraph)))
        .collect();
    let first = events.iter().position(|(e, _)| matches!(e, Event::Text(_)));
    let last = events.iter().rposition(|(e, _)| matches!(e, Event::Text(_)));
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
        || table_spans(props).iter().flatten().flatten().any(|span| *span != (1, 1))
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
            .map(|k| widths.get(k).copied().flatten().map_or_else(|| AUTO.to_string(), |n| n.to_string()))
            .collect();
        out.push_str(&format!(" widths=\"{}\"", tokens.join(" ")));
    }
    let aligns: Vec<&str> = (0..cols)
        .map(|k| {
            let entry = props.get("aligns").and_then(|a| a.get(k)).and_then(Value::as_str);
            entry.and_then(|a| COLUMN_ALIGNS.into_iter().find(|v| *v == a)).unwrap_or("none")
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
    out.push_str(if layout.head == rows.len() { "\n</thead>" } else { "\n</tbody>" });
    out.push_str("\n</table>");
    out
}

#[cfg(test)]
mod tests {
    mod properties;
}
