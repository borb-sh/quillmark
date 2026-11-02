//! The content property suite: the exact round-trip
//! (`import(export(rt)) == rt`), canonical serialization, diff-import
//! preserving the marks markdown cannot carry, and the JSON doors refusing
//! rather than panicking on what a caller hands them.

use proptest::prelude::*;
use quillmark_content::lines::island::IslandType;
use quillmark_content::lines::delta::diff_import;
use quillmark_content::lines::export::{to_markdown, to_markdown_annotated, to_markdown_checked, ExportLoss};
use quillmark_content::lines::import::from_markdown;
use quillmark_content::lines::model::{Content, Island, Line, LineKind, Mark, MarkKind, Lines};
use serde_json::{json, Value};

/// The mint's fixed point: normalizing again changes nothing.
///
/// Every repair [`Content::normalize`] performs is a strict rewrite — a kind
/// demoted, a flag cleared, a mark split at a line break, a zero-width mark
/// dropped, a row padded — so one
/// equality states the whole canonical form. Paired with `validate`, which
/// reports what the mint cannot repair, it is the oracle over an accepted op:
/// the two together cover the content's invariants.
fn renormalized(rt: &Lines) -> Content {
    let mut again = rt.clone().into_content();
    again.normalize();
    again
}

// A constrained markdown generator: inline tokens are space-separated so the
// properties exercise structure and marks without depending on CommonMark's
// delimiter-adjacency corners, which explicit unit tests pin instead.

// `clean_word` is safe content for inside a formatted span / url / code / cell.
fn clean_word() -> impl Strategy<Value = String> {
    "[a-z0-9]{1,6}"
}

// `plain_word` carries inline-special and astral chars as *literal text*, so
// escaping and scalar bounds are exercised by the round-trip. The first char stays
// alphanumeric so a block marker never *leads* the token (`- >` would make
// pulldown build a nested block); the tail carries `&` and the block-marker
// chars.
fn plain_word() -> impl Strategy<Value = String> {
    r"[a-z0-9][a-z0-9*_~\\&#>.+😀你-]{0,5}"
}

// `special_alt`/`special_url` carry destination- and markup-terminating chars in
// *markdown source* form. A raw `]`/`[`/`\` would break the markup at the source
// level, so those live only in `image_and_link_specials_round_trip`; here the url
// is angle-wrapped so a space, paren, or `&` reaches the content intact.
fn special_alt() -> impl Strategy<Value = String> {
    (clean_word(), r"[a-z0-9&*_~#.+]{0,4}").prop_map(|(a, b)| format!("{a}{b}"))
}

fn special_url() -> impl Strategy<Value = String> {
    (clean_word(), r"[a-z0-9 ()&]{1,5}").prop_map(|(a, b)| format!("ex.com/{a}{b}"))
}

// A code span whose content touches its fence: an edge backtick would join the
// fence run and edge spaces would be stripped, so export owes each a space pad.
fn code_span() -> impl Strategy<Value = String> {
    prop_oneof![
        clean_word().prop_map(|w| format!("`{w}`")),
        clean_word().prop_map(|w| format!("`` `{w} ``")),
        clean_word().prop_map(|w| format!("`` {w}` ``")),
        clean_word().prop_map(|w| format!("`` `{w}` ``")),
        clean_word().prop_map(|w| format!("`  {w}  `")),
        Just("`` ` ``".to_string()),
    ]
}

fn inline_token() -> impl Strategy<Value = String> {
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

fn prose() -> impl Strategy<Value = String> {
    // Mix single-space and hard-break (`\`+newline) separators, and let some
    // tokens abut, so delimiter-adjacency and hard breaks are covered.
    prop::collection::vec(inline_token(), 1..5).prop_map(|toks| toks.join(" "))
}

// Hard breaks join clean, non-empty *text* lines, and a mark can open on one
// line's last word and close on the next line's first, across the break. An
// empty line adjacent to one is a documented codec limit (the `export` module
// docs), and is not fuzzed here.
fn hard_break_line() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(clean_word(), 1..4)
}

fn hard_break_prose() -> impl Strategy<Value = String> {
    let wraps = ["**{}**", "_{}_", "~~{}~~", "<u>{}</u>", "[{}](https://ex.com/u)"];
    (
        prop::collection::vec(hard_break_line(), 2..4),
        prop::option::of((any::<prop::sample::Index>(), prop::sample::select(wraps.to_vec()))),
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

fn block() -> impl Strategy<Value = String> {
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

fn document() -> impl Strategy<Value = String> {
    let note = prop_oneof![
        prose(),
        (prose(), prose()).prop_map(|(a, b)| format!("{a}\n\n    {b}")),
        (prose(), prose()).prop_map(|(a, b)| format!("{a}\n    - {b}")),
    ];
    (prop::collection::vec(block(), 1..6), note.clone(), note).prop_map(|(blocks, a, b)| {
        format!("{}\n\n[^a]: {a}\n\n[^b]: {b}", blocks.join("\n\n"))
    })
}

/// Emphasis delimiters run together with one char from each Unicode class
/// CommonMark's flanking rules sort apart — `¡` and `—` punctuation, `±` a
/// symbol, `౸` a *number* rather than punctuation, `a`, `0` and space neither —
/// so which delimiters match turns on the class beside them. `*¡*x` is four
/// literal chars where `*౸*x` is emphasis. `+` rides along because it is the
/// bullet marker too, so a draw reaches the nested lists whose own markers can
/// spell a thematic break.
fn delimiter_run() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "*", "**", "_", "__", "~~", "+", "¡", "—", "±", "౸", "a", "0", " ",
        ]),
        1..12,
    )
    .prop_map(|toks| toks.concat())
}

/// The formatting kinds an editor can lay over a span, two font families
/// included.
const OV_KINDS: u8 = 6;

fn ov_kind(i: u8) -> MarkKind {
    match i % OV_KINDS {
        0 => MarkKind::Strong,
        1 => MarkKind::Emph,
        2 => MarkKind::Strike,
        3 => MarkKind::Underline,
        4 => MarkKind::FontFamily { name: "Carlito".into() },
        _ => MarkKind::FontFamily { name: "Noto Serif".into() },
    }
}

/// Both marks render as a run of the same `*` character (`strong`/`emph`).
fn both_asterisk(marks: &[Mark]) -> bool {
    marks
        .iter()
        .all(|m| matches!(m.kind, MarkKind::Strong | MarkKind::Emph))
}

/// The marks intersect but neither contains the other: the shape that forces a
/// close-and-reopen (and, for asterisk delimiters, an ambiguous `***` merge).
fn partial_overlap(marks: &[Mark]) -> bool {
    let (a, b) = (&marks[0], &marks[1]);
    (a.start < b.start && b.start < a.end && a.end < b.end)
        || (b.start < a.start && a.start < b.end && b.end < a.end)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Property 1: export∘import is the identity, and every imported content
    /// satisfies its invariants.
    #[test]
    fn content_round_trip_and_invariants(md in document()) {
        let rt = from_markdown(&md).unwrap().content;
        prop_assert_eq!(rt.validate(), Ok(()), "invariants for {:?}", md);
        prop_assert_eq!(&renormalized(&rt), &*rt, "import is not the mint's fixed point for {:?}", md);

        let md2 = to_markdown(&rt);
        let rt2 = from_markdown(&md2).unwrap().content;
        prop_assert_eq!(&rt, &rt2, "not a fixed point.\n in:  {:?}\n out: {:?}", md, md2);
    }

    /// Property 1a: the line edges `document()` cannot reach. Markdown mints no
    /// line leading or trailing with whitespace, and `plain_word` pins its first
    /// char alphanumeric so no block marker leads a token; an editor's change
    /// and `from_plaintext` mint both freely.
    #[test]
    fn edge_whitespace_and_block_markers_round_trip(
        lead in prop::collection::vec(prop::sample::select(vec![' ', '\t']), 0..5),
        body in prop::sample::select(vec![
            "foo", "- item", "# h", "1. x", "> q", "===", "+ p", "***", "a b", ": d", ":",
        ]),
        trail in prop::collection::vec(prop::sample::select(vec![' ', '\t']), 0..5),
        second in prop::option::of(prop::sample::select(vec!["===", "---", "  x", "y  ", ": d"])),
    ) {
        let mut text: String = lead.into_iter().collect();
        text.push_str(body);
        text.extend(trail);
        if let Some(s) = &second {
            text.push('\n');
            text.push_str(s);
        }
        let lines = (0..text.split('\n').count())
            .map(|i| Line::new(LineKind::Para).with_continues(i > 0))
            .collect();
        let rt = Content::new(text.clone(), lines).into_lines();
        prop_assume!(rt.validate().is_ok());

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        prop_assert_eq!(&rt2.text, &rt.text,
            "text drifted.\n in:  {:?}\n md:   {:?}\n out: {:?}", rt.text, md, rt2.text);
        prop_assert_eq!(&rt, &rt2, "not a fixed point: {:?}", md);
    }

    /// Property 1b: free (Peritext-style) overlap, which an editor's change
    /// produces but markdown import never does, exports to *balanced* markdown.
    /// The shape is a staircase (`s1 < s2 < e1 < e2 == n`) over contiguous
    /// word-char text. The `strong`+`emph` asterisk clash is unrepresentable and
    /// degrades to its nested subset; the text survives either way.
    #[test]
    fn overlapping_marks_export_is_text_safe(
        raw in "[a-z]{4,8}",
        x in 0usize..64, y in 0usize..64, z in 0usize..64,
        k1i in 0u8..OV_KINDS, k2i in 0u8..OV_KINDS,
    ) {
        let text = raw;
        let n = text.chars().count();
        // Three interior cut points → the staircase, by construction rather
        // than rejection so the whole family is covered.
        let s1 = x % (n - 2);
        let s2 = s1 + 1 + y % (n - s1 - 2);
        let e1 = s2 + 1 + z % (n - s2 - 1);
        let e2 = n;

        let marks = vec![
            Mark::new(s1, e1, ov_kind(k1i)),
            Mark::new(s2, e2, ov_kind(k2i)),
        ];
        let rt = Content::new(text.clone(), vec![Line::new(LineKind::Para)])
            .with_marks(marks)
            .into_lines();
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        prop_assert_eq!(&renormalized(&rt), &*rt, "the mint left a repairable shape");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        prop_assert_eq!(rt2.validate(), Ok(()), "re-import invalid for {:?}", md);
        // Overlap never corrupts the text: no unbalanced delimiter leaks in.
        prop_assert_eq!(&rt2.text, &rt.text, "overlap corrupted text: {:?}", md);

        // Distinct delimiters → an exact fixed point via close-and-reopen. Two
        // asterisk-family marks that still partially overlap after normalization
        // are unrepresentable, so only text-safety (asserted above) holds.
        let asterisk_clash = both_asterisk(&rt.marks) && rt.marks.len() == 2
            && partial_overlap(&rt.marks);
        if !asterisk_clash {
            prop_assert_eq!(&rt, &rt2, "distinct-delim overlap not a fixed point: {:?}", md);
        }
        // Whatever the shape, the re-imported content is itself a fixed point.
        prop_assert_eq!(&rt2, &from_markdown(&to_markdown(&rt2)).unwrap().content,
            "re-imported overlap content not a fixed point: {:?}", md);
    }

    /// Editor marks over *mixed* text stay text-safe: the punctuation, symbol,
    /// emoji, and whitespace the staircase above excludes. A mark whose edge
    /// falls between a word char and a punctuation/symbol/whitespace char is not
    /// representable under CommonMark flanking, so the verify-and-drop net drops
    /// it rather than leak a delimiter. Formatting may be lost; the text is not.
    #[test]
    fn editor_marks_over_mixed_text_are_text_safe(
        mid in prop::collection::vec(
            prop::sample::select(vec![
                'a', 'b', '9', '你', '.', ',', '#', '_', '*', '~', '✓', '€', '😀', ' ',
            ]),
            2..10,
        ),
        specs in prop::collection::vec((0usize..64, 0usize..64, 0u8..OV_KINDS), 0..4),
    ) {
        // Word-char edges keep the mark-free baseline a round-trip fixed point;
        // the mixed chars live in the interior, where marks clash with flanking
        // anyway.
        let text: String = std::iter::once('a')
            .chain(mid)
            .chain(std::iter::once('a'))
            .collect();
        let n = text.chars().count();
        let plain = Content::new(text.clone(), vec![Line::new(LineKind::Para)]).into_lines();
        prop_assume!(plain.validate().is_ok());
        prop_assume!(plain.len_scalars() == n);
        // Require the mark-free text to be a fixed point already, so a later
        // mismatch is mark-induced rather than an orthogonal markdown limit.
        prop_assume!(from_markdown(&to_markdown(&plain)).unwrap().content.text == plain.text);

        let marks: Vec<Mark> = specs
            .iter()
            .map(|&(a, b, k)| {
                let (s, e) = (a % (n + 1), b % (n + 1));
                Mark::new(s.min(e), s.max(e), ov_kind(k))
            })
            .filter(|m| m.start < m.end)
            .collect();
        let rt = plain.into_content().with_marks(marks).into_lines();
        prop_assert_eq!(rt.validate(), Ok(()), "editor marks left content invalid");
        prop_assert_eq!(&renormalized(&rt), &*rt, "editor marks left a repairable shape");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        // No clipped or dropped mark leaks a delimiter into the text. Mark
        // fidelity is not promised, only that the text survives.
        prop_assert_eq!(&rt2.text, &rt.text,
            "editor mark corrupted text.\n text: {:?}\n md:   {:?}\n out:  {:?}",
            rt.text, md, rt2.text);
        prop_assert_eq!(&from_markdown(&to_markdown(&rt2)).unwrap().content.text, &rt.text,
            "text drifted on the second cycle: {:?}", md);
    }

    /// The marks here are the *parser's* own matches, so a run reaches the
    /// nestings (`*¡*0*౸*0` is emphasis inside emphasis) and `***`
    /// re-segmentations the editor ops above cannot mint. Same-kind marks
    /// union, so a nested pair collapses to one span and the emission spells
    /// fewer delimiters than the source did: markdown source moves under the
    /// projection. The text under it does not, and the emission is its own
    /// fixed point — the net returns a rendering whose marks re-import where it
    /// put them, so what the second pass reads is what the first pass wrote.
    #[test]
    fn a_delimiter_run_keeps_its_text(src in delimiter_run()) {
        let once = from_markdown(&src).unwrap().content;
        let md = to_markdown(&once);
        let twice = from_markdown(&md).unwrap().content;
        prop_assert_eq!(&twice.text, &once.text,
            "text drifted.\n in:  {:?}\n md:  {:?}\n out: {:?}", src, md, twice.text);

        prop_assert_eq!(&to_markdown(&twice), &md,
            "the emission is not a fixed point.\n in:  {:?}\n md:  {:?}", src, md);
    }

    /// Image alt and image/link URLs carry the markup- and
    /// destination-terminating specials the codec must escape for the
    /// island/link to survive export∘import. Built directly in the shape import
    /// produces, so the escaper is hit without fighting source-level quirks.
    #[test]
    fn image_and_link_specials_round_trip(
        alt in r"[a-z0-9\]\[\\&<>*_~#().+ -]{0,10}",
        img_url in r"[a-z0-9 ()&<>\\]{0,10}",
        link_url in r"[a-z0-9 ()&<>\\]{0,10}",
    ) {
        // Import trims alt, so match that to stay in the fixed-point domain.
        let alt = alt.trim().to_string();
        let text = "lnk\u{FFFC}".to_string(); // link over "lnk", image slot after
        let rt = Content::new(text, vec![Line::new(LineKind::Para)])
            .with_marks(vec![Mark::new(0, 3, MarkKind::Link { url: link_url })])
            // The id import mints for the first island, so re-import compares equal.
            .with_islands(vec![Island::new("isl-0".into(), IslandType::Image)
                .with_props(json!({ "alt": alt, "url": img_url }))])
            .into_lines();
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        prop_assert_eq!(&renormalized(&rt), &*rt, "the mint left a repairable shape");
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        prop_assert_eq!(&rt, &rt2, "alt/url specials not a fixed point.\n  md: {:?}", md);
    }

    /// Property 2a: the *byte* fixed point and the *value* fixed point. Bytes
    /// can hold while a value that encodes to some other value's bytes loses
    /// the second.
    #[test]
    fn canonical_json_fixed_point(md in document()) {
        let rt = from_markdown(&md).unwrap().content;
        let json = rt.to_canonical_json();
        let back = Content::from_canonical_json(&json).unwrap();
        prop_assert_eq!(back.to_canonical_json(), json);
        prop_assert_eq!(back, rt);
    }

    /// Property 2b: canonical bytes are insensitive to mark *discovery* order.
    /// Islands are ordered by slot position, so only mark order is free.
    #[test]
    fn canonical_json_order_insensitive(md in document()) {
        let rt = from_markdown(&md).unwrap().content;
        let mut shuffled = rt.clone().into_content();
        shuffled.marks.reverse();
        prop_assert_eq!(
            rt.to_canonical_json(),
            shuffled.into_lines().to_canonical_json()
        );
    }

    /// Property 3: an anchor over surviving text is carried forward.
    #[test]
    fn diff_import_preserves_surviving_anchor(a in "[a-z]{3,8}", b in "[a-z]{3,8}") {
        let base_md = format!("keep {a} here");
        let mut base = from_markdown(&base_md).unwrap().content.into_content();
        let start = 5;
        let end = 5 + a.chars().count();
        prop_assert_eq!(&base.text[start..end], a.as_str());
        base.marks.push(Mark::new(start, end, MarkKind::Anchor { id: "c1".into() }));
        let base = base.into_lines();

        let new_md = format!("{b} keep {a} here");
        let (new_rt, _delta, _) = diff_import(&base, &new_md).unwrap();
        let anchor = new_rt.marks.iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"));
        prop_assert!(anchor.is_some(), "anchor lost across surviving edit");
        let anchor = anchor.unwrap();
        prop_assert_eq!(&new_rt.text[anchor.start..anchor.end], a.as_str());
    }

    /// The annotated read of an imported content spells no island token,
    /// imports as the plain projection does, lists every prose anchor once,
    /// in `(start, id)` order, and revises its content to itself, or names
    /// the mark its markdown cannot write.
    #[test]
    fn the_annotated_read_imports_as_the_plain_one(
        md in prop_oneof![document(), delimiter_run()],
        spans in prop::collection::vec((0usize..4096, 0usize..8, any::<bool>()), 1..5),
    ) {
        let mut rt = from_markdown(&md).unwrap().content.into_content();
        let len = rt.len_scalars();
        for (k, &(at, width, zero_width)) in spans.iter().enumerate() {
            let start = at % (len + 1);
            let end = if zero_width { start } else { (start + width).min(len) };
            rt.marks.push(Mark::new(start, end, MarkKind::Anchor { id: format!("{k}\"&<b>") }));
        }
        let rt = rt.into_lines();
        prop_assert_eq!(rt.validate(), Ok(()), "anchored content invalid for {:?}", md);

        let plain = to_markdown(&rt);
        let read = to_markdown_annotated(&rt);
        prop_assert!(!read.markdown.contains("<qm-island"), "a generated island has no markdown: {:?}", read.markdown);
        prop_assert_eq!(
            from_markdown(&read.markdown).unwrap().content,
            from_markdown(&plain).unwrap().content,
            "the read imports apart from the plain projection.\n read:  {:?}\n plain: {:?}",
            read.markdown, plain
        );
        let mut want: Vec<(usize, &str)> = rt.marks.iter().filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } => Some((m.start, id.as_str())),
            _ => None,
        }).collect();
        want.sort_unstable();
        let got: Vec<&str> = read.anchors.iter().map(|a| a.id.as_str()).collect();
        prop_assert_eq!(got, want.into_iter().map(|(_, id)| id).collect::<Vec<_>>());

        let (revised, _, warnings) = diff_import(&rt, &read.markdown).unwrap();
        prop_assert!(warnings.is_empty(), "{:?}", warnings);
        // An imported mark markdown cannot write back, a strike over escaped
        // punctuation a letter follows, is a named loss, not a silent one; the
        // text and the islands always come home.
        let losses = to_markdown_checked(&rt).losses;
        if losses.is_empty() {
            prop_assert_eq!(&revised, &rt, "the read does not revise to itself: {:?}", read.markdown);
        } else {
            prop_assert!(
                losses.iter().all(|l| matches!(l, ExportLoss::Mark { .. })),
                "the read loses more than marks: {:?} in {:?}", losses, read.markdown
            );
            prop_assert_eq!(&revised.text, &rt.text, "the read does not revise its text to itself: {:?}", read.markdown);
            prop_assert_eq!(&revised.islands, &rt.islands, "the read does not revise its islands to itself: {:?}", read.markdown);
        }
    }

}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Font marks in any raw arrangement normalize to one family per
    /// character, the one the raw marks rank first there (the later start,
    /// then the shorter, then the later name).
    #[test]
    fn font_marks_settle_to_one_family_per_character(
        raw in prop::collection::vec((0usize..12, 0usize..12, 0usize..3), 0..6),
    ) {
        const NAMES: [&str; 3] = ["Arimo", "Bitter", "Carlito"];
        let family = |i: usize| MarkKind::FontFamily { name: NAMES[i].into() };
        let per_char = |marks: &[Mark], len: usize| -> Vec<Option<String>> {
            (0..len)
                .map(|c| {
                    marks
                        .iter()
                        .filter(|m| m.start <= c && c < m.end)
                        .filter_map(|m| match &m.kind {
                            MarkKind::FontFamily { name } => Some((m.start, std::cmp::Reverse(m.end), name.clone())),
                            _ => None,
                        })
                        .max()
                        .map(|(_, _, name)| name)
                })
                .collect()
        };
        let overlap_free = |marks: &[Mark]| {
            let fonts: Vec<_> = marks.iter().filter(|m| matches!(m.kind, MarkKind::FontFamily { .. })).collect();
            fonts.windows(2).all(|w| w[0].end <= w[1].start)
        };

        let marks: Vec<Mark> = raw.iter().map(|&(a, b, f)| Mark::new(a.min(b), a.max(b), family(f))).collect();
        let rt = Content::new("abcdefghijkl".into(), vec![Line::new(LineKind::Para)])
            .with_marks(marks.clone())
            .into_lines();
        prop_assert_eq!(per_char(&rt.marks, 12), per_char(&marks, 12), "normalize changed a family");
        prop_assert!(overlap_free(&rt.marks), "font marks overlap: {:?}", rt.marks);
        prop_assert_eq!(&renormalized(&rt), &*rt, "the mint left a repairable shape");
    }
}

fn fixture_body(name: &str) -> String {
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
fn cell_token() -> impl Strategy<Value = String> {
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

fn cell_content() -> impl Strategy<Value = String> {
    prop::collection::vec(cell_token(), 1..3).prop_map(|toks| toks.join(" "))
}

fn alignment() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just("none"), Just("left"), Just("center"), Just("right")]
}

/// The canonical `{text, marks}` cells for a row of markdown contents, obtained
/// by importing a header-only table, so each cell is representable and its
/// marks are canonical by construction. `contents` must be non-empty.
fn import_row(contents: &[String]) -> Vec<Value> {
    let cols = contents.len();
    let header = contents.join(" | ");
    let delim = vec!["---"; cols].join(" | ");
    let body = vec!["x"; cols].join(" | ");
    let md = format!("| {header} |\n| {delim} |\n| {body} |");
    let rt = from_markdown(&md).unwrap().content;
    rt.islands[0].props["header"].as_array().unwrap().clone()
}

/// A single-table content with the given (possibly ill-shaped) props. `id` is the
/// first-island id import mints (`isl-0`), so a re-imported table compares equal.
fn table_content(aligns: Vec<&str>, header: Vec<Value>, rows: Vec<Vec<Value>>) -> Content {
    Content::new("\u{FFFC}".into(), vec![Line::new(LineKind::Para)]).with_islands(vec![
        Island::new("isl-0".into(), IslandType::Table)
            .with_props(json!({ "aligns": aligns, "header": header, "rows": rows })),
    ])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// An ill-shaped table island normalizes to a valid content that is a fixed
    /// point of export∘import.
    #[test]
    fn table_island_normalizes_and_round_trips(
        header in prop::collection::vec(cell_content(), 0..4),
        rows in prop::collection::vec(prop::collection::vec(cell_content(), 0..4), 0..4),
        aligns in prop::collection::vec(alignment(), 0..5),
    ) {
        let header_cells = if header.is_empty() { vec![] } else { import_row(&header) };
        let row_cells: Vec<Vec<Value>> = rows
            .iter()
            .map(|r| if r.is_empty() { vec![] } else { import_row(r) })
            .collect();

        // The widest column count drives normalization. A fully empty table has
        // no markdown projection, so it is outside the fixed-point contract.
        let cols = header_cells.len()
            .max(aligns.len())
            .max(row_cells.iter().map(Vec::len).max().unwrap_or(0));
        prop_assume!(cols >= 1);

        let rt = table_content(aligns.clone(), header_cells, row_cells).into_lines();
        prop_assert_eq!(rt.validate(), Ok(()), "normalized table invalid");
        prop_assert_eq!(&renormalized(&rt), &*rt, "the table mint is not a fixed point");

        let props = &rt.islands[0].props;
        prop_assert_eq!(props["header"].as_array().unwrap().len(), cols);
        prop_assert_eq!(props["aligns"].as_array().unwrap().len(), cols);
        for row in props["rows"].as_array().unwrap() {
            prop_assert_eq!(row.as_array().unwrap().len(), cols);
        }

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        prop_assert_eq!(&rt, &rt2, "table not a fixed point.\n  md: {:?}", md);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Editor marks over a cell holding `\n`: the shapes import never mints, a
    /// mark edge on a break and a code span across one. The text and the row
    /// survive whatever the marks, and a break costs no mark.
    #[test]
    fn editor_marks_over_a_cell_break_are_text_safe(
        chars in prop::collection::vec(
            prop::sample::select(vec!['a', 'b', '9', '\n', ' ']),
            1..10,
        ),
        specs in prop::collection::vec((0usize..64, 0usize..64, 0u8..=OV_KINDS), 0..4),
    ) {
        let text: String = chars.into_iter().collect();
        let n = text.chars().count();
        let marks: Vec<Value> = specs
            .iter()
            .map(|&(a, b, k)| {
                let (s, e) = (a % (n + 1), b % (n + 1));
                let kind = if k == OV_KINDS { MarkKind::Code } else { ov_kind(k) };
                quillmark_content::lines::serial::mark_to_value(&Mark::new(s.min(e), s.max(e), kind))
            })
            .collect();
        let rt = table_content(
            vec!["none"],
            vec![json!({"text": "h", "marks": []})],
            vec![vec![json!({"text": text, "marks": marks})]],
        )
        .into_lines();
        prop_assert_eq!(rt.validate(), Ok(()), "normalized table invalid");
        prop_assert_eq!(&renormalized(&rt), &*rt, "the table mint is not a fixed point");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        let rows = rt2.islands.first().map(|i| i.props["rows"].clone());
        prop_assert_eq!(rows.as_ref().and_then(|r| r.as_array()).map(Vec::len), Some(1),
            "the row split: {:?}", md);
        let cell = &rt2.islands[0].props["rows"][0][0];
        prop_assert_eq!(cell["text"].as_str(), Some(text.as_str()), "cell text drifted: {:?}", md);
        prop_assert_eq!(&from_markdown(&to_markdown(&rt2)).unwrap().content, &rt2,
            "the re-import is not a fixed point: {:?}", md);

        // A break flanks a delimiter as the punctuation `<br>` is, so the same
        // cell with each `\n` a `,` keeps the marks it keeps, up to the trim of
        // an edge off a break. A code span is left out: one across a break
        // re-imports as two spans, and its clip can land a wrap's edge on a
        // space the trim exposes.
        let (stored, kept) = quillmark_content::lines::serial::parse_cell(&rt.islands[0].props["rows"][0][0]);
        let cs: Vec<char> = stored.chars().collect();
        if !kept.iter().any(|m| m.kind == MarkKind::Code) {
            let commas: String = cs.iter().map(|&c| if c == '\n' { ',' } else { c }).collect();
            // The twin carries each mark as the cell's export writes it: off a
            // break, then off the whitespace that exposes under a delimiter.
            let delimited = |k: &MarkKind| matches!(k, MarkKind::Strong | MarkKind::Emph | MarkKind::Strike);
            let wire: Vec<Value> = kept
                .iter()
                .filter_map(|m| {
                    let mut m = m.clone();
                    let mut trim = |trims: &dyn Fn(char) -> bool| {
                        while m.start < m.end && trims(cs[m.start]) {
                            m.start += 1;
                        }
                        while m.end > m.start && trims(cs[m.end - 1]) {
                            m.end -= 1;
                        }
                    };
                    trim(&|c| c == '\n');
                    if delimited(&m.kind) {
                        trim(&|c: char| c.is_whitespace());
                    }
                    (m.start < m.end).then(|| quillmark_content::lines::serial::mark_to_value(&m))
                })
                .collect();
            let twin = table_content(
                vec!["none"],
                vec![json!({"text": "h", "marks": []})],
                vec![vec![json!({"text": commas, "marks": wire})]],
            )
            .into_lines();
            let twin2 = from_markdown(&to_markdown(&twin)).unwrap().content;
            let marks_of = |rt: &Lines| {
                let mut marks = quillmark_content::lines::serial::parse_cell(&rt.islands[0].props["rows"][0][0]).1;
                // Off the twin's `,` where the cell holds a `\n`, as a stored edge is.
                for m in &mut marks {
                    while m.start < m.end && cs[m.start] == '\n' {
                        m.start += 1;
                    }
                    while m.end > m.start && cs[m.end - 1] == '\n' {
                        m.end -= 1;
                    }
                }
                marks.retain(|m| m.start < m.end);
                marks.sort_by_key(|m| (m.start, m.end, m.kind.sort_key()));
                marks
            };
            prop_assert_eq!(marks_of(&rt2), marks_of(&twin2), "a break cost a mark: {:?}", md);
        }
    }
}

// The JSON doors: `install` takes a canonical content, `applyChange` takes an
// op wire, and both are reached from a binding with whatever the caller built.
// A panic there traps the WASM module and costs the document rather than the
// operation, so refusal is the property; what a decoded value must preserve
// belongs to the round-trip properties above.

/// The keys the decoders dispatch on, so generated objects reach past the first
/// branch; the noise arm keeps the rest of the space.
const DECODE_DISCRIMINATORS: &[&str] = &[
    "text", "lines", "marks", "islands", "kind", "attrs", "op", "line", "at", "delta", "ops",
    "retain", "insert", "islandOps", "lineOps", "markOps", "start", "end", "container", "type",
    "$name", "element", "instance", "containers", "continues", "id", "props", "url", "level",
    "ordered", "ordinal", "rows", "aligns", "widths", "align", "list_item", "quote", "table",
    "checked", "head", "colspan", "rowspan", "valign",
];

fn decode_key() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(DECODE_DISCRIMINATORS).prop_map(str::to_string),
        "\\PC{0,12}",
    ]
}

/// Arbitrary JSON, container-biased: the decoders branch on object keys and
/// array shapes, so a scalar-weighted generator would spend its budget failing
/// at the first `as_object()`.
fn decode_json() -> impl Strategy<Value = Value> {
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// The `install` lane.
    #[test]
    fn canonical_decode_never_panics(v in decode_json()) {
        let _ = quillmark_content::lines::serial::from_canonical_value(&v);
    }

    /// A line's containers inside an envelope that decodes up to them.
    #[test]
    fn container_decode_never_panics(containers in prop::collection::vec(decode_json(), 0..4)) {
        let content = serde_json::json!({
            "text": "a",
            "lines": [{"kind": "para", "containers": containers}],
            "marks": [],
            "islands": [],
        });
        let _ = quillmark_content::lines::serial::from_canonical_value(&content);
    }
}

/// A grid as a table's props spell it, either axis possibly empty, a row's
/// cells possibly short, and the layout keys holding anything.
fn grid_props() -> impl Strategy<Value = Value> {
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
                    let cells: serde_json::Map<String, Value> =
                        columns.iter().cloned().zip(slots.into_iter().map(Value::from)).collect();
                    json!({"id": id, "cells": cells})
                })
                .collect();
            let mut props = serde_json::Map::from_iter([("columns".to_string(), json!(columns)), ("rows".to_string(), Value::Array(rows))]);
            props.extend(layout.into_iter().map(|(k, v)| (k.to_string(), v)));
            Value::Object(props)
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// The model's door: a value shaped as content around one island of each
    /// type, its props generated, decodes or refuses, renders if it decodes,
    /// and an island insert carrying it applies or refuses.
    #[test]
    fn model_decode_never_panics(
        island_type in prop::sample::select(&["table", "footnote", "image", "math", "x-a"][..]),
        props in prop_oneof![grid_props(), decode_json()],
        slots in prop::collection::vec("s[0-3]|note", 0..3),
    ) {
        use quillmark_content::model::Content;
        let slots: serde_json::Map<String, Value> =
            slots.into_iter().map(|s| (s, json!({"text": "a", "blocks": [{"id": "b0", "kind": "para"}]}))).collect();
        let island = json!({"id": "t0", "type": island_type, "props": props, "slots": slots});
        let value = json!({"text": "\u{FFFC}", "blocks": [{"id": "b0", "kind": "para"}], "islands": [island.clone()]});
        if let Ok(c) = Content::from_value(&value) {
            let _ = quillmark_content::migrate::to_lines(&c);
        }
        let insert = json!({"steps": [{"island": {"at": 0, "island": island}}]});
        if let Ok(change) = quillmark_content::change::Change::from_value(&insert) {
            let _ = quillmark_content::change::apply(&Content::empty(), &change);
        }
    }
}

#[test]
fn fixture_bodies_import_and_are_valid() {
    for name in [
        "sample.md",
        "card_yaml_demo.md",
        "extended_metadata_demo.md",
    ] {
        let md = fixture_body(name);
        let rt = from_markdown(&md).unwrap_or_else(|e| panic!("import {name}: {e}")).content;
        assert_eq!(rt.validate(), Ok(()), "{name} invariants");
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap().content;
        assert_eq!(rt, rt2, "{name} content not a fixed point");
    }
}

// A merge takes three texts it did not write, so it states its algebra over
// generated line sets: one side unchanged is the other side's text, one side
// equal to the other is that text, and the two sides commute.
fn lines() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(vec!["a\n", "b\n", "c\n", "\n", "d"]), 0..7)
        .prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn merge_lines_algebra(base in lines(), ours in lines(), theirs in lines()) {
        use quillmark_content::lines::delta::merge_lines;
        prop_assert_eq!(merge_lines(&base, &base, &theirs), Some(theirs.clone()));
        prop_assert_eq!(merge_lines(&base, &ours, &base), Some(ours.clone()));
        prop_assert_eq!(merge_lines(&base, &ours, &ours), Some(ours.clone()));
        prop_assert_eq!(merge_lines(&base, &ours, &theirs), merge_lines(&base, &theirs, &ours));
    }
}

mod model {
    //! The content model over the line form: the migration is deterministic and
    //! lowers back to what it read, and the wire is a byte and value fixed point.

    use super::*;
    use quillmark_content::markdown;
    use quillmark_content::migrate::{from_lines, to_lines};
    use quillmark_content::model::Content as V2;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        #[test]
        fn migration_lowers_back_to_its_source(md in document()) {
            let rt = from_markdown(&md).unwrap().content;
            let v2 = from_lines(&rt);
            prop_assert_eq!(v2.validate(), Ok(()), "invariants for {:?}", md);
            prop_assert_eq!(&from_lines(&rt), &v2);
            prop_assert_eq!(&to_lines(&v2), &rt, "lowering differs for {:?}", md);
            prop_assert_eq!(from_lines(&to_lines(&v2)), v2);
            prop_assert_eq!(markdown::to_markdown(&from_lines(&rt)), to_markdown(&rt));
        }

        #[test]
        fn wire_fixed_point(md in document()) {
            let v2 = markdown::from_markdown(&md).unwrap().content;
            let json = v2.to_json();
            let back = V2::from_json(&json).unwrap();
            prop_assert_eq!(back.to_json(), json);
            prop_assert_eq!(back, v2);
        }

        #[test]
        fn a_v1_value_decodes_as_its_migration(md in document()) {
            let rt = from_markdown(&md).unwrap().content;
            let v1 = rt.to_canonical_json();
            prop_assert_eq!(V2::from_json(&v1).unwrap(), from_lines(&rt));
        }

        #[test]
        fn decode_never_panics(v in decode_json()) {
            let _ = V2::from_value(&v);
        }
    }

    /// Line-form shapes the model refuses migrate to the nearest it holds: a
    /// table sharing its line takes a block of its own, and an anchor id held
    /// twice keeps its first holder.
    #[test]
    fn a_shape_the_model_refuses_migrates_valid() {
        let mut rt = from_markdown("before\n\n| a |\n|---|\n| b |\n\nafter").unwrap().content.into_content();
        rt.text = "before\u{FFFC}after".into();
        rt.lines.truncate(1);
        rt.marks.push(Mark::new(0, 6, MarkKind::Anchor { id: "c1".into() }));
        rt.marks.push(Mark::new(7, 12, MarkKind::Anchor { id: "c1".into() }));
        let v2 = from_lines(&rt.into_lines());
        assert_eq!(v2.validate(), Ok(()));
        assert_eq!(v2.text, "before\n\u{FFFC}\nafter");
        assert_eq!(v2.anchors.len(), 1);
        assert_eq!((v2.anchors[0].start, v2.anchors[0].end), (0, 6));
    }

    #[test]
    fn fixture_bodies_migrate_and_lower_back() {
        for name in ["sample.md", "card_yaml_demo.md", "extended_metadata_demo.md"] {
            let rt = from_markdown(&fixture_body(name)).unwrap();
            let v2 = from_lines(&rt.content);
            assert_eq!(v2.validate(), Ok(()), "{name}");
            assert_eq!(to_lines(&v2), rt.content, "{name}");
        }
    }
}

mod strict {
    //! The write's decode against the stored read: a value spelled otherwise
    //! than `normalize` leaves it is refused by the one, read by the other to
    //! a value the first accepts, and canonical bytes decode to themselves.

    use super::*;
    use quillmark_content::markdown;
    use quillmark_content::model::Content as V2;

    /// One spelling `normalize` would rewrite, made at offsets `at` and `n`
    /// pick inside the value.
    #[derive(Debug, Clone, Copy)]
    enum Respell {
        ReverseMark,
        EmptyMark,
        SplitMark,
        MarkOverBreak,
        ExtMarkOverBreak,
        ShuffleMarks,
        ShuffleAnchors,
        UnusedContainer,
        SwapContainers,
        DefaultHead,
        CellParagraphs,
    }

    fn respell() -> impl Strategy<Value = Respell> {
        use Respell::*;
        prop::sample::select(vec![
            ReverseMark, EmptyMark, SplitMark, MarkOverBreak, ExtMarkOverBreak, ShuffleMarks, ShuffleAnchors,
            UnusedContainer, SwapContainers, DefaultHead, CellParagraphs,
        ])
    }

    /// `v` respelled, the respelling read at `at`; `v` itself where it holds
    /// nothing the respelling reaches.
    fn respelled(v: &Value, r: Respell, at: usize) -> Value {
        let mut v = v.clone();
        let text: Vec<char> = v["text"].as_str().unwrap().chars().collect();
        let marks = v.get("marks").and_then(Value::as_array).cloned().unwrap_or_default();
        let anchors = v.get("anchors").and_then(Value::as_array).cloned().unwrap_or_default();
        let newlines: Vec<usize> = text.iter().enumerate().filter(|(_, c)| **c == '\n').map(|(i, _)| i).collect();
        let newline = (!newlines.is_empty()).then(|| newlines[at % newlines.len()]);
        let push_mark = |v: &mut Value, mark: Value| {
            let mut all = marks.clone();
            all.push(mark);
            v["marks"] = Value::Array(all);
        };
        match r {
            Respell::ReverseMark | Respell::EmptyMark | Respell::SplitMark if !marks.is_empty() => {
                let k = at % marks.len();
                let (s, e) = (marks[k]["start"].as_u64().unwrap(), marks[k]["end"].as_u64().unwrap());
                let mut all = marks.clone();
                match r {
                    Respell::ReverseMark => (all[k]["start"], all[k]["end"]) = (e.into(), s.into()),
                    Respell::EmptyMark => all[k]["end"] = s.into(),
                    _ if e - s >= 2 => {
                        let mid = s + 1 + (at as u64 % (e - s - 1));
                        let mut tail = all[k].clone();
                        all[k]["end"] = mid.into();
                        tail["start"] = mid.into();
                        all.insert(k + 1, tail);
                    }
                    _ => {}
                }
                v["marks"] = Value::Array(all);
            }
            Respell::MarkOverBreak if newline.is_some() => {
                let n = newline.unwrap();
                push_mark(&mut v, json!({"start": n, "end": n + 1, "type": "strike"}));
            }
            Respell::ExtMarkOverBreak if newline.is_some() => {
                let n = newline.unwrap();
                push_mark(&mut v, json!({"start": n, "end": n + 1, "type": "x-tag"}));
            }
            Respell::ShuffleMarks if marks.len() >= 2 => {
                let mut all = marks.clone();
                all.rotate_left(1 + at % (marks.len() - 1));
                v["marks"] = Value::Array(all);
            }
            Respell::ShuffleAnchors => {
                let mut all = anchors.clone();
                all.push(json!({"id": "c-late", "start": text.len(), "end": text.len()}));
                all.push(json!({"id": "c-early", "start": 0, "end": 0}));
                v["anchors"] = Value::Array(all);
            }
            Respell::UnusedContainer => {
                let mut all = v.get("containers").and_then(Value::as_array).cloned().unwrap_or_default();
                all.insert(at % (all.len() + 1), json!({"id": "q-unused", "kind": "quote"}));
                v["containers"] = Value::Array(all);
            }
            Respell::SwapContainers => {
                if let Some(all) = v.get_mut("containers").and_then(Value::as_array_mut).filter(|a| a.len() >= 2) {
                    let k = at % (all.len() - 1);
                    all.swap(k, k + 1);
                }
            }
            Respell::DefaultHead => {
                if let Some(table) = tables(&mut v).next() {
                    table["props"]["head"] = json!(1);
                }
            }
            Respell::CellParagraphs => {
                if let Some(table) = tables(&mut v).next() {
                    let slots = table["slots"].as_object_mut().unwrap();
                    let n = slots.len();
                    let cell = slots.values_mut().nth(at % n).unwrap();
                    let text = format!("{}\nmore", cell["text"].as_str().unwrap());
                    cell["text"] = text.into();
                    cell["blocks"].as_array_mut().unwrap().push(json!({"id": "b-more", "kind": "para"}));
                }
            }
            _ => {}
        }
        v
    }

    fn tables(v: &mut Value) -> impl Iterator<Item = &mut Value> {
        v.get_mut("islands")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .filter(|i| i["type"] == "table")
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        #[test]
        fn strict_refuses_what_normalize_rewrites_and_the_lenient_read_settles_it(
            md in document(),
            r in respell(),
            at in 0usize..64,
        ) {
            let canonical = markdown::from_markdown(&md).unwrap().content.to_value();
            prop_assert_eq!(V2::from_value(&canonical).unwrap().to_value(), canonical.clone());

            let v = respelled(&canonical, r, at);
            let strict = V2::from_value(&v);
            if let Ok(c) = &strict {
                prop_assert_eq!(&c.to_value(), &v, "{:?} accepted a respelling", r);
            }
            let read = V2::from_value_lenient(&v).unwrap();
            prop_assert_eq!(read.rewritten, strict.is_err(), "{:?}", r);
            let settled = read.content.to_value();
            prop_assert_eq!(V2::from_value(&settled).map(|c| c.to_value()), Ok(settled.clone()), "{:?}", r);
            prop_assert_eq!(V2::from_value_lenient(&settled).unwrap().rewritten, false);
        }
    }
}
