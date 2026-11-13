//! The content property suite: the exact round-trip
//! (`import(export(rt)) == rt`), canonical serialization, and the JSON doors refusing
//! rather than panicking on what a caller hands them.

mod generate;

use generate::{decode_json, delimiter_run, document, fixture_body, grid_props};
use proptest::prelude::*;
use quillmark_content::markdown::{
    Loss, from_markdown, to_markdown, to_markdown_annotated, to_markdown_checked,
};
use quillmark_content::model::{Anchor, Content, HARD_BREAK, Island, IslandType, Mark, MarkKind};
use quillmark_content::table::Table;
use quillmark_content::view::{self, Cell};
use serde_json::{Value, json};

/// The mint's fixed point: normalizing again changes nothing. Paired with
/// `validate`, which reports what the mint cannot repair, it is the oracle
/// over a built content.
fn renormalized(c: &Content) -> Content {
    let mut again = c.clone();
    again.normalize();
    again
}

/// One paragraph holding `text`, each `\n` in it a hard break, under `marks`
/// and holding `islands`, as the mint settles it.
fn paragraph(text: &str, marks: Vec<Mark>, islands: Vec<Island>) -> Content {
    let mut c = Content::empty();
    c.text = text.replace('\n', &HARD_BREAK.to_string());
    c.marks = marks;
    c.islands = islands;
    c.normalize();
    c
}

/// The slot of the first body cell of the content's first table.
fn body_slot(c: &Content) -> String {
    let table = Table::read(&c.islands[0].props).unwrap();
    table.slot(1, 0).unwrap().to_string()
}

fn body_cell(c: &Content) -> Cell {
    view::cell(&c.islands[0].slots[&body_slot(c)])
}

/// A table of one header cell over one body cell, the body cell holding
/// `text` under `marks`.
fn one_cell_table(text: &str, marks: Vec<Mark>) -> Content {
    let mut c = from_markdown("| h |\n| --- |\n| x |").unwrap().content;
    let slot = body_slot(&c);
    c.islands[0]
        .slots
        .insert(slot, paragraph(text, marks, Vec::new()));
    c.normalize();
    c
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
        4 => MarkKind::FontFamily {
            name: "Carlito".into(),
        },
        _ => MarkKind::FontFamily {
            name: "Noto Serif".into(),
        },
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
        prop_assert_eq!(&renormalized(&rt), &rt, "import is not the mint's fixed point for {:?}", md);

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
        let rt = paragraph(&text, Vec::new(), Vec::new());
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
        let rt = paragraph(&text, marks, Vec::new());
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        prop_assert_eq!(&renormalized(&rt), &rt, "the mint left a repairable shape");

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
        let plain = paragraph(&text, Vec::new(), Vec::new());
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
        let rt = paragraph(&text, marks, Vec::new());
        prop_assert_eq!(rt.validate(), Ok(()), "editor marks left content invalid");
        prop_assert_eq!(&renormalized(&rt), &rt, "editor marks left a repairable shape");

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
        let rt = paragraph(
            &text,
            vec![Mark::new(0, 3, MarkKind::Link { url: link_url })],
            // The id import mints for the first island, so re-import compares equal.
            vec![Island::new("isl-0", IslandType::Image)
                .with_props(json!({ "alt": alt, "url": img_url }))],
        );
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        prop_assert_eq!(&renormalized(&rt), &rt, "the mint left a repairable shape");
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        prop_assert_eq!(&rt, &rt2, "alt/url specials not a fixed point.\n  md: {:?}", md);
    }

    /// The annotated read of an imported content spells no island token,
    /// imports as the plain projection does, and lists every prose anchor
    /// once, in `(start, id)` order.
    #[test]
    fn the_annotated_read_imports_as_the_plain_one(
        md in prop_oneof![document(), delimiter_run()],
        spans in prop::collection::vec((0usize..4096, 0usize..8, any::<bool>()), 1..5),
    ) {
        let mut rt = from_markdown(&md).unwrap().content;
        let len = rt.len_scalars();
        for (k, &(at, width, zero_width)) in spans.iter().enumerate() {
            let start = at % (len + 1);
            let end = if zero_width { start } else { (start + width).min(len) };
            rt.anchors.push(Anchor::new(format!("{k}\"&<b>"), start, end));
        }
        rt.normalize();
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
        let mut want: Vec<(usize, &str)> = rt.anchors.iter().map(|a| (a.start, a.id.as_str())).collect();
        want.sort_unstable();
        let got: Vec<&str> = read.anchors.iter().map(|a| a.id.as_str()).collect();
        prop_assert_eq!(got, want.into_iter().map(|(_, id)| id).collect::<Vec<_>>());

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
        let rt = paragraph("abcdefghijkl", marks.clone(), Vec::new());
        prop_assert_eq!(per_char(&rt.marks, 12), per_char(&marks, 12), "normalize changed a family");
        prop_assert!(overlap_free(&rt.marks), "font marks overlap: {:?}", rt.marks);
        prop_assert_eq!(&renormalized(&rt), &rt, "the mint left a repairable shape");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Editor marks over a cell holding `\n`: the shapes import never mints, a
    /// mark edge on a break and a code span across one, read as the content
    /// model holds the cell: its marks parted at each break, and a cell it
    /// holds otherwise left out. The text and the row survive whatever the
    /// marks, and a break costs no mark.
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
        let marks: Vec<Mark> = specs
            .iter()
            .map(|&(a, b, k)| {
                let (s, e) = (a % (n + 1), b % (n + 1));
                let kind = if k == OV_KINDS { MarkKind::Code } else { ov_kind(k) };
                Mark::new(s.min(e), s.max(e), kind)
            })
            .collect();
        let rt = one_cell_table(&text, marks);
        prop_assert_eq!(rt.validate(), Ok(()), "normalized table invalid");
        prop_assert_eq!(&renormalized(&rt), &rt, "the table mint is not a fixed point");

        let held = body_cell(&rt);
        prop_assume!(held.text == text);
        let checked = to_markdown_checked(&rt);
        prop_assume!(!checked.losses.iter().any(|l| matches!(l, Loss::SlotBreaks { .. })));
        let md = checked.markdown;
        let rt2 = from_markdown(&md).unwrap().content;
        let rows = rt2.islands.first().and_then(|i| Table::read(&i.props)).map(|t| t.rows.len());
        prop_assert_eq!(rows, Some(2), "the row split: {:?}", md);
        prop_assert_eq!(&body_cell(&rt2).text, &text, "cell text drifted: {:?}", md);
        prop_assert_eq!(&from_markdown(&to_markdown(&rt2)).unwrap().content, &rt2,
            "the re-import is not a fixed point: {:?}", md);

        // A break flanks a delimiter as the punctuation `<br>` is, so the same
        // cell with each `\n` a `,` keeps the marks it keeps, up to the trim of
        // an edge off a break. A code span is left out: one across a break
        // re-imports as two spans, and its clip can land a wrap's edge on a
        // space the trim exposes.
        let cs: Vec<char> = held.text.chars().collect();
        if !held.marks.iter().any(|m| m.kind == MarkKind::Code) {
            let commas: String = cs.iter().map(|&c| if c == '\n' { ',' } else { c }).collect();
            // The twin carries each mark as the cell's export writes it: off a
            // break, then off the whitespace that exposes under a delimiter.
            let delimited = |k: &MarkKind| matches!(k, MarkKind::Strong | MarkKind::Emph | MarkKind::Strike);
            let written: Vec<Mark> = held
                .marks
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
                    (m.start < m.end).then_some(m)
                })
                .collect();
            let twin = one_cell_table(&commas, written);
            let twin2 = from_markdown(&to_markdown(&twin)).unwrap().content;
            let marks_of = |c: &Content| {
                let mut marks = body_cell(c).marks;
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
        let slots: serde_json::Map<String, Value> =
            slots.into_iter().map(|s| (s, json!({"text": "a", "blocks": [{"id": "b0", "kind": "para"}]}))).collect();
        let island = json!({"id": "t0", "type": island_type, "props": props, "slots": slots});
        let value = json!({"text": "\u{FFFC}", "blocks": [{"id": "b0", "kind": "para"}], "islands": [island.clone()]});
        if let Ok(c) = Content::from_value(&value) {
            let _ = quillmark_content::view::View::of(&c);
            let _ = to_markdown(&c);
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
        let rt = from_markdown(&md)
            .unwrap_or_else(|e| panic!("import {name}: {e}"))
            .content;
        assert_eq!(rt.validate(), Ok(()), "{name} invariants");
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap().content;
        assert_eq!(rt, rt2, "{name} content not a fixed point");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn wire_fixed_point(md in document()) {
        let v2 = from_markdown(&md).unwrap().content;
        let json = v2.to_json();
        let back = Content::from_json(&json).unwrap();
        prop_assert_eq!(back.to_json(), json);
        prop_assert_eq!(back, v2);
    }

    #[test]
    fn decode_never_panics(v in decode_json()) {
        let _ = Content::from_value(&v);
    }
}

mod strict {
    //! The write's decode against the stored read: a value spelled otherwise
    //! than `normalize` leaves it is refused by the one, read by the other to
    //! a value the first accepts, and canonical bytes decode to themselves.

    use super::*;
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
        ])
    }

    /// `v` respelled, the respelling read at `at`; `v` itself where it holds
    /// nothing the respelling reaches.
    fn respelled(v: &Value, r: Respell, at: usize) -> Value {
        let mut v = v.clone();
        let text: Vec<char> = v["text"].as_str().unwrap().chars().collect();
        let marks = v
            .get("marks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let anchors = v
            .get("anchors")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let newlines: Vec<usize> = text
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .map(|(i, _)| i)
            .collect();
        let newline = (!newlines.is_empty()).then(|| newlines[at % newlines.len()]);
        let push_mark = |v: &mut Value, mark: Value| {
            let mut all = marks.clone();
            all.push(mark);
            v["marks"] = Value::Array(all);
        };
        match r {
            Respell::ReverseMark | Respell::EmptyMark | Respell::SplitMark if !marks.is_empty() => {
                let k = at % marks.len();
                let (s, e) = (
                    marks[k]["start"].as_u64().unwrap(),
                    marks[k]["end"].as_u64().unwrap(),
                );
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
                let mut all = v
                    .get("containers")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                all.insert(
                    at % (all.len() + 1),
                    json!({"id": "q-unused", "kind": "quote"}),
                );
                v["containers"] = Value::Array(all);
            }
            Respell::SwapContainers => {
                if let Some(all) = v
                    .get_mut("containers")
                    .and_then(Value::as_array_mut)
                    .filter(|a| a.len() >= 2)
                {
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
                    cell["blocks"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"id": "b-more", "kind": "para"}));
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
            let canonical = from_markdown(&md).unwrap().content.to_value();
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
