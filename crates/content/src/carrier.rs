//! The `quill-*` carrier (markdown-spec §6.4): the custom elements a markdown
//! spelling rides on where CommonMark has no syntax, their name and attribute
//! grammar, their canonical spelling, and [`strip`].

use crate::html;
use crate::import::options;
use crate::normalize::{blank_of, is_bidi_char, is_line_separator, normalize_markdown};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::Range;

pub(crate) mod cell;
pub(crate) mod table;

/// What every carrier tag name opens with.
pub const PREFIX: &str = "quill-";

/// Element names reserved for the construct they wrap (`table`, `cell`) and
/// for the anchor spelling (`anchor`): carrier names a quill never declares as
/// its own element.
pub const RESERVED: [&str; 3] = ["table", "cell", "anchor"];

/// Attribute names outside the grammar besides every `on*`: each is one a
/// downstream HTML renderer acts on.
const RESERVED_ATTRS: [&str; 6] = ["style", "class", "id", "href", "src", "name"];

/// Whether `name` is an element name, the part of a tag name after
/// [`PREFIX`]: `[a-z][a-z0-9]*(-[a-z0-9]+)*`.
pub fn is_element_name(name: &str) -> bool {
    name.split('-').enumerate().all(|(i, part)| {
        let mut b = part.bytes();
        b.next()
            .is_some_and(|c| c.is_ascii_lowercase() || (i > 0 && c.is_ascii_digit()))
            && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

/// The element a tag named `tag_name` carries: the name after [`PREFIX`],
/// when it is an [element name](is_element_name). HTML tag names compare
/// ASCII-case-insensitively, so the name is read lowercased: `<Quill-Keep>`
/// carries `keep`, and `<quill-a--b>` carries none.
pub fn element(tag_name: &str) -> Option<String> {
    let lower = tag_name.to_ascii_lowercase();
    let name = lower.strip_prefix(PREFIX)?;
    is_element_name(name).then(|| name.to_string())
}

/// Whether `name` is an attribute name: `[a-z][a-z0-9_]*`, neither `on*` nor
/// one of `style`, `class`, `id`, `href`, `src`, `name`.
pub fn is_attr_name(name: &str) -> bool {
    let mut b = name.bytes();
    b.next().is_some_and(|c| c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        && !name.starts_with("on")
        && !RESERVED_ATTRS.contains(&name)
}

/// A carrier tag's attributes as read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs {
    /// Each attribute's value, entities decoded, by lowercased name.
    pub values: BTreeMap<String, String>,
    /// Each attribute refused, lowercased, in source order: one outside the
    /// [grammar](is_attr_name), or one repeating a name already read.
    pub refused: Vec<String>,
}

/// Read a tag's attributes, each a name and its value as written (quoted,
/// single-quoted, unquoted, or empty for a bare attribute). A value decodes
/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;` and decimal or hex numeric
/// references; any other `&` is text.
pub fn decode_attrs(raw: &[(&str, &str)]) -> Attrs {
    let mut attrs = Attrs::default();
    for &(name, value) in raw {
        let name = name.to_ascii_lowercase();
        if !is_attr_name(&name) || attrs.values.contains_key(&name) {
            attrs.refused.push(name);
            continue;
        }
        let unquoted = match value.as_bytes() {
            [q @ (b'"' | b'\''), .., last] if q == last => &value[1..value.len() - 1],
            _ => value,
        };
        attrs.values.insert(name, decode_entities(unquoted));
    }
    attrs
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match entity(rest) {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character the reference opening `s` spells, and its length: one of the
/// five XML entities, or a numeric reference of at most 7 decimal or 6 hex
/// digits naming a Unicode scalar value.
fn entity(s: &str) -> Option<(char, usize)> {
    for (name, c) in [("&amp;", '&'), ("&lt;", '<'), ("&gt;", '>'), ("&quot;", '"'), ("&apos;", '\'')] {
        if s.starts_with(name) {
            return Some((c, name.len()));
        }
    }
    let num = s.strip_prefix("&#")?;
    let (digits, radix, max, lead) = match num.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16, 6, 3),
        None => (num, 10, 7, 2),
    };
    let n = digits.bytes().take_while(|b| (*b as char).is_digit(radix)).count();
    if n == 0 || n > max || digits.as_bytes().get(n) != Some(&b';') {
        return None;
    }
    let c = char::from_u32(u32::from_str_radix(&digits[..n], radix).ok()?)?;
    Some((c, lead + n + 1))
}

/// The element `tag` opens or closes, where the import models it: an element
/// name the carrier does not [reserve](RESERVED), on a tag that is not
/// self-closing.
pub(crate) fn modeled_tag(tag: &html::Tag) -> Option<String> {
    let name = element(tag.name).filter(|n| !RESERVED.contains(&n.as_str()))?;
    (!tag.self_closing).then_some(name)
}

/// The element a [`Container::Element`](crate::model::Container::Element) or
/// [`MarkKind::Element`](crate::model::MarkKind::Element) spells: `None` for a
/// name outside the grammar or [reserved](RESERVED), or an attribute name
/// outside its grammar, none of which the import models.
pub fn modeled(name: &str, attrs: &BTreeMap<String, String>) -> Option<Element> {
    if RESERVED.contains(&name) {
        return None;
    }
    Element::new(name, attrs.clone()).ok()
}

/// A name or attribute an [`Element`] refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Not an [element name](is_element_name).
    Name(String),
    /// Not an [attribute name](is_attr_name).
    Attr(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Name(n) => write!(f, "`{n}` is not a carrier element name"),
            Refused::Attr(a) => write!(f, "`{a}` is not a carrier attribute name"),
        }
    }
}

impl std::error::Error for Refused {}

/// One carrier element, its name and attributes in the grammar, and its
/// canonical spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    name: String,
    attrs: BTreeMap<String, String>,
}

impl Element {
    /// Refuses a name or an attribute name outside the grammar.
    pub fn new(name: impl Into<String>, attrs: BTreeMap<String, String>) -> Result<Self, Refused> {
        let name = name.into();
        if !is_element_name(&name) {
            return Err(Refused::Name(name));
        }
        if let Some(attr) = attrs.keys().find(|a| !is_attr_name(a)) {
            return Err(Refused::Attr(attr.clone()));
        }
        Ok(Element { name, attrs })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn attrs(&self) -> &BTreeMap<String, String> {
        &self.attrs
    }

    /// The open tag: attributes sorted by name, each value double-quoted with
    /// `&`, `<`, `>` and `"` as entities. A `|`, which would end a table cell,
    /// and a control character, bidi control or line separator, which the
    /// import's normalization would rewrite or which would end the tag's line,
    /// are hex references.
    pub fn open_tag(&self) -> String {
        let mut out = format!("<{PREFIX}{}", self.name);
        for (name, value) in &self.attrs {
            let _ = write!(out, " {name}=\"");
            for c in value.chars() {
                match c {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '"' => out.push_str("&quot;"),
                    c if c == '|' || c.is_control() || is_bidi_char(c) || is_line_separator(c) => {
                        let _ = write!(out, "&#x{:X};", c as u32);
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        out.push('>');
        out
    }

    pub fn close_tag(&self) -> String {
        format!("</{PREFIX}{}>", self.name)
    }

    /// The block wrapper around `children`: each tag alone on its line, a blank
    /// line between it and the children. A container's prefix on each line is
    /// the caller's.
    pub fn wrap_block(&self, children: &str) -> String {
        if children.is_empty() {
            return format!("{}\n\n{}", self.open_tag(), self.close_tag());
        }
        format!("{}\n\n{children}\n\n{}", self.open_tag(), self.close_tag())
    }

    /// The inline pair around `inner`, on one line with the text around it.
    pub fn wrap_inline(&self, inner: &str) -> String {
        format!("{}{inner}{}", self.open_tag(), self.close_tag())
    }
}

/// `markdown` without its `quill-*` tags, keeping what a wrapper holds. A tag
/// goes where the import reads it as markup, so one in a code span, a fence, a
/// comment or another tag's attribute stays. A line left holding nothing but
/// container markers is a blank line inside them, and a list item's marker
/// left bare drops the blank lines after it, which would end the item; every
/// other byte is kept.
pub fn strip(markdown: &str) -> String {
    let found = prefixed_tags(markdown);
    if found.is_empty() {
        return markdown.to_string();
    }
    let options = options();
    let mut marked = String::with_capacity(markdown.len() + found.len() * 4);
    let mut at = 0;
    for (k, tag) in found.iter().enumerate() {
        marked.push_str(&markdown[at..tag.name_end]);
        let _ = write!(marked, "{MARK}{k}");
        at = tag.name_end;
    }
    marked.push_str(&markdown[at..]);
    let mut markup = vec![false; found.len()];
    for name in markup_tag_names(&normalize_markdown(&marked, options).text, options) {
        let index = name.rsplit_once(MARK).and_then(|(_, k)| k.parse::<usize>().ok());
        let ours = |&k: &usize| found.get(k).is_some_and(|f| name == format!("{}{MARK}{k}", f.name));
        if let Some(k) = index.filter(ours) {
            markup[k] = true;
        }
    }
    let spans: Vec<Range<usize>> = found
        .iter()
        .zip(markup)
        .filter_map(|(tag, markup)| markup.then(|| tag.span.clone()))
        .collect();
    remove(markdown, &spans)
}

/// Appended to each `quill-*` tag's name, with the tag's index, so the
/// import's own parse of the marked text says which tags it drops. A suffix in
/// the tag-name charset changes no block or inline structure.
const MARK: &str = "-x";

struct Found<'a> {
    name: &'a str,
    span: Range<usize>,
    name_end: usize,
}

fn has_prefix(name: &str) -> bool {
    name.len() >= PREFIX.len() && name.as_bytes()[..PREFIX.len()].eq_ignore_ascii_case(PREFIX.as_bytes())
}

/// Every complete tag in `s` named with [`PREFIX`], markup or not.
fn prefixed_tags(s: &str) -> Vec<Found<'_>> {
    s.match_indices('<')
        .filter_map(|(i, _)| {
            let after = &s[i + 1..];
            let name = after.strip_prefix('/').unwrap_or(after);
            if !has_prefix(name.get(..PREFIX.len())?) {
                return None;
            }
            let tag = html::tag_at(s, i)?;
            let name_end = i + 1 + usize::from(tag.closing) + tag.name.len();
            Some(Found { name: tag.name, span: tag.span, name_end })
        })
        .collect()
}

/// The names of the tags the import drops as markup from `text`, which the
/// repair has made its parse's input.
fn markup_tag_names(text: &str, options: Options) -> Vec<String> {
    let mut names = Vec::new();
    let mut block: Option<String> = None;
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(Tag::HtmlBlock) => block = Some(String::new()),
            Event::Html(h) => {
                if let Some(b) = &mut block {
                    b.push_str(&h);
                }
            }
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(b) = block.take() {
                    names.extend(html::block_tags(&b).iter().map(|t| t.name.to_string()));
                }
            }
            Event::InlineHtml(h) => names.extend(html::tag_at(&h, 0).map(|t| t.name.to_string())),
            _ => {}
        }
    }
    names
}

/// `src` without `spans`. A line a span leaves holding only container markers
/// is a blank line inside them, as the import's repair puts a blank line above
/// a tag line.
fn remove(src: &str, spans: &[Range<usize>]) -> String {
    let line_end = |from: usize| src[from..].find('\n').map_or(src.len(), |i| from + i);
    let mut lines: Vec<String> = Vec::new();
    let mut spans = spans.iter().peekable();
    let mut start = 0;
    let mut after_marker: Option<String> = None;
    loop {
        let mut end = line_end(start);
        let mut cuts = Vec::new();
        while let Some(span) = spans.next_if(|s| s.start < end) {
            end = end.max(line_end(span.end));
            cuts.push(span.clone());
        }
        let line = match cut(src, start..end, &cuts) {
            Cut::Emptied { lead, cr } => format!("{}{cr}", blank_of(lead)),
            Cut::Kept(kept) => kept,
        };
        // A list item opens with at most one empty line, so the blank lines
        // after a marker its tags left bare would end the item.
        let blank = blank_of(&line);
        if !(is_blank(&line) && after_marker.as_ref() == Some(&blank)) {
            after_marker = (!cuts.is_empty() && is_bare_marker(&line)).then_some(blank);
            lines.push(line);
        }
        if end == src.len() {
            return lines.join("\n");
        }
        start = end + 1;
    }
}

enum Cut<'a> {
    /// Nothing but container markers is left: those before the first cut, and
    /// the line's `\r`.
    Emptied { lead: &'a str, cr: &'static str },
    Kept(String),
}

fn cut<'a>(src: &'a str, line: Range<usize>, cuts: &[Range<usize>]) -> Cut<'a> {
    let Some(first) = cuts.first() else {
        return Cut::Kept(src[line].to_string());
    };
    let mut kept = String::new();
    let mut at = line.start;
    for c in cuts {
        kept.push_str(&src[at..c.start]);
        at = c.end;
    }
    kept.push_str(&src[at..line.end]);
    let lead = &src[line.start..first.start];
    if lead.bytes().all(|b| matches!(b, b'>' | b' ' | b'\t')) && kept[lead.len()..].trim().is_empty() {
        let cr = if kept.ends_with('\r') { "\r" } else { "" };
        return Cut::Emptied { lead, cr };
    }
    Cut::Kept(kept)
}

fn is_blank(line: &str) -> bool {
    line.bytes().all(|b| matches!(b, b'>' | b' ' | b'\t' | b'\r'))
}

/// Container markers ending in a list item's marker, and nothing after it.
fn is_bare_marker(line: &str) -> bool {
    let mut words = line.split_whitespace().filter(|w| !w.bytes().all(|b| b == b'>'));
    let is_marker = |w: &str| {
        matches!(w, "-" | "+" | "*")
            || w.strip_suffix(['.', ')'])
                .is_some_and(|n| (1..=9).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
    };
    words.next_back().is_some_and(is_marker) && words.all(is_marker)
}

#[cfg(test)]
mod tests {
    mod properties;

    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn element_names_follow_the_grammar() {
        for name in ["a", "keep", "a-b", "a1-2b", "table", "x9-y-z"] {
            assert!(is_element_name(name), "{name:?}");
        }
        for name in ["", "A", "1a", "a--b", "a-", "-a", "a_b", "a.b", "ké"] {
            assert!(!is_element_name(name), "{name:?}");
        }
        assert_eq!(element("quill-a-b").as_deref(), Some("a-b"));
        assert_eq!(element("quill-table").as_deref(), Some("table"));
        assert_eq!(element("Quill-A").as_deref(), Some("a"));
        for tag in ["quill-", "quill-a--b", "quill-1", "quill", "keep", "xquill-a"] {
            assert_eq!(element(tag), None, "{tag:?}");
        }
        assert!(RESERVED.iter().all(|r| is_element_name(r)));
    }

    #[test]
    fn attributes_refuse_what_a_renderer_acts_on() {
        for name in ["widths", "align", "a_1", "ref", "x"] {
            assert!(is_attr_name(name), "{name:?}");
        }
        for name in [
            "onclick", "on", "one", "style", "class", "id", "href", "src", "name", "data-x", "_a", "1a", "A",
        ] {
            assert!(!is_attr_name(name), "{name:?}");
        }
        let read = decode_attrs(&[
            ("WIDTHS", "\"1 2\""),
            ("onclick", "\"x()\""),
            ("align", "center"),
            ("style", "'c'"),
            ("widths", "\"3\""),
            ("hidden", ""),
        ]);
        assert_eq!(read.values, attrs(&[("widths", "1 2"), ("align", "center"), ("hidden", "")]));
        assert_eq!(read.refused, ["onclick", "style", "widths"]);
    }

    #[test]
    fn values_decode_the_xml_entities_and_numeric_references() {
        let cases: &[(&str, &str)] = &[
            ("\"a &amp; b\"", "a & b"),
            ("\"&#38;&#x26;&#X26;\"", "&&&"),
            ("'&lt;&gt;&quot;&apos;'", "<>\"'"),
            (
                "\"&nbsp; &amp &#; &#x; &#xD800; &#12345678; &#x110000; & x\"",
                "&nbsp; &amp &#; &#x; &#xD800; &#12345678; &#x110000; & x",
            ),
            ("\"&#128512;\"", "😀"),
            ("\"&amp;amp;\"", "&amp;"),
        ];
        for (raw, value) in cases {
            assert_eq!(decode_attrs(&[("v", raw)]).values["v"], *value, "{raw}");
        }
    }

    #[test]
    fn the_open_tag_sorts_its_attributes_and_escapes_their_values() {
        let values = attrs(&[("widths", "1 2"), ("align", "a&b<c>\"d'e"), ("note", "x\ny\u{202E}|")]);
        let e = Element::new("table", values).unwrap();
        assert_eq!(
            e.open_tag(),
            "<quill-table align=\"a&amp;b&lt;c&gt;&quot;d'e\" note=\"x&#xA;y&#x202E;&#x7C;\" widths=\"1 2\">"
        );
        assert_eq!(e.close_tag(), "</quill-table>");
        assert_eq!(e.wrap_inline("x"), format!("{}x</quill-table>", e.open_tag()));
        let keep = Element::new("keep", BTreeMap::new()).unwrap();
        assert_eq!(keep.wrap_block("a\n\nb"), "<quill-keep>\n\na\n\nb\n\n</quill-keep>");
        assert_eq!(keep.wrap_block(""), "<quill-keep>\n\n</quill-keep>");

        assert_eq!(Element::new("A", BTreeMap::new()), Err(Refused::Name("A".into())));
        assert_eq!(
            Element::new("keep", attrs(&[("onclick", "x")])),
            Err(Refused::Attr("onclick".into()))
        );
    }

    #[test]
    fn strip_removes_the_tags_the_import_drops() {
        let cases: &[(&str, &str)] = &[
            ("a <quill-keep>b</quill-keep> c", "a b c"),
            ("<quill-keep>\n\npara\n\n</quill-keep>", "\n\npara\n\n"),
            ("> <quill-keep>\n> para\n> </quill-keep>", ">\n> para\n>"),
            ("> a\n<quill-keep>\n> b", "> a\n\n> b"),
            ("> a\n> <quill-keep>\n> b", "> a\n>\n> b"),
            ("x `<quill-x>` y", "x `<quill-x>` y"),
            ("```\n<quill-x>\n```", "```\n<quill-x>\n```"),
            ("<quill-keep>\n```\n<quill-x>\n```\n</quill-keep>", "\n```\n<quill-x>\n```\n"),
            ("<div>\nsome <quill-x>y</quill-x>\n</div>", "<div>\nsome y\n</div>"),
            ("<!-- <quill-x> -->", "<!-- <quill-x> -->"),
            ("<span title=\"<quill-x>\">t</span>", "<span title=\"<quill-x>\">t</span>"),
            ("| <quill-cell align=\"right\">1</quill-cell> |", "| 1 |"),
            ("- <quill-keep>\n\n  para\n\n  </quill-keep>", "- \n  para\n\n"),
            ("w<quill-anchor ref=\"c1\"></quill-anchor>\r\n<QUILL-A>\r\nv", "w\r\n\r\nv"),
            ("<quill-table\n  widths=\"1 2\">\n| a |\n|---|", "\n| a |\n|---|"),
            ("- <quill-x></quill-x>\n  text", "- \n  text"),
        ];
        for (md, stripped) in cases {
            assert_eq!(strip(md), *stripped, "{md:?}");
        }
    }
}
