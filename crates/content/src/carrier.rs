//! The `qm-*` carrier (markdown-spec §6.4): the custom elements a markdown
//! spelling rides on where CommonMark has no syntax, their name and attribute
//! grammar, and their canonical spelling.

use crate::normalize::{is_bidi_char, is_line_separator};
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub(crate) mod cell;
pub(crate) mod table;

/// What every carrier tag name opens with.
pub const PREFIX: &str = "qm-";

/// Element names reserved for the table wrapper (`table`) and the anchor
/// spelling (`anchor`), which no stored element carries.
pub const RESERVED: [&str; 2] = ["table", "anchor"];

/// Attribute names outside the grammar besides every `on*`: each is one a
/// downstream HTML renderer acts on.
pub const RESERVED_ATTRS: [&str; 5] = ["style", "class", "id", "href", "src"];

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
/// ASCII-case-insensitively, so the name is read lowercased: `<Qm-Keep>`
/// carries `keep`, and `<qm-a--b>` carries none.
pub fn element(tag_name: &str) -> Option<String> {
    let lower = tag_name.to_ascii_lowercase();
    let name = lower.strip_prefix(PREFIX)?;
    is_element_name(name).then(|| name.to_string())
}

/// Whether `name` is an attribute name: `[a-z][a-z0-9_]*`, neither `on*` nor
/// one of `style`, `class`, `id`, `href`, `src`.
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

/// The element a [`Container::Element`](crate::model::Container::Element)
/// spells, or what the carrier refuses of it: a name outside the grammar or
/// [reserved](RESERVED), or an attribute name outside its grammar, none of
/// which the import models or the wires read.
pub fn modeled(name: &str, attrs: &BTreeMap<String, String>) -> Result<Element, Refused> {
    if RESERVED.contains(&name) {
        return Err(Refused::Reserved(name.to_string()));
    }
    Element::new(name, attrs.clone())
}

/// A name or attribute an [`Element`] refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Not an [element name](is_element_name).
    Name(String),
    /// A [reserved](RESERVED) name, which no stored element carries.
    Reserved(String),
    /// Not an [attribute name](is_attr_name).
    Attr(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Name(n) => write!(f, "`{n}` is not a carrier element name"),
            Refused::Reserved(n) => write!(f, "`{n}` is a reserved carrier name"),
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

    /// The inline pair around `content`, its tags against it on its line.
    pub fn wrap_inline(&self, content: &str) -> String {
        format!("{}{content}{}", self.open_tag(), self.close_tag())
    }

    /// The block wrapper around `children`: each tag alone on its line, a blank
    /// line between it and the children, or the pair on one line around no
    /// children. A container's prefix on each line is the caller's.
    pub fn wrap_block(&self, children: &str) -> String {
        if children.is_empty() {
            return self.wrap_inline("");
        }
        format!("{}\n\n{children}\n\n{}", self.open_tag(), self.close_tag())
    }
}

/// The tags of `line` when it holds only tags that open or close a wrapper:
/// `qm-table` or an element, not self-closing. `qm-anchor` wraps nothing.
pub(crate) fn tag_line(line: &str) -> Option<Vec<crate::html::Tag<'_>>> {
    crate::html::tag_line(line).filter(|tags| {
        tags.iter()
            .all(|t| !t.self_closing && element(t.name).is_some_and(|name| name != "anchor"))
    })
}

pub(crate) fn has_prefix(name: &str) -> bool {
    name.len() >= PREFIX.len() && name.as_bytes()[..PREFIX.len()].eq_ignore_ascii_case(PREFIX.as_bytes())
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
        assert_eq!(element("qm-a-b").as_deref(), Some("a-b"));
        assert_eq!(element("qm-table").as_deref(), Some("table"));
        assert_eq!(element("Qm-A").as_deref(), Some("a"));
        for tag in ["qm-", "qm-a--b", "qm-1", "quill", "keep", "xqm-a"] {
            assert_eq!(element(tag), None, "{tag:?}");
        }
        assert!(RESERVED.iter().all(|r| is_element_name(r)));
    }

    #[test]
    fn attributes_refuse_what_a_renderer_acts_on() {
        for name in ["widths", "align", "a_1", "ref", "x", "name"] {
            assert!(is_attr_name(name), "{name:?}");
        }
        for name in [
            "onclick", "on", "one", "style", "class", "id", "href", "src", "data-x", "_a", "1a", "A", "$name",
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
            "<qm-table align=\"a&amp;b&lt;c&gt;&quot;d'e\" note=\"x&#xA;y&#x202E;&#x7C;\" widths=\"1 2\">"
        );
        assert_eq!(e.close_tag(), "</qm-table>");
        let keep = Element::new("keep", BTreeMap::new()).unwrap();
        assert_eq!(keep.wrap_block("a\n\nb"), "<qm-keep>\n\na\n\nb\n\n</qm-keep>");
        assert_eq!(keep.wrap_block(""), "<qm-keep></qm-keep>");

        assert_eq!(Element::new("A", BTreeMap::new()), Err(Refused::Name("A".into())));
        assert_eq!(
            Element::new("keep", attrs(&[("onclick", "x")])),
            Err(Refused::Attr("onclick".into()))
        );
    }
}
