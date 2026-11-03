//! A line-form island's cells: a table cell, a footnote's note, each a
//! `{text, marks}` object inside the island's `props`, its marks spelled as a
//! mark object `{start, end, type, attrs}` with offsets in scalar values into
//! its text.

use crate::lines::model::{is_font_family_name, Mark, MarkKind, Scalar};
use serde_json::{Map, Value};
use std::borrow::Cow;

fn as_slice(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn arr_or_empty<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).map(as_slice).unwrap_or_default()
}

/// Write a payload bag into an encoder's own map, omitting an empty one.
fn insert_attrs(m: &mut Map<String, Value>, attrs: Cow<'_, Value>) {
    if !crate::lines::model::is_empty_bag(&attrs) {
        m.insert("attrs".into(), attrs.into_owned());
    }
}

/// Encode a [`Mark`] (`start`, `end`, `type`, …) into its canonical wire object.
pub fn mark_to_value(mark: &Mark) -> Value {
    let mut m = Map::new();
    insert_attrs(&mut m, mark.kind.attrs());
    m.insert("end".into(), Value::from(mark.end));
    m.insert("start".into(), Value::from(mark.start));
    m.insert("type".into(), Value::String(mark.kind.tag().to_string()));
    Value::Object(m)
}

/// A mark object as the mark it spells, `None` where it spells none this
/// build reads.
pub(crate) fn mark_from_value(v: &Value) -> Option<Mark> {
    let o = v.as_object()?;
    let pos = |key: &str| o.get(key).and_then(Value::as_u64).and_then(|n| Scalar::try_from(n).ok());
    let (start, end) = (pos("start")?, pos("end")?);
    let attr = |key: &str| o.get("attrs").and_then(|a| a.get(key));
    let kind = match o.get("type").and_then(Value::as_str)? {
        "strong" => MarkKind::Strong,
        "emph" => MarkKind::Emph,
        "underline" => MarkKind::Underline,
        "strike" => MarkKind::Strike,
        "code" => MarkKind::Code,
        "link" => MarkKind::Link { url: attr("url").and_then(Value::as_str).unwrap_or_default().to_string() },
        "font_family" => MarkKind::FontFamily {
            name: attr("name").and_then(Value::as_str).filter(|name| is_font_family_name(name))?.to_string(),
        },
        "anchor" => MarkKind::Anchor { id: attr("id").and_then(Value::as_str).unwrap_or_default().to_string() },
        _ => return None,
    };
    Some(Mark { start, end, kind })
}

/// Parse a table-cell object `{text, marks}` leniently: its plain text plus the
/// marks over it, their ranges scalar offsets into that text and their wire shape
/// the one prose marks use. A malformed mark is skipped rather than failing.
/// Public so the typst emitter renders a cell through the same parse the codecs
/// use.
pub fn parse_cell(v: &Value) -> (String, Vec<Mark>) {
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let marks = v
        .get("marks")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(mark_from_value).collect())
        .unwrap_or_default();
    (text, marks)
}

/// Build a table-cell object `{marks, text}`.
pub(crate) fn cell_to_value(text: &str, marks: &[Mark]) -> Value {
    let mut m = Map::new();
    m.insert(
        "marks".into(),
        Value::Array(marks.iter().map(mark_to_value).collect()),
    );
    m.insert("text".into(), Value::String(text.to_string()));
    Value::Object(m)
}

/// Every cell object in a table island's props, header then each body row: the
/// undecoded half of [`table_cells`], walking the same cells in the same order.
pub(crate) fn table_cell_values(props: &Value) -> impl Iterator<Item = &Value> {
    let header = arr_or_empty(props, "header").iter();
    let rows = arr_or_empty(props, "rows")
        .iter()
        .flat_map(|row| as_slice(row).iter());
    header.chain(rows)
}

/// [`table_cell_values`], mutable.
pub(crate) fn table_cell_values_mut(props: &mut Value) -> Vec<&mut Value> {
    let Some(o) = props.as_object_mut() else {
        return Vec::new();
    };
    let mut cells = Vec::new();
    let (mut header, mut rows) = (None, None);
    for (key, value) in o.iter_mut() {
        match key.as_str() {
            "header" => header = Some(value),
            "rows" => rows = Some(value),
            _ => {}
        }
    }
    if let Some(Value::Array(header)) = header {
        cells.extend(header.iter_mut());
    }
    if let Some(Value::Array(rows)) = rows {
        for row in rows.iter_mut() {
            if let Value::Array(row) = row {
                cells.extend(row.iter_mut());
            }
        }
    }
    cells
}

/// Set a cell object's marks, its text and every other key kept.
pub(crate) fn set_cell_marks(cell: &mut Value, marks: &[Mark]) {
    let (text, _) = parse_cell(cell);
    match (cell.as_object_mut(), cell_to_value(&text, marks)) {
        (Some(o), Value::Object(fields)) => o.extend(fields),
        (_, canon) => *cell = canon,
    }
}

/// Every cell's `(text, marks)` in a table island's props: header then each
/// body row, in order. For [`Content::validate`]'s cell-mark invariant checks.
pub(crate) fn table_cells(props: &Value) -> Vec<(String, Vec<Mark>)> {
    table_cell_values(props).map(parse_cell).collect()
}

/// Repair a table island's props in place to the canonical shape:
///
/// - **One column count.** `cols` is the widest of the header, any body row, and
///   `aligns`; the header, each row, and `aligns` are padded up to it (padding
///   only grows: no cell is ever truncated). Materializing the count into the
///   header means the markdown projection (header-derived) and the Typst
///   projection (widest-row) agree on one number.
/// - **One break char.** A `\n` in a cell's text is a line break; a `\r` or a
///   [`crate::lines::normalize::is_line_separator`] char becomes a space. A 1:1
///   replacement keeps char offsets stable, so the cell's marks stay in range.
/// - **Canonical cell marks.** Each cell's marks are re-normalized (formatting
///   edges trimmed off `\n` as prose's are, sort, same-kind union, drop
///   zero-width) so equal cells serialize to equal bytes.
/// - **Arrays where arrays belong.** A present non-array `header`, `aligns`, or
///   row carries no cells, so it becomes an empty array.
/// - **Layout keys absent at their default.** See [`normalize_table_layout`].
/// - **Spans that cover only what they can.** See [`normalize_table_spans`].
pub(crate) fn normalize_table_props(props: &mut Value) {
    let cols = table_cols(props);
    let widths = crate::lines::island::TableLayout::of(props).widths;
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    let aligns = obj.entry("aligns").or_insert_with(|| Value::Array(vec![]));
    if !aligns.is_array() {
        *aligns = Value::Array(vec![]);
    }
    if let Some(a) = aligns.as_array_mut() {
        while a.len() < cols {
            a.push(Value::String("none".into()));
        }
    }
    let canon_row = |row: &mut Value| {
        pad_row(row, cols);
        if let Some(r) = row.as_array_mut() {
            r.iter_mut().for_each(canon_cell);
        }
    };
    let header = obj.entry("header").or_insert_with(|| Value::Array(vec![]));
    if !header.is_array() {
        *header = Value::Array(vec![]);
    }
    canon_row(header);
    if let Some(rows) = obj.get_mut("rows").and_then(Value::as_array_mut) {
        for row in rows.iter_mut() {
            if !row.is_array() {
                *row = Value::Array(vec![]);
            }
            canon_row(row);
        }
    }
    normalize_table_layout(obj, widths, cols);
    normalize_table_spans(props);
}

/// A table's layout keys, each absent at its default: `widths` holding only
/// weights and `null`s settles to `cols`, padded with `null` or truncated, and
/// all `null` is absent; a whole-number `head` settles to at most the row
/// count, absent at 1. `headless: true` sits beside `head: 0`, and is absent
/// beside any other `head`; a stored
/// `headless: true` settles to `head: 0` where `head` holds no whole number. A
/// value outside its set stays as written.
fn normalize_table_layout(obj: &mut Map<String, Value>, widths: Option<Vec<Option<u64>>>, cols: usize) {
    if let Some(mut widths) = widths {
        widths.resize(cols, None);
        match widths.iter().any(Option::is_some) {
            true => obj.insert("widths".into(), widths.into_iter().map(|w| w.map_or(Value::Null, Value::from)).collect()),
            false => obj.remove("widths"),
        };
    }
    let headless = obj.get("headless").and_then(Value::as_bool);
    if headless == Some(true) && obj.get("head").and_then(Value::as_u64).is_none() {
        obj.insert("head".into(), 0.into());
    }
    let rows = 1 + obj.get("rows").and_then(Value::as_array).map_or(0, Vec::len);
    let head = obj.get("head").and_then(Value::as_u64).map(|head| usize::try_from(head).unwrap_or(usize::MAX).min(rows));
    match head {
        Some(1) => obj.remove("head"),
        Some(head) => obj.insert("head".into(), head.into()),
        None => None,
    };
    match head {
        Some(0) => obj.insert("headless".into(), true.into()),
        _ if headless.is_some() => obj.remove("headless"),
        _ => None,
    };
}

/// Each drawn cell's `colspan` and `rowspan` settled to what it covers
/// ([`crate::lines::island::table_spans`]), absent at 1, and each covered cell's span
/// keys absent. A covered cell keeps its alignment, which it draws again once
/// no span covers it. A value outside its set stays as written.
fn normalize_table_spans(props: &mut Value) {
    use crate::lines::island::CELL_SPANS;
    let spans = crate::lines::island::table_spans(props);
    let settle = |row: &mut Value, spans: &[Option<(usize, usize)>]| {
        let cells = row.as_array_mut().into_iter().flatten().zip(spans);
        for (cell, span) in cells {
            let Some(cell) = cell.as_object_mut() else { continue };
            let settled = span.map_or([None, None], |(w, h)| [Some(w), Some(h)]);
            for (key, n) in CELL_SPANS.into_iter().zip(settled) {
                if !cell.get(key).and_then(Value::as_u64).is_some_and(|n| n >= 1) {
                    continue;
                }
                match n {
                    Some(n) if n > 1 => cell.insert(key.into(), n.into()),
                    _ => cell.remove(key),
                };
            }
        }
    };
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    let mut spans = spans.iter();
    if let (Some(header), Some(row)) = (obj.get_mut("header"), spans.next()) {
        settle(header, row);
    }
    if let Some(rows) = obj.get_mut("rows").and_then(Value::as_array_mut) {
        for (row, spans) in rows.iter_mut().zip(spans) {
            settle(row, spans);
        }
    }
}

/// A table's canonical column count: the widest of its header, any body row, and
/// its `aligns` array. Padding (never truncation) brings every part up to it.
fn table_cols(props: &Value) -> usize {
    let arr_len = |k: &str| props.get(k).and_then(Value::as_array).map(|a| a.len());
    let header = arr_len("header").unwrap_or(0);
    let aligns = arr_len("aligns").unwrap_or(0);
    let widest_row = props
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|r| r.as_array().map(|a| a.len()).unwrap_or(0))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    header.max(aligns).max(widest_row)
}

/// The slots a table's grid holds once padded: its rows, the header among
/// them, by its [column count](table_cols).
pub(crate) fn table_slots(props: &Value) -> usize {
    let rows = props.get("rows").and_then(Value::as_array).map_or(0, Vec::len);
    rows.saturating_add(1).saturating_mul(table_cols(props))
}

/// Pad a cell array (header or body row) up to `cols` with empty cells. Never
/// shrinks: `cols` is the widest, so a shorter array only grows.
fn pad_row(v: &mut Value, cols: usize) {
    if let Some(arr) = v.as_array_mut() {
        while arr.len() < cols {
            arr.push(cell_to_value("", &[]));
        }
    }
}

/// Every char a downstream lexer reads as a line break other than `\n`, the
/// separators [`crate::lines::normalize::is_line_separator`] names included. A cell's
/// one line break is `\n`.
fn is_cell_break(c: char) -> bool {
    c == '\r' || crate::lines::normalize::is_line_separator(c)
}

/// Space a cell's stray line-break chars (1:1, so mark offsets hold) and
/// re-normalize its marks. Writes back into the cell's **own** object rather
/// than minting a fresh one, so a key this build does not recognize survives.
pub(crate) fn canon_cell(cell: &mut Value) {
    let (text, mut marks) = parse_cell(cell);
    let text = if text.contains(is_cell_break) {
        text.replace(is_cell_break, " ")
    } else {
        text
    };
    if text.contains('\n') {
        // Union before the trim: the pieces a sweep reopens across a `\n` meet
        // at it, and trimmed first they would part there.
        marks = crate::lines::model::normalize_marks(marks);
        let chars: Vec<char> = text.chars().collect();
        crate::lines::model::trim_marks_off_newlines(&chars, &mut marks);
    }
    let canon = cell_to_value(&text, &crate::lines::model::normalize_marks(marks));
    match (cell.as_object_mut(), canon) {
        (Some(o), Value::Object(fields)) => o.extend(fields),
        (_, canon) => *cell = canon,
    }
}

/// Repair a footnote's props in place: [`canon_cell`], then the note's edge
/// line breaks dropped, a note opening or closing on one having nothing there
/// to break.
pub(crate) fn normalize_note_props(props: &mut Value) {
    canon_cell(props);
    let (text, marks) = parse_cell(props);
    if !text.starts_with('\n') && !text.ends_with('\n') {
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    let lead = chars.iter().take_while(|&&c| c == '\n').count();
    let end = lead.max(chars.len() - chars.iter().rev().take_while(|&&c| c == '\n').count());
    let clip = |at: Scalar| at.clamp(lead, end) - lead;
    let marks: Vec<Mark> = marks
        .into_iter()
        .map(|m| Mark::new(clip(m.start), clip(m.end), m.kind))
        .collect();
    let text: String = chars[lead..end].iter().collect();
    if let (Some(o), Value::Object(fields)) =
        (props.as_object_mut(), cell_to_value(&text, &crate::lines::model::normalize_marks(marks)))
    {
        o.extend(fields);
    }
}

/// Repair an equation's props in place: `tex` settled by [`settle_tex`], and
/// `display: false` absent. A `tex` that is no string rides as written.
pub(crate) fn normalize_math_props(props: &mut Value) {
    let Some(o) = props.as_object_mut() else {
        return;
    };
    if let Some(Value::String(tex)) = o.get_mut("tex") {
        let settled = settle_tex(tex);
        if settled != *tex {
            *tex = settled;
        }
    }
    if o.get("display") == Some(&Value::Bool(false)) {
        o.remove("display");
    }
}

/// TeX source as an equation stores it: each `%` comment dropped to the end
/// of its line, each line break or other control char a space, which TeX
/// reads it as, and the whitespace at either edge dropped.
pub fn settle_tex(tex: &str) -> String {
    let mut uncommented = String::with_capacity(tex.len());
    let (mut comment, mut escaped) = (false, false);
    for c in tex.chars() {
        let ends_line = c == '\n' || c == '\r' || crate::lines::normalize::is_line_separator(c);
        comment &= !ends_line;
        if !comment && c == '%' && !escaped {
            comment = true;
        }
        if !comment {
            uncommented.push(c);
        }
        escaped = c == '\\' && !escaped;
    }
    let spaced: String = uncommented
        .chars()
        .map(|c| if c.is_control() || crate::lines::normalize::is_line_separator(c) { ' ' } else { c })
        .filter(|&c| c != crate::lines::model::ISLAND_SLOT)
        .collect();
    spaced.trim().to_string()
}
