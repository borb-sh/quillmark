//! Markdown-string preprocessing at the
//! [`from_markdown`](crate::import::from_markdown) boundary (markdown-spec §7):
//! characters the content cannot hold, then the parser-guided repair.

use crate::carrier;
use crate::html::{self, BlockKind};
use pulldown_cmark::{Event, Options, Parser, Tag as PTag, TagEnd};
use std::borrow::Cow;
use std::ops::Range;

/// The Unicode bidi formatting controls, which sit adjacent to `**`/`_` and
/// defeat delimiter recognition.
#[inline]
pub(crate) fn is_bidi_char(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' // ARABIC LETTER MARK (ALM)
        | '\u{200E}' // LEFT-TO-RIGHT MARK (LRM)
        | '\u{200F}' // RIGHT-TO-LEFT MARK (RLM)
        | '\u{202A}' // LEFT-TO-RIGHT EMBEDDING (LRE)
        | '\u{202B}' // RIGHT-TO-LEFT EMBEDDING (RLE)
        | '\u{202C}' // POP DIRECTIONAL FORMATTING (PDF)
        | '\u{202D}' // LEFT-TO-RIGHT OVERRIDE (LRO)
        | '\u{202E}' // RIGHT-TO-LEFT OVERRIDE (RLO)
        | '\u{2066}' // LEFT-TO-RIGHT ISOLATE (LRI)
        | '\u{2067}' // RIGHT-TO-LEFT ISOLATE (RLI)
        | '\u{2068}' // FIRST STRONG ISOLATE (FSI)
        | '\u{2069}' // POP DIRECTIONAL ISOLATE (PDI)
    )
}

/// Every character Typst's lexer reads as a line break (`is_newline`) besides
/// `\n` and `\r`: one mid-paragraph reopens `at_start`, so what follows it is
/// read as a block marker the author never wrote, and two in a row are a
/// paragraph break.
#[inline]
pub fn is_line_separator(c: char) -> bool {
    matches!(
        c,
        '\u{000B}' // LINE TABULATION (VT)
        | '\u{000C}' // FORM FEED (FF)
        | '\u{0085}' // NEXT LINE (NEL)
        | '\u{2028}' // LINE SEPARATOR
        | '\u{2029}' // PARAGRAPH SEPARATOR
    )
}

/// What the content admits `c` as, `None` dropping it. A `\r` is dropped
/// because it pairs with a `\n` that stays; a line separator becomes a space,
/// both being Unicode whitespace, so dropping one would join the words it parts.
/// No downstream escape can neutralize a separator — a `\` before whitespace is
/// Typst's own linebreak — so it cannot survive in the text.
#[inline]
pub(crate) fn admit_char(c: char) -> Option<char> {
    match c {
        '\r' => None,
        c if is_bidi_char(c) => None,
        c if is_line_separator(c) => Some(' '),
        c => Some(c),
    }
}

fn admit_chars(s: &str) -> String {
    if !s.chars().any(|c| admit_char(c) != Some(c)) {
        return s.to_string();
    }

    s.chars().filter_map(admit_char).collect()
}

/// The text the import parses, the byte offset in it of each footnote-shaped
/// definition the repair made literal, and the name of each opening tag on
/// the lines it deleted, at the offset just past what replaced them.
pub(crate) struct Repaired {
    pub(crate) text: String,
    pub(crate) footnotes: Vec<usize>,
    pub(crate) tags: Vec<(usize, String)>,
}

/// Every markdown normalization in order (spec §7): CRLF → LF, bidi controls
/// dropped and line separators spaced, then [`repair`].
pub(crate) fn normalize_markdown(markdown: &str, options: Options) -> Repaired {
    let cleaned = admit_chars(&normalize_line_endings(markdown));
    repair(cleaned, options)
}

/// Rounds the repair takes at most. A round leaves work for the next only where
/// its edits make a new HTML block: a freed line opening a container that holds
/// one, a tag line split from paragraph text, or a block of tags moved into a
/// list item.
const REPAIR_ROUNDS: usize = 8;

/// How many following lines a tag left open at its line's end may close on.
const TAG_JOIN_LINES: usize = 8;

/// Rewrite `text` so the parse reads the markdown an HTML block, a line of tags
/// or a footnote-shaped definition would hide from it, and so a tag line keeps
/// a list item open as a blank line does (markdown-spec §6.2, §7 step 4). Each
/// round parses and edits only inside the spans that parse located, so a fence
/// is never touched; the rounds end at one that plans no edit. Where a line
/// inside a type 6 or 7 block opens a type 1–5 block or a fence, at its start
/// or past its quote and list markers, that does not close inside it, that
/// line and the rest of the block are deleted: the unrepaired parse drops them
/// with the block, and freed they would swallow what follows it. Past the last round, only the footnote-shaped
/// definitions it freed are made literal.
fn repair(text: String, options: Options) -> Repaired {
    let mut r = Repaired {
        text,
        footnotes: Vec::new(),
        tags: Vec::new(),
    };
    if !may_need_repair(&r.text) {
        return r;
    }
    for _ in 0..REPAIR_ROUNDS {
        let edits = plan(&r.text, options);
        if edits.is_empty() {
            return r;
        }
        apply(&mut r, &edits);
    }
    let edits = footnotes_only(&r.text, options);
    apply(&mut r, &edits);
    r
}

/// Whether some line could open an HTML block, hold a table row of tags, or
/// define a `^` label: its first character past container markers is `<`, or a
/// `[` with a `^` after it. Text failing this has nothing to repair.
fn may_need_repair(s: &str) -> bool {
    s.lines().any(|line| {
        let rest = line.trim_start_matches(|c: char| {
            c.is_ascii_whitespace() || c.is_ascii_digit() || matches!(c, '>' | '-' | '+' | '*' | '.' | ')')
        });
        rest.starts_with('<') || (rest.starts_with('[') && rest.contains('^'))
    })
}

#[derive(Default)]
struct Edit {
    range: Range<usize>,
    with: String,
    /// The `[` escape of a footnote-shaped definition.
    footnote: bool,
    /// The opening tags of the lines it deletes, by name.
    tags: Vec<String>,
}

fn plan(src: &str, options: Options) -> Vec<Edit> {
    let parser = Parser::new_ext(src, options);
    let defs = footnote_spans(&parser);
    let mut edits: Vec<Edit> = Vec::new();
    let mut leaves: Vec<Range<usize>> = Vec::new();
    let mut block: Option<(Vec<usize>, Option<usize>)> = None;
    let mut row: Option<usize> = None;
    let mut tag_rows: Vec<SrcLine> = Vec::new();
    let mut runs = Runs::default();
    let mut items: Vec<usize> = Vec::new();
    // The innermost list item closed since the last event that opens or holds
    // something, an HTML block's start aside: the item a blank line where that
    // block stands would have kept open.
    let mut ended: Option<usize> = None;
    for (event, range) in parser.into_offset_iter() {
        if !defs.is_empty() && !is_container(&event) {
            leaves.push(range.clone());
        }
        if let Some(run) = runs.feed(&event, &range) {
            edits.extend(run_tag_line_edits(src, &run));
        }
        if !matches!(event, Event::End(_)) {
            ended = ended.filter(|_| matches!(event, Event::Start(PTag::HtmlBlock)));
        }
        match event {
            Event::Start(PTag::Item) => items.push(range.start),
            Event::End(TagEnd::Item) => {
                let item = items.pop();
                ended = ended.or(item);
            }
            Event::Start(PTag::HtmlBlock) => block = Some((Vec::new(), ended.take())),
            Event::Html(_) => {
                if let Some((starts, _)) = &mut block {
                    starts.push(range.start);
                }
            }
            Event::End(TagEnd::HtmlBlock) => {
                if let Some((starts, item)) = block.take() {
                    let lines: Vec<SrcLine> = starts.iter().map(|&at| SrcLine::at(src, at)).collect();
                    match item.and_then(|at| into_item(src, at, &lines)) {
                        Some(edit) => {
                            edits.push(edit);
                            ended = item;
                        }
                        None => edits.extend(html_block_edit(src, &lines)),
                    }
                }
            }
            Event::Start(PTag::TableRow) => row = Some(range.start),
            Event::End(TagEnd::TableRow) => {
                let line = row.take().map(|at| SrcLine::at(src, at));
                if let Some(line) = line.filter(|l| html::tag_line(l.content).is_some()) {
                    tag_rows.push(line);
                }
            }
            _ => {}
        }
    }
    if let Some(run) = runs.finish() {
        edits.extend(run_tag_line_edits(src, &run));
    }
    edits.extend(tag_row_edits(src, &tag_rows));
    edits.extend(footnote_plan(src, &defs, leaves));
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    edits
}

/// The edits making every footnote-shaped definition of `src` literal, and
/// nothing else.
fn footnotes_only(src: &str, options: Options) -> Vec<Edit> {
    let parser = Parser::new_ext(src, options);
    let defs = footnote_spans(&parser);
    if defs.is_empty() {
        return Vec::new();
    }
    let leaves = parser
        .into_offset_iter()
        .filter(|(event, _)| !is_container(event))
        .map(|(_, range)| range)
        .collect();
    let mut edits = footnote_plan(src, &defs, leaves);
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    edits
}

/// The span of each label's first footnote-shaped definition.
fn footnote_spans(parser: &Parser) -> Vec<Range<usize>> {
    parser
        .reference_definitions()
        .iter()
        .filter(|(label, _)| label.starts_with('^'))
        .map(|(_, def)| def.span.clone())
        .collect()
}

/// The edits for `defs` and for every later definition of their labels,
/// `leaves` being the ranges of the parse's events that are not containers.
fn footnote_plan(src: &str, defs: &[Range<usize>], leaves: Vec<Range<usize>>) -> Vec<Edit> {
    if defs.is_empty() {
        return Vec::new();
    }
    let repeats = footnote_definitions(src, leaves)
        .into_iter()
        .filter(|&at| defs.iter().all(|d| d.start != at))
        .map(|at| at..at);
    defs.iter().cloned().chain(repeats).flat_map(|def| footnote_edits(src, def)).collect()
}

/// Apply `edits`, sorted by start, to `r.text`, skipping one that overlaps an
/// edit already applied (the next round plans it again). The offsets `r`
/// holds move with the text, and the applied edits' own are appended.
fn apply(r: &mut Repaired, edits: &[Edit]) {
    let src = &r.text;
    let mut out = String::with_capacity(src.len() + 64);
    let mut taken = 0;
    let mut applied: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    let mut footnotes = Vec::new();
    let mut tags = Vec::new();
    for e in edits {
        if e.range.start < taken {
            continue;
        }
        out.push_str(&src[taken..e.range.start]);
        if e.footnote {
            footnotes.push(out.len());
        }
        let new_start = out.len();
        out.push_str(&e.with);
        tags.extend(e.tags.iter().map(|name| (out.len(), name.clone())));
        applied.push((e.range.clone(), new_start..out.len()));
        taken = e.range.end;
    }
    out.push_str(&src[taken..]);
    let moved = |p: usize| match applied.iter().rev().find(|(old, _)| old.start <= p) {
        None => p,
        Some((old, new)) if p < old.end => new.start,
        Some((old, new)) => new.end + (p - old.end),
    };
    for p in r.footnotes.iter_mut().chain(r.tags.iter_mut().map(|(p, _)| p)) {
        *p = moved(*p);
    }
    r.footnotes.extend(footnotes);
    r.tags.extend(tags);
    r.text = out;
}

/// A block of tag lines that ended the list item opening at `item`, written
/// inside that item instead, a blank line between its lines. A blank line keeps
/// an item open for the line after it to continue, and a tag line drops as a
/// blank line does, so the list on either side of a wrapper's tag stays one
/// list. A block in other quotes than the item's stays, and so does one holding
/// an element tag (markdown-spec §6.4), which opens or closes its element where
/// its own indentation stands.
fn into_item(src: &str, item: usize, lines: &[SrcLine]) -> Option<Edit> {
    let marker = SrcLine::at(src, item);
    let rest = &marker.content[..marker.content.find('\n').unwrap_or(marker.content.len())];
    let width = match rest.bytes().next() {
        Some(b'-' | b'+' | b'*') => 1,
        _ => rest.bytes().take_while(u8::is_ascii_digit).count() + 1,
    };
    let after = &rest[width.min(rest.len())..];
    let gap = html::indent_columns(after);
    let gap = if after.trim().is_empty() || gap > 4 { 1 } else { gap };
    let cont = format!("{}{}", continuation_of(marker.prefix), " ".repeat(width + gap));
    let blank = blank_of(&cont);
    let fits = |l: &SrcLine| {
        html::tag_line(l.content).is_some_and(|tags| !tags.iter().any(|t| carrier::modeled_tag(t).is_some()))
            && blank_of(l.prefix) == blank
    };
    if !lines.iter().all(fits) {
        return None;
    }
    let (first, last) = (lines.first()?, lines.last()?);
    let with: Vec<String> = lines.iter().map(|l| format!("{cont}{}", l.content.trim())).collect();
    Some(Edit {
        range: first.start..last.end(),
        with: with.join(&format!("\n{blank}\n")),
        ..Edit::default()
    })
}

/// One source line of a span: its container prefix as the parser consumed it,
/// and the rest of the line.
#[derive(Clone, Copy)]
struct SrcLine<'a> {
    start: usize,
    prefix: &'a str,
    content: &'a str,
}

impl<'a> SrcLine<'a> {
    /// The line holding byte `at`, its prefix everything before `at`.
    fn at(src: &'a str, at: usize) -> Self {
        let start = src[..at].rfind('\n').map_or(0, |i| i + 1);
        let end = src[at..].find('\n').map_or(src.len(), |i| at + i);
        SrcLine {
            start,
            prefix: &src[start..at],
            content: &src[at..end],
        }
    }

    fn end(&self) -> usize {
        self.start + self.prefix.len() + self.content.len()
    }

    fn whole(&self) -> String {
        format!("{}{}", self.prefix, self.content)
    }
}

/// A blank line inside the containers `prefix` holds: each quote's `>` kept,
/// a list marker dropped.
pub(crate) fn blank_of(prefix: &str) -> String {
    let mut s: String = prefix
        .chars()
        .map(|c| if matches!(c, '>' | '\t') { c } else { ' ' })
        .collect();
    s.truncate(s.trim_end().len());
    s
}

/// The prefix a later line of the same containers carries: a list marker
/// becomes the spaces of its width.
fn continuation_of(prefix: &str) -> String {
    prefix
        .chars()
        .map(|c| if matches!(c, '>' | '\t') { c } else { ' ' })
        .collect()
}

/// Whether the line after byte `end` (a line's end) is one a block left open
/// before it would take in: it exists and holds more than quote markers.
fn next_line_continues(src: &str, end: usize) -> bool {
    let Some(rest) = src[end..].strip_prefix('\n') else {
        return false;
    };
    let next = &rest[..rest.find('\n').unwrap_or(rest.len())];
    !next
        .trim_matches(|c: char| c == '>' || c.is_ascii_whitespace())
        .is_empty()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Tags only: drops as an HTML block of its own.
    Tag,
    /// Freed markdown, possibly a paragraph a later line continues.
    Text,
    /// Kept whole: a type 1–5 block, a fence, or a type-6 tag that never
    /// closes and the lines it holds.
    Raw,
}

enum Row {
    Blank,
    Line(String, Kind),
}

#[derive(Default)]
struct Rows(Vec<Row>);

impl Rows {
    /// Whether the next line starts a block rather than continuing paragraph
    /// text.
    fn at_block_start(&self) -> bool {
        !matches!(self.0.last(), Some(Row::Line(_, Kind::Text)))
    }

    fn push(&mut self, line: String, kind: Kind) {
        self.0.push(Row::Line(line, kind));
    }

    /// Lines of tags, padded with a blank line above and below.
    fn tags(&mut self, lines: impl IntoIterator<Item = String>) {
        if matches!(self.0.last(), Some(Row::Line(..))) {
            self.0.push(Row::Blank);
        }
        for line in lines {
            self.push(line, Kind::Tag);
        }
        self.0.push(Row::Blank);
    }

    /// The span's replacement text. A blank line closes it when the line after
    /// would join it: always after freed text (a paragraph's lazy
    /// continuation), and after tags where `tag_needs_close`, the span having
    /// ended on something other than a blank line or a container's end.
    fn finish(mut self, blank: &str, next_continues: bool, tag_needs_close: bool) -> String {
        while matches!(self.0.last(), Some(Row::Blank)) {
            self.0.pop();
        }
        let close = match self.0.last() {
            Some(Row::Line(_, Kind::Text)) => true,
            Some(Row::Line(_, Kind::Tag)) => tag_needs_close,
            _ => false,
        };
        if close && next_continues {
            self.0.push(Row::Blank);
        }
        let lines: Vec<&str> = self
            .0
            .iter()
            .map(|r| match r {
                Row::Blank => blank,
                Row::Line(l, _) => l.as_str(),
            })
            .collect();
        lines.join("\n")
    }
}

/// What a piece of a line inside an HTML block is, read where a block may start.
enum Piece {
    Tags,
    /// Opens with a complete type-6 tag ending at this offset, text after it.
    BlockTag(usize),
    /// Opens a type 1–5 block.
    Opener(BlockKind),
    /// Opens a tag the line does not close.
    Open,
    Fence(char, usize),
    Text,
}

fn classify(t: &str) -> Piece {
    if let Some(after) = t.strip_prefix('<') {
        if let Some(kind) = html::block_start(t).filter(|k| k.end_marker().is_some()) {
            return Piece::Opener(kind);
        }
        if html::tag_line(t).is_some() {
            return Piece::Tags;
        }
        return match html::tag_at(t, 0) {
            Some(tag) if html::is_block_name(tag.name) => Piece::BlockTag(tag.span.end),
            Some(_) => Piece::Text,
            None => {
                let name = after.strip_prefix('/').unwrap_or(after);
                if name.starts_with(|c: char| c.is_ascii_alphabetic()) {
                    Piece::Open
                } else {
                    Piece::Text
                }
            }
        };
    }
    match fence_open(t) {
        Some((c, n)) => Piece::Fence(c, n),
        None => Piece::Text,
    }
}

fn fence_open(t: &str) -> Option<(char, usize)> {
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3 && (c == '~' || !t[n..].contains('`'))).then_some((c, n))
}

fn closes_fence(content: &str, c: char, n: usize) -> bool {
    if html::indent_columns(content) > 3 {
        return false;
    }
    let t = content.trim_start();
    let run = t.chars().take_while(|&x| x == c).count();
    run >= n && t[run..].trim().is_empty()
}

fn html_block_edit(src: &str, lines: &[SrcLine]) -> Option<Edit> {
    let (first, last) = (lines.first()?, lines.last()?);
    let kind = html::block_start(first.content)?;
    if let Some(edit) = close_edit(kind, lines) {
        return Some(edit);
    }
    let (start, (rows, tags), tag_needs_close) = match kind.end_marker() {
        Some(_) => (last.start, (rescue_end(kind, last)?, Vec::new()), true),
        None => (first.start, transparent(lines), false),
    };
    let end = last.end();
    let with = rows.finish(&blank_of(last.prefix), next_line_continues(src, end), tag_needs_close);
    (with != src[start..end]).then_some(Edit {
        range: start..end,
        with,
        tags,
        ..Edit::default()
    })
}

/// The edit respelling a type 1 block's first closing tag as the block's own in
/// lowercase, where it is spelled otherwise: CommonMark ends the block at that
/// line, and the parser only at its own closing tag in lowercase.
fn close_edit(kind: BlockKind, lines: &[SrcLine]) -> Option<Edit> {
    let BlockKind::Verbatim(close) = kind else {
        return None;
    };
    let (line, r) = lines
        .iter()
        .find_map(|l| html::verbatim_close(l.content).map(|r| (l, r)))?;
    let at = line.start + line.prefix.len();
    (&line.content[r.clone()] != close).then(|| Edit {
        range: at + r.start..at + r.end,
        with: close.to_string(),
        ..Edit::default()
    })
}

/// `s`, a kept type 1–5 block's text through its end marker, with a type 1
/// block's closing tag respelled as [`close_edit`] respells it.
fn respelled(kind: BlockKind, s: &str) -> Cow<'_, str> {
    let BlockKind::Verbatim(close) = kind else {
        return Cow::Borrowed(s);
    };
    match html::verbatim_close(s) {
        Some(r) if &s[r.clone()] != close => Cow::Owned(format!("{}{close}{}", &s[..r.start], &s[r.end..])),
        _ => Cow::Borrowed(s),
    }
}

/// A type 1–5 block's last line, split after its end marker when text follows
/// it, which keeps the line's indent; `None` when nothing follows, or when a
/// piece of what follows would open a block running past the line, which then
/// stays dropped.
fn rescue_end(kind: BlockKind, last: &SrcLine) -> Option<Rows> {
    let at = html::block_end(kind, last.content)?;
    let mut frag = last.content[at..].trim();
    if frag.is_empty() {
        return None;
    }
    let mut rows = Rows::default();
    rows.push(format!("{}{}", last.prefix, respelled(kind, &last.content[..at])), Kind::Raw);
    let prefix = format!("{}{}", continuation_of(last.prefix), shallow_lead(last.content));
    loop {
        match classify(frag) {
            Piece::Tags => {
                rows.tags([format!("{prefix}{frag}")]);
                return Some(rows);
            }
            Piece::BlockTag(end) => {
                rows.tags([format!("{prefix}{}", &frag[..end])]);
                frag = frag[end..].trim_start();
            }
            Piece::Opener(kind) => {
                let end = html::block_end(kind, frag)?;
                rows.push(format!("{prefix}{}", respelled(kind, &frag[..end])), Kind::Raw);
                frag = frag[end..].trim_start();
            }
            Piece::Open | Piece::Fence(..) => return None,
            Piece::Text => {
                rows.push(format!("{prefix}{frag}"), Kind::Text);
                return Some(rows);
            }
        }
        if frag.is_empty() {
            return Some(rows);
        }
    }
}

/// A type 6 or 7 block's lines, its tags padded out so every other line parses
/// as markdown, and the opening tags of the lines it deletes. A piece split off
/// after another keeps its line's indent, and so stays in the list item that
/// indent continues.
fn transparent(lines: &[SrcLine]) -> (Rows, Vec<String>) {
    let mut rows = Rows::default();
    // Four columns of indent past a block start make an indented code line, so
    // a line that starts a block drops them, and the lines continuing it drop
    // as many: HTML source indents a block's lines alike.
    let mut dedent = 0;
    let mut i = 0;
    'lines: while i < lines.len() {
        let mut prefix = lines[i].prefix.to_string();
        // The containers the line's own markers open before a piece that
        // opens a block, which the lines that block takes in continue.
        let mut nest = String::new();
        let mut frag = lines[i].content;
        if !rows.at_block_start() {
            frag = strip_columns(frag, dedent);
        }
        let mut indent = shallow_lead(frag);
        loop {
            let t = frag.trim_start();
            if t.is_empty() {
                break;
            }
            let deep = html::indent_columns(frag) > 3;
            if rows.at_block_start() {
                dedent = if deep { html::indent_columns(frag) } else { 0 };
            }
            // Past paragraph text, an indent that deep continues the paragraph,
            // whatever it indents.
            let continuation = deep && !rows.at_block_start();
            let mut lead = if deep { "" } else { &frag[..frag.len() - t.len()] }.to_string();
            let mut t = t;
            if let Some(m) = container_markers(t).filter(|_| !continuation) {
                if matches!(classify(&t[m..]), Piece::Opener(_) | Piece::Fence(..)) {
                    lead.push_str(&t[..m]);
                    nest.push_str(&continuation_of(&lead));
                    t = &t[m..];
                }
            }
            match classify(t) {
                Piece::Tags => {
                    rows.tags([format!("{prefix}{lead}{t}")]);
                    break;
                }
                Piece::BlockTag(end) => {
                    rows.tags([format!("{prefix}{lead}{}", &t[..end])]);
                    frag = t[end..].trim_start();
                }
                Piece::Opener(kind) if !continuation => {
                    if let Some(end) = html::block_end(kind, t) {
                        rows.push(format!("{prefix}{lead}{}", respelled(kind, &t[..end])), Kind::Raw);
                        frag = t[end..].trim_start();
                    } else {
                        let close = (i + 1..lines.len())
                            .find_map(|j| html::block_end(kind, lines[j].content).map(|end| (j, end)));
                        let Some((j, end)) = close else {
                            return (rows, opening_tags(t, &lines[i + 1..]));
                        };
                        rows.push(format!("{prefix}{lead}{t}"), Kind::Raw);
                        for line in &lines[i + 1..j] {
                            rows.push(format!("{}{nest}{}", line.prefix, line.content), Kind::Raw);
                        }
                        let closing = lines[j];
                        let close = respelled(kind, &closing.content[..end]);
                        rows.push(format!("{}{nest}{close}", closing.prefix), Kind::Raw);
                        i = j;
                        indent = shallow_lead(closing.content);
                        frag = closing.content[end..].trim_start();
                    }
                }
                Piece::Fence(c, n) if !continuation => {
                    let close = (i + 1..lines.len())
                        .find(|&j| closes_fence(strip_columns(lines[j].content, dedent), c, n));
                    let Some(j) = close else {
                        return (rows, opening_tags(t, &lines[i + 1..]));
                    };
                    rows.push(format!("{prefix}{lead}{t}"), Kind::Raw);
                    for line in &lines[i + 1..=j] {
                        let content = strip_columns(line.content, dedent);
                        rows.push(format!("{}{nest}{content}", line.prefix), Kind::Raw);
                    }
                    i = j;
                    break;
                }
                Piece::Open if !continuation => {
                    if let Some((j, end)) = join_tag(t, &lines[i + 1..]) {
                        let j = i + 1 + j;
                        let mut run = vec![format!("{prefix}{lead}{t}")];
                        run.extend(lines[i + 1..j].iter().map(SrcLine::whole));
                        run.push(format!("{}{}", lines[j].prefix, &lines[j].content[..end]));
                        rows.tags(run);
                        i = j;
                        indent = shallow_lead(lines[j].content);
                        frag = lines[j].content[end..].trim_start();
                    } else if html::block_start(t) == Some(BlockKind::BlockName) {
                        // Its block runs to the blank line padding the next
                        // tag line, as the unrepaired parse runs it to the
                        // block's end.
                        let to = (i + 1..lines.len())
                            .find(|&j| matches!(classify(lines[j].content.trim_start()), Piece::Tags))
                            .unwrap_or(lines.len());
                        rows.push(format!("{prefix}{lead}{t}"), Kind::Raw);
                        for line in &lines[i + 1..to] {
                            rows.push(line.whole(), Kind::Raw);
                        }
                        i = to;
                        continue 'lines;
                    } else {
                        rows.push(format!("{prefix}{lead}{t}"), Kind::Text);
                        break;
                    }
                }
                _ => {
                    let shown = if continuation { frag } else { t };
                    let lead = if continuation { "" } else { &lead };
                    rows.push(format!("{prefix}{lead}{shown}"), Kind::Text);
                    break;
                }
            }
            let inside = if nest.is_empty() { indent } else { &nest };
            prefix = format!("{}{inside}", continuation_of(lines[i].prefix));
        }
        i += 1;
    }
    (rows, Vec::new())
}

/// The length of the quote and list markers `t` opens with, each with the
/// space after it, where what follows them starts a block; `None` without
/// one, or where what follows is indented code.
fn container_markers(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    let mut i = 0;
    loop {
        if html::indent_columns(&t[i..]) > 3 {
            return None;
        }
        let at = i + t[i..].len() - t[i..].trim_start_matches([' ', '\t']).len();
        let digits = b[at..].iter().take_while(|c| c.is_ascii_digit()).count();
        let (width, quote) = match b.get(at + digits) {
            Some(b'>') if digits == 0 => (1, true),
            Some(b'-' | b'+' | b'*') if digits == 0 => (1, false),
            Some(b'.' | b')') if (1..=9).contains(&digits) => (digits + 1, false),
            _ => return (i > 0).then_some(at),
        };
        let past = at + width;
        i = match b.get(past) {
            Some(b' ' | b'\t') => past + 1,
            _ if quote => past,
            _ => return (i > 0).then_some(at),
        };
    }
}

/// The names of the opening tags in `first` and the lines after it, read as
/// one type 6 block reads them.
fn opening_tags(first: &str, rest: &[SrcLine]) -> Vec<String> {
    let text: Vec<&str> = std::iter::once(first).chain(rest.iter().map(|l| l.content)).collect();
    html::tags(&text.join("\n"))
        .into_iter()
        .filter(|t| !t.closing)
        .map(|t| t.name.to_string())
        .collect()
}

/// The whitespace leading `line` when it is shy of an indented code line's
/// four columns, else nothing.
fn shallow_lead(line: &str) -> &str {
    if html::indent_columns(line) > 3 {
        return "";
    }
    &line[..line.len() - line.trim_start().len()]
}

/// `line` without up to `n` columns of leading indentation.
fn strip_columns(line: &str, n: usize) -> &str {
    let mut col = 0;
    for (i, c) in line.char_indices() {
        if col >= n {
            return &line[i..];
        }
        match c {
            ' ' => col += 1,
            '\t' => col += 4 - col % 4,
            _ => return &line[i..],
        }
    }
    ""
}

/// Where the tag `t` opens closes on a later line: that line's index in
/// `rest` and the byte offset past its `>`.
fn join_tag(t: &str, rest: &[SrcLine]) -> Option<(usize, usize)> {
    let mut joined = t.to_string();
    let mut starts = Vec::new();
    for line in rest.iter().take(TAG_JOIN_LINES) {
        joined.push('\n');
        starts.push(joined.len());
        joined.push_str(line.content);
    }
    let end = html::tag_at(&joined, 0)?.span.end;
    let j = starts.iter().rposition(|&s| s < end)?;
    Some((j, end - starts[j]))
}

/// Inline content between two block events outside a table: a paragraph's, a
/// heading's, or a tight list item's text.
struct Run {
    /// The first inline event's offset.
    start: usize,
    /// Where the heading holding the run ends, its underline included.
    heading: Option<usize>,
    /// Each inline HTML event's offset.
    tags: Vec<usize>,
}

/// The [`Run`]s of one parse, fed its events in order.
#[derive(Default)]
struct Runs {
    open: Option<Run>,
    tables: usize,
    heading: Option<usize>,
}

impl Runs {
    /// The run `event` ends, if it ends one.
    fn feed(&mut self, event: &Event, range: &Range<usize>) -> Option<Run> {
        if is_inline(event) {
            if self.tables == 0 {
                let heading = self.heading;
                let run = self.open.get_or_insert_with(|| Run {
                    start: range.start,
                    heading,
                    tags: Vec::new(),
                });
                if matches!(event, Event::InlineHtml(_)) {
                    run.tags.push(range.start);
                }
            }
            return None;
        }
        match event {
            Event::Start(PTag::Heading { .. }) => self.heading = Some(range.end),
            Event::End(TagEnd::Heading(_)) => self.heading = None,
            Event::Start(PTag::Table(_)) => self.tables += 1,
            Event::End(TagEnd::Table) => self.tables -= 1,
            _ => {}
        }
        self.open.take()
    }

    fn finish(self) -> Option<Run> {
        self.open
    }
}

pub(crate) fn is_inline(event: &Event) -> bool {
    match event {
        Event::Text(_) | Event::Code(_) | Event::SoftBreak | Event::HardBreak | Event::InlineHtml(_) => {
            true
        }
        Event::Start(tag) => matches!(
            tag,
            PTag::Emphasis | PTag::Strong | PTag::Strikethrough | PTag::Link { .. } | PTag::Image { .. }
        ),
        Event::End(tag) => matches!(
            tag,
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image
        ),
        _ => false,
    }
}

/// The first line of a run holding only tags, which CommonMark reads as inline
/// HTML, made a type 7 block start: a blank line above it where text precedes
/// it in the run, and one tag per line. The block runs to the run's end, as
/// though a type 7 tag could interrupt a paragraph. A line carrying the run's
/// quote markers is written inside the run's containers; a lazy one keeps its
/// own prefix and leaves the quotes it lacks, as the blank line above it does,
/// and so does a line holding an element tag, which closes its element where
/// its own indentation stands. A heading's first line is never a tag line, and
/// every tag line under a setext heading's text moves, so written, below its
/// underline, where the heading has ended.
fn run_tag_line_edits(src: &str, run: &Run) -> Vec<Edit> {
    let escaped = run.start > 0 && src.as_bytes()[run.start - 1] == b'\\';
    let first = SrcLine::at(src, run.start - usize::from(escaped));
    let quotes = |p: &str| p.bytes().filter(|&b| b == b'>').count();
    let mut edits = Vec::new();
    let mut moved = String::new();
    let mut seen = None;
    for &at in &run.tags {
        let line = SrcLine::at(src, at);
        if seen == Some(line.start) {
            continue;
        }
        seen = Some(line.start);
        let opens = line.start == first.start;
        let leads = if opens {
            at == run.start && run.heading.is_none()
        } else {
            line.prefix.bytes().all(|b| matches!(b, b'>' | b' ' | b'\t'))
        };
        let Some(tags) = html::tag_line(line.content).filter(|_| leads) else {
            continue;
        };
        let element = tags.iter().any(|t| carrier::modeled_tag(t).is_some());
        let cont = if quotes(line.prefix) == quotes(first.prefix) && !element {
            continuation_of(first.prefix)
        } else {
            line.prefix.to_string()
        };
        let split: Vec<&str> = tags.iter().map(|t| &line.content[t.span.clone()]).collect();
        let split = split.join(&format!("\n{}", continuation_of(&cont)));
        let range = line.start..line.end();
        if run.heading.is_some() {
            moved.push_str(&format!("\n{cont}{split}"));
            edits.push(Edit {
                range: range.start - 1..range.end,
                ..Edit::default()
            });
            continue;
        }
        let with = if opens {
            format!("{}{split}", line.prefix)
        } else {
            format!("{}\n{cont}{split}", blank_of(line.prefix))
        };
        if with != src[range.clone()] {
            edits.push(Edit {
                range,
                with,
                ..Edit::default()
            });
        }
        return edits;
    }
    if let Some(end) = run.heading.filter(|_| !moved.is_empty()) {
        let underline = SrcLine::at(src, end - 1).end();
        edits.push(Edit {
            range: underline..underline,
            with: moved,
            ..Edit::default()
        });
    }
    edits
}

fn tag_row_edits(src: &str, rows: &[SrcLine]) -> Vec<Edit> {
    let mut edits = Vec::new();
    let mut k = 0;
    while k < rows.len() {
        let mut to = k + 1;
        while to < rows.len() && rows[to].start == rows[to - 1].end() + 1 {
            to += 1;
        }
        let group = &rows[k..to];
        let (first, last) = (group[0], group[group.len() - 1]);
        let blank = blank_of(first.prefix);
        let mut out = Rows(vec![Row::Blank]);
        for row in group {
            out.tags([row.whole()]);
        }
        let end = last.end();
        edits.push(Edit {
            range: first.start..end,
            with: out.finish(&blank, next_line_continues(src, end), true),
            ..Edit::default()
        });
        k = to;
    }
    edits
}

fn is_container(event: &Event) -> bool {
    matches!(
        event,
        Event::Start(PTag::BlockQuote(_) | PTag::List(_) | PTag::Item)
            | Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item)
    )
}

/// The start of each footnote-shaped definition, the repeats of a label the
/// parser's map omits included: each `[^label]:` opening a line inside its
/// containers where no leaf block or inline event reaches, a definition being
/// the one block the parse emits nothing for.
fn footnote_definitions(src: &str, mut leaves: Vec<Range<usize>>) -> Vec<usize> {
    leaves.sort_by_key(|r| r.start);
    let mut reach = Vec::with_capacity(leaves.len());
    let mut end = 0;
    for r in &leaves {
        end = end.max(r.end);
        reach.push(end);
    }
    let covered = |at: usize| {
        let k = leaves.partition_point(|r| r.start <= at);
        k > 0 && reach[k - 1] > at
    };
    src.match_indices("[^")
        .map(|(at, _)| at)
        .filter(|&at| {
            if covered(at) {
                return false;
            }
            let line = src[..at].rfind('\n').map_or(0, |i| i + 1);
            let lead = src[..at][line..].bytes().all(|b| {
                b.is_ascii_digit() || matches!(b, b'>' | b' ' | b'\t' | b'-' | b'+' | b'*' | b'.' | b')')
            });
            let label = &src[at + 2..];
            let shaped = label
                .find(['[', ']', '\\', '\n'])
                .is_some_and(|i| i > 0 && label[i..].starts_with("]:"));
            lead && shaped
        })
        .collect()
}

/// The `[` escape making a footnote-shaped definition literal, and a blank line
/// after it where the next line would otherwise continue its paragraph.
fn footnote_edits(src: &str, def: Range<usize>) -> Vec<Edit> {
    let mut edits = vec![Edit {
        range: def.start..def.start + 1,
        with: "\\[".to_string(),
        footnote: true,
        ..Edit::default()
    }];
    let end = src[def.end..].find('\n').map_or(src.len(), |i| def.end + i);
    if next_line_continues(src, end) {
        let prefix = SrcLine::at(src, def.start).prefix;
        edits.push(Edit {
            range: end..end,
            with: format!("\n{}", blank_of(prefix)),
            ..Edit::default()
        });
    }
    edits
}

// Applied only to the Markdown body (spec §7): YAML parsing normalizes its own
// scalars but passes the body verbatim, and some Windows/clipboard sources
// leave bare `\r` bytes.
fn normalize_line_endings(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bidi_controls_are_dropped_and_separators_spaced() {
        let cases: &[(&str, &str)] = &[
            ("hello world", "hello world"),
            ("", ""),
            ("**bold** text", "**bold** text"),
            ("intro\u{2028}- item", "intro - item"),
            ("intro\u{2029}= Heading", "intro = Heading"),
            ("intro\u{000B}- item", "intro - item"),
            ("intro\u{000C}- item", "intro - item"),
            ("intro\u{0085}= Heading", "intro = Heading"),
            ("- one\u{000C}\u{000C}two", "- one  two"),
            ("he\u{202D}llo", "hello"),
            ("**asdf** or \u{202D}**(1234**", "**asdf** or **(1234**"),
            ("a\u{200E}b\u{200F}c", "abc"),
            ("\u{202A}text\u{202B}more\u{202C}", "textmore"),
            ("\u{2066}a\u{2067}b\u{2068}c\u{2069}", "abc"),
            (
                "\u{061C}\u{200E}\u{200F}\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}",
                "",
            ),
            ("hello\u{061C}world", "helloworld"),
            ("\u{061C}**bold**", "**bold**"),
            ("你好世界", "你好世界"),
            ("مرحبا", "مرحبا"),
            ("🎉", "🎉"),
        ];

        for (input, expected) in cases {
            assert_eq!(admit_chars(input), *expected, "input: {:?}", input);
        }
    }

    fn normalized(md: &str) -> Repaired {
        normalize_markdown(md, crate::import::options())
    }

    #[test]
    fn test_normalize_markdown_basic() {
        assert_eq!(normalized("hello").text, "hello");
        assert_eq!(normalized("**bold** \u{202D}**more**").text, "**bold** **more**");
    }

    /// Fenced text is code to the parser, so no span the repair edits reaches
    /// into it.
    #[test]
    fn a_fence_is_never_rewritten() {
        for md in [
            "```\n<!-- a --> b\n```",
            "```\n<div>\ntext\n</div>\n```",
            "> ```\n> <div>x\n> [^1]: y\n> ```",
            "<div>\n\n```\n<!-- a --> b\n</div>\n```",
        ] {
            assert_eq!(normalized(md).text, md);
        }
    }

    /// A footnote's offset is minted in the round that escapes it and carried
    /// through every edit a later round lands ahead of it, a definition the
    /// last round frees among them.
    #[test]
    fn a_footnote_offset_follows_the_edits_before_it() {
        let nested: String = (0..REPAIR_ROUNDS).map(|d| format!("{}- <div>\n", "  ".repeat(d))).collect();
        for md in [
            "[^1]: a",
            "<div>\n<div>text\n</div>\n\n> [^1]: a",
            "- <div>\n  > <span>\n  > [^x]: b\n  </div>\n\n[^1]: a",
            &format!("{nested}{}- [^1]: a", "  ".repeat(REPAIR_ROUNDS)),
        ] {
            let r = normalized(md);
            assert!(!r.footnotes.is_empty(), "{md:?}");
            for &at in &r.footnotes {
                assert_eq!(&r.text[at..at + 3], "\\[^", "{md:?} -> {:?}", r.text);
            }
        }
    }
    /// Text split off after a comment keeps its line's indent, read inside an
    /// HTML block the same round frees, so it stays in the list item that
    /// indent continues.
    #[test]
    fn a_split_piece_keeps_its_lines_indent() {
        let cases = [
            ("<div>\n- a\n  <!-- c -->b", "<div>\n\n- a\n  <!-- c -->\n  b"),
            ("<div>\n- a\n\n  <!--\n  c\n  -->b", "<div>\n\n- a\n\n  <!--\n  c\n  -->\n  b"),
        ];
        for (md, repaired) in cases {
            assert_eq!(normalized(md).text, repaired, "{md:?}");
        }
    }
}
