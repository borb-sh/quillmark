//! The carrier placed where a writer or an author puts it: block wrappers
//! around paragraphs, tables, lists and quotes, written canonically, at top
//! level, in list items and in quotes; inline pairs and anchors in prose and
//! in table cells; a tag in a code span. The import never panics, reports each
//! element once with its count, and the content is the fixed point of a
//! re-import. Separately, an element's open tag carries any
//! attribute values through the import's normalization and a table cell.

use std::collections::BTreeMap;

use proptest::prelude::*;

use crate::carrier::{decode_attrs, element, is_attr_name, Attrs, Element};
use crate::export::to_markdown;
use crate::html;
use crate::import::tests::generate::{contained, counted, table, tally, word, Piece};
use crate::import::{from_markdown, options};
use pulldown_cmark::{Event, Parser};

/// Values draw from every scalar value, control characters, bidi controls and
/// line separators among them, which the canonical spelling writes as
/// references.
fn attrs() -> impl Strategy<Value = BTreeMap<String, String>> {
    let name = "[a-z][a-z0-9_]{0,5}".prop_filter("attribute name", |n| is_attr_name(n));
    let value = prop::collection::vec(any::<char>(), 0..8).prop_map(String::from_iter);
    prop::collection::btree_map(name, value, 0..3)
}

/// An element's open and closing tags and the construct it reports: an
/// element name in the grammar spelled canonically, or a `qm-*` tag name
/// outside the grammar.
#[derive(Debug, Clone)]
struct Carrier {
    open: String,
    close: String,
    reported: String,
    /// What it reports inside a line.
    inline: String,
}

fn carrier() -> impl Strategy<Value = Carrier> {
    let named = (
        prop_oneof![
            Just("keep".to_string()),
            Just("cell".to_string()),
            "[a-z][a-z0-9]{0,3}(-[a-z0-9]{1,3}){0,2}".prop_filter("anchor drops silently", |n| n != "anchor"),
        ],
        attrs(),
    )
        .prop_map(|(name, attrs)| {
            let inline = format!("qm-{name}");
            let e = Element::spelling(name, attrs).unwrap();
            Carrier { open: e.open_tag(), close: e.close_tag(), reported: inline.clone(), inline }
        });
    let outside = prop_oneof![Just("qm-a--b"), Just("qm-"), Just("qm-9")];
    let outside = outside.prop_map(|n| Carrier {
        open: format!("<{n}>"),
        close: format!("</{n}>"),
        reported: n.to_string(),
        inline: n.to_string(),
    });
    prop_oneof![3 => named, 1 => outside]
}

fn anchor() -> impl Strategy<Value = String> {
    any::<String>().prop_map(|r| {
        let e = Element::spelling("anchor", BTreeMap::from([("ref".to_string(), r)])).unwrap();
        format!("{}{}", e.open_tag(), e.close_tag())
    })
}

fn token() -> impl Strategy<Value = Piece> {
    prop_oneof![
        3 => word().prop_map(Piece::text),
        2 => (carrier(), word()).prop_map(|(c, w)| Piece {
            md: format!("{}{w}{}", c.open, c.close),
            reported: vec![c.inline],
            ..Piece::default()
        }),
        1 => (anchor(), word(), any::<bool>()).prop_map(|(a, w, before)| {
            Piece::text(if before { format!("{a}{w}") } else { format!("{w}{a}") })
        }),
        1 => word().prop_map(|w| Piece::text(format!("`<qm-{w}>`"))),
    ]
}

/// One line of tokens.
fn line() -> impl Strategy<Value = Piece> {
    prop::collection::vec(token(), 1..4).prop_map(|t| Piece::join(t, " "))
}

fn paragraph() -> impl Strategy<Value = Piece> {
    prop::collection::vec(line(), 1..4).prop_map(|lines| Piece::join(lines, "\n"))
}

fn leaf() -> impl Strategy<Value = Piece> {
    prop_oneof![3 => paragraph(), 1 => table(token)]
}

/// A block wrapper, written canonically.
fn wrapper(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (carrier(), prop::collection::vec(inner, 1..3)).prop_map(|(c, blocks)| {
        let body = Piece::join(blocks, "\n\n");
        let mut reported = vec![c.reported];
        reported.extend(body.reported);
        Piece {
            md: format!("{}\n\n{}\n\n{}", c.open, body.md, c.close),
            reported,
            ..Piece::default()
        }
    })
}

fn block() -> impl Strategy<Value = Piece> {
    let inside = prop_oneof![3 => leaf(), 1 => contained(leaf()), 1 => wrapper(leaf())];
    prop_oneof![
        2 => leaf(),
        3 => wrapper(inside),
        2 => contained(wrapper(leaf())),
    ]
}

fn document() -> impl Strategy<Value = Piece> {
    prop::collection::vec(block(), 1..4).prop_map(|blocks| Piece::join(blocks, "\n\n"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn the_carrier_imports_reports_and_reimports(doc in document()) {
        let imported = from_markdown(&doc.md).unwrap();
        prop_assert_eq!(imported.content.validate(), Ok(()), "{}", doc.md);

        prop_assert_eq!(counted(&imported.warnings), tally(&doc.reported), "{}", doc.md);

        let back = from_markdown(&to_markdown(&imported.content)).unwrap();
        prop_assert_eq!(&back.content, &imported.content, "{}", doc.md);
    }

    #[test]
    fn an_open_tag_carries_its_attributes_through_the_import(
        name in "[a-z][a-z0-9]{0,3}(-[a-z0-9]{1,3}){0,2}",
        attrs in attrs(),
    ) {
        let e = Element::spelling(name.clone(), attrs.clone()).unwrap();
        let values = attrs.into_iter().map(|(k, v)| (k, v.replace('\0', "\u{FFFD}"))).collect();
        let read = Attrs { values, refused: Vec::new() };

        let open = e.open_tag();
        let tag = html::tag_at(&open, 0).unwrap();
        prop_assert_eq!(tag.span.end, open.len());
        prop_assert_eq!(element(tag.name), Some(name));
        prop_assert_eq!(&decode_attrs(&tag.attrs), &read);

        let row = format!("| h |\n|---|\n| {}word{} |", e.open_tag(), e.close_tag());
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
}
