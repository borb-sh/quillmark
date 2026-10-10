//! Markdown import (cold): `normalize → pulldown → content`.
//!
//! Input is normalized by `normalize::parse_markdown` (CRLF→LF, bidi
//! controls dropped, line separators spaced, then the parser-guided repair that
//! splits text off a comment's line, ends a table at a row of tags and sets a
//! line of carrier tags in an HTML block apart) so the content invariants hold
//! by construction, then
//! parsed with `pulldown_cmark` (CommonMark + strikethrough + pipe tables +
//! task lists, and footnotes recognized only to drop them) and
//! walked into a [`Content`]. This is the one place the `<u>`/`<br>` allowlist
//! runs, and the one place a dropped raw tag is counted into the
//! [`ImportWarning`]s an import returns beside its content.
//!
//! ## Canonicalizations
//!
//! - A line of carrier tags in a paragraph or a list item's text is a tag
//!   line: the break above it ends the text, and the text under it opens a
//!   paragraph.
//! - A soft break is a space and a hard break a `continues` line, distinct from
//!   a paragraph boundary. Inside a heading a hard break is a space, a heading
//!   being one line; a setext heading spans two source lines and can carry one.
//!   Inside a table cell a hard break is a `\n` in the cell's text. An inline
//!   `<br>` is a hard break, dropped where no text precedes it on its line.
//! - Two adjacent sibling containers keep their boundary, told apart by
//!   `Container::instance`, minted here and canonicalized by `normalize`.
//! - An empty heading, code block or container keeps its line. An empty
//!   paragraph drops, markdown having no syntax to write one back.
//! - Island ids are minted sequentially (`isl-0`, `isl-1`, …), so import is a
//!   deterministic function of its markdown. Export drops the ids and re-import
//!   re-mints the same sequence.
//! - Tables and images are islands, block and inline respectively; a thematic
//!   break is a `Rule` line carrying no text.
//! - A task list item's marker is its [`Container::ListItem::checked`]; `[X]`
//!   reads as `[x]`.
//! - A footnote definition drops whole, reported, and a reference to one is
//!   the text it is.
//! - Raw HTML produces no content beyond the allowlist and the carrier. An HTML
//!   block drops whole, as CommonMark runs it; one of tag lines alone passes
//!   its carrier tags on. A `qm-table` wrapper pairs as an element does,
//!   and folds its attributes into the props of the one table it wraps. A
//!   `qm-cell` pair around a table cell's whole content folds its attributes
//!   into the cell.
//! - A `qm-*` element the carrier does not reserve is modeled: a pair of tag
//!   lines wraps the blocks between them in a [`Container::Element`]. One left
//!   unclosed drops, what it wraps importing as written.

use crate::model::{
    Container, Island, Line, LineKind, Mark, MarkKind, Content, Normalized, ISLAND_SLOT,
};
use crate::carrier;
use crate::html;
use crate::island::IslandType;
use crate::normalize::parse_markdown;
use crate::MAX_NESTING_DEPTH;
use pulldown_cmark::{Event, Options, Tag, TagEnd};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;

/// Import errors: just the nesting guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Container nesting exceeded [`MAX_NESTING_DEPTH`].
    NestingTooDeep { depth: usize, max: usize },
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NestingTooDeep { depth, max } => {
                write!(f, "nesting too deep: {depth} (max {max})")
            }
        }
    }
}
impl std::error::Error for ImportError {}

/// `count` instances of `construct`, something the markdown spelled that the
/// content has no place for, dropped. One per construct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    pub construct: Dropped,
    pub count: usize,
}

/// A construct an import drops. Its [`Display`](std::fmt::Display) is the
/// name `parse::dropped_construct` reports it under, given with each variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Dropped {
    /// A raw tag outside the carrier, or a `qm-anchor` reported with the
    /// markdown its HTML block drops: its lowercase name (`span`, `u`).
    Tag(String),
    /// A tag named with the carrier's prefix and no element name after it: its
    /// lowercase name (`qm-a--b`).
    BadName(String),
    /// A wrapper left unclosed, self-closing, inside a line of text or beside
    /// another tag in a block tight against markdown; a `qm-table` that does
    /// not hold exactly one table; a `qm-cell` pair that does not wrap a whole
    /// table cell: `qm-<name>`.
    Element(String),
    /// A wrapper attribute outside the grammar or repeating a name already
    /// read, and a `qm-table` or `qm-cell` attribute its table or cell does
    /// not fold: `qm-<name>[<attr>]`.
    ElementAttr { element: String, attr: String },
    /// A footnote definition, which the content has no construct for:
    /// `footnote`.
    Footnote,
}

impl std::fmt::Display for Dropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use carrier::PREFIX;
        match self {
            Dropped::Tag(name) | Dropped::BadName(name) => f.write_str(name),
            Dropped::Element(name) => write!(f, "{PREFIX}{name}"),
            Dropped::ElementAttr { element, attr } => write!(f, "{PREFIX}{element}[{attr}]"),
            Dropped::Footnote => f.write_str("footnote"),
        }
    }
}

/// A markdown import: the content, and what the markdown spelled that it
/// could not carry, in order of each construct's first occurrence.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "carries the import's warnings; read `.warnings` or bind it"]
pub struct Imported {
    pub content: Normalized,
    pub warnings: Vec<ImportWarning>,
}

/// The CommonMark extensions every markdown parse in this crate runs with.
pub(crate) fn options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options
}

/// Import markdown into a normalized, validated [`Content`], with an
/// [`ImportWarning`] per construct it dropped: each dropped opening tag, a
/// `<pre>`, `<script>`, `<style>` or `<textarea>` block's included, and a block
/// holding no other opening tag under its first where it drops markdown with
/// it. A comment, the content of a type 1–5 HTML block and any other closing
/// tag or `qm-anchor` tag drop silently.
pub fn from_markdown(markdown: &str) -> Result<Imported, ImportError> {
    let mut b = Builder::default();
    parse_markdown(markdown, options(), |text, events| b.run(MarkdownFixer::new(text, events.into_iter())))?;
    let (content, dropped) = b.finish();
    Ok(Imported {
        content: content.into_normalized(),
        warnings: dropped.into_warnings(),
    })
}

/// Import plain text (literal) into a [`Content`]: the literal-codec sibling of
/// [`from_markdown`]. Every character is content, never syntax: `*hi*` is four
/// literal chars, not emphasis. With [`crate::export::to_plaintext`] it pins the
/// fixed point `to_plaintext(from_plaintext(s)) == s` for any `s` free of `\r`,
/// bidi controls, line separators (VT, FF, NEL, U+2028, U+2029), and
/// [`ISLAND_SLOT`].
///
/// Line structure is **derived, not stored**: a lone `\n` between two non-empty
/// segments is a within-paragraph break ([`Line::continues`] `true`); a blank
/// line is a paragraph boundary. The text is stored verbatim, so the round trip
/// is byte-exact however structure is later re-derived.
pub fn from_plaintext(s: &str) -> Normalized {
    // Boundary cleanup so the content invariants hold; clean plaintext passes
    // through untouched.
    let text: String = s
        .chars()
        .filter(|&c| c != ISLAND_SLOT)
        .filter_map(crate::normalize::admit_char)
        .collect();
    // One line per `\n`-separated segment. A single streaming pass carries the
    // prior segment's non-emptiness, so line 0 is `false` and no intermediate
    // segment vector is allocated.
    let mut prev_nonempty = false;
    let lines = text
        .split('\n')
        .map(|seg| {
            let continues = prev_nonempty && !seg.is_empty();
            prev_nonempty = !seg.is_empty();
            Line::new(LineKind::Para).with_continues(continues)
        })
        .collect();
    Content {
        text,
        lines,
        marks: Vec::new(),
        islands: Vec::new(),
    }
    .into_normalized()
}

/// A flat inline accumulator: `text` plus `marks` over local USV offsets, with
/// the content char-filtering baked in. Serves both a prose line's inline
/// content (embedded in the [`Builder`]) and a table cell's isolated content.
#[derive(Default)]
struct Inline {
    text: String,
    /// USV position = char count of [`Self::text`].
    pos: usize,
    marks: Vec<Mark>,
    /// `(kind, start)` for each mark opened but not yet closed.
    open: Vec<(MarkKind, usize)>,
    /// Each `<u>` this run has read and whose `</u>` it awaits, innermost last:
    /// a bare one's start position and tag offset, `None` for one carrying an
    /// attribute, which pairs and underlines nothing.
    underlines: Vec<Option<(usize, usize)>>,
}

impl Inline {
    /// Append inline text under the [`crate::normalize::admit_char`] contract,
    /// with a stray [`ISLAND_SLOT`] dropped and a stray `\n` spaced, real line
    /// boundaries going through [`Self::push_raw`]. Admitting a bare slot char
    /// would break the slot-count invariant.
    fn push_text(&mut self, s: &str) {
        for c in s.chars() {
            let c = match c {
                ISLAND_SLOT => continue,
                '\n' => ' ',
                other => match crate::normalize::admit_char(other) {
                    Some(c) => c,
                    None => continue,
                },
            };
            self.text.push(c);
            self.pos += 1;
        }
    }

    /// Append one char verbatim (a line-boundary `\n`, an island slot), bypassing
    /// the [`Self::push_text`] filtering.
    fn push_raw(&mut self, c: char) {
        self.text.push(c);
        self.pos += 1;
    }

    fn open_mark(&mut self, kind: MarkKind) {
        self.open.push((kind, self.pos));
    }

    /// Close the innermost open mark (pulldown nests them well).
    fn close_mark(&mut self) {
        if let Some((kind, start)) = self.open.pop() {
            self.marks.push(Mark {
                start,
                end: self.pos,
                kind,
            });
        }
    }

    /// Close every mark left open (malformed input).
    fn close_marks(&mut self) {
        while !self.open.is_empty() {
            self.close_mark();
        }
    }

    /// Pair one `<u>` tag: a `</u>` closes the innermost open `<u>` wherever
    /// it sits among the marks, so an underline crosses any other, and one
    /// closing nothing drops silently.
    fn underline(&mut self, tag: UTag) {
        match tag {
            UTag::Open { at } => self.underlines.push(Some((self.pos, at))),
            UTag::Held => self.underlines.push(None),
            UTag::Close => {
                if let Some(Some((start, _))) = self.underlines.pop() {
                    if start < self.pos {
                        self.marks.push(Mark {
                            start,
                            end: self.pos,
                            kind: MarkKind::Underline,
                        });
                    }
                }
            }
        }
    }

    /// End the run: each bare `<u>` still open drops as unclosed.
    fn drop_open(&mut self, dropped: &mut Drops) {
        for (_, at) in self.underlines.drain(..).flatten() {
            dropped.add(Dropped::Tag("u".into()), at);
        }
    }

    /// Append inline code text and record its [`MarkKind::Code`] mark over it.
    fn push_code(&mut self, s: &str) {
        let start = self.pos;
        self.push_text(s);
        self.marks.push(Mark {
            start,
            end: self.pos,
            kind: MarkKind::Code,
        });
    }

    /// Drop `lead` chars from the start and `trail` from the end, the marks
    /// moving with the text.
    fn trim(&mut self, lead: usize, trail: usize) {
        let end = self.pos.saturating_sub(trail).max(lead);
        self.text = self.text.chars().skip(lead).take(end - lead).collect();
        self.pos = end - lead;
        for m in &mut self.marks {
            m.start = m.start.clamp(lead, end) - lead;
            m.end = m.end.clamp(lead, end) - lead;
        }
    }
}

/// A wrapper whose open tag line the import has read and whose close it
/// awaits.
struct Opened {
    /// The [wrapper](carrier::wrapper) name its tag carries.
    name: String,
    frame: Frame,
    /// Its attributes, reported or folded once it closes: one left unclosed
    /// drops whole and reports only itself.
    attrs: carrier::Attrs,
    /// The open tag's byte offset.
    at: usize,
    /// The containers open around it.
    depth: usize,
    /// Whether a close tag naming it dropped with markdown, which reports it
    /// once whether or not it drops unclosed.
    reported: bool,
}

enum Frame {
    /// An element, around its container's `instance`.
    Element { instance: u64 },
    /// A `qm-table` wrapper, opened with `islands` minted and `lines`
    /// [emitted](Builder::emitted), and whether another opened inside it.
    Table { islands: usize, lines: usize, holds_wrapper: bool },
}

#[derive(Default)]
struct Builder {
    /// The content text + marks; the [`Builder`] adds line/block structure around
    /// it (a `\n` boundary is [`Inline::push_raw`], inline content is the mark
    /// machinery). A table cell reuses the same [`Inline`] in isolation.
    inline: Inline,
    lines: Vec<Line>,
    cur: Option<Line>, // the line currently open (kind + containers fixed at open)
    /// A block start records `(kind, continues)` the next inline content should
    /// open a fresh line with. Set at Paragraph/Heading/Item (tight lists emit no
    /// Paragraph wrapper, so Item must force a line) with `continues = false`; a
    /// hard break sets `continues = true`. Cleared when a block that owns its own
    /// lines (List/Quote/CodeBlock/Table) takes over.
    pending: Option<(LineKind, bool)>,
    islands: Vec<Island>,
    containers: Vec<Container>,
    /// Parallel to `containers`: the [`Self::emitted`] count when each container
    /// opened, so a container that closes having emitted no line (an empty `>`
    /// quote, an empty `- ` item) can still get one.
    container_marks: Vec<usize>,
    list_stack: Vec<ListInfo>,
    /// Bumped at every container open, so two adjacent runs of one shape never
    /// carry the same `instance`. Only distinctness matters: `normalize`
    /// rewrites these to the canonical `0`/`1` alternation.
    next_instance: u64,
    // code block
    code_lang: Option<String>,
    in_code: bool,
    code_opened: bool, // whether the current code block has opened its first line
    /// Open-image nesting. An image's alt collects into `image_alt`, or, a
    /// table cell having no island slot, into the cell as plain text, its url
    /// dropping.
    image_depth: usize,
    image_url: String,
    image_alt: String,
    // table collection
    table: Option<TableAcc>,
    /// The wrappers open, innermost last.
    blocks: Vec<Opened>,
    /// The instances of block elements left unclosed, which
    /// [`Self::finish`] strips from every line's path.
    unclosed: Vec<u64>,
    /// What this walk drops: a raw tag, an unclosed `<u>`, a wrapper left
    /// unclosed or outside a tag line, an attribute it cannot carry.
    dropped: Drops,
}

#[derive(Clone)]
struct ListInfo {
    ordered: bool,
    start: u64,
    /// 0-based index of the next item: becomes the item's `ordinal`.
    count: u64,
    /// Shared by every item of this list, so the items of one list group and
    /// an adjacent list of the same shape does not join them.
    instance: u64,
}

#[derive(Default)]
struct TableAcc {
    aligns: Vec<&'static str>,
    /// Cells as canonical `{text, marks}` JSON (via `serial::cell_to_value`), so
    /// nothing downstream re-parses markdown to render a formatted cell.
    header: Vec<serde_json::Value>,
    rows: Vec<Vec<serde_json::Value>>,
    cur_row: Vec<serde_json::Value>,
    in_head: bool,
    /// The cell currently open (between `Tag::TableCell` start/end), building its
    /// inline text + marks with the same [`Inline`] machinery prose uses.
    cell: Option<Inline>,
    /// The current cell's `qm-cell` tags.
    pair: CellPair,
}

/// A table cell's `qm-cell` tags as they arrive. The pair folds when its open
/// tag is the cell's first inline and its close tag the cell's last.
#[derive(Default)]
struct CellPair {
    state: PairState,
    /// Each open tag's offset, reported where no pair folds.
    opens: Vec<usize>,
    /// The raw space after the open tag and before the close tag, which a
    /// pair that folds trims.
    edges: (usize, usize),
}

#[derive(Default)]
enum PairState {
    #[default]
    Empty,
    Open(carrier::Attrs),
    Closed(carrier::Attrs),
    /// Content before the pair or after it, or a tag no pair takes.
    Spoiled,
}

impl CellPair {
    fn open(&mut self, attrs: carrier::Attrs, at: usize, space: usize) {
        self.opens.push(at);
        self.edges.0 = space;
        self.state = match std::mem::take(&mut self.state) {
            PairState::Empty => PairState::Open(attrs),
            _ => PairState::Spoiled,
        };
    }

    fn close(&mut self, space: usize) {
        self.edges.1 = space;
        self.state = match std::mem::take(&mut self.state) {
            PairState::Open(attrs) => PairState::Closed(attrs),
            _ => PairState::Spoiled,
        };
    }

    /// Any other inline the cell reads.
    fn content(&mut self) {
        if !matches!(self.state, PairState::Open(_)) {
            self.state = PairState::Spoiled;
        }
    }

    /// The attributes of the pair wrapping the whole cell, which trims `cell`'s
    /// edge space, and its open tag's offset. Where none does, each open tag
    /// drops as `qm-cell`.
    fn finish(self, cell: &mut Inline, dropped: &mut Drops) -> Option<(carrier::Attrs, usize)> {
        match self.state {
            PairState::Closed(attrs) => {
                cell.trim(self.edges.0, self.edges.1);
                Some((attrs, self.opens[0]))
            }
            _ => {
                for at in self.opens {
                    dropped.add(Dropped::Element(CELL.into()), at);
                }
                None
            }
        }
    }
}

/// The element name a table cell's pair carries.
const CELL: &str = "cell";

fn align_str(a: &pulldown_cmark::Alignment) -> &'static str {
    match a {
        pulldown_cmark::Alignment::None => "none",
        pulldown_cmark::Alignment::Left => "left",
        pulldown_cmark::Alignment::Center => "center",
        pulldown_cmark::Alignment::Right => "right",
    }
}

impl Builder {
    /// Open a fresh line with `kind` and the current container path. The first
    /// open sets the line directly; each later one first closes the previous
    /// line with a single `\n` boundary, so `lines.len()` always equals the
    /// `\n`-segment count.
    fn open_line(&mut self, kind: LineKind, continues: bool) {
        // The first line (no line yet open) can never continue anything.
        let continues = continues && self.cur.is_some();
        if let Some(prev) = self.cur.take() {
            self.inline.push_raw('\n');
            self.lines.push(prev);
        }
        self.cur = Some(Line {
            kind,
            containers: self.containers.clone(),
            continues,
        });
    }

    /// Open a fresh line for a `pending_kind` set at the last block start, or
    /// (defensively) a `default` line if inline content arrives with none
    /// pending and no line open. A no-op when a line is already open and no new
    /// one is pending.
    fn ensure_open(&mut self, default: LineKind) {
        if let Some((k, cont)) = self.pending.take() {
            self.open_line(k, cont);
        } else if self.cur.is_none() {
            self.open_line(default, false);
        }
    }

    fn push_inline(&mut self, s: &str) {
        self.ensure_open(LineKind::Para);
        self.inline.push_text(s);
    }

    /// A container-instance value nothing else in this import holds.
    fn mint_instance(&mut self) -> u64 {
        self.next_instance += 1;
        self.next_instance
    }

    /// Lines emitted so far, counting the line currently open. A container that
    /// closes with this unchanged from when it opened produced nothing.
    fn emitted(&self) -> usize {
        self.lines.len() + usize::from(self.cur.is_some())
    }

    /// Open a line for a block that ended with no inline content (an empty
    /// heading `#`): otherwise the block, and any content model it carries, is
    /// silently lost. An empty *paragraph* — all of whose inline content was
    /// stripped HTML — is the exception: markdown spells it with nothing, so a
    /// line kept here would export as a blank line that re-import collapses,
    /// costing the fixed point. A container the drop leaves empty still gets
    /// its line from [`Self::close_container`].
    fn flush_empty_block(&mut self) {
        let Some((kind, continues)) = self.pending.take() else {
            return;
        };
        if matches!(kind, LineKind::Para) {
            return;
        }
        self.open_line(kind, continues);
    }

    /// Close a container: if it emitted no line, give it one empty `Para` line
    /// (an empty `- ` item, an empty `>` quote) so the structure survives; then
    /// pop it. `mark` is the [`Self::emitted`] snapshot from when it opened.
    fn close_container(&mut self, mark: usize) {
        if self.emitted() == mark {
            self.pending = None;
            self.open_line(LineKind::Para, false);
        }
        self.containers.pop();
    }

    fn open_mark(&mut self, kind: MarkKind) {
        // Resolve any armed line first, so a mark that begins a block records
        // the position *after* the block's line boundary. Otherwise the mark
        // swallows the separator and equal content from an editor vs from
        // import serializes to different canonical bytes.
        self.ensure_open(LineKind::Para);
        self.inline.open_mark(kind);
    }

    /// Minting `isl-{seq}` by position keeps import a pure function.
    fn mint_island(&mut self, kind: IslandType, props: serde_json::Value) {
        let id = format!("isl-{}", self.islands.len());
        self.islands.push(Island {
            id,
            island_type: kind,
            props,
        });
    }

    fn check_depth(&self) -> Result<(), ImportError> {
        // Container path plus open marks approximates the structural depth the
        // typst backend caps; bound it identically for parity. Nested
        // underlines union into one mark, so they count once.
        let underlined = self.inline.underlines.iter().any(Option::is_some);
        let depth = self.containers.len()
            + self.inline.open.len()
            + usize::from(underlined);
        if depth > MAX_NESTING_DEPTH {
            return Err(ImportError::NestingTooDeep {
                depth,
                max: MAX_NESTING_DEPTH,
            });
        }
        Ok(())
    }

    fn run<'a, I>(&mut self, iter: I) -> Result<(), ImportError>
    where
        I: Iterator<Item = Fixed<'a>>,
    {
        for item in iter {
            let event = match item {
                Fixed::Event(event) => event,
                Fixed::Carrier(tag) => {
                    self.carrier_tag(tag)?;
                    continue;
                }
                Fixed::Underline(tag) => {
                    self.underline_tag(tag)?;
                    continue;
                }
                Fixed::Swallowed(name, at) => {
                    self.swallowed(name, at);
                    continue;
                }
                Fixed::Dropped(construct, at) => {
                    self.dropped.add(construct, at);
                    continue;
                }
            };
            // An inline run ends at the first event outside it, and the
            // underlines it left open with it.
            if self.image_depth == 0 && self.table.is_none() && !is_inline(&event) {
                self.inline.drop_open(&mut self.dropped);
            }
            // Image alt collection intercepts everything until the image closes.
            if self.image_depth > 0 {
                match &event {
                    Event::Start(Tag::Image { .. }) => self.image_depth += 1,
                    Event::End(TagEnd::Image) => {
                        self.image_depth -= 1;
                        if self.image_depth == 0 && self.table.is_none() {
                            self.emit_image();
                        }
                    }
                    other => {
                        let alt = match other {
                            Event::Text(t) | Event::Code(t) => &**t,
                            Event::SoftBreak | Event::HardBreak => " ",
                            _ => "",
                        };
                        match self.table.as_mut().and_then(|acc| acc.cell.as_mut()) {
                            Some(cell) => cell.push_text(alt),
                            None => self.image_alt.push_str(alt),
                        }
                    }
                }
                continue;
            }

            // Table collection routes structural events and cell inline content
            // to the accumulator, so each cell is stored as canonical
            // `{text, marks}` with no markdown re-parse downstream.
            if self.table.is_some() {
                self.table_event(&event);
                if matches!(event, Event::End(TagEnd::Table)) {
                    self.emit_table();
                }
                continue;
            }

            match event {
                Event::Start(tag) => self.start_tag(tag)?,
                Event::End(tag) => self.end_tag(tag),
                Event::Text(t) => {
                    if self.in_code {
                        self.push_code_content(&t);
                    } else {
                        self.push_inline(&t);
                    }
                }
                Event::Code(t) => {
                    self.ensure_open(LineKind::Para);
                    self.inline.push_code(&t);
                }
                Event::Rule => {
                    self.open_line(LineKind::Rule, false);
                    self.rearm_item();
                }
                // A soft break that would open its block's first line, all
                // before it having dropped, is nothing.
                Event::SoftBreak if matches!(self.pending, Some((_, false))) => {}
                Event::SoftBreak => self.push_inline(" "),
                Event::TaskListMarker(done) => {
                    if let Some(Container::ListItem { checked, .. }) = self.containers.last_mut() {
                        *checked = Some(done);
                    }
                }
                Event::HardBreak => {
                    // A break with no text before it on its line is dropped:
                    // pulldown never emits one there, an inline `<br>` can, and
                    // arming a continuation would join the block to the one above.
                    let line_empty = matches!(self.pending, Some((_, false)))
                        || self.cur.is_none()
                        || self.inline.text.is_empty()
                        || self.inline.text.ends_with('\n');
                    if line_empty {
                        continue;
                    }
                    match self.cur.as_ref().map(|l| &l.kind) {
                        // A heading is one line.
                        Some(LineKind::Heading { .. }) => self.push_inline(" "),
                        // Elsewhere, arm a continuation line so the block stays
                        // one block and export re-emits a hard break.
                        _ => {
                            let kind = self
                                .cur
                                .as_ref()
                                .map(|l| l.kind.clone())
                                .unwrap_or(LineKind::Para);
                            self.pending = Some((kind, true));
                        }
                    }
                }
                // Html/InlineHtml already stripped or rewritten by the fixer.
                _ => {}
            }
        }
        Ok(())
    }

    fn start_tag<'a>(&mut self, tag: Tag<'a>) -> Result<(), ImportError> {
        match tag {
            // Block starts arm a pending line (new block, continues = false);
            // the next inline content opens it.
            Tag::Paragraph => self.pending = Some((LineKind::Para, false)),
            Tag::Heading { level, .. } => {
                self.pending = Some((LineKind::Heading { level: level as u8 }, false))
            }
            Tag::CodeBlock(kind) => {
                self.pending = None; // code opens its own lines
                self.in_code = true;
                self.code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        let l = sanitize_lang(&lang);
                        if l.is_empty() {
                            None
                        } else {
                            Some(l)
                        }
                    }
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };
                // The first code line opens on the first content chunk; an empty
                // block gets its line at `TagEnd::CodeBlock`.
                self.code_opened = false;
            }
            Tag::List(start) => {
                self.pending = None; // nested list content sets its own
                let instance = self.mint_instance();
                self.list_stack.push(ListInfo {
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    count: 0,
                    instance,
                });
            }
            Tag::Item => {
                // Tight-list items carry no Paragraph wrapper, so the item start
                // is what forces a new line for the item's first inline content.
                self.pending = Some((LineKind::Para, false));
                self.container_marks.push(self.emitted());
                let container = match self.list_stack.last_mut() {
                    Some(info) => {
                        let ordinal = info.count;
                        info.count += 1;
                        Container::ListItem {
                            ordered: info.ordered,
                            start: info.start,
                            ordinal,
                            checked: None,
                            instance: info.instance,
                        }
                    }
                    None => Container::ListItem {
                        ordered: false,
                        start: 1,
                        ordinal: 0,
                        checked: None,
                        instance: 0,
                    },
                };
                self.containers.push(container);
                self.check_depth()?;
            }
            Tag::BlockQuote(_) => {
                self.pending = None; // quote content sets its own
                self.container_marks.push(self.emitted());
                let instance = self.mint_instance();
                self.containers.push(Container::Quote { instance });
                self.check_depth()?;
            }
            Tag::Table(aligns) => {
                self.pending = None;
                self.open_line(LineKind::Para, false);
                self.inline.push_raw(ISLAND_SLOT);
                self.table = Some(TableAcc {
                    aligns: aligns.iter().map(align_str).collect(),
                    ..TableAcc::default()
                });
            }
            Tag::Image { dest_url, .. } => {
                self.image_url = dest_url.to_string();
                self.image_alt.clear();
                self.image_depth = 1;
            }
            tag => {
                if let Some(kind) = mark_kind(&tag) {
                    self.open_mark(kind);
                    self.check_depth()?;
                }
            }
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::CodeBlock => {
                if !self.code_opened {
                    // Empty code block: one empty Code line.
                    let lang = self.code_lang.take();
                    self.open_line(LineKind::Code { lang }, false);
                }
                self.in_code = false;
                self.code_lang = None;
                self.rearm_item();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.rearm_item();
            }
            TagEnd::Item => {
                self.drop_unclosed_blocks();
                let mark = self.container_marks.pop().unwrap_or(0);
                self.close_container(mark);
            }
            TagEnd::BlockQuote(_) => {
                self.drop_unclosed_blocks();
                let mark = self.container_marks.pop().unwrap_or(0);
                self.close_container(mark);
                self.rearm_item();
            }
            end if ends_mark(&end) => self.inline.close_mark(),
            // A block that produced no inline content still gets its line.
            TagEnd::Heading(_) | TagEnd::Paragraph => {
                self.flush_empty_block();
                self.rearm_item();
            }
            TagEnd::HtmlBlock | TagEnd::FootnoteDefinition => self.rearm_item(),
            _ => {}
        }
    }

    /// Pair one carrier tag: a tag line opens or closes a wrapper. Any other
    /// open tag drops, and any other close tag drops silently.
    fn carrier_tag(&mut self, tag: CarrierTag) -> Result<(), ImportError> {
        let CarrierTag { name, attrs, block, at, space } = tag;
        if let Some(acc) = self.table.as_mut().filter(|acc| acc.cell.is_some()) {
            match attrs {
                _ if name != CELL || self.image_depth > 0 => {
                    acc.pair.content();
                    if attrs.is_some() {
                        self.dropped.add(Dropped::Element(name), at);
                    }
                }
                Some(attrs) => acc.pair.open(attrs, at, space),
                None => acc.pair.close(space),
            }
            return Ok(());
        }
        match attrs {
            _ if self.image_depth > 0 || self.table.is_some() || !block => {
                if attrs.is_some() {
                    self.dropped.add(Dropped::Element(name), at);
                }
                Ok(())
            }
            Some(attrs) => self.open_wrapper(name, attrs, at),
            None => {
                self.close_wrapper(&name);
                Ok(())
            }
        }
    }

    fn open_wrapper(&mut self, name: String, attrs: carrier::Attrs, at: usize) -> Result<(), ImportError> {
        let depth = self.containers.len();
        let frame = if name == "table" {
            let outer = self.blocks.iter_mut().rev().find_map(|open| match &mut open.frame {
                Frame::Table { holds_wrapper, .. } => Some(holds_wrapper),
                Frame::Element { .. } => None,
            });
            if let Some(holds_wrapper) = outer {
                *holds_wrapper = true;
            }
            Frame::Table {
                islands: self.islands.len(),
                lines: self.emitted(),
                holds_wrapper: false,
            }
        } else {
            let instance = self.mint_instance();
            self.container_marks.push(self.emitted());
            self.containers.push(Container::Element {
                name: name.clone(),
                attrs: attrs.values.clone(),
                instance,
            });
            Frame::Element { instance }
        };
        self.blocks.push(Opened { name, frame, attrs, at, depth, reported: false });
        self.check_depth()
    }

    /// Close the innermost wrapper open, where `name` names it and every
    /// container opened inside it has closed; any other close tag drops
    /// silently. A `qm-table` folds its attributes into the table it wraps
    /// where what it wraps imported as that table alone: one island and its
    /// one line, inside no container but an element. Otherwise it drops whole.
    fn close_wrapper(&mut self, name: &str) {
        let innermost = self.blocks.last().is_some_and(|open| {
            let own_container = usize::from(matches!(open.frame, Frame::Element { .. }));
            open.name == name && self.containers.len() == open.depth + own_container
        });
        let Some(Opened { name, frame, attrs, at, .. }) = self.blocks.pop_if(|_| innermost) else {
            return;
        };
        match frame {
            Frame::Element { .. } => {
                let mark = self.container_marks.pop().unwrap_or(0);
                self.close_container(mark);
                self.dropped.attrs(&name, attrs.refused, at);
            }
            Frame::Table { islands, lines, holds_wrapper } => {
                let depth = self.containers.len();
                let alone = !holds_wrapper
                    && self.islands.len() == islands + 1
                    && self.islands[islands].island_type == IslandType::Table
                    && self.emitted() == lines + 1
                    && self.cur.as_ref().and_then(|l| l.containers.get(depth..)).is_some_and(|inside| {
                        inside.iter().all(|c| matches!(c, Container::Element { .. }))
                    });
                match alone {
                    true => self.dropped.fold(&name, attrs, at, &mut self.islands[islands].props, carrier::table::prop),
                    false => self.dropped.add(Dropped::Element(name), at),
                }
            }
        }
    }

    /// Pair one `<u>` tag in the run or table cell it stands in. In an image's
    /// alt text, where nothing is marked, it underlines nothing.
    fn underline_tag(&mut self, tag: UTag) -> Result<(), ImportError> {
        if self.image_depth > 0 {
            return Ok(());
        }
        if let Some(acc) = self.table.as_mut() {
            if let Some(cell) = acc.cell.as_mut() {
                acc.pair.content();
                cell.underline(tag);
            }
            return Ok(());
        }
        if matches!(tag, UTag::Open { .. }) {
            // Resolve an armed line first, as a mark does, so the underline
            // starts after the line boundary.
            self.ensure_open(LineKind::Para);
        }
        self.inline.underline(tag);
        self.check_depth()
    }

    /// Drop each wrapper still open where its list item, quote or the body
    /// ends: its tags drop and what it wraps stays, [`Self::finish`] taking an
    /// element off every line's path.
    fn drop_unclosed_blocks(&mut self) {
        let ending = self
            .containers
            .iter()
            .rposition(|c| !matches!(c, Container::Element { .. }))
            .map_or(0, |k| k + 1);
        let inside = self.blocks.partition_point(|open| open.depth < ending);
        for open in self.blocks.split_off(inside) {
            if let Frame::Element { instance } = open.frame {
                self.containers.pop();
                self.container_marks.pop();
                self.unclosed.push(instance);
            }
            if !open.reported {
                self.dropped.add(Dropped::Element(open.name), open.at);
            }
        }
    }

    /// Report a close tag that dropped with the markdown under it, once for
    /// the innermost wrapper it names: that wrapper stays open past it.
    fn swallowed(&mut self, name: String, at: usize) {
        match self.blocks.iter_mut().rev().find(|open| open.name == name) {
            Some(open) if open.reported => return,
            Some(open) => open.reported = true,
            None => {}
        }
        self.dropped.add(Dropped::Element(name), at);
    }

    /// A tight list item's inline content arrives with no `Paragraph` start to
    /// arm its line, so after a block nested in the item the next text must be
    /// armed here or it joins that block's last line.
    fn rearm_item(&mut self) {
        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            self.pending = Some((LineKind::Para, false));
        }
    }

    fn push_code_content(&mut self, content: &str) {
        // pulldown appends a trailing newline as the last line's terminator, not
        // content; drop exactly one so an N-line block yields N lines.
        let content = content.strip_suffix('\n').unwrap_or(content);
        for seg in content.split('\n') {
            // First line of the block starts it (continues = false); every later
            // line is a within-block continuation, so the fence stays one block.
            let continues = self.code_opened;
            self.open_line(
                LineKind::Code {
                    lang: self.code_lang.clone(),
                },
                continues,
            );
            self.code_opened = true;
            self.inline.push_text(seg);
        }
    }

    /// Route one table event: structural events shape the accumulator, inline
    /// events build the open cell with the same [`Inline`] machinery prose uses.
    /// A cell is flat inline, so its marks are USV offsets into its own text.
    fn table_event(&mut self, event: &Event) {
        let Some(acc) = self.table.as_mut() else {
            return;
        };
        match event {
            Event::Start(Tag::TableHead) => acc.in_head = true,
            Event::End(TagEnd::TableHead) => {
                acc.header = std::mem::take(&mut acc.cur_row);
                acc.in_head = false;
            }
            Event::Start(Tag::TableRow) => acc.cur_row.clear(),
            Event::End(TagEnd::TableRow) => {
                if !acc.in_head {
                    let row = std::mem::take(&mut acc.cur_row);
                    acc.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                acc.cell = Some(Inline::default());
                acc.pair = CellPair::default();
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(mut cell) = acc.cell.take() {
                    cell.close_marks();
                    cell.drop_open(&mut self.dropped);
                    let folded = std::mem::take(&mut acc.pair).finish(&mut cell, &mut self.dropped);
                    let mut value = crate::serial::cell_to_value(&cell.text, &cell.marks);
                    if let Some((attrs, at)) = folded {
                        self.dropped.fold(CELL, attrs, at, &mut value, carrier::cell::key);
                    }
                    acc.cur_row.push(value);
                }
            }
            _ => {
                let Some(cell) = acc.cell.as_mut() else {
                    return;
                };
                acc.pair.content();
                match event {
                    Event::Start(Tag::Image { .. }) => self.image_depth = 1,
                    Event::Text(t) => cell.push_text(t),
                    Event::Code(t) => cell.push_code(t),
                    Event::SoftBreak => cell.push_text(" "),
                    // A hard break is a `\n` in the cell's text, as in prose.
                    Event::HardBreak => cell.push_raw('\n'),
                    Event::Start(tag) => {
                        if let Some(kind) = mark_kind(tag) {
                            cell.open_mark(kind);
                        }
                    }
                    Event::End(end) if ends_mark(end) => cell.close_mark(),
                    _ => {}
                }
            }
        }
    }

    fn emit_table(&mut self) {
        if let Some(acc) = self.table.take() {
            let props = json!({
                "aligns": acc.aligns,
                "header": acc.header,
                "rows": acc.rows,
            });
            self.mint_island(IslandType::Table, props);
            self.rearm_item();
        }
    }

    fn emit_image(&mut self) {
        self.ensure_open(LineKind::Para);
        self.inline.push_raw(ISLAND_SLOT);
        let props = json!({
            "url": self.image_url,
            "alt": self.image_alt.trim(),
        });
        self.mint_island(IslandType::Image, props);
    }

    fn finish(mut self) -> (Content, Drops) {
        self.inline.drop_open(&mut self.dropped);
        self.drop_unclosed_blocks();
        if let Some(last) = self.cur.take() {
            self.lines.push(last);
        }
        if !self.unclosed.is_empty() {
            let unclosed = &self.unclosed;
            for line in &mut self.lines {
                line.containers.retain(
                    |c| !matches!(c, Container::Element { instance, .. } if unclosed.contains(instance)),
                );
            }
        }
        if self.lines.is_empty() {
            self.lines.push(Line::new(LineKind::Para));
        }
        self.inline.close_marks();
        let content = Content {
            text: self.inline.text,
            lines: self.lines,
            marks: self.inline.marks,
            islands: self.islands,
        };
        (content, self.dropped)
    }
}

fn mark_kind(tag: &Tag) -> Option<MarkKind> {
    Some(match tag {
        Tag::Emphasis => MarkKind::Emph,
        Tag::Strong => MarkKind::Strong,
        Tag::Strikethrough => MarkKind::Strike,
        Tag::Link { dest_url, .. } => MarkKind::Link { url: dest_url.to_string() },
        _ => return None,
    })
}

fn ends_mark(end: &TagEnd) -> bool {
    matches!(end, TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link)
}

/// Whether `event` stands inside an inline run.
fn is_inline(event: &Event) -> bool {
    match event {
        Event::Text(_) | Event::Code(_) | Event::SoftBreak | Event::HardBreak | Event::InlineHtml(_) => true,
        Event::Start(tag) => matches!(tag, Tag::Image { .. }) || mark_kind(tag).is_some(),
        Event::End(end) => matches!(end, TagEnd::Image) || ends_mark(end),
        _ => false,
    }
}

/// A code-block info string reduced to a language identifier: its leading run of
/// ASCII alphanumerics and `-`/`_`/`.`/`+`. Every stored `lang` has this shape,
/// so an emitter writes it into its own syntax unquoted and unescaped. Every
/// lane that mints a `LineKind::Code` runs it — the storage decode and the op
/// wires as much as this importer.
pub(crate) fn sanitize_lang(lang: &str) -> String {
    lang.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}

// `MarkdownFixer` is the raw-HTML filter between pulldown and the builder: it
// passes each inline `<u>` and `</u>` and each carrier tag for the builder to
// pair, allowlists an inline `<br>` as a hard break, and drops every other raw
// HTML event, an HTML block whole. It reports what it drops where it drops it,
// so the warnings and the drop cannot disagree.
// Delimiter arithmetic stays pulldown's, since a fixer that re-segments `***`
// runs can only disagree with CommonMark, and disagreeing means deleting an
// asterisk the author typed.

/// What the fixer hands the builder.
enum Fixed<'a> {
    Event(Event<'a>),
    Carrier(CarrierTag),
    Underline(UTag),
    /// A wrapper's close tag in an HTML block of closing tags that drops
    /// markdown with them, at the block's byte offset.
    Swallowed(String, usize),
    /// A construct the fixer dropped, at its byte offset.
    Dropped(Dropped, usize),
}

/// An inline `<u>` tag, paired by the builder as HTML pairs it.
#[derive(Clone, Copy)]
enum UTag {
    /// A bare `<u>`, at this byte offset: it underlines what it wraps.
    Open { at: usize },
    /// A `<u>` carrying an attribute, or self-closing: it drops, and still
    /// pairs with a `</u>`.
    Held,
    Close,
}

/// An open or close tag of a wrapper the import pairs: not self-closing.
struct CarrierTag {
    /// The [wrapper](carrier::wrapper) name.
    name: String,
    /// The open tag's attributes; `None` for a close tag.
    attrs: Option<carrier::Attrs>,
    /// Whether it stands on a tag line rather than inside a line of text.
    block: bool,
    /// The tag's byte offset, or its HTML block's.
    at: usize,
    /// The space and tab in the source on an inline tag's inner side: after an
    /// open tag, before a close tag.
    space: usize,
}

impl CarrierTag {
    fn of(tag: &html::Tag, block: bool, at: usize) -> Option<Self> {
        Some(CarrierTag {
            name: carrier::wrapper(tag.name).filter(|_| !tag.self_closing)?,
            attrs: (!tag.closing).then(|| carrier::decode_attrs(&tag.attrs)),
            block,
            at,
            space: 0,
        })
    }
}

/// What a tag named `name` reports where its opening tag drops: a wrapper as
/// its element, a `qm-*` name outside the grammar as its bad name, and any
/// other tag by its name. `qm-anchor`, the engine's own read-only spelling of
/// an anchor, is written to be dropped and reports nothing.
fn dropped_tag(name: &str) -> Option<Dropped> {
    let lower = name.to_ascii_lowercase();
    Some(match carrier::element(name) {
        Some(element) if element == "anchor" => return None,
        Some(element) => Dropped::Element(element),
        None if lower.starts_with(carrier::PREFIX) => Dropped::BadName(lower),
        None => Dropped::Tag(lower),
    })
}

/// Per construct, in order of its first report: its count and the byte offset
/// of its first occurrence.
#[derive(Default)]
struct Drops {
    entries: Vec<(Dropped, usize, usize)>,
    index: HashMap<Dropped, usize>,
}

impl Drops {
    fn add(&mut self, construct: Dropped, at: usize) {
        match self.index.get(&construct) {
            Some(&k) => {
                let (_, count, first) = &mut self.entries[k];
                *count += 1;
                *first = (*first).min(at);
            }
            None => {
                self.index.insert(construct.clone(), self.entries.len());
                self.entries.push((construct, 1, at));
            }
        }
    }

    /// Each attribute of `element` named in `names`, dropped alone.
    fn attrs(&mut self, element: &str, names: Vec<String>, at: usize) {
        for attr in names {
            self.add(Dropped::ElementAttr { element: element.into(), attr }, at);
        }
    }

    /// Fold a wrapper's attributes into the object `into`, each as `read`
    /// spells it; one refused or that `read` cannot spell drops alone.
    fn fold(
        &mut self,
        element: &str,
        attrs: carrier::Attrs,
        at: usize,
        into: &mut serde_json::Value,
        read: fn(&str, &str) -> Option<serde_json::Value>,
    ) {
        let mut unread = attrs.refused;
        for (name, value) in attrs.values {
            match (read(&name, &value), into.as_object_mut()) {
                (Some(v), Some(o)) => {
                    o.insert(name, v);
                }
                _ => unread.push(name),
            }
        }
        self.attrs(element, unread, at);
    }

    fn into_warnings(mut self) -> Vec<ImportWarning> {
        self.entries.sort_by_key(|&(_, _, first)| first);
        self.entries
            .into_iter()
            .map(|(construct, count, _)| ImportWarning { construct, count })
            .collect()
    }
}

struct MarkdownFixer<'a, I> {
    src: &'a str,
    inner: I,
    /// What the fixer hands on ahead of the next event: an HTML block's
    /// carrier tags and drops, a tag line's tags.
    held: VecDeque<Fixed<'a>>,
    /// An event read ahead, past a tag line or a line break, to see where a tag
    /// line ends or whether one follows.
    ahead: Option<(Event<'a>, Range<usize>)>,
    /// Whether the next event opens a line of a paragraph or a list item's text.
    line_start: bool,
    /// Open marks, links, images, headings and tables, where a line of carrier
    /// tags stays text.
    depth: usize,
    /// The `<u>` tags open in the current inline run, where a line of carrier
    /// tags stays text too.
    underlines: usize,
}

/// Whether a tag ending `end` holds a line of carrier tags as text.
fn deepens(end: &TagEnd) -> bool {
    ends_mark(end)
        || matches!(end, TagEnd::Image | TagEnd::Heading(_) | TagEnd::Table)
}

impl<'a, I> MarkdownFixer<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    fn new(src: &'a str, inner: I) -> Self {
        Self {
            src,
            inner,
            held: VecDeque::new(),
            ahead: None,
            line_start: false,
            depth: 0,
            underlines: 0,
        }
    }

    fn read(&mut self) -> Option<(Event<'a>, Range<usize>)> {
        if let Some(ahead) = self.ahead.take() {
            return Some(ahead);
        }
        let (event, range) = self.inner.next()?;
        match &event {
            Event::Start(tag) if deepens(&tag.to_end()) => self.depth += 1,
            Event::End(end) if deepens(end) => self.depth -= 1,
            _ => {}
        }
        Some((event, range))
    }

    /// Hand on what an opening tag reports where it drops.
    fn drop_tag(&mut self, tag: &html::Tag, at: usize) {
        if let Some(construct) = dropped_tag(tag.name).filter(|_| !tag.closing) {
            self.held.push_back(Fixed::Dropped(construct, at));
        }
    }

    /// The end and the tags of the line `event` opens, when it holds only the
    /// open and close tags of wrappers (markdown-spec §6.2).
    fn carrier_line(&self, event: &Event, range: &Range<usize>) -> Option<(usize, Vec<CarrierTag>)> {
        if self.depth > 0 || self.underlines > 0 || !matches!(event, Event::InlineHtml(_)) {
            return None;
        }
        let end = self.src[range.start..].find('\n').map_or(self.src.len(), |i| range.start + i);
        let tags = carrier::tag_line(&self.src[range.start..end])?;
        let carriers = tags.iter().filter_map(|tag| CarrierTag::of(tag, true, range.start + tag.span.start));
        Some((end, carriers.collect()))
    }

    /// Pass on the tags of a line of them ending at `end` and of each such
    /// line under it, the text that follows reading as a paragraph of its own.
    fn take_carrier_lines(&mut self, (mut end, mut tags): (usize, Vec<CarrierTag>)) {
        loop {
            self.held.extend(tags.drain(..).map(Fixed::Carrier));
            let mut next = self.read();
            while let Some((Event::InlineHtml(_) | Event::Text(_), range)) = &next {
                if range.start >= end {
                    break;
                }
                next = self.read();
            }
            let Some((event, range)) = next else { return };
            if !matches!(event, Event::SoftBreak | Event::HardBreak) {
                self.ahead = Some((event, range));
                return;
            }
            let Some((event, range)) = self.read() else { return };
            match self.carrier_line(&event, &range) {
                Some(line) => (end, tags) = line,
                None => {
                    self.held.push_back(Fixed::Event(Event::Start(Tag::Paragraph)));
                    self.ahead = Some((event, range));
                    return;
                }
            }
        }
    }

    /// Consume an HTML block through its end, reporting its
    /// [markup tags](html::block_tags). A block of tag lines alone passes its
    /// carrier tags on; one that drops text with them drops them too, each
    /// open tag reported, and its first tag where no open tag reports: a
    /// wrapper's close through the builder, which knows whether that wrapper
    /// reports already.
    fn drop_html_block(&mut self, at: usize) {
        let mut text = String::new();
        for (event, _) in self.inner.by_ref() {
            match event {
                Event::Html(h) => text.push_str(&h),
                Event::End(TagEnd::HtmlBlock) => break,
                _ => {}
            }
        }
        let tags = html::block_tags(&text);
        let swallows = text.lines().any(|l| !l.trim().is_empty() && html::tag_line(l).is_none());
        let reports = tags.iter().any(|t| !t.closing && dropped_tag(t.name).is_some());
        if let Some(first) = tags.first().filter(|_| swallows && !reports) {
            self.held.push_back(match carrier::wrapper(first.name).filter(|_| first.closing) {
                Some(name) => Fixed::Swallowed(name, at),
                None => {
                    let anchor = Dropped::Tag(first.name.to_ascii_lowercase());
                    Fixed::Dropped(dropped_tag(first.name).unwrap_or(anchor), at)
                }
            });
        }
        for tag in tags {
            match CarrierTag::of(&tag, true, at).filter(|_| !swallows) {
                Some(carrier) => self.held.push_back(Fixed::Carrier(carrier)),
                None => self.drop_tag(&tag, at),
            }
        }
        self.held.push_back(Fixed::Event(Event::End(TagEnd::HtmlBlock)));
    }

    /// Consume a footnote definition through its end, which alone reaches the
    /// builder.
    fn drop_note(&mut self, at: usize) {
        self.held.push_back(Fixed::Dropped(Dropped::Footnote, at));
        let mut depth = 1;
        for (event, _) in self.inner.by_ref() {
            match event {
                Event::Start(Tag::FootnoteDefinition(_)) => depth += 1,
                Event::End(TagEnd::FootnoteDefinition) => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
        self.held.push_back(Fixed::Event(Event::End(TagEnd::FootnoteDefinition)));
    }

    /// One event as the builder takes it, or `None` for one that `held` now
    /// carries or that drops.
    fn fix(&mut self, event: Event<'a>, range: Range<usize>) -> Option<Fixed<'a>> {
        let opens = matches!(event, Event::Start(Tag::Paragraph | Tag::Item) | Event::TaskListMarker(_));
        let line_start = std::mem::replace(&mut self.line_start, opens);
        if line_start {
            if let Some(line) = self.carrier_line(&event, &range) {
                self.take_carrier_lines(line);
                return None;
            }
        }
        if !is_inline(&event) {
            self.underlines = 0;
        }
        Some(match event {
            Event::SoftBreak | Event::HardBreak if self.depth == 0 => {
                let (next, at) = self.read()?;
                match self.carrier_line(&next, &at) {
                    Some(line) => {
                        self.take_carrier_lines(line);
                        return None;
                    }
                    None => self.ahead = Some((next, at)),
                }
                Fixed::Event(event)
            }
            Event::Start(Tag::HtmlBlock) => {
                self.drop_html_block(range.start);
                return None;
            }
            Event::InlineHtml(html) => {
                let tag = html::tag_at(&html, 0)?;
                if tag.name.eq_ignore_ascii_case("u") {
                    self.underlines = if tag.closing {
                        self.underlines.saturating_sub(1)
                    } else {
                        self.underlines + 1
                    };
                    return Some(Fixed::Underline(if tag.closing {
                        UTag::Close
                    } else if tag.attrs.is_empty() && !tag.self_closing {
                        UTag::Open { at: range.start }
                    } else {
                        self.drop_tag(&tag, range.start);
                        UTag::Held
                    }));
                }
                if tag.name.eq_ignore_ascii_case("br") && !tag.closing {
                    return Some(Fixed::Event(Event::HardBreak));
                }
                if let Some(mut carrier) = CarrierTag::of(&tag, false, range.start) {
                    let space = |c: &char| matches!(c, ' ' | '\t');
                    carrier.space = match carrier.attrs {
                        Some(_) => self.src[range.end..].chars().take_while(space).count(),
                        None => self.src[..range.start].chars().rev().take_while(space).count(),
                    };
                    return Some(Fixed::Carrier(carrier));
                }
                self.drop_tag(&tag, range.start);
                return None;
            }
            Event::Start(Tag::FootnoteDefinition(_)) => {
                self.drop_note(range.start);
                return None;
            }
            Event::FootnoteReference(label) => Fixed::Event(Event::Text(format!("[^{label}]").into())),
            other => Fixed::Event(other),
        })
    }
}

impl<'a, I> Iterator for MarkdownFixer<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    type Item = Fixed<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.held.pop_front() {
                return Some(item);
            }
            let (event, range) = self.read()?;
            if let Some(item) = self.fix(event, range) {
                return Some(item);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) mod generate;
    mod properties;

    use super::*;
    use crate::model::LineKind;

    fn imp(md: &str) -> Normalized {
        let rt = from_markdown(md).unwrap().content;
        assert_eq!(rt.validate(), Ok(()), "invariants for {md:?}");
        rt
    }

    fn imp_plain(s: &str) -> Normalized {
        let rt = from_plaintext(s);
        assert_eq!(rt.validate(), Ok(()), "invariants for {s:?}");
        rt
    }

    #[test]
    fn plaintext_is_literal_and_plain() {
        let rt = imp_plain("a *star* and _under_ #hash");
        assert_eq!(rt.text, "a *star* and _under_ #hash");
        assert!(rt.marks.is_empty());
        assert!(rt.islands.is_empty());
        assert!(rt.is_plain());
        assert!(rt.is_inline(), "one line with no formatting is also inline");
    }

    #[test]
    fn plaintext_round_trip_is_verbatim_and_idempotent() {
        for s in ["", "one line", "a\nb", "a\n\nb", "trailing\n", "*not bold*"] {
            let rt = imp_plain(s);
            assert_eq!(crate::export::to_plaintext(&rt), s, "verbatim for {s:?}");
            let rt2 = from_plaintext(&crate::export::to_plaintext(&rt));
            assert_eq!(rt2.text, rt.text, "idempotent for {s:?}");
            assert_eq!(rt2.lines, rt.lines, "idempotent structure for {s:?}");
        }
    }

    /// The literal codec's half of the slot contract; the markdown half is
    /// `prose_drops_a_stray_slot_before_flanking_is_read` below. A stray
    /// `ISLAND_SLOT` is an invariant violation the mint does not repair —
    /// `validate` reports it — so a codec that admitted one would hand out a
    /// `Normalized` that is not, and `imp_plain` would say so. The drop is also
    /// why this codec's fixed point names the character it excludes.
    #[test]
    fn plaintext_drops_a_stray_slot() {
        let rt = imp_plain("a\u{FFFC}b");
        assert_eq!(rt.text, "ab");
        assert!(rt.islands.is_empty());
        assert_eq!(crate::export::to_plaintext(&rt), "ab");
    }

    #[test]
    fn plaintext_derives_continues_from_line_structure() {
        let rt = imp_plain("a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert!(!rt.lines[0].continues);
        assert!(rt.lines[1].continues, "lone \\n is a within-paragraph break");

        let rt = imp_plain("a\n\nb");
        assert_eq!(rt.lines.len(), 3);
        assert!(!rt.lines[0].continues);
        assert!(!rt.lines[1].continues, "the blank line is a paragraph boundary");
        assert!(!rt.lines[2].continues, "text after a blank line starts a new block");
    }

    #[test]
    fn plaintext_strips_invariant_breakers() {
        let rt = imp_plain("a\r\nb");
        assert_eq!(rt.text, "a\nb", "CRLF collapses to LF");
        let rt = imp_plain(&format!("a{ISLAND_SLOT}b"));
        assert_eq!(rt.text, "ab", "the reserved island slot is dropped");
        assert_eq!(rt.islands.len(), 0);
    }

    #[test]
    fn line_separators_are_spaced_at_every_text_ingress() {
        for sep in ['\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}'] {
            let src = format!("intro{sep}- item");
            assert_eq!(imp_plain(&src).text, "intro - item", "plaintext {sep:?}");
            assert_eq!(imp(&src).text, "intro - item", "markdown {sep:?}");

            let held = Content::new(src, vec![Line::new(LineKind::Para)]);
            assert_eq!(
                held.validate(),
                Err(crate::model::Invariant::LineSeparator(sep))
            );
        }
    }

    #[test]
    fn plain_paragraph() {
        let rt = imp("Hello world");
        assert_eq!(rt.text, "Hello world");
        assert_eq!(rt.lines.len(), 1);
        assert_eq!(rt.lines[0].kind, LineKind::Para);
        assert!(rt.marks.is_empty());
    }

    #[test]
    fn bold_and_italic_marks() {
        let rt = imp("a **b** _c_");
        assert_eq!(rt.text, "a b c");
        // "b" at 2..3 strong, "c" at 4..5 emph
        assert!(rt.marks.contains(&Mark {
            start: 2,
            end: 3,
            kind: MarkKind::Strong
        }));
        assert!(rt.marks.contains(&Mark {
            start: 4,
            end: 5,
            kind: MarkKind::Emph
        }));
    }

    #[test]
    fn underline_from_u_tag() {
        let rt = imp("x <u>y</u> z");
        assert_eq!(rt.text, "x y z");
        assert!(rt
            .marks
            .iter()
            .any(|m| m.kind == MarkKind::Underline && m.start == 2 && m.end == 3));
    }

    /// A `</u>` closes the innermost open `<u>` wherever it sits among the
    /// marks, one carrying an attribute included, so it never closes a `**`
    /// and an underline crosses one freely.
    #[test]
    fn a_u_pair_crosses_other_marks() {
        let cases: &[(&str, &[Mark], &[(&str, usize)])] = &[
            ("<u>a **b</u> c**", &[Mark::new(0, 3, MarkKind::Underline), Mark::new(2, 5, MarkKind::Strong)], &[]),
            ("**a <u>b** c</u>", &[Mark::new(0, 3, MarkKind::Strong), Mark::new(2, 5, MarkKind::Underline)], &[]),
            ("**bold <u class=\"x\">a</u> b**", &[Mark::new(0, 8, MarkKind::Strong)], &[("u", 1)]),
            ("<u>a <u class=\"x\">b</u> c</u>", &[Mark::new(0, 5, MarkKind::Underline)], &[("u", 1)]),
            ("<u>a <u>b</u> c</u>", &[Mark::new(0, 5, MarkKind::Underline)], &[]),
            ("**a </u> b**", &[Mark::new(0, 4, MarkKind::Strong)], &[]),
        ];
        for (md, marks, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.marks, *marks, "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }

    /// A `<u>` still open where its paragraph, heading or item text ends drops
    /// there, reported, and underlines nothing; however many are open, the
    /// import does not fail.
    #[test]
    fn an_unclosed_u_drops_where_its_run_ends() {
        let imported = imp_fixed("<u>a\n\nb **c**");
        assert_eq!(imported.content.text, "a\nb c");
        assert_eq!(imported.content.marks, [Mark::new(4, 5, MarkKind::Strong)]);
        assert_eq!(dropped(&imported), [("u", 1)]);

        let imported = imp_fixed(&format!("{}x</u>", "<u>".repeat(MAX_NESTING_DEPTH + 1)));
        assert_eq!(imported.content.marks, [Mark::new(0, 1, MarkKind::Underline)]);
        assert_eq!(dropped(&imported), [("u", MAX_NESTING_DEPTH)]);

        let imported = imp_fixed("| <u>a **b</u> c** | <u>d |\n|---|---|\n| x | y |");
        let cells = crate::serial::table_cells(&imported.content.islands[0].props);
        assert_eq!(
            cells[0].1,
            [Mark::new(0, 3, MarkKind::Underline), Mark::new(2, 5, MarkKind::Strong)]
        );
        assert!(cells[1].1.is_empty());
        assert_eq!(dropped(&imported), [("u", 1)]);
    }

    #[test]
    fn other_html_stripped() {
        let rt = imp("a <span>b</span> c");
        assert_eq!(rt.text, "a b c");
    }

    /// `***a**` is a literal `*` followed by strong `a` (CommonMark's rule of
    /// three), and every shape here keeps its stars: a fixer re-segmenting the
    /// run would delete one.
    #[test]
    fn odd_asterisk_runs_keep_their_literal_star() {
        for (src, text) in [
            ("***a**", "*a"),
            ("***aa**", "*aa"),
            ("****a**", "**a"),
            ("a***a**", "a*a"),
        ] {
            assert_eq!(imp(src).text, text, "literal star dropped from {src:?}");
        }
        let rt = imp("***bold italic***");
        assert_eq!(rt.text, "bold italic");
        assert!(rt.marks.iter().any(|m| m.kind == MarkKind::Strong));
        assert!(rt.marks.iter().any(|m| m.kind == MarkKind::Emph));
    }

    /// `<ul>` is not `<u>`, so it is stripped like any other HTML: no
    /// underline, no strong.
    #[test]
    fn ul_lookalike_is_not_underline() {
        let rt = imp("x <ul>y</ul> z");
        assert_eq!(rt.text, "x y z");
        assert!(rt
            .marks
            .iter()
            .all(|m| m.kind != MarkKind::Underline && m.kind != MarkKind::Strong));
    }

    #[test]
    fn two_paragraphs_two_lines() {
        let rt = imp("one\n\ntwo");
        assert_eq!(rt.text, "one\ntwo");
        assert_eq!(rt.lines.len(), 2);
        assert!(rt.lines.iter().all(|l| l.kind == LineKind::Para));
    }

    #[test]
    fn heading_line_kind() {
        let rt = imp("## Title");
        assert_eq!(rt.text, "Title");
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 2 });
    }

    #[test]
    fn inline_code_mark() {
        let rt = imp("run `cargo test` now");
        assert_eq!(rt.text, "run cargo test now");
        assert!(rt
            .marks
            .iter()
            .any(|m| m.kind == MarkKind::Code && m.start == 4 && m.end == 14));
    }

    #[test]
    fn code_block_lines() {
        let rt = imp("```rust\nfn a() {}\nfn b() {}\n```");
        assert_eq!(rt.text, "fn a() {}\nfn b() {}");
        assert_eq!(rt.lines.len(), 2);
        assert!(rt.lines.iter().all(|l| l.kind
            == LineKind::Code {
                lang: Some("rust".into())
            }));
    }

    #[test]
    fn code_block_drops_a_stray_slot_and_breaks_on_a_bare_cr() {
        let rt = imp("```\na\u{FFFC}b\nc\rd\n```");
        assert_eq!(rt.text, "ab\nc\nd");
        assert_eq!(rt.lines.len(), 3);
        assert!(rt
            .lines
            .iter()
            .all(|l| l.kind == LineKind::Code { lang: None }));
    }

    /// The prose sibling of the code-block case above, on the other door.
    /// Dropping the slot moves what abuts a delimiter, so the emphasis the
    /// *second* pass reads is not the emphasis the first one wrote: `*a\u{FFFC}*`
    /// is a literal pair around the slot (`*` before U+FFFC is not left-flanking),
    /// and once the slot is gone `*a*` is emphasis. The drop is the contract —
    /// `ISLAND_SLOT` with no backing island is an invariant violation, so it
    /// cannot be admitted — and one markdown pass is where it must happen.
    #[test]
    fn prose_drops_a_stray_slot_before_flanking_is_read() {
        let rt = imp("x\u{FFFC}y");
        assert_eq!(rt.text, "xy");
        assert!(rt.islands.is_empty());

        // The flanking move itself: literal in, emphasized out, settled after.
        let once = imp("*a\u{FFFC}*");
        assert_eq!(once.text, "a");
        assert_eq!(once.marks, vec![Mark::new(0, 1, MarkKind::Emph)]);
        let twice = imp(&crate::export::to_markdown(&once));
        assert_eq!(twice, once);
    }

    #[test]
    fn bullet_list_containers() {
        let rt = imp("- a\n- b");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 0,
                checked: None,
                instance: 0,
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 1,
                checked: None,
                instance: 0,
            }]
        );
    }

    #[test]
    fn ordered_list_custom_start() {
        let rt = imp("3. a\n4. b");
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 0,
                checked: None,
                instance: 0,
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
                checked: None,
                instance: 0,
            }]
        );
    }

    #[test]
    fn multi_paragraph_list_item_shares_container() {
        let rt = imp("- first\n\n  second");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.lines[0].containers, rt.lines[1].containers);
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 0,
                checked: None,
                instance: 0,
            }]
        );
    }

    #[test]
    fn blockquote_container() {
        let rt = imp("> quoted");
        assert_eq!(rt.text, "quoted");
        assert_eq!(rt.lines[0].containers, vec![Container::Quote { instance: 0 }]);
    }

    #[test]
    fn thematic_break_is_rule_line() {
        for src in ["---", "***", "___"] {
            let md = format!("one\n\n{src}\n\ntwo");
            let rt = imp(&md);
            assert_eq!(rt.lines.len(), 3, "source: {src}");
            assert_eq!(rt.lines[0].kind, LineKind::Para);
            assert_eq!(rt.lines[1].kind, LineKind::Rule, "source: {src}");
            assert_eq!(rt.lines[2].kind, LineKind::Para);
            // The rule line carries no text of its own.
            assert_eq!(rt.text, "one\n\ntwo");
        }
    }

    #[test]
    fn table_is_block_island() {
        let rt = imp("| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(rt.text, "\u{FFFC}");
        // The block is the slot's markup; the line holding it is prose.
        assert_eq!(rt.lines[0].kind, LineKind::Para);
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, IslandType::Table);
    }

    /// A cell reuses the prose mark machinery through a second `Tag::Strong`
    /// site, so the two must agree on `<u>` vs `**`.
    #[test]
    fn underline_from_u_tag_in_table_cell() {
        let rt = imp("| h |\n|---|\n| <u>a</u> **b** |");
        let cells = crate::serial::table_cells(&rt.islands[0].props);
        let (text, marks) = cells.iter().find(|(t, _)| t == "a b").expect("cell");
        assert_eq!(text, "a b");
        let kinds: Vec<&MarkKind> = marks.iter().map(|m| &m.kind).collect();
        assert_eq!(kinds, [&MarkKind::Underline, &MarkKind::Strong]);
    }

    #[test]
    fn island_ids_are_deterministic_and_positional() {
        let md = "![a](x)\n\n| h |\n|---|\n| c |";
        let a = imp(md);
        let b = imp(md);
        assert_eq!(a.to_canonical_json(), b.to_canonical_json());
        let ids: Vec<&str> = a.islands.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["isl-0", "isl-1"]);
    }

    #[test]
    fn image_is_inline_island() {
        let rt = imp("see ![a cat](cat.png) here");
        assert_eq!(rt.text, "see \u{FFFC} here");
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, IslandType::Image);
        assert_eq!(rt.islands[0].props["url"], "cat.png");
        assert_eq!(rt.islands[0].props["alt"], "a cat");
    }

    /// A task marker is its item's own: a list mixes tasks and plain items,
    /// `[X]` reads as done, and every line of an item carries its marker.
    #[test]
    fn a_task_marker_is_its_items_checked() {
        let checked = |rt: &Normalized| -> Vec<Vec<Option<bool>>> {
            rt.lines
                .iter()
                .map(|l| {
                    l.containers
                        .iter()
                        .map(|c| match c {
                            Container::ListItem { checked, .. } => *checked,
                            _ => panic!("a list item: {c:?}"),
                        })
                        .collect()
                })
                .collect()
        };
        let rt = imp_fixed("- [ ] a\n- [X] b\n- c\n\n  more\n  - [x] d").content;
        assert_eq!(rt.text, "a\nb\nc\nmore\nd");
        assert_eq!(
            checked(&rt),
            [
                vec![Some(false)],
                vec![Some(true)],
                vec![None],
                vec![None],
                vec![None, Some(true)],
            ]
        );
        assert!(rt.lines.iter().all(|l| l.containers[0].instance() == 0), "one list");

        let rt = imp_fixed("- [x]\n  # h\n\n  p").content;
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 1 });
        assert_eq!(checked(&rt), [vec![Some(true)], vec![Some(true)]]);
    }

    /// A footnote definition drops whole, wherever it sits and whatever it
    /// holds, and a reference to one is its text; with no definition the
    /// reference was text already.
    #[test]
    fn a_footnote_definition_drops_and_its_reference_is_text() {
        let cases: &[(&str, &str, &[(&str, usize)])] = &[
            ("a[^1]\n\n[^1]: **x**\n\n    - y", "a[^1]", &[("footnote", 1)]),
            ("[^n]: x\n\nb[^N]", "b[^N]", &[("footnote", 1)]),
            ("- a\n\n  [^1]: <u>x</u>\n\n  b", "a\nb", &[("footnote", 1)]),
            ("| h[^1] |\n| --- |\n| c |\n\n[^1]: x\n\n[^2]: y", "\u{FFFC}", &[("footnote", 2)]),
            ("a[^9]", "a[^9]", &[]),
        ];
        for (md, text, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
        let table = imp_fixed("| h[^1] |\n| --- |\n| c |\n\n[^1]: x").content;
        assert_eq!(table.islands[0].props["header"][0]["text"], "h[^1]");
    }

    #[test]
    fn empty_list_item_keeps_its_line() {
        let rt = imp("- a\n-\n- b");
        assert_eq!(rt.lines.len(), 3, "empty middle item preserved");
    }

    /// A paragraph whose inline content is entirely stripped HTML leaves no
    /// line, markdown having no syntax to write one back; an empty heading or
    /// container keeps the line `# `, `- ` and `>` can write. Either way the
    /// import is a fixed point.
    #[test]
    fn empty_paragraph_drops_while_empty_heading_and_container_keep_their_line() {
        let cases: &[(&str, usize)] = &[
            ("a\n\n<span></span>\n\nb", 2),
            ("a\n\n<span></span>", 1),
            ("- a\n\n  <span></span>", 1),
            ("> a\n>\n> <span></span>", 1),
            ("# <span></span>", 1),
            ("- <span></span>", 1),
            ("> <span></span>", 1),
        ];
        for (md, lines) in cases {
            let rt = imp(md);
            assert_eq!(rt.lines.len(), *lines, "{md:?} -> {:?}", rt.lines);
            let rt2 = from_markdown(&crate::export::to_markdown(&rt)).unwrap().content;
            assert_eq!(rt2, rt, "{md:?} is not a fixed point");
        }
        assert_eq!(imp("a\n\n<span></span>\n\nb").text, "a\nb");
    }

    #[test]
    fn empty_blockquote_keeps_its_line() {
        let rt = imp("> ");
        assert_eq!(rt.lines.len(), 1);
        assert_eq!(rt.lines[0].containers, vec![Container::Quote { instance: 0 }]);
    }

    #[test]
    fn empty_input_one_empty_line() {
        let rt = imp("");
        assert_eq!(rt.text, "");
        assert_eq!(rt.lines.len(), 1);
    }

    #[test]
    fn mark_does_not_swallow_leading_newline() {
        let rt = imp("a\n\n**b**");
        assert_eq!(rt.text, "a\nb");
        let m = &rt.marks[0];
        assert_eq!((m.start, m.end), (2, 3));
        assert_eq!(rt.text.chars().nth(m.start), Some('b'));
    }

    #[test]
    fn import_and_editor_content_same_canonical_bytes() {
        // Equal content → equal bytes, whatever the producer.
        let imported = imp("a\n\n**b**");
        let editor = Content {
            text: "a\nb".into(),
            lines: vec![
                Line {
                    kind: LineKind::Para,
                    containers: vec![],
                    continues: false,
                },
                Line {
                    kind: LineKind::Para,
                    containers: vec![],
                    continues: false,
                },
            ],
            marks: vec![Mark {
                start: 2,
                end: 3,
                kind: MarkKind::Strong,
            }],
            islands: vec![],
        };
        assert_eq!(
            imported.to_canonical_json(),
            editor.into_normalized().to_canonical_json()
        );
    }

    #[test]
    fn hard_break_is_a_continuation_line() {
        let rt = imp("line one\\\nline two");
        assert_eq!(rt.text, "line one\nline two");
        assert_eq!(rt.lines.len(), 2);
        assert!(!rt.lines[0].continues);
        assert!(rt.lines[1].continues, "hard break -> continuation line");
    }

    #[test]
    fn inline_br_is_a_hard_break_where_text_precedes_it() {
        let rt = imp("line1<br>line2");
        assert_eq!(rt.text, "line1\nline2");
        assert!(rt.lines[1].continues);

        assert_eq!(imp("# a<br>b").text, "a b");

        let rt = imp("a\n\n<br>b");
        assert_eq!(rt.text, "a\nb");
        assert!(!rt.lines[1].continues, "two paragraphs stay two");

        let rt = imp("# <br>a");
        assert_eq!(rt.text, "a");
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 1 });

        let rt = imp("**<br>a**");
        assert_eq!(rt.text, "a");
        assert_eq!(rt.marks, vec![Mark::new(0, 1, MarkKind::Strong)]);
    }

    #[test]
    fn atx_heading_cannot_carry_a_hard_break() {
        // A setext heading can, and reaches the heading→space arm.
        let rt = imp("## a  \nb");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 2 });
        assert_eq!(rt.lines[1].kind, LineKind::Para);
        assert!(!rt.lines[1].continues, "separate block, not a continuation");
    }

    /// A tight item's text arrives with no `Paragraph` start, so text after a
    /// block nested in the item opens a line of its own rather than joining
    /// that block's last line.
    #[test]
    fn text_after_a_block_nested_in_a_tight_item_opens_its_own_line() {
        for (md, text) in [
            ("- item\n  ```\n  code\n  ```\n  after", "item\ncode\nafter"),
            ("- item\n  > # head\n  after", "item\nhead\nafter"),
            ("- item\n  <!-- c -->\n  after", "item\nafter"),
            ("- item\n  ***\n  after", "item\n\nafter"),
        ] {
            let rt = imp(md);
            assert_eq!(rt.text, text, "{md:?}");
            let last = rt.lines.last().unwrap();
            assert_eq!((&last.kind, last.containers.len()), (&LineKind::Para, 1), "{md:?}");
            assert_eq!(imp(&crate::export::to_markdown(&rt)), rt, "{md:?}");
        }
    }

    #[test]
    fn astral_positions_are_usv() {
        let rt = imp("a😀**b**");
        // 'a'(0) '😀'(1) 'b'(2): strong over "b" is 2..3 in USV.
        assert_eq!(rt.text, "a😀b");
        assert!(rt
            .marks
            .iter()
            .any(|m| m.start == 2 && m.end == 3 && m.kind == MarkKind::Strong));
    }

    /// `from_markdown` and the content fixed point over its result.
    fn imp_fixed(md: &str) -> Imported {
        let imported = from_markdown(md).unwrap();
        assert_eq!(imported.content.validate(), Ok(()), "invariants for {md:?}");
        let back = from_markdown(&crate::export::to_markdown(&imported.content)).unwrap();
        assert_eq!(back.content, imported.content, "{md:?} is not a fixed point");
        imported
    }

    /// Each warning's construct as `parse::dropped_construct` names it, leaked
    /// so a case compares it against a literal.
    fn dropped(imported: &Imported) -> Vec<(&'static str, usize)> {
        imported
            .warnings
            .iter()
            .map(|ImportWarning { construct, count }| {
                (&*construct.to_string().leak(), *count)
            })
            .collect()
    }

    fn table_rows(rt: &Normalized) -> Vec<Vec<String>> {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        assert_eq!(island.island_type, IslandType::Table);
        island.props["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|cell| crate::serial::parse_cell(cell).0)
                    .collect()
            })
            .collect()
    }

    /// An HTML block drops whole, as CommonMark reads it: a tag line other
    /// than a line of carrier tags alone, tight against markdown, takes the
    /// markdown with it, and blank lines set the markdown apart.
    #[test]
    fn a_tag_line_tight_against_markdown_drops_its_block() {
        for (md, tag) in [
            ("<div align=\"center\">\n| a | b |\n|---|---|\n| 1 | 2 |\n</div>", "div"),
            ("<x-keep>\n| a | b |\n|---|---|\n| 1 | 2 |\n</x-keep>", "x-keep"),
            ("<center>\n**Signed**\nJ. Doe\n</center>", "center"),
        ] {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, "", "{md:?}");
            assert_eq!(dropped(&imported), [(tag, 1)], "{md:?}");
        }

        let beside = imp_fixed("<span>\n<qm-keep>\nA\n\n</qm-keep>");
        assert_eq!(beside.content.text, "");
        assert_eq!(dropped(&beside), [("span", 1), ("qm-keep", 1)]);

        let imported = imp_fixed("<div align=\"center\">\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n</div>");
        assert_eq!(imported.content.text, "\u{FFFC}");
        assert_eq!(table_rows(&imported.content), [["1", "2"]]);
        assert_eq!(dropped(&imported), [("div", 1)]);
    }

    /// A line holding only carrier tags reads as tag lines wherever it
    /// stands, in the containers CommonMark puts it in, lazy continuation
    /// included.
    #[test]
    fn a_line_of_carrier_tags_reads_as_tag_lines_wherever_it_stands() {
        let cases: &[(&str, &str, &[&[&str]])] = &[
            ("<qm-keep>\n| a | b |\n|---|---|\n| 1 | 2 |\n</qm-keep>", "\u{FFFC}", &[&["element"]]),
            ("<qm-keep>\n**Signed**\nJ. Doe\n</qm-keep>", "Signed J. Doe", &[&["element"]]),
            ("<qm-keep>\n\nA\n</qm-keep>\nB", "A\nB", &[&["element"], &[]]),
            ("A\n<qm-keep>\nB\n</qm-keep>", "A\nB", &[&[], &["element"]]),
            ("A\n    <qm-keep>\nB\n</qm-keep>", "A\nB", &[&[], &["element"]]),
            ("A\\\n</qm-keep>\nB", "A\nB", &[&[], &[]]),
            ("- a\n  <qm-keep>\n  b\n  </qm-keep>", "a\nb", &[&["list_item"], &["list_item", "element"]]),
            ("> A\n<qm-keep>\n> B\n> </qm-keep>", "A\nB", &[&["quote"], &["quote", "element"]]),
            ("> A\n> <qm-keep>\n> B\n> </qm-keep>", "A\nB", &[&["quote"], &["quote", "element"]]),
            ("<qm-sig></qm-sig>", "", &[&["element"]]),
            ("<qm-sig name=\"a\"></qm-sig>\n<qm-sig name=\"b\"></qm-sig>", "\n", &[&["element"], &["element"]]),
            ("- [ ] <qm-sig></qm-sig>\n- [x] b", "\nb", &[&["list_item", "element"], &["list_item"]]),
            ("<qm-keep><qm-table>\n\n| a |\n|---|\n| 1 |\n\n</qm-table></qm-keep>", "\u{FFFC}", &[&["element"]]),
            ("| a |\n|---|\n<qm-sig></qm-sig>\nB", "\u{FFFC}\n\nB", &[&[], &["element"], &[]]),
        ];
        for (md, text, tags) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(container_tags(&imported.content), *tags, "{md:?}");
            assert!(imported.warnings.is_empty(), "{md:?}");
        }

        let void = &imp_fixed("<qm-sig>\n</qm-sig>").content;
        assert_eq!(crate::export::to_markdown(void), "<qm-sig></qm-sig>");

        let split = imp_fixed("1. a\n<qm-keep>\n2. b\n</qm-keep>");
        assert_eq!(container_tags(&split.content), [["list_item"], ["list_item"]]);
        assert_eq!(dropped(&split), [("qm-keep", 1)]);
    }

    /// The text under a tag line in a paragraph reads as the paragraph read
    /// it, a line opening a block in a fresh paragraph included, and exports
    /// to markdown that imports the same.
    #[test]
    fn the_text_under_a_tag_line_keeps_its_paragraph_reading() {
        let cases: &[(&str, &str, &[&[&str]])] = &[
            ("A\n<qm-keep>\n[x]: /url\nB\n</qm-keep>", "A\n[x]: /url B", &[&[], &["element"]]),
            ("A\n<qm-sig></qm-sig>\n    code", "A\n\ncode", &[&[], &["element"], &[]]),
            ("A\n<qm-sig></qm-sig>\n2. b", "A\n\n2. b", &[&[], &["element"], &[]]),
            ("A\n<qm-sig></qm-sig>\n<span>\nB", "A\n\nB", &[&[], &["element"], &[]]),
            ("A\n<qm-sig></qm-sig>\n<qm-x>\nB\n</qm-x>", "A\n\nB", &[&[], &["element"], &["element"]]),
            ("> <qm-sig></qm-sig>\nB", "\nB", &[&["quote", "element"], &["quote"]]),
            ("- a\n  <qm-sig></qm-sig>\nb", "a\n\nb", &[&["list_item"], &["list_item", "element"], &["list_item"]]),
            ("> A\n> <qm-keep>\nB\n> </qm-keep>", "A\nB", &[&["quote"], &["quote", "element"]]),
        ];
        for (md, text, tags) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(container_tags(&imported.content), *tags, "{md:?}");
            let again = imp_fixed(&crate::export::to_markdown(&imported.content));
            assert_eq!(again.content, imported.content, "{md:?}");
        }
    }

    /// A line of tags that are not all carrier ones, or one inside a mark,
    /// stays inline, its carrier tags dropping as any inline tag does.
    #[test]
    fn a_line_holding_other_tags_stays_inline() {
        let cases: &[(&str, &[(&str, usize)])] = &[
            ("A\n<span><qm-keep>\nB\n\n</qm-keep>", &[("span", 1), ("qm-keep", 1)]),
            ("*a\n<qm-keep>\nb*\n\n</qm-keep>", &[("qm-keep", 1)]),
            ("<u>a\n<qm-sig></qm-sig>\nb</u>", &[("qm-sig", 1)]),
            ("A\n<qm-sig/>\nB", &[("qm-sig", 1)]),
            ("A\n<qm-anchor ref=\"x\"></qm-anchor>\nB", &[]),
        ];
        for (md, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.lines.len(), 1, "{md:?}");
            assert!(imported.content.lines[0].containers.is_empty(), "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }

    fn layout(rt: &Normalized) -> serde_json::Value {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        let keys = ["widths", "align", "headless"];
        let props = island.props.as_object().unwrap();
        let kept = props.iter().filter(|(k, _)| keys.contains(&k.as_str()));
        serde_json::Value::Object(kept.map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    #[test]
    fn a_table_wrapper_folds_its_attributes_into_the_table_it_holds() {
        let table = "| a | b | c |\n|---|---|---|\n| 1 | 2 | 3 |";
        let canonical = "<qm-table align=\"center\" widths=\"2 6 auto\">\n\n\
                         | a | b | c |\n| --- | --- | --- |\n| 1 | 2 | 3 |\n\n</qm-table>";
        let expected = serde_json::json!({"align": "center", "widths": [2, 6, null]});
        let cases = [
            (format!("<qm-table widths=\"2 6\" align=center>\n\n{table}\n\n</qm-table>"), 0),
            (format!("<Qm-Table widths=\" 2  6 auto \" align=\"center\">\n\n{table}\n\n</Qm-Table>\n\nafter"), 0),
            (format!("- item\n- <qm-table widths=\"2 6\" align=\"center\">\n\n  {}\n\n  </qm-table>", table.replace('\n', "\n  ")), 1),
            (format!("- item\n\n<qm-table widths=\"2 6\" align=\"center\">\n\n{table}\n\n</qm-table>"), 0),
            (format!("> <qm-table widths=\"2 6 auto auto\" align=\"center\">\n>\n> {}\n>\n> </qm-table>", table.replace('\n', "\n> ")), 1),
            (format!("<qm-table widths=\"2 6\" align=\"center\">\n\n<qm-keep>\n\n{table}\n\n</qm-keep>\n\n</qm-table>"), 1),
        ];
        for (md, containers) in &cases {
            let imported = imp_fixed(md);
            assert_eq!(dropped(&imported), [], "{md:?}");
            let rt = &imported.content;
            let props = &rt.islands.iter().find(|i| i.island_type == IslandType::Table).unwrap().props;
            for key in ["widths", "align"] {
                assert_eq!(props[key], expected[key], "{key} in {md:?}");
            }
            let (_, line) = rt.text.split('\n').zip(&rt.lines).find(|(t, _)| t.contains(ISLAND_SLOT)).unwrap();
            assert_eq!(line.containers.len(), *containers, "{md:?}");
        }
        assert_eq!(crate::export::to_markdown(&imp_fixed(&cases[0].0).content), canonical);
        assert_eq!(
            crate::export::to_markdown(&imp_fixed(&cases[4].0).content),
            canonical.split('\n').map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n")
        );

        let headless = imp_fixed(&format!("<qm-table headless>\n\n{table}\n\n</qm-table>"));
        assert_eq!(layout(&headless.content), serde_json::json!({"headless": true}));
        let spelled = crate::export::to_markdown(&headless.content);
        assert!(spelled.starts_with("<qm-table headless=\"\">\n\n| a | b | c |"), "{spelled:?}");
        assert_eq!(imp_fixed(&spelled).content, headless.content);

        let defaults = imp_fixed(&format!("<qm-table widths=\"auto auto\">\n\n{table}\n\n</qm-table>"));
        assert_eq!(dropped(&defaults), []);
        assert_eq!(layout(&defaults.content), serde_json::json!({}));
        assert_eq!(crate::export::to_markdown(&defaults.content), from_markdown(table).map(|i| crate::export::to_markdown(&i.content)).unwrap());
    }

    #[test]
    fn a_table_wrapper_holding_anything_but_one_table_drops_whole() {
        let t = "| a |\n|---|\n| 1 |";
        for md in [
            "<qm-table align=\"center\">\n\npara\n\n</qm-table>".to_string(),
            format!("<qm-table align=\"center\">\n\n{t}\n\n{t}\n\n</qm-table>"),
            format!("<qm-table align=\"center\">\n\n{t}\n\npara\n\n</qm-table>"),
            format!("<qm-table align=\"center\">\n\n- {}\n\n</qm-table>", t.replace('\n', "\n  ")),
            format!("<qm-table align=\"center\">\n\n{t}"),
            "<qm-table align=\"center\">\n\n</qm-table>".to_string(),
            format!("<qm-table align=\"center\"/>\n\n{t}"),
        ] {
            let imported = imp_fixed(&md);
            assert_eq!(dropped(&imported), [("qm-table", 1)], "{md:?}");
            assert!(imported.content.islands.iter().all(|i| i.props.get("align").is_none()), "{md:?}");
        }

        let nested = imp_fixed(&format!("<qm-table align=\"left\">\n\n<qm-table align=\"right\">\n\n{t}\n\n</qm-table>\n\n</qm-table>"));
        assert_eq!(dropped(&nested), [("qm-table", 1)]);
        assert_eq!(layout(&nested.content), serde_json::json!({"align": "right"}));

        for md in [
            format!("- <qm-table align=\"center\">\n\n{t}\n\n</qm-table>"),
            format!("> <qm-table align=\"center\">\n\n{t}\n\n</qm-table>"),
            format!("<qm-table align=\"center\">\n\n<qm-keep>\n\n</qm-table>\n\n{t}\n\n</qm-keep>"),
        ] {
            let imported = imp_fixed(&md);
            assert_eq!(dropped(&imported), [("qm-table", 1)], "{md:?}");
            assert_eq!(layout(&imported.content), serde_json::json!({}), "{md:?}");
        }
    }

    #[test]
    fn a_table_wrapper_drops_each_attribute_it_cannot_read() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |";
        let cases: &[(&str, &[(&str, usize)], serde_json::Value)] = &[
            ("foo=\"1\" align=\"left\"", &[("qm-table[foo]", 1)], serde_json::json!({"align": "left"})),
            ("style=\"x\" onclick=\"y\" breakable=\"false\"", &[("qm-table[breakable]", 1), ("qm-table[onclick]", 1), ("qm-table[style]", 1)], serde_json::json!({})),
            ("widths=\"a b\" align=\"middle\" breakable=\"no\"", &[("qm-table[align]", 1), ("qm-table[breakable]", 1), ("qm-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"0 1\"", &[("qm-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"+1 2\"", &[("qm-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"1 null\"", &[("qm-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"1 *\"", &[("qm-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"1.5 2\"", &[("qm-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"9007199254740992 1\"", &[("qm-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"9007199254740991 1\"", &[], serde_json::json!({"widths": [9007199254740991u64, 1]})),
            ("breakable", &[("qm-table[breakable]", 1)], serde_json::json!({})),
            ("align=\"left\" align=\"right\"", &[("qm-table[align]", 1)], serde_json::json!({"align": "left"})),
            ("headless", &[], serde_json::json!({"headless": true})),
            ("headless=\"\" align=\"right\"", &[], serde_json::json!({"align": "right", "headless": true})),
            ("headless=\"true\"", &[("qm-table[headless]", 1)], serde_json::json!({})),
            ("headless=\"false\"", &[("qm-table[headless]", 1)], serde_json::json!({})),
        ];
        for (attrs, warned, kept) in cases {
            let md = format!("<qm-table {attrs}>\n\n{t}\n\n</qm-table>");
            let imported = imp_fixed(&md);
            let mut got = dropped(&imported);
            got.sort();
            assert_eq!(got, *warned, "{md:?}");
            assert_eq!(layout(&imported.content), *kept, "{md:?}");
        }
    }

    /// Each table cell's `align` and `valign`, header then body.
    fn cell_alignments(rt: &Normalized) -> Vec<serde_json::Value> {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        crate::serial::table_cell_values(&island.props)
            .map(|cell| {
                let keys = cell.as_object().unwrap().iter().filter(|(k, _)| ["align", "valign"].contains(&k.as_str()));
                serde_json::Value::Object(keys.map(|(k, v)| (k.clone(), v.clone())).collect())
            })
            .collect()
    }

    /// What a pair wraps imports as the cell would without it, and an
    /// alignment equal to its column's stays.
    #[test]
    fn a_cell_pair_around_a_whole_cell_folds_its_alignment() {
        use serde_json::json;
        let cases: &[(&str, &str, serde_json::Value)] = &[
            ("<qm-cell align=\"right\">1</qm-cell>", "1", json!({"align": "right"})),
            ("<qm-cell valign=middle>1</qm-cell>", "1", json!({"valign": "middle"})),
            (
                "<QM-CELL VALIGN='bottom' align=\"center\">**1** <u>x</u><br>y</QM-CELL>",
                "**1** <u>x</u><br>y",
                json!({"align": "center", "valign": "bottom"}),
            ),
            ("<qm-cell align=\"left\"></qm-cell>", "", json!({"align": "left"})),
            ("<qm-cell valign=\"top\">`a\\|b` [c](u)</qm-cell>", "`a\\|b` [c](u)", json!({"valign": "top"})),
        ];
        let row = |cell: &str| format!("| <qm-cell align=\"center\">h</qm-cell> | b |\n|---|:-:|\n| {cell} | 2 |");
        for (cell, inner, keys) in cases {
            let md = row(cell);
            let imported = imp_fixed(&md);
            assert_eq!(dropped(&imported), [], "{md:?}");
            let want = [json!({"align": "center"}), json!({}), keys.clone(), json!({})];
            assert_eq!(cell_alignments(&imported.content), want, "{md:?}");
            let bare = from_markdown(&row(inner)).unwrap().content;
            let cells = |rt: &Normalized| crate::serial::table_cells(&rt.islands[0].props);
            assert_eq!(cells(&imported.content), cells(&bare), "{md:?}");
        }

        for (cell, text) in [
            ("<qm-cell valign=\"top\"> \ta  </qm-cell>", "a"),
            ("<qm-cell valign=\"top\">  </qm-cell>", ""),
            ("<qm-cell valign=\"top\">&#32;a&#9;</qm-cell>", " a\t"),
            ("<qm-cell valign=\"top\"> &#32; </qm-cell>", " "),
            ("<qm-cell valign=\"top\"><u></u> a</qm-cell>", " a"),
        ] {
            let edges = imp_fixed(&format!("| h |\n|---|\n| {cell} |"));
            assert_eq!(table_rows(&edges.content), [[text]], "{cell:?}");
        }

        let rt = imp_fixed("| h |\n|:-:|\n| <qm-cell valign=\"bottom\" align=\"center\">**a**</qm-cell> |").content;
        assert_eq!(
            crate::export::to_markdown(&rt),
            "| h |\n| :---: |\n| <qm-cell align=\"center\" valign=\"bottom\">**a**</qm-cell> |"
        );
    }

    /// A pair folds only around its cell's whole content; any other `qm-cell`
    /// tag drops, each open tag reported, and the cell keeps what it wraps.
    #[test]
    fn a_cell_pair_that_does_not_wrap_its_whole_cell_drops() {
        let r = "<qm-cell align=\"right\">";
        let l = "<qm-cell align=\"left\">";
        let cases = [
            (format!("{r}a</qm-cell> b"), "a b", 1),
            (format!("a {r}b</qm-cell>"), "a b", 1),
            (format!("{r}a</qm-cell>**b**"), "ab", 1),
            (format!("{r}a</qm-cell>{l}b</qm-cell>"), "ab", 2),
            (format!("{r}{l}a</qm-cell></qm-cell>"), "a", 2),
            (format!("{r}a"), "a", 1),
            (format!("{r}a</qm-cell></qm-cell>"), "a", 1),
            ("<qm-cell align=\"right\"/>a".to_string(), "a", 1),
            (format!("**{r}a</qm-cell>**"), "a", 1),
            (format!("{r}**a</qm-cell>**"), "a", 1),
            (format!("![{r}a</qm-cell>](u)"), "a", 1),
            ("</qm-cell>a".to_string(), "a", 0),
        ];
        for (cell, text, count) in &cases {
            let md = format!("| h |\n| --- |\n| {cell} |");
            let imported = imp_fixed(&md);
            let want: &[(&str, usize)] = if *count == 0 { &[] } else { &[("qm-cell", *count)] };
            assert_eq!(dropped(&imported), want, "{md:?}");
            assert_eq!(table_rows(&imported.content), [[*text]], "{md:?}");
            assert_eq!(cell_alignments(&imported.content), vec![serde_json::json!({}); 2], "{md:?}");
        }

        let prose = imp_fixed(&format!("a {r}b</qm-cell> c"));
        assert_eq!(dropped(&prose), [("qm-cell", 1)]);
        assert_eq!(prose.content.text, "a b c");

        let block = imp_fixed("<qm-cell align=\"right\">\n\npara\n\n</qm-cell>");
        assert_eq!(dropped(&block), []);
        assert!(matches!(
            &block.content.lines[0].containers[..],
            [Container::Element { name, .. }] if name == "cell"
        ));
    }

    #[test]
    fn a_cell_pair_drops_each_attribute_it_cannot_read() {
        use serde_json::json;
        let cases: &[(&str, &[(&str, usize)], serde_json::Value)] = &[
            ("align=\"middle\" valign=\"middle\"", &[("qm-cell[align]", 1)], json!({"valign": "middle"})),
            ("valign=\"horizon\" align=\"left\"", &[("qm-cell[valign]", 1)], json!({"align": "left"})),
            ("foo=\"1\" style=\"x\" align=\"center\"", &[("qm-cell[foo]", 1), ("qm-cell[style]", 1)], json!({"align": "center"})),
            ("align=\"left\" align=\"right\"", &[("qm-cell[align]", 1)], json!({"align": "left"})),
            ("align=\"Right\" valign", &[("qm-cell[align]", 1), ("qm-cell[valign]", 1)], json!({})),
        ];
        for (attrs, warned, kept) in cases {
            let md = format!("| h |\n|---|\n| <qm-cell {attrs}>1</qm-cell> |");
            let imported = imp_fixed(&md);
            let mut got = dropped(&imported);
            got.sort();
            assert_eq!(got, *warned, "{md:?}");
            assert_eq!(cell_alignments(&imported.content), [json!({}), kept.clone()], "{md:?}");
            assert_eq!(table_rows(&imported.content), [["1"]], "{md:?}");
        }
    }

    /// A type-7 tag cannot interrupt a pipe table, so the parser reads
    /// `</qm-table>` after the rows as one more row; the repair ends the
    /// table there instead. A type-6 tag (`</div>`) interrupts it on its own,
    /// and its block drops with the text it holds.
    #[test]
    fn a_row_of_tags_ends_its_table() {
        for close in ["</qm-table>", "</qm-keep></qm-table>"] {
            let rt = imp_fixed(&format!("| a | b |\n|---|---|\n| 1 | 2 |\n{close}\nnext")).content;
            assert_eq!(table_rows(&rt), [["1", "2"]], "{close}");
            assert_eq!(rt.text, "\u{FFFC}\nnext", "{close}");
        }
        let imported = imp_fixed("| a | b |\n|---|---|\n| 1 | 2 |\n</div>\nnext");
        assert_eq!(table_rows(&imported.content), [["1", "2"]]);
        assert_eq!(imported.content.text, "\u{FFFC}");
        assert_eq!(dropped(&imported), [("div", 1)]);
        let rt = imp_fixed("> | a |\n> |---|\n> | 1 |\n> </qm-table>\n> next").content;
        assert_eq!(table_rows(&rt), [["1"]]);
        assert!(rt.lines.iter().all(|l| l.containers.len() == 1), "{:?}", rt.lines);
    }

    /// Text after a comment's `-->` on its last line moves to a line of its
    /// own, and ends there rather than joining the line after; one opening
    /// another HTML block opens it there. Mid-line, a comment is inline HTML
    /// and splits nothing.
    #[test]
    fn text_after_a_comment_imports() {
        let cases: &[(&str, &str)] = &[
            ("<!-- comment -->Same line text", "Same line text"),
            ("<!--\nmultiline\ncomment\n-->Trailing text", "Trailing text"),
            ("<!-- first -->Text\n\n<!-- second -->More text", "Text\nMore text"),
            ("<!--- comment --->Trailing text", "Trailing text"),
            ("<!-- <!-- -->Trailing", "Trailing"),
            ("   <!-- c -->Text", "Text"),
            ("    <!-- c -->Text", "<!-- c -->Text"),
            ("-->some text", "-->some text"),
            ("Some text before <!-- comment -->and after", "Some text before and after"),
            ("<!-- a -->Title\n===", "Title\n==="),
            ("<!-- a --><div>\nnext", "next"),
            ("<!-- a --><!-- b\nmore\n-->", "more -->"),
        ];
        for (md, text) in cases {
            assert_eq!(imp_fixed(md).content.text, *text, "{md:?}");
        }
        let rt = imp_fixed("> <!-- a -->text\noutside").content;
        assert_eq!(rt.text, "text\noutside");
        assert_eq!(rt.lines[0].containers.len(), 1);
        assert!(rt.lines[1].containers.is_empty());
    }

    /// A tag alone on its line opens an HTML block, whatever its name: the
    /// inline allowlist does not reach it.
    #[test]
    fn the_allowlist_is_inline_only() {
        let imported = imp_fixed("<u>\ntext\n</u>");
        assert_eq!(imported.content.text, "");
        assert_eq!(dropped(&imported), [("u", 1)]);

        let imported = imp_fixed("para\n\n<br>\n\nnext");
        assert_eq!(imported.content.text, "para\nnext");
        assert_eq!(dropped(&imported), [("br", 1)]);
    }

    /// One count per open or self-closing tag, by lowercase name, in order of
    /// first occurrence: closing tags, comments, a `<pre>` block's content, the
    /// inline allowlist, `qm-anchor` and a block element count nothing. A
    /// tag of the allowlist inside an HTML block drops with it and counts. An
    /// element inside a line, or one that drops unclosed, counts under
    /// `qm-<name>`, and a `qm-*` name outside the grammar under its full
    /// name. A block holding no other opening tag counts its first where it
    /// drops text with it.
    #[test]
    fn dropped_tags_count_once_per_opening() {
        let md = "<div>\n<span>a</span> <SPAN>b</SPAN><br> <u>c</u> <img src=x/>\n</div>\n\n\
                  <!-- <em>not markup</em> -->\n\n<pre><b>x</b></pre>\n\n\
                  x <span>y</span> <u>z</u><br>w\n\n\
                  <qm-anchor id=\"x\">t</qm-anchor> <qm-keep>k</qm-keep>\n\
                  <QM-ANCHOR ref=\"y\"></QM-ANCHOR>\n<Qm-Keep>\n\n\
                  | <span>cell</span> |\n|---|\n| <hr/> <qm-a--b>z</qm-a--b> |\n\n\
                  </Center>\ndropped";
        let imported = imp_fixed(md);
        assert_eq!(
            dropped(&imported),
            [
                ("div", 1),
                ("span", 4),
                ("br", 1),
                ("u", 1),
                ("img", 1),
                ("pre", 1),
                ("qm-keep", 2),
                ("hr", 1),
                ("qm-a--b", 1),
                ("center", 1)
            ]
        );
        assert!(imp_fixed("plain **text**, `<code>`, \\<escaped>").warnings.is_empty());
        assert!(imp_fixed("</div>\n\npara").warnings.is_empty());
    }

    /// A block that drops markdown reports under its first tag where no
    /// opening tag in it counts: a stray close tag, `qm-anchor`, and a close
    /// whose element then drops unclosed or closes later, reported once.
    #[test]
    fn a_block_dropping_markdown_reports_under_its_first_tag() {
        let cases = [
            ("a\n\n</qm-keep>\n</span>\ntext", ("qm-keep", 1)),
            ("a\n\n</qm-table>\n</span>\ntext", ("qm-table", 1)),
            ("a\n\n</qm-anchor>\ntext", ("qm-anchor", 1)),
            ("<qm-anchor ref=\"x\">\ntext", ("qm-anchor", 1)),
            ("<qm-keep>\n\nx\n\n</qm-keep>\n</span>\ntext", ("qm-keep", 1)),
            ("<qm-keep>\n\nx\n\n</qm-keep>\n</span>\ntext\n\n</qm-keep>", ("qm-keep", 1)),
        ];
        for (md, report) in cases {
            let imported = imp_fixed(md);
            assert_eq!(dropped(&imported), [report], "{md:?}");
            assert!(!imported.content.text.contains("text"), "{md:?}");
        }
    }

    /// A `<script>`, `<style>` or `<textarea>` block drops through the line
    /// holding its close tag, blank lines and markdown inside included, and
    /// reports its tag once.
    #[test]
    fn a_raw_text_block_drops_through_its_close_and_reports_its_tag() {
        for (md, tag) in [
            ("before\n\n<script>\nlet a = 1;\n\nlet b = 2;\n</script>\n\nafter", "script"),
            ("before\n\n<style>p { color: red }</style>\n\nafter", "style"),
            ("before\n\n<textarea>\n**not**\n\n<qm-keep>\n</textarea> tail\n\nafter", "textarea"),
        ] {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, "before\nafter", "{md:?}");
            assert!(imported.content.marks.is_empty(), "{md:?}");
            assert_eq!(dropped(&imported), [(tag, 1)], "{md:?}");
        }
    }

    /// A block of tag lines alone passes its carrier tags on and reports the
    /// rest; a close tag sharing its line with another is inline under
    /// CommonMark, so the element it would close drops.
    #[test]
    fn a_carrier_tag_among_raw_tag_lines_wraps_as_its_lines_read() {
        let imported = imp_fixed("<div>\n<qm-keep>\n\npara\n\n</qm-keep>\n</div>\n\nafter");
        assert_eq!(imported.content.text, "para\nafter");
        assert_eq!(container_tags(&imported.content), [&["element"][..], &[]]);
        assert_eq!(dropped(&imported), [("div", 1)]);

        let imported = imp_fixed("<div><qm-keep>\n\npara\n\n</qm-keep></div>\n\nafter");
        assert_eq!(imported.content.text, "para\nafter");
        assert!(imported.content.lines.iter().all(|l| l.containers.is_empty()));
        assert_eq!(dropped(&imported), [("div", 1), ("qm-keep", 1)]);
    }

    /// CRLF line endings import as LF ones do, through every block the
    /// carrier, a table cell's break and a fence hold.
    #[test]
    fn crlf_imports_as_lf() {
        let lf = "<qm-keep note=\"a\">\n\n| A | B |\n|---|---|\n| <u>x</u> | y<br>z |\n\n</qm-keep>\n\n\
                  - item\n  more\n\n> quote\n\n```\ncode\n```\n\n<span>x</span>\n";
        let crlf = lf.replace('\n', "\r\n");
        let (lf, crlf) = (imp_fixed(lf), imp_fixed(&crlf));
        assert_eq!(crlf.content, lf.content);
        assert_eq!(crlf.warnings, lf.warnings);
        assert_eq!(dropped(&lf), [("span", 1)]);
    }

    fn container_tags(rt: &Normalized) -> Vec<Vec<&'static str>> {
        rt.lines.iter().map(|l| l.containers.iter().map(Container::tag).collect()).collect()
    }

    /// An element tag alone on its line wraps the blocks up to its close tag,
    /// inside the containers around it, and stays where its own indentation
    /// puts it: after a list item it does not continue.
    #[test]
    fn an_element_tag_line_wraps_the_blocks_it_holds() {
        let cases: &[(&str, &str, &[&[&str]])] = &[
            ("<qm-keep>\n\npara\n\n</qm-keep>", "para", &[&["element"]]),
            ("<qm-keep>\n\n- a\n- b\n\n</qm-keep>", "a\nb", &[&["element", "list_item"], &["element", "list_item"]]),
            ("- <qm-keep>\n\n  a\n\n  </qm-keep>\n- b", "a\nb", &[&["list_item", "element"], &["list_item"]]),
            ("> <qm-keep>\n>\n> q\n>\n> </qm-keep>", "q", &[&["quote", "element"]]),
            ("<qm-keep>\n</qm-keep>", "", &[&["element"]]),
            ("- a\n\n<qm-keep>\n\n- b\n\n</qm-keep>", "a\nb", &[&["list_item"], &["element", "list_item"]]),
        ];
        for (md, text, tags) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(container_tags(&imported.content), *tags, "{md:?}");
            assert!(imported.warnings.is_empty(), "{md:?}");
        }

        let rt = imp_fixed("<qm-keep name=\"x\">\n\na\n\n</qm-keep>\n<qm-keep name=\"x\">\n\nb\n\n</qm-keep>").content;
        let keep = |instance| Container::Element {
            name: "keep".into(),
            attrs: [("name".to_string(), "x".to_string())].into(),
            instance,
        };
        assert_eq!(rt.lines[0].containers, [keep(0)]);
        assert_eq!(rt.lines[1].containers, [keep(1)]);
    }

    /// An inline element pair drops its tags and reports `qm-<name>` once
    /// per open tag, in prose, a heading and a table cell, what it holds
    /// importing as written.
    #[test]
    fn an_inline_element_pair_drops() {
        for (md, text) in [
            ("a <qm-hl tone=\"warm\">b</qm-hl> c", "a b c"),
            ("<qm-hl>a **b</qm-hl> c**", "a b c"),
            ("# a <qm-hl>b</qm-hl>", "a b"),
        ] {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, text, "{md:?}");
            assert!(imported.content.marks.iter().all(|m| m.kind == MarkKind::Strong), "{md:?}");
            assert_eq!(dropped(&imported), [("qm-hl", 1)], "{md:?}");
        }

        let imported = imp_fixed("| <qm-hl>x</qm-hl> y |\n|---|");
        let (text, marks) = crate::serial::parse_cell(&imported.content.islands[0].props["header"][0]);
        assert_eq!(text, "x y");
        assert!(marks.is_empty());
        assert_eq!(dropped(&imported), [("qm-hl", 1)]);
    }

    /// An element the carrier cannot read drops its tags and reports
    /// `qm-<name>`, what it wraps importing: one left open, a self-closing one,
    /// which HTML reads as an open tag, and one inside a line. An attribute
    /// outside the grammar drops alone. A close tag with nothing to close
    /// drops silently.
    #[test]
    fn an_element_drops_where_it_does_not_close() {
        let cases: &[(&str, &str, &[(&str, usize)])] = &[
            ("<qm-keep>\n\na", "a", &[("qm-keep", 1)]),
            ("<qm-keep/>\n\na", "a", &[("qm-keep", 1)]),
            ("- <qm-keep>\n\n  a\n- b\n\n</qm-keep>", "a\nb", &[("qm-keep", 1)]),
            ("> <qm-keep>\n>\n> a\n\n</qm-keep>", "a", &[("qm-keep", 1)]),
            ("</qm-keep>\n\na", "a", &[]),
            ("a\n</qm-keep>", "a", &[]),
            ("a <qm-hl>b", "a b", &[("qm-hl", 1)]),
            ("a<qm-hl></qm-hl>b", "ab", &[("qm-hl", 1)]),
        ];
        for (md, text, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert!(imported.content.marks.is_empty(), "{md:?}");
            assert!(
                imported.content.lines.iter().flat_map(|l| &l.containers).all(|c| c.tag() != "element"),
                "{md:?}"
            );
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }

        let imported = imp_fixed("<qm-keep onclick=\"x\" note=\"y\">\n\na\n\n</qm-keep>");
        assert_eq!(
            imported.content.lines[0].containers,
            [Container::Element {
                name: "keep".into(),
                attrs: [("note".to_string(), "y".to_string())].into(),
                instance: 0,
            }]
        );
        assert_eq!(dropped(&imported), [("qm-keep[onclick]", 1)]);

        let imported = imp_fixed(
            "<qm-keep note=\"a\" note=\"b\" class=\"c\">\n\nx\n\n</qm-keep>\n\n\
             <qm-keep CLASS=\"d\">\n\ny\n\n</qm-keep>",
        );
        let attrs: Vec<_> = imported.content.lines.iter().map(|l| &l.containers).collect();
        assert_eq!(
            attrs,
            [
                &vec![Container::Element {
                    name: "keep".into(),
                    attrs: [("note".to_string(), "a".to_string())].into(),
                    instance: 0,
                }],
                &vec![Container::Element { name: "keep".into(), attrs: [].into(), instance: 0 }],
            ]
        );
        assert_eq!(dropped(&imported), [("qm-keep[note]", 1), ("qm-keep[class]", 2)]);
    }
}
