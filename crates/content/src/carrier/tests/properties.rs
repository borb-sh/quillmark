//! The carrier placed where a writer or an author puts it: block wrappers
//! around paragraphs, tables, lists and quotes, written canonically or with no
//! blank line inside them or above them, at top level, in list items and in
//! quotes; inline pairs and anchors in prose and in table cells; a tag in a
//! code span. The import never panics, reads the document as it reads its
//! [`strip`] but for the keys a `quill-table` around one table and a
//! `quill-cell` around a whole table cell fold, reports each element nothing
//! models once with its count, and the content is the fixed point of a
//! re-import. Separately, an element's open tag carries any attribute values
//! through the import's normalization and a table cell.

use std::collections::BTreeMap;

use proptest::prelude::*;

use crate::carrier::{decode_attrs, element, is_attr_name, strip, Attrs, Element};
use crate::export::to_markdown;
use crate::html;
use crate::import::{from_markdown, options, ImportWarning};
use crate::island::IslandType;
use crate::model::Content;
use crate::normalize::normalize_markdown;
use pulldown_cmark::{Event, Parser};

/// Generated markdown and the construct each opening tag in it reports.
#[derive(Debug, Clone, Default)]
struct Piece {
    md: String,
    reported: Vec<String>,
    /// The pipe tables a `quill-table` wrapper around it would hold.
    tables: usize,
    /// Holds a block other than a pipe table or a tag line: what keeps a
    /// `quill-table` wrapper around it from folding.
    other: bool,
    /// What a `quill-cell` pair reports in place of `reported` when it is a
    /// whole table cell, and folds.
    cell: Option<Vec<String>>,
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

fn attrs() -> impl Strategy<Value = BTreeMap<String, String>> {
    let name = "[a-z][a-z0-9_]{0,5}".prop_filter("attribute name", |n| is_attr_name(n));
    prop::collection::btree_map(name, any::<String>(), 0..3)
}

/// A `quill-table` wrapper's valid attributes: `widths` (`None` an `auto`
/// column), `align`, `breakable`.
#[derive(Debug, Clone)]
struct Layout {
    widths: Option<Vec<Option<u64>>>,
    align: Option<&'static str>,
    breakable: Option<bool>,
}

fn layout() -> impl Strategy<Value = Layout> {
    (
        prop::option::of(prop::collection::vec(prop::option::of(1u64..13), 0..5)),
        prop::option::of(prop_oneof![Just("left"), Just("center"), Just("right")]),
        prop::option::of(any::<bool>()),
    )
        .prop_map(|(widths, align, breakable)| Layout { widths, align, breakable })
}

impl Layout {
    fn attrs(&self) -> BTreeMap<String, String> {
        let widths = self.widths.as_ref().map(|ws| {
            ws.iter().map(|w| w.map_or("auto".to_string(), |n| n.to_string())).collect::<Vec<_>>().join(" ")
        });
        [
            ("widths", widths),
            ("align", self.align.map(String::from)),
            ("breakable", self.breakable.map(|b| b.to_string())),
        ]
        .into_iter()
        .filter_map(|(k, v)| Some((k.to_string(), v?)))
        .collect()
    }

    /// The props keys a table of `cols` columns stores: `widths` settled to
    /// `cols` and divided by its weights' GCD, each default absent.
    fn stored(&self, cols: usize) -> serde_json::Map<String, serde_json::Value> {
        let mut out = serde_json::Map::new();
        if let Some(ws) = &self.widths {
            let mut ws = ws.clone();
            ws.resize(cols, None);
            fn gcd(a: u64, b: u64) -> u64 {
                if b == 0 { a } else { gcd(b, a % b) }
            }
            let gcd = ws.iter().flatten().fold(0, |a, &b| gcd(a, b));
            if gcd > 0 {
                out.insert("widths".into(), ws.iter().map(|w| w.map(|n| n / gcd)).collect::<Vec<_>>().into());
            }
        }
        if let Some(a) = self.align {
            out.insert("align".into(), a.into());
        }
        if self.breakable == Some(false) {
            out.insert("breakable".into(), false.into());
        }
        out
    }
}

/// A `quill-cell` pair's valid attributes.
#[derive(Debug, Clone)]
struct CellKeys {
    align: Option<&'static str>,
    valign: Option<&'static str>,
}

fn cell_keys() -> impl Strategy<Value = CellKeys> {
    (
        prop::option::of(prop_oneof![Just("left"), Just("center"), Just("right")]),
        prop::option::of(prop_oneof![Just("top"), Just("horizon"), Just("bottom")]),
    )
        .prop_map(|(align, valign)| CellKeys { align, valign })
}

impl CellKeys {
    fn attrs(&self) -> BTreeMap<String, String> {
        [("align", self.align), ("valign", self.valign)]
            .into_iter()
            .filter_map(|(k, v)| Some((k.to_string(), v?.to_string())))
            .collect()
    }

    /// The keys a cell in a column aligned `column` stores: `align` absent
    /// where it is the column's, `valign` absent at `top`.
    fn stored(&self, column: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut out = serde_json::Map::new();
        if let Some(a) = self.align.filter(|&a| a != column) {
            out.insert("align".into(), a.into());
        }
        if let Some(v) = self.valign.filter(|&v| v != "top") {
            out.insert("valign".into(), v.into());
        }
        out
    }
}

fn is_cell_key(name: &str, value: &str) -> bool {
    matches!((name, value), ("align", "left" | "center" | "right") | ("valign", "top" | "horizon" | "bottom"))
}

/// An element's open and closing tags and the construct it reports: an
/// element name in the grammar spelled canonically, or a `quill-*` tag name
/// outside it. A `quill-table` carries valid layout attributes, and one
/// `quill-cell` valid alignment attributes.
#[derive(Debug, Clone)]
struct Carrier {
    open: String,
    close: String,
    reported: String,
    table: bool,
    /// For `quill-cell`, what it reports as a whole table cell: each attribute
    /// it cannot fold.
    cell: Option<Vec<String>>,
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
            let cell = (name == "cell").then(|| {
                let unread = attrs.iter().filter(|(k, v)| !is_cell_key(k, v));
                unread.map(|(k, _)| format!("quill-cell[{k}]")).collect()
            });
            let e = Element::new(name.clone(), attrs).unwrap();
            Carrier { open: e.open_tag(), close: e.close_tag(), reported: format!("quill-{name}"), table: false, cell }
        });
    let table = layout().prop_map(|layout| {
        let e = Element::new("table", layout.attrs()).unwrap();
        Carrier { open: e.open_tag(), close: e.close_tag(), reported: "quill-table".into(), table: true, cell: None }
    });
    let cell = cell_keys().prop_map(|keys| {
        let e = Element::new("cell", keys.attrs()).unwrap();
        Carrier {
            open: e.open_tag(),
            close: e.close_tag(),
            reported: "quill-cell".into(),
            table: false,
            cell: Some(Vec::new()),
        }
    });
    let outside = prop_oneof![Just("quill-a--b"), Just("quill-"), Just("Quill-Keep"), Just("quill-9")];
    let outside = outside.prop_map(|n| Carrier {
        open: format!("<{n}>"),
        close: format!("</{n}>"),
        reported: n.to_ascii_lowercase(),
        table: false,
        cell: None,
    });
    prop_oneof![3 => named, 1 => table, 1 => cell, 1 => outside]
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
            cell: c.cell,
            ..Piece::default()
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
        5 => prop::collection::vec(token(), 1..4).prop_map(|t| Piece { other: true, ..Piece::join(t, " ") }),
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
                    let cells = row.into_iter().map(|mut cell| {
                        if let Some(reported) = cell.cell.take() {
                            cell.reported = reported;
                        }
                        cell
                    });
                    let mut p = Piece::join(cells.collect(), " | ");
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

/// A block wrapper: canonical, or without the blank line inside either tag.
fn wrapper(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (carrier(), prop::collection::vec(inner, 1..3), any::<bool>(), any::<bool>()).prop_map(
        |(c, blocks, pad_open, pad_close)| {
            let body = Piece::join(blocks, "\n\n");
            let (a, b) = (if pad_open { "\n\n" } else { "\n" }, if pad_close { "\n\n" } else { "\n" });
            let folds = c.table && body.tables == 1 && !body.other;
            let mut reported: Vec<String> = (!folds).then_some(c.reported).into_iter().collect();
            reported.extend(body.reported);
            Piece {
                md: format!("{}{a}{}{b}{}", c.open, body.md, c.close),
                reported,
                tables: body.tables,
                other: body.other || c.table,
                cell: None,
            }
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

/// `content` with no table layout keys and no cell alignment keys, which
/// `strip` takes with the `quill-table` and `quill-cell` tags.
fn without_layout(content: &Content) -> Content {
    let mut content = content.clone();
    for island in content.islands.iter_mut().filter(|i| i.island_type == IslandType::Table) {
        if let Some(props) = island.props.as_object_mut() {
            for key in ["widths", "align", "breakable"] {
                props.remove(key);
            }
        }
        let unalign = |cell: &mut serde_json::Value| {
            if let Some(cell) = cell.as_object_mut() {
                cell.remove("align");
                cell.remove("valign");
            }
        };
        if let Some(header) = island.props.get_mut("header").and_then(serde_json::Value::as_array_mut) {
            header.iter_mut().for_each(unalign);
        }
        for row in island.props.get_mut("rows").and_then(serde_json::Value::as_array_mut).into_iter().flatten() {
            row.as_array_mut().into_iter().flatten().for_each(unalign);
        }
    }
    content
}

/// A table cell's content: words, marked or not, with edge whitespace a
/// `quill-cell` pair keeps.
fn cell_content() -> impl Strategy<Value = String> {
    let part = (word(), 0..8u8).prop_map(|(w, k)| match k {
        0 => w,
        1 => format!("**{w}**"),
        2 => format!("*{w}*"),
        3 => format!("`{w}`"),
        4 => format!("<u>{w}</u>"),
        5 => format!("~~{w}~~"),
        6 => format!("[{w}](https://e.com)"),
        _ => format!("{w}<br>{w}"),
    });
    let edge = || prop_oneof![Just(""), Just(" "), Just("  ")];
    (edge(), prop::collection::vec(part, 1..3), edge())
        .prop_map(|(lead, parts, trail)| format!("{lead}{}{trail}", parts.join(" ")))
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
        prop_assert_eq!(&*stripped.content, &without_layout(&imported.content), "{}\n---\n{}", doc.md, strip(&doc.md));
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
    fn a_table_wrapper_folds_its_layout_and_round_trips(
        cols in 1usize..4,
        layout in layout(),
        before in prop_oneof![Just(""), Just("para\n\n"), Just("- item\n\n"), Just("> quote\n\n")],
        at in 0..3u8,
        pads in (any::<bool>(), any::<bool>()),
    ) {
        let row = format!("|{}", " c |".repeat(cols));
        let table = format!("{row}\n|{}\n{row}", "---|".repeat(cols));
        let e = Element::new("table", layout.attrs()).unwrap();
        let (a, b) = (if pads.0 { "\n\n" } else { "\n" }, if pads.1 { "\n\n" } else { "\n" });
        let wrapped = format!("{}{a}{table}{b}{}", e.open_tag(), e.close_tag());
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
            .filter(|(k, _)| ["widths", "align", "breakable"].contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let expected = layout.stored(cols);
        prop_assert_eq!(&stored, &expected, "{}", md);

        let exported = to_markdown(&imported.content);
        prop_assert_eq!(exported.contains("<quill-table"), !expected.is_empty(), "{}", exported);
        let back = from_markdown(&exported).unwrap();
        prop_assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        prop_assert_eq!(&back.content, &imported.content, "{}\n---\n{}", md, exported);
    }

    #[test]
    fn a_cell_pair_folds_its_keys_and_round_trips(
        aligns in prop::collection::vec(
            prop_oneof![Just("none"), Just("left"), Just("center"), Just("right")],
            1..4,
        ),
        rows in prop::collection::vec(
            prop::collection::vec((prop::option::of(cell_keys()), cell_content()), 3),
            2..4,
        ),
        at in 0..3u8,
    ) {
        let cols = aligns.len();
        let mut lines: Vec<String> = rows
            .iter()
            .map(|row| {
                let cells: Vec<String> = row[..cols]
                    .iter()
                    .map(|(keys, content)| match keys {
                        Some(keys) => Element::new("cell", keys.attrs()).unwrap().wrap_inline(content),
                        None => content.clone(),
                    })
                    .collect();
                format!("| {} |", cells.join(" | "))
            })
            .collect();
        let delimiter = aligns.iter().map(|a| match *a {
            "left" => ":---",
            "center" => ":---:",
            "right" => "---:",
            _ => "---",
        });
        lines.insert(1, format!("| {} |", delimiter.collect::<Vec<_>>().join(" | ")));
        let table = lines.join("\n");
        let md = match at {
            0 => table,
            1 => prefixed(&table, "- ", "  "),
            _ => prefixed(&table, "> ", "> "),
        };

        let imported = from_markdown(&md).unwrap();
        prop_assert!(imported.warnings.is_empty(), "{:?}: {}", imported.warnings, md);
        let island = imported.content.islands.iter().find(|i| i.island_type == IslandType::Table).unwrap();
        let mut any = false;
        for (r, row) in rows.iter().enumerate() {
            let row_at = if r == 0 { "/header".to_string() } else { format!("/rows/{}", r - 1) };
            for (k, (keys, _)) in row[..cols].iter().enumerate() {
                let cell = island.props.pointer(&format!("{row_at}/{k}")).unwrap().as_object().unwrap();
                let stored: serde_json::Map<_, _> = cell
                    .iter()
                    .filter(|(k, _)| ["align", "valign"].contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let expected = keys.as_ref().map(|keys| keys.stored(aligns[k])).unwrap_or_default();
                any |= !expected.is_empty();
                prop_assert_eq!(&stored, &expected, "{}/{} in {}", row_at, k, md);
            }
        }

        let exported = to_markdown(&imported.content);
        prop_assert_eq!(exported.contains("<quill-cell"), any, "{}", exported);
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
