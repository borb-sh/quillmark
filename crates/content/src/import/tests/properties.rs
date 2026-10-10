//! The import over raw HTML placed where authors put it: tag lines wrapping
//! markdown at top level, in list items and in quotes, with the blank lines
//! CommonMark wants; tags inline in prose and in cells; comments with text after them. The import
//! never panics, every word of prose outside a comment reaches the content,
//! each dropped opening tag is counted once (a `qm-keep` tag line only when
//! left unclosed), and the content is the fixed point of a re-import.

use proptest::prelude::*;

use super::generate::{contained, counted, prefixed, table, tally, word, Piece};
use crate::export::to_markdown;
use crate::import::from_markdown;
use crate::model::Normalized;

fn inline() -> impl Strategy<Value = Piece> {
    let tagged = (
        word(),
        prop_oneof![
            Just(("span", "")),
            Just(("b", " class=\"x\"")),
            Just(("font", " color=red")),
            Just(("qm-keep", "")),
        ],
    )
        .prop_map(|(w, (name, attrs))| Piece {
            md: format!("<{name}{attrs}>{w}</{name}>"),
            words: vec![w],
            reported: vec![name.to_string()],
            ..Piece::default()
        });
    prop_oneof![
        3 => word().prop_map(Piece::word),
        2 => tagged,
        1 => word().prop_map(|w| Piece { md: format!("<u>{w}</u>"), words: vec![w], ..Piece::default() }),
        1 => (word(), word()).prop_map(|(a, b)| Piece {
            md: format!("{a}<br>{b}"),
            words: vec![a, b],
            ..Piece::default()
        }),
        1 => word().prop_map(|w| Piece {
            md: format!("<qm-anchor id=\"a\">{w}</qm-anchor>"),
            words: vec![w],
            ..Piece::default()
        }),
        1 => word().prop_map(|w| Piece {
            md: format!("{w}<img src=\"p.png\">"),
            words: vec![w],
            reported: vec!["img".into()],
            ..Piece::default()
        }),
    ]
}

fn paragraph() -> impl Strategy<Value = Piece> {
    prop::collection::vec(
        prop::collection::vec(inline(), 1..4).prop_map(|p| Piece::join(p, " ")),
        1..3,
    )
    .prop_map(|lines| Piece { other: true, ..Piece::join(lines, "\n") })
}

fn cell() -> impl Strategy<Value = Piece> {
    prop_oneof![
        word().prop_map(Piece::word),
        word().prop_map(|w| Piece {
            md: format!("<span>{w}</span>"),
            words: vec![w],
            reported: vec!["span".into()],
            ..Piece::default()
        }),
    ]
}

/// A comment on one line or several, the text after it on its last line
/// importing.
fn comment() -> impl Strategy<Value = Piece> {
    (prop::collection::vec(word(), 1..3), any::<bool>(), prop::option::of(paragraph())).prop_map(
        |(hidden, multiline, tail)| {
            // Text after the comment's line-closing `-->` is a paragraph of its
            // own.
            let mut p = tail.unwrap_or_default();
            let sep = if multiline { "\n" } else { " " };
            p.md = format!("<!--{sep}{}{sep}-->{}", hidden.join(" "), p.md);
            p
        },
    )
}

fn leaf() -> impl Strategy<Value = Piece> {
    prop_oneof![3 => paragraph(), 1 => table(cell), 1 => comment()]
}

/// Tag lines around blocks with a blank line on either side, the closing tag
/// sometimes missing.
fn wrapper(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (
        prop_oneof![
            Just(("div", " align=\"center\"")),
            Just(("center", "")),
            Just(("details", "")),
            Just(("p", "")),
            Just(("section", " class=x")),
            Just(("qm-keep", "")),
            Just(("qm-table", " widths=\"1 2\"")),
            Just(("custom-box", "")),
        ],
        prop::collection::vec((inner, any::<bool>()), 1..3),
        prop::bool::weighted(0.8),
    )
        .prop_map(|((name, attrs), blocks, closed)| {
            // An unclosed `qm-table` takes the next `</qm-table>` as its
            // own; the unit tests hold that case.
            let closed = closed || name == "qm-table";
            let mut md = format!("<{name}{attrs}>\n\n");
            let mut words = Vec::new();
            let mut reported = Vec::new();
            let (mut tables, mut other) = (0, false);
            let mut after_table = false;
            let mut after_tag = false;
            for (i, (block, gap)) in blocks.into_iter().enumerate() {
                // A tag line under paragraph text is the paragraph's inline HTML,
                // and two quotes on adjacent lines are one.
                let first = block.md.trim_start_matches(['>', ' ']);
                let opens = first.starts_with('<') && !first.starts_with("<!--");
                if i > 0 {
                    md.push_str(if gap || after_table || after_tag || opens { "\n\n" } else { "\n" });
                }
                after_table = block.table;
                after_tag = block.closes;
                md.push_str(&block.md);
                words.extend(block.words);
                reported.extend(block.reported);
                tables += block.tables;
                other |= block.other;
            }
            if closed {
                md.push_str(&format!("\n\n</{name}>"));
            }
            let folds = name == "qm-table" && closed && tables == 1 && !other;
            let models = name == "qm-keep" && closed;
            if !folds && !models {
                reported.push(name.to_string());
            }
            // A closing tag straight after a table's rows is one more row to the
            // parser, so the table runs on past it. An unclosed `qm-keep` stays
            // innermost, so a `qm-table` around it never closes.
            Piece {
                md,
                words,
                reported,
                table: after_table && !closed,
                tables,
                other: other || name == "qm-table" || (name == "qm-keep" && !closed),
                closes: closed,
                cell: false,
            }
        })
}

fn block() -> impl Strategy<Value = Piece> {
    let inside = prop_oneof![
        4 => leaf(),
        1 => contained(leaf()),
        1 => wrapper(leaf()),
        1 => contained(wrapper(leaf())),
    ];
    prop_oneof![
        2 => leaf(),
        2 => wrapper(leaf()),
        2 => wrapper(inside),
    ]
}

/// Blocks at top level, in a list item, or in a quote.
fn placed() -> impl Strategy<Value = Piece> {
    (prop::collection::vec(block(), 1..3), 0..3u8).prop_map(|(blocks, at)| {
        let mut p = Piece::join(blocks, "\n\n");
        p.md = match at {
            0 => p.md,
            1 => prefixed(&p.md, "- ", "  "),
            _ => prefixed(&p.md, "> ", "> "),
        };
        p
    })
}

fn document() -> impl Strategy<Value = Piece> {
    prop::collection::vec(placed(), 1..4).prop_map(|items| Piece::join(items, "\n\n"))
}

/// The content's text and every table cell's, where a word can land.
fn imported_text(rt: &Normalized) -> String {
    let mut all = rt.text.clone();
    for island in &rt.islands {
        let cells = ["header", "rows"]
            .iter()
            .filter_map(|k| island.props.get(*k))
            .flat_map(|v| v.as_array().into_iter().flatten())
            .flat_map(|v| match v.as_array() {
                Some(row) => row.clone(),
                None => vec![v.clone()],
            });
        for cell in cells {
            all.push('\n');
            all.push_str(&crate::serial::parse_cell(&cell).0);
        }
    }
    all
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn raw_html_keeps_the_markdown_around_it_and_counts_its_tags(doc in document()) {
        let imported = from_markdown(&doc.md).unwrap();
        prop_assert_eq!(imported.content.validate(), Ok(()), "{}", doc.md);

        let text = imported_text(&imported.content);
        for w in &doc.words {
            prop_assert!(text.contains(w.as_str()), "{w:?} lost from {:?}: {text:?}", doc.md);
        }

        prop_assert_eq!(counted(&imported.warnings), tally(&doc.reported), "{}", doc.md);

        let back = from_markdown(&to_markdown(&imported.content)).unwrap();
        prop_assert_eq!(&back.content, &imported.content, "{}", doc.md);
        prop_assert!(back.warnings.is_empty());
    }

    /// Syntax soup: the repair slices the text it plans over by parser offsets,
    /// so any input reaches a valid content without a panic.
    #[test]
    fn markup_soup_imports(md in r#"([<>/!?=a-z0-9 "'|*`~^:#\[\]\-\t\n]|<div>|</div>|<!--|-->|<pre>|```|> |- |\[\^1\]: |😀){0,60}"#) {
        let imported = from_markdown(&md).unwrap();
        prop_assert_eq!(imported.content.validate(), Ok(()), "{:?}", md);
    }
}
