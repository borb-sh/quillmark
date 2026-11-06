//! Markdown export: content → markdown, per island type.
//!
//! Anchors are **omitted**, surviving a revise through the diff that carries
//! them rather than the projection; [`to_markdown_annotated`] spells each at
//! its start as a read-only tag the import drops. An `x-` block writes as a
//! paragraph and an `x-` mark writes nothing. The contract is the **content
//! fixed point**: for a content `c` from [`crate::markdown::from_markdown`],
//! `from_markdown(to_markdown(c)) == c`. Markdown source is not canonical; the
//! content is.
//!
//! The export walks a per-line [`View`] of the content: each block's text
//! parted at its hard breaks into lines, each line carrying the container
//! path markdown spells, with the item ordinals and the run parity that tells
//! two adjacent lists or quotes apart, which the content computes rather than
//! stores.
//!
//! ## Export is defined by import
//!
//! The `render_marked_core` safety net settles a line's markdown by re-parsing
//! it and dropping marks until the text comes back intact. CommonMark's
//! emphasis algorithm (delimiter-run matching, the rule of 3, `\*`-escape
//! adjacency) has corners no local rule captures, and a local approximation
//! drops good marks on the cases it gets wrong; verifying against the parser is
//! the only check exactly as strict as the format. The cost is that an importer
//! change moves exporter output, and that every [`to_markdown`] call depends on
//! `pulldown_cmark`.
//!
//! ## Codec limits
//!
//! These shapes markdown cannot represent do **not** round-trip. An empty line
//! in a hard-break block drops, and so does an empty paragraph beside another
//! block, markdown spelling neither. An image `alt` loses its edge
//! whitespace: the parser trims alt *after* decoding the character reference
//! that carries an edge run everywhere else. A link or image `url` carrying a
//! line ending comes back percent-encoded (`%0A`/`%0D`): CommonMark admits none
//! in a destination, and the import decodes no percent escape. A `url` or `alt`
//! carrying a character the import does not admit there (a bidi control, a
//! line separator, a line ending in an `alt`) comes back rewritten. A link over
//! a footnote reference comes back as two links, one each side: link text reads
//! no reference. A mark crossing a link's edge writes as one piece outside and
//! one inside the link text. An equation whose `tex` its dollars cannot read
//! back as itself comes back as other markup. Each arises only from adversarial
//! delimiter/break placement, a hand-built island prop or mark, a stored value,
//! a merge of concurrent edits, or an editor's empty line. A `*`, `_` or `~~`
//! mark over edge whitespace writes over its text without it, since such a
//! delimiter never sits against whitespace. [`to_markdown_checked`] names each
//! loss where a content meets one.

use crate::lines::island::IslandType;
use crate::markdown::{AnchorRead, Annotated, Loss};
use crate::model::{
    BlockKind, ContainerKind, Content, HARD_BREAK, ISLAND_SLOT, Island, Mark, MarkKind, NOTE_SLOT,
    Scalar,
};
use crate::table::Table;
use std::collections::HashMap;

/// The markdown of `c`.
pub(crate) fn to_markdown(c: &Content) -> String {
    let rt = View::of(c);
    let segments = line_segments(&rt);
    let line_marks = line_marks(&rt, &segments);
    let labels = note_labels(&rt);
    project(&Ctx {
        rt: &rt,
        segments: &segments,
        line_marks: &line_marks,
        labels: &labels,
        tags: &[],
        tokens: &[],
    })
}

/// [`to_markdown`] read back through [`crate::markdown::from_markdown`] and
/// compared with `c`: each mark the markdown writes over other text or not at
/// all, each island that reads back otherwise, and [`Loss::Text`] first where
/// the text or its blocks do. Anchors and `x-` marks are not compared, the
/// projection carrying neither.
pub(crate) fn to_markdown_checked(c: &Content) -> (String, Vec<Loss>) {
    let markdown = to_markdown(c);
    let Ok(back) = crate::markdown::from_markdown(&markdown) else {
        return (markdown, vec![Loss::Text]);
    };
    let back = back.content;
    let (rt, read) = (View::of(c), View::of(&back));
    let mut losses = Vec::new();
    let same_text = read.text == rt.text;
    if !same_text || read.lines != rt.lines {
        losses.push(Loss::Text);
    }
    let to_stored = (!same_text).then(|| crate::lines::delta::diff(&read.text, &rt.text));
    let held: Vec<(Scalar, Scalar, &MarkKind)> = read
        .marks
        .iter()
        .map(|m| match &to_stored {
            Some(d) => (
                d.map_pos(m.start, crate::lines::delta::Assoc::After),
                d.map_pos(m.end, crate::lines::delta::Assoc::Before),
                &m.kind,
            ),
            None => (m.start, m.end, &m.kind),
        })
        .collect();
    for m in &rt.marks {
        if held
            .iter()
            .any(|&(s, e, k)| (s, e) == (m.start, m.end) && *k == m.kind)
        {
            continue;
        }
        let now = held
            .iter()
            .find(|&&(s, e, k)| *k == m.kind && s < m.end && e > m.start)
            .map(|&(s, e, _)| (s, e));
        losses.push(Loss::Mark {
            kind: m.kind.clone(),
            start: m.start,
            end: m.end,
            now,
        });
    }
    for (index, isl) in c.islands.iter().enumerate() {
        let kept = back.islands.get(index).is_some_and(|b| {
            b.island_type == isl.island_type && island_props(b) == island_props(isl)
        });
        if !kept {
            losses.push(Loss::Island { id: isl.id.clone() });
        }
    }
    (markdown, losses)
}

/// [`to_markdown`] with each prose anchor spelled
/// `<qm-anchor ref="ID"></qm-anchor>` at its `start`, and each island whose
/// markup does not [read back](island_reads_back) spelled as its token
/// `<qm-island ref="ID"></qm-island>`: the two-way read
/// (markdown-spec §6.4). A revise reads both against the stored value; a cold
/// import drops both, so where no token stands the markdown imports cold as
/// [`to_markdown`]'s does. A range anchor reads as its start.
///
/// A tag sits after the delimiters of the marks closing at its position and
/// before those opening there. Inside a code span or a link it moves to the
/// span's start, with `at="span"`. A code-block line, a block island's line
/// and an empty line hold none, and neither does a table cell or a footnote's
/// note, whose anchors are listed unspelled. Where a line's tags change what it
/// imports to, they move to its end, each with `at="line"` so a revise pins
/// nothing by it, and where that changes it too, the line holds none.
pub(crate) fn to_markdown_annotated(c: &Content) -> Annotated {
    let rt = View::of(c);
    let segments = line_segments(&rt);
    let line_marks = line_marks(&rt, &segments);
    let labels = note_labels(&rt);
    let plain = Ctx {
        rt: &rt,
        segments: &segments,
        line_marks: &line_marks,
        labels: &labels,
        tags: &[],
        tokens: &[],
    };
    let sites = anchor_sites(c);
    let mut tags: Vec<Vec<(Scalar, &str)>> = vec![Vec::new(); segments.len()];
    let mut anchors = Vec::with_capacity(sites.len());
    for site in &sites {
        let (at, read) = anchor_read(&plain, site);
        anchors.push(read);
        let Some(i) = at else { continue };
        let (start, id) = (site.start, site.id.as_str());
        let seg = &segments[i];
        let holds_tags = rt
            .lines
            .get(i)
            .is_some_and(|l| !matches!(l.kind, LineKind::Code { .. }))
            && seg.start < seg.end
            && start <= seg.end
            && block_island(&plain, i).is_none();
        if holds_tags {
            tags[i].push((start - seg.start, id));
        }
    }
    let tokens: Vec<bool> = c.islands.iter().map(|i| !island_reads_back(i)).collect();
    let markdown = project(&Ctx {
        rt: &rt,
        segments: &segments,
        line_marks: &line_marks,
        labels: &labels,
        tags: &tags,
        tokens: &tokens,
    });
    Annotated { markdown, anchors }
}

/// The anchors [`to_markdown_annotated`] lists for `c` whose id `keep` holds,
/// in its order, read without projecting `c`.
pub(crate) fn anchors_where(c: &Content, keep: impl Fn(&str) -> bool) -> Vec<AnchorRead> {
    let found: Vec<Site> = anchor_sites(c)
        .into_iter()
        .filter(|site| keep(&site.id))
        .collect();
    if found.is_empty() {
        return Vec::new();
    }
    let rt = View::of(c);
    let segments = line_segments(&rt);
    let labels = note_labels(&rt);
    let ctx = Ctx {
        rt: &rt,
        segments: &segments,
        line_marks: &[],
        labels: &labels,
        tags: &[],
        tokens: &[],
    };
    found.iter().map(|site| anchor_read(&ctx, site).1).collect()
}

/// Whether `isl`'s markup reads back as itself, slot anchors aside, which no
/// markdown spells: the island alone in a paragraph, exported and imported.
pub(crate) fn island_reads_back(isl: &Island) -> bool {
    let mut island = isl.clone();
    for slot in island.slots.values_mut() {
        slot.anchors.clear();
    }
    let alone = Content {
        text: ISLAND_SLOT.to_string(),
        islands: vec![island],
        ..Content::empty()
    };
    let Ok(back) = crate::markdown::from_markdown(&to_markdown(&alone)) else {
        return false;
    };
    matches!(&back.content.islands[..], [one] if one.island_type == isl.island_type && island_props(one) == island_props(isl))
}

/// What of an island markdown reads back: its props and its slots' text and
/// marks in their reading order, ids and anchors aside, each slot as
/// [`held_slots`] holds it, so a slot's breaks that read back otherwise are
/// [`Loss::SlotBreaks`] and not the island's.
fn island_props(isl: &Island) -> serde_json::Value {
    let mut isl = isl.clone();
    for (id, slot) in held_slots(&isl) {
        isl.slots.insert(id, slot);
    }
    let mut props = crate::table::to_line_props(&isl);
    crate::lines::export::drop_anchor_marks(&mut props);
    props
}

/// Each slot of `isl` by its id as its cell holds it: a hard break at the
/// slot's edge gone, and two in a row, or one ending a paragraph another
/// follows, read as markdown reads their `<br>`s.
pub(crate) fn held_slots(isl: &Island) -> Vec<(String, Content)> {
    let mut props = crate::table::to_line_props(isl);
    isl.island_type.normalize_props(&mut props);
    let (held_props, mut held) =
        crate::table::from_line_props(&isl.island_type, &props, &mut crate::ids::Minter::default());
    match isl.island_type {
        IslandType::Table => {
            let (Some(from), Some(to)) = (Table::read(&isl.props), Table::read(&held_props)) else {
                return Vec::new();
            };
            from.slot_order()
                .into_iter()
                .zip(to.slot_order())
                .filter_map(|(id, at)| Some((id, held.remove(&at)?)))
                .collect()
        }
        _ => held.into_iter().collect(),
    }
}

/// The content as markdown reads it: one [`Line`] per run of text between
/// block and hard breaks, the text with each hard break a `\n`, the marks
/// markdown writes, and the islands.
struct View<'a> {
    text: String,
    lines: Vec<Line>,
    /// The content's marks but its `x-` marks, which markdown does not write.
    marks: Vec<&'a Mark>,
    islands: &'a [Island],
}

/// One line of a [`View`]: the block role it writes, the containers around
/// it, outermost first, and whether it continues the block above across a
/// hard break.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    kind: LineKind,
    containers: Vec<Level>,
    continues: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum LineKind {
    Para,
    Heading { level: u8 },
    Code { lang: Option<String> },
    Rule,
}

/// One container level of a line. `ordinal` counts the item within its list
/// from 0, and `instance` alternates between 0 and 1 across adjacent runs of
/// one shape, which markdown writes with alternating markers.
#[derive(Debug, Clone, PartialEq)]
enum Level {
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
    fn of(c: &'a Content) -> Self {
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
                .collect(),
            islands: &c.islands,
        }
    }
}

/// Which text an anchor sits in: the prose, or the `cell`th cell of the
/// `island`th island, cells read in [`cells`]' order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Surface {
    Prose,
    Cell { island: usize, cell: usize },
}

/// One anchor and the surface holding it, its range in that surface's text.
struct Site {
    id: String,
    surface: Surface,
    start: Scalar,
}

/// An island's cells as markdown writes them, in reading order: a table's
/// row by row from its header, a footnote's note, an `x-` island's slots by
/// id. Each is its text, each paragraph break a blank line and each hard
/// break a `\n`, and the marks and anchors over it.
fn cells(isl: &Island) -> Vec<(String, Vec<Mark>, Vec<(String, Scalar)>)> {
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

/// A slot as a cell: [`crate::table::slot_to_cell`]'s text, its marks, and
/// each anchor's id and start.
fn cell(slot: &Content) -> (String, Vec<Mark>, Vec<(String, Scalar)>) {
    let (text, line_marks) = crate::table::slot_to_cell(slot);
    let mut marks = Vec::new();
    let mut anchors = Vec::new();
    for m in line_marks {
        match &m.kind {
            crate::lines::model::MarkKind::Anchor { id } => anchors.push((id.clone(), m.start)),
            kind => marks.extend(MarkKind::from_line(kind).map(|k| Mark::new(m.start, m.end, k))),
        }
    }
    (text, marks, anchors)
}

/// Every anchor of `c`, in the prose and in each island's cells, in reading
/// order: by the position in the text where each stands, a cell's at its
/// island's slot, then by surface, start and id.
fn anchor_sites(c: &Content) -> Vec<Site> {
    let len = c.len_scalars();
    let mut sites: Vec<(Scalar, Site)> = c
        .anchors
        .iter()
        .filter(|a| a.start <= a.end && a.end <= len)
        .map(|a| {
            (
                a.start,
                Site {
                    id: a.id.clone(),
                    surface: Surface::Prose,
                    start: a.start,
                },
            )
        })
        .collect();
    let slots = c
        .text
        .chars()
        .enumerate()
        .filter(|&(_, ch)| ch == ISLAND_SLOT)
        .map(|(at, _)| at);
    for (island, (isl, at)) in c.islands.iter().zip(slots).enumerate() {
        for (cell, (_, _, anchors)) in cells(isl).into_iter().enumerate() {
            sites.extend(anchors.into_iter().map(|(id, start)| {
                (
                    at,
                    Site {
                        id,
                        surface: Surface::Cell { island, cell },
                        start,
                    },
                )
            }));
        }
    }
    sites.sort_by(|(a, x), (b, y)| {
        (a, x.surface, x.start, &x.id).cmp(&(b, y.surface, y.start, &y.id))
    });
    sites.into_iter().map(|(_, site)| site).collect()
}

/// The read of `site`, and the index of the line it sits on where that is in
/// the prose.
fn anchor_read(ctx: &Ctx, site: &Site) -> (Option<usize>, AnchorRead) {
    let id = site.id.clone();
    if let Surface::Cell { island, cell } = site.surface {
        let line = ctx
            .rt
            .islands
            .get(island)
            .and_then(|isl| cells(isl).into_iter().nth(cell))
            .map(|(text, _, _)| text)
            .unwrap_or_default();
        return (None, AnchorRead { id, line });
    }
    let segments = ctx.segments;
    let i = segments
        .partition_point(|s| s.end < site.start)
        .min(segments.len() - 1);
    let line = seg_str(ctx, i)
        .chars()
        .filter(|&c| c != ISLAND_SLOT)
        .collect();
    (Some(i), AnchorRead { id, line })
}

fn project(ctx: &Ctx) -> String {
    let mut out = String::new();
    emit_block(ctx, 0..ctx.rt.lines.len(), 0, &mut out);
    // `to_markdown` projects a *value*, not a file: it emits no final newline,
    // so `writer.set("subject", "Hello")` reads back as `"Hello"`. Document-file
    // writers own the file-final newline, and import is newline-insensitive, so
    // dropping it is round-trip-invisible.
    while out.ends_with('\n') {
        out.pop();
    }
    // Each note is defined after the body, at the root, where a blank line
    // closes whatever container the body ended in.
    for (k, (isl, label)) in ctx.rt.islands.iter().zip(ctx.labels).enumerate() {
        if isl.island_type == IslandType::Footnote && ctx.tokens.get(k) != Some(&true) {
            out.push_str(&format!("\n\n[^{label}]: "));
            out.push_str(&render_note_md(isl));
        }
    }
    out
}

/// Per island, the label its footnote reference is written with: the count of
/// footnotes up to and including it, so the notes number `1, 2, …` in text
/// order. Re-import pairs a reference with its definition by label and keeps
/// none of it.
fn note_labels(rt: &View) -> Vec<usize> {
    rt.islands
        .iter()
        .scan(0, |n, isl| {
            *n += usize::from(isl.island_type == IslandType::Footnote);
            Some(*n)
        })
        .collect()
}

struct Ctx<'a> {
    rt: &'a View<'a>,
    /// One entry per [`View::lines`] entry.
    segments: &'a [Segment],
    /// One entry per island: [`note_labels`].
    labels: &'a [usize],
    /// Per line, the indices of the marks overlapping it: [`line_marks`].
    line_marks: &'a [Vec<usize>],
    /// Per line, the anchor tags it holds at line-local positions, in
    /// `(position, id)` order; empty for the plain projection.
    tags: &'a [Vec<(Scalar, &'a str)>],
    /// Per island, whether it is spelled as its `qm-island` token: one
    /// [`island_reads_back`] refuses, in an annotated read. Empty for the
    /// plain projection.
    tokens: &'a [bool],
}

/// One line's char range `[start, end)` into the view's text, with the
/// matching byte range and the count of island slots before the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Segment {
    start: usize,
    end: usize,
    byte_start: usize,
    byte_end: usize,
    slots_before: usize,
}

/// Per line of `segments`, the indices of `rt`'s marks overlapping it, in
/// mark order.
fn line_marks(rt: &View, segments: &[Segment]) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); segments.len()];
    for (k, m) in rt.marks.iter().enumerate() {
        let first = segments.partition_point(|s| s.end <= m.start);
        for (i, seg) in segments.iter().enumerate().skip(first) {
            if seg.start >= m.end {
                break;
            }
            out[i].push(k);
        }
    }
    out
}

/// Per-line char/byte ranges and slot prefixes over a view, in line order.
fn line_segments(rt: &View) -> Vec<Segment> {
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
fn segment(lines: &[Line], range: std::ops::Range<usize>, depth: usize) -> std::ops::Range<usize> {
    let end = range.end.min(lines.len());
    let mut j = (range.start + 1).min(end);
    while j < end && lines[j].containers.len() == depth && lines[j].continues {
        j += 1;
    }
    range.start..j
}

/// The first item at `depth` in `range`: the lines from its first holding a
/// container there, while each holds the same one.
fn item(
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

/// One open level of the block walk: `at` is the next line of `range` to emit,
/// `head` the line opening its first block written, `buf` the markdown emitted
/// for the level so far, and `container` the one whose syntax prefixes `buf`
/// when the level closes (`None` at the root).
struct Frame<'a> {
    container: Option<&'a Level>,
    at: usize,
    head: Option<usize>,
    range: std::ops::Range<usize>,
    depth: usize,
    buf: String,
}

impl<'a> Frame<'a> {
    fn open(container: Option<&'a Level>, range: std::ops::Range<usize>, depth: usize) -> Self {
        Frame {
            container,
            at: range.start,
            head: None,
            range,
            depth,
            buf: String::new(),
        }
    }
}

/// Emit the lines in `range`, all sharing the container prefix of length
/// `depth`. Leaf lines render at this level; deeper lines are grouped by their
/// `depth`-th container and open a level of their own, which
/// [`close_container`] prefixes on the way back out. A frame stack, not
/// recursion, so a content nesting past what validation admits projects
/// rather than overflowing the stack.
fn emit_block(ctx: &Ctx, range: std::ops::Range<usize>, depth: usize, out: &mut String) {
    let lines = &ctx.rt.lines;
    let mut stack = vec![Frame::open(None, range, depth)];
    while let Some(frame) = stack.last_mut() {
        if frame.at < frame.range.end {
            let i = frame.at;
            if lines[i].containers.len() > frame.depth {
                let (container, range) = item(lines, i..frame.range.end, frame.depth);
                block_separator(&mut frame.buf, frame.head.is_none());
                frame.head.get_or_insert(i);
                frame.at = range.end;
                let child = Frame::open(Some(container), range, frame.depth + 1);
                stack.push(child);
            } else {
                let seg = segment(lines, i..frame.range.end, frame.depth);
                frame.at = seg.end;
                // Markdown spells an empty paragraph with nothing, so beside
                // another block one writes nothing; a container's only block
                // keeps its line.
                if is_empty_para(ctx, seg.clone())
                    && (frame.head.is_some() || seg.end < frame.range.end)
                {
                    continue;
                }
                block_separator(&mut frame.buf, frame.head.is_none());
                frame.head.get_or_insert(i);
                emit_leaf_block(ctx, seg, &mut frame.buf);
            }
            continue;
        }
        let done = stack.pop().expect("`last_mut` just yielded this frame");
        match stack.last_mut() {
            Some(parent) => {
                let key = done
                    .container
                    .expect("only the root frame opens without a container");
                let head = done.head.unwrap_or(done.range.start);
                let lead = &lines[head];
                let prose_head = lead.containers.len() == done.depth
                    && lead.kind == LineKind::Para
                    && block_island(ctx, head).is_none();
                close_container(key, &done.buf, prose_head, &mut parent.buf);
            }
            None => out.push_str(&done.buf),
        }
    }
}

fn block_separator(out: &mut String, first_block: bool) {
    if !first_block {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
}

/// Close a container level: prefix each line of `inner`, the block emitted for
/// it, with the container's markdown syntax. `prose_head` says the level opens
/// on a paragraph line of its own.
fn close_container(key: &Level, inner: &str, prose_head: bool, out: &mut String) {
    match key {
        Level::ListItem {
            ordered,
            start,
            ordinal,
            checked,
            instance,
        } => {
            // CommonMark starts a new list at a change of bullet char or of
            // ordered delimiter, and stores neither, so the marker is where the
            // `instance` alternation lands: it spells two adjacent lists apart
            // in the projection exactly as `instance` does in the model.
            //
            // `+` rather than `*` for the second bullet: `* ***` is four
            // asterisks separated by spaces, which reads as a thematic break
            // and would take the item with it (see
            // `rule_opening_a_list_item_keeps_its_item`).
            let marker = if *ordered {
                // `start`/`ordinal` are unbounded `u64` and `validate` does not
                // ceiling them, so a corrupt/adversarial content can drive the
                // sum past `u64::MAX`; saturate rather than panic (or wrap under
                // release overflow-checks) on the render path.
                let n = start.saturating_add(*ordinal);
                if instance % 2 == 0 {
                    format!("{n}. ")
                } else {
                    format!("{n}) ")
                }
            } else if instance % 2 == 0 {
                "- ".to_string()
            } else {
                "+ ".to_string()
            };
            let indent = " ".repeat(marker.len());
            let head = inner.split('\n').next().unwrap_or("");
            // Whether the item's first block moves to the line below its
            // marker. A task marker opens its item's paragraph, so text after
            // it on its line is read as that paragraph whatever block it
            // spells. A marker run that spells a thematic break outranks the
            // items spelling it: three nested empty bullets emit `- - - `,
            // which re-imports as a `Rule` with the nesting gone. Changing a
            // marker char is not the way out — a different bullet char starts
            // a new list, resetting `ordinal` on this item and every one after
            // it, and the empty item can have non-empty siblings. Moving the
            // content costs no marker and no list identity, and the check runs
            // per level, so a run of any depth breaks into pieces of two.
            let (lead, below) = match checked {
                Some(done) => (
                    format!("{marker}{} ", if *done { "[x]" } else { "[ ]" }),
                    !prose_head || head.is_empty(),
                ),
                None => (
                    marker.clone(),
                    is_thematic_break(&format!("{marker}{head}")),
                ),
            };
            // An empty item keeps its lead's trailing space: a task marker
            // reads as one only before whitespace, and the item can end the
            // markdown.
            if below && !inner.is_empty() {
                out.push_str(lead.trim_end());
                out.push('\n');
                prefix_lines(inner, &indent, &indent, out);
            } else {
                prefix_lines(inner, &lead, &indent, out);
            }
        }
        Level::Quote { .. } => {
            // `> ` on content lines, `>` on blank lines so paragraphs stay in
            // one quote on re-import.
            prefix_quote(inner, out);
        }
    }
}

/// CommonMark's thematic break: three or more of one of `-`, `_`, `*`, spaces
/// and tabs between them and nowhere else, under at most three of indent.
fn is_thematic_break(line: &str) -> bool {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return false;
    }
    let Some(c) = rest.chars().next().filter(|c| matches!(c, '-' | '_' | '*')) else {
        return false;
    };
    let mut n = 0;
    for ch in rest.chars() {
        if ch == c {
            n += 1;
        } else if ch != ' ' && ch != '\t' {
            return false;
        }
    }
    n >= 3
}

/// Prefix the first produced line with `first`, the rest but blank ones with
/// `cont`.
fn prefix_lines(inner: &str, first: &str, cont: &str, out: &mut String) {
    for (idx, line) in inner.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if idx == 0 || !line.is_empty() {
            out.push_str(if idx == 0 { first } else { cont });
            out.push_str(line);
        }
    }
}

fn prefix_quote(inner: &str, out: &mut String) {
    for (idx, line) in inner.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            out.push('>');
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
    }
}

fn emit_code(ctx: &Ctx, range: std::ops::Range<usize>, lang: Option<&str>, out: &mut String) {
    // Choose a fence long enough to not collide with backtick runs in content.
    let mut max_ticks = 0usize;
    for i in range.clone() {
        max_ticks = max_ticks.max(longest_backtick_run(seg_str(ctx, i)));
    }
    let fence = "`".repeat(max_ticks.max(2) + 1);
    out.push_str(&fence);
    if let Some(l) = lang {
        out.push_str(l);
    }
    out.push('\n');
    for i in range {
        out.push_str(seg_str(ctx, i));
        out.push('\n');
    }
    out.push_str(&fence);
}

/// Emit one leaf block: the lines `range.start` (a block start) plus any
/// continuation lines. A paragraph block joins its lines with a markdown hard
/// break (`\` + newline); a code block renders one fence; a heading is a single
/// line. A block island's line is a `Para` whose slot's markup *is* the block,
/// so the island writes the whole line and `normalize` leaves it no
/// continuation to join to it.
fn emit_leaf_block(ctx: &Ctx, range: std::ops::Range<usize>, out: &mut String) {
    // Ahead of the kind, which cannot tell this line from prose. Markdown has
    // no syntax wrapping a block in an inline mark: `[<pipe table>](u)`
    // re-imports as prose with the island gone, so a mark reaching over the
    // slot projects as nothing.
    if block_island(ctx, range.start).is_some() {
        island_markup(ctx, ctx.segments[range.start].slots_before, out);
        return;
    }
    let first = &ctx.rt.lines[range.start];
    let dollar = block_escapes_dollar(ctx, range.clone());
    match &first.kind {
        LineKind::Code { lang } => emit_code(ctx, range, lang.as_deref(), out),
        LineKind::Heading { level } => {
            for _ in 0..*level {
                out.push('#');
            }
            out.push(' ');
            // Headings never carry continuations (import maps a hard break in a
            // heading to a space), so only the first line contributes.
            out.push_str(&render_inline(ctx, range.start, true, dollar));
        }
        // An empty line between hard breaks writes nothing, which a trailing
        // `\` would spell as text.
        LineKind::Para => {
            let parts: Vec<String> = range
                .map(|i| render_inline(ctx, i, false, dollar))
                .filter(|part| !part.is_empty())
                .collect();
            out.push_str(&parts.join("\\\n"));
        }
        // `***`, not `---`: all three break spellings import alike, but `---` is
        // also a setext underline, a front-matter fence, and — with the spaces a
        // list prefix supplies — a bullet line, so `- ` + `---` re-imports as a
        // top-level break, losing the item. `***` survives every container
        // prefix.
        LineKind::Rule => out.push_str("***"),
    }
}

/// Whether the block in `range` writes each `$` of its text as `\$`: one
/// that holds a `$` and, counting each equation's two delimiters, another.
/// A lone `$` pairs with nothing, so `$5` stays as written, and an equation
/// can span the block's hard breaks.
fn block_escapes_dollar(ctx: &Ctx, range: std::ops::Range<usize>) -> bool {
    let (mut dollars, mut maths) = (0, 0);
    for i in range {
        let text = seg_str(ctx, i);
        dollars += text.matches('$').count();
        let first = ctx.segments[i].slots_before;
        let slots = text.matches(ISLAND_SLOT).count();
        maths += ctx.rt.islands[first.min(ctx.rt.islands.len())..]
            .iter()
            .take(slots)
            .filter(|isl| isl.island_type == IslandType::Math)
            .count();
    }
    escapes_dollar(dollars, maths)
}

/// Whether a fragment holding `dollars` `$` chars in its text and `maths`
/// equations writes each `$` escaped.
fn escapes_dollar(dollars: usize, maths: usize) -> bool {
    dollars > 0 && dollars + 2 * maths >= 2
}

/// Whether the block in `range` is a paragraph whose every line is empty.
fn is_empty_para(ctx: &Ctx, range: std::ops::Range<usize>) -> bool {
    ctx.rt.lines[range.start].kind == LineKind::Para
        && range.into_iter().all(|i| seg_str(ctx, i).is_empty())
}

fn seg_str<'a>(ctx: &'a Ctx, i: usize) -> &'a str {
    let seg = &ctx.segments[i];
    &ctx.rt.text[seg.byte_start..seg.byte_end]
}

/// The block-only island line `i` holds alone, which writes the whole line.
fn block_island<'a>(ctx: &'a Ctx, i: usize) -> Option<&'a Island> {
    let mut chars = seg_str(ctx, i).chars();
    let alone = (chars.next(), chars.next()) == (Some(ISLAND_SLOT), None);
    let isl = ctx.rt.islands.get(ctx.segments[i].slots_before);
    isl.filter(|isl| alone && isl.island_type.block_only())
}

/// Island `index`'s markup, or its token where the read spells one.
fn island_markup(ctx: &Ctx, index: usize, out: &mut String) {
    let isl = &ctx.rt.islands[index];
    match ctx.tokens.get(index) {
        Some(true) => out.push_str(&island_token(&isl.id)),
        _ => emit_island(isl, ctx.labels[index], out),
    }
}

/// The carrier's canonical spelling of island `id`'s token.
fn island_token(id: &str) -> String {
    crate::lines::carrier::Element::spelling(
        crate::lines::carrier::ISLAND,
        [("ref".to_string(), id.to_string())].into(),
    )
    .expect("`island` and `ref` are in the carrier grammar")
    .wrap_inline("")
}

/// An island's markup; a footnote's is its reference, written with `label`
/// ([`note_labels`]).
fn emit_island(isl: &Island, label: usize, out: &mut String) {
    match isl.island_type {
        IslandType::Table => emit_table(isl, out),
        IslandType::Image => emit_image(isl, out),
        IslandType::Footnote => out.push_str(&format!("[^{label}]")),
        IslandType::Math => emit_math(isl, out),
        IslandType::Ext(_) => {}
    }
}

/// `$tex$`, or `$$tex$$` for a display equation.
fn emit_math(isl: &Island, out: &mut String) {
    let tex = isl.props.get("tex").and_then(|v| v.as_str()).unwrap_or("");
    let display = isl.props.get("display") == Some(&serde_json::Value::Bool(true));
    let delim = if display { "$$" } else { "$" };
    out.push_str(delim);
    out.push_str(tex);
    out.push_str(delim);
}

/// Whether an equation's `tex`, as it rests, reads back as itself between its
/// delimiters: `$tex$`, or `$$tex$$` where `display`, parsed alone. An empty
/// inline `tex`, an unescaped `$` and braces that do not balance are what it
/// refuses. The authored lanes refuse the rest.
pub fn tex_is_writable(tex: &str, display: bool) -> bool {
    let delim = if display { "$$" } else { "$" };
    let md = format!("{delim}{tex}{delim}");
    let mut events =
        pulldown_cmark::Parser::new_ext(&md, crate::lines::import::options()).filter(|e| {
            !matches!(
                e,
                pulldown_cmark::Event::Start(_) | pulldown_cmark::Event::End(_)
            )
        });
    let read = match (events.next(), events.next()) {
        (Some(pulldown_cmark::Event::InlineMath(t)), None) if !display => t,
        (Some(pulldown_cmark::Event::DisplayMath(t)), None) if display => t,
        _ => return false,
    };
    &*read == tex
}

/// A table as a GFM pipe table, or as HTML where it holds what a pipe table
/// cannot write ([`crate::lines::html_table::writes_html`]). Each cell's
/// markdown is rebuilt from its slot, so nothing re-parses markdown and
/// `import(export(table))` is a fixed point.
fn emit_table(isl: &Island, out: &mut String) {
    let Some(table) = Table::read(&isl.props).filter(|t| !t.rows.is_empty()) else {
        return;
    };
    let cols = table.columns.len();
    if cols == 0 {
        return;
    }
    let render = |r: usize, c: usize, frag: Frag| {
        table
            .slot(r, c)
            .and_then(|s| isl.slots.get(s))
            .map_or_else(String::new, |slot| render_cell_md(slot, frag))
    };
    let layout = crate::table::to_line_props(isl);
    if crate::lines::html_table::writes_html(&layout) {
        out.push_str(&crate::lines::html_table::write(&layout, |r, c| {
            render(r, c, Frag::HtmlCell)
        }));
        return;
    }
    let aligns = isl.props.get("aligns").and_then(|v| v.as_object());
    let row = |r: usize| {
        (0..cols)
            .map(|c| render(r, c, Frag::Cell))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let mut out_table = format!("| {} |\n|", row(0));
    for column in &table.columns {
        let a = aligns
            .and_then(|a| a.get(column))
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        out_table.push_str(match a {
            "left" => " :--- |",
            "center" => " :---: |",
            "right" => " ---: |",
            _ => " --- |",
        });
    }
    for r in 1..table.rows.len() {
        out_table.push_str(&format!("\n| {} |", row(r)));
    }
    out.push_str(&out_table);
}

fn emit_image(isl: &Island, out: &mut String) {
    let url = isl.props.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let alt = isl.props.get("alt").and_then(|v| v.as_str()).unwrap_or("");
    // Alt is inline content of `![…]`: escape it like a link's display text so a
    // `]`/`\`/`&`/delimiter can't terminate the markup or decode on re-import.
    out.push_str("![");
    let dollar = escapes_dollar(alt.matches('$').count(), 0);
    out.push_str(&escape_run(
        &alt.chars().collect::<Vec<_>>(),
        Esc {
            frag: Frag::Prose,
            dollar,
        },
    ));
    out.push_str("](");
    emit_url(url, out);
    out.push(')');
}

/// Emit a link/image destination. A bare (unbracketed) destination round-trips
/// only for CommonMark's unbracketed form: no whitespace, control char, `<`,
/// `>`, `\`, or `&`, and balanced parentheses. Anything else is angle-wrapped,
/// with `<`, `>`, `\` and `&` backslash-escaped, since unescaped `<`/`>` are
/// illegal even inside the wrap and `\`/`&` would be consumed as an escape or
/// entity reference on re-import.
///
/// Every character re-imports to itself except the two
/// [`url_is_writable`] refuses: a line ending is illegal inside the wrap too, so
/// it is percent-encoded, ahead of the choice of form, and comes back in that
/// form.
pub(crate) fn emit_url(url: &str, out: &mut String) {
    let url = match url_is_writable(url) {
        true => std::borrow::Cow::Borrowed(url),
        false => std::borrow::Cow::Owned(url.replace('\n', "%0A").replace('\r', "%0D")),
    };
    if url_is_bare_safe(&url) {
        out.push_str(&url);
        return;
    }
    out.push('<');
    for c in url.chars() {
        match c {
            '<' | '>' | '\\' | '&' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('>');
}

/// Whether a link or image `url` survives the markdown export unchanged.
/// CommonMark admits no line ending in a destination, bare or angle-wrapped,
/// so an authored lane that stores a url refuses one and the export
/// percent-encodes the one a stored or hand-built content still holds.
pub fn url_is_writable(url: &str) -> bool {
    !url.contains(['\n', '\r'])
}

fn url_is_bare_safe(url: &str) -> bool {
    let mut depth: i32 = 0;
    for c in url.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            '<' | '>' | '\\' | '&' => return false,
            c if c.is_whitespace() || c.is_control() => return false,
            _ => {}
        }
    }
    depth == 0
}

/// Line `i`'s inline markdown, as a heading's text after its `#`s or as a
/// paragraph line, which escapes what would open a block. `dollar` escapes
/// each `$` ([`block_escapes_dollar`]).
fn render_inline(ctx: &Ctx, i: usize, heading: bool, dollar: bool) -> String {
    let escape_leading_block = !heading;
    let seg = &ctx.segments[i];
    let line_start = seg.start;
    let text = seg_str(ctx, i);
    let chars: Vec<char> = text.chars().collect();

    let marks = ctx.line_marks[i].iter().map(|&k| ctx.rt.marks[k]);
    let (code_ranges, fmt, links) = bucket_marks(marks, line_start, chars.len(), false);
    let mut slot = seg.slots_before;
    let note_labels: Vec<Option<usize>> = chars
        .iter()
        .map(|&c| {
            if c != ISLAND_SLOT {
                return None;
            }
            slot += 1;
            let isl = ctx.rt.islands.get(slot - 1)?;
            (isl.island_type == IslandType::Footnote).then(|| ctx.labels[slot - 1])
        })
        .collect();
    // A footnote reference inside link text is not read as one, so a link
    // over a reference parts around it.
    let links: Vec<(usize, usize, &str)> = links
        .into_iter()
        .flat_map(|(s, e, url)| {
            let mut parts = Vec::new();
            let mut run = s;
            for at in (s..e).filter(|&at| note_labels[at].is_some()) {
                if run < at {
                    parts.push((run, at, url));
                }
                run = at + 1;
            }
            if run < e {
                parts.push((run, e, url));
            }
            parts
        })
        .collect();
    // The probe reads the line back alone, so it carries a definition for
    // each reference the line writes.
    let probe_tail: String = note_labels
        .iter()
        .flatten()
        .map(|label| format!("\n\n[^{label}]: x"))
        .collect();

    let escape_punct_at = if escape_leading_block {
        list_marker_punct(&chars)
    } else {
        None
    };

    let (mut md, mut points) = render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        escape_punct_at,
        escape_leading_block,
        Esc {
            frag: Frag::Prose,
            dollar,
        },
        &probe_tail,
        |pos_local| {
            let before = seg.slots_before
                + chars[..pos_local]
                    .iter()
                    .filter(|&&c| c == ISLAND_SLOT)
                    .count();
            (before < ctx.rt.islands.len()).then(|| {
                let mut markup = String::new();
                island_markup(ctx, before, &mut markup);
                markup
            })
        },
    );
    // A trailing `#` run reads as an ATX closing sequence on re-import
    // (`# a #` → heading text "a"). One escaped hash defeats the whole closer,
    // and `\#` re-imports as a literal `#`. A tag after the run sits after the
    // escape.
    if heading && md.ends_with('#') {
        let end = md.len();
        md.insert(end - 1, '\\');
        for point in points.iter_mut().filter(|p| **p == end) {
            *point += 1;
        }
    }
    match ctx.tags.get(i).filter(|t| !t.is_empty()) {
        Some(tags) => annotate(md, &points, tags),
        None => md,
    }
}

/// Where a line's text starts `<digits>.` or `<digits>)`, the punctuation to
/// escape so it does not re-import as an ordered list.
fn list_marker_punct(chars: &[char]) -> Option<usize> {
    let lead_digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
    (lead_digits > 0 && lead_digits < chars.len() && matches!(chars[lead_digits], '.' | ')'))
        .then_some(lead_digits)
}

/// `md`, one line's markdown, with each of `tags` at its position's tag point
/// in `points`. A line whose tags change what it imports to holds them at its
/// end instead, and none where that changes it too: each spelling is read
/// back as the net reads one, inside `,…,`.
pub(crate) fn annotate(md: String, points: &[usize], tags: &[(Scalar, &str)]) -> String {
    let reads = |line: &str| {
        crate::lines::import::from_markdown(&format!(",{line},"))
            .ok()
            .map(|i| i.content)
    };
    let Some(plain) = reads(&md) else {
        return md;
    };
    let mut inline = String::new();
    let mut at = 0;
    for &(pos, id) in tags {
        let point = points[pos];
        inline.push_str(&md[at..point]);
        let moved = pos > 0 && points[pos - 1] == point;
        inline.push_str(&anchor_tag(id, moved.then_some("span")));
        at = point;
    }
    inline.push_str(&md[at..]);
    if reads(&inline).as_ref() == Some(&plain) {
        return inline;
    }
    let mut at_end = md.clone();
    for &(_, id) in tags {
        at_end.push_str(&anchor_tag(id, Some("line")));
    }
    if reads(&at_end).as_ref() == Some(&plain) {
        return at_end;
    }
    md
}

/// The carrier's canonical spelling of an anchor `id`'s tag, with `at`
/// naming where a tag that does not stand at its anchor's start moved to: the
/// start of the code span or link holding the anchor's start (`span`), or the
/// line's end (`line`).
fn anchor_tag(id: &str, at: Option<&str>) -> String {
    let mut attrs: std::collections::BTreeMap<String, String> =
        [("ref".to_string(), id.to_string())].into();
    if let Some(at) = at {
        attrs.insert("at".to_string(), at.to_string());
    }
    crate::lines::carrier::Element::spelling(crate::lines::carrier::ANCHOR, attrs)
        .expect("`anchor`, `at` and `ref` are in the carrier grammar")
        .wrap_inline("")
}

/// Route `marks` into the three lists [`render_marked_core`] takes, each range
/// clipped to the `n` chars starting at `line_start` and rebased to local
/// offsets. An `x-` mark is dropped: the projection has no syntax for it.
/// `drop_empty` also drops a range the clip left empty, which a cell wants and
/// a prose line does not.
fn bucket_marks<'m>(
    marks: impl IntoIterator<Item = &'m Mark>,
    line_start: usize,
    n: usize,
    drop_empty: bool,
) -> (
    Vec<(usize, usize)>,
    Vec<(usize, usize, &'m MarkKind)>,
    Vec<(usize, usize, &'m str)>,
) {
    let mut code_ranges: Vec<(usize, usize)> = Vec::new();
    let mut fmt: Vec<(usize, usize, &MarkKind)> = Vec::new();
    let mut links: Vec<(usize, usize, &str)> = Vec::new();
    for m in marks {
        if m.end <= line_start || m.start >= line_start + n {
            continue;
        }
        let s = m.start.saturating_sub(line_start).min(n);
        let e = m.end.saturating_sub(line_start).min(n);
        if drop_empty && s >= e {
            continue;
        }
        match &m.kind {
            MarkKind::Ext { .. } => {}
            MarkKind::Code => code_ranges.push((s, e)),
            MarkKind::Link { url } => links.push((s, e, url.as_str())),
            _ => fmt.push((s, e, &m.kind)),
        }
    }
    (code_ranges, fmt, links)
}

/// Render marks over a standalone char slice to markdown: the projection's mark
/// boundary sweep, shared by prose lines, table cells and footnote notes.
/// `code_ranges`/`fmt`/`links` are the marks clipped to `chars` (local
/// offsets); `esc` says which of the three `chars` is and whether a `$`
/// escapes; `probe_tail` follows a prose line when the net reads it back;
/// `island_markup_at` renders an island slot (prose) or yields `None` (cells
/// and notes carry no slot).
///
/// Returns the markdown and each position's tag point, one entry per position
/// `0..=n`: the byte offset in the markdown after the delimiters of the marks
/// closing there and before those opening there. A position inside a code
/// span or link takes the span's start's.
///
/// The model permits free (Peritext-style) overlap but markdown syntax nests.
/// The sweep closes every mark ending at a boundary and reopens the deeper
/// survivors, so a partial overlap lowers to balanced markdown
/// (`**ab~~cd~~**~~ef~~`), which re-imports to the same content for marks with
/// *distinct* delimiters. Two shapes the sweep can't express are clipped first:
///
/// - **Atomic spans** (`code`/`link`) can't carry a partial wrap, and the
///   sweep's cursor jumps their interior, so a wrap edge inside would be left
///   unbalanced. [`clip_fmt_to_atomic`] pulls such edges to the span boundary.
/// - **`*`/`**` share a delimiter character**, so a reopened `*` abutting a `**`
///   merges into an ambiguous `***` run CommonMark re-segments wrong.
///   [`clip_asterisk_overlap`] nests the two by truncation, keeping the text and
///   losing the crossing tail.
#[allow(clippy::too_many_arguments)]
fn render_marked_core(
    chars: &[char],
    code_ranges: &[(usize, usize)],
    fmt: &[(usize, usize, &MarkKind)],
    links: &[(usize, usize, &str)],
    escape_punct_at: Option<usize>,
    escape_leading_block: bool,
    esc: Esc,
    probe_tail: &str,
    island_markup_at: impl Fn(usize) -> Option<String>,
) -> (String, Vec<usize>) {
    let escape_pipe = esc.frag == Frag::Cell;
    let n = chars.len();

    // A code span is emitted verbatim between backticks, so a slot inside it
    // would land raw and re-import as nothing, taking the island with it, and a
    // cell's `\n` would land raw and split the pipe row. Markdown has no honest
    // encoding for either (`` `![x](y)` `` and `` `<br>` `` are *literal* text),
    // so split each code range around every slot and `\n` it covers and let
    // the island or break render between the surviving code spans.
    //
    // A code span in an HTML table's cell ends at the subset's next tag, so a
    // range holding one splits around it too, the tag written as escaped text.
    let mut code_ranges: Vec<(usize, usize)> = code_ranges
        .iter()
        .flat_map(|&(s, e)| split_around_slots(chars, s, e))
        .flat_map(|(s, e)| match esc.frag {
            Frag::HtmlCell => split_around_table_tags(chars, s, e),
            _ => vec![(s, e)],
        })
        .collect();
    // The sweep reaches the atomic spans with a cursor, so both lists are put in
    // start order here rather than re-scanned at every position.
    code_ranges.sort_unstable();
    let code_ranges = &code_ranges[..];
    let mut links: Vec<(usize, usize, &str)> = links.to_vec();
    links.sort_by_key(|&(s, _, _)| s);
    let links = &links[..];

    // Clip the wrapping marks so the sweep only ever sees a representable shape.
    let mut fmt: Vec<(usize, usize, &MarkKind)> = fmt.to_vec();
    // A mark crossing one edge of a link writes as one piece outside the link
    // and one inside its text; one over the whole link wraps it.
    for &(ls, le, _) in links {
        for edge in [ls, le] {
            let crosses = |m: &(usize, usize, &MarkKind)| {
                m.0 < edge && edge < m.1 && !(m.0 <= ls && le <= m.1)
            };
            let crossing: Vec<_> = fmt
                .iter()
                .filter(|m| crosses(m))
                .map(|m| (edge, m.1, m.2))
                .collect();
            for m in fmt.iter_mut().filter(|m| crosses(m)) {
                m.1 = edge;
            }
            fmt.extend(crossing);
        }
    }
    let inside_link = |m: &(usize, usize, &MarkKind)| {
        links
            .iter()
            .any(|&(ls, le, _)| ls <= m.0 && m.1 <= le && (m.0, m.1) != (ls, le))
    };
    let (mut inner, mut outer): (Vec<_>, Vec<_>) = fmt.drain(..).partition(|m| inside_link(m));
    let mut atomics: Vec<(usize, usize)> = code_ranges.to_vec();
    clip_fmt_to_atomic(&mut inner, &atomics);
    atomics.extend(links.iter().map(|(s, e, _)| (*s, *e)));
    clip_fmt_to_atomic(&mut outer, &atomics);
    fmt.extend(outer);
    fmt.extend(inner);
    // A clip can land an edge on a cell's `\n`, where storage trims a mark's
    // edge, so the probe below would read back a mark other than the one asked.
    for m in fmt.iter_mut() {
        while m.0 < m.1 && chars[m.0] == '\n' {
            m.0 += 1;
        }
        while m.1 > m.0 && chars[m.1 - 1] == '\n' {
            m.1 -= 1;
        }
    }
    // A `*`, `_` or `~~` run beside whitespace never closes or opens a span,
    // so such a mark writes over its text without the whitespace at its edges.
    for m in fmt
        .iter_mut()
        .filter(|m| matches!(m.2, MarkKind::Strong | MarkKind::Emph | MarkKind::Strike))
    {
        while m.0 < m.1 && chars[m.0].is_whitespace() {
            m.0 += 1;
        }
        while m.1 > m.0 && chars[m.1 - 1].is_whitespace() {
            m.1 -= 1;
        }
    }
    fmt.retain(|m| m.0 < m.1);
    // The net's re-add priority is document order, so it reads the spans as
    // written, not as stored: two marks the trims land on one span rank by kind.
    fmt.sort_by(|a, b| (a.0, a.1, a.2.sort_key()).cmp(&(b.0, b.1, b.2.sort_key())));
    clip_asterisk_overlap(&mut fmt);

    // The slice's edge whitespace runs, found on the content chars: the heading,
    // quote, or list prefix and the hard break's `\` land around them later. An
    // all-whitespace slice is one leading run.
    let lead_edge = chars
        .iter()
        .take_while(|c| edge_space_ref(**c).is_some())
        .count();
    let trail_edge = n - chars[lead_edge..]
        .iter()
        .rev()
        .take_while(|c| edge_space_ref(**c).is_some())
        .count();

    // `fmt` by start, longest span (outer) first at a tie. `pos` only ever
    // advances, so one cursor over this order opens every mark exactly once.
    // Built once here, not per sweep: the net below sweeps the same `fmt` up to
    // `PROBE_BUDGET` times.
    //
    // Two marks over the *same* span have no outer by length, and which one takes
    // the inside decides whether either survives: `*` pairs only against
    // non-punctuation, so `~~*a*~~` holds where `*~~a~~*` leaves both delimiters
    // as literal text. The asterisk family goes innermost, against the content it
    // needs; `~~` and `<u>` pair regardless of what abuts them.
    let ast_last = |k: &MarkKind| u8::from(matches!(k, MarkKind::Strong | MarkKind::Emph));
    let mut by_start: Vec<usize> = (0..fmt.len()).collect();
    by_start.sort_by(|&a, &b| {
        fmt[a]
            .0
            .cmp(&fmt[b].0)
            .then(fmt[b].1.cmp(&fmt[a].1))
            .then(ast_last(fmt[a].2).cmp(&ast_last(fmt[b].2)))
    });

    // An underline is a `<u>` tag pair. At one position its tags sit outside
    // the delimiters closing and opening there, so a tag's `<` and `>` never
    // abut a delimiter from inside its run, where they would change how it
    // flanks. An underline spanning the position stays open around the
    // delimiters, apart from their nesting.
    let is_underline = |fi: usize| matches!(fmt[fi].2, MarkKind::Underline);

    // A font mark is a `qm-span` pair, its tags outside the underlines' at a
    // position as theirs are outside the delimiters'. Font marks never overlap,
    // so neither do the pairs: one closes where the next opens.
    let is_font = |fi: usize| matches!(fmt[fi].2, MarkKind::FontFamily { .. });

    // One mark sweep over the marks `keep` selects (indices into `fmt`) → inline
    // markdown and its tag points.
    let sweep = |keep: &[bool], d: Delims| -> (String, Vec<usize>) {
        let mut out = String::new();
        let mut points = Vec::with_capacity(n + 1);
        // Marks currently open, outermost first, underlines and delimiters
        // apart. Storing the `fmt` index (not `(end, kind)`) keeps each open
        // mark's identity, so a reopened mark re-emits its OWN delimiter.
        let mut stack: Vec<usize> = Vec::new();
        let mut underlines: Vec<usize> = Vec::new();
        let mut font: Option<usize> = None;
        let (mut oi, mut li, mut ci) = (0usize, 0usize, 0usize);
        let mut pos = 0usize;
        let mut link: Option<OpenLink> = None;
        while pos <= n {
            // A link's text closes the marks it opened before its `](url)`.
            if let Some(open) = link.filter(|l| l.end == pos) {
                while stack.len() > open.stack {
                    out.push_str(delim(fmt[stack.pop().unwrap()].2, d));
                }
                while underlines.len() > open.underlines {
                    underlines.pop();
                    out.push_str("</u>");
                }
                if open.font
                    && let Some(fi) = font.take()
                {
                    out.push_str(&font_pair(fmt[fi].2).close_tag());
                }
                out.push_str("](");
                let mut dest = String::new();
                emit_url(open.url, &mut dest);
                push_piped(&dest, escape_pipe, &mut out);
                out.push(')');
                link = None;
            }
            // A mark whose start the cursor already passed was jumped over by
            // an atomic span's interior and never opens; clipping keeps that
            // set empty.
            while oi < by_start.len() && fmt[by_start[oi]].0 < pos {
                oi += 1;
            }
            let from = oi;
            while oi < by_start.len() && fmt[by_start[oi]].0 == pos {
                oi += 1;
            }
            while li < links.len() && links[li].0 < pos {
                li += 1;
            }
            let link_here = links
                .get(li)
                .copied()
                .filter(|l| l.0 == pos && link.is_none());
            let in_link = |fi: usize| {
                link_here.is_some_and(|(ls, le, _)| {
                    ls <= fmt[fi].0 && fmt[fi].1 <= le && (fmt[fi].0, fmt[fi].1) != (ls, le)
                })
            };
            let (inner, opening): (Vec<usize>, Vec<usize>) = by_start[from..oi]
                .iter()
                .copied()
                .filter(|&fi| keep[fi])
                .partition(|&fi| in_link(fi));
            let mut reopen: Vec<usize> = Vec::new();
            if let Some(idx) = stack.iter().position(|&fi| fmt[fi].1 == pos) {
                while stack.len() > idx {
                    let fi = stack.pop().unwrap();
                    out.push_str(delim(fmt[fi].2, d));
                    if fmt[fi].1 != pos {
                        reopen.push(fi);
                    }
                }
            }
            let mut reopen_underlines: Vec<usize> = Vec::new();
            if let Some(idx) = underlines.iter().position(|&fi| fmt[fi].1 == pos) {
                while underlines.len() > idx {
                    let fi = underlines.pop().unwrap();
                    out.push_str("</u>");
                    if fmt[fi].1 != pos {
                        reopen_underlines.push(fi);
                    }
                }
            }
            if let Some(fi) = font.filter(|&fi| fmt[fi].1 == pos) {
                out.push_str(&font_pair(fmt[fi].2).close_tag());
                font = None;
            }
            let point = out.len();
            points.push(link.map_or(point, |l| l.point));
            if let Some(fi) = opening.iter().copied().find(|&fi| is_font(fi)) {
                out.push_str(&font_pair(fmt[fi].2).open_tag());
                font = Some(fi);
            }
            let fresh = opening.iter().copied().filter(|&fi| is_underline(fi));
            for fi in reopen_underlines.into_iter().rev().chain(fresh) {
                out.push_str("<u>");
                underlines.push(fi);
            }
            // Open formatting marks starting here BEFORE any atomic run, so a
            // formatting mark beginning at the same position as inline code/link
            // still wraps it rather than being dropped.
            let fresh = opening
                .iter()
                .copied()
                .filter(|&fi| !is_underline(fi) && !is_font(fi));
            for fi in reopen.into_iter().rev().chain(fresh) {
                out.push_str(delim(fmt[fi].2, d));
                stack.push(fi);
            }
            // A link opens as `[`, its text sweeps with the marks inside it,
            // and the frame closes as `](url)` at its end. An island in its
            // text writes its markup (a linked image is `[![alt](src)](url)`).
            if let Some((ls, le, url)) = link_here {
                // `[^1][…](…)` reads the reference as text, so an empty
                // comment, which the import drops, parts the two.
                if ls > 0 && chars[ls - 1] == ISLAND_SLOT && out.ends_with(']') {
                    out.push_str("<!---->");
                }
                out.push('[');
                link = Some(OpenLink {
                    start: ls,
                    end: le,
                    url,
                    point,
                    stack: stack.len(),
                    underlines: underlines.len(),
                    font: false,
                });
                if let Some(fi) = inner.iter().copied().find(|&fi| is_font(fi)) {
                    out.push_str(&font_pair(fmt[fi].2).open_tag());
                    font = Some(fi);
                    link.as_mut().unwrap().font = true;
                }
                for fi in inner.iter().copied().filter(|&fi| is_underline(fi)) {
                    out.push_str("<u>");
                    underlines.push(fi);
                }
                for fi in inner
                    .iter()
                    .copied()
                    .filter(|&fi| !is_underline(fi) && !is_font(fi))
                {
                    out.push_str(delim(fmt[fi].2, d));
                    stack.push(fi);
                }
            }
            // A code range is atomic.
            while ci < code_ranges.len() && code_ranges[ci].0 < pos {
                ci += 1;
            }
            if let Some(&(cs, ce)) = code_ranges.get(ci).filter(|r| r.0 == pos) {
                let content: String = chars[cs..ce].iter().collect();
                let ticks = longest_backtick_run(&content) + 1;
                let fence = "`".repeat(ticks.max(1));
                // CommonMark folds an edge backtick into the fence and strips
                // one space off each side of a span that begins and ends with
                // one, unless it is all spaces; the pad defeats both.
                let pad = content.starts_with('`')
                    || content.ends_with('`')
                    || (content.starts_with(' ')
                        && content.ends_with(' ')
                        && content.chars().any(|c| c != ' '));
                out.push_str(&fence);
                if pad {
                    out.push(' ');
                }
                push_piped(&content, escape_pipe, &mut out);
                if pad {
                    out.push(' ');
                }
                out.push_str(&fence);
                points.resize(ce, link.map_or(point, |l| l.point));
                pos = ce;
                continue;
            }
            if pos < n {
                let c = chars[pos];
                if c == ISLAND_SLOT {
                    if let Some(slot) = island_markup_at(pos) {
                        out.push_str(&slot);
                    }
                } else if let Some(open) = link {
                    escape_char_into(c, pos == open.start, esc, &mut out);
                } else if Some(pos) == escape_punct_at
                    // A bare `!` before a link's `[` or a reference's `[^`
                    // would open an image.
                    || (c == '!' && links.get(li).is_some_and(|l| l.0 == pos + 1))
                    || (c == '!' && chars.get(pos + 1) == Some(&ISLAND_SLOT))
                    // After a footnote reference, `(` would read `[^1](…)` as
                    // a link and `:` would read `[^1]:` as a definition.
                    || (matches!(c, '(' | ':') && pos > 0 && chars[pos - 1] == ISLAND_SLOT && out.ends_with(']'))
                {
                    out.push('\\');
                    out.push(c);
                } else if let Some(space) = edge_space_ref(c)
                    && (pos < lead_edge || pos >= trail_edge)
                {
                    out.push_str(space);
                } else {
                    escape_char_into(c, pos == 0 && escape_leading_block, esc, &mut out);
                }
            }
            pos += 1;
        }
        // Clipping keeps every wrap `end` reachable, so this normally drains
        // nothing.
        while let Some(fi) = stack.pop() {
            out.push_str(delim(fmt[fi].2, d));
        }
        out.push_str(&"</u>".repeat(underlines.len()));
        if let Some(fi) = font {
            out.push_str(&font_pair(fmt[fi].2).close_tag());
        }
        (out, points)
    };

    // Verify-and-drop safety net. An editor's change can build a mark
    // over a span markdown can't represent, and it lowers to a `**`/`*`/`~~` run
    // pulldown re-reads as literal text, leaking a delimiter into the content.
    // Re-parse the rendered line and, if its plain text drifted, search for a set
    // of flanking marks that keeps the text intact. Underlines and font marks
    // are read back beside them and dropped the same way: one the line cannot
    // hold comes back as other markup or as nothing.
    let is_flanking = |k: &MarkKind| {
        matches!(
            k,
            MarkKind::Strong
                | MarkKind::Emph
                | MarkKind::Strike
                | MarkKind::Underline
                | MarkKind::FontFamily { .. }
        )
    };
    let all = vec![true; fmt.len()];
    if !fmt.iter().any(|m| is_flanking(m.2)) {
        return sweep(&all, DELIM_SPELLINGS[0]);
    }
    // The probe wraps the fragment in `,…,`: parsed standalone, a leading `0. ` /
    // `# ` / `> ` would read as a list/heading/quote marker and drop a good mark.
    // A punctuation sentinel blocks every leading-block construct, preserves edge
    // whitespace, and is flanking-equivalent to the line start/end it replaces,
    // so it never masks or invents a leak.
    //
    // A slot whose island has no markdown projection re-imports as nothing, so
    // the expected text drops it; every other slot stays, so the probe still
    // catches a leak that eats an island's markup. A dropped slot shortens every
    // position after it, so the same pass records where each char lands:
    // `kept[i]` is char `i`'s position in the probe, `kept[n]` the end.
    let mut expected = String::with_capacity(n);
    let mut kept: Vec<Scalar> = Vec::with_capacity(n + 1);
    let mut at = 1;
    for (i, &c) in chars.iter().enumerate() {
        kept.push(at);
        if c != ISLAND_SLOT || island_markup_at(i).is_some() {
            expected.push(c);
            at += 1;
        }
    }
    kept.push(at);
    let want = format!(",{expected},");
    // The flanking marks a `keep` set asks for, in the probe's coordinates and the
    // form re-import hands back: a mark the sweep splits around an overlap
    // re-imports as one span, so both sides are normalized before they are compared.
    let want_marks = |keep: &[bool]| {
        crate::lines::model::normalize_marks(
            fmt.iter()
                .enumerate()
                .filter(|&(i, m)| keep[i] && is_flanking(m.2))
                .filter_map(|(_, &(s, e, k))| {
                    Some(crate::lines::model::Mark::new(
                        kept[s],
                        kept[e],
                        k.to_line()?,
                    ))
                })
                .collect(),
        )
    };
    // `None` where the rendering leaks a delimiter into the text, else whether the
    // marks it was asked to carry came back as themselves. A mark that re-imports
    // over different text is not the mark the content held: `**a**_b_**c**` lowers
    // to a `***` run CommonMark re-segments into one `Strong` over all three spans,
    // spending no character and moving where bold starts and ends.
    //
    // A cell fragment is read back as a cell and a note as a note, where `<br>`
    // is a `\n` wherever it sits; prose reads a break at a mark edge or a
    // second one in a row differently.
    let probe = |md: &str, want_marks: &[crate::lines::model::Mark]| -> Option<bool> {
        let (text, marks) = match esc.frag {
            Frag::Cell => {
                let rt =
                    crate::lines::import::from_markdown(&format!("| h |\n| --- |\n| ,{md}, |"))
                        .ok()?
                        .content;
                let cell = rt.islands.first()?.props.get("rows")?.get(0)?.get(0)?;
                crate::lines::serial::parse_cell(cell)
            }
            Frag::HtmlCell => {
                let rt = crate::lines::import::from_markdown(&format!(
                    "<table><tr><td>,{md},</td></tr></table>"
                ))
                .ok()?
                .content;
                crate::lines::serial::parse_cell(rt.islands.first()?.props.get("header")?.get(0)?)
            }
            Frag::Note => {
                let rt = crate::lines::import::from_markdown(&format!("a[^1]\n\n[^1]: ,{md},"))
                    .ok()?
                    .content;
                crate::lines::serial::parse_cell(&rt.islands.first()?.props)
            }
            Frag::Prose => {
                let rt = crate::lines::import::from_markdown(&format!(",{md},{probe_tail}"))
                    .ok()?
                    .content
                    .into_content();
                (rt.text, rt.marks)
            }
        };
        if text != want {
            return None;
        }
        let got: Vec<&crate::lines::model::Mark> = marks
            .iter()
            .filter(|m| MarkKind::from_line(&m.kind).is_some_and(|k| is_flanking(&k)))
            .collect();
        Some(got.into_iter().eq(want_marks.iter()))
    };
    // Re-spell before dropping anything. The leak is rarely an unrepresentable
    // span; it is two same-character delimiters abutting: a `Strong` closing `**`
    // against the `*` of an `Emph` that starts where it ends makes a `***` run,
    // and CommonMark re-segments that run rather than pairing it as written.
    // Markdown spells both kinds a second way, and `__a\*\*__*b*` carries what
    // `**a\*\***​*b*` loses — same content, delimiters that cannot merge. `**`/`*`
    // is swept first and returned whole on a line it already round-trips, so the
    // `_` forms are reached only where the leader is *shown* to lose something,
    // and each is verified before it is used. Four probes at most.
    let intent = want_marks(&all);
    for &d in &DELIM_SPELLINGS {
        let cand = sweep(&all, d);
        if probe(&cand.0, &intent) == Some(true) {
            return cand;
        }
    }
    // The flanking marks in document order. That order is the re-add priority
    // below (when two marks can't both survive, the earlier one wins) and is the
    // one user-visible choice in this search, so it is fixed here rather than
    // falling out of the traversal.
    let cands: Vec<usize> = (0..fmt.len()).filter(|&i| is_flanking(fmt[i].2)).collect();
    // Render with only the flanking marks in `keep`; every other mark rides.
    // The default spelling: a line reaching here has had all four rejected with
    // every mark on, so the choice is which marks survive, not how they are spelled.
    let mask_of = |keep: &[usize]| -> Vec<bool> {
        let mut mask: Vec<bool> = fmt.iter().map(|m| !is_flanking(m.2)).collect();
        for &i in keep {
            mask[i] = true;
        }
        mask
    };
    let render = |keep: &[usize]| sweep(&mask_of(keep), DELIM_SPELLINGS[0]);
    let survives = |md: &str, keep: &[usize]| probe(md, &want_marks(&mask_of(keep))) == Some(true);
    // Drop the whole flanking set, then re-add by halves: a chunk that survives is
    // accepted whole, one that doesn't splits and its halves are retried, a lone
    // mark that still doesn't is dropped. `m` marks cost ~2·log(m) probes when one
    // is at fault and 2 when none can survive. Greedy, so the result is a maximal
    // surviving set, not necessarily the largest one.
    //
    // Dropping every flanking mark is the floor the search can't go below, so if
    // even that fails, no re-add can fix it. A lone candidate has nowhere to
    // split, so the floor is already its answer.
    let mut out = render(&[]);
    if cands.len() == 1 || !survives(&out.0, &[]) {
        return out;
    }
    let mut kept: Vec<usize> = Vec::new();
    // A chunk `(lo, hi)` and the `kept` length at which its trial is *already
    // known to fail*: a right half popped with every mark of its left sibling
    // accepted re-renders exactly the chunk their parent failed on, so it splits
    // without spending a probe.
    let mid = cands.len() / 2;
    let mut work: Vec<(usize, usize, usize)> = vec![(mid, cands.len(), mid), (0, mid, usize::MAX)];
    let mut budget = PROBE_BUDGET;
    while let Some((lo, hi, known_bad_at)) = work.pop() {
        let split = |work: &mut Vec<_>, kept_len: usize| {
            if hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                work.push((mid, hi, kept_len + (mid - lo)));
                work.push((lo, mid, usize::MAX));
            }
        };
        if known_bad_at == kept.len() {
            split(&mut work, kept.len());
            continue;
        }
        if budget == 0 {
            break;
        }
        budget -= 1;
        // `work` is a left-to-right DFS, so every accepted mark precedes this
        // chunk and `trial` stays in document order.
        let trial: Vec<usize> = kept.iter().chain(&cands[lo..hi]).copied().collect();
        let md = render(&trial);
        if survives(&md.0, &trial) {
            kept = trial;
            out = md;
        } else {
            split(&mut work, kept.len());
        }
    }
    out
}

/// A link the sweep is inside: its text's range, its url, the tag point of
/// its start, and the marks open outside it, which its `](url)` closes after.
#[derive(Clone, Copy)]
struct OpenLink<'u> {
    start: usize,
    end: usize,
    url: &'u str,
    point: usize,
    stack: usize,
    underlines: usize,
    font: bool,
}

/// Probes the verify-and-drop net will spend on one line before giving up and
/// dropping the flanking marks it hasn't cleared. Resolving `m` of them exactly
/// costs at most `2m-2` probes, so at 64 no line carrying 32 or fewer loses a
/// mark to the budget. The ceiling keeps export cost linear in document size
/// whatever marks are thrown at it.
pub(crate) const PROBE_BUDGET: usize = 64;

/// [`clip_range_to_atomic`] over every wrapping mark, dropping the ones an
/// atomic span swallowed whole.
fn clip_fmt_to_atomic(fmt: &mut Vec<(usize, usize, &MarkKind)>, atomics: &[(usize, usize)]) {
    for m in fmt.iter_mut() {
        clip_range_to_atomic(&mut m.0, &mut m.1, atomics);
    }
    fmt.retain(|m| m.0 < m.1);
}

/// The atomic-balance rule for a single `[*start, *end)` range against a set of
/// atomic spans. A range edge landing strictly inside an atomic `[cs, ce)` span
/// is pulled to that span's boundary (`start`→`ce`, `end`→`cs`); an edge outside
/// every span is untouched, and a range that strictly *contains* a span keeps
/// both edges. Applying the spans in sequence is order-independent across
/// ranges, so a caller may loop ranges or spans on the outside; a range whose
/// edges cross after clipping was swallowed whole and is left empty for the
/// caller to drop. Shared with the Typst backend's inline emitter.
pub fn clip_range_to_atomic(start: &mut usize, end: &mut usize, atomics: &[(usize, usize)]) {
    for &(cs, ce) in atomics {
        if cs < *start && *start < ce {
            *start = ce;
        }
        if cs < *end && *end < ce {
            *end = cs;
        }
    }
}

/// The maximal subranges of `[start, end)` free of [`ISLAND_SLOT`] and `\n`:
/// the range itself when it covers neither, else one range per run between
/// them (empty runs dropped). Used to keep a code span off an island or a cell
/// break it cannot represent.
fn split_around_slots(chars: &[char], start: usize, end: usize) -> Vec<(usize, usize)> {
    let cut = |c: char| c == ISLAND_SLOT || c == '\n';
    let end = end.min(chars.len());
    if !chars[start.min(end)..end].iter().any(|&c| cut(c)) {
        return vec![(start, end)];
    }
    let mut out = Vec::new();
    let mut run = start;
    for (i, _) in chars[start..end]
        .iter()
        .enumerate()
        .map(|(i, c)| (start + i, c))
        .filter(|&(_, &c)| cut(c))
    {
        if run < i {
            out.push((run, i));
        }
        run = i + 1;
    }
    if run < end {
        out.push((run, end));
    }
    out
}

/// The pieces of the code range `start..end` of `chars` outside each of the
/// HTML table subset's tags its text holds.
fn split_around_table_tags(chars: &[char], start: usize, end: usize) -> Vec<(usize, usize)> {
    let text: String = chars[start..end].iter().collect();
    let mut at = start;
    let mut out = Vec::new();
    for (piece, tag) in crate::lines::html_table::tag_pieces(&text) {
        let len = text[piece].chars().count();
        if !tag {
            out.push((at, at + len));
        }
        at += len;
    }
    out
}

/// Nest `strong`/`emph` marks that partially overlap, by truncating the
/// later-opening one to its enclosing sibling's end. Both render as runs of the
/// same character, so a reopened `*` abutting a `**` would merge into an
/// ambiguous `***`. Truncation keeps the nested portion of both marks, dropping
/// only the crossing tail. Marks with distinct delimiters are left to the
/// sweep's close-and-reopen, which round-trips them exactly.
fn clip_asterisk_overlap(fmt: &mut [(usize, usize, &MarkKind)]) {
    let is_ast = |k: &MarkKind| matches!(k, MarkKind::Strong | MarkKind::Emph);
    // Asterisk-family marks, outermost first (start asc, then longer span first).
    let mut idx: Vec<usize> = (0..fmt.len()).filter(|&i| is_ast(fmt[i].2)).collect();
    idx.sort_by(|&a, &b| fmt[a].0.cmp(&fmt[b].0).then(fmt[b].1.cmp(&fmt[a].1)));
    // Ends of the enclosing ancestors still open at the current mark's start.
    let mut open_ends: Vec<usize> = Vec::new();
    for &i in &idx {
        let (s, mut e, _) = fmt[i];
        while open_ends.last().is_some_and(|&end| end <= s) {
            open_ends.pop();
        }
        if let Some(&parent_end) = open_ends.last() {
            if parent_end < e {
                e = parent_end;
                fmt[i].1 = e;
            }
        }
        open_ends.push(e);
    }
}

/// A table cell's markdown, rebuilt from its slot: the prose mark sweep, in a
/// pipe table with `|`→`\|` escaping so the cell survives re-import through
/// `pulldown`'s pipe splitting. A cell is flat inline: no islands, no
/// leading-block escape.
fn render_cell_md(slot: &Content, frag: Frag) -> String {
    let (text, marks, _) = cell(slot);
    let chars: Vec<char> = text.chars().collect();
    let (code_ranges, fmt, links) = bucket_marks(&marks, 0, chars.len(), true);
    let esc = Esc {
        frag,
        dollar: escapes_dollar(text.matches('$').count(), 0),
    };
    render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        None,
        false,
        esc,
        "",
        |_| None,
    )
    .0
}

/// A footnote's note as the text of its definition: a cell's sweep on one
/// source line, opening a block as a paragraph line does.
fn render_note_md(isl: &Island) -> String {
    let empty = Content::empty();
    let (text, marks, _) = cell(isl.slots.get(NOTE_SLOT).unwrap_or(&empty));
    let chars: Vec<char> = text.chars().collect();
    let (code_ranges, fmt, links) = bucket_marks(&marks, 0, chars.len(), true);
    let esc = Esc {
        frag: Frag::Note,
        dollar: escapes_dollar(text.matches('$').count(), 0),
    };
    let punct = list_marker_punct(&chars);
    render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        punct,
        true,
        esc,
        "",
        |_| None,
    )
    .0
}

/// What a fragment [`render_marked_core`] renders is, and so the escapes it
/// takes and how its net reads it back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Frag {
    Prose,
    /// A pipe table's cell: `|` escaped, a `\n` a `<br>`.
    Cell,
    /// An HTML table's cell: a `\n` a `<br>`.
    HtmlCell,
    /// A footnote's note: a `\n` a `<br>`.
    Note,
}

/// The escapes a fragment's text takes: its [`Frag`]'s, and `$` as `\$`
/// where `dollar`.
#[derive(Clone, Copy)]
struct Esc {
    frag: Frag,
    dollar: bool,
}

/// Which of markdown's two spellings each asterisk-family kind is emitted with.
/// The pair is chosen per line, not per mark: a run is only ambiguous where two
/// delimiters *of the same character* abut, so swapping one kind's spelling is
/// what breaks the run up.
#[derive(Clone, Copy)]
struct Delims {
    strong: &'static str,
    emph: &'static str,
}

/// The spellings the net tries, in order. `**`/`*` leads because `_` cannot do
/// intraword emphasis (CommonMark flanking), so `_a_你` re-imports as literal
/// text where `*a*你` emphasizes correctly; the rest are reached only once the
/// leader has been *shown* to lose text, and each is verified before it is used.
const DELIM_SPELLINGS: [Delims; 4] = [
    Delims {
        strong: "**",
        emph: "*",
    },
    Delims {
        strong: "__",
        emph: "*",
    },
    Delims {
        strong: "**",
        emph: "_",
    },
    Delims {
        strong: "__",
        emph: "_",
    },
];

/// The `qm-span` pair a `font_family` mark writes.
fn font_pair(kind: &MarkKind) -> crate::lines::carrier::Element {
    match kind {
        MarkKind::FontFamily { name } => crate::lines::carrier::span::pair(name),
        _ => unreachable!("only a font_family mark is a qm-span pair"),
    }
}

/// The delimiter that opens and closes `kind`. Code, link, anchor, underline
/// and font family are written elsewhere.
fn delim(kind: &MarkKind, d: Delims) -> &'static str {
    match kind {
        MarkKind::Strong => d.strong,
        MarkKind::Emph => d.emph,
        MarkKind::Strike => "~~",
        _ => "",
    }
}

/// The character reference for a space or tab, the whitespace markdown strips
/// from the edges of a line and of a table cell. Backslash escapes cover ASCII
/// punctuation only, so an edge run has no escaped spelling; the reference
/// re-imports as the character and renders as the character. Every other
/// whitespace character, and either of these away from an edge, survives
/// verbatim.
fn edge_space_ref(c: char) -> Option<&'static str> {
    match c {
        ' ' => Some("&#32;"),
        '\t' => Some("&#9;"),
        _ => None,
    }
}

/// Push `s`, a code span's content or a link's destination, with each `|`
/// as `\|` where `escape_pipe`: a table row splits its cells at a `|` inside
/// either, and reads `\|` as `|` before it parses the cell.
fn push_piped(s: &str, escape_pipe: bool, out: &mut String) {
    if escape_pipe {
        out.push_str(&s.replace('|', "\\|"));
    } else {
        out.push_str(s);
    }
}

fn escape_run(chars: &[char], esc: Esc) -> String {
    let mut s = String::new();
    for (i, c) in chars.iter().enumerate() {
        escape_char_into(*c, i == 0, esc, &mut s);
    }
    s
}

/// Push `c` into `out` escaped so it re-imports as literal text: the char
/// verbatim, or a `&'static str` escape. `leading` also escapes block-starter
/// chars that would otherwise open a heading/list/quote. A cell adds
/// `|`→`\|`, and a cell or note `\n`→`<br>`, so each survives on its one
/// source line; `esc.dollar` adds `$`→`\$`.
fn escape_char_into(c: char, leading: bool, esc: Esc, out: &mut String) {
    let esc: &str = match c {
        '\\' => "\\\\",
        '*' => "\\*",
        '_' => "\\_",
        '`' => "\\`",
        '[' => "\\[",
        ']' => "\\]",
        '<' => "\\<",
        '~' => "\\~",
        // `&` starts a CommonMark entity reference, decoded on re-import, so
        // `&word;`-shaped text would collapse to the entity's character. Always
        // escaped: detecting "would form an entity" is not worth the fragility.
        '&' => "\\&",
        '|' if esc.frag == Frag::Cell => "\\|",
        '\n' if esc.frag != Frag::Prose => "<br>",
        '$' if esc.dollar => "\\$",
        '#' if leading => "\\#",
        '>' if leading => "\\>",
        '-' if leading => "\\-",
        '+' if leading => "\\+",
        // A leading `=` run underlines the paragraph line above it into a setext
        // heading, so only a continuation line can start one. One escaped `=`
        // defeats the whole underline.
        '=' if leading => "\\=",
        other => {
            out.push(other);
            return;
        }
    };
    out.push_str(esc);
}

fn longest_backtick_run(s: &str) -> usize {
    let mut max = 0;
    let mut run = 0;
    for c in s.chars() {
        if c == '`' {
            run += 1;
            max = max.max(run);
        } else {
            run = 0;
        }
    }
    max
}
