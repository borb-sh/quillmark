//! The content property suite: round-trip modulo loss class
//! (`import(export(rt)) == rt`, exact here since the generator emits only
//! lossless islands), canonical serialization, and diff-import preserving the
//! marks markdown cannot carry.

use proptest::prelude::*;
use quillmark_content::delta::diff_import;
use quillmark_content::export::to_markdown;
use quillmark_content::import::from_markdown;
use quillmark_content::model::{Line, Mark, MarkKind};
use quillmark_content::{Content, Delta, Island, IslandOp, LineKind, LineOp, MarkOp, Op};
use serde_json::{json, Value};

// A constrained markdown generator: inline tokens are space-separated so the
// properties exercise structure and marks without depending on CommonMark's
// delimiter-adjacency corners, which explicit unit tests pin instead.

// `clean_word` is safe content for inside a formatted span / url / code / cell.
fn clean_word() -> impl Strategy<Value = String> {
    "[a-z0-9]{1,6}"
}

// `plain_word` carries inline-special and astral chars as *literal text*, so
// escaping and USV bounds are exercised by the round-trip. The first char stays
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
        (clean_word(), clean_word()).prop_map(|(t, u)| format!("[{t}](https://ex.com/{u})")),
        (clean_word(), special_url()).prop_map(|(t, u)| format!("[{t}](<{u}>)")),
        (special_alt(), special_url()).prop_map(|(a, u)| format!("![{a}](<{u}>)")),
        // A link over an image: a mark over an island slot.
        (special_alt(), special_url(), clean_word())
            .prop_map(|(a, u, l)| format!("[![{a}](<{u}>)](https://ex.com/{l})")),
    ]
}

fn prose() -> impl Strategy<Value = String> {
    // Mix single-space and hard-break (`\`+newline) separators, and let some
    // tokens abut, so delimiter-adjacency and hard breaks are covered.
    prop::collection::vec(inline_token(), 1..5).prop_map(|toks| toks.join(" "))
}

// Hard breaks join clean, non-empty *text* lines. A mark spanning a hard break,
// or an empty line adjacent to one, is a documented codec limit (see
// `export::tests::known_hard_break_limits`), not fuzzed here.
fn hard_break_line() -> impl Strategy<Value = String> {
    prop::collection::vec(clean_word(), 1..4).prop_map(|w| w.join(" "))
}

fn hard_break_prose() -> impl Strategy<Value = String> {
    prop::collection::vec(hard_break_line(), 2..4).prop_map(|lines| lines.join("\\\n"))
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
        (clean_word(), clean_word())
            .prop_map(|(a, b)| format!("| {a} | {b} |\n| --- | --- |\n| 1 | 2 |")),
    ]
}

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(block(), 1..6).prop_map(|blocks| blocks.join("\n\n"))
}

fn ov_kind(i: u8) -> MarkKind {
    match i % 4 {
        0 => MarkKind::Strong,
        1 => MarkKind::Emph,
        2 => MarkKind::Strike,
        _ => MarkKind::Underline,
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
        let rt = from_markdown(&md).unwrap();
        prop_assert_eq!(rt.validate(), Ok(()), "invariants for {:?}", md);

        let md2 = to_markdown(&rt);
        let rt2 = from_markdown(&md2).unwrap();
        prop_assert_eq!(&rt, &rt2, "not a fixed point.\n in:  {:?}\n out: {:?}", md, md2);
    }

    /// Property 1a: the line edges `document()` cannot reach. Markdown mints no
    /// line leading or trailing with whitespace, and `plain_word` pins its first
    /// char alphanumeric so no block marker leads a token; `apply_text_delta`
    /// and `from_plaintext` mint both freely.
    #[test]
    fn edge_whitespace_and_block_markers_round_trip(
        lead in prop::collection::vec(prop::sample::select(vec![' ', '\t']), 0..5),
        body in prop::sample::select(vec![
            "foo", "- item", "# h", "1. x", "> q", "===", "+ p", "***", "a b",
        ]),
        trail in prop::collection::vec(prop::sample::select(vec![' ', '\t']), 0..5),
        second in prop::option::of(prop::sample::select(vec!["===", "---", "  x", "y  "])),
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
        let rt = Content::new(text.clone(), lines).into_normalized();
        prop_assume!(rt.validate().is_ok());

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        prop_assert_eq!(&rt2.text, &rt.text,
            "text drifted.\n in:  {:?}\n md:   {:?}\n out: {:?}", rt.text, md, rt2.text);
        prop_assert_eq!(&rt, &rt2, "not a fixed point: {:?}", md);
    }

    /// Property 1b: free (Peritext-style) overlap, which `apply_mark_ops`
    /// produces but markdown import never does, exports to *balanced* markdown.
    /// The shape is a staircase (`s1 < s2 < e1 < e2 == n`) over contiguous
    /// word-char text. The `strong`+`emph` asterisk clash is unrepresentable and
    /// degrades to its nested subset; the text survives either way.
    #[test]
    fn overlapping_marks_export_is_text_safe(
        raw in "[a-z]{4,8}",
        x in 0usize..64, y in 0usize..64, z in 0usize..64,
        k1i in 0u8..4, k2i in 0u8..4,
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
            .into_normalized();
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
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
        prop_assert_eq!(&rt2, &from_markdown(&to_markdown(&rt2)).unwrap(),
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
        specs in prop::collection::vec((0usize..64, 0usize..64, 0u8..4), 0..4),
    ) {
        // Word-char edges keep the mark-free baseline a round-trip fixed point;
        // the mixed chars live in the interior, where marks clash with flanking
        // anyway.
        let text: String = std::iter::once('a')
            .chain(mid)
            .chain(std::iter::once('a'))
            .collect();
        let n = text.chars().count();
        let mut rt = Content::new(text.clone(), vec![Line::new(LineKind::Para)]).into_normalized();
        prop_assume!(rt.validate().is_ok());
        prop_assume!(rt.len_usv() == n);
        // Require the mark-free text to be a fixed point already, so a later
        // mismatch is mark-induced rather than an orthogonal markdown limit.
        prop_assume!(from_markdown(&to_markdown(&rt)).unwrap().text == rt.text);

        let ops: Vec<MarkOp> = specs
            .iter()
            .map(|&(a, b, k)| {
                let (s, e) = (a % (n + 1), b % (n + 1));
                MarkOp::Add { start: s.min(e), end: s.max(e), kind: ov_kind(k) }
            })
            .filter(|op| matches!(op, MarkOp::Add { start, end, .. } if start < end))
            .collect();
        rt.apply_mark_ops(&ops).unwrap();
        prop_assert_eq!(rt.validate(), Ok(()), "editor marks left content invalid");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        // No clipped or dropped mark leaks a delimiter into the text. Mark
        // fidelity is not promised, only that the text survives.
        prop_assert_eq!(&rt2.text, &rt.text,
            "editor mark corrupted text.\n text: {:?}\n md:   {:?}\n out:  {:?}",
            rt.text, md, rt2.text);
        prop_assert_eq!(&from_markdown(&to_markdown(&rt2)).unwrap().text, &rt.text,
            "text drifted on the second cycle: {:?}", md);
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
            .with_islands(vec![Island::new("isl-0".into(), "image".into())
                .with_props(json!({ "alt": alt, "url": img_url }))])
            .into_normalized();
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        prop_assert_eq!(&rt, &rt2, "alt/url specials not a fixed point.\n  md: {:?}", md);
    }

    /// Property 2a: the *byte* fixed point.
    #[test]
    fn canonical_json_fixed_point(md in document()) {
        let rt = from_markdown(&md).unwrap();
        let json = rt.to_canonical_json();
        let back = Content::from_canonical_json(&json).unwrap();
        prop_assert_eq!(back.to_canonical_json(), json);
    }

    /// Property 2a': the *value* fixed point, the other promise. Bytes can hold
    /// while a value that encodes to some other value's bytes loses this.
    #[test]
    fn canonical_value_fixed_point(md in document()) {
        let rt = from_markdown(&md).unwrap();
        let back = Content::from_canonical_json(&rt.to_canonical_json()).unwrap();
        prop_assert_eq!(back, rt);
    }

    /// Property 2b: canonical bytes are insensitive to mark *discovery* order.
    /// Islands are ordered by slot position, so only mark order is free.
    #[test]
    fn canonical_json_order_insensitive(md in document()) {
        let rt = from_markdown(&md).unwrap();
        let mut shuffled = rt.clone().into_content();
        shuffled.marks.reverse();
        prop_assert_eq!(
            rt.to_canonical_json(),
            shuffled.into_normalized().to_canonical_json()
        );
    }

    /// Property 3: an anchor over surviving text is carried forward.
    #[test]
    fn diff_import_preserves_surviving_anchor(a in "[a-z]{3,8}", b in "[a-z]{3,8}") {
        let base_md = format!("keep {a} here");
        let mut base = from_markdown(&base_md).unwrap().into_content();
        let start = 5;
        let end = 5 + a.chars().count();
        prop_assert_eq!(&base.text[start..end], a.as_str());
        base.marks.push(Mark::new(start, end, MarkKind::Anchor { id: "c1".into() }));
        let base = base.into_normalized();

        let new_md = format!("{b} keep {a} here");
        let (new_rt, _delta) = diff_import(&base, &new_md).unwrap();
        let anchor = new_rt.marks.iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"));
        prop_assert!(anchor.is_some(), "anchor lost across surviving edit");
        let anchor = anchor.unwrap();
        prop_assert_eq!(&new_rt.text[anchor.start..anchor.end], a.as_str());
    }

    /// Property 3': an unknown mark has no markdown projection either, so the
    /// fresh import cannot re-derive it and diff-import carries it forward
    /// whole, tag and attrs intact.
    #[test]
    fn diff_import_preserves_surviving_unknown_mark(a in "[a-z]{3,8}", b in "[a-z]{3,8}") {
        let base_md = format!("keep {a} here");
        let mut base = from_markdown(&base_md).unwrap().into_content();
        let start = 5;
        let end = 5 + a.chars().count();
        prop_assert_eq!(&base.text[start..end], a.as_str());
        let kind = MarkKind::Unknown {
            tag: "highlight".into(),
            attrs: json!({ "color": "yellow" }),
        };
        base.marks.push(Mark::new(start, end, kind.clone()));
        let base = base.into_normalized();

        let new_md = format!("{b} keep {a} here");
        let (new_rt, _delta) = diff_import(&base, &new_md).unwrap();
        let mark = new_rt.marks.iter()
            .find(|m| matches!(&m.kind, MarkKind::Unknown { tag, .. } if tag == "highlight"));
        prop_assert!(mark.is_some(), "unknown mark lost across surviving edit");
        let mark = mark.unwrap();
        prop_assert_eq!(&new_rt.text[mark.start..mark.end], a.as_str());
        prop_assert_eq!(&mark.kind, &kind);
    }

}

// Edit-channel invariant properties: a successful apply on a valid content
// leaves it valid.

/// `Op::Retain(n)`, or nothing when `n == 0` (no empty retains in the script).
fn retain(n: usize) -> Vec<Op> {
    if n == 0 {
        vec![]
    } else {
        vec![Op::Retain(n)]
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// `apply_text_delta` preserves `validate()`. A deletion can span an island
    /// slot, and the cascade must drop the backing island so the slot/island
    /// counts stay in sync. The insert charset includes `\r` and bidi controls,
    /// which are stripped rather than left to break an invariant; U+FFFC is
    /// excluded, that insert being rejected outright.
    #[test]
    fn apply_text_delta_preserves_validate(
        md in document(),
        ins in "[a-z0-9 \n\r\u{202E}\u{2069}]{0,10}",
        pos_seed in 0usize..4096,
        del_seed in 0usize..4096,
        is_delete in any::<bool>(),
    ) {
        let mut rt = from_markdown(&md).unwrap();
        prop_assert_eq!(rt.validate(), Ok(()), "import invalid for {:?}", md);
        let len = rt.len_usv();
        let pos = pos_seed % (len + 1);
        let delta = if is_delete {
            let k = del_seed % (len - pos + 1);
            let ops = retain(pos)
                .into_iter()
                .chain(std::iter::once(Op::Delete(k)))
                .chain(retain(len - pos - k))
                .collect();
            Delta { ops }
        } else {
            let ops = retain(pos)
                .into_iter()
                .chain(std::iter::once(Op::Insert(ins.clone())))
                .chain(retain(len - pos))
                .collect();
            Delta { ops }
        };
        if rt.apply_text_delta(&delta).is_ok() {
            prop_assert_eq!(rt.validate(), Ok(()), "text delta broke an invariant");
        }
    }

    /// A surviving anchor carries the exact id it started with. The astral char
    /// in the seeded id would expose byte-level munging.
    #[test]
    fn anchor_id_bit_invariant_under_splice(
        md in document(),
        a_seed in 0usize..4096,
        b_seed in 0usize..4096,
        ins in "[a-z0-9 \n]{0,10}",
        pos_seed in 0usize..4096,
        del_seed in 0usize..4096,
        is_delete in any::<bool>(),
    ) {
        const ID: &str = "anchor-\u{1f4a1}-42";
        let mut rt = from_markdown(&md).unwrap().into_content();
        let len = rt.len_usv();
        let a = a_seed % (len + 1);
        let b = b_seed % (len + 1);
        rt.marks.push(Mark::new(a.min(b), a.max(b), MarkKind::Anchor { id: ID.into() }));
        let mut rt = rt.into_normalized();
        prop_assert_eq!(rt.validate(), Ok(()), "seeded content invalid for {:?}", md);

        let len = rt.len_usv();
        let pos = pos_seed % (len + 1);
        let delta = if is_delete {
            let k = del_seed % (len - pos + 1);
            let ops = retain(pos)
                .into_iter()
                .chain(std::iter::once(Op::Delete(k)))
                .chain(retain(len - pos - k))
                .collect();
            Delta { ops }
        } else {
            let ops = retain(pos)
                .into_iter()
                .chain(std::iter::once(Op::Insert(ins.clone())))
                .chain(retain(len - pos))
                .collect();
            Delta { ops }
        };
        if rt.apply_text_delta(&delta).is_ok() {
            for m in &rt.marks {
                if let MarkKind::Anchor { id } = &m.kind {
                    prop_assert_eq!(id.as_str(), ID, "rebase rewrote an anchor id");
                }
            }
            prop_assert_eq!(rt.validate(), Ok(()), "splice broke an invariant");
        }
    }

    /// `apply_island_ops` preserves `validate()`: an accepted insert lands a
    /// slot and its entry together wherever the position falls, including inside
    /// a code fence, whose line normalization then demotes.
    #[test]
    fn apply_island_ops_preserves_validate(
        md in document(),
        pos_seed in 0usize..4096,
    ) {
        let mut rt = from_markdown(&md).unwrap();
        let at = pos_seed % (rt.len_usv() + 1);
        let op = IslandOp::Insert {
            at,
            island: Island::new("isl-prop".into(), "image".into())
                .with_props(json!({ "url": "ex.com", "alt": "a" })),
        };
        if rt.apply_island_ops(&[op]).is_ok() {
            prop_assert_eq!(rt.validate(), Ok(()), "island op broke an invariant");
        }
    }

    /// `apply_mark_ops` preserves `validate()`: an accepted Add over a clamped
    /// range leaves the content valid (normalization trims edges / drops
    /// zero-width).
    #[test]
    fn apply_mark_ops_preserves_validate(
        md in document(),
        s_seed in 0usize..4096,
        e_seed in 0usize..4096,
    ) {
        let mut rt = from_markdown(&md).unwrap();
        let len = rt.len_usv();
        let a = s_seed % (len + 1);
        let b = e_seed % (len + 1);
        let op = MarkOp::Add { start: a.min(b), end: a.max(b), kind: MarkKind::Strong };
        if rt.apply_mark_ops(&[op]).is_ok() {
            prop_assert_eq!(rt.validate(), Ok(()), "mark op broke an invariant");
        }
    }

    /// `apply_line_ops` preserves `validate()` across an accepted
    /// split/join/set-kind. Split/join splice a `\n` and rebase marks through
    /// that one-char change, so keeping the imported marks exercises the remap.
    #[test]
    fn apply_line_ops_preserves_validate(
        md in document(),
        pos_seed in 0usize..4096,
        line_seed in 0usize..64,
        which in 0u8..3,
    ) {
        let mut rt = from_markdown(&md).unwrap();
        let len = rt.len_usv();
        let nlines = rt.lines.len().max(1);
        let op = match which {
            0 => LineOp::Split { at: pos_seed % (len + 1) },
            1 => LineOp::Join { line: line_seed % nlines },
            _ => LineOp::SetKind { line: line_seed % nlines, kind: LineKind::Heading { level: 2 } },
        };
        if rt.apply_line_ops(&[op]).is_ok() {
            prop_assert_eq!(rt.validate(), Ok(()), "line op broke an invariant");
        }
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
    let rt = from_markdown(&md).unwrap();
    rt.islands[0].props["header"].as_array().unwrap().clone()
}

/// A single-table content with the given (possibly ill-shaped) props. `id` is the
/// first-island id import mints (`isl-0`), so a re-imported table compares equal.
fn table_content(aligns: Vec<&str>, header: Vec<Value>, rows: Vec<Vec<Value>>) -> Content {
    Content::new("\u{FFFC}".into(), vec![Line::new(LineKind::Island)]).with_islands(vec![
        Island::new("isl-0".into(), "table".into())
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

        let rt = table_content(aligns.clone(), header_cells, row_cells).into_normalized();
        prop_assert_eq!(rt.validate(), Ok(()), "normalized table invalid");

        let props = &rt.islands[0].props;
        prop_assert_eq!(props["header"].as_array().unwrap().len(), cols);
        prop_assert_eq!(props["aligns"].as_array().unwrap().len(), cols);
        for row in props["rows"].as_array().unwrap() {
            prop_assert_eq!(row.as_array().unwrap().len(), cols);
        }

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        prop_assert_eq!(&rt, &rt2, "table not a fixed point.\n  md: {:?}", md);
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
        let rt = from_markdown(&md).unwrap_or_else(|e| panic!("import {name}: {e}"));
        assert_eq!(rt.validate(), Ok(()), "{name} invariants");
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap();
        assert_eq!(rt, rt2, "{name} content not a fixed point");
    }
}
