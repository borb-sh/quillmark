//! A table island's props: `columns` (column ids), `rows` (`{id, cells}`, each
//! cell a column id mapped to the slot holding the cell's content) and the
//! layout keys `head`, `aligns`, `widths`, `align` and `cell_attrs`. `rows[0]`
//! is the header row; `head` counts header rows from the top, absent at 1;
//! `aligns` and `widths` are keyed by column id and `cell_attrs` by slot id.
//! A key the table does not name rides untouched.

use crate::ids::{Minter, is_id, prefix};
use crate::lines::island::IslandType;
use crate::lines::model::{self as line, canonicalize_keys};
use crate::model::{Anchor, Block, BlockKind, Content, HARD_BREAK, Invariant, Island, Mark, MarkKind, NOTE_SLOT};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

/// The props keys a table names.
pub const TABLE_KEYS: [&str; 7] = ["columns", "rows", "head", "aligns", "widths", "align", "cell_attrs"];

/// The keys the line form's table names.
const LINE_TABLE_KEYS: [&str; 7] = ["header", "rows", "aligns", "widths", "align", "head", "headless"];

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
        self.rows.iter().flat_map(|r| r.cells.iter().cloned()).collect()
    }

    /// The slot at `(row, column)`.
    pub fn slot(&self, row: usize, column: usize) -> Option<&str> {
        self.rows.get(row)?.cells.get(column).map(String::as_str)
    }

    /// Writes `columns` and `rows` into `props`.
    pub fn write_into(&self, props: &mut Map<String, Value>) {
        props.insert("columns".into(), self.columns.iter().cloned().map(Value::String).collect());
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

pub(crate) fn check(island: &Island) -> Result<(), Invariant> {
    let bad = |reason| Invariant::BadIsland { id: island.id.clone(), reason };
    let Some(table) = Table::read(&island.props) else {
        return Err(bad("a table's `columns` and `rows` do not spell a grid"));
    };
    if table.columns.is_empty() || table.rows.is_empty() {
        return Err(bad("a table holds at least one row and one column"));
    }
    let cells = table.columns.len().saturating_mul(table.rows.len());
    if cells > crate::MAX_TABLE_CELLS {
        return Err(Invariant::TableTooLarge { cells, max: crate::MAX_TABLE_CELLS });
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
    if slots.len() != island.slots.len() || !island.slots.keys().all(|k| slots.contains(k.as_str())) {
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
        IslandType::Footnote | IslandType::Image => {}
    }
    canonicalize_keys(&mut island.props);
}

fn normalize_table(island: &mut Island) {
    let Some(table) = Table::read(&island.props) else { return };
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
    for (r, row) in crate::lines::island::table_rows(lined).into_iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let Some(slot) = table.slot(r, c) else { continue };
            let Some(obj) = cell.as_object() else { continue };
            let rest: Map<String, Value> =
                obj.iter().filter(|(k, _)| *k != "text" && *k != "marks").map(|(k, v)| (k.clone(), v.clone())).collect();
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
/// note's text inline, anchors as anchor marks.
pub(crate) fn to_line_props(island: &Island) -> Value {
    match island.island_type {
        IslandType::Table => {
            let Some(table) = Table::read(&island.props) else { return island.props.clone() };
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
                cell.insert("marks".into(), marks.iter().map(crate::lines::serial::mark_to_value).collect());
                cell.insert("text".into(), Value::String(text));
                Value::Object(cell)
            };
            let row = |r: &Row| Value::Array(r.cells.iter().map(|s| cell(s)).collect());
            out.insert("header".into(), row(&table.rows[0]));
            out.insert("rows".into(), table.rows[1..].iter().map(row).collect());
            let by_column = |key: &str, default: Value| -> Option<Value> {
                let map = obj.get(key)?.as_object()?;
                Some(table.columns.iter().map(|c| map.get(c).cloned().unwrap_or_else(|| default.clone())).collect())
            };
            out.insert(
                "aligns".into(),
                by_column("aligns", Value::String("none".into()))
                    .unwrap_or_else(|| table.columns.iter().map(|_| Value::String("none".into())).collect()),
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
            out.insert("marks".into(), marks.iter().map(crate::lines::serial::mark_to_value).collect());
            out.insert("text".into(), Value::String(text));
            Value::Object(out)
        }
        IslandType::Image | IslandType::Math => island.props.clone(),
    }
}

/// A line-form island's props and the slots they hold, ids minted by
/// `minter` in reading order.
pub(crate) fn from_line_props(
    island_type: IslandType,
    props: &Value,
    minter: &mut Minter,
) -> (Value, BTreeMap<String, Content>) {
    match island_type {
        IslandType::Table => {
            let grid = crate::lines::island::table_rows(props);
            let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
            let columns: Vec<String> = (0..cols).map(|_| minter.mint(prefix::COLUMN)).collect();
            let mut slots = BTreeMap::new();
            let mut rows = Vec::new();
            for r in &grid {
                let id = minter.mint(prefix::ROW);
                let mut cells = Vec::new();
                for c in 0..cols {
                    let slot = minter.mint(prefix::SLOT);
                    let (text, marks) = r.get(c).map(crate::lines::serial::parse_cell).unwrap_or_default();
                    slots.insert(slot.clone(), cell_to_slot(&text, &marks));
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
            slots.insert(NOTE_SLOT.to_string(), cell_to_slot(&text, &marks));
            (Value::Object(out), slots)
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
    let mut marks: Vec<line::Mark> =
        slot.marks.iter().map(|m| line::Mark::new(at(m.start), at(m.end), m.kind.to_line())).collect();
    marks.extend(
        slot.anchors
            .iter()
            .map(|a| line::Mark::new(at(a.start), at(a.end), line::MarkKind::Anchor { id: a.id.clone() })),
    );
    let chars: Vec<char> = text.chars().collect();
    let marks = line::split_marks_at_newlines(&chars, marks);
    (text, line::normalize_marks(marks))
}

/// A line-form cell as a slot: each `\n\n` a paragraph break, each other
/// `\n` a hard break.
pub(crate) fn cell_to_slot(text: &str, marks: &[line::Mark]) -> Content {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut shift = Vec::with_capacity(chars.len() + 1);
    let mut dropped = 0usize;
    let mut paragraphs = 1usize;
    let mut i = 0;
    while i < chars.len() {
        shift.push(dropped);
        match chars[i] {
            '\n' if chars.get(i + 1) == Some(&'\n') => {
                out.push('\n');
                paragraphs += 1;
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
    let blocks = (0..paragraphs).map(|n| Block::new(format!("{}{n}", prefix::BLOCK), BlockKind::Para)).collect();
    let mut slot = Content { text: out, blocks, ..Content::empty() };
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
