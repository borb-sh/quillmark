//! Markdown import (cold): `normalize → pulldown → content`.
//!
//! Input is normalized by `normalize::normalize_markdown` (CRLF→LF, bidi
//! controls dropped, line separators spaced, then the parser-guided repair that
//! frees the markdown inside an HTML block and makes a footnote-shaped
//! definition literal) so the content invariants hold by construction, then
//! parsed with `pulldown_cmark` (CommonMark + strikethrough + pipe tables) and
//! walked into a [`Content`]. This is the one place the `<u>`/`<br>` allowlist
//! runs, and the one place a dropped raw tag is counted into the
//! [`ImportWarning`]s an import returns beside its content.
//!
//! ## Canonicalizations
//!
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
//! - Raw HTML produces no content beyond the allowlist and the carrier. A tag
//!   alone on its line is a block wrapper: it drops, and the lines around it
//!   parse as markdown. A type 1–5 HTML block (`<pre>`, a comment, …) drops
//!   whole. A `quill-table` wrapper around one table folds its attributes into
//!   the table's props, and a `quill-cell` pair around a whole table cell into
//!   the cell.
//! - A `quill-*` element the carrier does not reserve is modeled: a pair of tag
//!   lines wraps the blocks between them in a [`Container::Element`], and a pair
//!   inside one inline run marks the text between with a [`MarkKind::Element`].
//!   One left unclosed drops, what it wraps importing as written.

use crate::model::{
    Container, Island, Line, LineKind, Mark, MarkKind, Content, Normalized, ISLAND_SLOT,
};
use crate::carrier;
use crate::html;
use crate::island::IslandType;
use crate::normalize::{normalize_markdown, Repaired};
use crate::MAX_NESTING_DEPTH;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::VecDeque;
use std::ops::Range;

/// What `event` contributes to the alt text of an image being collected: one
/// rule shared by the top-level `String` accumulator and the table-cell one.
fn image_alt_text<'e>(event: &'e Event<'e>) -> Option<&'e str> {
    match event {
        Event::Text(t) | Event::Code(t) => Some(t),
        Event::SoftBreak | Event::HardBreak => Some(" "),
        _ => None,
    }
}
use serde_json::json;

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

/// Something the markdown spelled that the content has no place for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportWarning {
    /// `count` instances of `construct` dropped: a raw tag by its lowercase
    /// name (`span`, `div`, `x-note`), counted at its open or self-closing
    /// form; `footnote_definition` for a footnote-shaped definition imported
    /// as literal text; `quill-table` for a `quill-table` wrapper not holding
    /// exactly one table, and `quill-table[<attr>]` for an attribute one
    /// holding a table cannot fold; `quill-cell` for a `quill-cell` tag in a
    /// table cell it does not wrap whole, and `quill-cell[<attr>]` for an
    /// attribute one wrapping its cell cannot fold; `quill-<name>` for an
    /// element left unclosed, self-closing, or paired inline around nothing,
    /// and `quill-<name>[<attr>]` for an attribute one cannot carry. One entry
    /// per construct.
    DroppedConstruct { construct: String, count: usize },
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
    options
}

/// Import markdown into a normalized, validated [`Content`], with an
/// [`ImportWarning`] per construct it dropped. A comment, the content of a type
/// 1–5 HTML block and a `quill-anchor` tag drop silently; a `<pre>`,
/// `<script>`, `<style>` or `<textarea>` block's opening tag counts, and a
/// closing tag never does.
pub fn from_markdown(markdown: &str) -> Result<Imported, ImportError> {
    let options = options();
    let Repaired { text, footnotes, tags } = normalize_markdown(markdown, options);
    let mut fixer = MarkdownFixer::new(Parser::new_ext(&text, options).into_offset_iter());
    let mut b = Builder::new();
    b.run(&mut fixer)?;
    let (mut content, elements) = b.finish();
    let (mut dropped, folds) = fixer.finish();
    dropped.absorb(elements);
    for at in footnotes {
        dropped.add("footnote_definition", at);
    }
    for (at, name) in tags {
        dropped.opening(&name, at);
    }
    let mut tables: Vec<&mut Island> = content
        .islands
        .iter_mut()
        .filter(|i| i.island_type == IslandType::Table)
        .collect();
    for Fold { table, at, keys } in folds {
        let into = tables.get_mut(table).and_then(|t| t.props.pointer_mut(&at));
        if let Some(o) = into.and_then(serde_json::Value::as_object_mut) {
            o.extend(keys);
        }
    }
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
    /// Each element whose open tag this run has read and whose close it awaits,
    /// its `from` the position its mark starts at.
    elements: Vec<Opened>,
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

    fn open_element(&mut self, name: String, attrs: carrier::Attrs, at: usize) {
        self.elements.push(Opened::new(name, attrs, at, self.pos as u64));
    }

    /// Close the innermost open element named `name`, wherever it sits among
    /// the marks: an element mark crosses any other. A close naming none drops
    /// silently, and a pair around nothing drops as its element.
    fn close_element(&mut self, name: &str, dropped: &mut Dropped) {
        let Some(i) = self.elements.iter().rposition(|e| e.name == name) else {
            return;
        };
        let e = self.elements.remove(i);
        let start = e.from as usize;
        if start == self.pos {
            return dropped.add(&format!("{}{}", carrier::PREFIX, e.name), e.at);
        }
        e.report_refused(dropped);
        self.marks.push(Mark {
            start,
            end: self.pos,
            kind: MarkKind::Element {
                name: e.name,
                attrs: e.attrs,
            },
        });
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

    /// End the run: each element and each bare `<u>` still open drops as
    /// unclosed.
    fn drop_open(&mut self, dropped: &mut Dropped) {
        for e in self.elements.drain(..) {
            dropped.add(&format!("{}{}", carrier::PREFIX, e.name), e.at);
        }
        for (_, at) in self.underlines.drain(..).flatten() {
            dropped.add("u", at);
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

/// An element whose open tag the import has read and whose close it awaits.
struct Opened {
    name: String,
    attrs: std::collections::BTreeMap<String, String>,
    /// The attributes outside the carrier grammar, reported once the element
    /// closes: an unclosed element drops whole, and reports only itself.
    refused: Vec<String>,
    /// The open tag's byte offset.
    at: usize,
    /// An inline element's start position, or a block element's instance.
    from: u64,
}

impl Opened {
    fn new(name: String, attrs: carrier::Attrs, at: usize, from: u64) -> Self {
        Opened {
            name,
            attrs: attrs.values,
            refused: attrs.refused,
            at,
            from,
        }
    }

    fn report_refused(&self, dropped: &mut Dropped) {
        for attr in &self.refused {
            dropped.add(&format!("{}{}[{attr}]", carrier::PREFIX, self.name), self.at);
        }
    }
}

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
    island_seq: usize,
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
    // image collection
    image_depth: usize,
    image_url: String,
    image_alt: String,
    // table collection
    table: Option<TableAcc>,
    /// The block elements open, innermost last, each's `from` its instance.
    blocks: Vec<Opened>,
    /// The instances of block elements left unclosed, which
    /// [`Self::finish`] strips from every line's path.
    unclosed: Vec<u64>,
    /// The element tags this walk drops: an unclosed element, an inline pair
    /// around nothing, an attribute outside the grammar.
    dropped: Dropped,
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
    /// Open-image nesting inside the current cell. GFM permits inline images in
    /// cells, but a cell has no island slot to carry one; while `> 0` the
    /// image's alt flows into the cell as plain text and its url is dropped.
    img_depth: usize,
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
    fn new() -> Self {
        Builder {
            inline: Inline::default(),
            lines: Vec::new(),
            cur: None,
            pending: None,
            islands: Vec::new(),
            island_seq: 0,
            containers: Vec::new(),
            container_marks: Vec::new(),
            list_stack: Vec::new(),
            next_instance: 0,
            code_lang: None,
            in_code: false,
            code_opened: false,
            image_depth: 0,
            image_url: String::new(),
            image_alt: String::new(),
            table: None,
            blocks: Vec::new(),
            unclosed: Vec::new(),
            dropped: Dropped::default(),
        }
    }

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

    fn close_mark(&mut self) {
        self.inline.close_mark();
    }

    /// Minting `isl-{seq}` by position keeps import a pure function.
    fn mint_island(&mut self, kind: IslandType, props: serde_json::Value) {
        let id = format!("isl-{}", self.island_seq);
        self.island_seq += 1;
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
            + self.inline.elements.len()
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
                Fixed::Element(tag) => {
                    self.element_tag(tag)?;
                    continue;
                }
                Fixed::Underline(tag) => {
                    self.underline_tag(tag)?;
                    continue;
                }
            };
            // An inline run ends at the first event outside it, and the
            // elements and underlines it left open with it.
            if self.image_depth == 0 && self.table.is_none() && !crate::normalize::is_inline(&event) {
                self.inline.drop_open(&mut self.dropped);
            }
            // Image alt collection intercepts everything until the image closes.
            if self.image_depth > 0 {
                match &event {
                    Event::Start(Tag::Image { .. }) => self.image_depth += 1,
                    Event::End(TagEnd::Image) => {
                        self.image_depth -= 1;
                        if self.image_depth == 0 {
                            self.emit_image();
                        }
                    }
                    other => {
                        if let Some(s) = image_alt_text(other) {
                            self.image_alt.push_str(s);
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
                Event::SoftBreak => self.push_inline(" "),
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
                // Html/InlineHtml already stripped or rewritten by the fixer;
                // math/footnotes/etc. produce no content.
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
                self.pending = Some((
                    LineKind::Heading {
                        level: heading_level(level),
                    },
                    false,
                ))
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
                            instance: info.instance,
                        }
                    }
                    None => Container::ListItem {
                        ordered: false,
                        start: 1,
                        ordinal: 0,
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
                    header: Vec::new(),
                    rows: Vec::new(),
                    cur_row: Vec::new(),
                    in_head: false,
                    cell: None,
                    img_depth: 0,
                });
            }
            Tag::Emphasis => {
                self.open_mark(MarkKind::Emph);
                self.check_depth()?;
            }
            Tag::Strong => {
                self.open_mark(MarkKind::Strong);
                self.check_depth()?;
            }
            Tag::Strikethrough => {
                self.open_mark(MarkKind::Strike);
                self.check_depth()?;
            }
            Tag::Link { dest_url, .. } => {
                self.open_mark(MarkKind::Link {
                    url: dest_url.to_string(),
                });
                self.check_depth()?;
            }
            Tag::Image { dest_url, .. } => {
                self.image_url = dest_url.to_string();
                self.image_alt.clear();
                self.image_depth = 1;
            }
            _ => {}
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
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.close_mark()
            }
            // A block that produced no inline content still gets its line.
            TagEnd::Heading(_) | TagEnd::Paragraph => {
                self.flush_empty_block();
                self.rearm_item();
            }
            TagEnd::HtmlBlock => self.rearm_item(),
            _ => {}
        }
    }

    /// Pair one element tag: a tag line opens or closes a block element, any
    /// other an inline one in the run or table cell it stands in. In an
    /// image's alt text, where nothing is marked, an open tag drops.
    fn element_tag(&mut self, tag: ElementTag) -> Result<(), ImportError> {
        let ElementTag { name, attrs, block, at } = tag;
        let report = |dropped: &mut Dropped, attrs: &Option<carrier::Attrs>| {
            if attrs.is_some() {
                dropped.add(&format!("{}{name}", carrier::PREFIX), at);
            }
        };
        if self.image_depth > 0 {
            report(&mut self.dropped, &attrs);
            return Ok(());
        }
        if let Some(acc) = self.table.as_mut() {
            match acc.cell.as_mut().filter(|_| acc.img_depth == 0) {
                Some(cell) => match attrs {
                    Some(attrs) => cell.open_element(name, attrs, at),
                    None => cell.close_element(&name, &mut self.dropped),
                },
                None => report(&mut self.dropped, &attrs),
            }
            return Ok(());
        }
        match (attrs, block) {
            (Some(attrs), true) => {
                let instance = self.mint_instance();
                self.container_marks.push(self.emitted());
                self.containers.push(Container::Element {
                    name: name.clone(),
                    attrs: attrs.values.clone(),
                    instance,
                });
                self.blocks.push(Opened::new(name, attrs, at, instance));
                self.check_depth()?;
            }
            (None, true) => {
                let innermost = matches!(
                    self.containers.last(),
                    Some(Container::Element { name: open, .. }) if *open == name
                );
                if innermost {
                    let mark = self.container_marks.pop().unwrap_or(0);
                    self.close_container(mark);
                    if let Some(e) = self.blocks.pop() {
                        e.report_refused(&mut self.dropped);
                    }
                }
            }
            (Some(attrs), false) => {
                // Resolve an armed line first, as a mark does, so the element
                // starts after the line boundary.
                self.ensure_open(LineKind::Para);
                self.inline.open_element(name, attrs, at);
                self.check_depth()?;
            }
            (None, false) => self.inline.close_element(&name, &mut self.dropped),
        }
        Ok(())
    }

    /// Pair one `<u>` tag in the run or table cell it stands in. In an image's
    /// alt text, where nothing is marked, it underlines nothing.
    fn underline_tag(&mut self, tag: UTag) -> Result<(), ImportError> {
        if self.image_depth > 0 {
            return Ok(());
        }
        if let Some(acc) = self.table.as_mut() {
            if let Some(cell) = acc.cell.as_mut().filter(|_| acc.img_depth == 0) {
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

    /// Drop each block element still open where its list item, quote or the
    /// body ends: its tags drop and what it wraps stays, [`Self::finish`]
    /// taking it off every line's path.
    fn drop_unclosed_blocks(&mut self) {
        while matches!(self.containers.last(), Some(Container::Element { .. })) {
            self.containers.pop();
            self.container_marks.pop();
            if let Some(e) = self.blocks.pop() {
                self.unclosed.push(e.from);
                self.dropped.add(&format!("{}{}", carrier::PREFIX, e.name), e.at);
            }
        }
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
        // An image open inside the current cell intercepts everything until it
        // closes: the alt lands as plain text and the url is dropped, a cell
        // having no slot to carry an image.
        if acc.img_depth > 0 {
            match event {
                Event::Start(Tag::Image { .. }) => acc.img_depth += 1,
                Event::End(TagEnd::Image) => acc.img_depth -= 1,
                other => {
                    if let Some(s) = image_alt_text(other) {
                        if let Some(c) = acc.cell.as_mut() {
                            c.push_text(s);
                        }
                    }
                }
            }
            return;
        }
        match event {
            Event::Start(Tag::Image { .. }) => acc.img_depth += 1,
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
                    // Close any marks pulldown left open (malformed input).
                    while !cell.open.is_empty() {
                        cell.close_mark();
                    }
                    cell.drop_open(&mut self.dropped);
                    acc.cur_row
                        .push(crate::serial::cell_to_value(&cell.text, &cell.marks));
                }
            }
            // Inline content of the open cell. A hard break is a `\n` in the
            // cell's text, as in prose.
            Event::Text(t) => {
                if let Some(c) = acc.cell.as_mut() {
                    c.push_text(t);
                }
            }
            Event::Code(t) => {
                if let Some(c) = acc.cell.as_mut() {
                    c.push_code(t);
                }
            }
            Event::SoftBreak => {
                if let Some(c) = acc.cell.as_mut() {
                    c.push_text(" ");
                }
            }
            Event::HardBreak => {
                if let Some(c) = acc.cell.as_mut() {
                    c.push_raw('\n');
                }
            }
            Event::Start(Tag::Emphasis) => {
                if let Some(c) = acc.cell.as_mut() {
                    c.open_mark(MarkKind::Emph);
                }
            }
            Event::Start(Tag::Strong) => {
                if let Some(c) = acc.cell.as_mut() {
                    c.open_mark(MarkKind::Strong);
                }
            }
            Event::Start(Tag::Strikethrough) => {
                if let Some(c) = acc.cell.as_mut() {
                    c.open_mark(MarkKind::Strike);
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if let Some(c) = acc.cell.as_mut() {
                    c.open_mark(MarkKind::Link {
                        url: dest_url.to_string(),
                    });
                }
            }
            Event::End(TagEnd::Emphasis)
            | Event::End(TagEnd::Strong)
            | Event::End(TagEnd::Strikethrough)
            | Event::End(TagEnd::Link) => {
                if let Some(c) = acc.cell.as_mut() {
                    c.close_mark();
                }
            }
            _ => {}
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

    fn finish(mut self) -> (Content, Dropped) {
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
        // Close any marks left open (malformed input).
        while !self.inline.open.is_empty() {
            self.close_mark();
        }
        let content = Content {
            text: self.inline.text,
            lines: self.lines,
            marks: self.inline.marks,
            islands: self.islands,
        };
        (content, self.dropped)
    }
}

fn heading_level(level: pulldown_cmark::HeadingLevel) -> u8 {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => 1,
        H2 => 2,
        H3 => 3,
        H4 => 4,
        H5 => 5,
        H6 => 6,
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
// passes each inline `<u>` and `</u>` and each element tag for the builder to
// pair, allowlists an inline `<br>` as a hard break, and drops every other raw
// HTML event, an HTML block whole. It counts each tag it drops, so the warnings and
// the drop cannot disagree, and reads the `quill-table` wrappers and
// `quill-cell` pairs whose attributes `from_markdown` folds.
// Delimiter arithmetic stays pulldown's, since a fixer that re-segments `***`
// runs can only disagree with CommonMark, and disagreeing means deleting an
// asterisk the author typed.

/// What the fixer hands the builder.
enum Fixed<'a> {
    Event(Event<'a>),
    Element(ElementTag),
    Underline(UTag),
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

/// An open or close tag of an element the import models
/// ([`carrier::modeled_tag`]).
struct ElementTag {
    name: String,
    /// The open tag's attributes; `None` for a close tag.
    attrs: Option<carrier::Attrs>,
    /// Whether it stands on a tag line, in an HTML block, rather than inline.
    block: bool,
    /// The tag's byte offset, or its HTML block's.
    at: usize,
}

impl ElementTag {
    fn of(tag: &html::Tag, block: bool, at: usize) -> Option<Self> {
        Some(ElementTag {
            name: carrier::modeled_tag(tag)?,
            attrs: (!tag.closing).then(|| carrier::decode_attrs(&tag.attrs)),
            block,
            at,
        })
    }
}

/// The `quill-cell` tag `event` is, if it is one.
fn cell_tag<'e>(event: &'e Event) -> Option<html::Tag<'e>> {
    match event {
        Event::InlineHtml(h) => html::tag_at(h, 0).filter(|t| carrier::element(t.name).as_deref() == Some("cell")),
        _ => None,
    }
}


/// Per construct: its count and the byte offset of its first occurrence.
#[derive(Default)]
struct Dropped(Vec<(String, usize, usize)>);

impl Dropped {
    fn add(&mut self, construct: &str, at: usize) {
        match self.0.iter_mut().find(|(c, ..)| c == construct) {
            Some((_, count, first)) => {
                *count += 1;
                *first = (*first).min(at);
            }
            None => self.0.push((construct.to_string(), 1, at)),
        }
    }

    /// An opening tag, under its lowercase name. A carrier element nothing
    /// folds counts like any tag; `quill-anchor`, the engine's own read-only
    /// spelling of an anchor, is written to be dropped and counts nothing.
    fn tag(&mut self, tag: &html::Tag, at: usize) {
        if !tag.closing {
            self.opening(tag.name, at);
        }
    }

    fn opening(&mut self, name: &str, at: usize) {
        if carrier::element(name).is_some_and(|e| e == "anchor") {
            return;
        }
        self.add(&name.to_ascii_lowercase(), at);
    }

    /// Fold in another walk's drops, each at its own first offset.
    fn absorb(&mut self, other: Dropped) {
        for (construct, count, at) in other.0 {
            match self.0.iter_mut().find(|(c, ..)| *c == construct) {
                Some((_, n, first)) => {
                    *n += count;
                    *first = (*first).min(at);
                }
                None => self.0.push((construct, count, at)),
            }
        }
    }

    fn into_warnings(mut self) -> Vec<ImportWarning> {
        self.0.sort_by_key(|&(_, _, first)| first);
        self.0
            .into_iter()
            .map(|(construct, count, _)| ImportWarning::DroppedConstruct { construct, count })
            .collect()
    }
}

struct MarkdownFixer<'a, I> {
    inner: I,
    dropped: Dropped,
    /// The `quill-table` block wrappers open at this point, innermost last.
    wrappers: Vec<TableWrapper>,
    tables: usize,
    in_table: bool,
    /// The open table row's pointer into its table's props, `/header` or
    /// `/rows/<n>`, and the cells it has opened.
    row: (String, usize),
    body_rows: usize,
    folds: Vec<Fold>,
    /// A table cell's events, read through its end and passed on once its
    /// fold is decided, and an HTML block's element tags ahead of its end.
    held: VecDeque<Fixed<'a>>,
}

/// The keys a carrier element folds into the table island of ordinal `table`:
/// into its props, or into one of its cells, by `at`, a JSON pointer into the
/// props.
struct Fold {
    table: usize,
    at: String,
    keys: serde_json::Map<String, serde_json::Value>,
}

/// A `quill-table` block wrapper whose open tag the fixer has read: it folds
/// its attributes into the table island it holds when it holds exactly one
/// table and nothing else.
struct TableWrapper {
    attrs: carrier::Attrs,
    at: usize,
    /// The ordinal among the document's tables of each table it holds.
    tables: Vec<usize>,
    holds_other: bool,
}

impl<'a, I> MarkdownFixer<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    fn new(inner: I) -> Self {
        Self {
            inner,
            dropped: Dropped::default(),
            wrappers: Vec::new(),
            tables: 0,
            in_table: false,
            row: (String::new(), 0),
            body_rows: 0,
            folds: Vec::new(),
            held: VecDeque::new(),
        }
    }

    /// The drops and the folds. A wrapper left open folds nothing.
    fn finish(mut self) -> (Dropped, Vec<Fold>) {
        for w in std::mem::take(&mut self.wrappers) {
            self.dropped.add("quill-table", w.at);
        }
        (self.dropped, self.folds)
    }

    /// Record what a block event passed to the builder puts inside the
    /// innermost open wrapper.
    fn observe(&mut self, event: &Event) {
        match event {
            Event::Start(Tag::Table(_)) => {
                if let Some(w) = self.wrappers.last_mut() {
                    w.tables.push(self.tables);
                }
                self.tables += 1;
                self.in_table = true;
                self.body_rows = 0;
            }
            Event::Start(Tag::TableHead) => self.row = ("/header".into(), 0),
            Event::Start(Tag::TableRow) => {
                self.row = (format!("/rows/{}", self.body_rows), 0);
                self.body_rows += 1;
            }
            Event::End(TagEnd::Table) => self.in_table = false,
            // The repair writes a tag line after a list item into the item, so
            // a wrapper's open tag can sit in a container its table follows.
            Event::End(_) => {}
            _ if self.in_table => {}
            _ => {
                if let Some(w) = self.wrappers.last_mut() {
                    w.holds_other = true;
                }
            }
        }
    }

    /// A wrapper holding anything but one table drops whole as `quill-table`;
    /// otherwise each attribute the engine does not name or cannot read drops
    /// as `quill-table[<name>]`, and the rest fold.
    fn close_wrapper(&mut self, w: TableWrapper) {
        let [table] = w.tables[..] else {
            return self.dropped.add("quill-table", w.at);
        };
        if w.holds_other {
            return self.dropped.add("quill-table", w.at);
        }
        let keys = self.fold_attrs("quill-table", w.attrs, w.at, carrier::table::prop);
        self.folds.push(Fold { table, at: String::new(), keys });
    }

    /// The keys `attrs` fold through `read`, each attribute refused or read as
    /// `None` dropping alone as `<construct>[<name>]`.
    fn fold_attrs(
        &mut self,
        construct: &str,
        attrs: carrier::Attrs,
        at: usize,
        read: fn(&str, &str) -> Option<serde_json::Value>,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut keys = serde_json::Map::new();
        for name in &attrs.refused {
            self.dropped.add(&format!("{construct}[{name}]"), at);
        }
        for (name, value) in attrs.values {
            match read(&name, &value) {
                Some(v) => {
                    keys.insert(name, v);
                }
                None => self.dropped.add(&format!("{construct}[{name}]"), at),
            }
        }
        keys
    }

    /// Read the table cell just opened through its end into `held`. A
    /// `quill-cell` pair whose open tag is the cell's first event and whose
    /// close tag its last, with no other `quill-cell` tag between, folds its
    /// attributes into the cell and its tags pass on nothing; otherwise every
    /// tag drops as it does anywhere.
    fn read_cell(&mut self) {
        let at = format!("{}/{}", self.row.0, self.row.1);
        self.row.1 += 1;
        let mut events = Vec::new();
        let mut end = None;
        for (event, range) in self.inner.by_ref() {
            if matches!(event, Event::End(TagEnd::TableCell)) {
                end = Some(event);
                break;
            }
            events.push((event, range));
        }
        let tags: Vec<usize> = (0..events.len()).filter(|&i| cell_tag(&events[i].0).is_some()).collect();
        let pair = match tags[..] {
            [0, last] if last + 1 == events.len() => {
                let open = cell_tag(&events[0].0).filter(|t| !t.closing && !t.self_closing);
                let close = cell_tag(&events[last].0).filter(|t| t.closing);
                open.zip(close).map(|(open, _)| (carrier::decode_attrs(&open.attrs), events[0].1.start))
            }
            _ => None,
        };
        let inner = match pair {
            Some((attrs, start)) => {
                let keys = self.fold_attrs("quill-cell", attrs, start, carrier::cell::key);
                self.folds.push(Fold { table: self.tables - 1, at, keys });
                events.pop();
                events.remove(0);
                events
            }
            None => events,
        };
        for (event, range) in inner {
            if let Some(item) = self.fix(event, range) {
                self.held.push_back(item);
            }
        }
        if let Some(end) = end {
            self.held.push_back(Fixed::Event(end));
        }
    }

    /// Consume an HTML block through its end, counting its
    /// [markup tags](html::block_tags). Only its element tags and the block's
    /// end reach the builder, queued in `held`.
    fn drop_html_block(&mut self, at: usize) {
        let mut text = String::new();
        for (event, _) in self.inner.by_ref() {
            match event {
                Event::Html(h) => text.push_str(&h),
                Event::End(TagEnd::HtmlBlock) => break,
                _ => {}
            }
        }
        for tag in html::block_tags(&text) {
            if let Some(element) = ElementTag::of(&tag, true, at) {
                self.held.push_back(Fixed::Element(element));
            } else if tag.self_closing || carrier::element(tag.name).as_deref() != Some("table") {
                self.dropped.tag(&tag, at);
            } else if tag.closing {
                if let Some(w) = self.wrappers.pop() {
                    self.close_wrapper(w);
                }
            } else {
                if let Some(outer) = self.wrappers.last_mut() {
                    outer.holds_other = true;
                }
                self.wrappers.push(TableWrapper {
                    attrs: carrier::decode_attrs(&tag.attrs),
                    at,
                    tables: Vec::new(),
                    holds_other: false,
                });
            }
        }
        self.held.push_back(Fixed::Event(Event::End(TagEnd::HtmlBlock)));
    }

    /// One event as the builder takes it, or `None` for one that drops or
    /// that `held` now carries.
    fn fix(&mut self, event: Event<'a>, range: Range<usize>) -> Option<Fixed<'a>> {
        Some(match event {
            Event::Start(Tag::HtmlBlock) => {
                self.drop_html_block(range.start);
                return None;
            }
            Event::InlineHtml(html) => {
                let tag = html::tag_at(&html, 0)?;
                if tag.name.eq_ignore_ascii_case("u") {
                    return Some(Fixed::Underline(if tag.closing {
                        UTag::Close
                    } else if tag.attrs.is_empty() && !tag.self_closing {
                        UTag::Open { at: range.start }
                    } else {
                        self.dropped.tag(&tag, range.start);
                        UTag::Held
                    }));
                }
                if tag.name.eq_ignore_ascii_case("br") && !tag.closing {
                    return Some(Fixed::Event(Event::HardBreak));
                }
                if let Some(element) = ElementTag::of(&tag, false, range.start) {
                    return Some(Fixed::Element(element));
                }
                self.dropped.tag(&tag, range.start);
                return None;
            }
            other => {
                self.observe(&other);
                Fixed::Event(other)
            }
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
            let (event, range) = self.inner.next()?;
            if matches!(event, Event::Start(Tag::TableCell)) {
                self.read_cell();
                return Some(Fixed::Event(event));
            }
            if let Some(item) = self.fix(event, range) {
                return Some(item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
                instance: 0,
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 1,
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
                instance: 0,
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
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
    fn a_cell_image_lands_as_its_alt_text() {
        // A cell has no island slot, so the alt lands as plain text and the url
        // is dropped.
        let rt = imp("| a | b |\n|---|---|\n| ![a cat](cat.png) | 2 |");
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, IslandType::Table);
        assert_eq!(rt.islands[0].props["rows"][0][0]["text"], "a cat");
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

    fn dropped(imported: &Imported) -> Vec<(&str, usize)> {
        imported
            .warnings
            .iter()
            .map(|ImportWarning::DroppedConstruct { construct, count }| (construct.as_str(), *count))
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

    #[test]
    fn a_tag_wrapping_markdown_drops_while_the_markdown_imports() {
        for md in [
            "<div align=\"center\">\n| a | b |\n|---|---|\n| 1 | 2 |\n</div>",
            "<x-keep>\n| a | b |\n|---|---|\n| 1 | 2 |\n</x-keep>",
            "<div align=\"center\">\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n</div>",
        ] {
            let rt = imp_fixed(md).content;
            assert_eq!(rt.text, "\u{FFFC}", "{md:?}");
            assert_eq!(table_rows(&rt), [["1", "2"]], "{md:?}");
        }

        let imported = imp_fixed("<center>\n**Signed**\nJ. Doe\n</center>");
        assert_eq!(imported.content.text, "Signed J. Doe");
        assert_eq!(imported.content.marks, [Mark::new(0, 6, MarkKind::Strong)]);
        assert_eq!(dropped(&imported), [("center", 1)]);
    }

    fn layout(rt: &Normalized) -> serde_json::Value {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        let keys = ["widths", "align", "breakable"];
        let props = island.props.as_object().unwrap();
        let kept = props.iter().filter(|(k, _)| keys.contains(&k.as_str()));
        serde_json::Value::Object(kept.map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    #[test]
    fn a_table_wrapper_folds_its_attributes_into_the_table_it_holds() {
        let table = "| a | b | c |\n|---|---|---|\n| 1 | 2 | 3 |";
        let canonical = "<quill-table align=\"center\" breakable=\"false\" widths=\"1 3 auto\">\n\n\
                         | a | b | c |\n| --- | --- | --- |\n| 1 | 2 | 3 |\n\n</quill-table>";
        let expected = serde_json::json!({"align": "center", "breakable": false, "widths": [1, 3, null]});
        let cases = [
            (format!("<quill-table widths=\"2 6\" align=center BREAKABLE='false'>\n{table}\n</quill-table>"), 0),
            (format!("<Quill-Table widths=\" 2  6 auto \" breakable=\"false\" align=\"center\">\n\n{table}\n\n</Quill-Table>\n\nafter"), 0),
            (format!("- item\n- <quill-table widths=\"2 6\" align=\"center\" breakable=\"false\">\n\n  {}\n\n  </quill-table>", table.replace('\n', "\n  ")), 1),
            (format!("- item\n\n<quill-table widths=\"2 6\" align=\"center\" breakable=\"false\">\n\n{table}\n\n</quill-table>"), 0),
            (format!("> <quill-table widths=\"2 6 auto auto\" align=\"center\" breakable=\"false\">\n> {}\n> </quill-table>", table.replace('\n', "\n> ")), 1),
        ];
        for (md, containers) in &cases {
            let imported = imp_fixed(md);
            assert_eq!(dropped(&imported), [], "{md:?}");
            let rt = &imported.content;
            let props = &rt.islands.iter().find(|i| i.island_type == IslandType::Table).unwrap().props;
            for key in ["widths", "align", "breakable"] {
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

        let defaults = imp_fixed(&format!("<quill-table widths=\"auto auto\" breakable=\"true\">\n{table}\n</quill-table>"));
        assert_eq!(dropped(&defaults), []);
        assert_eq!(layout(&defaults.content), serde_json::json!({}));
        assert_eq!(crate::export::to_markdown(&defaults.content), from_markdown(table).map(|i| crate::export::to_markdown(&i.content)).unwrap());
    }

    #[test]
    fn a_table_wrapper_holding_anything_but_one_table_drops_whole() {
        let t = "| a |\n|---|\n| 1 |";
        for md in [
            "<quill-table align=\"center\">\n\npara\n\n</quill-table>".to_string(),
            format!("<quill-table align=\"center\">\n\n{t}\n\n{t}\n\n</quill-table>"),
            format!("<quill-table align=\"center\">\n\n{t}\n\npara\n\n</quill-table>"),
            format!("<quill-table align=\"center\">\n\n- {}\n\n</quill-table>", t.replace('\n', "\n  ")),
            format!("<quill-table align=\"center\">\n\n{t}"),
            "<quill-table align=\"center\">\n\n</quill-table>".to_string(),
            format!("<quill-table align=\"center\"/>\n\n{t}"),
        ] {
            let imported = imp_fixed(&md);
            assert_eq!(dropped(&imported), [("quill-table", 1)], "{md:?}");
            assert!(imported.content.islands.iter().all(|i| i.props.get("align").is_none()), "{md:?}");
            assert_eq!(imported.content, imp_fixed(&crate::carrier::strip(&md)).content, "{md:?}");
        }

        let nested = imp_fixed(&format!("<quill-table align=\"left\">\n\n<quill-table align=\"right\">\n\n{t}\n\n</quill-table>\n\n</quill-table>"));
        assert_eq!(dropped(&nested), [("quill-table", 1)]);
        assert_eq!(layout(&nested.content), serde_json::json!({"align": "right"}));
    }

    #[test]
    fn a_table_wrapper_drops_each_attribute_it_cannot_read() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |";
        let cases: &[(&str, &[(&str, usize)], serde_json::Value)] = &[
            ("foo=\"1\" align=\"left\"", &[("quill-table[foo]", 1)], serde_json::json!({"align": "left"})),
            ("style=\"x\" onclick=\"y\" breakable=\"false\"", &[("quill-table[onclick]", 1), ("quill-table[style]", 1)], serde_json::json!({"breakable": false})),
            ("widths=\"a b\" align=\"middle\" breakable=\"no\"", &[("quill-table[align]", 1), ("quill-table[breakable]", 1), ("quill-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"0 1\"", &[("quill-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"+1 2\"", &[("quill-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"1 null\"", &[("quill-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"1 *\"", &[("quill-table[widths]", 1)], serde_json::json!({})),
            ("widths=\"1.5 2\"", &[("quill-table[widths]", 1)], serde_json::json!({})),
            ("breakable", &[("quill-table[breakable]", 1)], serde_json::json!({})),
            ("align=\"left\" align=\"right\"", &[("quill-table[align]", 1)], serde_json::json!({"align": "left"})),
        ];
        for (attrs, warned, kept) in cases {
            let md = format!("<quill-table {attrs}>\n\n{t}\n\n</quill-table>");
            let imported = imp_fixed(&md);
            let mut got = dropped(&imported);
            got.sort();
            assert_eq!(got, *warned, "{md:?}");
            assert_eq!(layout(&imported.content), *kept, "{md:?}");
        }
    }

    /// A cell's `align` and `valign`, by its JSON pointer into the one table's
    /// props.
    fn cell_keys(rt: &Normalized, at: &str) -> serde_json::Value {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        let cell = island.props.pointer(at).unwrap().as_object().unwrap();
        let kept = cell.iter().filter(|(k, _)| ["align", "valign"].contains(&k.as_str()));
        serde_json::Value::Object(kept.map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    #[test]
    fn a_cell_pair_around_its_whole_cell_folds_its_attributes_into_the_cell() {
        let md = "| <quill-cell valign=\"bottom\">h</quill-cell> | b |\n| --- | ---: |\n\
                  | <Quill-Cell VALIGN=horizon align='center'>**1** <u>x</u></Quill-Cell> \
                  | <quill-cell align=\"left\"> 2 </quill-cell> |";
        let imported = imp_fixed(md);
        assert_eq!(dropped(&imported), []);
        let rt = &imported.content;
        assert_eq!(cell_keys(rt, "/header/0"), serde_json::json!({"valign": "bottom"}));
        assert_eq!(cell_keys(rt, "/header/1"), serde_json::json!({}));
        assert_eq!(cell_keys(rt, "/rows/0/0"), serde_json::json!({"align": "center", "valign": "horizon"}));
        assert_eq!(cell_keys(rt, "/rows/0/1"), serde_json::json!({"align": "left"}));
        let (text, marks) = crate::serial::parse_cell(&rt.islands[0].props["rows"][0][0]);
        assert_eq!(text, "1 x");
        assert_eq!(marks, [Mark::new(0, 1, MarkKind::Strong), Mark::new(2, 3, MarkKind::Underline)]);
        assert_eq!(table_rows(rt), [["1 x", " 2 "]]);
        assert_eq!(
            crate::export::to_markdown(rt),
            "| <quill-cell valign=\"bottom\">h</quill-cell> | b |\n| --- | ---: |\n\
             | <quill-cell align=\"center\" valign=\"horizon\">**1** <u>x</u></quill-cell> \
             | <quill-cell align=\"left\">&#32;2&#32;</quill-cell> |"
        );

        let respelled = imp_fixed("| h |\n| --- |\n| <quill-cell align=\"right\">__a**__*b*</quill-cell> |");
        let (text, marks) = crate::serial::parse_cell(&respelled.content.islands[0].props["rows"][0][0]);
        assert_eq!(text, "a**b");
        assert_eq!(marks, [Mark::new(0, 3, MarkKind::Strong), Mark::new(3, 4, MarkKind::Emph)]);
        assert_eq!(cell_keys(&respelled.content, "/rows/0/0"), serde_json::json!({"align": "right"}));
    }

    #[test]
    fn a_cell_pair_at_its_default_folds_to_nothing() {
        let imported = imp_fixed(
            "| h | i |\n| ---: | --- |\n| <quill-cell align=\"right\" valign=\"top\">1</quill-cell> \
             | <quill-cell>2</quill-cell> |",
        );
        assert_eq!(dropped(&imported), []);
        assert_eq!(cell_keys(&imported.content, "/rows/0/0"), serde_json::json!({}));
        assert_eq!(crate::export::to_markdown(&imported.content), "| h | i |\n| ---: | --- |\n| 1 | 2 |");
    }

    /// The cell imports as its [`strip`](crate::carrier::strip) does, each
    /// `quill-cell` open tag reported.
    #[test]
    fn a_cell_pair_not_wrapping_its_whole_cell_drops() {
        let r = "<quill-cell align=\"right\">";
        let l = "<quill-cell align=\"left\">";
        let cases = [
            (format!("a {r}b</quill-cell>"), "a b", 1),
            (format!("{r}a</quill-cell> b"), "a b", 1),
            (format!("{r}a</quill-cell>{l}b</quill-cell>"), "ab", 2),
            (format!("{r}{l}a</quill-cell></quill-cell>"), "a", 2),
            (format!("{r}a"), "a", 1),
            (format!("{r}a</quill-cell></quill-cell>"), "a", 1),
            ("<quill-cell align=\"right\"/>a".to_string(), "a", 1),
            (format!("**{r}a</quill-cell>**"), "a", 1),
            (format!("<!-- c -->{r}a</quill-cell>"), "a", 1),
        ];
        for (cell, text, count) in &cases {
            let md = format!("| h |\n| --- |\n| {cell} |");
            let imported = imp_fixed(&md);
            assert_eq!(dropped(&imported), [("quill-cell", *count)], "{md:?}");
            assert_eq!(table_rows(&imported.content), [[*text]], "{md:?}");
            assert_eq!(cell_keys(&imported.content, "/rows/0/0"), serde_json::json!({}), "{md:?}");
            assert_eq!(imported.content, imp_fixed(&crate::carrier::strip(&md)).content, "{md:?}");
        }

        let prose = imp_fixed(&format!("a {r}b</quill-cell> c"));
        assert_eq!(dropped(&prose), [("quill-cell", 1)]);
        assert_eq!(prose.content.text, "a b c");
    }

    #[test]
    fn a_cell_pair_drops_each_attribute_it_cannot_read() {
        let cases: &[(&str, &[(&str, usize)], serde_json::Value)] = &[
            ("foo=\"1\" align=\"right\"", &[("quill-cell[foo]", 1)], serde_json::json!({"align": "right"})),
            ("style=\"x\" onclick=\"y\" valign=\"bottom\"", &[("quill-cell[onclick]", 1), ("quill-cell[style]", 1)], serde_json::json!({"valign": "bottom"})),
            ("align=\"middle\" valign=\"center\"", &[("quill-cell[align]", 1), ("quill-cell[valign]", 1)], serde_json::json!({})),
            ("align=\"Right\"", &[("quill-cell[align]", 1)], serde_json::json!({})),
            ("valign", &[("quill-cell[valign]", 1)], serde_json::json!({})),
            ("align=\"left\" align=\"right\"", &[("quill-cell[align]", 1)], serde_json::json!({"align": "left"})),
        ];
        for (attrs, warned, kept) in cases {
            let md = format!("| <quill-cell {attrs}>h</quill-cell> |\n| --- |\n| <quill-cell {attrs}>1</quill-cell> |");
            let imported = imp_fixed(&md);
            let mut got = dropped(&imported);
            got.sort();
            let doubled: Vec<(&str, usize)> = warned.iter().map(|&(c, n)| (c, 2 * n)).collect();
            assert_eq!(got, doubled, "{md:?}");
            assert_eq!(cell_keys(&imported.content, "/header/0"), *kept, "{md:?}");
            assert_eq!(cell_keys(&imported.content, "/rows/0/0"), *kept, "{md:?}");
        }
    }

    /// A type-7 tag cannot interrupt a pipe table, so the parser reads
    /// `</quill-table>` after the rows as one more row; the repair ends the
    /// table there instead. A type-6 tag (`</div>`) interrupts it on its own.
    #[test]
    fn a_row_of_tags_ends_its_table() {
        for close in ["</quill-table>", "</div>", "</quill-cell></quill-table>"] {
            let rt = imp_fixed(&format!("| a | b |\n|---|---|\n| 1 | 2 |\n{close}\nnext")).content;
            assert_eq!(table_rows(&rt), [["1", "2"]], "{close}");
            assert_eq!(rt.text, "\u{FFFC}\nnext", "{close}");
        }
        let rt = imp_fixed("> | a |\n> |---|\n> | 1 |\n> </quill-table>\n> next").content;
        assert_eq!(table_rows(&rt), [["1"]]);
        assert!(rt.lines.iter().all(|l| l.containers.len() == 1), "{:?}", rt.lines);
    }

    /// `<div>` inside a quote inside a list item: the block's lines free inside
    /// both containers, and the item's text after the quote stays the item's.
    #[test]
    fn a_nested_html_block_frees_its_lines_inside_their_containers() {
        let rt = imp_fixed("- item\n  > <div>\n  > **bold** text\n  > </div>\n  after").content;
        assert_eq!(rt.text, "item\nbold text\nafter");
        let depths: Vec<usize> = rt.lines.iter().map(|l| l.containers.len()).collect();
        assert_eq!(depths, [1, 2, 1]);
        assert!(matches!(rt.lines[1].containers[1], Container::Quote { .. }));
        assert_eq!(rt.marks, [Mark::new(5, 9, MarkKind::Strong)]);

        let rt = imp_fixed("<div>\n> <div>\n> inner *em*\n> </div>\n</div>\n\nafter").content;
        assert_eq!(rt.text, "inner em\nafter");
        assert_eq!(rt.lines[0].containers, [Container::Quote { instance: 0 }]);
        assert!(rt.lines[1].containers.is_empty());
    }

    /// A line inside a type 6/7 block that opens a comment, a `<pre>` or a fence
    /// keeps it whole when it closes inside the block; when it does not, it drops
    /// with the rest of the block, its tags counted, rather than swallowing what
    /// follows.
    #[test]
    fn a_freed_line_never_swallows_what_follows_its_block() {
        let cases: &[(&str, &str)] = &[
            ("<div>\n<!-- c\nunclosed\n</div>\n\nmore text", "more text"),
            ("<div>\n```\ncode\n</div>\n\nmore text", "more text"),
            ("<div>\nkept\n<pre>\nunclosed\n</div>\n\nmore text", "kept\nmore text"),
            ("<div>\n<!-- c\nx -->\nkept\n</div>\n\nmore", "kept\nmore"),
            ("<div>\n<pre>\nx\n</pre> tail\n</div>", "tail"),
            ("<div>\n```\n<b>\n```\n</div>", "<b>"),
            ("<div\n  class=\"x\">\ntext\n</div>", "text"),
            ("<div class=\"never closed\ntext\n</div>\n\nafter", "after"),
            ("<div>\n    indented\n</div>", "indented"),
            ("<div>\n- a\n    - b\n</div>", "a\nb"),
            ("<div>\n- <!-- note\n\n  more text", "more text"),
            ("<div>\n> 1. ```\ncode\n</div>\n\nmore text", "more text"),
            ("<div>\n- ```\ncode\n```\n</div>", "code"),
            ("<div>\n> <!-- c\nx --> kept\n</div>", "kept"),
        ];
        for (md, text) in cases {
            assert_eq!(imp_fixed(md).content.text, *text, "{md:?}");
        }
        for md in [
            "<div>\n```\n<b>x</b> <quill-anchor ref=\"r\">\n</div>",
            "<div>\n- <!-- c\n<b>x</b>\n</div>",
        ] {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, "", "{md:?}");
            assert_eq!(dropped(&imported), [("div", 1), ("b", 1)], "{md:?}");
        }
    }

    /// Text after a type 1–5 block's end marker on its last line moves to a
    /// line of its own, and ends there rather than joining the line after.
    /// Mid-line, a comment is inline HTML and splits nothing.
    #[test]
    fn text_after_a_block_end_marker_imports() {
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
            ("<pre>x</pre> tail", "tail"),
            ("<?php x ?> tail", "tail"),
            ("<!DOCTYPE html> tail", "tail"),
            ("<![CDATA[x]]> tail", "tail"),
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

    #[test]
    fn a_fenced_comment_round_trips_byte_equal() {
        let md = "```\n<!-- a --> b\n```";
        let imported = imp_fixed(md);
        assert_eq!(imported.content.text, "<!-- a --> b");
        assert_eq!(crate::export::to_markdown(&imported.content), md);
    }

    /// A line holding only tags is a tag line under paragraph text too, where
    /// CommonMark reads it as inline HTML: it ends the paragraph and opens a
    /// block running to the paragraph's end, inside the paragraph's
    /// containers. A lazy one leaves the quotes it lacks.
    #[test]
    fn a_tag_line_under_paragraph_text_opens_a_block() {
        let cases: &[(&str, &str, &[usize])] = &[
            ("para\n<x-keep>\nmore\n</x-keep>", "para\nmore", &[0, 0]),
            ("text\n<u>\nmore\n</u>", "text\nmore", &[0, 0]),
            ("text\n<br>\nmore", "text\nmore", &[0, 0]),
            ("> a\n> <x>\n>      b", "a\nb", &[1, 1]),
            ("> a\n<x>\nb", "a\nb", &[1, 0]),
            ("- a\n  <x>\n      b\n- c", "a\nb\nc", &[1, 1, 1]),
            ("- <a></a>\n  text", "text", &[1]),
            ("a\n<x>\n    <p>\n    <!-- c -->b\n    <img src=\"p.png\"> c\n    </p>\n</x>", "a\nb  c", &[0, 0]),
            ("   \\*a\n<span>\nb", "*a\nb", &[0, 0]),
            (">    \\*a\n> <span>\n> b", "*a\nb", &[1, 1]),
        ];
        for (md, text, depths) in cases {
            let rt = imp_fixed(md).content;
            assert_eq!(rt.text, *text, "{md:?}");
            assert!(rt.marks.is_empty(), "{md:?}");
            let got: Vec<usize> = rt.lines.iter().map(|l| l.containers.len()).collect();
            assert_eq!(got, *depths, "{md:?}");
        }
    }

    /// A tag line under a setext heading's text moves below its underline,
    /// where the heading has ended, so the heading keeps its lines; an
    /// element's tag opens or closes its element there.
    #[test]
    fn a_tag_line_under_a_setext_heading_moves_below_it() {
        let cases: &[(&str, &str, &[(&str, usize)])] = &[
            ("a\n<span>\n===", "# a", &[("span", 1)]),
            ("a\n<span>\n---", "## a", &[("span", 1)]),
            ("a\n<span>\n===\nb", "# a\n\nb", &[("span", 1)]),
            ("a\n<b> <i>\nc\n===", "# a c", &[("b", 1), ("i", 1)]),
            ("- a\n  <span>\n  ---\n- b", "- ## a\n\n- b", &[("span", 1)]),
            ("> a\n> <span>\n> ===\n> b", "> # a\n>\n> b", &[("span", 1)]),
            ("<quill-keep>\n\nTitle\n</quill-keep>\n===", "<quill-keep>\n\n# Title\n\n</quill-keep>", &[]),
        ];
        for (md, exported, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(crate::export::to_markdown(&imported.content), *exported, "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }

    /// A type 1 block ends at the first line holding any of its four closing
    /// tags, in any case, as CommonMark ends it, so what follows imports.
    #[test]
    fn a_type_1_block_ends_at_any_closing_tag_in_any_case() {
        let cases: &[(&str, &str)] = &[
            ("<PRE>x</PRE>\n\nafter", "after"),
            ("<pre>\nx\n</PRE> tail\n\nafter", "tail\nafter"),
            ("<pre>\nx\n</Script>\nafter", "after"),
            ("- <Script>\n  x\n  </SCRIPT>\n- after", "\nafter"),
            ("<div>\n<PRE>x</PRE> after\n</div>", "after"),
            ("<div>\n<pre>\nx\n</Pre> tail\n\npara\n</div>", "tail\npara"),
        ];
        for (md, text) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert!(dropped(&imported).iter().any(|(c, _)| ["pre", "script"].contains(c)), "{md:?}");
        }
    }

    /// A tag line drops as a blank line does, so it keeps a list item open as a
    /// blank line would: the list on either side of a wrapper's tag is one
    /// list. A quote the item sits in does not reach a tag outside it.
    #[test]
    fn a_tag_line_between_list_items_keeps_one_list() {
        for md in [
            "<x-keep>\n- a\n</x-keep>\n\n- b",
            "- a\n\n<x-keep>\n- b\n</x-keep>",
            "- a\n\n<quill-anchor ref=\"r\"></quill-anchor>\n\n- b",
            "- a\n<div>\n- b",
            "- x\n  - a\n\n</x>\n\n  - b",
        ] {
            let rt = imp_fixed(md).content;
            let items: Vec<&Container> = rt.lines.iter().filter_map(|l| l.containers.last()).collect();
            let [a, b] = items[items.len() - 2..] else { unreachable!() };
            assert!(
                matches!((a, b), (
                    Container::ListItem { ordinal: 0, instance: i, .. },
                    Container::ListItem { ordinal: 1, instance: j, .. },
                ) if i == j),
                "{md:?}: {:?}",
                rt.lines
            );
        }
        let rt = imp_fixed("> - a\n\n</x>\n\n- b").content;
        assert_eq!(rt.lines[0].containers.len(), 2);
        assert_eq!(rt.lines[1].containers.len(), 1);
    }

    /// `[^1]: Word` is a link reference definition to CommonMark, which would
    /// make `[^1]` a link to `Word`; import keeps both as the text they are.
    /// A definition that follows it on the next line stays one.
    #[test]
    fn a_footnote_shaped_definition_imports_as_text() {
        let imported = imp_fixed("[^1]: Word\n\ntext[^1]");
        assert_eq!(imported.content.text, "[^1]: Word\ntext[^1]");
        assert!(imported.content.marks.is_empty());
        assert_eq!(dropped(&imported), [("footnote_definition", 1)]);

        let imported = imp_fixed("> [^a]: one\n> [^A]: two\n> [x]: /url\n\nsee [^a] and [x]");
        assert_eq!(imported.content.text, "[^a]: one\n[^A]: two\nsee [^a] and x");
        assert_eq!(
            imported.content.marks,
            [Mark::new(33, 34, MarkKind::Link { url: "/url".into() })]
        );
        assert_eq!(dropped(&imported), [("footnote_definition", 2)]);

        let imported = imp_fixed(&format!("{}x[^1]", "[^1]: a\n\n".repeat(9)));
        assert!(imported.content.marks.is_empty());
        assert_eq!(dropped(&imported), [("footnote_definition", 9)]);
    }

    /// A tag alone on its line is a block wrapper, whatever its name: the
    /// inline allowlist does not reach it.
    #[test]
    fn the_allowlist_is_inline_only() {
        let imported = imp_fixed("<u>\ntext\n</u>");
        assert_eq!(imported.content.text, "text");
        assert!(imported.content.marks.is_empty());
        assert_eq!(dropped(&imported), [("u", 1)]);

        let imported = imp_fixed("para\n\n<br>\n\nnext");
        assert_eq!(imported.content.text, "para\nnext");
        assert_eq!(dropped(&imported), [("br", 1)]);
    }

    /// One count per open or self-closing tag, by lowercase name, in order of
    /// first occurrence: closing tags, comments, a `<pre>` block's content, the
    /// inline allowlist, `quill-anchor` and a modeled element, inline or alone
    /// on its line, count nothing. An element that drops unclosed counts under
    /// `quill-<name>`, and a `quill-*` name outside the grammar under its full
    /// name.
    #[test]
    fn dropped_tags_count_once_per_opening() {
        let md = "<div>\n<span>a</span> <SPAN>b</SPAN><br> <u>c</u> <img src=x/>\n</div>\n\n\
                  <!-- <em>not markup</em> -->\n\n<pre><b>x</b></pre>\n\n\
                  <quill-anchor id=\"x\">t</quill-anchor> <quill-keep>k</quill-keep>\n\
                  <QUILL-ANCHOR ref=\"y\"></QUILL-ANCHOR>\n<Quill-Keep>\n\n[^1]: f\n\n\
                  | <span>cell</span> |\n|---|\n| <hr/> <quill-a--b>z</quill-a--b> |";
        let imported = imp_fixed(md);
        assert_eq!(
            dropped(&imported),
            [
                ("div", 1),
                ("span", 3),
                ("img", 1),
                ("pre", 1),
                ("quill-keep", 1),
                ("footnote_definition", 1),
                ("hr", 1),
                ("quill-a--b", 1)
            ]
        );
        assert!(imp_fixed("plain **text**, `<code>`, \\<escaped>").warnings.is_empty());
    }

    fn container_tags(rt: &Normalized) -> Vec<Vec<&'static str>> {
        rt.lines.iter().map(|l| l.containers.iter().map(Container::tag).collect()).collect()
    }

    /// An element tag alone on its line wraps the blocks up to its close tag,
    /// inside the containers around it, and stays where its own indentation
    /// puts it: under paragraph text, and after a list item it does not
    /// continue.
    #[test]
    fn an_element_tag_line_wraps_the_blocks_it_holds() {
        let cases: &[(&str, &str, &[&[&str]])] = &[
            ("<quill-keep>\npara\n</quill-keep>", "para", &[&["element"]]),
            ("<quill-keep>\n- a\n- b\n</quill-keep>", "a\nb", &[&["element", "list_item"], &["element", "list_item"]]),
            ("- <quill-keep>\n\n  a\n\n  </quill-keep>\n- b", "a\nb", &[&["list_item", "element"], &["list_item"]]),
            ("<quill-keep>\n> q\n</quill-keep>", "q", &[&["element", "quote"]]),
            ("<quill-keep>\n</quill-keep>", "", &[&["element"]]),
            ("para\n<quill-keep>\nmore\n</quill-keep>", "para\nmore", &[&[], &["element"]]),
            ("- a\n\n<quill-keep>\n- b\n</quill-keep>", "a\nb", &[&["list_item"], &["element", "list_item"]]),
        ];
        for (md, text, tags) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(container_tags(&imported.content), *tags, "{md:?}");
            assert!(imported.warnings.is_empty(), "{md:?}");
        }

        let rt = imp_fixed("<quill-keep note=\"x\">\na\n</quill-keep>\n<quill-keep note=\"x\">\nb\n</quill-keep>").content;
        let keep = |instance| Container::Element {
            name: "keep".into(),
            attrs: [("note".to_string(), "x".to_string())].into(),
            instance,
        };
        assert_eq!(rt.lines[0].containers, [keep(0)]);
        assert_eq!(rt.lines[1].containers, [keep(1)]);
    }

    /// An inline element pair is an element mark over what it holds, crossing
    /// other marks freely, in prose, a heading and a table cell.
    #[test]
    fn an_inline_element_pair_is_a_mark() {
        let hl = |attrs: &[(&str, &str)]| MarkKind::Element {
            name: "hl".into(),
            attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        let rt = imp_fixed("a <quill-hl tone=\"warm\">b</quill-hl> c").content;
        assert_eq!(rt.marks, [Mark::new(2, 3, hl(&[("tone", "warm")]))]);

        let rt = imp_fixed("<quill-hl>a **b</quill-hl> c**").content;
        assert_eq!(rt.marks, [Mark::new(0, 3, hl(&[])), Mark::new(2, 5, MarkKind::Strong)]);

        let rt = imp_fixed("# a <quill-hl>b</quill-hl>").content;
        assert_eq!(rt.marks, [Mark::new(2, 3, hl(&[]))]);

        let rt = imp_fixed("| <quill-hl>x</quill-hl> y |\n|---|").content;
        let (text, marks) = crate::serial::parse_cell(&rt.islands[0].props["header"][0]);
        assert_eq!(text, "x y");
        assert_eq!(marks, [Mark::new(0, 1, hl(&[]))]);
    }

    /// An element the carrier cannot read drops its tags and reports
    /// `quill-<name>`, what it wraps importing; an attribute outside the
    /// grammar drops alone. A close tag with nothing to close drops silently.
    #[test]
    fn an_element_drops_where_it_does_not_close() {
        let cases: &[(&str, &str, &[(&str, usize)])] = &[
            ("<quill-keep>\na", "a", &[("quill-keep", 1)]),
            ("<quill-keep/>\na", "a", &[("quill-keep", 1)]),
            ("- <quill-keep>\n  a\n- b\n</quill-keep>", "a\nb", &[("quill-keep", 1)]),
            ("> <quill-keep>\n> a\n\n</quill-keep>", "a", &[("quill-keep", 1)]),
            ("</quill-keep>\na", "a", &[]),
            ("a <quill-hl>b", "a b", &[("quill-hl", 1)]),
            ("a<quill-hl></quill-hl>b", "ab", &[("quill-hl", 1)]),
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

        let imported = imp_fixed("<quill-keep onclick=\"x\" note=\"y\">\na\n</quill-keep>");
        assert_eq!(
            imported.content.lines[0].containers,
            [Container::Element {
                name: "keep".into(),
                attrs: [("note".to_string(), "y".to_string())].into(),
                instance: 0,
            }]
        );
        assert_eq!(dropped(&imported), [("quill-keep[onclick]", 1)]);
    }
}
