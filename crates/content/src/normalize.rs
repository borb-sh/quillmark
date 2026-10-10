//! Markdown-string preprocessing at the
//! [`from_markdown`](crate::import::from_markdown) boundary (markdown-spec §7):
//! characters the content cannot hold, then the parser-guided repair.

use crate::carrier;
use crate::html::{self, BlockKind};
use pulldown_cmark::{Event, Options, Parser, Tag as PTag, TagEnd};
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

/// Every markdown normalization in order (spec §7): CRLF → LF, bidi controls
/// dropped and line separators spaced, then [`repair`].
pub(crate) fn normalize_markdown(markdown: &str, options: Options) -> String {
    let cleaned = admit_chars(&normalize_line_endings(markdown));
    repair(cleaned, options)
}

/// Rounds the repair takes at most. A round leaves work for the next only where
/// the text it splits off a comment opens another, or a row of tags it ends a
/// table at holds more than one carrier tag.
const REPAIR_ROUNDS: usize = 8;

/// Rewrite `text` so the parse reads the text after a comment's `-->` on its
/// line, ends a table at a line of tags under its rows, and reads a line of
/// carrier tags alone as tag lines (markdown-spec §6.2, §7 step 4). Each round
/// parses and edits only inside the spans that parse located, so a fence is
/// never touched; the rounds end at one that plans no edit.
fn repair(text: String, options: Options) -> String {
    let mut text = text;
    if may_need_repair(&text) {
        for _ in 0..REPAIR_ROUNDS {
            let edits = plan(&text, options);
            if edits.is_empty() {
                break;
            }
            text = apply(&text, &edits);
        }
    }
    text
}

/// Whether some line could open an HTML block, hold a table row of tags or
/// hold carrier tags alone: its first character past container markers and a
/// task marker is `<`. Text failing this has nothing to repair.
fn may_need_repair(s: &str) -> bool {
    s.lines().any(|line| {
        let rest = line.trim_start_matches(|c: char| {
            c.is_ascii_whitespace() || c.is_ascii_digit() || matches!(c, '>' | '-' | '+' | '*' | '.' | ')')
        });
        let rest = ["[ ]", "[x]", "[X]"]
            .iter()
            .find_map(|task| rest.strip_prefix(task))
            .map_or(rest, str::trim_start);
        rest.starts_with('<')
    })
}

struct Edit {
    range: Range<usize>,
    with: String,
}

fn plan(src: &str, options: Options) -> Vec<Edit> {
    let mut edits: Vec<Edit> = Vec::new();
    let mut block: Option<Vec<usize>> = None;
    let mut row: Option<usize> = None;
    let mut tag_rows: Vec<SrcLine> = Vec::new();
    let mut runs = CarrierRuns::default();
    for (event, range) in Parser::new_ext(src, options).into_offset_iter() {
        runs.read(src, &event, &range);
        match event {
            Event::Start(PTag::HtmlBlock) => block = Some(Vec::new()),
            Event::Html(_) => {
                if let Some(starts) = &mut block {
                    starts.push(range.start);
                }
            }
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(starts) = block.take() {
                    let lines: Vec<SrcLine> = starts.iter().map(|&at| SrcLine::at(src, at)).collect();
                    edits.extend(comment_edit(src, &lines));
                    if runs.notes == 0 {
                        edits.extend(carrier_block_edit(&lines));
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
    edits.extend(tag_row_edits(src, &tag_rows));
    edits.extend(runs.edits());
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    edits
}

/// Apply `edits`, sorted by start, to `src`, skipping one that overlaps an
/// edit already applied (the next round plans it again).
fn apply(src: &str, edits: &[Edit]) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    let mut taken = 0;
    for e in edits {
        if e.range.start < taken {
            continue;
        }
        out.push_str(&src[taken..e.range.start]);
        out.push_str(&e.with);
        taken = e.range.end;
    }
    out.push_str(&src[taken..]);
    out
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
fn blank_of(prefix: &str) -> String {
    let mut s = continuation_of(prefix);
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

fn fence_open(t: &str) -> bool {
    let Some(c) = t.chars().next().filter(|c| matches!(c, '`' | '~')) else {
        return false;
    };
    let n = t.chars().take_while(|&x| x == c).count();
    n >= 3 && (c == '~' || !t[n..].contains('`'))
}

/// The edit splitting a comment's last line after its `-->`, when text follows
/// it, keeping the line's indent; `None` when nothing follows, or when what
/// follows opens a fence or a type 1–5 block it does not close, which would run
/// past the line and so drops with the comment. Every other HTML block drops
/// whole, as CommonMark reads it.
fn comment_edit(src: &str, lines: &[SrcLine]) -> Option<Edit> {
    let (first, last) = (lines.first()?, lines.last()?);
    let kind = html::block_start(first.content).filter(|k| matches!(k, BlockKind::Comment))?;
    let at = html::block_end(kind, last.content)?;
    let rest = last.content[at..].trim();
    let runs_on = match html::block_start(rest) {
        Some(k) if !k.ends_at_blank_line() => html::block_end(k, rest).is_none(),
        _ => fence_open(rest),
    };
    if rest.is_empty() || runs_on {
        return None;
    }
    let prefix = format!("{}{}", continuation_of(last.prefix), shallow_lead(last.content));
    let mut lines = vec![
        format!("{}{}", last.prefix, &last.content[..at]),
        format!("{prefix}{rest}"),
    ];
    let (start, end) = (last.start, last.end());
    if next_line_continues(src, end) {
        lines.push(blank_of(last.prefix));
    }
    let with = lines.join("\n");
    (with != src[start..end]).then_some(Edit { range: start..end, with })
}

/// The whitespace leading `line` when it is shy of an indented code line's
/// four columns, else nothing.
fn shallow_lead(line: &str) -> &str {
    if html::indent_columns(line) > 3 {
        return "";
    }
    &line[..line.len() - line.trim_start().len()]
}

/// The edit setting each run of carrier tag lines in a type 6 or 7 HTML block
/// apart from the markdown lines beside it, each of its tags on a line of its
/// own, so the run reads as a block of tag lines and the markdown as itself.
fn carrier_block_edit(lines: &[SrcLine]) -> Option<Edit> {
    let (first, last) = (lines.first()?, lines.last()?);
    if !html::block_start(first.content).is_some_and(BlockKind::ends_at_blank_line) {
        return None;
    }
    let carrier: Vec<bool> = lines.iter().map(|l| carrier::tag_line(l.content).is_some()).collect();
    let markdown: Vec<bool> = lines.iter().map(|l| html::tag_line(l.content).is_none()).collect();
    let mut out = Vec::with_capacity(lines.len() + 2);
    let mut changed = false;
    let mut k = 0;
    while k < lines.len() {
        if !carrier[k] {
            out.push(lines[k].whole());
            k += 1;
            continue;
        }
        let mut to = k + 1;
        while to < lines.len() && carrier[to] {
            to += 1;
        }
        let above = k > 0 && markdown[k - 1];
        let below = to < lines.len() && markdown[to];
        if above {
            out.push(blank_of(lines[k].prefix));
        }
        for line in &lines[k..to] {
            if above || below {
                let lead = format!("{}{}", line.prefix, shallow_lead(line.content));
                out.extend(one_per_line(line.content, &lead));
            } else {
                out.push(line.whole());
            }
        }
        if below {
            out.push(blank_of(lines[to - 1].prefix));
        }
        changed |= above || below;
        k = to;
    }
    changed.then(|| Edit {
        range: first.start..last.end(),
        with: out.join("\n"),
    })
}

/// Each carrier tag `content` holds on a line of its own, the first behind
/// `lead` and the rest behind its continuation.
fn one_per_line(content: &str, lead: &str) -> Vec<String> {
    let rest = continuation_of(lead);
    carrier::tag_line(content)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, tag)| format!("{}{}", if i == 0 { lead } else { &rest }, &content[tag.span.clone()]))
        .collect()
}

/// The lines of carrier tags alone that the parse reads as inline HTML, in a
/// paragraph or a list item's text: each group of adjacent ones is set apart
/// from the text around it, each tag on a line of its own. A line inside a
/// mark, a link, a heading, a table or a footnote definition is left as it is.
#[derive(Default)]
struct CarrierRuns<'a> {
    groups: Vec<CarrierGroup<'a>>,
    open: Option<CarrierGroup<'a>>,
    prev: Prev,
    /// The end of the last carrier line read: its tags and the spaces between
    /// them arrive before it.
    line_end: usize,
    /// Open marks and links.
    marks: usize,
    /// Open headings, tables and footnote definitions.
    held: usize,
    /// Open footnote definitions.
    notes: usize,
    /// Each open list item's content column.
    items: Vec<usize>,
}

/// What the event before an inline one was.
#[derive(Default)]
enum Prev {
    /// A block's start or end: an inline here opens a run.
    #[default]
    Block,
    /// A task marker, at this range.
    Task(Range<usize>),
    /// A line break, at this range.
    Break(Range<usize>),
    /// A carrier line.
    Carrier,
    Inline,
}

struct CarrierGroup<'a> {
    /// The break ending the text above the group's first line.
    after: Option<Range<usize>>,
    /// The task marker the group's first line shares.
    task: Option<Range<usize>>,
    /// Each line and the prefix its first tag takes.
    lines: Vec<(SrcLine<'a>, String)>,
    /// Whether text continues the run below the group.
    follows: bool,
}

impl<'a> CarrierRuns<'a> {
    fn read(&mut self, src: &'a str, event: &Event, range: &Range<usize>) {
        if matches!(event, Event::InlineHtml(_) | Event::Text(_)) && range.start < self.line_end {
            return;
        }
        match event {
            Event::Start(PTag::Item) => {
                self.items.push(item_column(src, range.start));
                self.block();
            }
            Event::End(TagEnd::Item) => {
                self.items.pop();
                self.block();
            }
            Event::Start(PTag::Table(_) | PTag::Heading { .. }) => {
                self.held += 1;
                self.block();
            }
            Event::End(TagEnd::Table | TagEnd::Heading(_)) => {
                self.held -= 1;
                self.block();
            }
            Event::Start(PTag::FootnoteDefinition(_)) => {
                self.held += 1;
                self.notes += 1;
                self.block();
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                self.held -= 1;
                self.notes -= 1;
                self.block();
            }
            Event::Start(
                PTag::Emphasis | PTag::Strong | PTag::Strikethrough | PTag::Link { .. } | PTag::Image { .. },
            ) => {
                self.marks += 1;
                self.inline();
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image) => {
                self.marks -= 1;
                self.inline();
            }
            Event::TaskListMarker(_) => self.prev = Prev::Task(range.clone()),
            Event::SoftBreak | Event::HardBreak => self.prev = Prev::Break(range.clone()),
            Event::InlineHtml(_) if self.held == 0 && self.marks == 0 && !matches!(self.prev, Prev::Inline) => {
                let line = SrcLine::at(src, range.start);
                if carrier::tag_line(line.content).is_some() {
                    self.carrier_line(src, line);
                } else {
                    self.inline();
                }
            }
            Event::Text(_) | Event::Code(_) | Event::InlineHtml(_) | Event::FootnoteReference(_) => self.inline(),
            _ => self.block(),
        }
    }

    fn carrier_line(&mut self, src: &str, line: SrcLine<'a>) {
        self.line_end = line.end();
        let lead = lead_of(line.prefix, self.items.last().copied().unwrap_or(0));
        match (&mut self.open, &self.prev) {
            (Some(group), Prev::Break(_)) => group.lines.push((line, lead)),
            _ => {
                let (after, task) = match &self.prev {
                    Prev::Break(r) => (Some(r.clone()), None),
                    Prev::Task(r) if r.start >= line.start => (None, Some(r.clone())),
                    _ => (None, None),
                };
                let lead = match &task {
                    Some(task) => continuation_of(&src[line.start..task.start]),
                    None => lead,
                };
                self.open = Some(CarrierGroup { after, task, lines: vec![(line, lead)], follows: false });
            }
        }
        self.prev = Prev::Carrier;
    }

    fn inline(&mut self) {
        if let Some(mut group) = self.open.take() {
            group.follows = true;
            self.groups.push(group);
        }
        self.prev = Prev::Inline;
    }

    fn block(&mut self) {
        self.groups.extend(self.open.take());
        self.prev = Prev::Block;
    }

    fn edits(mut self) -> Vec<Edit> {
        self.block();
        self.groups.iter().map(CarrierGroup::edit).collect()
    }
}

impl CarrierGroup<'_> {
    /// A blank line above the group where text precedes it, below it where
    /// text follows, and each tag on a line of its own. Where the first line
    /// shares a task marker's line, its tags move to the lines under the
    /// marker, which a blank line would end the item at.
    fn edit(&self) -> Edit {
        let (first, last) = (&self.lines[0].0, &self.lines[self.lines.len() - 1].0);
        let mut out = Vec::new();
        let start = match (&self.after, &self.task) {
            (Some(after), _) => {
                out.push(String::new());
                out.push(blank_of(first.prefix));
                after.start
            }
            (None, Some(task)) => {
                out.push(String::new());
                task.end
            }
            (None, None) => first.start,
        };
        for (line, lead) in &self.lines {
            out.extend(one_per_line(line.content, lead));
        }
        if self.follows {
            out.push(blank_of(last.prefix));
        }
        Edit {
            range: start..last.end(),
            with: out.join("\n"),
        }
    }
}

/// Columns `s` spans from a line's start, a tab advancing to the next multiple
/// of 4.
fn columns(s: &str) -> usize {
    s.chars().fold(0, |col, c| if c == '\t' { col + 4 - col % 4 } else { col + 1 })
}

/// The content column of the list item whose marker the line holding byte
/// `at` opens.
fn item_column(src: &str, at: usize) -> usize {
    let line = SrcLine::at(src, at);
    let body = line.content.trim_start_matches([' ', '\t']);
    let marker = body.bytes().take_while(u8::is_ascii_digit).count() + 1;
    let end = columns(&src[line.start..line.end() - body.len()]) + marker;
    let rest = body.get(marker..).unwrap_or("");
    let ws = &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()];
    let pad = columns(&format!("{}{ws}", " ".repeat(end))) - end;
    if ws.len() == rest.len() || pad > 4 {
        end + 1
    } else {
        end + pad
    }
}

/// The prefix a carrier line split off its text takes: its own, unless that
/// indents it an indented code line's four columns past the content of its
/// innermost container, a quote or the list item whose content starts at
/// column `item`, where it takes that content's column.
fn lead_of(prefix: &str, item: usize) -> String {
    let quoted = prefix.rfind('>').map_or(0, |i| {
        let after = i + 1;
        after + usize::from(prefix[after..].starts_with([' ', '\t']))
    });
    let marks = &prefix[..quoted];
    let base = columns(marks).max(item);
    if columns(prefix) < base + 4 {
        return prefix.to_string();
    }
    format!("{marks}{}", " ".repeat(base.saturating_sub(columns(marks))))
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

/// A line holding only tags under a table's rows, which a type 7 tag cannot
/// interrupt, so the parser reads it as one more row: set apart by blank lines,
/// it ends the table and reads as an HTML block of its own.
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
        let mut lines = Vec::with_capacity(2 * group.len() + 1);
        for row in group {
            lines.push(blank.clone());
            lines.push(row.whole());
        }
        let end = last.end();
        if next_line_continues(src, end) {
            lines.push(blank);
        }
        edits.push(Edit {
            range: first.start..end,
            with: lines.join("\n"),
        });
        k = to;
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

    fn normalized(md: &str) -> String {
        normalize_markdown(md, crate::import::options())
    }

    #[test]
    fn test_normalize_markdown_basic() {
        assert_eq!(normalized("hello"), "hello");
        assert_eq!(normalized("**bold** \u{202D}**more**"), "**bold** **more**");
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
            assert_eq!(normalized(md), md);
        }
    }

    /// A line of carrier tags alone is set apart from the text around it, each
    /// tag on a line of its own, inside the containers its line stands in. A
    /// task marker sharing its line keeps the line, and an indent deeper than
    /// an indented code line's is cut to its container's content column.
    #[test]
    fn a_carrier_line_is_set_apart_from_its_text() {
        let cases = [
            ("A\n<qm-keep>\nB", "A\n\n<qm-keep>\n\nB"),
            ("<qm-sig></qm-sig>", "<qm-sig>\n</qm-sig>"),
            ("<qm-keep>\nA\n</qm-keep>", "<qm-keep>\n\nA\n\n</qm-keep>"),
            ("> A\n> <qm-a></qm-a>", "> A\n>\n> <qm-a>\n> </qm-a>"),
            ("- [ ] <qm-sig></qm-sig>", "- [ ]\n  <qm-sig>\n  </qm-sig>"),
            ("A\n    <qm-keep>", "A\n\n<qm-keep>"),
            ("- a\n      <qm-keep>", "- a\n\n  <qm-keep>"),
            ("A\\\n</qm-keep>", "A\n\n</qm-keep>"),
        ];
        for (md, repaired) in cases {
            assert_eq!(normalized(md), repaired, "{md:?}");
        }
        for md in [
            "*a\n<qm-keep>\nb*",
            "A\n<qm-anchor ref=\"x\"></qm-anchor>\nB",
            "A\n<span><qm-keep>",
            "A\n<qm-keep/>",
            "<div>\nA",
            "<qm-keep>\n<qm-table>\n\nA",
        ] {
            assert_eq!(normalized(md), md);
        }
    }

    /// Text split off after a comment keeps its line's indent, so it stays in
    /// the list item that indent continues.
    #[test]
    fn a_split_piece_keeps_its_lines_indent() {
        let cases = [
            ("- a\n\n  <!-- c -->b", "- a\n\n  <!-- c -->\n  b"),
            ("- a\n\n  <!--\n  c\n  -->b", "- a\n\n  <!--\n  c\n  -->\n  b"),
        ];
        for (md, repaired) in cases {
            assert_eq!(normalized(md), repaired, "{md:?}");
        }
    }
}
