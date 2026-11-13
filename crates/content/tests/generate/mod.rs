//! The generators the property suites share: markdown documents and the
//! tokens they are made of, table cells, and JSON shaped to reach past a
//! decoder's first branch.

#![allow(dead_code)]

use proptest::prelude::*;
use serde_json::{Value, json};

// A constrained markdown generator: inline tokens are space-separated so the
// properties exercise structure and marks without depending on CommonMark's
// delimiter-adjacency corners, which explicit unit tests pin instead.

// `clean_word` is safe content for inside a formatted span / url / code / cell.
pub fn clean_word() -> impl Strategy<Value = String> {
    "[a-z0-9]{1,6}"
}

// `plain_word` carries inline-special and astral chars as *literal text*, so
// escaping and scalar bounds are exercised by the round-trip. The first char stays
// alphanumeric so a block marker never *leads* the token (`- >` would make
// pulldown build a nested block); the tail carries `&` and the block-marker
// chars.
pub fn plain_word() -> impl Strategy<Value = String> {
    r"[a-z0-9][a-z0-9*_~\\&#>.+😀你-]{0,5}"
}

// `special_alt`/`special_url` carry destination- and markup-terminating chars in
// *markdown source* form. A raw `]`/`[`/`\` would break the markup at the source
// level, so those live only in `image_and_link_specials_round_trip`; here the url
// is angle-wrapped so a space, paren, or `&` reaches the content intact.
pub fn special_alt() -> impl Strategy<Value = String> {
    (clean_word(), r"[a-z0-9&*_~#.+]{0,4}").prop_map(|(a, b)| format!("{a}{b}"))
}

pub fn special_url() -> impl Strategy<Value = String> {
    (clean_word(), r"[a-z0-9 ()&]{1,5}").prop_map(|(a, b)| format!("ex.com/{a}{b}"))
}

// A code span whose content touches its fence: an edge backtick would join the
// fence run and edge spaces would be stripped, so export owes each a space pad.
pub fn code_span() -> impl Strategy<Value = String> {
    prop_oneof![
        clean_word().prop_map(|w| format!("`{w}`")),
        clean_word().prop_map(|w| format!("`` `{w} ``")),
        clean_word().prop_map(|w| format!("`` {w}` ``")),
        clean_word().prop_map(|w| format!("`` `{w}` ``")),
        clean_word().prop_map(|w| format!("`  {w}  `")),
        Just("`` ` ``".to_string()),
    ]
}

pub fn inline_token() -> impl Strategy<Value = String> {
    prop_oneof![
        plain_word(),
        clean_word().prop_map(|w| format!("**{w}**")),
        clean_word().prop_map(|w| format!("_{w}_")),
        clean_word().prop_map(|w| format!("~~{w}~~")),
        code_span(),
        clean_word().prop_map(|w| format!("<u>{w}</u>")),
        clean_word().prop_map(|w| format!("<qm-span family=\"Carlito\">{w}</qm-span>")),
        (clean_word(), clean_word()).prop_map(|(w, v)| format!(
            "<qm-span family=\"Noto Serif\">{w} <qm-span family=\"Carlito\">**{v}**</qm-span></qm-span>"
        )),
        (clean_word(), clean_word()).prop_map(|(t, u)| format!("[{t}](https://ex.com/{u})")),
        (clean_word(), special_url()).prop_map(|(t, u)| format!("[{t}](<{u}>)")),
        (special_alt(), special_url()).prop_map(|(a, u)| format!("![{a}](<{u}>)")),
        // A link over an image: a mark over an island slot.
        (special_alt(), special_url(), clean_word())
            .prop_map(|(a, u, l)| format!("[![{a}](<{u}>)](https://ex.com/{l})")),
        // A footnote reference, read off one of `document()`'s definitions.
        prop::sample::select(vec!["[^a]", "[^B]"]).prop_map(String::from),
        // An equation, inline or display, and a `$` that is text.
        prop::sample::select(vec!["$x^2$", "$\\{a\\}$", "$$\\frac{a}{b}$$", "\\$5", "$5"]).prop_map(String::from),
    ]
}

pub fn prose() -> impl Strategy<Value = String> {
    // Mix single-space and hard-break (`\`+newline) separators, and let some
    // tokens abut, so delimiter-adjacency and hard breaks are covered.
    prop::collection::vec(inline_token(), 1..5).prop_map(|toks| toks.join(" "))
}

// Hard breaks join clean, non-empty *text* lines, and a mark can open on one
// line's last word and close on the next line's first, across the break. An
// empty line adjacent to one is a documented codec limit (the `export` module
// docs), and is not fuzzed here.
pub fn hard_break_line() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(clean_word(), 1..4)
}

pub fn hard_break_prose() -> impl Strategy<Value = String> {
    let wraps = [
        "**{}**",
        "_{}_",
        "~~{}~~",
        "<u>{}</u>",
        "[{}](https://ex.com/u)",
    ];
    (
        prop::collection::vec(hard_break_line(), 2..4),
        prop::option::of((
            any::<prop::sample::Index>(),
            prop::sample::select(wraps.to_vec()),
        )),
    )
        .prop_map(|(lines, wrap)| {
            let mut lines: Vec<String> = lines.into_iter().map(|w| w.join(" ")).collect();
            if let Some((at, wrap)) = wrap {
                let k = at.index(lines.len() - 1);
                let (open, close) = wrap.split_once("{}").unwrap();
                let tail = lines[k].rfind(' ').map_or(0, |i| i + 1);
                lines[k].insert_str(tail, open);
                let head = lines[k + 1].find(' ').unwrap_or(lines[k + 1].len());
                lines[k + 1].insert_str(head, close);
            }
            lines.join("\\\n")
        })
}

pub fn block() -> impl Strategy<Value = String> {
    prop_oneof![
        prose(),
        hard_break_prose(),
        (1u8..=6, prose()).prop_map(|(lvl, p)| format!("{} {p}", "#".repeat(lvl as usize))),
        // Bullet list, some items multi-paragraph (nested blank line).
        prop::collection::vec(prose(), 1..4).prop_map(|items| items
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")),
        prop::collection::vec(prose(), 1..4).prop_map(|items| items
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}. {p}", i + 1))
            .collect::<Vec<_>>()
            .join("\n")),
        // Nested bullet list (two container levels).
        (prose(), prose(), prose()).prop_map(|(a, b, c)| format!("- {a}\n  - {b}\n  - {c}")),
        Just("***".to_string()),
        // A block construct as a list item's own block, which no arm above
        // reaches. Both marker families and both positions, since only one
        // combination collides: a rule as a *bullet* item's *first* block.
        // Spelled `***`, not `---`: `- ---` is four spaced dashes, which imports
        // as a top-level break and would assert the fixed point vacuously.
        (1u8..=6, prose(), prose()).prop_map(|(lvl, h, p)| format!(
            "- {} {h}\n\n  {p}",
            "#".repeat(lvl as usize)
        )),
        prose().prop_map(|p| format!("- ***\n\n  {p}")),
        prose().prop_map(|p| format!("1. ***\n\n   {p}")),
        prose().prop_map(|p| format!("- {p}\n\n  ***")),
        prose().prop_map(|p| format!("> {p}")),
        // Task items: mixed with plain ones, nested, loose, empty, and opening
        // on a block a paragraph line cannot hold.
        (prose(), prose(), prose()).prop_map(|(a, b, c)| format!("- [ ] {a}\n- [x] {b}\n- {c}")),
        (prose(), prose()).prop_map(|(a, b)| format!("1. [x] {a}\n   - [ ] {b}")),
        (prose(), prose()).prop_map(|(a, b)| format!("- [ ] {a}\n\n  {b}")),
        prose().prop_map(|p| format!("- [x] \n- {p}")),
        (1u8..=6, prose()).prop_map(|(lvl, h)| format!("- [ ]\n  {} {h}", "#".repeat(lvl as usize))),
        prop::collection::vec(clean_word(), 1..3)
            .prop_map(|ls| format!("- [x]\n  ```\n  {}\n  ```", ls.join("\n  "))),
        // Two adjacent sibling lists, in each spelling CommonMark reads as a
        // boundary: a bullet-char change, an ordered-delimiter change, and the
        // comment separator, which is the only one that carries a differing
        // `start`. Adjacent *quotes* need no arm — `document()` joins blocks
        // with a blank line, which already ends one.
        (prose(), prose()).prop_map(|(a, b)| format!("- {a}\n\n+ {b}")),
        (prose(), prose()).prop_map(|(a, b)| format!("1. {a}\n\n1) {b}")),
        (prose(), prose()).prop_map(|(a, b)| format!("- {a}\n\n<!-- -->\n\n- {b}")),
        (prose(), prose(), 2u64..9).prop_map(|(a, b, start)| format!(
            "1. {a}\n\n<!-- -->\n\n{start}. {b}"
        )),
        prop::collection::vec(clean_word(), 1..4)
            .prop_map(|ls| format!("```\n{}\n```", ls.join("\n"))),
        // `qm-*` tags the carrier does not read, which drop around what they
        // wrap: at the top level, around a list, in an item, two adjacent
        // pairs, and a pair around nothing.
        prose().prop_map(|p| format!("<qm-keep note=\"x\">\n\n{p}\n\n</qm-keep>")),
        (prose(), prose()).prop_map(|(a, b)| format!("<qm-keep>\n\n- {a}\n- {b}\n\n</qm-keep>")),
        (prose(), prose()).prop_map(|(a, b)| format!("- {a}\n- <qm-keep>\n\n  {b}\n\n  </qm-keep>")),
        (prose(), prose()).prop_map(|(a, b)| format!(
            "<qm-keep>\n\n{a}\n\n</qm-keep>\n\n<qm-keep>\n\n{b}\n\n</qm-keep>"
        )),
        Just("<qm-sig></qm-sig>".to_string()),
        // Definition list spellings, which read as the paragraphs and lists
        // CommonMark reads: tight and loose, two descriptions of one term, two
        // terms, a term across a hard break, an empty description, and a list,
        // a nested definition and a list item each holding one.
        (prose(), prose()).prop_map(|(t, d)| format!("{t}\n: {d}")),
        (prose(), prose(), prose()).prop_map(|(t, a, b)| format!("{t}\n: {a}\n\n  {b}\n: {a}")),
        (prose(), prose(), prose()).prop_map(|(t, d, u)| format!("{t}\n: {d}\n\n{u}\n: {d}")),
        (hard_break_prose(), prose()).prop_map(|(t, d)| format!("{t}\n: {d}")),
        prose().prop_map(|t| format!("{t}\n:")),
        (prose(), prose(), prose()).prop_map(|(t, a, b)| format!("{t}\n: {a}\n  - {b}\n  - {a}")),
        (prose(), prose(), prose()).prop_map(|(t, u, d)| format!("{t}\n: {u}\n  : {d}")),
        (prose(), prose()).prop_map(|(t, d)| format!("- {t}\n  : {d}")),
        (clean_word(), clean_word())
            .prop_map(|(a, b)| format!("| {a} | {b} |\n| --- | --- |\n| 1 | 2 |")),
        (clean_word(), clean_word(), 1u64..9).prop_map(|(a, b, w)| format!(
            "<table align=\"center\" widths=\"{w} auto\">\n<thead>\n<tr><th>{a}</th><th>{b}</th></tr>\n</thead>\n\
             <tbody>\n<tr><td>1</td><td>2</td></tr>\n</tbody>\n</table>"
        )),
        // An HTML table holding a span, in a list item and in a quote, and one
        // with nothing a pipe table cannot write, which re-exports as pipes.
        (prose(), prose(), any::<bool>()).prop_map(|(a, b, quote)| {
            let table = format!("<table>\n<tr><td rowspan=\"2\">{a}</td><td>{b}</td></tr>\n<tr><td>x</td></tr>\n</table>");
            let marker = if quote { "> " } else { "- " };
            let rest = if quote { "> " } else { "  " };
            table.lines().enumerate().map(|(i, l)| format!("{}{l}", if i == 0 { marker } else { rest })).collect::<Vec<_>>().join("\n")
        }),
        (clean_word(), prose()).prop_map(|(a, b)| format!("<table><tr><th>{a}<th>h<tr><td>{b}<td>c</table>")),
    ]
}

pub fn document() -> impl Strategy<Value = String> {
    let note = prop_oneof![
        prose(),
        (prose(), prose()).prop_map(|(a, b)| format!("{a}\n\n    {b}")),
        (prose(), prose()).prop_map(|(a, b)| format!("{a}\n    - {b}")),
    ];
    (prop::collection::vec(block(), 1..6), note.clone(), note)
        .prop_map(|(blocks, a, b)| format!("{}\n\n[^a]: {a}\n\n[^b]: {b}", blocks.join("\n\n")))
}

/// Emphasis delimiters run together with one char from each Unicode class
/// CommonMark's flanking rules sort apart — `¡` and `—` punctuation, `±` a
/// symbol, `౸` a *number* rather than punctuation, `a`, `0` and space neither —
/// so which delimiters match turns on the class beside them. `*¡*x` is four
/// literal chars where `*౸*x` is emphasis. `+` rides along because it is the
/// bullet marker too, so a draw reaches the nested lists whose own markers can
/// spell a thematic break.
pub fn delimiter_run() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "*", "**", "_", "__", "~~", "+", "¡", "—", "±", "౸", "a", "0", " ",
        ]),
        1..12,
    )
    .prop_map(|toks| toks.concat())
}

pub fn fixture_body(name: &str) -> String {
    let path = quillmark_fixtures::resource_path(name);
    std::fs::read_to_string(path).unwrap()
}

// Structured-table property: an editor-built table island whose shape markdown
// import never produces (ragged rows, a short header, `aligns` out of sync) is a
// fixed point of export∘import after normalization. Cell content comes from
// import-produced cells, so representability and canonical marks are guaranteed;
// the entropy is the table *shape* plus in-cell literal specials.

/// A cell's markdown content: clean words, formatted spans, and words carrying
/// literal specials that must escape to survive a pipe cell and round-trip.
pub fn cell_token() -> impl Strategy<Value = String> {
    prop_oneof![
        clean_word(),
        clean_word().prop_map(|w| format!("{w}\\|{w}")), // literal pipe
        clean_word().prop_map(|w| format!("{w}\\`{w}")), // literal backtick
        clean_word().prop_map(|w| format!("{w}\\\\{w}")), // literal backslash
        clean_word().prop_map(|w| format!("{w}你{w}")),  // BMP unicode
        clean_word().prop_map(|w| format!("{w}😀")),     // astral unicode
        clean_word().prop_map(|w| format!("**{w}**")),
        clean_word().prop_map(|w| format!("*{w}*")),
        clean_word().prop_map(|w| format!("~~{w}~~")),
        clean_word().prop_map(|w| format!("`{w}`")),
        (clean_word(), clean_word()).prop_map(|(t, u)| format!("[{t}](https://ex.com/{u})")),
        clean_word().prop_map(|w| format!("{w}<br>{w}")),
        clean_word().prop_map(|w| format!("**{w}<br>{w}**")),
        clean_word().prop_map(|w| format!("<br>{w}")),
        clean_word().prop_map(|w| format!("{w}<br>")),
        clean_word().prop_map(|w| format!("<qm-hl>{w}</qm-hl>")),
    ]
}

pub fn cell_content() -> impl Strategy<Value = String> {
    prop::collection::vec(cell_token(), 1..3).prop_map(|toks| toks.join(" "))
}

pub fn alignment() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just("none"), Just("left"), Just("center"), Just("right")]
}

// The JSON doors: `install` takes a canonical content, `applyChange` takes an
// op wire, and both are reached from a binding with whatever the caller built.
// A panic there traps the WASM module and costs the document rather than the
// operation, so refusal is the property; what a decoded value must preserve
// belongs to the round-trip properties above.

/// The keys the decoders dispatch on, so generated objects reach past the first
/// branch; the noise arm keeps the rest of the space.
pub const DECODE_DISCRIMINATORS: &[&str] = &[
    "text",
    "lines",
    "marks",
    "islands",
    "kind",
    "attrs",
    "op",
    "line",
    "at",
    "delta",
    "ops",
    "retain",
    "insert",
    "islandOps",
    "lineOps",
    "markOps",
    "start",
    "end",
    "container",
    "type",
    "$name",
    "element",
    "instance",
    "containers",
    "continues",
    "id",
    "props",
    "url",
    "level",
    "ordered",
    "ordinal",
    "rows",
    "aligns",
    "widths",
    "align",
    "list_item",
    "quote",
    "table",
    "checked",
    "head",
    "colspan",
    "rowspan",
    "valign",
];

pub fn decode_key() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(DECODE_DISCRIMINATORS).prop_map(str::to_string),
        "\\PC{0,12}",
    ]
}

/// Arbitrary JSON, container-biased: the decoders branch on object keys and
/// array shapes, so a scalar-weighted generator would spend its budget failing
/// at the first `as_object()`.
pub fn decode_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(Value::from),
        decode_key().prop_map(Value::from),
    ];
    leaf.prop_recursive(6, 96, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::hash_map(decode_key(), inner, 0..6)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// A grid as a table's props spell it, either axis possibly empty, a row's
/// cells possibly short, and the layout keys holding anything.
pub fn grid_props() -> impl Strategy<Value = Value> {
    (
        prop::collection::vec("k[0-2]", 0..3),
        prop::collection::vec(("r[0-2]", prop::collection::vec("s[0-3]", 0..3)), 0..3),
        prop::collection::hash_map(
            prop::sample::select(&["head", "aligns", "widths", "align", "cell_attrs"][..]),
            decode_json(),
            0..3,
        ),
    )
        .prop_map(|(columns, rows, layout)| {
            let rows: Vec<Value> = rows
                .into_iter()
                .map(|(id, slots)| {
                    let cells: serde_json::Map<String, Value> = columns
                        .iter()
                        .cloned()
                        .zip(slots.into_iter().map(Value::from))
                        .collect();
                    json!({"id": id, "cells": cells})
                })
                .collect();
            let mut props = serde_json::Map::from_iter([
                ("columns".to_string(), json!(columns)),
                ("rows".to_string(), Value::Array(rows)),
            ]);
            props.extend(layout.into_iter().map(|(k, v)| (k.to_string(), v)));
            Value::Object(props)
        })
}
