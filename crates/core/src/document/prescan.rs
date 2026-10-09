//! Pre-scan of a card-yaml block's YAML payload to recover what the value parse
//! discards: comments and tags. One pass over the parser's event stream gives
//! each comment its container and slot, and each tag its node.
//!
//! Top-level comments become [`super::PayloadItem::Comment`]. Comments inside
//! block mappings and sequences are captured with their structural path and an
//! ordinal, which the emitter re-injects at (see [`NestedComment`]). A comment
//! inside a flow collection or a multi-line scalar belongs to the block entry
//! holding that value: its trailer, or an own-line comment after it.
//!
//! A mapping holding a merge (`<<`) reads its own keys, then each key the merge
//! brings that it does not already hold. Once it closes, its comments are placed
//! again among those keys: one ahead of, on or inside a merge sits with the
//! keys that merge brings.
//!
//! Every tagged node is recorded at its path in the value, for the assembler to
//! warn on; the value parse applies a core `!!` tag, ignores any other, and
//! keeps no tag.

use std::collections::{HashMap, HashSet};

use serde_saphyr::granit_parser::{
    Event, Marker, Options, Parser, Placement, ScalarStyle, Span, StructureStyle, Tag,
};

use crate::value::PathSegment;

/// One ordered hint extracted from the fence body. `Field` captures only the
/// key; the value comes from the value parse. An inline `Comment` immediately
/// follows its host `Field` in the item stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreItem {
    Field { key: String },
    Comment { text: String, inline: bool },
}

/// A comment inside a nested mapping or sequence.
///
/// `container_path` locates the immediate parent. For own-line comments
/// (`inline = false`), `position` is the child slot ordinal (`0..=child_count`,
/// where `child_count` means "after all children"). For inline comments
/// (`inline = true`), `position` is the host child's index; orphaned inlines
/// degrade to own-line at emit time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedComment {
    pub container_path: Vec<PathSegment>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Output of [`prescan_fence_content`].
#[derive(Debug, Clone, Default)]
pub(crate) struct PreScan {
    /// Top-level fields and comments, in the order `to_markdown` writes them.
    pub items: Vec<PreItem>,
    /// In the order `to_markdown` writes them.
    pub nested_comments: Vec<NestedComment>,
    /// Paths of the tagged nodes, relative to the fence root (the first
    /// segment is the owning top-level key).
    pub unsupported_tags: Vec<Vec<PathSegment>>,
}

/// The paths a block's comments and tags record outgrew [`budget`].
#[derive(Debug)]
pub(crate) struct OverBudget {
    pub(crate) budget: usize,
}

/// Why the scan refuses a block the value parse reads.
#[derive(Debug)]
pub(crate) enum Refusal {
    OverBudget(OverBudget),
    /// Text other than comments and a `...` follows the root node, which the
    /// value parse reads alone; at this 1-indexed line and column.
    PastRoot { line: usize, column: usize },
    /// A mapping's second key of this text, which the value parse folds into
    /// the first, such as `"1"` after `1`; at this 1-indexed line and column.
    SharedKey {
        key: String,
        line: usize,
        column: usize,
    },
}

impl From<OverBudget> for Refusal {
    fn from(over: OverBudget) -> Self {
        Refusal::OverBudget(over)
    }
}

impl Refusal {
    fn past_root(at: Marker) -> Self {
        Refusal::PastRoot {
            line: at.line(),
            column: at.col() + 1,
        }
    }
}

/// Bytes of recorded path a block of `len` bytes may hold. Each comment clones
/// its container's path, so many comments under long keys would otherwise
/// grow with the square of the input.
pub(crate) fn budget(len: usize) -> usize {
    len.saturating_mul(64).saturating_add(64 * 1024)
}

/// Scan `yaml`, which the value parse reads. That parse stops at the end of
/// the root node, and the scan refuses anything past it but comments and a
/// `...`: a parser error there, or a second document. It also refuses a
/// mapping's second key of one text, which the value parse folds into the
/// first.
pub(crate) fn prescan_fence_content(yaml: &str) -> Result<PreScan, Refusal> {
    let mut walk = Walk::new(yaml);
    let mut ended = false;
    for next in Parser::new_from_str_with_options(yaml, options()) {
        let (event, span) = next.map_err(|refusal| Refusal::past_root(*refusal.marker()))?;
        match event {
            Event::DocumentEnd => ended = true,
            Event::DocumentStart(..) if ended => return Err(Refusal::past_root(span.start)),
            _ => {}
        }
        walk.step(&event, span)?;
    }
    Ok(walk.finish()?)
}

/// The parser options the scan reads with, which refuse no text the value
/// parse reads. That parse counts no comments, so neither does this one.
fn options() -> Options {
    let mut options = Options::default();
    options.max_buffered_comment_events = usize::MAX;
    options
}

/// What a node event starts, as far as comments care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    BlockMapping,
    BlockSequence,
    Flow,
    /// A scalar with no source text that reads as null: a key's or a dash's
    /// missing value.
    Absent,
    Scalar,
}

impl Shape {
    /// `reads_null` answers for a tagged scalar with no source text.
    fn of(event: &Event<'_>, span: &Span, reads_null: impl FnOnce(&Tag) -> bool) -> Self {
        match event {
            Event::MappingStart(StructureStyle::Block, ..) => Shape::BlockMapping,
            Event::SequenceStart(StructureStyle::Block, ..) => Shape::BlockSequence,
            Event::MappingStart(..) | Event::SequenceStart(..) => Shape::Flow,
            Event::Scalar(_, ScalarStyle::Plain, _, tag)
                if span.start.index() == span.end.index()
                    && tag.as_deref().is_none_or(reads_null) =>
            {
                Shape::Absent
            }
            _ => Shape::Scalar,
        }
    }

    fn is_block(self) -> bool {
        matches!(self, Shape::BlockMapping | Shape::BlockSequence)
    }
}

/// Where a node sits in the collection holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Root,
    Key,
    Value,
    Item,
}

#[derive(Debug)]
struct Comment {
    text: String,
    line: usize,
    column: usize,
    offset: usize,
    /// How many collections were open when it arrived: one closed before it
    /// cannot hold it.
    open: usize,
}

#[derive(Debug)]
struct Frame {
    sequence: bool,
    flow: bool,
    count: usize,
    /// A mapping whose current key awaits its value.
    awaiting_value: bool,
    /// The key column of the mapping entry holding this collection, past which
    /// a comment is inside it.
    held_at: Option<usize>,
    /// For a sequence item's collection, the column of its first key or dash:
    /// a comment at or past it after the last child is inside it.
    column: usize,
    /// Own-line comments sit ahead of its first key.
    led: bool,
    /// The child the last node started.
    entry: Option<Entry>,
    /// A trailer is recorded for the item the next node starts.
    next_trailed: bool,
    /// Each child of a mapping, and each item of a sequence that collects.
    children: Vec<Child>,
    /// The text of each own key of a mapping.
    keys: HashSet<String>,
    /// A sequence keeping what each item brings: a merge's value, an item of
    /// one that collects, or one an anchor names.
    collects: bool,
    /// It is written inside a merge's value: the mapping holding the merge
    /// places its comments.
    merged: bool,
    /// The anchor it defines, `0` for none.
    anchor: usize,
    /// Where its comments start among `Walk::nested`.
    first: usize,
}

#[derive(Debug)]
struct Entry {
    index: usize,
    segment: PathSegment,
    /// The column of a mapping entry's key.
    column: usize,
    line: usize,
    /// A comment trails it or follows it, so the next one follows it.
    trailed: bool,
    /// Its value is absent or an empty flow collection, which holds the
    /// comments indented past its key.
    empty: bool,
}

/// Where a comment in a run of own-line comments may land, deepest first.
#[derive(Debug, Clone, Copy)]
enum Slot {
    /// Inside the empty value of `frames[.0]`'s current entry, whose key sits
    /// at column `.1`.
    Under(usize, usize),
    /// After the last child of a collection the run closes.
    Last(usize),
    /// Ahead of the next child of the collection that continues.
    Next(usize),
}

/// Where a comment sits among the children of the collection holding it.
/// `After(i)` and `Ahead(i + 1)` record one slot; they part where a merge moves
/// a mapping's keys away from its children's order.
#[derive(Debug, Clone, Copy)]
enum At {
    /// Ahead of the child at this position, or after the last when none is.
    Ahead(usize),
    /// After the child at this position.
    After(usize),
    /// The trailer of the child at this position.
    Trailer(usize),
}

impl At {
    /// The position and inline flag a [`NestedComment`] records.
    fn slot(self) -> (usize, bool) {
        match self {
            At::Ahead(position) => (position, false),
            At::After(index) => (index + 1, false),
            At::Trailer(index) => (index, true),
        }
    }
}

/// A comment inside a collection, beside its place in emit order. Comments
/// sharing a slot emit in source order.
#[derive(Debug)]
struct Recorded {
    order: Vec<usize>,
    at: At,
    offset: usize,
    comment: NestedComment,
}

/// What an anchor names, as far as a key or a merge reads it.
#[derive(Debug)]
enum Anchored {
    /// A scalar's text, and whether it spells the merge key.
    Scalar(String, bool),
    /// The keys a collection brings to a merge, each once.
    Keys(Vec<String>),
}

/// A child of a mapping, or an item of a sequence that collects.
#[derive(Debug)]
enum Child {
    /// An entry's own key.
    Key(String),
    /// What a merge entry's value, or the item, brings.
    Merged(Flat),
}

/// A collection's keys as the value parse reads them through merges: a
/// mapping's own keys, then what each of its merges brings, and a sequence's
/// items' keys in turn.
#[derive(Debug, Default)]
struct Flat {
    keys: Vec<String>,
    /// Each child's place among `keys`, in source order.
    parts: Vec<Part>,
}

#[derive(Debug)]
enum Part {
    /// An own key, at this offset.
    Key(usize),
    /// What a merge's value or a sequence item brings, over `start..end`.
    Merged {
        start: usize,
        end: usize,
        parts: Vec<Part>,
    },
}

/// Where a comment inside a mapping holding a merge lands among its keys.
#[derive(Debug)]
enum Landing {
    /// Ahead of the key at this offset, or after the last.
    Ahead(usize),
    /// The trailer of the key at this offset.
    Trailer(usize),
    /// Inside the value of the key at this offset, its container that many
    /// levels below the mapping.
    Inside(usize, usize),
}

impl Flat {
    fn of(children: Vec<Child>) -> Self {
        let own = children.iter().filter(|c| matches!(c, Child::Key(_))).count();
        let mut keys = Vec::with_capacity(own);
        let mut merged = Vec::new();
        let parts = children
            .into_iter()
            .map(|child| match child {
                Child::Key(key) => {
                    keys.push(key);
                    Part::Key(keys.len() - 1)
                }
                Child::Merged(mut flat) => {
                    let start = own + merged.len();
                    merged.append(&mut flat.keys);
                    Part::Merged {
                        start,
                        end: own + merged.len(),
                        parts: flat.parts,
                    }
                }
            })
            .collect();
        keys.append(&mut merged);
        Self { keys, parts }
    }

    /// What a merge's value or a collecting sequence's item brings when
    /// `event`, its node, is an alias or a scalar. A collection's keys come in
    /// when it closes.
    fn of_node(event: &Event<'_>, anchors: &HashMap<usize, Anchored>) -> Self {
        match event {
            Event::Alias(id) => match anchors.get(id) {
                Some(Anchored::Keys(keys)) => Self {
                    keys: keys.clone(),
                    parts: Vec::new(),
                },
                _ => Self::default(),
            },
            _ => Self::default(),
        }
    }
}

impl Part {
    fn start(&self) -> usize {
        match *self {
            Part::Key(offset) => offset,
            Part::Merged { start, .. } => start,
        }
    }

    fn end(&self) -> usize {
        match *self {
            Part::Key(offset) => offset + 1,
            Part::Merged { end, .. } => end,
        }
    }
}

/// Where a comment recorded `rest` below a collection, at `at`, lands among
/// the `len` keys its children's `parts` bring.
fn land(parts: &[Part], len: usize, rest: &[usize], at: At) -> Landing {
    let ahead = |position: usize| parts.get(position).map_or(len, Part::start);
    match *rest {
        [child, 2, ref below @ ..] => match parts.get(child) {
            Some(&Part::Key(offset)) => Landing::Inside(offset, 1),
            Some(&Part::Merged {
                start,
                end,
                parts: ref inner,
            }) => match land(inner, end - start, below, at) {
                Landing::Ahead(offset) => Landing::Ahead(start + offset),
                Landing::Trailer(offset) => Landing::Trailer(start + offset),
                Landing::Inside(offset, levels) => Landing::Inside(start + offset, levels + 1),
            },
            None => Landing::Ahead(len),
        },
        _ => match at {
            At::Trailer(index) => match parts.get(index) {
                Some(&Part::Key(offset)) => Landing::Trailer(offset),
                _ => Landing::Ahead(ahead(index)),
            },
            At::Ahead(position) => Landing::Ahead(ahead(position)),
            At::After(index) => Landing::Ahead(parts.get(index).map_or(len, Part::end)),
        },
    }
}

/// For each offset into `keys`, how many keys ahead of it the value holds, and
/// one more for the end: the value holds the first key of each text, so the
/// one at `o` when `seat[o + 1] > seat[o]`.
fn seats(keys: &[String]) -> Vec<usize> {
    let mut seen = HashSet::new();
    let mut seat = Vec::with_capacity(keys.len() + 1);
    let mut held = 0;
    seat.push(held);
    for key in keys {
        held += usize::from(seen.insert(key.as_str()));
        seat.push(held);
    }
    seat
}

/// Move `r`, recorded inside the mapping at `chain` and `path` that holds a
/// merge, into the value of the key of `flat` it sits inside, or answer the
/// slot of the mapping it lands on. The comments on and inside a key the value
/// does not hold land ahead of where it would sit.
fn reseat(
    r: &mut Recorded,
    chain: &[usize],
    path: &[PathSegment],
    flat: &Flat,
    seat: &[usize],
) -> Option<(usize, bool)> {
    let held = |offset: usize| seat[offset + 1] > seat[offset];
    match land(&flat.parts, flat.keys.len(), &r.order[chain.len()..], r.at) {
        Landing::Inside(offset, levels) if held(offset) => {
            let mut order = chain.to_vec();
            order.extend([seat[offset], 2]);
            order.extend_from_slice(&r.order[chain.len() + 2 * levels..]);
            let mut container = path.to_vec();
            container.push(PathSegment::Key(flat.keys[offset].clone()));
            container.extend_from_slice(&r.comment.container_path[path.len() + levels..]);
            r.order = order;
            r.comment.container_path = container;
            None
        }
        Landing::Trailer(offset) if held(offset) => Some((seat[offset], true)),
        Landing::Ahead(offset) | Landing::Trailer(offset) | Landing::Inside(offset, _) => {
            Some((seat[offset], false))
        }
    }
}

/// Whether a key spells the merge key: `<<` plain and untagged, or under
/// `!!merge`.
fn is_merge_key(text: &str, style: ScalarStyle, tag: Option<&Tag>) -> bool {
    text == "<<"
        && match tag {
            Some(tag) => tag
                .suffix_in_namespace("tag:yaml.org,2002:")
                .is_some_and(|suffix| suffix == "merge"),
            None => style == ScalarStyle::Plain,
        }
}

struct Walk<'a> {
    src: &'a str,
    budget: usize,
    left: usize,
    items: Vec<PreItem>,
    /// Each comment inside a collection, in the order recorded.
    nested: Vec<Recorded>,
    tags: Vec<Vec<PathSegment>>,
    /// Open collections, outermost first, then the ones a run of own-line
    /// comments closed before it found its slots.
    frames: Vec<Frame>,
    live: usize,
    pending: Vec<Comment>,
    /// A trailing comment on a line whose syntax has no event yet: the first
    /// dash of a sequence at its key's column.
    held: Option<Comment>,
    last_line: usize,
    /// A block scalar's header trailer arrives after the scalar, from before
    /// its start.
    last_start: usize,
    last_end: usize,
    /// Byte ranges of the comments past the last event holding source text.
    gap: Vec<(usize, usize)>,
    /// Whether an empty plain scalar reads as null, by the tag on it.
    nulls: HashMap<String, bool>,
    /// What each anchor names, by anchor id.
    anchors: HashMap<usize, Anchored>,
    /// Where the root collection's items start among `items`.
    root_start: usize,
    /// Each comment on a root slot: the slot, and the comment's offset.
    root_marks: Vec<(At, usize)>,
}

fn byte(marker: Marker) -> usize {
    marker.byte_offset().unwrap_or(0)
}

/// The text after `#`, less any further `#`, one space, and the whitespace
/// ending it: the block's text is trimmed, so the comment closing it keeps
/// none.
fn comment_text(raw: &str) -> String {
    let after = raw.trim_start_matches('#');
    after.strip_prefix(' ').unwrap_or(after).trim_end().to_string()
}

impl<'a> Walk<'a> {
    fn new(src: &'a str) -> Self {
        let budget = budget(src.len());
        Self {
            src,
            budget,
            left: budget,
            items: Vec::new(),
            nested: Vec::new(),
            tags: Vec::new(),
            frames: Vec::new(),
            live: 0,
            pending: Vec::new(),
            held: None,
            last_line: 0,
            last_start: 0,
            last_end: 0,
            gap: Vec::new(),
            nulls: HashMap::new(),
            anchors: HashMap::new(),
            root_start: 0,
            root_marks: Vec::new(),
        }
    }

    /// Whether the value parse reads an empty plain scalar under `tag` as null,
    /// as it does one with no tag: `!!str` and `!` read it as text. The probe
    /// spells the tag verbatim, which resolves without the block's `%TAG` lines.
    fn reads_null(&mut self, tag: &Tag) -> bool {
        *self.nulls.entry(format!("!<{tag}>")).or_insert_with_key(|probe| {
            matches!(
                crate::value::parse_yaml::<serde_json::Value>(probe),
                Ok(serde_json::Value::Null)
            )
        })
    }

    fn step(&mut self, event: &Event<'_>, span: Span) -> Result<(), Refusal> {
        match event {
            Event::Comment(text, placement) => Ok(self.comment(text, *placement, span)?),
            Event::Scalar(..)
            | Event::Alias(..)
            | Event::SequenceStart(..)
            | Event::MappingStart(..) => self.node(event, span),
            Event::SequenceEnd | Event::MappingEnd => Ok(self.end(span)?),
            _ => Ok(()),
        }
    }

    fn finish(mut self) -> Result<PreScan, OverBudget> {
        self.pending.extend(self.held.take());
        let run = std::mem::take(&mut self.pending);
        if self.live == 0 {
            for c in run {
                self.record(0, At::Ahead(0), c)?;
            }
        } else {
            self.place_run(run)?;
        }
        self.nested.sort_by(|a, b| (&a.order, a.offset).cmp(&(&b.order, b.offset)));
        Ok(PreScan {
            items: self.items,
            nested_comments: self.nested.into_iter().map(|r| r.comment).collect(),
            unsupported_tags: self.tags,
        })
    }

    /// Note the last event holding source text. A block scalar's span ends on
    /// the line after its text, and no trailer shares its lines.
    fn content(&mut self, span: Span, block_scalar: bool) {
        self.last_line = if block_scalar { 0 } else { span.end.line() };
        self.last_start = byte(span.start);
        let end = byte(span.end);
        // A key's absent value sits on the key's `:`.
        let on_colon = span.start.index() == span.end.index()
            && self.src.as_bytes().get(end) == Some(&b':');
        self.last_end = end + usize::from(on_colon);
        let last_end = self.last_end;
        self.gap.retain(|&(from, _)| from >= last_end);
    }

    fn comment(&mut self, text: &str, placement: Placement, span: Span) -> Result<(), OverBudget> {
        self.gap.push((byte(span.start), byte(span.end)));
        let c = Comment {
            text: comment_text(text),
            line: span.start.line(),
            column: span.start.col(),
            offset: byte(span.start),
            open: self.live,
        };
        if placement == Placement::Right {
            self.trailing(c)
        } else if let Some(host) = self.flow_host() {
            self.after(host, c)
        } else {
            self.pending.push(c);
            Ok(())
        }
    }

    /// The innermost block collection under the open flow collection a comment
    /// sits in: its current entry holds that flow value.
    fn flow_host(&self) -> Option<usize> {
        let top = self.live.checked_sub(1)?;
        if !self.frames[top].flow {
            return None;
        }
        (0..top).rev().find(|&f| !self.frames[f].flow)
    }

    fn trailing(&mut self, c: Comment) -> Result<(), OverBudget> {
        if let Some(host) = self.flow_host() {
            return self.trail(host, c);
        }
        let Some(top) = self.live.checked_sub(1) else {
            self.pending.push(c);
            return Ok(());
        };
        let frame = &self.frames[top];
        let on_entry = frame.entry.as_ref().is_some_and(|e| {
            e.line == c.line || self.last_line == c.line || c.offset < self.last_start
        });
        if on_entry {
            self.trail(top, c)
        } else if frame.sequence {
            let position = frame.count;
            self.frames[top].next_trailed = true;
            self.record(top, At::Trailer(position), c)
        } else if frame.awaiting_value {
            self.pending.extend(self.held.replace(c));
            Ok(())
        } else {
            self.pending.push(c);
            Ok(())
        }
    }

    /// `c` as the trailer of `frames[f]`'s current entry, or after it when
    /// one already trails it. The first key of a sequence item's mapping lends
    /// its trailer to the item when nothing else would keep the key on a line
    /// below the dash: `to_markdown` writes that key on the dash line, where a
    /// trailer is the item's.
    fn trail(&mut self, f: usize, c: Comment) -> Result<(), OverBudget> {
        let lends = self.is_item_mapping(f) && !self.frames[f].led;
        let Some(entry) = self.frames[f].entry.as_mut() else {
            return Ok(());
        };
        if entry.trailed {
            let index = entry.index;
            return self.record(f, At::After(index), c);
        }
        entry.trailed = true;
        let index = entry.index;
        if index == 0
            && lends
            && let Some(item) = self.frames[f - 1].entry.as_mut().filter(|i| !i.trailed)
        {
            item.trailed = true;
            let position = item.index;
            return self.record(f - 1, At::Trailer(position), c);
        }
        self.record(f, At::Trailer(index), c)
    }

    fn is_item_mapping(&self, f: usize) -> bool {
        let frame = &self.frames[f];
        f > 0
            && !frame.sequence
            && !frame.flow
            && frame.held_at.is_none()
            && self.frames[f - 1].sequence
            && !self.frames[f - 1].flow
    }

    /// `c` as an own-line comment after `frames[f]`'s current entry.
    fn after(&mut self, f: usize, c: Comment) -> Result<(), OverBudget> {
        let Some(entry) = self.frames[f].entry.as_mut() else {
            return Ok(());
        };
        entry.trailed = true;
        let index = entry.index;
        self.record(f, At::After(index), c)
    }

    fn node(&mut self, event: &Event<'_>, span: Span) -> Result<(), Refusal> {
        let shape = Shape::of(event, &span, |tag| self.reads_null(tag));
        let top = self.live.checked_sub(1);
        let awaiting = top.is_some_and(|t| self.frames[t].awaiting_value);
        let dash_trailer = match self.held.take() {
            Some(c) if shape == Shape::BlockSequence && awaiting => Some(c),
            other => {
                self.pending.extend(other);
                None
            }
        };
        let bound = self.settle(byte(span.start))?;
        self.close(self.live);

        let role = match top {
            None => Role::Root,
            Some(t) => self.place(t, event, &span, shape)?,
        };
        if let Event::Scalar(text, style, anchor @ 1.., tag) = event {
            let merge = is_merge_key(text, *style, tag.as_deref());
            self.anchors.insert(*anchor, Anchored::Scalar(text.to_string(), merge));
        }
        let depth = top.map_or(0, |t| t + 1);
        if event.tag().is_some() {
            let path = self.merged_path(depth);
            self.charge_path(&path, 0)?;
            self.tags.push(path);
        }
        let first = self.nested.len();
        let led = self.bind(bound, top, shape, role)?;

        if let Event::MappingStart(..) | Event::SequenceStart(..) = event {
            let held_at = match (role, top) {
                (Role::Value, Some(t)) => self.frames[t].entry.as_ref().map(|e| e.column),
                _ => None,
            };
            let parent = top.map(|t| &self.frames[t]);
            let feeds = parent.is_some_and(|p| matches!(p.children.last(), Some(Child::Merged(_))));
            let sequence = matches!(event, Event::SequenceStart(..));
            let anchor = event.anchor_id().unwrap_or(0);
            if top.is_none() {
                self.root_start = self.items.len();
            }
            self.frames.push(Frame {
                sequence,
                flow: shape == Shape::Flow && top.is_some(),
                count: 0,
                awaiting_value: false,
                held_at,
                column: span.start.col(),
                led,
                entry: None,
                next_trailed: false,
                children: Vec::new(),
                keys: HashSet::new(),
                collects: sequence && (feeds || anchor != 0),
                merged: feeds && parent.is_some_and(|p| !p.sequence || p.merged),
                anchor,
                first,
            });
            self.live = self.frames.len();
            if let Some(c) = dash_trailer {
                let f = self.live - 1;
                self.frames[f].next_trailed = true;
                self.record(f, At::Trailer(0), c)?;
            }
        }
        let block_scalar = matches!(
            event,
            Event::Scalar(_, ScalarStyle::Literal | ScalarStyle::Folded, ..)
        );
        self.content(span, block_scalar);
        Ok(())
    }

    /// Start the child of `frames[top]` the node at `span` begins.
    fn place(
        &mut self,
        top: usize,
        event: &Event<'_>,
        span: &Span,
        shape: Shape,
    ) -> Result<Role, Refusal> {
        let line = span.start.line();
        let frame = &mut self.frames[top];
        if frame.sequence {
            let index = frame.count;
            frame.count += 1;
            frame.entry = Some(Entry {
                index,
                segment: PathSegment::Index(index),
                column: span.start.col(),
                line,
                trailed: std::mem::take(&mut frame.next_trailed),
                empty: false,
            });
            if frame.collects {
                frame.children.push(Child::Merged(Flat::of_node(event, &self.anchors)));
            }
            return Ok(Role::Item);
        }
        if frame.awaiting_value {
            frame.awaiting_value = false;
            if let Some(entry) = frame.entry.as_mut() {
                entry.empty = shape == Shape::Absent;
            }
            if let Some(Child::Merged(flat)) = frame.children.last_mut() {
                *flat = Flat::of_node(event, &self.anchors);
            }
            return Ok(Role::Value);
        }
        let (key, merge) = match event {
            Event::Scalar(text, style, _, tag) => (
                Some(text.to_string()),
                is_merge_key(text, *style, tag.as_deref()),
            ),
            Event::Alias(id) => match self.anchors.get(id) {
                Some(Anchored::Scalar(text, merge)) => (Some(text.clone()), *merge),
                _ => (None, false),
            },
            _ => (None, false),
        };
        if let Some(key) = key.as_ref().filter(|_| !merge)
            && !frame.keys.insert(key.clone())
        {
            return Err(Refusal::SharedKey {
                key: key.clone(),
                line: span.start.line(),
                column: span.start.col() + 1,
            });
        }
        let column = span.indent.unwrap_or(span.start.col());
        if frame.count == 0 {
            frame.column = column;
        }
        let index = frame.count;
        frame.count += 1;
        frame.awaiting_value = true;
        frame.entry = Some(Entry {
            index,
            segment: PathSegment::Key(key.clone().unwrap_or_default()),
            column,
            line,
            trailed: false,
            empty: false,
        });
        frame.children.push(match merge {
            true => Child::Merged(Flat::default()),
            false => Child::Key(key.clone().unwrap_or_default()),
        });
        if let (0, Some(key), false) = (top, key, merge) {
            self.items.push(PreItem::Field { key });
        }
        Ok(Role::Key)
    }

    /// Place the own-line comments waiting on a node starting at byte `start`,
    /// returning those that sit between the node and the key or dash it
    /// belongs to.
    fn settle(&mut self, start: usize) -> Result<Vec<Comment>, OverBudget> {
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        // The parser hands the comments below a node with no text of its own,
        // `k: !!str`, ahead of it: they wait for the node after it.
        let (mut run, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|c| c.offset < start);
        self.pending = later;
        let Some(top) = self.live.checked_sub(1) else {
            return Ok(run);
        };
        let frame = &self.frames[top];
        if frame.awaiting_value {
            return Ok(run);
        }
        let bound = if frame.sequence {
            let dash = self.dash(start);
            let split = run.partition_point(|c| dash.is_some_and(|d| c.offset < d));
            run.split_off(split)
        } else {
            Vec::new()
        };
        self.place_run(run)?;
        Ok(bound)
    }

    /// The byte offset of the dash between the last event holding source text
    /// and `start`, past whitespace and comments: the only other thing a block
    /// sequence puts there.
    fn dash(&self, start: usize) -> Option<usize> {
        let bytes = self.src.as_bytes();
        let end = start.min(bytes.len());
        let mut at = self.last_end;
        let mut comments = self.gap.iter().copied().peekable();
        while at < end {
            if let Some(&(from, to)) = comments.peek() {
                if at >= to {
                    comments.next();
                    continue;
                }
                if at >= from {
                    at = to;
                    comments.next();
                    continue;
                }
            }
            match bytes[at] {
                b' ' | b'\t' | b'\n' | b'\r' => at += 1,
                b'-' => return Some(at),
                _ => return None,
            }
        }
        None
    }

    /// Place a run of own-line comments that ends at the next child of the
    /// innermost open collection, or at the end of the input. Each lands in the
    /// deepest slot it is indented into: an empty value, a collection the run
    /// closes, or ahead of that next child; and never deeper than the comment
    /// before it.
    fn place_run(&mut self, run: Vec<Comment>) -> Result<(), OverBudget> {
        let Some(top) = self.live.checked_sub(1) else {
            for c in run {
                self.record(0, At::Ahead(0), c)?;
            }
            return Ok(());
        };
        let deepest = self.frames.len() - 1;
        let mut slots = Vec::new();
        let last = &self.frames[deepest];
        if let Some(entry) = last.entry.as_ref()
            && !last.sequence
            && !last.awaiting_value
            && entry.empty
        {
            slots.push(Slot::Under(deepest, entry.column));
        }
        slots.extend((self.live..self.frames.len()).rev().map(Slot::Last));
        slots.push(Slot::Next(top));

        let mut floor = 0;
        for c in run {
            let from = slots
                .iter()
                .position(|slot| match *slot {
                    Slot::Under(f, _) | Slot::Last(f) => f < c.open,
                    Slot::Next(_) => true,
                })
                .unwrap_or(slots.len() - 1)
                .max(floor);
            let pick = (from..slots.len())
                .find(|&i| self.holds(slots[i], c.column))
                .unwrap_or(slots.len() - 1);
            floor = pick;
            match slots[pick] {
                Slot::Under(f, _) => self.record(f + 1, At::Ahead(0), c)?,
                Slot::Last(f) | Slot::Next(f) => {
                    let position = self.frames[f].count;
                    self.record(f, At::Ahead(position), c)?;
                }
            }
        }
        Ok(())
    }

    fn holds(&self, slot: Slot, column: usize) -> bool {
        match slot {
            Slot::Under(_, key) => column > key,
            Slot::Last(f) => match self.frames[f].held_at {
                Some(key) => column > key,
                None => column >= self.frames[f].column,
            },
            Slot::Next(_) => true,
        }
    }

    /// Place the comments between a node and the key or dash it belongs to:
    /// inside it when it is a block collection or a key's absent value they are
    /// indented past, otherwise after its entry. Answers whether any landed
    /// inside.
    fn bind(
        &mut self,
        comments: Vec<Comment>,
        top: Option<usize>,
        shape: Shape,
        role: Role,
    ) -> Result<bool, OverBudget> {
        let Some(top) = top else {
            for c in comments {
                self.record(0, At::Ahead(0), c)?;
            }
            return Ok(false);
        };
        let key = self.frames[top].entry.as_ref().map_or(0, |e| e.column);
        let mut inside = shape.is_block() || (shape == Shape::Absent && role == Role::Value);
        let mut led = false;
        for c in comments {
            inside &= shape.is_block() || c.column > key;
            led |= inside;
            if inside {
                self.record(top + 1, At::Ahead(0), c)?;
            } else {
                self.after(top, c)?;
            }
        }
        Ok(led)
    }

    fn end(&mut self, span: Span) -> Result<(), OverBudget> {
        let Some(top) = self.live.checked_sub(1) else {
            return Ok(());
        };
        if self.frames[top].flow {
            self.close(top + 1);
            let frame = &self.frames[top];
            if frame.count == 0
                && frame.held_at.is_some()
                && let Some(entry) = self.frames[top - 1].entry.as_mut()
            {
                entry.empty = true;
            }
            self.close(top);
            self.live = top;
            self.content(span, false);
            return Ok(());
        }
        if top == 0 {
            self.pending.extend(self.held.take());
            let run = std::mem::take(&mut self.pending);
            self.place_run(run)?;
            self.close(0);
            self.live = 0;
            return Ok(());
        }
        self.live = top;
        if self.pending.is_empty() && self.held.is_none() {
            self.close(top);
        }
        Ok(())
    }

    /// The path of the collection at `depth`: the root at 0, and past it the
    /// value of `frames[depth - 1]`'s current entry.
    fn path(&self, depth: usize) -> Vec<PathSegment> {
        self.frames[..depth]
            .iter()
            .filter_map(|f| f.entry.as_ref().map(|e| e.segment.clone()))
            .collect()
    }

    /// [`path`](Self::path) as the value holds it: through a merge, the
    /// merged keys sit in the mapping holding the merge.
    fn merged_path(&self, depth: usize) -> Vec<PathSegment> {
        self.frames[..depth]
            .iter()
            .filter(|f| match f.sequence {
                true => !f.merged,
                false => !matches!(f.children.last(), Some(Child::Merged(_))),
            })
            .filter_map(|f| f.entry.as_ref().map(|e| e.segment.clone()))
            .collect()
    }

    fn charge_path(&mut self, path: &[PathSegment], extra: usize) -> Result<(), OverBudget> {
        let cost = path
            .iter()
            .map(|s| {
                std::mem::size_of::<PathSegment>()
                    + match s {
                        PathSegment::Key(k) => k.len(),
                        PathSegment::Index(_) => 0,
                    }
            })
            .sum::<usize>()
            + extra;
        self.left = self.left.checked_sub(cost).ok_or(OverBudget {
            budget: self.budget,
        })?;
        Ok(())
    }

    /// Record `c` at `at` in the collection at `depth`.
    fn record(&mut self, depth: usize, at: At, c: Comment) -> Result<(), OverBudget> {
        let (position, inline) = at.slot();
        let text = c.text;
        if depth == 0 {
            if !self.frames.is_empty() {
                self.root_marks.push((at, c.offset));
            }
            self.items.push(PreItem::Comment { text, inline });
            return Ok(());
        }
        let container_path = self.path(depth);
        let mut order = self.chain(depth);
        order.extend([position, usize::from(inline)]);
        self.charge_path(&container_path, order.len() * std::mem::size_of::<usize>())?;
        self.nested.push(Recorded {
            order,
            at,
            offset: c.offset,
            comment: NestedComment {
                container_path,
                position,
                text,
                inline,
            },
        });
        Ok(())
    }

    /// The place in emit order of the collection at `depth`. At each slot come
    /// own-line comments (`0`), the trailer (`1`), then the comments inside the
    /// child there (`2`).
    fn chain(&self, depth: usize) -> Vec<usize> {
        self.frames[..depth]
            .iter()
            .flat_map(|f| [f.entry.as_ref().map_or(0, |e| e.index), 2])
            .collect()
    }

    /// Drop the frames past `keep`, deepest first. Each hands its keys to the
    /// anchor it defines and to the merge or item holding it, and a mapping
    /// holding a merge, written outside any merge, places its comments again.
    fn close(&mut self, keep: usize) {
        while self.frames.len() > keep {
            let depth = self.frames.len() - 1;
            let item = self.is_item_mapping(depth);
            let frame = self.frames.pop().expect("a frame past `keep`");
            let merges = frame.children.iter().any(|c| matches!(c, Child::Merged(_)));
            let flat = Flat::of(frame.children);
            if merges && !frame.sequence && !frame.merged {
                if depth == 0 {
                    self.rebuild_root(&flat);
                } else {
                    self.resolve(depth, frame.first, &flat);
                    if item && flat.keys.is_empty() {
                        self.vacate(depth, frame.first);
                    } else if item {
                        self.lend(depth, frame.first);
                    }
                }
            }
            if frame.anchor != 0 {
                let mut seen = HashSet::new();
                let keys = flat.keys.iter().filter(|k| seen.insert(k.as_str())).cloned();
                self.anchors.insert(frame.anchor, Anchored::Keys(keys.collect()));
            }
            if let Some(Child::Merged(slot)) =
                self.frames.last_mut().and_then(|f| f.children.last_mut())
            {
                *slot = flat;
            }
        }
        if keep == 0 {
            self.root_marks.clear();
        }
    }

    /// Place again the comments recorded since `first` inside the mapping at
    /// `depth`, which holds a merge, among the keys of `flat`.
    fn resolve(&mut self, depth: usize, first: usize, flat: &Flat) {
        let seat = seats(&flat.keys);
        let chain = self.chain(depth);
        let path = self.path(depth);
        for r in &mut self.nested[first..] {
            if !r.order.starts_with(&chain) {
                continue;
            }
            if let Some((position, inline)) = reseat(r, &chain, &path, flat, &seat) {
                r.at = if inline { At::Trailer(position) } else { At::Ahead(position) };
                r.order = [chain.as_slice(), &[position, usize::from(inline)]].concat();
                r.comment.container_path = path.clone();
                r.comment.position = position;
                r.comment.inline = inline;
            }
        }
    }

    /// Lend the item the trailer of its mapping's first key, as `trail` does,
    /// once a merge has placed the mapping's comments again: no own-line
    /// comment leads that key, and the item has no trailer of its own.
    fn lend(&mut self, depth: usize, first: usize) {
        let Some(index) = self.frames[depth - 1]
            .entry
            .as_ref()
            .filter(|e| !e.trailed)
            .map(|e| e.index)
        else {
            return;
        };
        let chain = self.chain(depth);
        let slot = |inline: usize| [chain.as_slice(), &[0, inline]].concat();
        let (led, trailer) = (slot(0), slot(1));
        let mine = &mut self.nested[first..];
        if mine.iter().any(|r| r.order == led) {
            return;
        }
        let Some(r) = mine.iter_mut().find(|r| r.order == trailer) else {
            return;
        };
        r.order = [&chain[..chain.len() - 2], &[index, 1]].concat();
        r.at = At::Trailer(index);
        r.comment.container_path.pop();
        r.comment.position = index;
        if let Some(entry) = self.frames[depth - 1].entry.as_mut() {
            entry.trailed = true;
        }
    }

    /// Move the comments of an item's mapping that holds no key after the item:
    /// `to_markdown` writes that item `{}`, which holds none.
    fn vacate(&mut self, depth: usize, first: usize) {
        let Some(index) = self.frames[depth - 1].entry.as_ref().map(|e| e.index) else {
            return;
        };
        let chain = self.chain(depth);
        let after = [&chain[..chain.len() - 2], &[index + 1, 0]].concat();
        for r in &mut self.nested[first..] {
            if r.order.starts_with(&chain) {
                r.order = after.clone();
                r.at = At::After(index);
                r.comment.container_path.pop();
                r.comment.position = index + 1;
                r.comment.inline = false;
            }
        }
    }

    /// Place again the items of a root mapping holding a merge among the keys
    /// of `flat`: its fields, the comments on its slots, and each comment
    /// inside it that lands on one.
    fn rebuild_root(&mut self, flat: &Flat) {
        let seat = seats(&flat.keys);
        let mut landed = Vec::new();
        let marks = std::mem::take(&mut self.root_marks);
        let comments = self
            .items
            .split_off(self.root_start)
            .into_iter()
            .filter_map(|item| match item {
                PreItem::Comment { text, .. } => Some(text),
                PreItem::Field { .. } => None,
            });
        for ((at, offset), text) in marks.into_iter().zip(comments) {
            let (position, inline) = at.slot();
            let mut r = Recorded {
                order: vec![position, usize::from(inline)],
                at,
                offset,
                comment: NestedComment {
                    container_path: Vec::new(),
                    position,
                    text,
                    inline,
                },
            };
            if let Some((position, inline)) = reseat(&mut r, &[], &[], flat, &seat) {
                landed.push((position, inline, offset, r.comment.text));
            }
        }
        for mut r in std::mem::take(&mut self.nested) {
            match reseat(&mut r, &[], &[], flat, &seat) {
                Some((position, inline)) => {
                    landed.push((position, inline, r.offset, r.comment.text));
                }
                None => self.nested.push(r),
            }
        }
        landed.sort_by_key(|&(position, inline, offset, _)| (position, inline, offset));
        let mut landed = landed.into_iter().peekable();
        let fields = flat.keys.iter().enumerate().filter(|&(o, _)| seat[o + 1] > seat[o]);
        for (position, (_, key)) in fields.enumerate() {
            while let Some((.., text)) = landed.next_if(|l| (l.0, l.1) == (position, false)) {
                self.items.push(PreItem::Comment { text, inline: false });
            }
            self.items.push(PreItem::Field { key: key.clone() });
            while let Some((.., text)) = landed.next_if(|l| (l.0, l.1) == (position, true)) {
                self.items.push(PreItem::Comment { text, inline: true });
            }
        }
        self.items.extend(landed.map(|(.., text)| PreItem::Comment { text, inline: false }));
    }
}

#[cfg(test)]
mod tests {
    mod properties;

    use super::*;

    fn scan(yaml: &str) -> PreScan {
        prescan_fence_content(yaml).expect("within budget")
    }

    fn key(k: &str) -> PathSegment {
        PathSegment::Key(k.to_string())
    }

    fn nested(path: Vec<PathSegment>, position: usize, text: &str, inline: bool) -> NestedComment {
        NestedComment {
            container_path: path,
            position,
            text: text.to_string(),
            inline,
        }
    }

    fn field(k: &str) -> PreItem {
        PreItem::Field { key: k.to_string() }
    }

    fn comment(text: &str, inline: bool) -> PreItem {
        PreItem::Comment {
            text: text.to_string(),
            inline,
        }
    }

    #[test]
    fn extracts_own_line_comments() {
        let out = scan("# top\ntitle: foo\n# mid\nauthor: bar\n");
        assert_eq!(
            out.items,
            vec![comment("top", false), field("title"), comment("mid", false), field("author")]
        );
        assert!(out.nested_comments.is_empty());
    }

    #[test]
    fn splits_trailing_comments() {
        let out = scan("title: foo # inline\n");
        assert_eq!(out.items, vec![field("title"), comment("inline", true)]);
    }

    #[test]
    fn a_tag_is_recorded_at_its_key() {
        for input in ["dept: !custom Department\n", "dept: !custom\n"] {
            let out = scan(input);
            assert_eq!(out.items, vec![field("dept")]);
            assert_eq!(out.unsupported_tags, vec![vec![key("dept")]]);
        }
    }

    #[test]
    fn crlf_line_ends_reach_no_comment_text() {
        let out = scan("dept: !t\r\n# note\r\ntitle: x # trailing\r\n");
        assert_eq!(
            out.items,
            vec![
                field("dept"),
                comment("note", false),
                field("title"),
                comment("trailing", true),
            ]
        );
        assert_eq!(out.unsupported_tags, vec![vec![key("dept")]]);
    }

    #[test]
    fn nested_comment_in_sequence_captured() {
        let out = scan("arr:\n  # before-first\n  - a\n  # between\n  - b\n  # after-last\n");
        assert_eq!(
            out.nested_comments,
            vec![
                nested(vec![key("arr")], 0, "before-first", false),
                nested(vec![key("arr")], 1, "between", false),
                nested(vec![key("arr")], 2, "after-last", false),
            ]
        );
    }

    #[test]
    fn nested_comment_in_mapping_captured() {
        let out = scan("outer:\n  # comment\n  inner: 1\n");
        assert_eq!(out.nested_comments, vec![nested(vec![key("outer")], 0, "comment", false)]);
    }

    /// An empty flow collection's comments sit under it, whether its key opens
    /// its own line or a sequence item's.
    #[test]
    fn a_comment_under_an_empty_flow_collection_is_inside_it() {
        let out = scan("rows: []\n  # - a\nrow:\n  - key: {}\n      # b\n    next: 1\n");
        assert_eq!(
            out.nested_comments,
            vec![
                nested(vec![key("rows")], 0, "- a", false),
                nested(vec![key("row"), PathSegment::Index(0), key("key")], 0, "b", false),
            ]
        );
    }

    #[test]
    fn deep_nested_comment_path() {
        let out = scan("outer:\n  inner:\n    # deep\n    leaf: 1\n");
        assert_eq!(
            out.nested_comments,
            vec![nested(vec![key("outer"), key("inner")], 0, "deep", false)]
        );
    }

    #[test]
    fn comment_inside_seq_of_maps() {
        let out = scan("items:\n  - name: a\n    # inside-first\n    val: 1\n  - name: b\n");
        assert_eq!(
            out.nested_comments,
            vec![nested(vec![key("items"), PathSegment::Index(0)], 1, "inside-first", false)]
        );
    }

    #[test]
    fn nested_inline_on_sequence_item() {
        let out = scan("arr:\n  - a # tail\n  - b\n");
        assert_eq!(out.nested_comments, vec![nested(vec![key("arr")], 0, "tail", true)]);
    }

    #[test]
    fn nested_inline_on_mapping_field() {
        let out = scan("outer:\n  inner: 1 # tail\n");
        assert_eq!(out.nested_comments, vec![nested(vec![key("outer")], 0, "tail", true)]);
    }

    #[test]
    fn a_tag_records_its_node_path_at_any_depth() {
        let out = scan(
            "addr:\n  street: !custom Main\nto:\n  - name: !custom\nl:\n- !t a\n- [!t b]\nk: !t\n  x: 1\n",
        );
        assert_eq!(
            out.unsupported_tags,
            vec![
                vec![key("addr"), key("street")],
                vec![key("to"), PathSegment::Index(0), key("name")],
                vec![key("l"), PathSegment::Index(0)],
                vec![key("l"), PathSegment::Index(1), PathSegment::Index(0)],
                vec![key("k")],
            ]
        );
    }

    #[test]
    fn multibyte_text_around_a_comment_keeps_its_offsets() {
        let out = scan("arr:\n  - – en-dash # c\n  -\n    # d\n    \u{1F600} emoji\n  - \u{201C}q\u{201D}\n");
        assert_eq!(
            out.nested_comments,
            vec![
                nested(vec![key("arr")], 0, "c", true),
                nested(vec![key("arr")], 2, "d", false),
            ]
        );
    }

    #[test]
    fn block_scalar_content_is_not_parsed_as_structure() {
        let out = scan(
            "bio: |-\n  ## About me\n\n  - point one\n  role: engineer\n  Done.\nname: jane\nitems:\n  - |-\n    ## Heading\n    role: x\n  - second\n",
        );
        assert_eq!(out.items, vec![field("bio"), field("name"), field("items")]);
        assert!(out.nested_comments.is_empty());
    }

    /// A `#` opens a comment only after whitespace and outside a quoted scalar,
    /// which a tag or anchor may precede and a flow collection may hold.
    #[test]
    fn a_hash_is_a_comment_only_where_yaml_reads_one() {
        let cases = [
            ("k: it's a test # note\n", Some("note")),
            ("k: 'a # b'\n", None),
            ("k: \"a # b\"\n", None),
            ("k: 'a # b' # real\n", Some("real")),
            ("k: 'it''s # x' # real\n", Some("real")),
            ("k: \"a \\\" # b\" # real\n", Some("real")),
            ("k: [a, \"b # c\"] # real\n", Some("real")),
            ("k: [a, \"b # c\"]\n", None),
            ("k: !t \"a # b\" # real\n", Some("real")),
            ("k: a#b\n", None),
        ];
        for (input, trailer) in cases {
            let want = match trailer {
                Some(t) => vec![field("k"), comment(t, true)],
                None => vec![field("k")],
            };
            assert_eq!(scan(input).items, want, "{input}");
        }
    }

    /// A sequence at its key's column holds the comments between its items,
    /// and one at that column after its last item is the next key's.
    #[test]
    fn a_sequence_at_its_key_column_holds_its_comments() {
        let out = scan("to:\n# lead\n- name: a # c1\n  # inner\n  rank: 1\n- b\n # last\n# next\nn: 1\n");
        let to = || vec![key("to")];
        assert_eq!(
            out.nested_comments,
            vec![
                nested(to(), 0, "lead", false),
                nested(to(), 0, "c1", true),
                nested(vec![key("to"), PathSegment::Index(0)], 1, "inner", false),
                nested(to(), 2, "last", false),
            ]
        );
        assert_eq!(out.items, vec![field("to"), comment("next", false), field("n")]);
    }

    /// A run of own-line comments that closes collections lands each comment in
    /// the deepest one it is indented into, never deeper than the one before.
    #[test]
    fn a_closing_run_lands_by_column() {
        let out = scan("a:\n  b:\n    c: 1\n    # x\n  # y\n      # z\n# w\nd: 1\n");
        assert_eq!(
            out.nested_comments,
            vec![
                nested(vec![key("a"), key("b")], 1, "x", false),
                nested(vec![key("a")], 1, "y", false),
                nested(vec![key("a")], 1, "z", false),
            ]
        );
        assert_eq!(out.items, vec![field("a"), comment("w", false), field("d")]);
    }

    /// A dash line's trailer is the item's, whether the line holds the item's
    /// first key or a bare dash, and whether the sequence is indented or at its
    /// key's column. The first key's trailer is the item's too, unless the item
    /// carries one or a comment leads the key: the spellings `to_markdown`
    /// writes for each.
    #[test]
    fn a_dash_line_trailer_is_the_items() {
        let item = |i| vec![key("l"), PathSegment::Index(i)];
        for src in [
            "l:\n  - k: v # a\n  - # b\n    k: v # c\n",
            "l:\n- k: v # a\n- # b\n  k: v # c\n",
        ] {
            assert_eq!(
                scan(src).nested_comments,
                vec![
                    nested(vec![key("l")], 0, "a", true),
                    nested(vec![key("l")], 1, "b", true),
                    nested(item(1), 0, "c", true),
                ],
                "{src}"
            );
        }
        assert_eq!(
            scan("l:\n- # b\n  k: v # c\n").nested_comments,
            vec![nested(vec![key("l")], 0, "b", true), nested(item(0), 0, "c", true)]
        );
        assert_eq!(
            scan("l:\n  -\n    k: v # a\n").nested_comments,
            vec![nested(vec![key("l")], 0, "a", true)]
        );
        assert_eq!(
            scan("l:\n  -\n    # o\n    k: v # a\n").nested_comments,
            vec![nested(item(0), 0, "o", false), nested(item(0), 0, "a", true)]
        );
    }

    /// A block scalar's header trailer is its entry's, though the parser reads
    /// it after the scalar's text, and the next dash line's is the next item's.
    #[test]
    fn a_block_scalar_header_trailer_is_its_entrys() {
        let out = scan("l:\n  - | # a\n    text\n  - >- # b\n    x\n  - # c\n    k: v\nm: | # d\n  t\n");
        assert_eq!(
            out.nested_comments,
            vec![
                nested(vec![key("l")], 0, "a", true),
                nested(vec![key("l")], 1, "b", true),
                nested(vec![key("l")], 2, "c", true),
            ]
        );
        assert_eq!(out.items, vec![field("l"), field("m"), comment("d", true)]);
    }

    /// Recorded paths cost no more than a fixed multiple of the input, however
    /// deep or long the run: the budget never trips on a deep structure with a
    /// comment at every level and a long run of comments.
    #[test]
    fn deep_nesting_and_long_comment_runs_stay_within_budget() {
        let mut yaml = String::new();
        for depth in 0..60 {
            yaml.push_str(&" ".repeat(depth * 2));
            yaml.push_str(&format!("k{depth}: # t\n"));
        }
        yaml.push_str(&format!("{}x: 1\n", " ".repeat(120)));
        for _ in 0..5_000 {
            yaml.push_str(&format!("{}# c\n", " ".repeat(120)));
        }
        let compact = format!("l:\n  {}x # c\n", "- ".repeat(60));
        for input in [yaml, compact] {
            let out = scan(&input);
            assert!(!out.nested_comments.is_empty());
        }
    }
}
