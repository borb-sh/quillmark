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
/// the text it splits off a comment opens another.
const REPAIR_ROUNDS: usize = 8;

/// Rewrite `text` so the parse reads the text after a comment's `-->` on its
/// line, ends a table at a line of tags under its rows, and sets a line of
/// carrier tags in an HTML block apart from the markdown beside it
/// (markdown-spec §6.2, §7 step 4). Each round
/// parses and edits only inside the spans that parse located, so a fence is
/// never touched; the rounds end at one that plans no edit.
fn repair(mut text: String, options: Options) -> String {
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

/// Whether some line could open an HTML block or hold a table row of tags: its
/// first character past container markers is `<`. Text failing this has
/// nothing to repair.
fn may_need_repair(s: &str) -> bool {
    s.lines().any(|line| {
        let rest = line.trim_start_matches(|c: char| {
            c.is_ascii_whitespace() || c.is_ascii_digit() || matches!(c, '>' | '-' | '+' | '*' | '.' | ')')
        });
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
    let mut notes = 0usize;
    for (event, range) in Parser::new_ext(src, options).into_offset_iter() {
        match event {
            Event::Start(PTag::FootnoteDefinition(_)) => notes += 1,
            Event::End(TagEnd::FootnoteDefinition) => notes -= 1,
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
                    if notes == 0 {
                        edits.extend(carrier_block_edit(src, &lines));
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

/// The fence character, run length and info string of the code fence `line`
/// opens (CommonMark §4.5): a run of three or more backticks or tildes
/// indented at most three spaces, a backtick fence's info string holding no
/// backtick.
pub fn fence_opener(line: &str) -> Option<(u8, usize, &str)> {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    let t = line.get(indent..).filter(|_| indent <= 3)?;
    let c = t.bytes().next().filter(|c| matches!(c, b'`' | b'~'))?;
    let n = t.bytes().take_while(|&b| b == c).count();
    let info = &t[n..];
    (n >= 3 && (c == b'~' || !info.contains('`'))).then_some((c, n, info))
}

/// The edit splitting a comment's last line after its `-->`, when text follows
/// it, keeping the line's indent; `None` when nothing follows, or when what
/// follows opens a fence or a type 1–5 block it does not close, which would run
/// past the line and so drops with the comment. Every other HTML block drops
/// whole, as CommonMark reads it.
fn comment_edit(src: &str, lines: &[SrcLine]) -> Option<Edit> {
    let (first, last) = (lines.first()?, lines.last()?);
    let kind = html::block_start(first.content).filter(|k| *k == BlockKind::Marked("-->"))?;
    let at = html::block_end(kind, last.content)?;
    let rest = last.content[at..].trim();
    let runs_on = match html::block_start(rest) {
        Some(k) if !k.ends_at_blank_line() => html::block_end(k, rest).is_none(),
        _ => fence_opener(rest).is_some(),
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
/// apart from the markdown lines beside it, where no line of other tags
/// borders the run, so the run reads as tag lines and the markdown as blocks
/// of its own. A line the edit leaves opening a block keeps an indent only shy
/// of an indented code line's, and a blank line closes the block's last
/// markdown where the next line would continue it lazily.
fn carrier_block_edit(src: &str, lines: &[SrcLine]) -> Option<Edit> {
    let (first, last) = (lines.first()?, lines.last()?);
    if !html::block_start(first.content).is_some_and(BlockKind::ends_at_blank_line) {
        return None;
    }
    let tags: Vec<_> = lines.iter().map(|l| html::tag_line(l.content)).collect();
    let carrier: Vec<bool> = tags.iter().map(|t| t.as_ref().is_some_and(|t| t.iter().all(carrier::wraps))).collect();
    let markdown: Vec<bool> = tags.iter().map(Option::is_none).collect();
    let lead = |line: &SrcLine| format!("{}{}{}", line.prefix, shallow_lead(line.content), line.content.trim_start());
    let mut out = Vec::with_capacity(lines.len() + 3);
    let mut changed = false;
    let mut opens = false;
    let mut k = 0;
    while k < lines.len() {
        if !carrier[k] {
            out.push(if std::mem::take(&mut opens) { lead(&lines[k]) } else { lines[k].whole() });
            k += 1;
            continue;
        }
        let mut to = k + 1;
        while to < lines.len() && carrier[to] {
            to += 1;
        }
        let foreign = (k > 0 && !markdown[k - 1]) || (to < lines.len() && !markdown[to]);
        let above = !foreign && k > 0;
        let below = !foreign && to < lines.len();
        if above {
            out.push(blank_of(lines[k].prefix));
        }
        for line in &lines[k..to] {
            out.push(if above || below { lead(line) } else { line.whole() });
        }
        if below {
            out.push(blank_of(lines[to - 1].prefix));
        }
        changed |= above || below;
        opens = below;
        k = to;
    }
    if !changed {
        return None;
    }
    if markdown[lines.len() - 1] && next_line_continues(src, last.end()) {
        out.push(blank_of(last.prefix));
    }
    Some(Edit {
        range: first.start..last.end(),
        with: out.join("\n"),
    })
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

    /// A run of carrier tag lines in an HTML block is set apart from the
    /// markdown lines beside it, inside the containers its lines stand in, and
    /// the block's last markdown from a line that would continue it lazily. A
    /// line left opening a block loses an indented code line's indent. A
    /// carrier line in a paragraph is the import's to read, and a run beside
    /// a line of other tags drops with its block.
    #[test]
    fn a_carrier_line_in_an_html_block_is_set_apart_from_its_markdown() {
        let cases = [
            ("<qm-keep>\nA\n</qm-keep>", "<qm-keep>\n\nA\n\n</qm-keep>"),
            ("<qm-keep>\nA\n    </qm-keep>\nB", "<qm-keep>\n\nA\n\n</qm-keep>\n\nB"),
            ("> <qm-keep>\n> **B**", "> <qm-keep>\n>\n> **B**"),
            ("- <qm-keep>\n  A", "- <qm-keep>\n\n  A"),
            ("<qm-keep>\n<qm-sig></qm-sig>\nA", "<qm-keep>\n<qm-sig></qm-sig>\n\nA"),
            ("- <qm-keep>\n  A\nB", "- <qm-keep>\n\n  A\n\nB"),
            ("> <qm-keep>\n> A\nB", "> <qm-keep>\n>\n> A\n>\nB"),
            (
                "<qm-keep>\n    **Signed**\n    J. Doe\n</qm-keep>",
                "<qm-keep>\n\n**Signed**\n    J. Doe\n\n</qm-keep>",
            ),
        ];
        for (md, repaired) in cases {
            assert_eq!(normalized(md), repaired, "{md:?}");
        }
        for md in [
            "A\n<qm-keep>\nB",
            "<qm-sig></qm-sig>\nA",
            "- [ ] <qm-sig></qm-sig>",
            "<qm-keep>\n<div>\nA",
            "<span>\n<qm-keep>\nA",
            "<div>\nA",
            "<qm-keep>\n<qm-table>\n\nA",
            "[^n]: <qm-keep>\n    A",
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
