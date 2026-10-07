//! The carrier placed where a writer or an author puts it: block wrappers
//! around paragraphs, tables, lists and quotes, written canonically or with no
//! blank line inside them or above them, at top level, in list items and in
//! quotes; inline pairs and anchors in prose and in table cells; a tag in a
//! code span. The import never panics, reads the document as it reads its
//! [`strip`], reports each element nothing models once with its count, and
//! the content is the fixed point of a re-import. Separately, an element's
//! open tag carries any attribute values through the import's normalization
//! and a table cell.

use std::collections::BTreeMap;

use proptest::prelude::*;

use crate::carrier::{decode_attrs, element, is_attr_name, strip, Attrs, Element};
use crate::export::to_markdown;
use crate::html;
use crate::import::{from_markdown, options, ImportWarning};
use crate::normalize::normalize_markdown;
use pulldown_cmark::{Event, Parser};

/// Generated markdown and the construct each opening tag in it reports.
#[derive(Debug, Clone, Default)]
struct Piece {
    md: String,
    reported: Vec<String>,
}

impl Piece {
    fn text(md: String) -> Piece {
        Piece { md, reported: Vec::new() }
    }

    fn join(pieces: Vec<Piece>, sep: &str) -> Piece {
        let mut out = Piece::default();
        for (i, p) in pieces.into_iter().enumerate() {
            if i > 0 {
                out.md.push_str(sep);
            }
            out.md.push_str(&p.md);
            out.reported.extend(p.reported);
        }
        out
    }
}

fn word() -> impl Strategy<Value = String> {
    "[a-z]{2,5}[0-9]"
}

fn attrs() -> impl Strategy<Value = BTreeMap<String, String>> {
    let name = "[a-z][a-z0-9_]{0,5}".prop_filter("attribute name", |n| is_attr_name(n));
    prop::collection::btree_map(name, any::<String>(), 0..3)
}

/// An element's open and closing tags and the construct it reports: an
/// element name in the grammar spelled canonically, or a `quill-*` tag name
/// outside it.
#[derive(Debug, Clone)]
struct Carrier {
    open: String,
    close: String,
    reported: String,
}

fn carrier() -> impl Strategy<Value = Carrier> {
    let named = (
        prop_oneof![
            Just("keep".to_string()),
            Just("table".to_string()),
            Just("cell".to_string()),
            "[a-z][a-z0-9]{0,3}(-[a-z0-9]{1,3}){0,2}".prop_filter("anchor drops silently", |n| n != "anchor"),
        ],
        attrs(),
    )
        .prop_map(|(name, attrs)| {
            let e = Element::new(name.clone(), attrs).unwrap();
            Carrier { open: e.open_tag(), close: e.close_tag(), reported: format!("quill-{name}") }
        });
    let outside = prop_oneof![Just("quill-a--b"), Just("quill-"), Just("Quill-Keep"), Just("quill-9")];
    let outside = outside.prop_map(|n| Carrier {
        open: format!("<{n}>"),
        close: format!("</{n}>"),
        reported: n.to_ascii_lowercase(),
    });
    prop_oneof![4 => named, 1 => outside]
}

fn anchor() -> impl Strategy<Value = String> {
    any::<String>().prop_map(|r| {
        Element::new("anchor", BTreeMap::from([("ref".to_string(), r)])).unwrap().wrap_inline("")
    })
}

fn token() -> impl Strategy<Value = Piece> {
    prop_oneof![
        3 => word().prop_map(Piece::text),
        2 => (carrier(), word()).prop_map(|(c, w)| Piece {
            md: format!("{}{w}{}", c.open, c.close),
            reported: vec![c.reported],
        }),
        1 => (anchor(), word(), any::<bool>()).prop_map(|(a, w, before)| {
            Piece::text(if before { format!("{a}{w}") } else { format!("{w}{a}") })
        }),
        1 => word().prop_map(|w| Piece::text(format!("`<quill-{w}>`"))),
    ]
}

/// One line of tokens, or an anchor alone on its line.
fn line() -> impl Strategy<Value = Piece> {
    prop_oneof![
        5 => prop::collection::vec(token(), 1..4).prop_map(|t| Piece::join(t, " ")),
        1 => anchor().prop_map(Piece::text),
    ]
}

fn paragraph() -> impl Strategy<Value = Piece> {
    prop::collection::vec(line(), 1..4).prop_map(|lines| Piece::join(lines, "\n"))
}

fn table() -> impl Strategy<Value = Piece> {
    (1usize..4).prop_flat_map(|cols| {
        prop::collection::vec(prop::collection::vec(token(), cols), 2..4).prop_map(move |rows| {
            let mut lines: Vec<Piece> = rows
                .into_iter()
                .map(|row| {
                    let mut p = Piece::join(row, " | ");
                    p.md = format!("| {} |", p.md);
                    p
                })
                .collect();
            lines.insert(1, Piece::text(format!("|{}", "---|".repeat(cols))));
            Piece::join(lines, "\n")
        })
    })
}

fn leaf() -> impl Strategy<Value = Piece> {
    prop_oneof![3 => paragraph(), 1 => table()]
}

/// Blocks in a list item or a quote.
fn contained(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (inner, any::<bool>()).prop_map(|(mut p, list)| {
        p.md = if list { prefixed(&p.md, "- ", "  ") } else { prefixed(&p.md, "> ", "> ") };
        p
    })
}

fn prefixed(md: &str, first: &str, rest: &str) -> String {
    md.split('\n')
        .enumerate()
        .map(|(i, line)| {
            let p = if i == 0 { first } else { rest };
            if line.is_empty() { p.trim_end().to_string() } else { format!("{p}{line}") }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A block wrapper: canonical, or without the blank line inside either tag.
fn wrapper(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (carrier(), prop::collection::vec(inner, 1..3), any::<bool>(), any::<bool>()).prop_map(
        |(c, blocks, pad_open, pad_close)| {
            let body = Piece::join(blocks, "\n\n");
            let (a, b) = (if pad_open { "\n\n" } else { "\n" }, if pad_close { "\n\n" } else { "\n" });
            let mut reported = vec![c.reported];
            reported.extend(body.reported);
            Piece { md: format!("{}{a}{}{b}{}", c.open, body.md, c.close), reported }
        },
    )
}

/// A paragraph with a wrapper on the line after it, where CommonMark reads the
/// wrapper's open tag as the paragraph's text.
fn glued() -> impl Strategy<Value = Piece> {
    (paragraph(), wrapper(paragraph())).prop_map(|(p, w)| Piece::join(vec![p, w], "\n"))
}

fn block() -> impl Strategy<Value = Piece> {
    let inside = prop_oneof![3 => leaf(), 1 => contained(leaf()), 1 => wrapper(leaf())];
    prop_oneof![
        2 => leaf(),
        3 => wrapper(inside),
        2 => glued(),
        2 => contained(wrapper(leaf())),
        1 => contained(glued()),
    ]
}

fn document() -> impl Strategy<Value = Piece> {
    prop::collection::vec(block(), 1..4).prop_map(|blocks| Piece::join(blocks, "\n\n"))
}

fn counted(warnings: &[ImportWarning]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = warnings
        .iter()
        .map(|ImportWarning::DroppedConstruct { construct, count }| (construct.clone(), *count))
        .collect();
    out.sort();
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn the_carrier_is_transparent_and_strips_to_the_same_content(doc in document()) {
        let imported = from_markdown(&doc.md).unwrap();
        prop_assert_eq!(imported.content.validate(), Ok(()), "{}", doc.md);

        let stripped = from_markdown(&strip(&doc.md)).unwrap();
        prop_assert_eq!(&stripped.content, &imported.content, "{}\n---\n{}", doc.md, strip(&doc.md));
        prop_assert!(stripped.warnings.is_empty(), "{:?}", stripped.warnings);

        let mut expected: Vec<(String, usize)> = Vec::new();
        for c in &doc.reported {
            match expected.iter_mut().find(|(n, _)| n == c) {
                Some((_, n)) => *n += 1,
                None => expected.push((c.clone(), 1)),
            }
        }
        expected.sort();
        prop_assert_eq!(counted(&imported.warnings), expected, "{}", doc.md);

        let back = from_markdown(&to_markdown(&imported.content)).unwrap();
        prop_assert_eq!(&back.content, &imported.content, "{}", doc.md);
    }

    #[test]
    fn an_open_tag_carries_its_attributes_through_the_import(
        name in "[a-z][a-z0-9]{0,3}(-[a-z0-9]{1,3}){0,2}",
        attrs in attrs(),
    ) {
        let e = Element::new(name.clone(), attrs.clone()).unwrap();
        let read = Attrs { values: attrs, refused: Vec::new() };

        let open = e.open_tag();
        let tag = html::tag_at(&open, 0).unwrap();
        prop_assert_eq!(tag.span.end, open.len());
        prop_assert_eq!(element(tag.name), Some(name));
        prop_assert_eq!(&decode_attrs(&tag.attrs), &read);

        let block = normalize_markdown(&e.wrap_block("word"), options()).text;
        let tag = html::tag_at(&block, 0).unwrap();
        prop_assert_eq!(&decode_attrs(&tag.attrs), &read, "{:?}", block);

        let row = format!("| h |\n|---|\n| {} |", e.wrap_inline("word"));
        let html: Vec<String> = Parser::new_ext(&row, options())
            .filter_map(|ev| match ev {
                Event::InlineHtml(h) => Some(h.to_string()),
                _ => None,
            })
            .collect();
        prop_assert_eq!(html.len(), 2, "{:?}", row);
        prop_assert_eq!(&decode_attrs(&html::tag_at(&html[0], 0).unwrap().attrs), &read);
    }

    #[test]
    fn raw_attributes_decode_without_a_panic(
        raw in prop::collection::vec((
            "[a-zA-Z_:][a-zA-Z0-9_.:-]{0,6}",
            r#"["']?([&#;xX0-9a-f"' ]|&amp;|&#[0-9]{1,8};|&#[xX][0-9a-fA-F]{1,7};|\PC){0,12}["']?"#,
        ), 0..4),
    ) {
        let pairs: Vec<(&str, &str)> = raw.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
        let read = decode_attrs(&pairs);
        prop_assert_eq!(read.values.len() + read.refused.len(), pairs.len());
    }

    #[test]
    fn markup_soup_strips(md in r#"([<>/!=a-z0-9 "'|*`^:#\[\]\-\t\n]|<quill-keep>|</quill-keep>|<quill-anchor ref="r">|<div>|<!--|-->|```|> |- |\r){0,60}"#) {
        let stripped = strip(&md);
        prop_assert_eq!(from_markdown(&stripped).unwrap().content.validate(), Ok(()), "{:?}", stripped);
    }
}
