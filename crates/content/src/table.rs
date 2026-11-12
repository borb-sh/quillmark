//! A table island's props: `columns` (column ids), `rows` (`{id, cells}`, each
//! cell a column id mapped to the slot holding the cell's content) and the
//! layout keys `head`, `aligns`, `widths`, `align` and `cell_attrs`. `rows[0]`
//! is the header row; `head` counts header rows from the top, absent at 1;
//! `aligns` and `widths` are keyed by column id and `cell_attrs` by slot id.
//! A key the table does not name rides untouched.

use crate::ids::{Minter, is_id, prefix};
use crate::json::canonicalize_keys;
use crate::lines::model as line;
use crate::model::{
    Anchor, Block, BlockKind, Content, HARD_BREAK, Invariant, Island, IslandType, Mark, MarkKind,
    NOTE_SLOT,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The props keys a table names.
pub const TABLE_KEYS: [&str; 7] = [
    "columns",
    "rows",
    "head",
    "aligns",
    "widths",
    "align",
    "cell_attrs",
];

/// The keys the line form's table names.
const LINE_TABLE_KEYS: [&str; 7] = [
    "header", "rows", "aligns", "widths", "align", "head", "headless",
];

/// The values of a table's `align` key, its placement, and of a cell's, its
/// horizontal alignment.
pub const TABLE_ALIGNS: [&str; 3] = ["left", "center", "right"];

/// The values of a table cell's `valign` key, its vertical alignment.
pub const CELL_VALIGNS: [&str; 3] = ["top", "middle", "bottom"];

/// A table cell's alignment keys, each with its values.
pub const CELL_KEYS: [(&str, &[&str]); 2] = [("align", &TABLE_ALIGNS), ("valign", &CELL_VALIGNS)];

/// A table cell's span keys: the columns and the rows it covers, each a whole
/// number from 1, absent at 1.
pub const CELL_SPANS: [&str; 2] = ["colspan", "rowspan"];

/// The largest column weight, JavaScript's `Number.MAX_SAFE_INTEGER`: the
/// WASM binding hands no larger integer to JavaScript.
pub const MAX_WEIGHT: u64 = (1 << 53) - 1;

/// Whether `n` is a column weight, `1..=`[`MAX_WEIGHT`].
pub fn is_weight(n: u64) -> bool {
    (1..=MAX_WEIGHT).contains(&n)
}

/// A table's layout as [`crate::view::layout`] spells its props, each key at
/// its default where absent or outside its set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLayout {
    /// A [weight](is_weight), or `None` for an auto-fit column, per column.
    pub widths: Option<Vec<Option<u64>>>,
    /// The table's placement, one of [`TABLE_ALIGNS`].
    pub align: Option<&'static str>,
    /// How many rows from the top of `header` then `rows` are header rows,
    /// at most every row: `head` where it holds a whole number, else `1`.
    pub head: usize,
}

impl Default for TableLayout {
    fn default() -> Self {
        TableLayout {
            widths: None,
            align: None,
            head: 1,
        }
    }
}

impl TableLayout {
    pub fn of(props: &Value) -> Self {
        let widths = props
            .get("widths")
            .and_then(Value::as_array)
            .and_then(|ws| {
                ws.iter()
                    .map(|w| match w {
                        Value::Null => Some(None),
                        w => w.as_u64().filter(|n| is_weight(*n)).map(Some),
                    })
                    .collect()
            });
        let head = props
            .get("head")
            .and_then(Value::as_u64)
            .map_or(1, saturate);
        TableLayout {
            widths,
            align: one_of(props.get("align"), &TABLE_ALIGNS),
            head: head.min(table_rows(props).len()),
        }
    }
}

fn saturate(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// A table cell's `colspan` and `rowspan` as it holds them, each `1` where the
/// cell holds no whole number from 1.
pub fn cell_span(cell: &Value) -> (usize, usize) {
    let read = |key: &str| {
        cell.get(key)
            .and_then(Value::as_u64)
            .filter(|n| *n >= 1)
            .map_or(1, saturate)
    };
    let [colspan, rowspan] = CELL_SPANS;
    (read(colspan), read(rowspan))
}

/// The rows of a table [`crate::view::layout`] spells, `header` first, each a
/// row's cells; a row that is no array holds none.
pub fn table_rows(props: &Value) -> Vec<&[Value]> {
    fn row(v: &Value) -> &[Value] {
        v.as_array().map_or(&[], Vec::as_slice)
    }
    let header = props.get("header").map_or(&[][..], row);
    let rows = props
        .get("rows")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    std::iter::once(header)
        .chain(rows.iter().map(row))
        .collect()
}

/// Per row of [`table_rows`] and per cell, the columns and rows a drawn cell
/// spans, or `None` for a cell another's span covers.
///
/// Cells claim slots in reading order, so an earlier span wins. A span takes
/// the columns to its right, then the rows below, while each slot it would
/// cover holds a cell with no text and no mark, a comment anchor included, that
/// no other span covers, stopping at the
/// grid's edge and at the end of its own section: the [`TableLayout::head`]
/// rows or the body. Its cell's own `colspan` and `rowspan` bound it.
pub fn table_spans(props: &Value) -> Vec<Vec<Option<(usize, usize)>>> {
    let rows = table_rows(props);
    let head = TableLayout::of(props).head;
    let mut covered: Vec<Vec<bool>> = rows.iter().map(|r| vec![false; r.len()]).collect();
    let mut spans: Vec<Vec<Option<(usize, usize)>>> =
        rows.iter().map(|r| vec![None; r.len()]).collect();
    let free = |covered: &[Vec<bool>], r: usize, c: usize| {
        let empty = rows[r].get(c).is_some_and(|cell| {
            let text = cell.get("text").and_then(Value::as_str).unwrap_or("");
            let marks = cell
                .get("marks")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            text.is_empty() && marks == 0
        });
        empty && !covered[r][c]
    };
    for r in 0..rows.len() {
        let end = if r < head { head } else { rows.len() };
        for c in 0..rows[r].len() {
            if covered[r][c] {
                continue;
            }
            let (colspan, rowspan) = cell_span(&rows[r][c]);
            let mut w = 1;
            while w < colspan && free(&covered, r, c + w) {
                w += 1;
            }
            let mut h = 1;
            while h < rowspan && r + h < end && (c..c + w).all(|x| free(&covered, r + h, x)) {
                h += 1;
            }
            for row in &mut covered[r..r + h] {
                row[c..c + w].iter_mut().for_each(|slot| *slot = true);
            }
            spans[r][c] = Some((w, h));
        }
    }
    spans
}

/// A table cell's `align` and `valign`, each `None` where the cell holds no
/// value in its set.
pub fn cell_alignment(cell: &Value) -> (Option<&'static str>, Option<&'static str>) {
    let [(align, aligns), (valign, valigns)] = CELL_KEYS;
    (
        one_of(cell.get(align), aligns),
        one_of(cell.get(valign), valigns),
    )
}

fn one_of(value: Option<&Value>, set: &[&'static str]) -> Option<&'static str> {
    let value = value?.as_str()?;
    set.iter().copied().find(|v| *v == value)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    /// The slot of each column, in column order.
    pub cells: Vec<String>,
}

impl Table {
    /// The grid `props` spells, or `None` where it spells none.
    pub fn read(props: &Value) -> Option<Table> {
        let columns: Vec<String> = props
            .get("columns")?
            .as_array()?
            .iter()
            .map(|c| c.as_str().map(str::to_string))
            .collect::<Option<_>>()?;
        let rows = props
            .get("rows")?
            .as_array()?
            .iter()
            .map(|r| {
                let id = r.get("id")?.as_str()?.to_string();
                let cells = r.get("cells")?.as_object()?;
                if cells.len() != columns.len() {
                    return None;
                }
                let cells = columns
                    .iter()
                    .map(|c| cells.get(c)?.as_str().map(str::to_string))
                    .collect::<Option<_>>()?;
                Some(Row { id, cells })
            })
            .collect::<Option<_>>()?;
        Some(Table { columns, rows })
    }

    pub fn slot_order(&self) -> Vec<String> {
        self.rows
            .iter()
            .flat_map(|r| r.cells.iter().cloned())
            .collect()
    }

    /// The slot at `(row, column)`.
    pub fn slot(&self, row: usize, column: usize) -> Option<&str> {
        self.rows.get(row)?.cells.get(column).map(String::as_str)
    }

    /// Writes `columns` and `rows` into `props`.
    pub fn write_into(&self, props: &mut Map<String, Value>) {
        props.insert(
            "columns".into(),
            self.columns.iter().cloned().map(Value::String).collect(),
        );
        props.insert(
            "rows".into(),
            self.rows
                .iter()
                .map(|r| {
                    let cells: Map<String, Value> = self
                        .columns
                        .iter()
                        .zip(&r.cells)
                        .map(|(c, s)| (c.clone(), Value::String(s.clone())))
                        .collect();
                    let mut row = Map::new();
                    row.insert("cells".into(), Value::Object(cells));
                    row.insert("id".into(), Value::String(r.id.clone()));
                    Value::Object(row)
                })
                .collect(),
        );
    }
}

/// Renames a table's column, row and slot ids through the maps, an id a map
/// does not name kept, and every key that names one with it.
pub(crate) fn rename(
    island: &mut Island,
    columns: &HashMap<String, String>,
    rows: &HashMap<String, String>,
    slots: &HashMap<String, String>,
) {
    let Some(mut table) = Table::read(&island.props) else {
        return;
    };
    let to =
        |m: &HashMap<String, String>, id: &String| m.get(id).cloned().unwrap_or_else(|| id.clone());
    table.columns = table.columns.iter().map(|c| to(columns, c)).collect();
    for row in &mut table.rows {
        row.id = to(rows, &row.id);
        row.cells = row.cells.iter().map(|s| to(slots, s)).collect();
    }
    let Some(props) = island.props.as_object_mut() else {
        return;
    };
    table.write_into(props);
    for (key, map) in [
        ("aligns", columns),
        ("widths", columns),
        ("cell_attrs", slots),
    ] {
        if let Some(Value::Object(keyed)) = props.get_mut(key) {
            *keyed = std::mem::take(keyed)
                .into_iter()
                .map(|(k, v)| (to(map, &k), v))
                .collect();
        }
    }
    island.slots = std::mem::take(&mut island.slots)
        .into_iter()
        .map(|(k, v)| (to(slots, &k), v))
        .collect();
}

pub(crate) fn check(island: &Island) -> Result<(), Invariant> {
    let bad = |reason| Invariant::BadIsland {
        id: island.id.clone(),
        reason,
    };
    let Some(table) = Table::read(&island.props) else {
        return Err(bad("a table's `columns` and `rows` do not spell a grid"));
    };
    if table.columns.is_empty() || table.rows.is_empty() {
        return Err(bad("a table holds at least one row and one column"));
    }
    let cells = table.columns.len().saturating_mul(table.rows.len());
    if cells > crate::MAX_TABLE_CELLS {
        return Err(Invariant::TableTooLarge {
            cells,
            max: crate::MAX_TABLE_CELLS,
        });
    }
    let mut ids = HashSet::new();
    for id in &table.columns {
        if !is_id(id) {
            return Err(Invariant::BadId { id: id.clone() });
        }
        if !ids.insert(id.as_str()) {
            return Err(Invariant::DuplicateId { id: id.clone() });
        }
    }
    let mut rows = HashSet::new();
    let mut slots = HashSet::new();
    for row in &table.rows {
        if !is_id(&row.id) {
            return Err(Invariant::BadId { id: row.id.clone() });
        }
        if !rows.insert(row.id.as_str()) {
            return Err(Invariant::DuplicateId { id: row.id.clone() });
        }
        for slot in &row.cells {
            if !slots.insert(slot.as_str()) {
                return Err(Invariant::DuplicateId { id: slot.clone() });
            }
        }
    }
    if slots.len() != island.slots.len() || !island.slots.keys().all(|k| slots.contains(k.as_str()))
    {
        return Err(bad("a table's slots are not the ones its rows name"));
    }
    Ok(())
}

/// Settle an island's props: a table's layout as the line form settles it, an
/// equation's TeX, and every key sorted.
pub(crate) fn normalize_island(island: &mut Island) {
    match island.island_type {
        IslandType::Table => normalize_table(island),
        IslandType::Math => IslandType::Math.normalize_props(&mut island.props),
        IslandType::Footnote | IslandType::Image | IslandType::Ext(_) => {}
    }
    canonicalize_keys(&mut island.props);
}

fn normalize_table(island: &mut Island) {
    let Some(table) = Table::read(&island.props) else {
        return;
    };
    let mut lined = to_line_props(island);
    IslandType::Table.normalize_props(&mut lined);
    let mut props = Map::new();
    if let Some(obj) = island.props.as_object() {
        for (k, v) in obj {
            if !TABLE_KEYS.contains(&k.as_str()) {
                props.insert(k.clone(), v.clone());
            }
        }
    }
    table.write_into(&mut props);
    layout_from_line(&lined, &table, &mut props);
    island.props = Value::Object(props);
}

/// The layout keys of a line-form table, keyed onto `table`'s ids.
fn layout_from_line(lined: &Value, table: &Table, props: &mut Map<String, Value>) {
    if let Some(head) = lined.get("head").filter(|h| h.as_u64() != Some(1)) {
        props.insert("head".into(), head.clone());
    }
    if let Some(align) = lined.get("align") {
        props.insert("align".into(), align.clone());
    }
    let keyed = |key: &str, skip: &dyn Fn(&Value) -> bool| -> Option<Value> {
        let arr = lined.get(key)?.as_array()?;
        let map: Map<String, Value> = table
            .columns
            .iter()
            .zip(arr)
            .filter(|(_, v)| !skip(v))
            .map(|(c, v)| (c.clone(), v.clone()))
            .collect();
        (!map.is_empty()).then_some(Value::Object(map))
    };
    if let Some(aligns) = keyed("aligns", &|v| v.as_str() == Some("none")) {
        props.insert("aligns".into(), aligns);
    }
    if let Some(widths) = keyed("widths", &Value::is_null) {
        props.insert("widths".into(), widths);
    }
    let mut attrs = Map::new();
    for (r, row) in table_rows(lined).into_iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let Some(slot) = table.slot(r, c) else {
                continue;
            };
            let Some(obj) = cell.as_object() else {
                continue;
            };
            let rest: Map<String, Value> = obj
                .iter()
                .filter(|(k, _)| *k != "text" && *k != "marks")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if !rest.is_empty() {
                attrs.insert(slot.to_string(), Value::Object(rest));
            }
        }
    }
    if !attrs.is_empty() {
        props.insert("cell_attrs".into(), Value::Object(attrs));
    }
}

/// The island's props as the line form spells them: a table's cells and a
/// note's text inline, anchors as anchor marks, and an `x-` island's props
/// beside its slots as cells.
pub(crate) fn to_line_props(island: &Island) -> Value {
    match island.island_type {
        IslandType::Table => {
            let Some(table) = Table::read(&island.props) else {
                return island.props.clone();
            };
            let Some((header, body)) = table.rows.split_first() else {
                return island.props.clone();
            };
            let mut out = Map::new();
            let obj = island.props.as_object().cloned().unwrap_or_default();
            for (k, v) in &obj {
                if !TABLE_KEYS.contains(&k.as_str()) {
                    out.insert(k.clone(), v.clone());
                }
            }
            let attrs = obj.get("cell_attrs").and_then(Value::as_object);
            let cell = |slot: &str| {
                let content = island.slots.get(slot).cloned().unwrap_or_default();
                let mut cell = match attrs.and_then(|a| a.get(slot)).and_then(Value::as_object) {
                    Some(a) => a.clone(),
                    None => Map::new(),
                };
                let (text, marks) = slot_to_cell(&content);
                cell.insert(
                    "marks".into(),
                    marks
                        .iter()
                        .map(crate::lines::serial::mark_to_value)
                        .collect(),
                );
                cell.insert("text".into(), Value::String(text));
                Value::Object(cell)
            };
            let row = |r: &Row| Value::Array(r.cells.iter().map(|s| cell(s)).collect());
            out.insert("header".into(), row(header));
            out.insert("rows".into(), body.iter().map(row).collect());
            let by_column = |key: &str, default: Value| -> Option<Value> {
                let map = obj.get(key)?.as_object()?;
                Some(
                    table
                        .columns
                        .iter()
                        .map(|c| map.get(c).cloned().unwrap_or_else(|| default.clone()))
                        .collect(),
                )
            };
            out.insert(
                "aligns".into(),
                by_column("aligns", Value::String("none".into())).unwrap_or_else(|| {
                    table
                        .columns
                        .iter()
                        .map(|_| Value::String("none".into()))
                        .collect()
                }),
            );
            if let Some(widths) = by_column("widths", Value::Null) {
                out.insert("widths".into(), widths);
            }
            if let Some(align) = obj.get("align") {
                out.insert("align".into(), align.clone());
            }
            if let Some(head) = obj.get("head") {
                if head.as_u64() == Some(0) {
                    out.insert("headless".into(), Value::Bool(true));
                }
                out.insert("head".into(), head.clone());
            }
            Value::Object(out)
        }
        IslandType::Footnote => {
            let mut out = island.props.as_object().cloned().unwrap_or_default();
            let note = island.slots.get(NOTE_SLOT).cloned().unwrap_or_default();
            let (text, marks) = slot_to_cell(&note);
            out.insert(
                "marks".into(),
                marks
                    .iter()
                    .map(crate::lines::serial::mark_to_value)
                    .collect(),
            );
            out.insert("text".into(), Value::String(text));
            Value::Object(out)
        }
        IslandType::Ext(_) => {
            let cells: Map<String, Value> = island
                .slots
                .iter()
                .map(|(id, slot)| {
                    let (text, marks) = slot_to_cell(slot);
                    let marks = marks
                        .iter()
                        .map(crate::lines::serial::mark_to_value)
                        .collect();
                    (
                        id.clone(),
                        Value::Object(Map::from_iter([
                            ("marks".into(), marks),
                            ("text".into(), Value::String(text)),
                        ])),
                    )
                })
                .collect();
            Value::Object(Map::from_iter([
                ("props".into(), island.props.clone()),
                ("slots".into(), Value::Object(cells)),
            ]))
        }
        IslandType::Image | IslandType::Math => island.props.clone(),
    }
}

/// A line-form island's props and the slots they hold, ids minted by
/// `minter` in reading order.
pub(crate) fn from_line_props(
    island_type: &IslandType,
    props: &Value,
    minter: &mut Minter,
) -> (Value, BTreeMap<String, Content>) {
    match island_type {
        IslandType::Table => {
            let grid = table_rows(props);
            let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
            let columns: Vec<String> = (0..cols).map(|_| minter.mint(prefix::COLUMN)).collect();
            let mut slots = BTreeMap::new();
            let mut rows = Vec::new();
            for r in &grid {
                let id = minter.mint(prefix::ROW);
                let mut cells = Vec::new();
                for c in 0..cols {
                    let slot = minter.mint(prefix::SLOT);
                    let (text, marks) = r
                        .get(c)
                        .map(crate::lines::serial::parse_cell)
                        .unwrap_or_default();
                    slots.insert(slot.clone(), cell_to_slot(&text, &marks, false));
                    cells.push(slot);
                }
                rows.push(Row { id, cells });
            }
            let table = Table { columns, rows };
            let mut out = Map::new();
            if let Some(obj) = props.as_object() {
                for (k, v) in obj {
                    if !LINE_TABLE_KEYS.contains(&k.as_str()) {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            table.write_into(&mut out);
            let mut lined = props.clone();
            if let Some(obj) = lined.as_object_mut() {
                obj.remove("headless");
            }
            layout_from_line(&lined, &table, &mut out);
            let mut out = Value::Object(out);
            canonicalize_keys(&mut out);
            (out, slots)
        }
        IslandType::Footnote => {
            let (text, marks) = crate::lines::serial::parse_cell(props);
            let mut out = props.as_object().cloned().unwrap_or_default();
            out.remove("text");
            out.remove("marks");
            let mut slots = BTreeMap::new();
            slots.insert(NOTE_SLOT.to_string(), cell_to_slot(&text, &marks, false));
            (Value::Object(out), slots)
        }
        IslandType::Ext(_) => {
            let slots = match props.get("slots") {
                Some(Value::Object(cells)) => cells
                    .iter()
                    .map(|(id, cell)| {
                        let (text, marks) = crate::lines::serial::parse_cell(cell);
                        (id.clone(), cell_to_slot(&text, &marks, true))
                    })
                    .collect(),
                _ => BTreeMap::new(),
            };
            (
                props
                    .get("props")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Map::new())),
                slots,
            )
        }
        IslandType::Image | IslandType::Math => (props.clone(), BTreeMap::new()),
    }
}

/// A slot as a line-form cell: its paragraphs joined by a blank break, each
/// hard break a `\n`, and its anchors as anchor marks.
pub(crate) fn slot_to_cell(slot: &Content) -> (String, Vec<line::Mark>) {
    let mut text = String::with_capacity(slot.text.len());
    let mut shift = Vec::with_capacity(slot.text.len() + 1);
    let mut extra = 0usize;
    for c in slot.text.chars() {
        shift.push(extra);
        match c {
            '\n' => {
                text.push_str("\n\n");
                extra += 1;
            }
            HARD_BREAK => text.push('\n'),
            c => text.push(c),
        }
    }
    shift.push(extra);
    let at = |p: usize| p + shift[p.min(shift.len() - 1)];
    let mut marks: Vec<line::Mark> = slot
        .marks
        .iter()
        .filter_map(|m| Some(line::Mark::new(at(m.start), at(m.end), m.kind.to_line()?)))
        .collect();
    marks.extend(slot.anchors.iter().map(|a| {
        line::Mark::new(
            at(a.start),
            at(a.end),
            line::MarkKind::Anchor { id: a.id.clone() },
        )
    }));
    let chars: Vec<char> = text.chars().collect();
    let marks = line::split_marks_at_newlines(&chars, marks);
    (text, line::normalize_marks(marks))
}

/// A line-form cell as a slot: each `\n\n` a paragraph break where the slot
/// holds `paragraphs`, a hard break where it is one paragraph, and each other
/// `\n` a hard break.
pub(crate) fn cell_to_slot(text: &str, marks: &[line::Mark], paragraphs: bool) -> Content {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut shift = Vec::with_capacity(chars.len() + 1);
    let mut dropped = 0usize;
    let mut blocks = 1usize;
    let mut i = 0;
    while i < chars.len() {
        shift.push(dropped);
        match chars[i] {
            '\n' if chars.get(i + 1) == Some(&'\n') => {
                match paragraphs {
                    true => {
                        out.push('\n');
                        blocks += 1;
                    }
                    false => out.push(HARD_BREAK),
                }
                dropped += 1;
                shift.push(dropped);
                i += 2;
                continue;
            }
            '\n' => out.push(HARD_BREAK),
            c => out.push(c),
        }
        i += 1;
    }
    shift.push(dropped);
    let at = |p: usize| p - shift[p.min(shift.len() - 1)];
    let blocks = (0..blocks)
        .map(|n| Block::new(format!("{}{n}", prefix::BLOCK), BlockKind::Para))
        .collect();
    let mut slot = Content {
        text: out,
        blocks,
        ..Content::empty()
    };
    for m in marks {
        let (start, end) = (at(m.start), at(m.end));
        match &m.kind {
            line::MarkKind::Anchor { id } => slot.anchors.push(Anchor::new(id.clone(), start, end)),
            kind => {
                if let Some(kind) = MarkKind::from_line(kind) {
                    slot.marks.push(Mark::new(start, end, kind));
                }
            }
        }
    }
    slot.normalize();
    slot
}
