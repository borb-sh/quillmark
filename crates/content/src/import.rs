//! Markdown import (cold): `normalize → pulldown → content`.
//!
//! Input is normalized by `normalize::parse_markdown` (CRLF→LF, bidi
//! controls dropped, line separators spaced, then the parser-guided repair that
//! splits text off a comment's line, ends a table at a row of tags and sets a
//! line of carrier tags in an HTML block apart) so the content invariants hold
//! by construction, then
//! parsed with `pulldown_cmark` (CommonMark + strikethrough + pipe tables +
//! task lists + footnotes + math + definition lists) and
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
//!   `<br>` is a hard break, dropped at its block's start or right after
//!   another break, and a soft break beside a hard break is nothing.
//! - Two adjacent sibling containers keep their boundary, told apart by
//!   `Container::instance`, minted here and canonicalized by `normalize`.
//! - An empty heading, code block or container keeps its line. An empty
//!   paragraph or continuation line drops, markdown having no syntax to write
//!   one back, one a mark or `<u>` opened whose content all dropped included.
//! - Island ids are minted sequentially (`isl-0`, `isl-1`, …), so import is a
//!   deterministic function of its markdown. Export drops the ids and re-import
//!   re-mints the same sequence.
//! - Tables and images are islands, block and inline respectively; a thematic
//!   break is a `Rule` line carrying no text. A table reads from a pipe table
//!   or from an HTML block opening `<table>`, whose subset (markdown-spec
//!   §6.8) resolves into the same grid, each cell's content read as one line
//!   of inline markdown.
//! - A task list item's marker is its [`Container::ListItem::checked`]; `[X]`
//!   reads as `[x]`.
//! - A footnote reference is an inline island holding its note, read off the
//!   definition its label names. A note is flat, as a table cell is: each
//!   block after its first starts a line, and a block's structure drops,
//!   reported, while its text stays. Two references to one definition are
//!   two notes, and where the definition sits, before or after them, is not
//!   kept.
//! - `$…$` and `$$…$$` are an inline `math` island holding the TeX between the
//!   delimiters, `display` for `$$`. Where no island can sit, in a table
//!   cell, a note or an image's alt text, and where the TeX, its edge
//!   whitespace dropped, is not what its delimiters write back, as `$ $`
//!   around a Unicode space is empty, the markup stays as its text, reported.
//! - A definition list is its terms and descriptions: each term a `Term`
//!   line, each description a [`Container::Description`] under the term above
//!   it. Where one list ends and the next begins is not kept, and neither is
//!   whether a description is tight.
//! - Raw HTML produces no content beyond the allowlist, a table and the
//!   carrier. Any other HTML block drops whole, as CommonMark runs it; one of
//!   tag lines alone passes its carrier tags on.
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
use unicase::UniCase;

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
    /// another tag in a block tight against markdown, and a reserved name's
    /// open tag: `qm-<name>`.
    Element(String),
    /// A wrapper attribute outside the grammar or repeating a name already
    /// read: `qm-<name>[<attr>]`.
    ElementAttr { element: String, attr: String },
    /// An HTML table its block ends before its `</table>`, or another
    /// `<table>` interrupts, which closes there: `table`.
    Table,
    /// An HTML table's row too short for its grid, which pads, or too long,
    /// whose cells past the edge drop: `table[row]`.
    TableRow,
    /// Text in an HTML table's block outside every cell: `table[text]`.
    TableText,
    /// An attribute of an HTML table's tag the subset does not read, repeating
    /// one already read or with a value outside its spelling, and a `colspan`
    /// cut short of a slot another span covers: `<tag>[<attr>]`.
    TableAttr { tag: String, attr: String },
    /// A footnote definition no reference reads, one repeating a label
    /// already defined or nested in another, or a reference where no note
    /// can sit (a table cell, an image's alt text, another note): `footnote`.
    Footnote,
    /// The structure of a block inside a footnote, whose text the note keeps:
    /// `footnote[<block>]`, the block `list`, `heading`, `code`, `quote`,
    /// `table`, `rule` or `definition list`.
    NoteBlock(&'static str),
    /// An equation where no island can sit, or whose TeX its delimiters would
    /// not write back, which stays as its markup's text: `math`.
    Math,
}

impl std::fmt::Display for Dropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use carrier::PREFIX;
        match self {
            Dropped::Tag(name) | Dropped::BadName(name) => f.write_str(name),
            Dropped::Element(name) => write!(f, "{PREFIX}{name}"),
            Dropped::ElementAttr { element, attr } => write!(f, "{PREFIX}{element}[{attr}]"),
            Dropped::Footnote => f.write_str("footnote"),
            Dropped::NoteBlock(block) => write!(f, "footnote[{block}]"),
            Dropped::Math => f.write_str("math"),
            Dropped::Table => f.write_str("table"),
            Dropped::TableRow => f.write_str("table[row]"),
            Dropped::TableText => f.write_str("table[text]"),
            Dropped::TableAttr { tag, attr } => write!(f, "{tag}[{attr}]"),
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
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_DEFINITION_LIST);
    options
}

/// Import markdown into a normalized, validated [`Content`], with an
/// [`ImportWarning`] per construct it dropped: each dropped opening tag, a
/// `<pre>`, `<script>`, `<style>` or `<textarea>` block's included, a block
/// holding no other opening tag under its first where it drops markdown with
/// it, each footnote no reference reads and each equation that stays as its
/// markup's text. A comment, the content of a type 1–5 HTML block, and any other
/// closing tag or `qm-anchor` tag drop silently. What a footnote nothing
/// reads holds reports nothing of its own.
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
        for c in admitted(s) {
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

    /// Take back the last char pushed, a mark at its end moving to its start.
    fn unpush(&mut self) {
        self.text.pop();
        self.pos -= 1;
        let pos = self.pos;
        for m in &mut self.marks {
            m.start = m.start.min(pos);
            m.end = m.end.min(pos);
        }
        for (_, start) in &mut self.open {
            *start = (*start).min(pos);
        }
        for (start, _) in self.underlines.iter_mut().flatten() {
            *start = (*start).min(pos);
        }
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

    /// One inline event of a flat run, a note's: text and the marks over it,
    /// a hard break a `\n`.
    fn flat_event(&mut self, event: &Event) {
        match event {
            Event::Text(t) => self.push_text(t),
            Event::Code(t) => self.push_code(t),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.push_raw('\n'),
            Event::Start(tag) => {
                if let Some(kind) = mark_kind(tag) {
                    self.open_mark(kind);
                }
            }
            Event::End(end) if ends_mark(end) => self.close_mark(),
            _ => {}
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
}

/// An element whose open tag line the import has read and whose close it
/// awaits.
struct Opened {
    /// The [wrapper](carrier::wrapper) name its tag carries.
    name: String,
    /// Its container's `instance`.
    instance: u64,
    /// Its refused attributes, reported once it closes: one left unclosed
    /// drops whole and reports only itself.
    refused: Vec<String>,
    /// The open tag's byte offset.
    at: usize,
    /// The containers open around it.
    depth: usize,
    /// Whether a close tag naming it dropped with markdown, which reports it
    /// once whether or not it drops unclosed.
    reported: bool,
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
    /// The text position after the boundary of the line a mark or `<u>` opened
    /// [early](Self::open_early), while nothing has landed in it.
    early: Option<usize>,
    /// The text position after a soft break's space, and after a hard break:
    /// a soft break beside a hard break folds into it.
    soft_at: Option<usize>,
    broke_at: Option<usize>,
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
    /// The footnote definition being read.
    note: Option<NoteAcc>,
    /// Each definition read, by the label pulldown matches references with.
    notes: HashMap<UniCase<String>, Note>,
    /// Each footnote island minted, by index, and the label it reads.
    note_refs: Vec<(usize, UniCase<String>)>,
}

/// A footnote definition whose events the import is reading into a note.
#[derive(Default)]
struct NoteAcc {
    label: String,
    /// The definition's byte offset.
    at: usize,
    inline: Inline,
    /// Whether the note's current line holds anything, and whether a block
    /// opened since, so the next content starts a line.
    filled: bool,
    broken: bool,
    img_depth: usize,
    in_code: bool,
    /// Whether the table row being read has a cell already.
    in_row: bool,
    /// Definitions opened inside this one, which drop.
    nested: usize,
    /// What the note drops, reported once a reference reads it.
    dropped: Drops,
}

impl NoteAcc {
    /// A block opens: its content starts a line where the note holds some.
    fn block(&mut self) {
        self.broken |= self.filled;
        self.filled = false;
    }

    /// Inline content arrives, on the line a block opened if one did.
    fn fill(&mut self) {
        if std::mem::take(&mut self.broken) {
            self.inline.push_raw('\n');
        }
        self.filled = true;
    }
}

/// A note read off its definition.
struct Note {
    props: serde_json::Value,
    at: usize,
    used: bool,
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
    /// The cell currently open, a pipe table's between `Tag::TableCell` start
    /// and end or an HTML table's through its content, building its inline
    /// text + marks with the same [`Inline`] machinery prose uses.
    cell: Option<Inline>,
}

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
        self.open_early();
        self.inline.open_mark(kind);
    }

    /// [`Self::ensure_open`] ahead of any text, for a mark or `<u>` whose start
    /// must follow the line boundary. A line opened here that nothing fills is
    /// an empty paragraph, which [`Self::retract_empty_line`] takes back.
    fn open_early(&mut self) {
        let emitted = self.emitted();
        self.ensure_open(LineKind::Para);
        if self.emitted() > emitted {
            self.early = Some(self.inline.pos);
        }
    }

    /// Take back the paragraph line [`Self::open_early`] opened where nothing
    /// landed in it, everything it would hold having dropped. A heading keeps
    /// its line.
    fn retract_empty_line(&mut self) {
        if self.early.take() != Some(self.inline.pos)
            || !self.cur.as_ref().is_some_and(|l| l.kind == LineKind::Para)
        {
            return;
        }
        self.cur = self.lines.pop();
        if self.cur.is_some() {
            self.inline.unpush();
        }
        self.soft_at = None;
        self.broke_at = None;
    }

    /// Whether nothing precedes the next inline on its line.
    fn line_empty(&self) -> bool {
        matches!(self.pending, Some((_, false)))
            || self.cur.is_none()
            || self.inline.text.is_empty()
            || self.inline.text.ends_with('\n')
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
        let depth = self.containers.len();
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
                    if tag.block && self.note.is_none() {
                        self.retract_empty_line();
                    }
                    self.carrier_tag(tag)?;
                    continue;
                }
                Fixed::Underline(tag) => {
                    self.underline_tag(tag);
                    continue;
                }
                Fixed::Swallowed(name, at) => {
                    self.swallowed(name, at);
                    continue;
                }
                Fixed::Dropped(construct, at) => {
                    match self.note.as_mut() {
                        Some(note) => note.dropped.add(construct, at),
                        None => self.dropped.add(construct, at),
                    }
                    continue;
                }
                Fixed::NoteDef { label, at } => {
                    match self.note.as_mut() {
                        Some(note) => {
                            note.nested += 1;
                            self.dropped.add(Dropped::Footnote, at);
                        }
                        None => self.note = Some(NoteAcc { label, at, ..NoteAcc::default() }),
                    }
                    continue;
                }
                Fixed::NoteRef { label, at } => {
                    self.note_ref(label, at);
                    continue;
                }
                Fixed::Math { tex, display, source, at } => {
                    self.math(&tex, display, source, at);
                    continue;
                }
                Fixed::HtmlTable(table) => {
                    self.html_table(table)?;
                    continue;
                }
            };
            if self.note.is_some() {
                self.note_event(event);
                continue;
            }
            // An inline run ends at the first event outside it, and the
            // underlines it left open with it.
            if self.image_depth == 0 && self.table.is_none() && !is_inline(&event) {
                self.inline.drop_open(&mut self.dropped);
                self.retract_empty_line();
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
                        let alt = alt_text(other).unwrap_or_default();
                        match self.table.as_mut().and_then(|acc| acc.cell.as_mut()) {
                            Some(cell) => cell.push_text(alt),
                            None => self.image_alt.extend(admitted(alt)),
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
                // A soft break with nothing before it on its line, all before it
                // having dropped, is nothing, and so is one beside a hard break.
                Event::SoftBreak if self.line_empty() || self.broke_at == Some(self.inline.pos) => {}
                Event::SoftBreak => {
                    self.push_inline(" ");
                    self.soft_at = Some(self.inline.pos);
                }
                Event::TaskListMarker(done) => {
                    if let Some(Container::ListItem { checked, .. }) = self.containers.last_mut() {
                        *checked = Some(done);
                    }
                }
                Event::HardBreak => {
                    if self.soft_at.take() == Some(self.inline.pos) {
                        self.inline.unpush();
                    }
                    self.broke_at = Some(self.inline.pos);
                    // A break with no text before it on its line is dropped:
                    // pulldown never emits one there, an inline `<br>` can, and
                    // arming a continuation would join the block to the one above.
                    if self.line_empty() {
                        continue;
                    }
                    match self.cur.as_ref().map(|l| &l.kind) {
                        // A heading is one line.
                        Some(LineKind::Heading { .. }) => {
                            self.push_inline(" ");
                            self.broke_at = Some(self.inline.pos);
                        }
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
            Tag::DefinitionList => self.pending = None,
            Tag::DefinitionListTitle => self.pending = Some((LineKind::Term, false)),
            Tag::DefinitionListDefinition => {
                // A tight description's text arrives with no `Paragraph`, as a
                // tight list item's does.
                self.pending = Some((LineKind::Para, false));
                self.container_marks.push(self.emitted());
                let instance = self.mint_instance();
                self.containers.push(Container::Description { instance });
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
                self.image_url = admit_url(&dest_url);
                self.image_alt.clear();
                self.image_depth = 1;
            }
            tag => {
                if let Some(kind) = mark_kind(&tag) {
                    self.open_mark(kind);
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
            TagEnd::BlockQuote(_) | TagEnd::DefinitionListDefinition => {
                self.drop_unclosed_blocks();
                let mark = self.container_marks.pop().unwrap_or(0);
                self.close_container(mark);
                self.rearm_item();
            }
            TagEnd::DefinitionList => self.rearm_item(),
            end if ends_mark(&end) => self.inline.close_mark(),
            // A block that produced no inline content still gets its line.
            TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::DefinitionListTitle => {
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
        let CarrierTag { name, attrs, block, at } = tag;
        if let Some(note) = self.note.as_mut() {
            if attrs.is_some() {
                note.dropped.add(Dropped::Element(name), at);
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
        let instance = self.mint_instance();
        self.container_marks.push(self.emitted());
        let element = carrier::Element::new(name.clone(), attrs.values)
            .expect("a wrapper's name and kept attributes are in the grammar");
        self.containers.push(Container::Element { element, instance });
        self.blocks.push(Opened { name, instance, refused: attrs.refused, at, depth, reported: false });
        self.check_depth()
    }

    /// Close the innermost element open, where `name` names it and every
    /// container opened inside it has closed; any other close tag drops
    /// silently.
    fn close_wrapper(&mut self, name: &str) {
        let innermost = self
            .blocks
            .last()
            .is_some_and(|open| open.name == name && self.containers.len() == open.depth + 1);
        let Some(Opened { name, refused, at, .. }) = self.blocks.pop_if(|_| innermost) else {
            return;
        };
        let mark = self.container_marks.pop().unwrap_or(0);
        self.close_container(mark);
        self.dropped.attrs(&name, refused, at);
    }

    /// Pair one `<u>` tag in the run or table cell it stands in. In an image's
    /// alt text, where nothing is marked, it underlines nothing.
    fn underline_tag(&mut self, tag: UTag) {
        if let Some(note) = self.note.as_mut() {
            if note.img_depth == 0 {
                if matches!(tag, UTag::Open { .. }) {
                    note.fill();
                }
                note.inline.underline(tag);
            }
            return;
        }
        if self.image_depth > 0 {
            return;
        }
        if let Some(acc) = self.table.as_mut() {
            if let Some(cell) = acc.cell.as_mut() {
                cell.underline(tag);
            }
            return;
        }
        if matches!(tag, UTag::Open { .. }) {
            // Resolve an armed line first, as a mark does, so the underline
            // starts after the line boundary.
            self.open_early();
        }
        self.inline.underline(tag);
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
            self.containers.pop();
            self.container_marks.pop();
            self.unclosed.push(open.instance);
            if !open.reported {
                self.dropped.add(Dropped::Element(open.name), open.at);
            }
        }
    }

    /// Report a close tag that dropped with the markdown under it, once for
    /// the innermost wrapper it names: that wrapper stays open past it.
    fn swallowed(&mut self, name: String, at: usize) {
        if let Some(note) = self.note.as_mut() {
            return note.dropped.add(Dropped::Element(name), at);
        }
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
        if matches!(self.containers.last(), Some(Container::ListItem { .. } | Container::Description { .. })) {
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
            Event::Start(Tag::TableCell) => acc.cell = Some(Inline::default()),
            Event::End(TagEnd::TableCell) => {
                if let Some(mut cell) = acc.cell.take() {
                    cell.close_marks();
                    cell.drop_open(&mut self.dropped);
                    acc.cur_row.push(crate::serial::cell_to_value(&cell.text, &cell.marks));
                }
            }
            _ => {
                let Some(cell) = acc.cell.as_mut() else {
                    return;
                };
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

    /// An HTML table: an island on a line of its own, as a pipe table is, each
    /// cell's content read through the inline import into the cell. In a
    /// footnote's note it is the note's text, a line per row.
    fn html_table(&mut self, table: crate::html_table::Table) -> Result<(), ImportError> {
        use crate::html_table::Slot;
        if self.note.is_some() {
            return self.note_table(table);
        }
        self.inline.drop_open(&mut self.dropped);
        self.retract_empty_line();
        self.pending = None;
        self.open_line(LineKind::Para, false);
        self.inline.push_raw(ISLAND_SLOT);
        self.table = Some(TableAcc::default());
        let mut grid = Vec::with_capacity(table.rows.len());
        for row in table.rows {
            let mut cells = Vec::with_capacity(row.len());
            for slot in row {
                cells.push(match slot {
                    Slot::Empty => crate::serial::cell_to_value("", &[]),
                    Slot::Cell(cell) => self.html_cell(cell)?,
                });
            }
            grid.push(serde_json::Value::Array(cells));
        }
        self.table = None;
        let mut props = table.layout;
        let mut rows = grid.into_iter();
        props.insert("aligns".into(), json!(table.aligns));
        props.insert("header".into(), rows.next().unwrap_or_else(|| json!([])));
        props.insert("rows".into(), serde_json::Value::Array(rows.collect()));
        self.mint_island(IslandType::Table, serde_json::Value::Object(props));
        self.rearm_item();
        Ok(())
    }

    fn html_cell(&mut self, cell: crate::html_table::Cell) -> Result<serde_json::Value, ImportError> {
        if let Some(acc) = self.table.as_mut() {
            acc.cell = Some(Inline::default());
        }
        self.inline_source(&cell.content, cell.at)?;
        let mut inline = self.table.as_mut().and_then(|acc| acc.cell.take()).unwrap_or_default();
        inline.close_marks();
        inline.drop_open(&mut self.dropped);
        let mut value = crate::serial::cell_to_value(&inline.text, &inline.marks);
        if let Some(o) = value.as_object_mut() {
            o.extend(cell.keys);
        }
        Ok(value)
    }

    /// An HTML table inside a footnote's note, flattened as a pipe table is:
    /// each row a line of the note, its cells' text apart by a space.
    fn note_table(&mut self, table: crate::html_table::Table) -> Result<(), ImportError> {
        use crate::html_table::Slot;
        if let Some(note) = self.note.as_mut() {
            note.dropped.add(Dropped::NoteBlock("table"), note.at);
        }
        for row in table.rows {
            if let Some(note) = self.note.as_mut() {
                note.block();
                note.in_row = false;
            }
            for slot in row {
                let Slot::Cell(cell) = slot else { continue };
                if let Some(note) = self.note.as_mut()
                    && std::mem::replace(&mut note.in_row, true)
                {
                    note.fill();
                    note.inline.push_text(" ");
                }
                self.inline_source(&cell.content, cell.at)?;
            }
        }
        Ok(())
    }

    /// Read an HTML table cell's content, at source offset `at`, through the
    /// inline import, as one line of markdown
    /// ([`cell_source`](crate::html_table::cell_source)).
    fn inline_source(&mut self, content: &str, at: usize) -> Result<(), ImportError> {
        let src = crate::html_table::cell_source(content);
        let events = crate::html_table::cell_events(&src);
        self.run(MarkdownFixer::new(&src, events.into_iter()).map(|item| item.shifted(at)))
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

    /// One event of the definition being read into its note.
    fn note_event(&mut self, event: Event) {
        let Some(note) = self.note.as_mut() else {
            return;
        };
        if note.img_depth > 0 {
            match &event {
                Event::Start(Tag::Image { .. }) => note.img_depth += 1,
                Event::End(TagEnd::Image) => note.img_depth -= 1,
                other => {
                    if let Some(s) = alt_text(other) {
                        note.fill();
                        note.inline.push_text(s);
                    }
                }
            }
            return;
        }
        let structure = match &event {
            Event::Start(Tag::List(_)) => Some("list"),
            Event::Start(Tag::Heading { .. }) => Some("heading"),
            Event::Start(Tag::CodeBlock(_)) => Some("code"),
            Event::Start(Tag::BlockQuote(_)) => Some("quote"),
            Event::Start(Tag::Table(_)) => Some("table"),
            Event::Start(Tag::DefinitionList) => Some("definition list"),
            Event::Rule => Some("rule"),
            _ => None,
        };
        if let Some(block) = structure {
            note.dropped.add(Dropped::NoteBlock(block), note.at);
        }
        match event {
            Event::End(TagEnd::FootnoteDefinition) if note.nested > 0 => note.nested -= 1,
            Event::End(TagEnd::FootnoteDefinition) => {
                self.close_note();
                self.rearm_item();
            }
            Event::Start(
                Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::Item
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition
                | Tag::TableHead
                | Tag::TableRow,
            ) => {
                note.block();
                note.in_row = false;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                note.block();
                note.in_code = true;
            }
            Event::End(TagEnd::CodeBlock) => note.in_code = false,
            Event::Rule => note.block(),
            Event::Start(Tag::TableCell) => {
                if std::mem::replace(&mut note.in_row, true) {
                    note.fill();
                    note.inline.push_text(" ");
                }
            }
            Event::Text(t) if note.in_code => {
                let t = t.strip_suffix('\n').unwrap_or(&t);
                for (k, line) in t.split('\n').enumerate() {
                    note.fill();
                    if k > 0 {
                        note.inline.push_raw('\n');
                    }
                    note.inline.push_code(line);
                }
            }
            Event::Start(Tag::Image { .. }) => {
                note.fill();
                note.img_depth = 1;
            }
            other => {
                if !matches!(other, Event::End(_)) && is_inline(&other) {
                    note.fill();
                }
                note.inline.flat_event(&other);
            }
        }
    }

    /// Settle the definition being read into its note: the first under its
    /// label is the one references read.
    fn close_note(&mut self) {
        let Some(mut note) = self.note.take() else {
            return;
        };
        note.inline.close_marks();
        note.inline.drop_open(&mut note.dropped);
        let key = UniCase::new(note.label);
        if self.notes.contains_key(&key) {
            return self.dropped.add(Dropped::Footnote, note.at);
        }
        let props = crate::serial::cell_to_value(&note.inline.text, &note.inline.marks);
        self.notes.insert(key, Note { props, at: note.at, used: false, dropped: note.dropped });
    }

    /// A footnote reference: an island in prose, its note filled in once
    /// every definition is read. Anywhere else it drops.
    fn note_ref(&mut self, label: String, at: usize) {
        if let Some(note) = self.note.as_mut() {
            return note.dropped.add(Dropped::Footnote, at);
        }
        if self.table.is_some() || self.image_depth > 0 {
            return self.dropped.add(Dropped::Footnote, at);
        }
        self.ensure_open(LineKind::Para);
        self.inline.push_raw(ISLAND_SLOT);
        self.note_refs.push((self.islands.len(), UniCase::new(label)));
        self.mint_island(IslandType::Footnote, serde_json::Value::Null);
    }

    /// An equation: an island in prose, and its markup's text, `source`,
    /// where no island can sit.
    fn math(&mut self, tex: &str, display: bool, source: &str, at: usize) {
        if let Some(note) = self.note.as_mut() {
            note.fill();
            note.inline.push_text(source);
            return note.dropped.add(Dropped::Math, at);
        }
        let cell = self.table.as_mut().and_then(|acc| acc.cell.as_mut());
        if self.image_depth > 0 || cell.is_some() || !crate::export::tex_is_writable(&crate::serial::settle_tex(tex), display) {
            match cell {
                Some(cell) => cell.push_text(source),
                None if self.image_depth > 0 => self.image_alt.extend(admitted(source)),
                None => {
                    self.ensure_open(LineKind::Para);
                    self.inline.push_text(source);
                }
            }
            return self.dropped.add(Dropped::Math, at);
        }
        self.ensure_open(LineKind::Para);
        self.inline.push_raw(ISLAND_SLOT);
        let props = match display {
            true => json!({ "tex": tex, "display": true }),
            false => json!({ "tex": tex }),
        };
        self.mint_island(IslandType::Math, props);
    }

    /// Put `text` where island `index`'s slot sits, the island gone.
    fn unslot(&mut self, index: usize, text: &str) {
        let Some((at, (byte, _))) = self
            .inline
            .text
            .char_indices()
            .enumerate()
            .filter(|(_, (_, c))| *c == ISLAND_SLOT)
            .nth(index)
        else {
            return;
        };
        self.inline.text.replace_range(byte..byte + ISLAND_SLOT.len_utf8(), text);
        let grown = text.chars().count() - 1;
        for m in &mut self.inline.marks {
            if m.start > at {
                m.start += grown;
            }
            if m.end > at {
                m.end += grown;
            }
        }
        self.islands.remove(index);
    }

    fn finish(mut self) -> (Content, Drops) {
        self.close_note();
        // A reference whose definition dropped, nested in another, reads
        // back as the text it is, as one naming no definition does.
        for (index, label) in std::mem::take(&mut self.note_refs).into_iter().rev() {
            match self.notes.get_mut(&label) {
                Some(note) => {
                    self.islands[index].props = note.props.clone();
                    note.used = true;
                }
                None => self.unslot(index, &format!("[^{}]", label.into_inner())),
            }
        }
        for (n, island) in self.islands.iter_mut().enumerate() {
            island.id = format!("isl-{n}");
        }
        for note in std::mem::take(&mut self.notes).into_values() {
            match note.used {
                true => self.dropped.absorb(note.dropped),
                false => self.dropped.add(Dropped::Footnote, note.at),
            }
        }
        self.inline.drop_open(&mut self.dropped);
        self.retract_empty_line();
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

/// What `event` contributes to an image's alt text.
fn alt_text<'e>(event: &'e Event) -> Option<&'e str> {
    match event {
        Event::Text(t) | Event::Code(t) => Some(t),
        Event::SoftBreak | Event::HardBreak => Some(" "),
        _ => None,
    }
}

fn mark_kind(tag: &Tag) -> Option<MarkKind> {
    Some(match tag {
        Tag::Emphasis => MarkKind::Emph,
        Tag::Strong => MarkKind::Strong,
        Tag::Strikethrough => MarkKind::Strike,
        Tag::Link { dest_url, .. } => MarkKind::Link { url: admit_url(dest_url) },
        _ => return None,
    })
}

/// `s` as inline text admits it: under [`crate::normalize::admit_char`], a
/// stray [`ISLAND_SLOT`] dropped and a stray `\n` spaced.
fn admitted(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().filter_map(|c| match c {
        ISLAND_SLOT => None,
        '\n' => Some(' '),
        other => crate::normalize::admit_char(other),
    })
}

/// A link or image destination under [`crate::normalize::admit_char`], with
/// no line ending, which a destination can hold only as a character
/// reference and a URL parser strips.
fn admit_url(url: &str) -> String {
    url.chars().filter(|&c| c != '\n').filter_map(crate::normalize::admit_char).collect()
}

fn ends_mark(end: &TagEnd) -> bool {
    matches!(end, TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link)
}

/// Whether `event` stands inside an inline run.
fn is_inline(event: &Event) -> bool {
    match event {
        Event::Text(_)
        | Event::Code(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::InlineHtml(_)
        | Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::FootnoteReference(_) => true,
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
    /// A footnote definition's start, at its byte offset.
    NoteDef { label: String, at: usize },
    /// A footnote reference, at its byte offset.
    NoteRef { label: String, at: usize },
    /// An equation: its TeX, whether `$$` delimits it, and its markup as
    /// written, at its byte offset.
    Math { tex: pulldown_cmark::CowStr<'a>, display: bool, source: &'a str, at: usize },
    /// A table an HTML block spells.
    HtmlTable(crate::html_table::Table),
}

impl Fixed<'_> {
    /// This item with each byte offset it carries `by` further on, for an
    /// item read off text that starts there.
    fn shifted(self, by: usize) -> Self {
        match self {
            Fixed::Carrier(tag) => Fixed::Carrier(CarrierTag { at: tag.at + by, ..tag }),
            Fixed::Underline(UTag::Open { at }) => Fixed::Underline(UTag::Open { at: at + by }),
            Fixed::Swallowed(name, at) => Fixed::Swallowed(name, at + by),
            Fixed::Dropped(construct, at) => Fixed::Dropped(construct, at + by),
            Fixed::NoteDef { label, at } => Fixed::NoteDef { label, at: at + by },
            Fixed::NoteRef { label, at } => Fixed::NoteRef { label, at: at + by },
            Fixed::Math { tex, display, source, at } => Fixed::Math { tex, display, source, at: at + by },
            other => other,
        }
    }
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
}

impl CarrierTag {
    fn of(tag: &html::Tag, block: bool, at: usize) -> Option<Self> {
        Some(CarrierTag {
            name: carrier::wrapper(tag.name).filter(|_| !tag.self_closing)?,
            attrs: (!tag.closing).then(|| carrier::decode_attrs(&tag.attrs)),
            block,
            at,
        })
    }
}

/// What a tag named `name` reports where its opening tag drops: a wrapper as
/// its element, a `qm-*` name outside the grammar as its bad name, and any
/// other tag by its name. `qm-anchor`, Quillmark's own read-only spelling of
/// an anchor, is written to be dropped and reports nothing.
pub(crate) fn dropped_tag(name: &str) -> Option<Dropped> {
    let lower = name.to_ascii_lowercase();
    Some(match carrier::element(name) {
        Some(element) if element == carrier::ANCHOR => return None,
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

    /// Every drop `other` counted, at its own offset.
    fn absorb(&mut self, other: Drops) {
        for (construct, count, at) in other.entries {
            for _ in 0..count {
                self.add(construct.clone(), at);
            }
        }
    }

    /// Each attribute of `element` named in `names`, dropped alone.
    fn attrs(&mut self, element: &str, names: Vec<String>, at: usize) {
        for attr in names {
            self.add(Dropped::ElementAttr { element: element.into(), attr }, at);
        }
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

    /// Consume an HTML block through its end. One opening `<table>` hands on
    /// the tables it spells ([`crate::html_table::scan`]). Any other reports
    /// its [markup tags](html::block_tags): a block of tag lines alone passes its
    /// carrier tags on; one that drops text with them drops them too, each
    /// open tag reported, and its first tag where no open tag reports: a
    /// wrapper's close through the builder, which knows whether that wrapper
    /// reports already.
    fn drop_html_block(&mut self, at: usize) {
        let mut text = String::new();
        let mut lines: Vec<(usize, usize)> = Vec::new();
        for (event, range) in self.inner.by_ref() {
            match event {
                Event::Html(h) => {
                    lines.push((text.len(), range.start));
                    text.push_str(&h);
                }
                Event::End(TagEnd::HtmlBlock) => break,
                _ => {}
            }
        }
        if crate::html_table::opens(&text) {
            let source = |off: usize| {
                let k = lines.partition_point(|&(block, _)| block <= off).saturating_sub(1);
                lines.get(k).map_or(at, |&(block, src)| src + (off - block))
            };
            let (tables, drops) = crate::html_table::scan(&text, source);
            self.held.extend(drops.into_iter().map(|(construct, at)| Fixed::Dropped(construct, at)));
            self.held.extend(tables.into_iter().map(Fixed::HtmlTable));
            self.held.push_back(Fixed::Event(Event::End(TagEnd::HtmlBlock)));
            return;
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
                if let Some(carrier) = CarrierTag::of(&tag, false, range.start) {
                    return Some(Fixed::Carrier(carrier));
                }
                self.drop_tag(&tag, range.start);
                return None;
            }
            Event::Start(Tag::FootnoteDefinition(label)) => Fixed::NoteDef {
                label: label.to_string(),
                at: range.start,
            },
            Event::FootnoteReference(label) => Fixed::NoteRef {
                label: label.to_string(),
                at: range.start,
            },
            Event::InlineMath(tex) => Fixed::Math { tex, display: false, source: &self.src[range.clone()], at: range.start },
            Event::DisplayMath(tex) => Fixed::Math { tex, display: true, source: &self.src[range.clone()], at: range.start },
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

    fn keep(attrs: &[(&str, &str)]) -> carrier::Element {
        carrier::Element::new("keep", attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()).unwrap()
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

    /// Each footnote reference is an inline island holding a copy of the note
    /// its label names, however the label is cased and wherever the
    /// definition sits; a note keeps the text of the blocks it holds, a line
    /// each.
    #[test]
    fn a_footnote_reference_is_an_island_holding_its_note() {
        let md = "[^n]: **bold** note\n\nOne[^1] two[^N].\n\n# Head[^1]\n\n\
                  [^1]: first<br>second\n\n    para\n    - item";
        let imported = imp_fixed(md);
        let rt = &imported.content;
        assert_eq!(rt.text, "One\u{FFFC} two\u{FFFC}.\nHead\u{FFFC}");
        let notes: Vec<(String, Vec<Mark>)> = rt
            .islands
            .iter()
            .map(|isl| {
                assert_eq!(isl.island_type, IslandType::Footnote);
                crate::serial::parse_cell(&isl.props)
            })
            .collect();
        let first = ("first\nsecond\npara\nitem".to_string(), vec![]);
        assert_eq!(
            notes,
            [
                first.clone(),
                ("bold note".into(), vec![Mark::new(0, 4, MarkKind::Strong)]),
                first,
            ]
        );
        assert_eq!(dropped(&imported), [("footnote[list]", 1)]);
    }

    /// A definition nothing reads and one repeating a label drop, and so does
    /// a reference where no note can sit; a reference to no definition is
    /// text.
    #[test]
    fn a_footnote_with_no_place_drops() {
        let cases: &[(&str, &str, &[(&str, usize)])] = &[
            ("a[^1]\n\n[^1]: x\n\n[^2]: y", "a\u{FFFC}", &[("footnote", 1)]),
            ("a[^1]\n\n[^1]: x\n\n[^1]: y", "a\u{FFFC}", &[("footnote", 1)]),
            ("| h[^1] |\n| --- |\n| c |\n\n[^1]: x", "\u{FFFC}", &[("footnote", 2)]),
            ("a[^1]\n\n[^1]: x[^2]\n\n[^2]: y", "a\u{FFFC}", &[("footnote", 2)]),
            ("a[^9]", "a[^9]", &[]),
        ];
        for (md, text, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }


    /// `$…$` is an inline equation and `$$…$$` a display one, each an island
    /// holding its TeX; where no island can sit, the source stays as text.
    #[test]
    fn a_reference_to_a_definition_nested_in_another_reads_as_its_text() {
        let imported = imp_fixed("a[^2] b[^1] c\n\n[^1]: x\n\n    > [^2]: y");
        let rt = &imported.content;
        assert_eq!(rt.text, "a[^2] b\u{FFFC} c");
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].id, "isl-0");
        assert_eq!(rt.islands[0].props["text"], "x\ny");
    }

    #[test]
    fn a_definition_list_is_term_lines_and_the_descriptions_under_them() {
        let d = |instance| Container::Description { instance };
        let rt = imp_fixed("T\\\nU\n: a\n\n  b\n: c\n  - x\n\nV\n:").content;
        assert_eq!(rt.text, "T\nU\na\nb\nc\nx\nV\n");
        let shape: Vec<(LineKind, Vec<Container>, bool)> =
            rt.lines.iter().map(|l| (l.kind.clone(), l.containers.clone(), l.continues)).collect();
        let item = Container::ListItem { ordered: false, start: 1, ordinal: 0, checked: None, instance: 0 };
        assert_eq!(
            shape,
            [
                (LineKind::Term, vec![], false),
                (LineKind::Term, vec![], true),
                (LineKind::Para, vec![d(0)], false),
                (LineKind::Para, vec![d(0)], false),
                (LineKind::Para, vec![d(1)], false),
                (LineKind::Para, vec![d(1), item], false),
                (LineKind::Term, vec![], false),
                (LineKind::Para, vec![d(0)], false),
            ]
        );

        let rt = imp_fixed("n[^1]\n\n[^1]: T\n    : d");
        assert_eq!(rt.content.islands[0].props["text"], "T\nd");
        assert_eq!(dropped(&rt), [("footnote[definition list]", 1)]);
    }

    #[test]
    fn math_is_an_island_holding_its_tex() {
        let imported = imp_fixed("Let $x^2$ and $$\\frac{a}{b}$$ cost $5 and $10.");
        let rt = &imported.content;
        assert_eq!(rt.text, "Let \u{FFFC} and \u{FFFC} cost $5 and $10.");
        let props: Vec<&serde_json::Value> = rt.islands.iter().map(|isl| &isl.props).collect();
        assert_eq!(
            props,
            [
                &serde_json::json!({"tex": "x^2"}),
                &serde_json::json!({"tex": "\\frac{a}{b}", "display": true}),
            ]
        );
        assert!(dropped(&imported).is_empty());

        let cases: &[(&str, &str)] = &[
            ("| $a$ |\n| --- |\n| c |", "\u{FFFC}"),
            ("![$a$ b](u)", "\u{FFFC}"),
            ("n[^1]\n\n[^1]: $a$", "n\u{FFFC}"),
        ];
        for (md, text) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(dropped(&imported), [("math", 1)], "{md:?}");
        }
        let rt = imp("| $a$ |\n| --- |\n| c |");
        assert_eq!(rt.islands[0].props["header"][0]["text"], "$a$");
        let rt = imp("![$a$ b](u)");
        assert_eq!(rt.islands[0].props["alt"], "$a$ b");
        let imported = imp_fixed("a $\u{2000}$ b");
        assert_eq!(imported.content.text, "a $\u{2000}$ b", "a blank equation stays as its text");
        assert_eq!(dropped(&imported), [("math", 1)]);
        let rt = imp("n[^1]\n\n[^1]: $a$");
        assert_eq!(rt.islands[0].props["text"], "$a$");
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
    fn a_soft_break_beside_a_hard_break_is_nothing() {
        for md in ["one<br>\ntwo", "one\n<br>two", "one<br>\n<br>two", "one\n\\\ntwo"] {
            let rt = imp(md);
            assert_eq!(rt.text, "one\ntwo", "{md:?}");
            assert!(rt.lines[1].continues, "{md:?}");
        }
        let rt = imp("a\n**<br>b**");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.marks, vec![Mark::new(2, 3, MarkKind::Strong)]);
        assert_eq!(imp("a<br>\nb\n===").text, "a b");
        assert_eq!(imp("a <br>\nb").text, "a \nb", "an authored space stays");
    }

    /// A mark or `<u>` opens its line ahead of its text, so a line all it would
    /// hold dropped from is an empty paragraph or continuation line.
    #[test]
    fn a_line_emptied_by_what_dropped_drops() {
        for (md, text, depths) in [
            ("a\n\n[](u)", "a", vec![0]),
            ("a\n<qm-keep>\n<u>", "a", vec![0]),
            ("a\\\n[](u)", "a", vec![0]),
            ("a<br><u></u>", "a", vec![0]),
            ("[](u)\nb", "b", vec![0]),
            ("- [x] [](u)\n\n  b", "b", vec![1]),
            ("- [](u)\n  - b", "b", vec![2]),
            ("[](u)\n<qm-keep>\n\nb\n\n</qm-keep>", "b", vec![1]),
        ] {
            let rt = imp_fixed(md).content;
            assert_eq!(rt.text, text, "{md:?}");
            assert_eq!(rt.lines.iter().map(|l| l.containers.len()).collect::<Vec<_>>(), depths, "{md:?}");
        }
        let rt = imp_fixed("- [x] [](u)").content;
        assert!(
            matches!(rt.lines[..], [Line { ref containers, .. }] if matches!(containers[..], [Container::ListItem { checked: Some(true), .. }])),
            "an item keeps its line: {:?}",
            rt.lines
        );
        assert_eq!(imp_fixed("# [](u)").content.lines[0].kind, LineKind::Heading { level: 1 });
    }

    #[test]
    fn a_reference_imports_as_text_admits_it_in_a_destination_and_an_alt() {
        let rt = imp_fixed("[x](u&#x202E;&#10;v) ![a&#13;&#10;b](u&#x2028;v) n&#0;\0").content;
        assert_eq!(rt.marks[0].kind, MarkKind::Link { url: "uv".into() });
        assert_eq!(rt.islands[0].props, json!({"alt": "a b", "url": "u v"}));
        assert!(rt.text.ends_with("n\u{FFFD}\u{FFFD}"), "{:?}", rt.text);
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
            ("<qm-keep><qm-box>\n\n| a |\n|---|\n| 1 |\n\n</qm-box></qm-keep>", "\u{FFFC}", &[&["element", "element"]]),
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

    fn grid(rt: &Normalized) -> serde_json::Value {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        assert_eq!(island.island_type, IslandType::Table);
        let mut props = island.props.clone();
        let strip = |row: &mut serde_json::Value| {
            for cell in row.as_array_mut().unwrap() {
                let o = cell.as_object_mut().unwrap();
                if o["marks"].as_array().unwrap().is_empty() {
                    o.remove("marks");
                }
            }
        };
        strip(&mut props["header"]);
        for row in props["rows"].as_array_mut().unwrap() {
            strip(row);
        }
        props
    }

    /// The issue's roster: a title row across the table, a header cell across
    /// two columns and a body cell down two rows, every covered cell an empty
    /// placeholder. It is the export's own spelling.
    #[test]
    fn an_html_table_resolves_its_spans_into_the_grid() {
        use serde_json::json;
        let md = "<table widths=\"1 2 2\" aligns=\"left left right\" align=\"center\">\n<thead>\n\
                  <tr><th colspan=\"3\">FLIGHT X PERSONNEL</th></tr>\n\
                  <tr><th>Date</th><th colspan=\"2\">POC</th></tr>\n</thead>\n<tbody>\n\
                  <tr><td rowspan=\"2\" valign=\"middle\">12 Nov</td><td>SSgt Doe</td><td>555-0101</td></tr>\n\
                  <tr><td>SrA Roe</td><td>555-0102</td></tr>\n</tbody>\n</table>";
        let imported = imp_fixed(md);
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(
            grid(&imported.content),
            json!({
                "aligns": ["left", "left", "right"], "align": "center", "head": 2, "widths": [1, 2, 2],
                "header": [{"text": "FLIGHT X PERSONNEL", "colspan": 3}, {"text": ""}, {"text": ""}],
                "rows": [
                    [{"text": "Date"}, {"text": "POC", "colspan": 2}, {"text": ""}],
                    [{"text": "12 Nov", "rowspan": 2, "valign": "middle"}, {"text": "SSgt Doe"}, {"text": "555-0101"}],
                    [{"text": ""}, {"text": "SrA Roe"}, {"text": "555-0102"}],
                ],
            })
        );
        assert_eq!(crate::export::to_markdown(&imported.content), md);
    }

    /// The subset reads as HTML reads it where a writer leaves things out: an
    /// unquoted value, a row outside any group, an end tag left out, a `tfoot`
    /// after the body, a `th` in a body row, and the `th` rows opening a table
    /// with no `thead`, which head it. Each table here imports as the pipe
    /// table beside it.
    #[test]
    fn an_html_table_reads_as_html_reads_it() {
        let cases = [
            ("<table><tr><th>a<th>b<tr><td>1<td>2</table>", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
            ("<TABLE><THEAD><TR><TH>a</TH><TH>b</TH></TR></THEAD><TR><TD>1</TD><TD>2</TD></TR></TABLE>", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
            ("<table aligns=left,right>\n<tr><th>a</th><th>b</th></tr>\n</table>", "| a | b |\n| --- | --- |"),
            ("<table aligns='Left Right'>\n<thead><tr><th>a</th><th>b</th></tr></thead>\n</table>", "| a | b |\n| :--- | ---: |"),
            ("<table>\n<tr><th>a</th><th>b</th></tr>\n<tfoot><tr><td>f</td><td>g</td></tr></tfoot>\n<tr><td>1</td><td>2</td></tr>\n</table>", "| a | b |\n| --- | --- |\n| 1 | 2 |\n| f | g |"),
            ("<table>\n<thead><tr><td>a</td><td>b</td></tr></thead>\n<tbody><tr><th>1</th><td/>2</tr></tbody>\n</table>", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
            ("<table>\n<tr><th>a</th><th>b</th></tr>\n<!-- </table> -->\n<tr><td>1</td><td>2</td></tr>\n</table>", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
        ];
        for (html, pipes) in cases {
            let imported = imp_fixed(html);
            let warned = dropped(&imported);
            let want = match html.contains("aligns=left,right") {
                true => vec![("table[aligns]", 1)],
                false => vec![],
            };
            assert_eq!(warned, want, "{html:?}");
            assert_eq!(imported.content, imp_fixed(pipes).content, "{html:?}");
        }
        let headless = imp_fixed("<table>\n<tbody>\n<tr><th>a</th></tr>\n</tbody>\n</table>");
        assert_eq!(grid(&headless.content)["head"], 0);
        let reaching = imp_fixed("<table>\n<tr><th rowspan=2>a</th><th>b</th></tr>\n<tr><th>c</th></tr>\n<tr><td>1</td><td>2</td></tr>\n</table>");
        assert_eq!(grid(&reaching.content)["head"], 2);
        let past = imp_fixed("<table>\n<tr><th rowspan=2>a</th><th>b</th></tr>\n<tr><td>1</td></tr>\n</table>");
        assert!(past.warnings.is_empty(), "{:?}", past.warnings);
        assert_eq!(grid(&past.content)["head"], 0);
        assert_eq!(grid(&past.content)["header"][0]["rowspan"], 2);
    }

    /// What the grid cannot hold as written drops alone, reported: an
    /// attribute the subset does not read or a value outside its spelling, a
    /// tag outside the subset and text outside every cell, a row too short or
    /// too long for the grid, which is as wide as its first row, and a
    /// `colspan` cut short where another span covers a slot. A span past the
    /// grid's edge or its row group stops there silently.
    #[test]
    fn what_an_html_table_cannot_hold_drops_alone() {
        use serde_json::json;
        let cases: &[(&str, &[(&str, usize)], serde_json::Value)] = &[
            (
                "<table border=1 widths=\"0 1\"><tr><td class=x colspan=0>a</td><td rowspan=two align=justify valign=baseline>b</td></tr></table>",
                &[("table[border]", 1), ("table[widths]", 1), ("td[class]", 1), ("td[colspan]", 1), ("td[align]", 1), ("td[rowspan]", 1), ("td[valign]", 1)],
                json!({"aligns": ["none", "none"], "head": 0, "header": [{"text": "a"}, {"text": "b"}], "rows": []}),
            ),
            (
                "<table><caption>Roster</caption><colgroup><col></colgroup><tr><td>a</td>stray<td>b</td></tr></table>",
                &[("caption", 1), ("table[text]", 2), ("colgroup", 1), ("col", 1)],
                json!({"aligns": ["none", "none"], "head": 0, "header": [{"text": "a"}, {"text": "b"}], "rows": []}),
            ),
            (
                "<table><tr><td>a</td><td>b</td></tr><tr><td>1</td></tr><tr><td>2</td><td>3</td><td>4</td></tr></table>",
                &[("table[row]", 2)],
                json!({"aligns": ["none", "none"], "head": 0, "header": [{"text": "a"}, {"text": "b"}], "rows": [[{"text": "1"}, {"text": ""}], [{"text": "2"}, {"text": "3"}]]}),
            ),
            (
                "<table><tr><td rowspan=2>a</td><td>b</td><td>c</td></tr><tr><td></td><td>x</td><td>y</td></tr></table>",
                &[("table[row]", 1)],
                json!({"aligns": ["none", "none", "none"], "head": 0, "header": [{"text": "a", "rowspan": 2}, {"text": "b"}, {"text": "c"}], "rows": [[{"text": ""}, {"text": ""}, {"text": "x"}]]}),
            ),
            (
                "<table><tr><td>a</td><td rowspan=2>b</td><td>c</td></tr><tr><td colspan=3>d</td></tr></table>",
                &[("td[colspan]", 1), ("table[row]", 1)],
                json!({"aligns": ["none", "none", "none"], "head": 0, "header": [{"text": "a"}, {"text": "b", "rowspan": 2}, {"text": "c"}], "rows": [[{"text": "d"}, {"text": ""}, {"text": ""}]]}),
            ),
            (
                "<table><thead><tr><th rowspan=5>h</th><th>a</th></tr></thead><tr><td>1</td><td colspan=9>2</td></tr></table>",
                &[],
                json!({"aligns": ["none", "none"], "header": [{"text": "h"}, {"text": "a"}], "rows": [[{"text": "1"}, {"text": "2"}]]}),
            ),
        ];
        for (md, warned, want) in cases {
            let imported = imp_fixed(md);
            let mut got = dropped(&imported);
            got.sort();
            let mut warned = warned.to_vec();
            warned.sort();
            assert_eq!(got, warned, "{md:?}");
            assert_eq!(grid(&imported.content), *want, "{md:?}");
        }
    }

    /// A table is one HTML block: a blank line inside it ends the table there,
    /// in a quote as at the top level, and what follows reads as markdown, an
    /// HTML block of its own that drops. Another `<table>` ends one too, and
    /// one that holds no cell imports nothing.
    #[test]
    fn an_html_table_ends_with_its_block() {
        for md in [
            "<table>\n<tr><td>a</td></tr>\n\n<tr><td>b</td></tr>\n</table>",
            "> <table>\n> <tr><td>a</td></tr>\n>\n> <tr><td>b</td></tr>\n> </table>",
        ] {
            let imported = imp_fixed(md);
            assert_eq!(dropped(&imported), [("table", 1), ("tr", 1), ("td", 1)], "{md:?}");
            assert_eq!(table_rows(&imported.content), Vec::<Vec<String>>::new(), "{md:?}");
            assert_eq!(grid(&imported.content)["header"], serde_json::json!([{"text": "a"}]), "{md:?}");
        }
        let two = imp_fixed("<table><tr><td>a<table><tr><td>b</td></tr></table>\n<table><tr><td>c</td></tr></table>");
        assert_eq!(two.content.islands.len(), 3);
        assert_eq!(two.content.text, "\u{FFFC}\n\u{FFFC}\n\u{FFFC}");
        assert_eq!(dropped(&two), [("table", 1)]);
        let empty = imp_fixed("para\n\n<table>\n<tr></tr>\n</table>");
        assert_eq!((empty.content.text.as_str(), empty.warnings.len()), ("para", 0));
    }

    /// A cell's content is one line of inline markdown, a line break in the
    /// source a space: marks, a code span or an escaped `<` shielding a tag, an
    /// angle-bracketed link destination, `<br>` and `<u>`. An equation keeps
    /// its source, as in a pipe table's cell, and a footnote reference, read
    /// apart from the definitions, is its text.
    #[test]
    fn an_html_cell_holds_inline_markdown() {
        let md = "<table><tr><td>**Total**\n  and *more*</td><td>`</td>` \\<td> [l](<td x>)</td>\
                  <td>a<br><u>b</u> &amp; &#32;</td><td>$x$ y[^1]</td></tr></table>\n\n[^1]: n";
        let imported = imp_fixed(md);
        let props = &imported.content.islands[0].props;
        let cells: Vec<(String, Vec<Mark>)> = crate::serial::table_cells(props);
        assert_eq!(cells[0], ("Total and more".into(), vec![Mark::new(0, 5, MarkKind::Strong), Mark::new(10, 14, MarkKind::Emph)]));
        assert_eq!(
            cells[1],
            ("</td> <td> l".into(), vec![Mark::new(0, 5, MarkKind::Code), Mark::new(11, 12, MarkKind::Link { url: "td x".into() })])
        );
        assert_eq!(cells[2], ("a\nb &  ".into(), vec![Mark::new(2, 3, MarkKind::Underline)]));
        assert_eq!(cells[3].0, "$x$ y[^1]");
        assert_eq!(dropped(&imported), [("math", 1), ("footnote", 1)]);
    }

    /// A note holding an HTML table keeps its cells' text, a line per row, as
    /// it keeps a pipe table's.
    #[test]
    fn an_html_table_in_a_note_is_its_text() {
        let imported = imp_fixed("a[^1]\n\n[^1]: <table><tr><td>x</td><td>**y**</td></tr><tr><td>z</td><td>w</td></tr></table>");
        let note = crate::serial::parse_cell(&imported.content.islands[0].props);
        assert_eq!(note, ("x y\nz w".into(), vec![Mark::new(2, 3, MarkKind::Strong)]));
        assert_eq!(dropped(&imported), [("footnote[table]", 1)]);
    }

    /// `qm-table` is a reserved name and `qm-cell` an element name, so neither
    /// spells a table's layout: each drops, reported, and the table imports
    /// bare.
    #[test]
    fn the_qm_table_and_qm_cell_tags_drop() {
        let imported = imp_fixed(
            "<qm-table widths=\"1 2\">\n\n| a | b |\n|---|---|\n| <qm-cell align=\"right\">1</qm-cell> | 2 |\n\n</qm-table>",
        );
        assert_eq!(dropped(&imported), [("qm-table", 1), ("qm-cell", 1)]);
        assert_eq!(imported.content, imp_fixed("| a | b |\n|---|---|\n| 1 | 2 |").content);
    }

    /// A type-7 tag cannot interrupt a pipe table, so the parser reads
    /// `</qm-keep>` after the rows as one more row; the repair ends the
    /// table there instead. A type-6 tag (`</div>`) interrupts it on its own,
    /// and its block drops with the text it holds.
    #[test]
    fn a_row_of_tags_ends_its_table() {
        for close in ["</qm-keep>", "</qm-sig></qm-keep>"] {
            let rt = imp_fixed(&format!("| a | b |\n|---|---|\n| 1 | 2 |\n{close}\nnext")).content;
            assert_eq!(table_rows(&rt), [["1", "2"]], "{close}");
            assert_eq!(rt.text, "\u{FFFC}\nnext", "{close}");
        }
        let imported = imp_fixed("| a | b |\n|---|---|\n| 1 | 2 |\n</div>\nnext");
        assert_eq!(table_rows(&imported.content), [["1", "2"]]);
        assert_eq!(imported.content.text, "\u{FFFC}");
        assert_eq!(dropped(&imported), [("div", 1)]);
        let rt = imp_fixed("> | a |\n> |---|\n> | 1 |\n> </qm-keep>\n> next").content;
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
        let keep = |instance| Container::Element { element: keep(&[("name", "x")]), instance };
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
            [Container::Element { element: keep(&[("note", "y")]), instance: 0 }]
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
                &vec![Container::Element { element: keep(&[("note", "a")]), instance: 0 }],
                &vec![Container::Element { element: keep(&[]), instance: 0 }],
            ]
        );
        assert_eq!(dropped(&imported), [("qm-keep[note]", 1), ("qm-keep[class]", 2)]);
    }
}
