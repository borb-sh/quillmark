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
//! Every tagged node is recorded at its path, for the assembler to warn on; the
//! value parse applies a core `!!` tag, ignores any other, and keeps no tag.

use serde_saphyr::granit_parser::{
    Event, Marker, Parser, Placement, ScalarStyle, Span, StructureStyle,
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
    /// Top-level fields and comments in source order.
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

/// Bytes of recorded path a block of `len` bytes may hold. Each comment clones
/// its container's path, so many comments under long keys would otherwise
/// grow with the square of the input.
pub(crate) fn budget(len: usize) -> usize {
    len.saturating_mul(64).saturating_add(64 * 1024)
}

/// Scan `yaml`, the text the value parse reads. A parser error ends the scan
/// with what it has read: the value parse is the one that refuses.
pub(crate) fn prescan_fence_content(yaml: &str) -> Result<PreScan, OverBudget> {
    let mut walk = Walk::new(yaml);
    for next in Parser::new_from_str(yaml) {
        let Ok((event, span)) = next else { break };
        walk.step(&event, span)?;
    }
    walk.finish()
}

/// What a node event starts, as far as comments care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    BlockMapping,
    BlockSequence,
    Flow,
    /// A scalar with no source text: a key's or a dash's missing value.
    Absent,
    Scalar,
}

impl Shape {
    fn of(event: &Event<'_>, span: &Span) -> Self {
        match event {
            Event::MappingStart(StructureStyle::Block, ..) => Shape::BlockMapping,
            Event::SequenceStart(StructureStyle::Block, ..) => Shape::BlockSequence,
            Event::MappingStart(..) | Event::SequenceStart(..) => Shape::Flow,
            Event::Scalar(_, ScalarStyle::Plain, ..) if span.start.index() == span.end.index() => {
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

struct Walk<'a> {
    src: &'a str,
    budget: usize,
    left: usize,
    items: Vec<PreItem>,
    /// Each comment beside its place in emit order.
    nested: Vec<(Vec<usize>, NestedComment)>,
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
    /// Byte ranges of the comments since the last event holding source text.
    gap: Vec<(usize, usize)>,
}

fn byte(marker: Marker) -> usize {
    marker.byte_offset().unwrap_or(0)
}

/// The text after `#`, less any further `#` and one space.
fn comment_text(raw: &str) -> String {
    let after = raw.trim_start_matches('#');
    after.strip_prefix(' ').unwrap_or(after).to_string()
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
        }
    }

    fn step(&mut self, event: &Event<'_>, span: Span) -> Result<(), OverBudget> {
        match event {
            Event::Comment(text, placement) => self.comment(text, *placement, span),
            Event::Scalar(..)
            | Event::Alias(..)
            | Event::SequenceStart(..)
            | Event::MappingStart(..) => self.node(event, span),
            Event::SequenceEnd | Event::MappingEnd => self.end(span),
            _ => Ok(()),
        }
    }

    fn finish(mut self) -> Result<PreScan, OverBudget> {
        self.pending.extend(self.held.take());
        let run = std::mem::take(&mut self.pending);
        if self.live == 0 {
            for c in run {
                self.record(0, 0, c.text, false)?;
            }
        } else {
            self.place_run(run)?;
        }
        self.nested.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(PreScan {
            items: self.items,
            nested_comments: self.nested.into_iter().map(|(_, c)| c).collect(),
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
        self.gap.clear();
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
            self.after(host, c.text)
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
            return self.trail(host, c.text);
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
            self.trail(top, c.text)
        } else if frame.sequence {
            let position = frame.count;
            self.frames[top].next_trailed = true;
            self.record(top, position, c.text, true)
        } else if frame.awaiting_value {
            self.pending.extend(self.held.replace(c));
            Ok(())
        } else {
            self.pending.push(c);
            Ok(())
        }
    }

    /// `text` as the trailer of `frames[f]`'s current entry, or after it when
    /// one already trails it. The first key of a sequence item's mapping lends
    /// its trailer to the item when nothing else would keep the key on a line
    /// below the dash: `to_markdown` writes that key on the dash line, where a
    /// trailer is the item's.
    fn trail(&mut self, f: usize, text: String) -> Result<(), OverBudget> {
        let lends = self.is_item_mapping(f) && !self.frames[f].led;
        let Some(entry) = self.frames[f].entry.as_mut() else {
            return Ok(());
        };
        if entry.trailed {
            let position = entry.index + 1;
            return self.record(f, position, text, false);
        }
        entry.trailed = true;
        let index = entry.index;
        if index == 0
            && lends
            && let Some(item) = self.frames[f - 1].entry.as_mut().filter(|i| !i.trailed)
        {
            item.trailed = true;
            let position = item.index;
            return self.record(f - 1, position, text, true);
        }
        self.record(f, index, text, true)
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

    /// `text` as an own-line comment after `frames[f]`'s current entry.
    fn after(&mut self, f: usize, text: String) -> Result<(), OverBudget> {
        let Some(entry) = self.frames[f].entry.as_mut() else {
            return Ok(());
        };
        entry.trailed = true;
        let position = entry.index + 1;
        self.record(f, position, text, false)
    }

    fn node(&mut self, event: &Event<'_>, span: Span) -> Result<(), OverBudget> {
        let shape = Shape::of(event, &span);
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
        self.frames.truncate(self.live);

        let role = match top {
            None => Role::Root,
            Some(t) => self.place(t, event, &span, shape),
        };
        let depth = top.map_or(0, |t| t + 1);
        if event.tag().is_some() {
            let path = self.path(depth);
            self.charge_path(&path, 0)?;
            self.tags.push(path);
        }
        let led = self.bind(bound, top, shape, role)?;

        if let Event::MappingStart(..) | Event::SequenceStart(..) = event {
            let held_at = match (role, top) {
                (Role::Value, Some(t)) => self.frames[t].entry.as_ref().map(|e| e.column),
                _ => None,
            };
            self.frames.push(Frame {
                sequence: matches!(event, Event::SequenceStart(..)),
                flow: shape == Shape::Flow && top.is_some(),
                count: 0,
                awaiting_value: false,
                held_at,
                column: span.start.col(),
                led,
                entry: None,
                next_trailed: false,
            });
            self.live = self.frames.len();
            if let Some(c) = dash_trailer {
                let f = self.live - 1;
                self.frames[f].next_trailed = true;
                self.record(f, 0, c.text, true)?;
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
    fn place(&mut self, top: usize, event: &Event<'_>, span: &Span, shape: Shape) -> Role {
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
            return Role::Item;
        }
        if frame.awaiting_value {
            frame.awaiting_value = false;
            if let Some(entry) = frame.entry.as_mut() {
                entry.empty = shape == Shape::Absent;
            }
            return Role::Value;
        }
        let key = match event {
            Event::Scalar(text, ..) => Some(text.to_string()),
            _ => None,
        };
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
        if let (0, Some(key)) = (top, key) {
            self.items.push(PreItem::Field { key });
        }
        Role::Key
    }

    /// Place the own-line comments waiting on a node starting at byte `start`,
    /// returning those that sit between the node and the key or dash it
    /// belongs to.
    fn settle(&mut self, start: usize) -> Result<Vec<Comment>, OverBudget> {
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        let mut run = std::mem::take(&mut self.pending);
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
                self.record(0, 0, c.text, false)?;
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
                Slot::Under(f, _) => self.record(f + 1, 0, c.text, false)?,
                Slot::Last(f) | Slot::Next(f) => {
                    let position = self.frames[f].count;
                    self.record(f, position, c.text, false)?;
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
                self.record(0, 0, c.text, false)?;
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
                self.record(top + 1, 0, c.text, false)?;
            } else {
                self.after(top, c.text)?;
            }
        }
        Ok(led)
    }

    fn end(&mut self, span: Span) -> Result<(), OverBudget> {
        let Some(top) = self.live.checked_sub(1) else {
            return Ok(());
        };
        if self.frames[top].flow {
            self.frames.truncate(top + 1);
            let frame = self.frames.pop().expect("the top frame is present");
            self.live = top;
            if frame.count == 0
                && frame.held_at.is_some()
                && let Some(entry) = self.frames[top - 1].entry.as_mut()
            {
                entry.empty = true;
            }
            self.content(span, false);
            return Ok(());
        }
        if top == 0 {
            self.pending.extend(self.held.take());
            let run = std::mem::take(&mut self.pending);
            self.place_run(run)?;
            self.frames.clear();
            self.live = 0;
            return Ok(());
        }
        self.live = top;
        if self.pending.is_empty() && self.held.is_none() {
            self.frames.truncate(top);
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

    /// Record `text` at `position` in the collection at `depth`.
    fn record(
        &mut self,
        depth: usize,
        position: usize,
        text: String,
        inline: bool,
    ) -> Result<(), OverBudget> {
        if depth == 0 {
            self.items.push(PreItem::Comment { text, inline });
            return Ok(());
        }
        let container_path = self.path(depth);
        // At each slot: own-line comments (`0`), the trailer (`1`), then the
        // comments inside the child there (`2`).
        let mut order: Vec<usize> = self.frames[..depth]
            .iter()
            .flat_map(|f| [f.entry.as_ref().map_or(0, |e| e.index), 2])
            .collect();
        order.extend([position, usize::from(inline)]);
        self.charge_path(&container_path, order.len() * std::mem::size_of::<usize>())?;
        self.nested.push((
            order,
            NestedComment {
                container_path,
                position,
                text,
                inline,
            },
        ));
        Ok(())
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

    #[test]
    fn many_comments_under_long_keys_are_refused() {
        let long = "k".repeat(1000);
        let mut yaml = String::new();
        for depth in 0..8 {
            yaml.push_str(&" ".repeat(depth * 2));
            yaml.push_str(&format!("{long}{depth}:\n"));
        }
        let indent = " ".repeat(16);
        yaml.push_str(&format!("{indent}x: 1\n"));
        for _ in 0..2000 {
            yaml.push_str(&format!("{indent}#\n"));
        }
        let err = prescan_fence_content(&yaml).expect_err("over budget");
        assert_eq!(err.budget, budget(yaml.len()));
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
