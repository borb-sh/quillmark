//! The import over raw HTML placed where authors put it: tag lines wrapping
//! markdown at top level, in list items and in quotes, with the blank lines
//! CommonMark wants; tags inline in prose and in cells; comments with text after them. The import
//! never panics, every word of prose outside a comment reaches the content,
//! each dropped opening tag is counted once (a `quill-keep` tag line only when
//! left unclosed), and the content is the fixed point of a re-import.

use proptest::prelude::*;

use crate::export::to_markdown;
use crate::import::{from_markdown, ImportWarning};
use crate::model::Normalized;

/// Generated markdown with the words it must import and the tag names it must
/// count.
#[derive(Debug, Clone, Default)]
struct Piece {
    md: String,
    words: Vec<String>,
    tags: Vec<String>,
    /// Ends in a pipe table, which takes in the lines after it as rows until a
    /// blank line.
    table: bool,
    /// The pipe tables a `quill-table` wrapper around it would hold.
    tables: usize,
    /// Holds a block other than a pipe table, a comment or a tag line: what
    /// keeps a `quill-table` wrapper around it from folding.
    other: bool,
    /// Ends in a closing tag line, whose block takes in the lines after it
    /// until a blank line.
    closes: bool,
}

impl Piece {
    fn join(pieces: Vec<Piece>, sep: &str) -> Piece {
        let mut out = Piece::default();
        for (i, p) in pieces.into_iter().enumerate() {
            if i > 0 {
                out.md.push_str(sep);
            }
            out.md.push_str(&p.md);
            out.words.extend(p.words);
            out.tags.extend(p.tags);
            out.tables += p.tables;
            out.other |= p.other;
        }
        out
    }
}

fn word() -> impl Strategy<Value = String> {
    "[a-z]{2,5}[0-9]"
}

fn inline() -> impl Strategy<Value = Piece> {
    let tagged = (
        word(),
        prop_oneof![
            Just(("span", "")),
            Just(("b", " class=\"x\"")),
            Just(("font", " color=red")),
            Just(("quill-keep", "")),
        ],
    )
        .prop_map(|(w, (name, attrs))| Piece {
            md: format!("<{name}{attrs}>{w}</{name}>"),
            words: vec![w],
            tags: vec![name.to_string()],
            ..Piece::default()
        });
    prop_oneof![
        3 => word().prop_map(|w| Piece { md: w.clone(), words: vec![w], tags: vec![], ..Piece::default() }),
        2 => tagged,
        1 => word().prop_map(|w| Piece { md: format!("<u>{w}</u>"), words: vec![w], tags: vec![], ..Piece::default() }),
        1 => (word(), word()).prop_map(|(a, b)| Piece {
            md: format!("{a}<br>{b}"),
            words: vec![a, b],
            tags: vec![],
            ..Piece::default()
        }),
        1 => word().prop_map(|w| Piece {
            md: format!("<quill-anchor id=\"a\">{w}</quill-anchor>"),
            words: vec![w],
            tags: vec![],
            ..Piece::default()
        }),
        1 => word().prop_map(|w| Piece {
            md: format!("{w}<img src=\"p.png\">"),
            words: vec![w],
            tags: vec!["img".into()],
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
        word().prop_map(|w| Piece { md: w.clone(), words: vec![w], tags: vec![], ..Piece::default() }),
        word().prop_map(|w| Piece {
            md: format!("<span>{w}</span>"),
            words: vec![w],
            tags: vec!["span".into()],
            ..Piece::default()
        }),
    ]
}

fn table() -> impl Strategy<Value = Piece> {
    (1usize..4).prop_flat_map(|cols| {
        prop::collection::vec(prop::collection::vec(cell(), cols), 2..4).prop_map(move |rows| {
            let mut lines: Vec<Piece> = rows
                .into_iter()
                .map(|row| {
                    let mut p = Piece::join(row, " | ");
                    p.md = format!("| {} |", p.md);
                    p
                })
                .collect();
            lines.insert(
                1,
                Piece { md: format!("|{}", "---|".repeat(cols)), ..Piece::default() },
            );
            Piece { table: true, tables: 1, ..Piece::join(lines, "\n") }
        })
    })
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
    prop_oneof![3 => paragraph(), 1 => table(), 1 => comment()]
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
            Just(("quill-keep", "")),
            Just(("quill-table", " widths=\"1 2\"")),
            Just(("custom-box", "")),
        ],
        prop::collection::vec((inner, any::<bool>()), 1..3),
        prop::bool::weighted(0.8),
    )
        .prop_map(|((name, attrs), blocks, closed)| {
            // An unclosed `quill-table` takes the next `</quill-table>` as its
            // own; the unit tests hold that case.
            let closed = closed || name == "quill-table";
            let mut md = format!("<{name}{attrs}>\n\n");
            let mut words = Vec::new();
            let mut tags = Vec::new();
            let (mut tables, mut other) = (0, false);
            let mut after_table = false;
            let mut after_tag = false;
            for (i, (block, gap)) in blocks.into_iter().enumerate() {
                // A tag line under paragraph text is the paragraph's inline HTML.
                let opens = block.md.starts_with('<') && !block.md.starts_with("<!--");
                if i > 0 {
                    md.push_str(if gap || after_table || after_tag || opens { "\n\n" } else { "\n" });
                }
                after_table = block.table;
                after_tag = block.closes;
                md.push_str(&block.md);
                words.extend(block.words);
                tags.extend(block.tags);
                tables += block.tables;
                other |= block.other;
            }
            if closed {
                md.push_str(&format!("\n\n</{name}>"));
            }
            let folds = name == "quill-table" && closed && tables == 1 && !other;
            let models = name == "quill-keep" && closed;
            if !folds && !models {
                tags.push(name.to_string());
            }
            // A closing tag straight after a table's rows is one more row to the
            // parser, so the table runs on past it.
            Piece {
                md,
                words,
                tags,
                table: after_table && !closed,
                tables,
                other: other || name == "quill-table",
                closes: closed,
            }
        })
}

/// Blocks in a list item or a quote.
fn contained(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (inner, any::<bool>()).prop_map(|(mut p, list)| {
        p.other = true;
        p.md = if list { prefixed(&p.md, "- ", "  ") } else { prefixed(&p.md, "> ", "> ") };
        p
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

        let mut counted: Vec<(String, usize)> = imported
            .warnings
            .iter()
            .map(|ImportWarning::DroppedConstruct { construct, count }| (construct.clone(), *count))
            .collect();
        counted.sort();
        let mut expected: Vec<(String, usize)> = Vec::new();
        for t in &doc.tags {
            match expected.iter_mut().find(|(n, _)| n == t) {
                Some((_, c)) => *c += 1,
                None => expected.push((t.clone(), 1)),
            }
        }
        expected.sort();
        prop_assert_eq!(counted, expected, "{}", doc.md);

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
