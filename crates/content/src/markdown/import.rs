//! Markdown import (cold): `normalize → pulldown → content`.
//!
//! Input is normalized by `normalize::parse_markdown` (CRLF→LF, bidi
//! controls dropped, line separators spaced, then the parser-guided repair that
//! splits text off a comment's line, ends a table at a row of tags and sets a
//! line of carrier tags in an HTML block apart) so the content invariants hold
//! by construction, then
//! parsed with `pulldown_cmark` (CommonMark + strikethrough + pipe tables +
//! task lists + footnotes + math) and
//! walked into a [`Content`]. This is the one place the `<u>`/`<br>` allowlist
//! runs, and the one place a dropped raw tag is counted into the
//! [`ImportWarning`]s an import returns beside its content.
//!
//! ## Canonicalizations
//!
//! - A line of carrier tags in a paragraph or a list item's text is a tag
//!   line: the break above it ends the text, and the text under it opens a
//!   paragraph.
//! - A newline inside a paragraph is a line break, a `continues` line distinct
//!   from a paragraph boundary, and CommonMark's two hard break spellings and
//!   an inline `<br>` are the same break. Inside a heading a break is a space,
//!   a heading being one line; a setext heading spans two source lines and can
//!   carry one. Inside a table cell a `<br>` is a `\n` in the cell's text, and
//!   in a cell or a footnote's note a newline is a space. A break at its
//!   block's start or right after another break is nothing.
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
//! - A task list item's marker is its [`Level::ListItem::checked`]; `[X]`
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
//! - A definition list is not read: `Term` and `: description` lines are
//!   one paragraph, as CommonMark reads them.
//! - Raw HTML produces no content beyond the allowlist, a table and the
//!   carrier. Any other HTML block drops whole, as CommonMark runs it; one of
//!   tag lines alone passes its carrier tags on.
//! - A `qm-*` tag outside `qm-span`, `qm-anchor` and `qm-island` wraps
//!   nothing: an open tag drops, reported under its name, a close tag drops
//!   silently, and the blocks between them import as written.
//! - An annotated read's `qm-anchor` tags and `qm-island` tokens are read by a
//!   revise ([`from_markdown_against`]) and dropped by a cold import, a token
//!   reported.

use crate::MAX_NESTING_DEPTH;
use crate::carry::Pin;
use crate::ids::prefix;
use crate::markdown::carrier;
use crate::markdown::html;
use crate::markdown::normalize::parse_markdown;
use crate::model::{
    Anchor, Block, BlockKind, Codec, Container, ContainerKind, Content, HARD_BREAK, ISLAND_SLOT,
    Island, IslandType, Mark, MarkKind, NOTE_SLOT, Scalar,
};
use crate::table::Table;
use pulldown_cmark::{Event, Options, Tag, TagEnd};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use unicase::UniCase;

/// Why an import refused its markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Container nesting exceeded [`MAX_NESTING_DEPTH`].
    NestingTooDeep { depth: usize, max: usize },
    /// The markdown is longer than
    /// [`MAX_MARKDOWN_SIZE`](crate::MAX_MARKDOWN_SIZE) bytes.
    TooLarge { size: usize, max: usize },
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NestingTooDeep { depth, max } => {
                write!(f, "nesting too deep: {depth} (max {max})")
            }
            ImportError::TooLarge { size, max } => {
                write!(f, "markdown of {size} bytes is past the limit of {max}")
            }
        }
    }
}
impl std::error::Error for ImportError {}

/// `count` instances of `construct`, something the markdown spelled that the
/// content has no place for, dropped. One per construct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    pub construct: DroppedConstruct,
    pub count: usize,
}

/// A construct an import drops. Its [`Display`](std::fmt::Display) is the
/// name `parse::dropped_construct` reports it under, given with each variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DroppedConstruct {
    /// A raw tag outside the carrier, or a `qm-anchor` reported with the
    /// markdown its HTML block drops: its lowercase name (`span`, `u`).
    Tag(String),
    /// A tag named with the carrier's prefix and no element name after it: its
    /// lowercase name (`qm-a--b`).
    BadName(String),
    /// The open tag of a `qm-*` name the carrier does not read, a `qm-span`
    /// tag that drops, and a `qm-island` token outside a revise or where no
    /// island sits: `qm-<name>`.
    Element(String),
    /// A `qm-span` attribute it does not read, and the `ref` of a `qm-anchor`
    /// tag or `qm-island` token naming nothing a revise can keep:
    /// `qm-<element>[<attr>]`.
    ElementAttr { element: String, attr: String },
    /// An HTML table its block ends before its `</table>`, or another
    /// `<table>` interrupts, which closes there: `table`.
    Table,
    /// An HTML table's row too short for its grid, which pads: `table:row`.
    TableRow,
    /// Text in an HTML table's block outside every cell: `table:text`.
    TableText,
    /// A table's rows past the [`MAX_TABLE_CELLS`](crate::MAX_TABLE_CELLS)
    /// slots it may hold or the slots the document's tables share, or the
    /// whole of one whose first row alone is wider than the slots left:
    /// `table:size`.
    TableSize,
    /// An attribute of an HTML table's tag the subset does not read, repeating
    /// one already read or with a value outside its spelling: `<tag>[<attr>]`.
    TableAttr { tag: String, attr: String },
    /// A footnote definition no reference reads, one repeating a label
    /// already defined or nested in another, or a reference where no note
    /// can sit (a table cell, an image's alt text, another note): `footnote`.
    Footnote,
    /// A reference past the [`MAX_NOTE_COPIES`](crate::MAX_NOTE_COPIES)
    /// characters a document's notes may copy, which stays as its text:
    /// `footnote:size`.
    FootnoteSize,
    /// The structure of a block inside a footnote, whose text the note keeps:
    /// `footnote[<block>]`, the block `list`, `heading`, `code`, `quote`,
    /// `table` or `rule`.
    NoteBlock(&'static str),
    /// An equation where no island can sit, or whose TeX its delimiters would
    /// not write back, which stays as its markup's text: `math`.
    Math,
    /// A bidi formatting control, which the content does not admit: `bidi`.
    Bidi,
}

impl DroppedConstruct {
    /// Whether this reports a `qm-anchor` tag or a `qm-island` token, which
    /// [`crate::markdown::from_markdown_unresolved`] drops silently.
    pub fn is_held_tag(&self) -> bool {
        match self {
            DroppedConstruct::Element(element) => element == carrier::ISLAND,
            DroppedConstruct::ElementAttr { element, .. } => {
                carrier::RESERVED.contains(&element.as_str())
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for DroppedConstruct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use carrier::PREFIX;
        match self {
            DroppedConstruct::Tag(name) | DroppedConstruct::BadName(name) => f.write_str(name),
            DroppedConstruct::Element(name) => write!(f, "{PREFIX}{name}"),
            DroppedConstruct::ElementAttr { element, attr } => {
                write!(f, "{PREFIX}{element}[{attr}]")
            }
            DroppedConstruct::Footnote => f.write_str("footnote"),
            DroppedConstruct::FootnoteSize => f.write_str("footnote:size"),
            DroppedConstruct::NoteBlock(block) => write!(f, "footnote[{block}]"),
            DroppedConstruct::Math => f.write_str("math"),
            DroppedConstruct::Bidi => f.write_str("bidi"),
            DroppedConstruct::Table => f.write_str("table"),
            DroppedConstruct::TableRow => f.write_str("table:row"),
            DroppedConstruct::TableText => f.write_str("table:text"),
            DroppedConstruct::TableSize => f.write_str("table:size"),
            DroppedConstruct::TableAttr { tag, attr } => write!(f, "{tag}[{attr}]"),
        }
    }
}

/// The CommonMark extensions every markdown parse in this crate runs with.
pub(crate) fn options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_MATH);
    options
}

/// A cold import of `markdown`, or with `unresolved` one that drops each
/// `qm-island` token silently.
pub(crate) fn walk(
    markdown: &str,
    unresolved: bool,
) -> Result<(Content, Vec<ImportWarning>), ImportError> {
    let tags = match unresolved {
        true => Tags::Unresolved,
        false => Tags::Cold,
    };
    let (walked, warnings) = import(markdown, tags)?;
    Ok((walked.lift().0, warnings))
}

/// A [`walk_against`] import.
pub(crate) struct Against {
    /// The content, each token's island in it and no anchor.
    pub(crate) content: Content,
    /// A cold import's warnings, with each tag and token reported in place
    /// of a token's drop.
    pub(crate) warnings: Vec<ImportWarning>,
    /// Each anchor a tag pins, in position order.
    pub(crate) pins: Vec<Pin>,
}

/// Import `markdown` as a revise of `stored`, reading the tags of an
/// annotated read (markdown-spec §6.4). A `<qm-anchor ref="ID">` tag in prose
/// pins the anchor `ID` the stored value holds in prose to start where the
/// tag sits; one with an `at` attribute, which the read writes where its tag
/// moved to its line's end, pins nothing. A `<qm-island ref="ID">` token in
/// prose is the stored island `ID`, its type and props as stored but for its
/// cells' anchors, which the revise carries onto the island that continues it
/// ([`crate::carry::carry_onto`]), so a token written twice copies none. A tag or a
/// token naming nothing the stored value holds there, and a second tag naming
/// one anchor, drop reported under `qm-anchor[ref]` or `qm-island[ref]`; a
/// token in a table cell, a note, an image's alt text or a code span drops
/// reported under `qm-island`. An anchor tag outside prose pins nothing. A
/// note holds no hard break at its edge.
pub(crate) fn walk_against(markdown: &str, stored: &Content) -> Result<Against, ImportError> {
    let anchors = stored.anchors.iter().map(|a| a.id.clone()).collect();
    let islands = stored
        .islands
        .iter()
        .map(|i| (i.id.clone(), i.clone()))
        .collect();
    let (walked, warnings) = import(markdown, Tags::Against { anchors, islands })?;
    let (mut content, pins) = walked.lift();
    for island in &mut content.islands {
        if island.island_type == IslandType::Footnote {
            island.slots = crate::export::held_slots(island).into_iter().collect();
        }
    }
    Ok(Against {
        content,
        warnings,
        pins,
    })
}

fn import(markdown: &str, tags: Tags) -> Result<(Walked, Vec<ImportWarning>), ImportError> {
    if markdown.len() > crate::MAX_MARKDOWN_SIZE {
        return Err(ImportError::TooLarge {
            size: markdown.len(),
            max: crate::MAX_MARKDOWN_SIZE,
        });
    }
    let mut b = Builder {
        tags,
        ..Builder::default()
    };
    let cells = b.tags.cells();
    parse_markdown(markdown, options(), |text, events| {
        b.refs = crate::markdown::html_table::refs(events.reference_definitions());
        b.run(MarkdownFixer::new(text, events.into_iter(), cells))
    })?;
    let (content, mut dropped) = b.finish();
    let mut at = 0;
    for c in markdown.chars() {
        if crate::model::is_bidi_char(c) {
            dropped.add(DroppedConstruct::Bidi, at);
        } else if c != '\r' {
            at += c.len_utf8();
        }
    }
    Ok((content, dropped.into_warnings()))
}

/// One line the walk opens: the block it starts or, across a hard break,
/// continues, and the lists and quotes around it, outermost first.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    kind: BlockKind,
    containers: Vec<Level>,
    continues: bool,
}

/// A list item or a quote around a line. `instance` tells a list or quote
/// from an adjacent one of its shape, and `ordinal` an item from its list's
/// others.
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

impl Level {
    /// Whether `other` sits in the same list or quote.
    fn same_run(&self, other: &Level) -> bool {
        match (self, other) {
            (
                Level::ListItem {
                    ordered: a,
                    start: s,
                    instance: i,
                    ..
                },
                Level::ListItem {
                    ordered: b,
                    start: t,
                    instance: j,
                    ..
                },
            ) => a == b && s == t && i == j,
            (Level::Quote { instance: i }, Level::Quote { instance: j }) => i == j,
            _ => false,
        }
    }
}

/// What a walk read: the text, a `\n` between lines, one [`Line`] per
/// segment, the marks and pins over it and an island per slot.
struct Walked {
    text: String,
    lines: Vec<Line>,
    marks: Vec<Mark>,
    pins: Vec<Pin>,
    islands: Vec<Island>,
}

impl Walked {
    /// The content, ids minted in reading order, and each pin where
    /// normalizing leaves it: a line run joined by hard breaks is one block,
    /// and a table sharing its line takes a block of its own.
    fn lift(mut self) -> (Content, Vec<Pin>) {
        self.split_block_islands();
        self.settle_continues();
        let mut text = String::with_capacity(self.text.len());
        let mut lines = self.lines.iter();
        let mut segs = self.text.split('\n');
        let mut starts: Vec<&Line> = Vec::new();
        if let (Some(first), Some(seg)) = (lines.next(), segs.next()) {
            text.push_str(seg);
            starts.push(first);
        }
        for (l, seg) in lines.zip(segs) {
            if l.continues {
                text.push(HARD_BREAK);
            } else {
                text.push('\n');
                starts.push(l);
            }
            text.push_str(seg);
        }
        enum Open {
            List { list: String, item: String },
            Quote(String),
        }
        let mut blocks: Vec<Block> = Vec::with_capacity(starts.len());
        let mut containers: Vec<Container> = Vec::new();
        let mut minter = crate::ids::Minter::default();
        let mut open: Vec<Open> = Vec::new();
        let mut prev: Option<&[Level]> = None;
        for (n, l) in starts.iter().enumerate() {
            let mut path = Vec::new();
            let mut opened = false;
            for (d, here) in l.containers.iter().enumerate() {
                let before = prev.and_then(|p| p.get(d));
                let same_run = !opened && before.is_some_and(|b| b.same_run(here));
                if !same_run {
                    open.truncate(d);
                    opened = true;
                    open.push(match here {
                        Level::ListItem {
                            ordered,
                            start,
                            checked,
                            ..
                        } => {
                            let list = minter.mint(prefix::LIST);
                            let item = minter.mint(prefix::ITEM);
                            containers.push(Container::new(
                                &list,
                                ContainerKind::List {
                                    ordered: *ordered,
                                    start: *start,
                                },
                            ));
                            containers.push(Container::new(
                                &item,
                                ContainerKind::Item { checked: *checked },
                            ));
                            Open::List { list, item }
                        }
                        Level::Quote { .. } => {
                            let quote = minter.mint(prefix::QUOTE);
                            containers.push(Container::new(&quote, ContainerKind::Quote));
                            Open::Quote(quote)
                        }
                    });
                } else if let (Level::ListItem { checked, .. }, Some(b)) = (here, before)
                    && b != here
                {
                    open.truncate(d + 1);
                    opened = true;
                    let item = minter.mint(prefix::ITEM);
                    containers.push(Container::new(
                        &item,
                        ContainerKind::Item { checked: *checked },
                    ));
                    if let Some(Open::List { item: held, .. }) = open.get_mut(d) {
                        *held = item;
                    }
                }
                match &open[d] {
                    Open::List { list, item } => {
                        path.push(list.clone());
                        path.push(item.clone());
                    }
                    Open::Quote(q) => path.push(q.clone()),
                }
            }
            open.truncate(l.containers.len());
            prev = Some(&l.containers);
            let kind = match &l.kind {
                BlockKind::Code { lang } => BlockKind::Code {
                    lang: lang.clone().filter(|l| !l.is_empty()),
                },
                kind => kind.clone(),
            };
            blocks.push(Block::new(format!("{}{n}", prefix::BLOCK), kind).with_path(path));
        }
        let mut content = Content {
            text,
            blocks,
            containers,
            marks: self.marks,
            anchors: self
                .pins
                .into_iter()
                .map(|pin| Anchor::new(pin.id, pin.at, pin.at))
                .collect(),
            islands: self.islands,
            codec: Codec::Rich,
        };
        content.normalize();
        let pins = std::mem::take(&mut content.anchors)
            .into_iter()
            .map(|a| Pin {
                at: a.start,
                id: a.id,
            })
            .collect();
        (content, pins)
    }

    /// Clear `continues` on a line no hard break can join to the one above:
    /// one under a different container path, under a heading or a rule,
    /// under a line a table's slot ends, or holding a table's slot alone.
    fn settle_continues(&mut self) {
        let mut slot = 0usize;
        let tables: Vec<(bool, bool)> = self
            .text
            .split('\n')
            .map(|seg| {
                let slots = seg.chars().filter(|&c| c == ISLAND_SLOT).count();
                let ends = seg.ends_with(ISLAND_SLOT)
                    && self
                        .islands
                        .get(slot + slots - 1)
                        .is_some_and(|i| i.island_type.block_only());
                slot += slots;
                (ends, ends && seg.chars().count() == 1)
            })
            .collect();
        for i in 0..self.lines.len() {
            let joins = i > 0
                && self.lines[i].containers == self.lines[i - 1].containers
                && matches!(
                    self.lines[i - 1].kind,
                    BlockKind::Para | BlockKind::Code { .. }
                )
                && !tables.get(i - 1).is_some_and(|t| t.0)
                && !tables.get(i).is_some_and(|t| t.1);
            self.lines[i].continues &= joins;
        }
    }

    /// Give a table's slot the line its markup needs: a `\n` on each side
    /// that has other content, so the prose around it is its own line and the
    /// slot is alone. The slot's line is a paragraph, the prose pieces keep
    /// the line's role, and only the first still continues the block above. A
    /// pin right after the slot moves onto the line after it.
    fn split_block_islands(&mut self) {
        use crate::delta::{Assoc, Delta, Op};

        let chars: Vec<char> = self.text.chars().collect();
        let alone = |at: usize| {
            (at == 0 || chars[at - 1] == '\n') && matches!(chars.get(at + 1), None | Some('\n'))
        };
        let breaks: Vec<Scalar> = chars
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == ISLAND_SLOT)
            .zip(&self.islands)
            .filter(|&((at, _), island)| island.island_type.block_only() && !alone(at))
            .map(|((at, _), _)| at)
            .collect();
        if breaks.is_empty() {
            return;
        }
        let mut cuts: Vec<Scalar> = Vec::with_capacity(breaks.len() * 2);
        for &at in &breaks {
            if at > 0 && chars[at - 1] != '\n' {
                cuts.push(at);
            }
            if chars.get(at + 1).is_some_and(|&c| c != '\n') {
                cuts.push(at + 1);
            }
        }
        cuts.dedup();

        let fragment = |line: &Line, span: Range<Scalar>, first: bool| {
            if span.len() == 1 && breaks.binary_search(&span.start).is_ok() {
                return Line {
                    kind: BlockKind::Para,
                    containers: line.containers.clone(),
                    continues: false,
                };
            }
            Line {
                kind: line.kind.clone(),
                containers: line.containers.clone(),
                continues: first && line.continues,
            }
        };
        let mut lines = Vec::with_capacity(self.lines.len() + cuts.len());
        let mut cut = cuts.iter().copied().peekable();
        let mut pos = 0usize;
        for (line, seg) in self.lines.iter().zip(self.text.split('\n')) {
            let end = pos + seg.chars().count();
            let mut start = pos;
            let mut first = true;
            while let Some(p) = cut.next_if(|&p| p < end) {
                lines.push(fragment(line, start..p, first));
                (start, first) = (p, false);
            }
            lines.push(fragment(line, start..end, first));
            pos = end + 1;
        }

        let mut text = String::with_capacity(self.text.len() + cuts.len());
        let mut at_cut = cuts.iter().copied().peekable();
        for (i, &c) in chars.iter().enumerate() {
            if at_cut.next_if_eq(&i).is_some() {
                text.push('\n');
            }
            text.push(c);
        }
        let mut ops = Vec::with_capacity(cuts.len() * 2);
        let mut last = 0usize;
        for &p in &cuts {
            ops.push(Op::Retain(p - last));
            ops.push(Op::Insert("\n".to_string()));
            last = p;
        }
        let delta = Delta { ops };
        for m in &mut self.marks {
            m.start = delta.map_pos(m.start, Assoc::After);
            m.end = delta.map_pos(m.end, Assoc::Before);
        }
        for pin in &mut self.pins {
            let assoc = match breaks.contains(&(pin.at.wrapping_sub(1))) {
                true => Assoc::After,
                false => Assoc::Before,
            };
            pin.at = delta.map_pos(pin.at, assoc);
        }
        self.text = text;
        self.lines = lines;
    }
}

/// A table's props and slots from the rows its source writes, the header
/// first: `layout` the layout keys an HTML table's tags set, and `aligns`
/// each column's alignment.
fn table_island(
    layout: Map<String, Value>,
    aligns: &[&'static str],
    rows: Vec<Vec<Cell>>,
) -> (Value, BTreeMap<String, Content>) {
    let aligns: Vec<Value> = aligns.iter().map(|a| Value::from(*a)).collect();
    let rows: Vec<Vec<crate::table::GridCell>> = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| crate::table::GridCell {
                    text: cell.text,
                    marks: cell.marks,
                    anchors: Vec::new(),
                    keys: cell.keys,
                })
                .collect()
        })
        .collect();
    crate::table::from_grid(
        &layout,
        Some(&aligns),
        &rows,
        &mut crate::ids::Minter::default(),
    )
}

/// A flat inline accumulator: `text` plus `marks` over local scalar offsets, with
/// the content char-filtering baked in. Serves both a prose line's inline
/// content (embedded in the [`Builder`]) and a table cell's isolated content.
#[derive(Default)]
struct Inline {
    text: String,
    /// scalar position = char count of [`Self::text`].
    pos: usize,
    marks: Vec<Mark>,
    /// Each anchor a revise's tag pins, where the tag sits.
    pins: Vec<Pin>,
    /// `(kind, start)` for each mark opened but not yet closed.
    open: Vec<(MarkKind, usize)>,
    /// Each `<u>` this run has read and whose `</u>` it awaits, innermost last:
    /// a bare one's start position and tag offset, `None` for one carrying an
    /// attribute, which pairs and underlines nothing.
    underlines: Vec<Option<(usize, usize)>>,
    /// Each `<qm-span>` this run has read and whose `</qm-span>` it awaits,
    /// innermost last, `None` for one setting nothing.
    fonts: Vec<Option<OpenSpan>>,
}

/// An open `qm-span` pair setting a family: the stretches it sets so far,
/// which land as marks only when it closes.
struct OpenSpan {
    /// The open tag's byte offset.
    at: usize,
    family: String,
    /// Where its current stretch began, while no inner family sets the text.
    from: usize,
    /// Its stretches closed by an inner pair setting a family.
    done: Vec<(usize, usize)>,
}

impl Inline {
    /// Append inline text under the [`crate::model::admit_char`] contract,
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
        for pin in &mut self.pins {
            pin.at = pin.at.min(pos);
        }
        for (_, start) in &mut self.open {
            *start = (*start).min(pos);
        }
        for (start, _) in self.underlines.iter_mut().flatten() {
            *start = (*start).min(pos);
        }
        for span in self.fonts.iter_mut().flatten() {
            span.from = span.from.min(pos);
            for (start, end) in &mut span.done {
                *start = (*start).min(pos);
                *end = (*end).min(pos);
            }
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

    /// Pair one `<qm-span>` tag as [`underline`](Self::underline) pairs a
    /// `<u>`. An inner pair setting a family overrides the outer one over its
    /// text, and the outer resumes past it, so the marks a run lands never
    /// overlap; a pair setting nothing leaves the family in force.
    fn font(&mut self, tag: SpanTag) {
        let pos = self.pos;
        match tag {
            SpanTag::Open { at, family } => {
                if let Some(outer) = self.fonts.iter_mut().rev().find_map(Option::as_mut) {
                    outer.done.push((outer.from, pos));
                }
                self.fonts.push(Some(OpenSpan {
                    at,
                    family,
                    from: pos,
                    done: Vec::new(),
                }));
            }
            SpanTag::Held => self.fonts.push(None),
            SpanTag::Close => {
                let Some(Some(mut span)) = self.fonts.pop() else {
                    return;
                };
                span.done.push((span.from, pos));
                for (start, end) in span.done {
                    if start < end {
                        let kind = MarkKind::FontFamily {
                            name: span.family.clone(),
                        };
                        self.marks.push(Mark { start, end, kind });
                    }
                }
                if let Some(outer) = self.fonts.iter_mut().rev().find_map(Option::as_mut) {
                    outer.from = pos;
                }
            }
        }
    }

    /// End the run: each bare `<u>` and each `<qm-span>` setting something
    /// still open drops as unclosed.
    fn drop_open(&mut self, dropped: &mut Drops) {
        for (_, at) in self.underlines.drain(..).flatten() {
            dropped.add(DroppedConstruct::Tag("u".into()), at);
        }
        for span in self.fonts.drain(..).flatten() {
            dropped.add(DroppedConstruct::Element(carrier::SPAN.into()), span.at);
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
    pending: Option<(BlockKind, bool)>,
    /// The text position after the boundary of the line a mark or `<u>` opened
    /// [early](Self::open_early), while nothing has landed in it.
    early: Option<usize>,
    /// The text position after a line break: a second break there is nothing.
    broke_at: Option<usize>,
    islands: Vec<Island>,
    containers: Vec<Level>,
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
    /// Each `qm-*` name's open tags no close tag has met yet, which a close tag
    /// dropping with markdown answers rather than reporting again.
    opened: HashMap<String, usize>,
    /// What this walk drops: a raw tag, an unclosed `<u>`, a wrapper left
    /// unclosed or outside a tag line, an attribute it cannot carry.
    dropped: Drops,
    /// The footnote definition being read.
    note: Option<NoteAcc>,
    /// Each definition read, by the label pulldown matches references with.
    notes: HashMap<UniCase<String>, Note>,
    /// Each footnote island minted, by index, the label it reads and the
    /// reference's byte offset.
    note_refs: Vec<(usize, UniCase<String>, usize)>,
    /// How the walk reads an anchor tag or an island token.
    tags: Tags,
    /// Each anchor id a tag has named, so a second tag naming it reports.
    pinned: HashSet<String>,
    /// The document's link reference definitions, which an HTML table's
    /// cells, read by parses of their own, resolve against.
    refs: crate::markdown::html_table::Refs,
}

/// How an import reads the `qm-anchor` tags and `qm-island` tokens of an
/// annotated read.
#[derive(Default)]
enum Tags {
    /// A cold import: both drop, a token reported.
    #[default]
    Cold,
    /// Both drop silently, a later [`from_markdown_against`] reading them.
    Unresolved,
    /// A revise of a stored value: its prose anchors' ids and its islands by
    /// id.
    Against {
        anchors: HashSet<String>,
        islands: HashMap<String, Island>,
    },
}

impl Tags {
    /// The slots the import's tables share:
    /// [`MAX_TABLE_CELLS`](crate::MAX_TABLE_CELLS), or a revise's stored
    /// tables' slots where they pass it, so a read of the stored value imports
    /// every row it holds.
    fn cells(&self) -> usize {
        let stored = match self {
            Tags::Against { islands, .. } => islands
                .values()
                .filter(|i| i.island_type == IslandType::Table)
                .filter_map(|i| Table::read(&i.props))
                .map(|t| t.rows.len().saturating_mul(t.columns.len()))
                .fold(0, usize::saturating_add),
            Tags::Cold | Tags::Unresolved => 0,
        };
        stored.max(crate::MAX_TABLE_CELLS)
    }
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
    /// The note's own [`Builder::opened`].
    opened: HashMap<String, usize>,
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
    text: String,
    marks: Vec<Mark>,
    /// The note's characters, which each reference past the first copies.
    chars: usize,
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

/// One cell a table's source writes: its text and marks, and the cell keys
/// its attributes set.
#[derive(Default)]
struct Cell {
    text: String,
    marks: Vec<Mark>,
    keys: Map<String, Value>,
}

#[derive(Default)]
struct TableAcc {
    aligns: Vec<&'static str>,
    header: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
    cur_row: Vec<Cell>,
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
    fn open_line(&mut self, kind: BlockKind, continues: bool) {
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
    fn ensure_open(&mut self, default: BlockKind) {
        if let Some((k, cont)) = self.pending.take() {
            self.open_line(k, cont);
        } else if self.cur.is_none() {
            self.open_line(default, false);
        }
    }

    fn push_inline(&mut self, s: &str) {
        self.ensure_open(BlockKind::Para);
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
        if matches!(kind, BlockKind::Para) {
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
            self.open_line(BlockKind::Para, false);
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
        self.ensure_open(BlockKind::Para);
        if self.emitted() > emitted {
            self.early = Some(self.inline.pos);
        }
    }

    /// Take back the paragraph line [`Self::open_early`] opened where nothing
    /// landed in it, everything it would hold having dropped. A heading keeps
    /// its line.
    fn retract_empty_line(&mut self) {
        if self.early.take() != Some(self.inline.pos)
            || !self.cur.as_ref().is_some_and(|l| l.kind == BlockKind::Para)
        {
            return;
        }
        self.cur = self.lines.pop();
        if self.cur.is_some() {
            self.inline.unpush();
        }
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
    fn mint_island(
        &mut self,
        kind: IslandType,
        props: serde_json::Value,
        slots: BTreeMap<String, Content>,
    ) {
        let id = format!("isl-{}", self.islands.len());
        self.islands.push(Island {
            id,
            island_type: kind,
            props,
            slots,
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
                    self.carrier_tag(tag);
                    continue;
                }
                Fixed::Underline(tag) => {
                    self.underline_tag(tag);
                    continue;
                }
                Fixed::Span(tag) => {
                    self.span_tag(tag);
                    continue;
                }
                Fixed::Swallowed(name, at) => {
                    self.swallowed(name, at);
                    continue;
                }
                Fixed::DroppedConstruct(construct, at) => {
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
                            self.dropped.add(DroppedConstruct::Footnote, at);
                        }
                        None => {
                            self.note = Some(NoteAcc {
                                label,
                                at,
                                ..NoteAcc::default()
                            })
                        }
                    }
                    continue;
                }
                Fixed::NoteRef { label, at } => {
                    self.note_ref(label, at);
                    continue;
                }
                Fixed::Math {
                    tex,
                    display,
                    source,
                    at,
                } => {
                    self.math(&tex, display, source, at);
                    continue;
                }
                Fixed::HtmlTable(table) => {
                    self.html_table(table)?;
                    continue;
                }
                Fixed::Held(tag) => {
                    self.held_tag(tag);
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
                    self.ensure_open(BlockKind::Para);
                    self.inline.push_code(&t);
                }
                Event::Rule => {
                    self.open_line(BlockKind::Rule, false);
                    self.rearm_item();
                }
                // A soft break with nothing before it on its line, all before it
                // having dropped, is nothing, and so is one beside a hard break.
                Event::SoftBreak if self.line_empty() || self.broke_at == Some(self.inline.pos) => {
                }
                Event::TaskListMarker(done) => {
                    if let Some(Level::ListItem { checked, .. }) = self.containers.last_mut() {
                        *checked = Some(done);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    self.broke_at = Some(self.inline.pos);
                    // A break with no text before it on its line is dropped:
                    // pulldown never emits one there, an inline `<br>` can, and
                    // arming a continuation would join the block to the one above.
                    if self.line_empty() {
                        continue;
                    }
                    match self.cur.as_ref().map(|l| &l.kind) {
                        // A heading is one line.
                        Some(BlockKind::Heading { .. }) => {
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
                                .unwrap_or(BlockKind::Para);
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
            Tag::Paragraph => self.pending = Some((BlockKind::Para, false)),
            Tag::Heading { level, .. } => {
                self.pending = Some((BlockKind::Heading { level: level as u8 }, false))
            }
            Tag::CodeBlock(kind) => {
                self.pending = None; // code opens its own lines
                self.in_code = true;
                self.code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        let l = sanitize_lang(&lang);
                        if l.is_empty() { None } else { Some(l) }
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
                self.pending = Some((BlockKind::Para, false));
                self.container_marks.push(self.emitted());
                let container = match self.list_stack.last_mut() {
                    Some(info) => {
                        let ordinal = info.count;
                        info.count += 1;
                        Level::ListItem {
                            ordered: info.ordered,
                            start: info.start,
                            ordinal,
                            checked: None,
                            instance: info.instance,
                        }
                    }
                    None => Level::ListItem {
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
                self.containers.push(Level::Quote { instance });
                self.check_depth()?;
            }
            Tag::Table(aligns) => {
                self.pending = None;
                self.open_line(BlockKind::Para, false);
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
                    self.open_line(BlockKind::Code { lang }, false);
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
                let mark = self.container_marks.pop().unwrap_or(0);
                self.close_container(mark);
            }
            TagEnd::BlockQuote(_) => {
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

    /// Drop one carrier tag: an open tag reports `qm-<name>`, a close tag
    /// drops silently, and what lies between them imports as written.
    fn carrier_tag(&mut self, tag: CarrierTag) {
        let CarrierTag {
            name, attrs, at, ..
        } = tag;
        let (opened, dropped) = match self.note.as_mut() {
            Some(note) => (&mut note.opened, &mut note.dropped),
            None => (&mut self.opened, &mut self.dropped),
        };
        let open = opened.entry(name.clone()).or_default();
        if attrs.is_none() {
            *open = open.saturating_sub(1);
            return;
        }
        *open += 1;
        dropped.add(DroppedConstruct::Element(name), at);
    }

    /// One anchor tag or island token. A revise pins a prose anchor its
    /// stored value holds where a tag without `at` names it, and places the
    /// stored island a token names where prose can hold it. A tag naming no
    /// anchor or island the stored value holds, or an anchor another tag named
    /// already, reports its `ref`, and a token where no island sits reports
    /// itself.
    fn held_tag(&mut self, tag: HeldTag) {
        let HeldTag { element, attrs, at } = tag;
        let id = attrs.values.get("ref");
        let prose =
            self.note.is_none() && self.image_depth == 0 && self.table.is_none() && !self.in_code;
        let report = match &self.tags {
            Tags::Unresolved => return,
            Tags::Cold if element == carrier::ANCHOR => return,
            Tags::Cold => Some(DroppedConstruct::Element(element)),
            Tags::Against { islands, .. } if element == carrier::ISLAND => {
                match id.and_then(|id| islands.get(id)) {
                    Some(island) if prose => {
                        let (props, slots) = crate::table::token_island(island);
                        let island_type = island.island_type.clone();
                        self.ensure_open(BlockKind::Para);
                        self.inline.push_raw(ISLAND_SLOT);
                        self.mint_island(island_type, props, slots);
                        None
                    }
                    Some(_) => Some(DroppedConstruct::Element(element)),
                    None => Some(DroppedConstruct::ElementAttr {
                        element,
                        attr: "ref".into(),
                    }),
                }
            }
            Tags::Against { anchors, .. } => match id {
                Some(id) if anchors.contains(id) && self.pinned.insert(id.clone()) => {
                    if prose && !attrs.values.contains_key("at") {
                        self.open_early();
                        let pos = self.inline.pos;
                        self.inline.pins.push(Pin {
                            at: pos,
                            id: id.clone(),
                        });
                    }
                    None
                }
                _ => Some(DroppedConstruct::ElementAttr {
                    element,
                    attr: "ref".into(),
                }),
            },
        };
        if let Some(construct) = report {
            match self.note.as_mut() {
                Some(note) => note.dropped.add(construct, at),
                None => self.dropped.add(construct, at),
            }
        }
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

    /// Pair one `<qm-span>` tag in the run, table cell or note it stands in,
    /// as [`Self::underline_tag`] pairs a `<u>`.
    fn span_tag(&mut self, tag: SpanTag) {
        if let Some(note) = self.note.as_mut() {
            if note.img_depth == 0 {
                if matches!(tag, SpanTag::Open { .. }) {
                    note.fill();
                }
                note.inline.font(tag);
            }
            return;
        }
        if self.image_depth > 0 {
            return;
        }
        if let Some(acc) = self.table.as_mut() {
            if let Some(cell) = acc.cell.as_mut() {
                cell.font(tag);
            }
            return;
        }
        if matches!(tag, SpanTag::Open { .. }) {
            self.open_early();
        }
        self.inline.font(tag);
    }

    /// Report a close tag that dropped with the markdown under it, unless an
    /// open tag of its name already reported the element.
    fn swallowed(&mut self, name: String, at: usize) {
        let (opened, dropped) = match self.note.as_mut() {
            Some(note) => (&mut note.opened, &mut note.dropped),
            None => (&mut self.opened, &mut self.dropped),
        };
        if let Some(open) = opened.get_mut(&name).filter(|n| **n > 0) {
            *open -= 1;
            return;
        }
        dropped.add(DroppedConstruct::Element(name), at);
    }

    /// A tight list item's inline content arrives with no `Paragraph` start to
    /// arm its line, so after a block nested in the item the next text must be
    /// armed here or it joins that block's last line.
    fn rearm_item(&mut self) {
        if matches!(self.containers.last(), Some(Level::ListItem { .. })) {
            self.pending = Some((BlockKind::Para, false));
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
                BlockKind::Code {
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
    /// A cell is flat inline, so its marks are scalar offsets into its own text.
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
                    acc.cur_row.push(Cell {
                        text: cell.text,
                        marks: cell.marks,
                        keys: Map::new(),
                    });
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
            let mut rows = vec![acc.header];
            rows.extend(acc.rows);
            let (props, slots) = table_island(Map::new(), &acc.aligns, rows);
            self.mint_island(IslandType::Table, props, slots);
            self.rearm_item();
        }
    }

    /// An HTML table: an island on a line of its own, as a pipe table is, each
    /// cell's content read through the inline import into the cell. In a
    /// footnote's note it is the note's text, a line per row.
    fn html_table(&mut self, table: crate::markdown::html_table::Table) -> Result<(), ImportError> {
        use crate::markdown::html_table::Slot;
        if self.note.is_some() {
            return self.note_table(table);
        }
        self.inline.drop_open(&mut self.dropped);
        self.retract_empty_line();
        self.pending = None;
        self.open_line(BlockKind::Para, false);
        self.inline.push_raw(ISLAND_SLOT);
        self.table = Some(TableAcc::default());
        let mut grid = Vec::with_capacity(table.rows.len());
        for row in table.rows {
            let mut cells = Vec::with_capacity(row.len());
            for slot in row {
                cells.push(match slot {
                    Slot::Empty => Cell::default(),
                    Slot::Cell(cell) => self.html_cell(cell)?,
                });
            }
            grid.push(cells);
        }
        self.table = None;
        if grid.is_empty() {
            grid.push(Vec::new());
        }
        let (props, slots) = table_island(table.layout, &table.aligns, grid);
        self.mint_island(IslandType::Table, props, slots);
        self.rearm_item();
        Ok(())
    }

    fn html_cell(&mut self, cell: crate::markdown::html_table::Cell) -> Result<Cell, ImportError> {
        if let Some(acc) = self.table.as_mut() {
            acc.cell = Some(Inline::default());
        }
        let paired = self.inline_source(&cell.content, cell.at)?;
        let mut inline = self.take_cell();
        inline.drop_open(&mut self.dropped);
        if paired {
            let md = crate::export::html_cell_markdown(&inline.text, &inline.marks);
            if let Some(acc) = self.table.as_mut() {
                acc.cell = Some(Inline::default());
            }
            let dropped = std::mem::take(&mut self.dropped);
            self.inline_source(&md, cell.at)?;
            self.dropped = dropped;
            inline = self.take_cell();
            inline.drop_open(&mut Drops::default());
        }
        Ok(Cell {
            text: inline.text,
            marks: inline.marks,
            keys: cell.keys,
        })
    }

    /// The open HTML table cell's run, its marks closed.
    fn take_cell(&mut self) -> Inline {
        let mut inline = self
            .table
            .as_mut()
            .and_then(|acc| acc.cell.take())
            .unwrap_or_default();
        inline.close_marks();
        inline
    }

    /// An HTML table inside a footnote's note, flattened as a pipe table is:
    /// each row a line of the note, its cells' text apart by a space.
    fn note_table(&mut self, table: crate::markdown::html_table::Table) -> Result<(), ImportError> {
        use crate::markdown::html_table::Slot;
        if let Some(note) = self.note.as_mut() {
            note.dropped
                .add(DroppedConstruct::NoteBlock("table"), note.at);
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
    /// ([`cell_source`](crate::markdown::html_table::cell_source)): whether a
    /// formatting tag in it read as a mark.
    fn inline_source(&mut self, content: &str, at: usize) -> Result<bool, ImportError> {
        let src = crate::markdown::html_table::cell_source(
            &crate::markdown::html_table::cell_paragraphs(content),
        );
        let (events, paired) = crate::markdown::html_table::cell_events(&src, &self.refs);
        self.run(
            MarkdownFixer::new(&src, events.into_iter(), crate::MAX_TABLE_CELLS)
                .map(|item| item.shifted(at)),
        )?;
        Ok(paired)
    }

    fn emit_image(&mut self) {
        self.ensure_open(BlockKind::Para);
        self.inline.push_raw(ISLAND_SLOT);
        let props = json!({
            "url": self.image_url,
            "alt": self.image_alt.trim(),
        });
        self.mint_island(IslandType::Image, props, BTreeMap::new());
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
            Event::Rule => Some("rule"),
            _ => None,
        };
        if let Some(block) = structure {
            note.dropped
                .add(DroppedConstruct::NoteBlock(block), note.at);
        }
        match event {
            Event::End(TagEnd::FootnoteDefinition) if note.nested > 0 => note.nested -= 1,
            Event::End(TagEnd::FootnoteDefinition) => {
                self.close_note();
                self.rearm_item();
            }
            Event::Start(
                Tag::Paragraph | Tag::Heading { .. } | Tag::Item | Tag::TableHead | Tag::TableRow,
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
            return self.dropped.add(DroppedConstruct::Footnote, note.at);
        }
        let chars = note.inline.text.chars().count();
        self.notes.insert(
            key,
            Note {
                text: note.inline.text,
                marks: note.inline.marks,
                chars,
                at: note.at,
                used: false,
                dropped: note.dropped,
            },
        );
    }

    /// A footnote reference: an island in prose, its note filled in once
    /// every definition is read. Anywhere else it drops.
    fn note_ref(&mut self, label: String, at: usize) {
        if let Some(note) = self.note.as_mut() {
            return note.dropped.add(DroppedConstruct::Footnote, at);
        }
        if self.table.is_some() || self.image_depth > 0 {
            return self.dropped.add(DroppedConstruct::Footnote, at);
        }
        self.ensure_open(BlockKind::Para);
        self.inline.push_raw(ISLAND_SLOT);
        self.note_refs
            .push((self.islands.len(), UniCase::new(label), at));
        self.mint_island(IslandType::Footnote, json!({}), BTreeMap::new());
    }

    /// An equation: an island in prose, and its markup's text, `source`,
    /// where no island can sit.
    fn math(&mut self, tex: &str, display: bool, source: &str, at: usize) {
        if let Some(note) = self.note.as_mut() {
            note.fill();
            note.inline.push_text(source);
            return note.dropped.add(DroppedConstruct::Math, at);
        }
        let cell = self.table.as_mut().and_then(|acc| acc.cell.as_mut());
        if self.image_depth > 0
            || cell.is_some()
            || !crate::export::tex_is_writable(&crate::model::settle_tex(tex), display)
        {
            match cell {
                Some(cell) => cell.push_text(source),
                None if self.image_depth > 0 => self.image_alt.extend(admitted(source)),
                None => {
                    self.ensure_open(BlockKind::Para);
                    self.inline.push_text(source);
                }
            }
            return self.dropped.add(DroppedConstruct::Math, at);
        }
        self.ensure_open(BlockKind::Para);
        self.inline.push_raw(ISLAND_SLOT);
        let props = match display {
            true => json!({ "tex": tex, "display": true }),
            false => json!({ "tex": tex }),
        };
        self.mint_island(IslandType::Math, props, BTreeMap::new());
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
        self.inline
            .text
            .replace_range(byte..byte + ISLAND_SLOT.len_utf8(), text);
        let grown = text.chars().count() - 1;
        for m in &mut self.inline.marks {
            if m.start > at {
                m.start += grown;
            }
            if m.end > at {
                m.end += grown;
            }
        }
        for pin in &mut self.inline.pins {
            if pin.at > at {
                pin.at += grown;
            }
        }
        self.islands.remove(index);
    }

    fn finish(mut self) -> (Walked, Drops) {
        self.close_note();
        // A reference whose definition dropped, nested in another, reads
        // back as the text it is, as one naming no definition does, and so
        // does one past the budget its note's copies share.
        let mut budget = crate::MAX_NOTE_COPIES;
        let mut read: HashSet<&UniCase<String>> = HashSet::new();
        let copies: Vec<bool> = self
            .note_refs
            .iter()
            .map(|(_, label, _)| match self.notes.get(label) {
                Some(_) if read.insert(label) => true,
                Some(note) if note.chars <= budget => {
                    budget -= note.chars;
                    true
                }
                _ => false,
            })
            .collect();
        for ((index, label, at), copied) in std::mem::take(&mut self.note_refs)
            .into_iter()
            .zip(copies)
            .rev()
        {
            match self.notes.get_mut(&label) {
                Some(note) if copied => {
                    let slot = crate::table::cell_slot(&note.text, &note.marks, &[], false);
                    self.islands[index].slots = [(NOTE_SLOT.to_string(), slot)].into();
                    note.used = true;
                }
                held => {
                    if held.is_some() {
                        self.dropped.add(DroppedConstruct::FootnoteSize, at);
                    }
                    self.unslot(index, &format!("[^{}]", label.into_inner()));
                }
            }
        }
        for (n, island) in self.islands.iter_mut().enumerate() {
            island.id = format!("isl-{n}");
        }
        for note in std::mem::take(&mut self.notes).into_values() {
            match note.used {
                true => self.dropped.absorb(note.dropped),
                false => self.dropped.add(DroppedConstruct::Footnote, note.at),
            }
        }
        self.inline.drop_open(&mut self.dropped);
        self.retract_empty_line();
        if let Some(last) = self.cur.take() {
            self.lines.push(last);
        }
        if self.lines.is_empty() {
            self.lines.push(Line {
                kind: BlockKind::Para,
                containers: Vec::new(),
                continues: false,
            });
        }
        self.inline.close_marks();
        let walked = Walked {
            text: self.inline.text,
            lines: self.lines,
            marks: self.inline.marks,
            pins: self.inline.pins,
            islands: self.islands,
        };
        (walked, self.dropped)
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
        Tag::Link { dest_url, .. } => MarkKind::Link {
            url: admit_url(dest_url),
        },
        _ => return None,
    })
}

/// `s` as inline text admits it: under [`crate::model::admit_char`], a
/// stray [`ISLAND_SLOT`] dropped and a stray `\n` spaced.
fn admitted(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().filter_map(|c| match c {
        ISLAND_SLOT => None,
        '\n' => Some(' '),
        other => crate::model::admit_char(other),
    })
}

/// A link or image destination under [`crate::model::admit_char`], with
/// no line ending, which a destination can hold only as a character
/// reference and a URL parser strips.
fn admit_url(url: &str) -> String {
    url.chars()
        .filter(|&c| c != '\n')
        .filter_map(crate::model::admit_char)
        .collect()
}

fn ends_mark(end: &TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link
    )
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
/// lane that mints a `BlockKind::Code` runs it — the storage decode and the op
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
    Span(SpanTag),
    /// A wrapper's close tag in an HTML block of closing tags that drops
    /// markdown with them, at the block's byte offset.
    Swallowed(String, usize),
    /// A construct the fixer dropped, at its byte offset.
    DroppedConstruct(DroppedConstruct, usize),
    /// A footnote definition's start, at its byte offset.
    NoteDef {
        label: String,
        at: usize,
    },
    /// A footnote reference, at its byte offset.
    NoteRef {
        label: String,
        at: usize,
    },
    /// An equation: its TeX, whether `$$` delimits it, and its markup as
    /// written, at its byte offset.
    Math {
        tex: pulldown_cmark::CowStr<'a>,
        display: bool,
        source: &'a str,
        at: usize,
    },
    /// A table an HTML block spells.
    HtmlTable(crate::markdown::html_table::Table),
    /// An inline `qm-anchor` or `qm-island` open tag.
    Held(HeldTag),
}

/// An inline open tag of a [reserved](carrier::RESERVED) element: an anchor
/// tag or an island token, which only a revise reads.
struct HeldTag {
    element: String,
    attrs: carrier::Attrs,
    /// The tag's byte offset.
    at: usize,
}

impl Fixed<'_> {
    /// This item with each byte offset it carries `by` further on, for an
    /// item read off text that starts there.
    fn shifted(self, by: usize) -> Self {
        match self {
            Fixed::Carrier(tag) => Fixed::Carrier(CarrierTag {
                at: tag.at + by,
                ..tag
            }),
            Fixed::Underline(UTag::Open { at }) => Fixed::Underline(UTag::Open { at: at + by }),
            Fixed::Swallowed(name, at) => Fixed::Swallowed(name, at + by),
            Fixed::DroppedConstruct(construct, at) => Fixed::DroppedConstruct(construct, at + by),
            Fixed::NoteDef { label, at } => Fixed::NoteDef { label, at: at + by },
            Fixed::NoteRef { label, at } => Fixed::NoteRef { label, at: at + by },
            Fixed::Math {
                tex,
                display,
                source,
                at,
            } => Fixed::Math {
                tex,
                display,
                source,
                at: at + by,
            },
            other => other,
        }
    }
}

/// An inline `<u>` tag, paired by the builder as HTML pairs it.
#[derive(Clone, Copy)]
enum UTag {
    /// A bare `<u>`, at this byte offset: it underlines what it wraps.
    Open {
        at: usize,
    },
    /// A `<u>` carrying an attribute, or self-closing: it drops, and still
    /// pairs with a `</u>`.
    Held,
    Close,
}

/// An inline `<qm-span>` tag, paired by the builder as a `<u>` is.
enum SpanTag {
    /// An open tag setting a family, at this byte offset.
    Open {
        at: usize,
        family: String,
    },
    /// An open tag setting none, or self-closing: it drops, and still pairs
    /// with a `</qm-span>`.
    Held,
    Close,
}

/// An open or close `qm-*` tag the carrier does not read, which drops: not
/// self-closing.
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
pub(crate) fn dropped_tag(name: &str) -> Option<DroppedConstruct> {
    let lower = name.to_ascii_lowercase();
    Some(match carrier::element(name) {
        Some(element) if element == carrier::ANCHOR => return None,
        Some(element) => DroppedConstruct::Element(element),
        None if lower.starts_with(carrier::PREFIX) => DroppedConstruct::BadName(lower),
        None => DroppedConstruct::Tag(lower),
    })
}

/// Per construct, in order of its first report: its count and the byte offset
/// of its first occurrence.
#[derive(Default)]
struct Drops {
    entries: Vec<(DroppedConstruct, usize, usize)>,
    index: HashMap<DroppedConstruct, usize>,
}

impl Drops {
    fn add(&mut self, construct: DroppedConstruct, at: usize) {
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
    /// The `<u>` and `<qm-span>` tags open in the current inline run, where a
    /// line of carrier tags stays text too.
    underlines: usize,
    /// The open pipe table's column count, the slots its rows take so far, and
    /// whether a row past `cells` dropped.
    grid: Option<(usize, usize, bool)>,
    /// The slots the document's tables may still take, from [`Tags::cells`].
    cells: usize,
}

/// Whether a tag ending `end` holds a line of carrier tags as text.
fn deepens(end: &TagEnd) -> bool {
    ends_mark(end) || matches!(end, TagEnd::Image | TagEnd::Heading(_) | TagEnd::Table)
}

impl<'a, I> MarkdownFixer<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    fn new(src: &'a str, inner: I, cells: usize) -> Self {
        Self {
            src,
            inner,
            held: VecDeque::new(),
            ahead: None,
            line_start: false,
            depth: 0,
            underlines: 0,
            grid: None,
            cells,
        }
    }

    /// The slots the next table may take: what the document's tables leave,
    /// up to [`MAX_TABLE_CELLS`](crate::MAX_TABLE_CELLS).
    fn room(&self) -> usize {
        self.cells.min(crate::MAX_TABLE_CELLS)
    }

    /// Read past the next event `end` closes.
    fn skip_through(&mut self, end: TagEnd) {
        while let Some((event, _)) = self.read() {
            if event == Event::End(end) {
                return;
            }
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

    /// One inline `<qm-span>` tag, its refused attributes and a tag setting
    /// nothing handed on as drops.
    fn span_tag(&mut self, tag: &html::Tag, at: usize) -> SpanTag {
        if tag.closing {
            self.underlines = self.underlines.saturating_sub(1);
            return SpanTag::Close;
        }
        self.underlines += 1;
        let (family, refused) = carrier::span::read(&carrier::decode_attrs(&tag.attrs));
        for attr in refused {
            let element = carrier::SPAN.to_string();
            self.held.push_back(Fixed::DroppedConstruct(
                DroppedConstruct::ElementAttr { element, attr },
                at,
            ));
        }
        match family {
            Some(family) if !tag.self_closing => SpanTag::Open { at, family },
            _ => {
                self.drop_tag(tag, at);
                SpanTag::Held
            }
        }
    }

    /// Hand on what an opening tag reports where it drops.
    fn drop_tag(&mut self, tag: &html::Tag, at: usize) {
        if let Some(construct) = dropped_tag(tag.name).filter(|_| !tag.closing) {
            self.held.push_back(Fixed::DroppedConstruct(construct, at));
        }
    }

    /// The end and the tags of the line `event` opens, when it holds only the
    /// open and close tags of wrappers (markdown-spec §6.2).
    fn carrier_line(
        &self,
        event: &Event,
        range: &Range<usize>,
    ) -> Option<(usize, Vec<CarrierTag>)> {
        if self.depth > 0 || self.underlines > 0 || !matches!(event, Event::InlineHtml(_)) {
            return None;
        }
        let end = self.src[range.start..]
            .find('\n')
            .map_or(self.src.len(), |i| range.start + i);
        let tags = carrier::tag_line(&self.src[range.start..end])?;
        let carriers = tags
            .iter()
            .filter_map(|tag| CarrierTag::of(tag, true, range.start + tag.span.start));
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
            let Some((event, range)) = self.read() else {
                return;
            };
            match self.carrier_line(&event, &range) {
                Some(line) => (end, tags) = line,
                None => {
                    self.held
                        .push_back(Fixed::Event(Event::Start(Tag::Paragraph)));
                    self.ahead = Some((event, range));
                    return;
                }
            }
        }
    }

    /// Consume an HTML block through its end. One opening `<table>` hands on
    /// the tables it spells ([`crate::markdown::html_table::scan`]). Any other reports
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
        if crate::markdown::html_table::opens(&text) {
            let source = |off: usize| {
                let k = lines
                    .partition_point(|&(block, _)| block <= off)
                    .saturating_sub(1);
                lines.get(k).map_or(at, |&(block, src)| src + (off - block))
            };
            let (tables, drops) = crate::markdown::html_table::scan(&text, source, &mut self.cells);
            self.held.extend(
                drops
                    .into_iter()
                    .map(|(construct, at)| Fixed::DroppedConstruct(construct, at)),
            );
            self.held.extend(tables.into_iter().map(Fixed::HtmlTable));
            self.held
                .push_back(Fixed::Event(Event::End(TagEnd::HtmlBlock)));
            return;
        }
        let tags = html::block_tags(&text);
        let swallows = text
            .lines()
            .any(|l| !l.trim().is_empty() && html::tag_line(l).is_none());
        let reports = tags
            .iter()
            .any(|t| !t.closing && dropped_tag(t.name).is_some());
        if let Some(first) = tags.first().filter(|_| swallows && !reports) {
            self.held.push_back(
                match carrier::wrapper(first.name).filter(|_| first.closing) {
                    Some(name) => Fixed::Swallowed(name, at),
                    None => {
                        let anchor = DroppedConstruct::Tag(first.name.to_ascii_lowercase());
                        Fixed::DroppedConstruct(dropped_tag(first.name).unwrap_or(anchor), at)
                    }
                },
            );
        }
        for tag in tags {
            match CarrierTag::of(&tag, true, at).filter(|_| !swallows) {
                Some(carrier) => self.held.push_back(Fixed::Carrier(carrier)),
                None => self.drop_tag(&tag, at),
            }
        }
        self.held
            .push_back(Fixed::Event(Event::End(TagEnd::HtmlBlock)));
    }

    /// One event as the builder takes it, or `None` for one that `held` now
    /// carries or that drops.
    fn fix(&mut self, event: Event<'a>, range: Range<usize>) -> Option<Fixed<'a>> {
        let opens = matches!(
            event,
            Event::Start(Tag::Paragraph | Tag::Item) | Event::TaskListMarker(_)
        );
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
            Event::Start(Tag::Table(aligns)) if aligns.len() > self.room() => {
                self.skip_through(TagEnd::Table);
                Fixed::DroppedConstruct(DroppedConstruct::TableSize, range.start)
            }
            Event::Start(Tag::Table(aligns)) => {
                self.grid = Some((aligns.len(), aligns.len(), false));
                Fixed::Event(Event::Start(Tag::Table(aligns)))
            }
            Event::Start(Tag::TableRow)
                if self.grid.is_some_and(|(cols, n, _)| n + cols > self.room()) =>
            {
                self.skip_through(TagEnd::TableRow);
                let (_, _, cut) = self.grid.as_mut()?;
                return (!std::mem::replace(cut, true)).then_some(Fixed::DroppedConstruct(
                    DroppedConstruct::TableSize,
                    range.start,
                ));
            }
            Event::Start(Tag::TableRow) => {
                if let Some((cols, n, _)) = self.grid.as_mut() {
                    *n += *cols;
                }
                Fixed::Event(event)
            }
            Event::End(TagEnd::Table) => {
                if let Some((_, n, _)) = self.grid.take() {
                    self.cells -= n;
                }
                Fixed::Event(event)
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
                if carrier::wrapper(tag.name).as_deref() == Some(carrier::SPAN) {
                    return Some(Fixed::Span(self.span_tag(&tag, range.start)));
                }
                if let Some(carrier) = CarrierTag::of(&tag, false, range.start) {
                    return Some(Fixed::Carrier(carrier));
                }
                if let Some(element) =
                    carrier::element(tag.name).filter(|e| carrier::RESERVED.contains(&e.as_str()))
                {
                    return (!tag.closing).then(|| {
                        Fixed::Held(HeldTag {
                            element,
                            attrs: carrier::decode_attrs(&tag.attrs),
                            at: range.start,
                        })
                    });
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
            Event::InlineMath(tex) => Fixed::Math {
                tex,
                display: false,
                source: &self.src[range.clone()],
                at: range.start,
            },
            Event::DisplayMath(tex) => Fixed::Math {
                tex,
                display: true,
                source: &self.src[range.clone()],
                at: range.start,
            },
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
pub(crate) mod tests;
