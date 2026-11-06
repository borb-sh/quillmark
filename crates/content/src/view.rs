//! A content as its renderers read it: one [`Line`] per run of text between
//! block and hard breaks, each carrying the block role it writes and the
//! container path around it, with the item ordinals and the run parity that
//! tells two adjacent lists or quotes apart, which the content computes rather
//! than stores. The markdown export and the Typst emitter walk it.

use crate::lines::island::IslandType;
use crate::model::{
    BlockKind, ContainerKind, Content, HARD_BREAK, ISLAND_SLOT, Island, Mark, MarkKind, NOTE_SLOT,
    Scalar,
};
use crate::table::Table;
use std::collections::HashMap;

/// One cell of an island as its renderers read it: the slot's text, each
/// paragraph break a blank line and each hard break a `\n`, its marks but
/// its `x-` ones, and each anchor's id and start.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    pub marks: Vec<Mark>,
    pub anchors: Vec<(String, Scalar)>,
}

/// The island's props with each cell its text and marks inline, the shape
/// the table readers in [`crate::lines::island`] read its layout from.
pub fn layout(isl: &Island) -> serde_json::Value {
    crate::table::to_line_props(isl)
}

/// The first run at `depth` in `range`: the lines from its first holding a
/// container there, while each holds one in the same list or quote.
pub fn run(
    lines: &[Line],
    range: std::ops::Range<usize>,
    depth: usize,
) -> (&Level, std::ops::Range<usize>) {
    let end = range.end.min(lines.len());
    let start = range.start;
    let container = &lines[start].containers[depth];
    let mut j = start + 1;
    while j < end
        && lines[j]
            .containers
            .get(depth)
            .is_some_and(|c| c.same_run(container))
    {
        j += 1;
    }
    (container, start..j)
}

impl Level {
    /// Whether `other` sits in the same list or quote: one of its shape and
    /// instance.
    pub fn same_run(&self, other: &Level) -> bool {
        match (self, other) {
            (
                Level::ListItem {
                    ordered: a,
                    start: b,
                    instance: i,
                    ..
                },
                Level::ListItem {
                    ordered: c,
                    start: d,
                    instance: j,
                    ..
                },
            ) => (a, b, i) == (c, d, j),
            (Level::Quote { instance: i }, Level::Quote { instance: j }) => i == j,
            _ => false,
        }
    }
}

/// The content as markdown reads it: one [`Line`] per run of text between
/// block and hard breaks, the text with each hard break a `\n`, the marks
/// markdown writes, and the islands.
pub struct View<'a> {
    pub text: String,
    pub lines: Vec<Line>,
    /// The content's marks but its `x-` marks, which no renderer writes.
    pub marks: Vec<Mark>,
    pub islands: &'a [Island],
}

/// One line of a [`View`]: the block role it writes, the containers around
/// it, outermost first, and whether it continues the block above across a
/// hard break.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: LineKind,
    pub containers: Vec<Level>,
    pub continues: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    Para,
    Heading { level: u8 },
    Code { lang: Option<String> },
    Rule,
}

/// One container level of a line. `ordinal` counts the item within its list
/// from 0, and `instance` alternates between 0 and 1 across adjacent runs of
/// one shape, which markdown writes with alternating markers.
#[derive(Debug, Clone, PartialEq)]
pub enum Level {
    ListItem {
        ordered: bool,
        start: u64,
        ordinal: u64,
        checked: Option<bool>,
        instance: u64,
    },
    Quote {
        instance: u64,
    },
}

/// An open container run while the view walks the blocks: the list or quote,
/// the item holding the current line, and the ordinal and instance it writes.
struct Run<'c> {
    id: &'c str,
    item: Option<&'c str>,
    weld: Option<bool>,
    ordinal: u64,
    checked: Option<bool>,
    instance: u64,
}

/// A container a block's path opens: the list or quote id, the item id
/// within a list, its kind, and the item's `checked`.
type Opened<'c> = (&'c str, Option<&'c str>, &'c ContainerKind, Option<bool>);

impl<'a> View<'a> {
    pub fn of(c: &'a Content) -> Self {
        let text: String = c
            .text
            .chars()
            .map(|ch| if ch == HARD_BREAK { '\n' } else { ch })
            .collect();
        let kinds: HashMap<&str, &ContainerKind> = c
            .containers
            .iter()
            .map(|k| (k.id.as_str(), &k.kind))
            .collect();
        let mut lines = Vec::with_capacity(c.blocks.len());
        let mut state: Vec<Run> = Vec::new();
        for (b, seg) in c.blocks.iter().zip(c.text.split('\n')) {
            let mut ids = b.path.iter().map(String::as_str);
            let mut levels: Vec<Opened> = Vec::new();
            while let Some(id) = ids.next() {
                match kinds.get(id) {
                    Some(kind @ ContainerKind::List { .. }) => {
                        let Some(item) = ids.next() else { break };
                        let checked = match kinds.get(item) {
                            Some(ContainerKind::Item { checked }) => *checked,
                            _ => None,
                        };
                        levels.push((id, Some(item), kind, checked));
                    }
                    Some(kind @ ContainerKind::Quote) => levels.push((id, None, kind, None)),
                    _ => {}
                }
            }
            let mut opened_above = false;
            let mut path = Vec::with_capacity(levels.len());
            for (d, &(id, item, kind, checked)) in levels.iter().enumerate() {
                let weld = match kind {
                    ContainerKind::List { ordered, .. } => Some(*ordered),
                    _ => None,
                };
                if !opened_above && state.get(d).is_some_and(|r| r.id == id) {
                    let run = &mut state[d];
                    if run.item != item {
                        run.ordinal += 1;
                        run.item = item;
                        run.checked = checked;
                        state.truncate(d + 1);
                        opened_above = true;
                    }
                } else {
                    let instance = match state.get(d) {
                        Some(prev) if !opened_above && prev.weld == weld => 1 - prev.instance,
                        _ => 0,
                    };
                    state.truncate(d);
                    state.push(Run {
                        id,
                        item,
                        weld,
                        ordinal: 0,
                        checked,
                        instance,
                    });
                    opened_above = true;
                }
                let run = &state[d];
                path.push(match kind {
                    ContainerKind::List { ordered, start } => Level::ListItem {
                        ordered: *ordered,
                        start: *start,
                        ordinal: run.ordinal,
                        checked: run.checked,
                        instance: run.instance,
                    },
                    _ => Level::Quote {
                        instance: run.instance,
                    },
                });
            }
            state.truncate(levels.len());
            let kind = match &b.kind {
                BlockKind::Heading { level } => LineKind::Heading { level: *level },
                BlockKind::Code { lang } => LineKind::Code { lang: lang.clone() },
                BlockKind::Rule => LineKind::Rule,
                BlockKind::Para | BlockKind::Ext { .. } => LineKind::Para,
            };
            let breaks = seg.chars().filter(|&ch| ch == HARD_BREAK).count();
            let continues = matches!(kind, LineKind::Para | LineKind::Code { .. });
            for n in 0..=breaks {
                lines.push(Line {
                    kind: kind.clone(),
                    containers: path.clone(),
                    continues: n > 0 && continues,
                });
            }
        }
        View {
            text,
            lines,
            marks: c
                .marks
                .iter()
                .filter(|m| !matches!(m.kind, MarkKind::Ext { .. }))
                .cloned()
                .collect(),
            islands: &c.islands,
        }
    }
}

/// An island's cells as markdown writes them, in reading order: a table's
/// row by row from its header, a footnote's note, an `x-` island's slots by
/// id. Each is its text, each paragraph break a blank line and each hard
/// break a `\n`, and the marks and anchors over it.
pub fn cells(isl: &Island) -> Vec<Cell> {
    let slots: Vec<&Content> = match isl.island_type {
        IslandType::Table => Table::read(&isl.props)
            .map(|t| {
                t.slot_order()
                    .iter()
                    .filter_map(|s| isl.slots.get(s))
                    .collect()
            })
            .unwrap_or_default(),
        IslandType::Footnote => isl.slots.get(NOTE_SLOT).into_iter().collect(),
        IslandType::Ext(_) => isl.slots.values().collect(),
        IslandType::Image | IslandType::Math => Vec::new(),
    };
    slots.into_iter().map(cell).collect()
}

/// A slot as a [`Cell`].
pub fn cell(slot: &Content) -> Cell {
    let (text, line_marks) = crate::table::slot_to_cell(slot);
    let mut marks = Vec::new();
    let mut anchors = Vec::new();
    for m in line_marks {
        match &m.kind {
            crate::lines::model::MarkKind::Anchor { id } => anchors.push((id.clone(), m.start)),
            kind => marks.extend(MarkKind::from_line(kind).map(|k| Mark::new(m.start, m.end, k))),
        }
    }
    Cell {
        text,
        marks,
        anchors,
    }
}
/// One line's char range `[start, end)` into the view's text, with the
/// matching byte range and the count of island slots before the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub slots_before: usize,
}
/// Per-line char/byte ranges and slot prefixes over a view, in line order.
pub fn line_segments(rt: &View) -> Vec<Segment> {
    let mut segs = Vec::with_capacity(rt.lines.len());
    let (mut start, mut byte_start, mut slots_before, mut line_slots, mut pos) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for (b, c) in rt.text.char_indices() {
        if c == '\n' {
            segs.push(Segment {
                start,
                end: pos,
                byte_start,
                byte_end: b,
                slots_before,
            });
            start = pos + 1;
            byte_start = b + 1;
            slots_before += line_slots;
            line_slots = 0;
        } else if c == ISLAND_SLOT {
            line_slots += 1;
        }
        pos += 1;
    }
    segs.push(Segment {
        start,
        end: pos,
        byte_start,
        byte_end: rt.text.len(),
        slots_before,
    });
    let total_slots = slots_before + line_slots;
    while segs.len() < rt.lines.len() {
        segs.push(Segment {
            start: pos,
            end: pos,
            byte_start: rt.text.len(),
            byte_end: rt.text.len(),
            slots_before: total_slots,
        });
    }
    segs
}

/// The lines of `range` from its first, up to the first at `depth` that
/// starts a block of its own: one leaf block and its continuations.
pub fn segment(
    lines: &[Line],
    range: std::ops::Range<usize>,
    depth: usize,
) -> std::ops::Range<usize> {
    let end = range.end.min(lines.len());
    let mut j = (range.start + 1).min(end);
    while j < end && lines[j].containers.len() == depth && lines[j].continues {
        j += 1;
    }
    range.start..j
}

/// The first item at `depth` in `range`: the lines from its first holding a
/// container there, while each holds the same one.
pub fn item(
    lines: &[Line],
    range: std::ops::Range<usize>,
    depth: usize,
) -> (&Level, std::ops::Range<usize>) {
    let end = range.end.min(lines.len());
    let start = range.start;
    let container = &lines[start].containers[depth];
    let mut j = start + 1;
    while j < end && lines[j].containers.get(depth) == Some(container) {
        j += 1;
    }
    (container, start..j)
}

/// The text with each hard break a `\n` and island slots removed: what a
/// form field holds of a value, every mark and island dropped.
pub fn plaintext(c: &Content) -> String {
    c.text
        .chars()
        .filter(|&ch| ch != ISLAND_SLOT)
        .map(|ch| if ch == HARD_BREAK { '\n' } else { ch })
        .collect()
}
