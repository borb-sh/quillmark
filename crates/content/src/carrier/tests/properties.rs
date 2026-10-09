//! The carrier placed where a writer or an author puts it: block wrappers
//! around paragraphs, tables, lists and quotes, written canonically, at top
//! level, in list items and in quotes; inline pairs and anchors in prose and in table cells; a tag in a
//! code span. The import never panics, reports each element nothing models
//! once with its count, and the content is the fixed point of a re-import.
//! Separately, an element's open tag carries any attribute values through the
//! import's normalization and a table cell.

use std::collections::BTreeMap;

use proptest::prelude::*;

use crate::carrier::{decode_attrs, element, is_attr_name, Attrs, Element};
use crate::export::to_markdown;
use crate::html;
use crate::import::{from_markdown, options, ImportWarning};
use crate::island::IslandType;
use crate::normalize::normalize_markdown;
use pulldown_cmark::{Event, Parser};

/// Generated markdown and the construct each opening tag in it reports.
#[derive(Debug, Clone, Default)]
struct Piece {
    md: String,
    reported: Vec<String>,
    /// The pipe tables a `qm-table` wrapper around it would hold.
    tables: usize,
    /// Holds a block other than a pipe table or a tag line: what keeps a
    /// `qm-table` wrapper around it from folding.
    other: bool,
}

impl Piece {
    fn text(md: String) -> Piece {
        Piece { md, ..Piece::default() }
    }

    fn join(pieces: Vec<Piece>, sep: &str) -> Piece {
        let mut out = Piece::default();
        for (i, p) in pieces.into_iter().enumerate() {
            if i > 0 {
                out.md.push_str(sep);
            }
            out.md.push_str(&p.md);
            out.reported.extend(p.reported);
            out.tables += p.tables;
            out.other |= p.other;
        }
        out
    }
}

fn word() -> impl Strategy<Value = String> {
    "[a-z]{2,5}[0-9]"
}

/// Values draw from every scalar value, control characters, bidi controls and
/// line separators among them, which the canonical spelling writes as
/// references.
fn attrs() -> impl Strategy<Value = BTreeMap<String, String>> {
    let name = "[a-z][a-z0-9_]{0,5}".prop_filter("attribute name", |n| is_attr_name(n));
    let value = prop::collection::vec(any::<char>(), 0..8).prop_map(String::from_iter);
    prop::collection::btree_map(name, value, 0..3)
}

/// A `qm-table` wrapper's valid attributes: `widths` (`None` an `auto`
/// column) and `align`.
#[derive(Debug, Clone)]
struct Layout {
    widths: Option<Vec<Option<u64>>>,
    align: Option<&'static str>,
}

fn layout() -> impl Strategy<Value = Layout> {
    (
        prop::option::of(prop::collection::vec(prop::option::of(1u64..13), 0..5)),
        prop::option::of(prop_oneof![Just("left"), Just("center"), Just("right")]),
    )
        .prop_map(|(widths, align)| Layout { widths, align })
}

impl Layout {
    fn attrs(&self) -> BTreeMap<String, String> {
        let widths = self.widths.as_ref().map(|ws| {
            ws.iter().map(|w| w.map_or("auto".to_string(), |n| n.to_string())).collect::<Vec<_>>().join(" ")
        });
        [
            ("widths", widths),
            ("align", self.align.map(String::from)),
        ]
        .into_iter()
        .filter_map(|(k, v)| Some((k.to_string(), v?)))
        .collect()
    }

    /// The props keys a table of `cols` columns stores: `widths` settled to
    /// `cols`, each default absent.
    fn stored(&self, cols: usize) -> serde_json::Map<String, serde_json::Value> {
        let mut out = serde_json::Map::new();
        if let Some(ws) = &self.widths {
            let mut ws = ws.clone();
            ws.resize(cols, None);
            if ws.iter().any(Option::is_some) {
                out.insert("widths".into(), ws.into());
            }
        }
        if let Some(a) = self.align {
            out.insert("align".into(), a.into());
        }
        out
    }
}

/// An element's open and closing tags and the construct it reports: an
/// element name in the grammar spelled canonically, which the import models,
/// or a `qm-*` tag name outside it. A `qm-table` carries valid layout
/// attributes.
#[derive(Debug, Clone)]
struct Carrier {
    open: String,
    close: String,
    reported: Option<String>,
    /// What it reports inside a line, where no element is modeled.
    inline: String,
    table: bool,
}

fn carrier() -> impl Strategy<Value = Carrier> {
    let named = (
        prop_oneof![
            Just("keep".to_string()),
            "[a-z][a-z0-9]{0,3}(-[a-z0-9]{1,3}){0,2}".prop_filter("anchor drops silently", |n| n != "anchor"),
        ],
        attrs(),
    )
        .prop_map(|(name, attrs)| {
            let inline = format!("qm-{name}");
            let e = Element::new(name, attrs).unwrap();
            Carrier { open: e.open_tag(), close: e.close_tag(), reported: None, inline, table: false }
        });
    let table = layout().prop_map(|layout| {
        let e = Element::new("table", layout.attrs()).unwrap();
        Carrier {
            open: e.open_tag(),
            close: e.close_tag(),
            reported: Some("qm-table".into()),
            inline: "qm-table".into(),
            table: true,
        }
    });
    let outside = prop_oneof![Just("qm-a--b"), Just("qm-"), Just("qm-9")];
    let outside = outside.prop_map(|n| Carrier {
        open: format!("<{n}>"),
        close: format!("</{n}>"),
        reported: Some(n.to_string()),
        inline: n.to_string(),
        table: false,
    });
    prop_oneof![3 => named, 1 => table, 1 => outside]
}

fn anchor() -> impl Strategy<Value = String> {
    any::<String>().prop_map(|r| {
        let e = Element::new("anchor", BTreeMap::from([("ref".to_string(), r)])).unwrap();
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
    prop::collection::vec(token(), 1..4).prop_map(|t| Piece { other: true, ..Piece::join(t, " ") })
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
            Piece { tables: 1, other: false, ..Piece::join(lines, "\n") }
        })
    })
}

fn leaf() -> impl Strategy<Value = Piece> {
    prop_oneof![3 => paragraph(), 1 => table()]
}

/// Blocks in a list item or a quote.
fn contained(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (inner, any::<bool>()).prop_map(|(mut p, list)| {
        p.other = true;
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

/// A block wrapper, written canonically.
fn wrapper(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (carrier(), prop::collection::vec(inner, 1..3)).prop_map(|(c, blocks)| {
        let body = Piece::join(blocks, "\n\n");
        let folds = c.table && body.tables == 1 && !body.other;
        let mut reported: Vec<String> = c.reported.filter(|_| !folds).into_iter().collect();
        reported.extend(body.reported);
        Piece {
            md: format!("{}\n\n{}\n\n{}", c.open, body.md, c.close),
            reported,
            tables: body.tables,
            other: body.other || c.table,
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

fn counted(warnings: &[ImportWarning]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = warnings
        .iter()
        .map(|ImportWarning::DroppedConstruct { construct, count }| (construct.to_string(), *count))
        .collect();
    out.sort();
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn the_carrier_imports_reports_and_reimports(doc in document()) {
        let imported = from_markdown(&doc.md).unwrap();
        prop_assert_eq!(imported.content.validate(), Ok(()), "{}", doc.md);

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
    fn a_table_wrapper_folds_its_layout_and_round_trips(
        cols in 1usize..4,
        layout in layout(),
        before in prop_oneof![Just(""), Just("para\n\n"), Just("- item\n\n"), Just("> quote\n\n")],
        at in 0..3u8,
    ) {
        let row = format!("|{}", " c |".repeat(cols));
        let table = format!("{row}\n|{}\n{row}", "---|".repeat(cols));
        let e = Element::new("table", layout.attrs()).unwrap();
        let wrapped = format!("{}\n\n{table}\n\n{}", e.open_tag(), e.close_tag());
        let placed = match at {
            0 => wrapped,
            1 => prefixed(&wrapped, "- ", "  "),
            _ => prefixed(&wrapped, "> ", "> "),
        };
        let md = format!("{before}{placed}");

        let imported = from_markdown(&md).unwrap();
        prop_assert!(imported.warnings.is_empty(), "{:?}: {}", imported.warnings, md);
        let island = imported.content.islands.iter().find(|i| i.island_type == IslandType::Table).unwrap();
        let stored: serde_json::Map<_, _> = island
            .props
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| ["widths", "align"].contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let expected = layout.stored(cols);
        prop_assert_eq!(&stored, &expected, "{}", md);

        let exported = to_markdown(&imported.content);
        prop_assert_eq!(exported.contains("<qm-table"), !expected.is_empty(), "{}", exported);
        let back = from_markdown(&exported).unwrap();
        prop_assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        prop_assert_eq!(&back.content, &imported.content, "{}\n---\n{}", md, exported);
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

        let block = normalize_markdown(&e.wrap_block("word"), options());
        let tag = html::tag_at(&block, 0).unwrap();
        prop_assert_eq!(&decode_attrs(&tag.attrs), &read, "{:?}", block);

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
