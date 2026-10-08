//! Raw HTML as CommonMark 0.31.2 reads it: §6.6's open and closing tags and
//! §4.6's HTML-block start and end conditions, at the line grain the import's
//! repair works at. Where the two disagree this follows `pulldown_cmark` 0.13,
//! since the repair predicts the blocks that parser builds: a type-1 block ends
//! only at the lowercase closing tag of its own name, and a type-7 tag holds no
//! line ending.

use std::ops::Range;

/// One open or closing tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tag<'a> {
    /// As written; HTML names compare ASCII-case-insensitively.
    pub(crate) name: &'a str,
    /// Each attribute's name and its value as written: quotes included,
    /// entities undecoded, empty for a bare attribute.
    pub(crate) attrs: Vec<(&'a str, &'a str)>,
    pub(crate) closing: bool,
    /// Ends `/>`.
    pub(crate) self_closing: bool,
    /// `<` through `>`, as byte offsets into the scanned text.
    pub(crate) span: Range<usize>,
}

/// The start condition a line meets, in §4.6's numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockKind {
    /// Type 1: `<pre`, `<script`, `<style` or `<textarea`, ending at the line
    /// holding the closing tag carried here.
    Verbatim(&'static str),
    /// Type 2: `<!--`, ending at `-->`.
    Comment,
    /// Type 3: `<?`, ending at `?>`.
    Instruction,
    /// Type 4: `<!` and a letter, ending at `>`.
    Declaration,
    /// Type 5: `<![CDATA[`, ending at `]]>`.
    Cdata,
    /// Type 6: a tag named on [`BLOCK_NAMES`], complete or not. Interrupts a
    /// paragraph and ends at a blank line.
    BlockName,
    /// Type 7: one complete tag of any other name, alone on its line. Cannot
    /// interrupt a paragraph; ends at a blank line.
    Tag,
}

impl BlockKind {
    /// What a line must contain to end a type 1–5 block; `None` for types 6
    /// and 7.
    pub(crate) fn end_marker(self) -> Option<&'static str> {
        match self {
            BlockKind::Verbatim(close) => Some(close),
            BlockKind::Comment => Some("-->"),
            BlockKind::Instruction => Some("?>"),
            BlockKind::Declaration => Some(">"),
            BlockKind::Cdata => Some("]]>"),
            BlockKind::BlockName | BlockKind::Tag => None,
        }
    }
}

/// The type-6 start condition's names.
pub(crate) const BLOCK_NAMES: [&str; 62] = [
    "address", "article", "aside", "base", "basefont", "blockquote", "body", "caption", "center",
    "col", "colgroup", "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset",
    "figcaption", "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3", "h4", "h5",
    "h6", "head", "header", "hr", "html", "iframe", "legend", "li", "link", "main", "menu",
    "menuitem", "nav", "noframes", "ol", "optgroup", "option", "p", "param", "search", "section",
    "summary", "table", "tbody", "td", "tfoot", "th", "thead", "title", "tr", "track", "ul",
];

const VERBATIM: [(&str, &str); 4] = [
    ("pre", "</pre>"),
    ("style", "</style>"),
    ("script", "</script>"),
    ("textarea", "</textarea>"),
];

pub(crate) fn is_block_name(name: &str) -> bool {
    BLOCK_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// Whitespace inside a tag. A line ending counts, so a tag scanned from an
/// inline event or a joined run of lines may span lines.
fn is_tag_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n')
}

fn skip_space(b: &[u8], mut i: usize) -> usize {
    while b.get(i).copied().is_some_and(is_tag_space) {
        i += 1;
    }
    i
}

fn attr_value_end(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        q @ (b'"' | b'\'') => {
            let close = b[i + 1..].iter().position(|&c| c == q)?;
            Some(i + 1 + close + 1)
        }
        _ => {
            let n = b[i..]
                .iter()
                .take_while(|&&c| !is_tag_space(c) && !matches!(c, b'"' | b'\'' | b'=' | b'<' | b'>' | b'`'))
                .count();
            (n > 0).then_some(i + n)
        }
    }
}

/// The complete tag opening at byte `at` of `s`, if one does.
pub(crate) fn tag_at(s: &str, at: usize) -> Option<Tag<'_>> {
    let b = s.as_bytes();
    if b.get(at) != Some(&b'<') {
        return None;
    }
    let mut i = at + 1;
    let closing = b.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    let name_start = i;
    if !b.get(i).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'-') {
        i += 1;
    }
    let name = &s[name_start..i];
    let mut attrs = Vec::new();
    if !closing {
        loop {
            let ws = skip_space(b, i);
            if matches!(b.get(ws), Some(b'/' | b'>')) {
                i = ws;
                break;
            }
            if ws == i {
                return None;
            }
            i = ws;
            let attr_start = i;
            if !b.get(i).is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, b'_' | b':')) {
                return None;
            }
            while b
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b':' | b'-'))
            {
                i += 1;
            }
            let attr = &s[attr_start..i];
            let eq = skip_space(b, i);
            if b.get(eq) == Some(&b'=') {
                let value_start = skip_space(b, eq + 1);
                let value_end = attr_value_end(b, value_start)?;
                attrs.push((attr, &s[value_start..value_end]));
                i = value_end;
            } else {
                attrs.push((attr, ""));
            }
        }
    }
    i = skip_space(b, i);
    let self_closing = !closing && b.get(i) == Some(&b'/');
    if self_closing {
        i += 1;
    }
    (b.get(i) == Some(&b'>')).then(|| Tag {
        name,
        attrs,
        closing,
        self_closing,
        span: at..i + 1,
    })
}

/// The tags of a line holding one or more complete tags and nothing else but
/// whitespace; `None` for any other line.
pub(crate) fn tag_line(line: &str) -> Option<Vec<Tag<'_>>> {
    let b = line.as_bytes();
    let mut tags = Vec::new();
    let mut i = skip_space(b, 0);
    while i < b.len() {
        let tag = tag_at(line, i)?;
        i = skip_space(b, tag.span.end);
        tags.push(tag);
    }
    (!tags.is_empty()).then_some(tags)
}

/// The tags of an HTML block's text that are markup: every tag of a type 6 or 7
/// block, the opening tag of a type 1 block, and none of a type 2–5 block, whose
/// content is not markup.
pub(crate) fn block_tags(text: &str) -> Vec<Tag<'_>> {
    match block_start(text.lines().next().unwrap_or("")) {
        Some(BlockKind::Verbatim(_)) => text.find('<').and_then(|i| tag_at(text, i)).into_iter().collect(),
        Some(kind) if kind.end_marker().is_some() => Vec::new(),
        _ => tags(text),
    }
}

/// Every complete tag in `text`, scanned left to right.
pub(crate) fn tags(text: &str) -> Vec<Tag<'_>> {
    let mut tags = Vec::new();
    let mut i = 0;
    while let Some(off) = text[i..].find('<') {
        match tag_at(text, i + off) {
            Some(tag) => {
                i = tag.span.end;
                tags.push(tag);
            }
            None => i += off + 1,
        }
    }
    tags
}

/// Columns of leading indentation, a tab advancing to the next multiple of 4.
pub(crate) fn indent_columns(line: &str) -> usize {
    let mut col = 0;
    for c in line.chars() {
        match c {
            ' ' => col += 1,
            '\t' => col += 4 - col % 4,
            _ => break,
        }
    }
    col
}

/// The HTML block `line` opens, its container prefix removed. Type 7's further
/// condition, that the line not continue a paragraph, is the caller's.
pub(crate) fn block_start(line: &str) -> Option<BlockKind> {
    if indent_columns(line) > 3 {
        return None;
    }
    let rest = line.trim_start_matches([' ', '\t']).strip_prefix('<')?;
    let b = rest.as_bytes();
    for (name, close) in VERBATIM {
        if b.len() >= name.len()
            && b[..name.len()].eq_ignore_ascii_case(name.as_bytes())
            && b.get(name.len()).is_none_or(|&c| c.is_ascii_whitespace() || c == b'>')
        {
            return Some(BlockKind::Verbatim(close));
        }
    }
    if rest.starts_with("!--") {
        return Some(BlockKind::Comment);
    }
    if rest.starts_with('?') {
        return Some(BlockKind::Instruction);
    }
    if rest.starts_with("![CDATA[") {
        return Some(BlockKind::Cdata);
    }
    if rest.starts_with('!') && b.get(1).is_some_and(u8::is_ascii_alphabetic) {
        return Some(BlockKind::Declaration);
    }
    let name = rest.strip_prefix('/').unwrap_or(rest);
    let n = name.bytes().take_while(u8::is_ascii_alphanumeric).count();
    if is_block_name(&name[..n]) {
        let after = &name[n..];
        if after.is_empty() || after.starts_with([' ', '\t', '>']) || after.starts_with("/>") {
            return Some(BlockKind::BlockName);
        }
    }
    let at = line.len() - rest.len() - 1;
    let tag = tag_at(line, at)?;
    line[tag.span.end..]
        .trim()
        .is_empty()
        .then_some(BlockKind::Tag)
}

/// The byte offset just past the marker on `line` that ends a block of `kind`,
/// or `None` where the line leaves it open (always, for types 6 and 7).
pub(crate) fn block_end(kind: BlockKind, line: &str) -> Option<usize> {
    let marker = kind.end_marker()?;
    line.find(marker).map(|at| at + marker.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_scan_names_attributes_and_their_closing_forms() {
        let t = tag_at(r#"x <a href="u" title='t' data-n=3 hidden/> y"#, 2).unwrap();
        assert_eq!(t.name, "a");
        assert_eq!(
            t.attrs,
            [("href", r#""u""#), ("title", "'t'"), ("data-n", "3"), ("hidden", "")]
        );
        assert!(!t.closing && t.self_closing);
        assert_eq!(t.span, 2..41);

        let t = tag_at("</quill-table >", 0).unwrap();
        assert!(t.closing);
        assert_eq!(t.name, "quill-table");

        let multi = tag_at("<img src=\"x\"\n     width=\"2\">", 0).unwrap();
        assert_eq!(multi.attrs.len(), 2);

        for not_a_tag in [
            "< a>",
            "<a",
            "<1a>",
            "<a b=>",
            "<a b='c>",
            "<a/ >",
            "<a b=c d=e`>",
            "</a b>",
            "<a\"b\">",
            "<https://x.com>",
        ] {
            assert_eq!(tag_at(not_a_tag, 0), None, "{not_a_tag:?}");
        }
    }

    #[test]
    fn a_tag_line_is_tags_and_whitespace_only() {
        assert_eq!(tag_line("  <p align=center> <img src=x>  ").map(|t| t.len()), Some(2));
        assert_eq!(tag_line("</div>").map(|t| t.len()), Some(1));
        for not_tag_only in ["", "  ", "<span>text", "text <b>", "<!-- c -->", "<a> | <b>"] {
            assert!(tag_line(not_tag_only).is_none(), "{not_tag_only:?}");
        }
    }

    #[test]
    fn block_starts_follow_the_seven_conditions() {
        use BlockKind::*;
        let cases: &[(&str, Option<BlockKind>)] = &[
            ("<pre>", Some(Verbatim("</pre>"))),
            ("<SCRIPT type=x>", Some(Verbatim("</script>"))),
            ("<textarea", Some(Verbatim("</textarea>"))),
            ("<prefix>", Some(Tag)),
            ("<!-- c", Some(Comment)),
            ("<?php", Some(Instruction)),
            ("<!DOCTYPE html>", Some(Declaration)),
            ("<![CDATA[x", Some(Cdata)),
            ("<div>", Some(BlockName)),
            ("</DIV> trailing", Some(BlockName)),
            ("<div class=\"unclosed", Some(BlockName)),
            ("<hr/>", Some(BlockName)),
            ("   <center>", Some(BlockName)),
            ("    <div>", None),
            ("<div-x>", Some(Tag)),
            ("<span>", Some(Tag)),
            ("</quill-table>  ", Some(Tag)),
            ("<span>text", None),
            ("<span><b>", None),
            ("<span", None),
            ("text", None),
        ];
        for (line, kind) in cases {
            assert_eq!(block_start(line), *kind, "{line:?}");
        }
    }

    #[test]
    fn a_block_ends_past_its_marker() {
        assert_eq!(block_end(BlockKind::Comment, "<!-- a --> b"), Some(10));
        assert_eq!(block_end(BlockKind::Comment, "<!-->x"), Some(5));
        assert_eq!(block_end(BlockKind::Verbatim("</pre>"), "x</PRE>"), None);
        assert_eq!(block_end(BlockKind::Verbatim("</pre>"), "x</pre>y"), Some(7));
        assert_eq!(block_end(BlockKind::Declaration, "<!X a>b"), Some(6));
        assert_eq!(block_end(BlockKind::Tag, "</span>"), None);
    }
}
