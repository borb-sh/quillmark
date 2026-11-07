//! The line form's markdown export: each call lifts the value onto the
//! content model with [`crate::migrate::from_lines`] and exports it there
//! ([`crate::markdown`]). [`to_plaintext`], [`char_count`] and
//! [`line_segments`] read the line form itself.

use crate::lines::model::{Content, ISLAND_SLOT, Lines};
use crate::migrate::from_lines;

pub use crate::export::{clip_range_to_atomic, tex_is_writable, url_is_writable};
pub use crate::markdown::{AnchorRead, Annotated};

/// Render a content to markdown.
pub fn to_markdown(rt: &Lines) -> String {
    crate::export::to_markdown(&from_lines(rt))
}

/// The two-way read: [`crate::markdown::to_markdown_annotated`].
pub fn to_markdown_annotated(rt: &Lines) -> Annotated {
    crate::export::to_markdown_annotated(&from_lines(rt))
}

/// `props` with every anchor mark of each `{text, marks}` it holds gone.
pub(crate) fn drop_anchor_marks(props: &mut serde_json::Value) {
    match props {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(marks)) = map.get_mut("marks") {
                marks.retain(|m| m.get("type").and_then(|t| t.as_str()) != Some("anchor"));
            }
            map.values_mut().for_each(drop_anchor_marks);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(drop_anchor_marks),
        _ => {}
    }
}

/// Render a content to plaintext: [`Content::text`] with island slots
/// ([`ISLAND_SLOT`]) removed. The lossy sibling of [`to_markdown`]: it drops
/// every mark and island, keeping only literal text.
///
/// Tables and images having no plaintext form is a **decided limitation**: the
/// acroform backend fills a form field from this projection, so a field bound to
/// a table-bearing content renders the surrounding text and omits the table,
/// under a `backend::declined_construct` warning, rather than emitting a
/// row/tab dump that would read as faithful.
pub fn to_plaintext(rt: &Content) -> String {
    rt.text.chars().filter(|&c| c != ISLAND_SLOT).collect()
}

/// The character count a text cap (`max:`) measures on a plaintext projection:
/// Unicode scalar values, trailing line breaks aside.
pub fn char_count(text: &str) -> usize {
    text.trim_end_matches(['\n', '\r']).chars().count()
}

/// One line's char range `[start, end)` into the content, with the matching byte
/// range and the count of island slots before the line, so a caller indexes the
/// content text and the island list in O(1) without rescanning. A derived view
/// [`line_segments`] recomputes whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// scalar index of the line's first char.
    pub start: usize,
    /// scalar index one past the line's last char (its `\n`, or the content end).
    pub end: usize,
    /// Byte offset of `start` in the content text.
    pub byte_start: usize,
    /// Byte offset of `end` in the content text.
    pub byte_end: usize,
    /// Count of [`ISLAND_SLOT`] chars before `start`.
    pub slots_before: usize,
}

/// Per-line char/byte ranges and slot prefixes over a content, in line order.
pub fn line_segments(rt: &Content) -> Vec<Segment> {
    let mut segs = Vec::with_capacity(rt.lines.len());
    let mut start = 0usize;
    let mut byte_start = 0usize;
    let mut slots_before = 0usize;
    let mut line_slots = 0usize;
    let mut pos = 0usize;
    for (b, c) in rt.text.char_indices() {
        if c == '\n' {
            segs.push(Segment {
                start,
                end: pos,
                byte_start,
                byte_end: b,
                slots_before,
            });
            start = pos + 1;
            byte_start = b + 1; // `\n` is one byte
            slots_before += line_slots;
            line_slots = 0;
        } else if c == ISLAND_SLOT {
            line_slots += 1;
        }
        pos += 1;
    }
    segs.push(Segment {
        start,
        end: pos,
        byte_start,
        byte_end: rt.text.len(),
        slots_before,
    });
    // Defensive: a malformed content (lines.len() != segments) still gets one
    // segment per line so indexing never panics.
    let total_slots = slots_before + line_slots;
    while segs.len() < rt.lines.len() {
        segs.push(Segment {
            start: pos,
            end: pos,
            byte_start: rt.text.len(),
            byte_end: rt.text.len(),
            slots_before: total_slots,
        });
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{annotate, emit_url};
    use crate::lines::import::from_markdown;
    use crate::lines::island::IslandType;
    use crate::lines::model::{Container, Island, LineKind};
    use crate::lines::model::{Line, Mark, MarkKind};

    /// export∘import is the identity on the content.
    /// A content built by hand, as a client writing `Content` directly builds
    /// one, normalized the way every write lane normalizes it.
    fn stored(text: &str, containers: Vec<Vec<Container>>) -> Lines {
        let lines = containers
            .into_iter()
            .map(|c| {
                let mut l = crate::lines::model::Line::new(LineKind::Para);
                l.containers = c;
                l
            })
            .collect();
        let rt = Content::new(text.to_string(), lines).into_lines();
        rt.validate().expect("stored content validates");
        rt
    }

    /// The lane [`Lines`] does not close: a Rust embedder hand-builds a
    /// content nested past `MAX_NESTING_DEPTH`, which `normalize` cannot repair
    /// and no mint rejects.
    #[test]
    fn nesting_past_what_validate_allows_projects_rather_than_overflowing() {
        const DEPTH: usize = 10_000;
        let mut line = Line::new(LineKind::Para);
        line.containers = vec![Container::Quote { instance: 0 }; DEPTH];
        let rt = Content::new("x".to_string(), vec![line]).into_lines();
        assert!(rt.validate().is_err(), "the shape `validate` refuses");
        assert_eq!(to_markdown(&rt), format!("{}x", "> ".repeat(DEPTH)));
    }

    /// Coincident marks nest in the stored order, `MarkKind::sort_key` at a tie,
    /// so this projection's bytes move when that key does. Both spellings
    /// re-import to one content: the fixed point is over the content, not over
    /// the markdown.
    #[test]
    fn coincident_marks_project_in_the_stored_order() {
        let rt = Content::new("x".to_string(), vec![Line::new(LineKind::Para)])
            .with_marks(vec![
                Mark::new(0, 1, MarkKind::Strong),
                Mark::new(0, 1, MarkKind::Strike),
            ])
            .into_lines();
        rt.validate().expect("validates");
        assert_eq!(to_markdown(&rt), "~~**x**~~");
        assert_eq!(from_markdown("**~~x~~**").unwrap().content, rt);
    }

    fn li(ordinal: u64, instance: u64) -> Vec<Container> {
        vec![Container::ListItem {
            ordered: false,
            start: 1,
            ordinal,
            checked: None,
            instance,
        }]
    }

    fn oli(ordinal: u64, instance: u64) -> Vec<Container> {
        vec![Container::ListItem {
            ordered: true,
            start: 1,
            ordinal,
            checked: None,
            instance,
        }]
    }

    fn note(text: &str, id: &str) -> Island {
        Island::new(id.into(), IslandType::Footnote)
            .with_props(serde_json::json!({ "text": text, "marks": [] }))
    }

    fn math(tex: &str, display: bool, id: &str) -> Island {
        let props = if display {
            serde_json::json!({ "tex": tex, "display": true })
        } else {
            serde_json::json!({ "tex": tex })
        };
        Island::new(id.into(), IslandType::Math).with_props(props)
    }

    /// A reference abutting what would read it as a link or a definition
    /// escapes that char; a note opening on what would open a block escapes
    /// it, its edge line breaks dropping and an inner one written as `<br>`.
    #[test]
    fn a_footnote_reference_and_its_note_round_trip() {
        let rt = Content::new(
            "\u{FFFC}: a\u{FFFC}(b)\n\u{FFFC}".into(),
            vec![
                Line::new(LineKind::Para),
                Line::new(LineKind::Heading { level: 2 }),
            ],
        )
        .with_islands(vec![
            note("\n# x\ny\n", "isl-0"),
            note("1. z", "isl-1"),
            note("", "isl-2"),
        ])
        .into_lines();
        rt.validate().expect("validates");
        assert_eq!(
            crate::lines::serial::parse_cell(&rt.islands[0].props).0,
            "# x\ny"
        );
        let md = to_markdown(&rt);
        assert_eq!(
            md,
            "[^1]\\: a[^2]\\(b)\n\n## [^3]\n\n[^1]: \\# x<br>y\n\n[^2]: 1\\. z\n\n[^3]: "
        );
        assert_eq!(from_markdown(&md).unwrap().content, rt);
    }

    /// Link text holds no footnote reference, so a link over one parts around
    /// it: the text and the note come back, the link as two.
    #[test]
    fn a_link_over_a_footnote_reference_parts_around_it() {
        let rt = Content::new("a\u{FFFC}b".into(), vec![Line::new(LineKind::Para)])
            .with_marks(vec![Mark::new(0, 3, MarkKind::Link { url: "u".into() })])
            .with_islands(vec![note("n", "isl-0")])
            .into_lines();
        let md = to_markdown(&rt);
        assert_eq!(md, "[a](u)[^1]<!---->[b](u)\n\n[^1]: n");
        let back = from_markdown(&md).unwrap().content;
        assert_eq!((&back.text, &back.islands), (&rt.text, &rt.islands));
        assert_eq!(back.marks.len(), 2);
    }

    /// An equation writes between its delimiters, and a `$` of the text
    /// escapes only where another `$` or an equation could pair with it.
    #[test]
    fn math_and_the_dollars_beside_it_round_trip() {
        let cases: &[(&str, Vec<Island>, &str)] = &[
            (
                "a\u{FFFC} costs $5",
                vec![math("x^2", false, "isl-0")],
                "a$x^2$ costs \\$5",
            ),
            ("$5 or $6", vec![], "\\$5 or \\$6"),
            ("$5", vec![], "$5"),
            (
                "\u{FFFC}",
                vec![math("\\frac{a}{b}", true, "isl-0")],
                "$$\\frac{a}{b}$$",
            ),
            (
                "p\u{FFFC}q",
                vec![math("\\{a\\} \\$", false, "isl-0")],
                "p$\\{a\\} \\$$q",
            ),
        ];
        for (text, islands, md) in cases {
            let rt = Content::new((*text).into(), vec![Line::new(LineKind::Para)])
                .with_islands(islands.clone())
                .into_lines();
            assert_eq!(to_markdown(&rt), *md, "{text:?}");
            assert_eq!(from_markdown(md).unwrap().content, rt, "{md:?}");
        }
        let cell = Content::new("\u{FFFC}".into(), vec![Line::new(LineKind::Para)])
            .with_islands(vec![
                Island::new("isl-0".into(), IslandType::Table).with_props(serde_json::json!({
                    "header": [{"text": "$a$", "marks": []}],
                    "aligns": ["none"],
                    "rows": [],
                })),
            ])
            .into_lines();
        let md = to_markdown(&cell);
        assert!(md.starts_with("| \\$a\\$ |"), "{md:?}");
        assert_eq!(from_markdown(&md).unwrap().content, cell);
    }

    /// TeX reads back between its delimiters only where it opens and closes
    /// on no space, holds no bare `$`, and balances its braces.
    #[test]
    fn tex_is_writable_where_its_delimiters_read_it_back() {
        for (tex, display, writable) in [
            ("x", false, true),
            ("a \\$ b", false, true),
            ("\\{", false, true),
            ("", false, false),
            ("", true, true),
            ("a$b", false, false),
            ("{a", false, false),
            ("a}", true, false),
        ] {
            assert_eq!(tex_is_writable(tex, display), writable, "{tex:?} {display}");
        }
    }

    /// A task marker opens its item's paragraph: an item opening on any other
    /// block writes the marker alone above it, and an empty one ends on a
    /// space, the marker reading as one only before whitespace.
    #[test]
    fn a_task_item_writes_its_marker_where_it_reads_back() {
        for md in [
            "- [x] a\n\n- [ ]\n  # h",
            "1. [ ]\n   > q\n\n2. [x] ",
            "- a\n\n  - [ ] ",
        ] {
            let rt = from_markdown(md).unwrap().content;
            assert_eq!(to_markdown(&rt), md);
        }
    }

    /// `instance` spells these, and the marker alternation CommonMark already
    /// reads carries them through markdown. Each is checked *from storage*, not
    /// from an import, since a client writing `Content` directly is the lane
    /// that mints them.
    #[test]
    fn adjacent_sibling_containers_project_and_return() {
        let cases: &[(Lines, &str)] = &[
            // Two one-item lists, which weld into one without a discriminator
            // with an unnumbered continuation paragraph.
            (stored("a\nb", vec![li(0, 0), li(0, 1)]), "- a\n\n+ b"),
            // Which is still distinct from one item spanning two paragraphs.
            (stored("a\nb", vec![li(0, 0), li(0, 0)]), "- a\n\n  b"),
            // A one-item list then a longer one: no ordinal below 0 to restart
            // to, so nothing but `instance` can hold this apart.
            (
                stored("a\nb\nc", vec![li(0, 0), li(0, 1), li(1, 1)]),
                "- a\n\n+ b\n\n+ c",
            ),
            // Two adjacent quotes, which markdown spells with the blank line.
            (
                stored(
                    "a\nb",
                    vec![
                        vec![Container::Quote { instance: 0 }],
                        vec![Container::Quote { instance: 1 }],
                    ],
                ),
                "> a\n\n> b",
            ),
            // Still distinct from one quote of two paragraphs.
            (
                stored(
                    "a\nb",
                    vec![
                        vec![Container::Quote { instance: 0 }],
                        vec![Container::Quote { instance: 0 }],
                    ],
                ),
                "> a\n>\n> b",
            ),
        ];
        for (rt, expected) in cases {
            let md = to_markdown(rt);
            assert_eq!(&md, expected);
            assert_eq!(
                &from_markdown(&md).unwrap().content,
                rt,
                "{md:?} did not return"
            );
        }
    }

    /// `+ ***` is a list item holding a thematic break; `* ***` would be four
    /// spaced asterisks, a break outright that takes the item with it — which is
    /// why the second bullet is `+`.
    #[test]
    fn alternate_markers_do_not_collide_with_a_rule() {
        let mut rt = stored("a\n\nb", vec![li(0, 0), li(0, 1), li(1, 1)]).into_content();
        rt.lines[1].kind = LineKind::Rule;
        let rt = rt.into_lines();
        rt.validate().expect("validates");
        assert_eq!(to_markdown(&rt), "- a\n\n+ ***\n\n+ b");
        assert_eq!(from_markdown(&to_markdown(&rt)).unwrap().content, rt);

        let mut rt = stored("a\n\nb", vec![oli(0, 0), oli(0, 1), oli(1, 1)]).into_content();
        rt.lines[1].kind = LineKind::Rule;
        let rt = rt.into_lines();
        assert_eq!(to_markdown(&rt), "1. a\n\n1) ***\n\n2) b");
        assert_eq!(from_markdown(&to_markdown(&rt)).unwrap().content, rt);
    }

    fn round_trips(md: &str) {
        let rt = from_markdown(md).unwrap().content;
        let md2 = to_markdown(&rt);
        let rt2 = from_markdown(&md2).unwrap().content;
        assert_eq!(
            rt, rt2,
            "content not a fixed point.\n  in:  {md:?}\n  mid: {md2:?}"
        );
    }

    /// A `!` before a link stays text, not the `!` of an image.
    #[test]
    fn a_bang_before_a_link_round_trips() {
        round_trips("\\![i](y)");
        round_trips("wow!![i](y)");
    }

    /// The link arm must route its display text through the island renderer
    /// rather than emit the slot char raw, which re-imports as nothing.
    #[test]
    fn link_over_island_slot_round_trips() {
        round_trips("[![a cat](cat.png)](https://e.com)");
        round_trips("[a ![cat](cat.png) b](https://e.com)");
        assert_eq!(
            to_markdown(
                &from_markdown("[![a cat](cat.png)](https://e.com)")
                    .unwrap()
                    .content
            ),
            "[![a cat](cat.png)](https://e.com)"
        );
    }

    /// Formatting over part of a link's text writes inside its brackets, and a
    /// mark crossing the link's edge parts there into a piece either side.
    #[test]
    fn marks_inside_link_text_round_trip() {
        round_trips("see [the *official* docs](https://x.io) now");
        round_trips("[the `code` docs](u)");
        round_trips("[the ~~code~~ docs](u)");
        round_trips("[the <u>code</u> docs](u)");
        round_trips("[the <span style=\"font-family: Fira\">code</span> docs](u)");
        round_trips("**[the code docs](u)**");
        let rt = marked(
            "see the code docs",
            vec![
                Mark {
                    start: 0,
                    end: 10,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 4,
                    end: 17,
                    kind: MarkKind::Link { url: "u".into() },
                },
            ],
        );
        assert_eq!(to_markdown(&rt), "**see** [**the co**de docs](u)");
    }

    /// Editor-buildable, not importable. A code span is emitted verbatim, so it
    /// splits around the slot, keeping both the text and the island.
    #[test]
    fn code_mark_over_island_slot_keeps_the_island() {
        let mut rt = from_markdown("a ![x](y.png) b")
            .unwrap()
            .content
            .into_content();
        rt.marks.push(Mark {
            start: 0,
            end: 5,
            kind: MarkKind::Code,
        });
        let rt = rt.into_lines();
        assert_eq!(rt.validate(), Ok(()));
        let md = to_markdown(&rt);
        assert_eq!(md, "`a `![x](y.png)` b`");
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, rt.text);
        assert_eq!(rt2.islands.len(), 1);
    }

    #[test]
    fn plaintext_drops_marks_and_islands() {
        let rt = marked(
            "bold text",
            vec![Mark {
                start: 0,
                end: 4,
                kind: MarkKind::Strong,
            }],
        );
        assert_eq!(to_plaintext(&rt), "bold text");
        let rt = Content {
            text: format!("see {ISLAND_SLOT} here"),
            lines: vec![Line {
                kind: LineKind::Para,
                containers: vec![],
                continues: false,
            }],
            marks: vec![],
            islands: vec![Island {
                id: String::new(),
                island_type: IslandType::Image,
                props: serde_json::Value::Null,
            }],
        }
        .into_lines();
        assert_eq!(to_plaintext(&rt), "see  here");
    }

    /// A single-paragraph content with hand-placed `marks`: the free-overlap
    /// shapes an editor produces but markdown import never does.
    fn marked(text: &str, marks: Vec<Mark>) -> Lines {
        let rt = Content {
            text: text.to_string(),
            lines: vec![Line {
                kind: LineKind::Para,
                containers: vec![],
                continues: false,
            }],
            marks,
            islands: vec![],
        }
        .into_lines();
        assert_eq!(rt.validate(), Ok(()), "content invariants");
        rt
    }

    /// One deterministic fixed-point smoke per construct `properties.rs` fuzzes,
    /// labeled so a break localizes without a proptest seed. Constructs with no
    /// generator coverage stay as their own tests below.
    #[test]
    fn single_constructs_round_trip() {
        for (label, md) in [
            ("paragraph", "Hello world"),
            ("two_paragraphs", "one\n\ntwo"),
            ("marks", "a **b** _c_ ~~d~~ <u>e</u>"),
            ("heading", "## Title here"),
            ("inline_code", "run `cargo test` now"),
            ("bullet_list", "- a\n- b\n- c"),
            ("ordered_list", "3. a\n4. b"),
            ("multi_paragraph_item", "- first\n\n  second"),
            ("heading_item", "- # Title\n\n  body text"),
            ("rule_item", "* ---"),
            ("thematic_break", "one\n\n***\n\ntwo"),
            ("hard_break", "line one\\\nline two"),
            ("blockquote", "> quoted text"),
            ("link", "see [our site](https://example.com) now"),
            ("table", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
            ("image", "see ![a cat](cat.png) here"),
            ("element", "<qm-keep note=\"x\">\n\npara\n\n</qm-keep>"),
            (
                "element_around_list",
                "<qm-keep>\n\n- a\n\n- b\n\n</qm-keep>",
            ),
            ("element_in_item", "- <qm-keep>\n\n  a\n\n  </qm-keep>"),
        ] {
            println!("construct: {label}");
            round_trips(md);
        }
    }

    #[test]
    fn nested_marks() {
        round_trips("**bold _and italic_**");
    }

    #[test]
    fn code_block() {
        round_trips("```rust\nfn a() {}\nfn b() {}\n```");
    }

    #[test]
    fn thematic_break_canonicalizes_to_stars() {
        for src in ["---", "___", "- - -"] {
            let rt = from_markdown(&format!("one\n\n{src}\n\ntwo"))
                .unwrap()
                .content;
            let md = to_markdown(&rt);
            assert!(md.contains("\n\n***\n\n"), "source: {src}, got: {md:?}");
        }
    }

    /// A rule as a bullet item's first block is the one shape where marker and
    /// rule can spell one token: `- ` + `---` is four spaced dashes, a thematic
    /// break, and a break outranks a list item wherever both readings fit.
    #[test]
    fn rule_opening_a_list_item_keeps_its_item() {
        for md in ["* ---", "+ ---", "- ***", "- ___", "- - ***", "- > ***"] {
            round_trips(md);
        }
        assert_eq!(
            to_markdown(&from_markdown("* ---").unwrap().content),
            "- ***"
        );
        // The shapes that never collide, pinned against a fix that trades one
        // collision for another: swapping the bullet marker to `*`/`+` starts a
        // *new* list, resetting `ordinal` on this item and every one after.
        round_trips("1. ---");
        round_trips("- one\n\n  ---");
        round_trips("- a\n- ***\n- c");
    }

    /// The other half of that collision: not a rule *inside* an item but the
    /// markers themselves. Three nested empty bullets spell `- - - `, a break
    /// that outranks the items spelling it, and the nesting is gone after one
    /// pass. The content moves off the marker line rather than changing a
    /// marker char — the last case is why, since the empty item shares its list
    /// with `a` and a bullet char change would take `a` into a new list.
    #[test]
    fn a_marker_run_that_spells_a_rule_breaks_its_line() {
        for md in ["+ + +", "+ + + + +", "> + + +", "+ + +\n    + a", "+ + + a"] {
            round_trips(md);
        }
    }

    #[test]
    fn literal_asterisks_escaped() {
        round_trips("2 * 3 = 6 and a_b_c");
    }

    #[test]
    fn hard_break_in_list_item() {
        round_trips("- one\\\ntwo\n- three");
    }

    #[test]
    fn leading_ordered_marker_escaped() {
        let mut rt = from_markdown("x").unwrap().content.into_content();
        rt.text = "1. not a list".into();
        let rt = rt.into_lines();
        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap().content;
        assert_eq!(back.lines[0].kind, LineKind::Para);
        assert!(back.lines[0].containers.is_empty());
        assert_eq!(back, rt);
    }

    /// Markdown spells an empty paragraph, and an empty line between hard
    /// breaks, with nothing: beside other content each writes nothing, where a
    /// list item's would push what follows out of the list and a trailing
    /// break's `\` would read as text. A container's only line keeps it.
    #[test]
    fn an_empty_line_beside_other_content_writes_nothing() {
        let item = |checked| Container::ListItem {
            ordered: false,
            start: 1,
            ordinal: 0,
            checked,
            instance: 0,
        };
        let quote = Container::Quote { instance: 0 };
        for (rt, md) in [
            (hard_break_block("a\n"), "a"),
            (hard_break_block("a\n\nb"), "a\\\nb"),
            (stored("\nb", vec![vec![], vec![]]), "b"),
            (stored("\nb", vec![vec![item(None)]; 2]), "- b"),
            (stored("\nb", vec![vec![item(Some(true))]; 2]), "- [x] b"),
            (
                stored(
                    "\nb",
                    vec![vec![item(Some(true))], vec![item(Some(true)), item(None)]],
                ),
                "- [x]\n  - b",
            ),
            (stored("\nb", vec![vec![quote.clone()]; 2]), "> b"),
            (stored("", vec![vec![item(Some(false))]]), "- [ ] "),
        ] {
            let out = to_markdown(&rt);
            assert_eq!(out, md);
            assert_eq!(to_markdown(&from_markdown(&out).unwrap().content), out);
        }
    }

    /// One Para block over the `\n`-separated lines of `text`, joined by hard
    /// breaks: the shape that puts a line under a paragraph line.
    fn hard_break_block(text: &str) -> Lines {
        let lines = (0..text.split('\n').count())
            .map(|i| Line::new(LineKind::Para).with_continues(i > 0))
            .collect();
        let rt = Content::new(text.to_string(), lines).into_lines();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        rt
    }

    /// Markdown strips space and tab from a line's edges, behind a heading,
    /// quote, or list prefix too. The escape also dissolves the block markers a
    /// whitespace prefix hides from the position-0 escapes: `   - item` parses
    /// as a bullet item, and the model holds it as one line of text.
    #[test]
    fn edge_whitespace_survives_export() {
        const TEXTS: &[&str] = &[
            " foo",
            "    foo",
            "\tfoo",
            "foo   ",
            "  foo  ",
            "   ",
            "   - item",
            "  # not a heading",
            "  1. not a list",
            "  > not a quote",
            // A trailing space keeps a heading's `#` out of the closing-sequence
            // shape the position-0 escapes do not reach.
            "a # ",
        ];
        for text in TEXTS {
            for containers in [vec![], vec![Container::Quote { instance: 0 }], li(0, 0)] {
                let rt = stored(text, vec![containers.clone()]);
                let md = to_markdown(&rt);
                assert_eq!(
                    &from_markdown(&md).unwrap().content,
                    &rt,
                    "{text:?} under {containers:?} did not return: {md:?}"
                );
            }
            let mut rt = stored(text, vec![vec![]]).into_content();
            rt.lines[0].kind = LineKind::Heading { level: 2 };
            let rt = rt.into_lines();
            let md = to_markdown(&rt);
            assert_eq!(
                &from_markdown(&md).unwrap().content,
                &rt,
                "heading {text:?} did not return: {md:?}"
            );
        }
    }

    /// Every line of a hard-break block carries the escape, so an edge run
    /// ahead of the `\` survives too.
    #[test]
    fn edge_whitespace_survives_a_hard_break() {
        for text in ["abc   \n   def", "   \nabc", "abc\n   ", "\tabc\ndef\t"] {
            let rt = hard_break_block(text);
            let md = to_markdown(&rt);
            assert_eq!(
                &from_markdown(&md).unwrap().content,
                &rt,
                "{text:?} → {md:?}"
            );
        }
    }

    /// A `=` run continuing a paragraph line underlines it into a setext
    /// heading, taking the hard break's `\` into the text with it.
    #[test]
    fn a_setext_underline_on_a_continuation_line_stays_text() {
        for text in [
            "abc\n===",
            "abc\n=",
            "abc\n=== ",
            "abc\n---",
            "abc\n= b",
            "abc\n   ---",
            "abc\n  ===",
        ] {
            let rt = hard_break_block(text);
            let md = to_markdown(&rt);
            assert_eq!(
                &from_markdown(&md).unwrap().content,
                &rt,
                "{text:?} → {md:?}"
            );
        }
    }

    /// The verify-and-drop net wraps its probe in `,…,` to block every
    /// leading-block construct, so it cannot see the indented code block an
    /// unescaped edge run opens: it reads such a line as safe while the `**`
    /// ships into the text. The escape is the only thing holding this line.
    #[test]
    fn a_marked_line_with_edge_whitespace_keeps_its_delimiters_out_of_the_text() {
        let rt = marked("    foo", vec![Mark::new(4, 7, MarkKind::Strong)]);
        let back = from_markdown(&to_markdown(&rt)).unwrap().content;
        assert_eq!(back.text, "    foo");
        assert_eq!(back, rt);
    }

    /// A cell's edges are trimmed by the pipe split, so cell text carries the
    /// line escape.
    #[test]
    fn table_cell_edge_whitespace_round_trips() {
        let cell = |t: &str| serde_json::json!({"marks": [], "text": t});
        let rt = Content {
            text: ISLAND_SLOT.to_string(),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![],
            islands: vec![Island::new("isl-0".into(), IslandType::Table).with_props(
                serde_json::json!({
                    "aligns": ["none"],
                    "header": [cell(" h ")],
                    "rows": [[cell("  a")], [cell("b  ")], [cell("   ")]],
                }),
            )],
        }
        .into_lines();
        assert_eq!(rt.validate(), Ok(()), "table island invalid");
        let md = to_markdown(&rt);
        assert_eq!(
            &from_markdown(&md).unwrap().content,
            &rt,
            "cell edges lost: {md:?}"
        );
    }

    /// The island writes its line whole, so a mark reaching over its slot
    /// projects as nothing: `[<pipe table>](u)` re-imports as prose with the
    /// island gone, taking the table out of the document.
    #[test]
    fn a_mark_reaching_over_a_block_islands_slot_does_not_wrap_its_markup() {
        let rt = crate::lines::serial::from_canonical_value(&serde_json::json!({
            "text": format!("a\n{ISLAND_SLOT}\nbc"),
            "lines": [{"kind": "para", "containers": []}, {"kind": "para", "containers": []}, {"kind": "para", "containers": []}],
            "marks": [{"start": 0, "end": 6, "type": "link", "attrs": {"url": "u"}}],
            "islands": [{"id": "isl-t", "type": "table", "props": {
                "aligns": ["none"],
                "header": [{"marks": [], "text": "h"}],
                "rows": [[{"marks": [], "text": "c"}]],
            }}],
        }))
        .expect("the slot is alone on its line");
        assert_eq!(rt.text, format!("a\n{ISLAND_SLOT}\nbc"));
        assert_eq!(
            rt.marks,
            vec![
                Mark::new(0, 1, MarkKind::Link { url: "u".into() }),
                Mark::new(2, 3, MarkKind::Link { url: "u".into() }),
                Mark::new(4, 6, MarkKind::Link { url: "u".into() }),
            ],
            "the link splits at each line join, the slot's line included"
        );

        let md = to_markdown(&rt);
        assert_eq!(md, "[a](u)\n\n| h |\n| --- |\n| c |\n\n[bc](u)");
        assert_eq!(
            from_markdown(&md).unwrap().content.islands.len(),
            1,
            "the table left the document: {md:?}"
        );
    }

    #[test]
    fn table_with_formatted_cells_round_trips() {
        round_trips("| Name | Note |\n| --- | --- |\n| **bold** | _italic_ |");
        round_trips("| A |\n| --- |\n| **b** and _i_ `c` [d](https://e.com) ~~e~~ |");
        round_trips("| A |\n| --- |\n| <u>under</u> |");
        // A literal pipe inside a cell survives via `\|` re-escaping on export.
        round_trips("| A |\n| --- |\n| a \\| b |");
    }

    /// A row splits its cells at a `|` inside a code span or a link
    /// destination too, so export writes it `\|` there as in text.
    #[test]
    fn a_pipe_in_a_cells_code_span_or_link_url_stays_in_its_cell() {
        for (body, text, url) in [
            ("`a\\|b`", "a|b", None),
            ("`a\\\\|b`", "a\\|b", None),
            ("[x](https://e.com/a\\|b)", "x", Some("https://e.com/a|b")),
            (
                "[x](<https://e.com/a\\\\\\|b c>)",
                "x",
                Some("https://e.com/a\\|b c"),
            ),
        ] {
            let md = format!("| h |\n| --- |\n| {body} |");
            let rt = from_markdown(&md).unwrap().content;
            let cell = &rt.islands[0].props["rows"][0][0];
            assert_eq!(cell["text"], text, "{md:?}");
            assert_eq!(cell["marks"][0]["attrs"]["url"].as_str(), url, "{md:?}");
            assert_eq!(to_markdown(&rt), md, "{md:?}");
        }
    }

    fn grid_table(props: serde_json::Value) -> Lines {
        with_islands(
            &ISLAND_SLOT.to_string(),
            vec![Island::new("isl-0".into(), IslandType::Table).with_props(props)],
        )
    }

    /// Storage decides the spelling: a table holding nothing a pipe table
    /// cannot write is pipes, one key it cannot write makes it HTML, and
    /// dropping that key makes it pipes again. A value outside its set writes
    /// nothing, and each spelling reads back as the table it writes.
    #[test]
    fn a_table_is_html_exactly_where_pipes_cannot_carry_it() {
        use serde_json::json;
        let cell = |text: &str| json!({"marks": [], "text": text});
        let plain = json!({"aligns": ["right", "none"], "header": [cell("a"), cell("b")], "rows": [[cell("1"), cell("")], [cell("2"), cell("3")]]});
        let keys: &[(&str, serde_json::Value, bool)] = &[
            ("/head", json!(0), true),
            ("/head", json!(2), true),
            ("/widths", json!([1, null]), true),
            ("/align", json!("center"), true),
            ("/rows/0/0/align", json!("left"), true),
            ("/header/1/valign", json!("bottom"), true),
            ("/rows/0/0/colspan", json!(2), true),
            ("/header/0/rowspan", json!(2), false),
            ("/rows/1/1/rowspan", json!(3), false),
            ("/rows/0/0/colspan", json!(1), false),
            ("/rows/0/0/valign", json!("horizon"), false),
            ("/head", json!(1), false),
        ];
        let pipes = to_markdown(&grid_table(plain.clone()));
        assert!(pipes.starts_with("| a | b |"), "{pipes}");
        for (at, value, html) in keys {
            let mut props = plain.clone();
            let (parent, key) = at.rsplit_once('/').unwrap();
            props
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), value.clone());
            let rt = grid_table(props);
            let md = to_markdown(&rt);
            assert_eq!(md.starts_with("<table"), *html, "{at}: {value} wrote {md}");
            if !html {
                assert_eq!(md, pipes, "{at}: {value}");
            }
            let back = from_markdown(&md).unwrap();
            assert!(
                back.warnings.is_empty(),
                "{at}: {value}: {:?}",
                back.warnings
            );
            if *value != json!("horizon") {
                assert_eq!(back.content, rt, "{at}: {value} wrote {md}");
            }
        }
    }

    /// An HTML cell's text writes so no `<` reads as a tag and no `&` as an
    /// entity, a link destination holding a table tag included, and a `|`
    /// needs no escape there. A code span never holds a table tag, so a code
    /// mark over one writes it as escaped text between the spans either side.
    #[test]
    fn an_html_cell_writes_what_would_read_as_markup() {
        use serde_json::json;
        let text = "a | </td> &amp; <tr> x";
        let marks = json!([
            {"start": 2, "end": 9, "type": "code"},
            {"start": 16, "end": 20, "type": "link", "attrs": {"url": "td x</tr>"}},
            {"start": 0, "end": 1, "type": "strong"},
        ]);
        let rt = grid_table(json!({
            "aligns": ["none"], "head": 0,
            "header": [{"marks": marks, "text": text}],
            "rows": [],
        }));
        let md = to_markdown(&rt);
        assert_eq!(
            md,
            "<table>\n<tbody>\n<tr><td>**a** `| `\\</td> \\&amp; [\\<tr>](<td x\\</tr\\>>) x</td></tr>\n</tbody>\n</table>"
        );
        let back = from_markdown(&md).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let (text_back, marks_back) =
            crate::lines::serial::parse_cell(&back.content.islands[0].props["header"][0]);
        assert_eq!(text_back, text);
        assert!(
            marks_back.contains(&Mark::new(2, 4, MarkKind::Code)),
            "{marks_back:?}"
        );
        assert_eq!(to_markdown(&back.content), md);
    }

    #[test]
    fn formatted_cell_marks_are_structured_not_reparsed() {
        let rt = from_markdown("| H |\n| --- |\n| **bold** |")
            .unwrap()
            .content;
        let cell = &rt.islands[0].props["rows"][0][0];
        assert_eq!(cell["text"], "bold");
        assert_eq!(cell["marks"][0]["type"], "strong");
        assert_eq!(cell["marks"][0]["start"], 0);
        assert_eq!(cell["marks"][0]["end"], 4);
        assert!(to_markdown(&rt).contains("**bold**"));
    }

    #[test]
    fn a_cell_line_break_round_trips_as_br() {
        for body in [
            "line1<br>line2",
            "a <br> b",
            "<br>a",
            "a<br>",
            "**a<br>b**",
            "\\<br>",
        ] {
            let md = format!("| h |\n| --- |\n| {body} |");
            let rt = from_markdown(&md).unwrap().content;
            let out = to_markdown(&rt);
            assert_eq!(
                crate::markdown::from_markdown(&out).unwrap().content,
                crate::markdown::from_markdown(&md).unwrap().content,
                "cell {body:?}"
            );
            if body != "**a<br>b**" {
                assert_eq!(out, md, "cell {body:?}");
            }
        }
        let rt = from_markdown("| h |\n| --- |\n| \\<br> |").unwrap().content;
        assert_eq!(rt.islands[0].props["rows"][0][0]["text"], "<br>");
        let rt = from_markdown("| h |\n| --- |\n| a<br><br>b |")
            .unwrap()
            .content;
        assert_eq!(to_markdown(&rt), "| h |\n| --- |\n| a<br>b |");
    }

    #[test]
    fn every_br_spelling_is_one_cell_break() {
        for br in ["<br/>", "<br />", "<BR>", "<br clear=\"all\">"] {
            let rt = from_markdown(&format!("| h |\n| --- |\n| a{br}b |"))
                .unwrap()
                .content;
            assert_eq!(rt.islands[0].props["rows"][0][0]["text"], "a\nb", "{br}");
            assert_eq!(to_markdown(&rt), "| h |\n| --- |\n| a<br>b |", "{br}");
        }
    }

    /// A content of one Para line holding `text`, whose slots the `islands`
    /// back: the shape a wire island op mints.
    fn with_islands(text: &str, islands: Vec<Island>) -> Lines {
        let rt = Content {
            text: text.to_string(),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![],
            islands,
        }
        .into_lines();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        rt
    }

    /// A one-cell table island, the block-only type.
    fn table() -> Island {
        Island::new("isl-0".into(), IslandType::Table).with_props(serde_json::json!({
            "aligns": ["none"],
            "header": [{"marks": [], "text": "h"}],
            "rows": [[{"marks": [], "text": "c"}]],
        }))
    }

    /// A table is block markup, so a slot a paragraph's prose holds gets the
    /// line its markup needs from the write, the mint having left the text as
    /// it was. The prose on each side keeps its marks in the coordinates the
    /// break leaves, and what re-imports is the broken content.
    #[test]
    fn a_block_island_inside_a_paragraph_is_written_on_a_line_of_its_own() {
        let rt = Content {
            text: format!("a{ISLAND_SLOT}bold"),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![Mark::new(2, 6, MarkKind::Strong)],
            islands: vec![table()],
        }
        .into_lines();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        assert_eq!(
            rt.text,
            format!("a{ISLAND_SLOT}bold"),
            "the mint moved text"
        );

        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap().content;
        assert_eq!(back.text, format!("a\n{ISLAND_SLOT}\nbold"), "{md:?}");
        assert_eq!(back.marks, vec![Mark::new(4, 8, MarkKind::Strong)]);
        assert_eq!(
            to_markdown(&back),
            md,
            "the broken content is a fixed point"
        );

        let mut anchored = rt.clone().into_content();
        anchored.marks.push(anchor(2, 6, "c1"));
        let read = to_markdown_annotated(&anchored.into_lines());
        assert_eq!(read.markdown.replace(&tag("c1"), ""), md);
        assert!(
            read.markdown.contains(&format!("{}**bold**", tag("c1"))),
            "{:?}",
            read.markdown
        );
        assert_eq!(read.anchors[0].line, "bold");
    }

    /// A table ending a line sets as a block, so a hard-break continuation after
    /// it would join nothing: the mint clears the flag, and the line below writes
    /// as a paragraph of its own.
    #[test]
    fn a_hard_break_after_a_broken_line_keeps_its_text() {
        let mut second = Line::new(LineKind::Para);
        second.continues = true;
        let rt = Content {
            text: format!("a{ISLAND_SLOT}\nmore"),
            lines: vec![Line::new(LineKind::Para), second],
            marks: vec![],
            islands: vec![table()],
        }
        .into_lines();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        assert!(!rt.lines[1].continues);

        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap().content;
        assert_eq!(back.text, format!("a\n{ISLAND_SLOT}\nmore"), "{md:?}");
    }

    /// An image's markup does re-import as a slot, so the net still expects one
    /// back and a mark that would eat it is dropped. Here both `**` sit against
    /// the image's punctuation, where CommonMark flanking refuses them.
    #[test]
    fn a_mark_leaking_around_an_image_slot_is_still_dropped() {
        let image = || {
            Island::new("isl-0".into(), IslandType::Image)
                .with_props(serde_json::json!({"alt": "a", "url": "u"}))
        };
        let over_slot = Content {
            text: format!("a{ISLAND_SLOT}b"),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![Mark::new(1, 2, MarkKind::Strong)],
            islands: vec![image()],
        }
        .into_lines();
        let md = to_markdown(&over_slot);
        assert_eq!(md, "a![a](u)b");
        let back = from_markdown(&md).unwrap().content;
        assert_eq!(back.text, over_slot.text);
        assert_eq!(back.islands.len(), 1);
        assert!(back.marks.is_empty(), "leaking mark kept: {md:?}");
        // A mark on the same line whose delimiters do flank rides through.
        let before_slot = Content {
            text: format!("a{ISLAND_SLOT}b"),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![Mark::new(0, 1, MarkKind::Strong)],
            islands: vec![image()],
        }
        .into_lines();
        let md = to_markdown(&before_slot);
        assert_eq!(md, "**a**![a](u)b");
        assert_eq!(from_markdown(&md).unwrap().content, before_slot);
    }

    /// An image `alt` is the one place the character reference cannot carry an
    /// edge run: the parser decodes it, then trims alt.
    #[test]
    fn known_image_alt_limit() {
        let rt = with_islands(
            &ISLAND_SLOT.to_string(),
            vec![
                Island::new("isl-0".into(), IslandType::Image)
                    .with_props(serde_json::json!({"alt": " a ", "url": "u"})),
            ],
        );
        let back = from_markdown(&to_markdown(&rt)).unwrap().content;
        assert_eq!(
            back.islands[0].props["alt"], "a",
            "if this ever round-trips, promote it out of the known-limits list"
        );
    }

    /// A mark over a `<br>` line join imports as one mark per line, the form
    /// export writes, so the first import is already the fixed point. The
    /// properties fuzz the `\` join.
    #[test]
    fn a_mark_across_a_hard_break_is_a_fixed_point() {
        for (md, kind) in [
            ("**a<br>x**", MarkKind::Strong),
            ("<u>a<br>x</u>", MarkKind::Underline),
        ] {
            let rt = from_markdown(md).unwrap().content;
            assert_eq!(rt.text, "a\nx", "{md}");
            assert_eq!(
                rt.marks,
                vec![Mark::new(0, 1, kind.clone()), Mark::new(2, 3, kind)],
                "{md}"
            );
            assert_eq!(
                from_markdown(&to_markdown(&rt)).unwrap().content,
                rt,
                "{md}"
            );
        }
    }

    #[test]
    fn anchor_marks_omitted_but_text_survives() {
        let mut rt = from_markdown("comment target here")
            .unwrap()
            .content
            .into_content();
        rt.marks.push(Mark {
            start: 8,
            end: 14,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        let rt = rt.into_lines();
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "comment target here");
        assert!(!md.contains("c1"));
    }

    fn anchor(start: usize, end: usize, id: &str) -> Mark {
        Mark::new(start, end, MarkKind::Anchor { id: id.into() })
    }

    fn tag(id: &str) -> String {
        format!("<qm-anchor ref=\"{id}\"></qm-anchor>")
    }

    /// The read imports as the plain projection does, anchors gone.
    fn reads_as_plain(read: &Annotated, rt: &Lines) {
        let plain = from_markdown(&to_markdown(rt)).unwrap().content;
        assert!(
            plain
                .marks
                .iter()
                .all(|m| !matches!(m.kind, MarkKind::Anchor { .. }))
        );
        assert_eq!(
            from_markdown(&read.markdown).unwrap().content,
            plain,
            "{:?}",
            read.markdown
        );
    }

    fn read_ids(read: &Annotated) -> Vec<&str> {
        read.anchors.iter().map(|a| a.id.as_str()).collect()
    }

    /// A tag sits after the delimiters closing at its position and before those
    /// opening there, so it parts no delimiter run from its text. A range anchor
    /// reads as its start, and two anchors at one position spell in id order.
    #[test]
    fn a_tag_sits_between_the_marks_closing_and_opening_at_its_start() {
        let rt = marked(
            "abcd ef",
            vec![
                Mark::new(0, 2, MarkKind::Strong),
                Mark::new(2, 4, MarkKind::Strike),
                anchor(0, 0, "open"),
                anchor(2, 7, "range"),
                anchor(4, 4, "close"),
                anchor(4, 4, "also"),
            ],
        );
        let read = to_markdown_annotated(&rt);
        assert_eq!(
            read.markdown,
            format!(
                "{}**ab**{}~~cd~~{}{} ef",
                tag("open"),
                tag("range"),
                tag("also"),
                tag("close")
            )
        );
        assert_eq!(read_ids(&read), ["open", "range", "also", "close"]);
        assert!(read.anchors.iter().all(|a| a.line == "abcd ef"));
        reads_as_plain(&read, &rt);
    }

    /// A code span or link has no position inside it a tag could take without
    /// becoming its text, so the tag moves to the span's start, marked
    /// `at="span"` so a revise pins nothing by it.
    #[test]
    fn a_tag_inside_a_code_span_or_link_moves_to_its_start() {
        let rt = marked(
            "run code at site",
            vec![
                Mark::new(4, 8, MarkKind::Code),
                Mark::new(12, 16, MarkKind::Link { url: "u".into() }),
                Mark::new(12, 16, MarkKind::Strong),
                anchor(6, 6, "c"),
                anchor(14, 15, "l"),
            ],
        );
        let read = to_markdown_annotated(&rt);
        assert_eq!(
            read.markdown,
            "run <qm-anchor at=\"span\" ref=\"c\"></qm-anchor>`code` at \
             <qm-anchor at=\"span\" ref=\"l\"></qm-anchor>**[site](u)**"
        );
        reads_as_plain(&read, &rt);
    }

    /// A code-block line, a block island's line and an empty line have no
    /// inline slot that keeps their structure, so they hold no tag; their
    /// anchors are still listed, and so is a table cell's, after the table's
    /// own and with its cell's text.
    #[test]
    fn a_line_with_no_inline_slot_holds_no_tag() {
        let mut rt = from_markdown("```\nfn a() {}\n```\n\n| h |\n| --- |\n| c |\n\n- a\n-\n- b")
            .unwrap()
            .content
            .into_content();
        assert_eq!(rt.text, format!("fn a() {{}}\n{ISLAND_SLOT}\na\n\nb"));
        rt.marks.extend([
            anchor(3, 3, "code"),
            anchor(10, 10, "table"),
            anchor(14, 14, "empty"),
        ]);
        rt.islands[0].props["rows"][0][0]["marks"] =
            serde_json::json!([{"attrs": {"id": "cell"}, "end": 1, "start": 0, "type": "anchor"}]);
        let rt = rt.into_lines();
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(
            rt.islands[0].props["rows"][0][0]["marks"][0]["type"],
            "anchor"
        );

        let read = to_markdown_annotated(&rt);
        assert_eq!(read.markdown, to_markdown(&rt));
        let lines: Vec<(&str, &str)> = read
            .anchors
            .iter()
            .map(|a| (a.id.as_str(), a.line.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                ("code", "fn a() {}"),
                ("table", ""),
                ("cell", "c"),
                ("empty", "")
            ]
        );
    }

    /// A heading's trailing `#` keeps the escape that holds it out of an ATX
    /// closing sequence wherever its tag lands, so the read is the plain
    /// projection with tags added.
    #[test]
    fn a_heading_keeps_its_trailing_hash_escape_under_a_tag() {
        let mut rt = from_markdown("# a \\#").unwrap().content.into_content();
        assert_eq!(rt.text, "a #");
        rt.marks
            .extend([anchor(2, 2, "before"), anchor(3, 3, "after")]);
        let rt = rt.into_lines();
        let read = to_markdown_annotated(&rt);
        assert_eq!(
            read.markdown,
            format!("# a {}\\#{}", tag("before"), tag("after"))
        );
        reads_as_plain(&read, &rt);
    }

    /// The tag is the carrier's canonical spelling, so an id holding markup
    /// characters is entity-encoded and the import still drops the tag whole.
    #[test]
    fn an_anchor_id_is_entity_encoded() {
        let rt = marked("ab", vec![anchor(1, 1, "a\"&<b>")]);
        let read = to_markdown_annotated(&rt);
        assert_eq!(
            read.markdown,
            "a<qm-anchor ref=\"a&quot;&amp;&lt;b&gt;\"></qm-anchor>b"
        );
        assert_eq!(read_ids(&read), ["a\"&<b>"]);
        reads_as_plain(&read, &rt);
    }

    /// Every tag on a line moves to its end where a tag in place would change
    /// what the line imports to, marked `at="line"`, and none is written where
    /// the end changes it too. The lines and points here are hand-made, so
    /// each case is met.
    #[test]
    fn a_line_its_tags_would_change_holds_them_at_its_end_or_not_at_all() {
        // `!` and `[x](u)` parted by a tag are text and a link, not an image.
        assert_eq!(
            annotate("![x](u) y".into(), &[0, 1], &[(1, "t")]),
            "![x](u) y<qm-anchor at=\"line\" ref=\"t\"></qm-anchor>"
        );
        // A trailing `\` escapes the `<` that would open the tag.
        assert_eq!(annotate("a\\".into(), &[0, 1, 2], &[(2, "t")]), "a\\");
    }

    /// `strong` and `emph` share the `*` delimiter, so their overlap is
    /// unrepresentable: a naive `**ab*cdef*` re-imports as literal text. Export
    /// nests them by truncation instead, keeping the text.
    #[test]
    fn overlapping_asterisk_marks_stay_text_safe() {
        let rt = marked(
            "abcdef",
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 2,
                    end: 6,
                    kind: MarkKind::Emph,
                },
            ],
        );
        let md = to_markdown(&rt);
        assert_eq!(md, "**ab*cd***ef", "balanced, no literal `**` leak");
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "abcdef");
        // Documented limit: same-delimiter overlap degrades to its nested subset.
        assert_eq!(
            rt2.marks,
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong
                },
                Mark {
                    start: 2,
                    end: 4,
                    kind: MarkKind::Emph
                },
            ]
        );
    }

    /// An underline's tags sit outside the delimiters opening and closing at
    /// their position, so neither changes how a delimiter flanks.
    #[test]
    fn an_underline_sharing_an_edge_with_a_delimiter_writes_outside_it() {
        for (text, under, strong, md) in [
            ("xab", (1, 3), (1, 2), "x<u>**a**b</u>"),
            ("abx", (0, 2), (1, 2), "<u>a**b**</u>x"),
        ] {
            let rt = marked(
                text,
                vec![
                    Mark {
                        start: under.0,
                        end: under.1,
                        kind: MarkKind::Underline,
                    },
                    Mark {
                        start: strong.0,
                        end: strong.1,
                        kind: MarkKind::Strong,
                    },
                ],
            );
            assert_eq!(to_markdown(&rt), md);
            assert_eq!(from_markdown(md).unwrap().content, rt);
        }
    }

    /// A font mark is a `qm-span` pair, its tags outside the underlines' and
    /// delimiters' at a position, so a second export spells the first.
    /// Pairs never nest: nested markdown comes back as one flat pair per run.
    #[test]
    fn a_font_mark_writes_flat_pairs() {
        for (md, want) in [
            (
                "The <qm-span family=\"Carlito\">phrase</qm-span> and **bold <qm-span family=\"Arimo\">sans</qm-span>**.",
                "The <qm-span family=\"Carlito\">phrase</qm-span> and **bold <qm-span family=\"Arimo\">sans**</qm-span>.",
            ),
            (
                "<qm-span family=\"A\">a<qm-span family=\"B\">b</qm-span>c</qm-span>",
                "<qm-span family=\"A\">a</qm-span><qm-span family=\"B\">b</qm-span><qm-span family=\"A\">c</qm-span>",
            ),
            (
                "x<qm-span family=\"A\"><u>**a**</u>b</qm-span>",
                "x<qm-span family=\"A\"><u>**a**</u>b</qm-span>",
            ),
            (
                "*a<qm-span family=\"A\">b*c</qm-span>",
                "*a<qm-span family=\"A\">b*c</qm-span>",
            ),
            (
                "<qm-span family=\"A\">a `b` c</qm-span>",
                "<qm-span family=\"A\">a `b` c</qm-span>",
            ),
            (
                "| <qm-span family=\"A&#x7C;B &quot;C&quot;\">a</qm-span> |\n| --- |",
                "| <qm-span family=\"A&#x7C;B &quot;C&quot;\">a</qm-span> |\n| --- |",
            ),
            (
                "a[^1]\n\n[^1]: <qm-span family=\"A\">n</qm-span>",
                "a[^1]\n\n[^1]: <qm-span family=\"A\">n</qm-span>",
            ),
        ] {
            let rt = from_markdown(md).unwrap().content;
            assert_eq!(to_markdown(&rt), want, "export of {md:?}");
            assert_eq!(
                from_markdown(want).unwrap().content,
                rt,
                "{want:?} did not return"
            );
        }

        let family = |start, end, name: &str| Mark {
            start,
            end,
            kind: MarkKind::FontFamily { name: name.into() },
        };
        for (marks, md) in [
            (
                vec![family(0, 2, "A"), family(2, 6, "B")],
                "<qm-span family=\"A\">ab</qm-span><qm-span family=\"B\">cdef</qm-span>",
            ),
            (
                vec![family(1, 3, "B"), family(4, 6, "A")],
                "a<qm-span family=\"B\">bc</qm-span>d<qm-span family=\"A\">ef</qm-span>",
            ),
        ] {
            let rt = marked("abcdef", marks);
            assert_eq!(to_markdown(&rt), md);
            assert_eq!(from_markdown(md).unwrap().content, rt);
        }
    }

    /// A formatting mark partially overlapping an atomic `code` span clips to
    /// the text outside it, so the markdown stays balanced.
    #[test]
    fn wrap_over_code_stays_balanced() {
        let rt = marked(
            "abcdef",
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 2,
                    end: 6,
                    kind: MarkKind::Code,
                },
            ],
        );
        let md = to_markdown(&rt);
        assert_eq!(md, "**ab**`cdef`");
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "abcdef");
    }

    /// CommonMark reads a code span's edge backtick as part of the fence run and
    /// strips one space off each side of a span that begins *and* ends with one,
    /// so both shapes need the pad the parser then removes. A span of nothing but
    /// spaces is exempt from the strip, so a pad there would grow it.
    #[test]
    fn code_span_edges_keep_their_pad() {
        for (md, text, want) in [
            ("`` `a ``", "`a", "`` `a ``"),
            ("`` a` ``", "a`", "`` a` ``"),
            ("`` `a` ``", "`a`", "`` `a` ``"),
            ("`` ` ``", "`", "`` ` ``"),
            ("`  a  `", " a ", "`  a  `"),
            ("`a`", "a", "`a`"),
            ("`  `", "  ", "`  `"),
        ] {
            let rt = from_markdown(md).unwrap().content;
            assert_eq!(rt.text, text, "import of {md:?}");
            assert_eq!(to_markdown(&rt), want);
            round_trips(md);
        }
    }

    /// Entity-shaped text must not re-import as the decoded entity: exporting
    /// the content text "&amp;" unescaped would re-import as "&".
    #[test]
    fn ampersand_and_entities_round_trip() {
        round_trips("a & b");
        round_trips("copyright \\&copy; sign");
        let rt = from_markdown("\\&amp;").unwrap().content;
        assert_eq!(rt.text, "&amp;");
        let md = to_markdown(&rt);
        assert!(md.contains("\\&"), "the `&` must be escaped, got {md:?}");
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "&amp;", "entity-shaped text must not decode");
        assert_eq!(rt, rt2);
    }

    /// Heading text ending in a `#` run must not re-import as an ATX closing
    /// sequence: `# a #` would come back as "a", dropping the `#`.
    #[test]
    fn heading_trailing_hash_round_trips() {
        let rt = from_markdown("# a \\#").unwrap().content;
        assert_eq!(rt.text, "a #");
        let md = to_markdown(&rt);
        assert!(
            md.contains("\\#"),
            "trailing `#` must be escaped, got {md:?}"
        );
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "a #", "trailing `#` must survive");
        assert_eq!(rt, rt2);
        round_trips("# heading \\#\\#");
        round_trips("## title\\#");
    }

    #[test]
    fn emit_url_bare_when_safe() {
        let mut bare = String::new();
        emit_url("https://ex.com/a(b)c", &mut bare);
        assert_eq!(bare, "https://ex.com/a(b)c", "balanced parens stay bare");
        let mut wrapped = String::new();
        emit_url("a b", &mut wrapped);
        assert_eq!(wrapped, "<a b>", "space forces the wrap");
        let mut esc = String::new();
        emit_url("a&<\\b", &mut esc);
        assert_eq!(esc, "<a\\&\\<\\\\b>", "specials escaped inside the wrap");
    }

    /// A line ending in a `url` has no destination spelling at all, so the
    /// writer percent-encodes it: the mark survives, addressing the encoded URL.
    /// Only a storage-lane or hand-built content reaches this — the authored
    /// lanes refuse the url outright.
    #[test]
    fn a_url_carrying_a_line_ending_still_projects_as_a_link() {
        let rt = Content::new("t x".to_string(), vec![Line::new(LineKind::Para)])
            .with_marks(vec![Mark::new(0, 1, MarkKind::Link { url: "a\nb".into() })])
            .into_lines();
        let back = from_markdown(&to_markdown(&rt))
            .expect("re-imports")
            .content;
        assert_eq!(
            to_markdown(&back),
            to_markdown(&rt),
            "one round trip settles it"
        );
        assert_eq!(back.text, "t x", "the display text did not leak");
        assert_eq!(
            back.marks.first().map(|m| &m.kind),
            Some(&MarkKind::Link {
                url: "a%0Ab".into()
            })
        );

        let isl = crate::lines::model::Island::new("i1".into(), IslandType::Image)
            .with_props(serde_json::json!({"alt": "a", "url": "u\rv"}));
        let rt = Content::new(format!("x{ISLAND_SLOT}"), vec![Line::new(LineKind::Para)])
            .with_islands(vec![isl])
            .into_lines();
        let back = from_markdown(&to_markdown(&rt))
            .expect("re-imports")
            .content;
        assert_eq!(back.islands.len(), 1, "the island survived");
        assert_eq!(back.islands[0].props["url"], "u%0Dv");
    }

    /// A strong mark whose delimiters land where CommonMark won't read them as a
    /// run is dropped, and every other mark on the line is kept. Re-add order is
    /// document order, so a good mark before *or* after a leaking one lives.
    #[test]
    fn net_drops_only_the_leaking_marks() {
        for (label, text, spans, want) in [
            ("leaking alone", "a.b", vec![(0, 2)], "a.b"),
            ("representable alone", "ab cd", vec![(0, 2)], "**ab** cd"),
            (
                "leaking then representable",
                "a.b and cd ef",
                vec![(1, 3), (8, 10)],
                "a.b and **cd** ef",
            ),
            (
                "representable then leaking",
                "cd ef and a.b",
                vec![(0, 2), (11, 13)],
                "**cd** ef and a.b",
            ),
        ] {
            let rt = marked(
                text,
                spans
                    .into_iter()
                    .map(|(start, end)| Mark {
                        start,
                        end,
                        kind: MarkKind::Strong,
                    })
                    .collect(),
            );
            let md = to_markdown(&rt);
            assert_eq!(md, want, "{label}");
            assert_eq!(
                from_markdown(&md).unwrap().content.text,
                text,
                "{label}: text drift"
            );
        }
    }

    /// A mark whose delimiters merge with their neighbour's is re-spelled, not
    /// dropped: only a mark markdown cannot carry *at all* reaches the drop
    /// search. The ambiguous run costs the text in the first three shapes — a
    /// `Strong` ending in a literal `*` against a following `Emph`, two marks
    /// over one span where `*` must sit inside `~~` to pair — and only the marks
    /// in the last two, where every character comes back and the `***` between
    /// bold and the emphasis beside it re-segments into one `Strong` over all
    /// three spans.
    #[test]
    fn a_mark_is_respelled_before_it_is_dropped() {
        for (label, src) in [
            ("strong ending in `*`, then emph", "__a**__*b*"),
            ("the same over non-ASCII", "__౸**__*0_*———"),
            ("emph and strike over one span", "౸~~*¡± ±*~~"),
            ("bold, emph, bold", "**a±**_b_**c**"),
            ("the same over a literal `*`", "__*__*౸*__a**0__"),
        ] {
            let once = from_markdown(src).unwrap().content;
            assert!(once.marks.len() >= 2, "{label}: nothing to lose");
            let md = to_markdown(&once);
            let twice = from_markdown(&md).unwrap().content;
            assert_eq!(&twice.text, &once.text, "{label}: text drift. md: {md:?}");
            assert_eq!(&twice.marks, &once.marks, "{label}: mark lost. md: {md:?}");
        }
    }

    /// At the [`PROBE_BUDGET`] guarantee (32 flanking marks, half unrepresentable)
    /// the budget covers the full search, so no representable mark is lost to it.
    #[test]
    fn net_resolves_a_line_at_the_budget_guarantee() {
        let (mut text, mut marks) = (String::new(), Vec::new());
        for i in 0..32 {
            let at = i * 6;
            if i % 2 == 0 {
                text.push_str("abcde "); // `**abcde**`: a run on both edges
                marks.push((at, at + 5));
            } else {
                text.push_str("abc.e "); // `**.e**`: cannot open against `.`
                marks.push((at + 3, at + 5));
            }
        }
        let text = text.trim_end();
        let rt = marked(
            text,
            marks
                .into_iter()
                .map(|(start, end)| Mark {
                    start,
                    end,
                    kind: MarkKind::Strong,
                })
                .collect(),
        );
        let md = to_markdown(&rt);
        assert_eq!(md.matches("**abcde**").count(), 16, "every good mark kept");
        assert_eq!(md.matches("**").count(), 32, "every leaking mark dropped");
        assert_eq!(from_markdown(&md).unwrap().content.text, text);
    }

    /// Past the budget the net still terminates and preserves the text, dropping
    /// what it has not cleared.
    #[test]
    fn net_bounds_a_pathological_line() {
        let text = "a.b, ".repeat(200);
        let text = text.trim_end();
        let rt = marked(
            text,
            (0..200)
                .map(|i| Mark {
                    start: i * 5 + 1,
                    end: i * 5 + 4,
                    kind: MarkKind::Strong,
                })
                .collect(),
        );
        let md = to_markdown(&rt);
        assert!(!md.contains('*'), "every leaking mark dropped: {md:?}");
        assert_eq!(from_markdown(&md).unwrap().content.text, text);
    }

    #[test]
    fn ordered_list_marker_saturates_on_overflow() {
        // `validate` does not ceiling `start`/`ordinal`, so a corrupt content can
        // carry `start == u64::MAX`.
        let json = format!(
            r#"{{"text":"x","lines":[{{"kind":"para","containers":[{{"container":"list_item","ordered":true,"start":{},"ordinal":5}}]}}],"marks":[],"islands":[]}}"#,
            u64::MAX
        );
        let rt = Content::from_canonical_json(&json).unwrap();
        let md = to_markdown(&rt);
        assert!(
            md.contains(&format!("{}. ", u64::MAX)),
            "marker saturates to u64::MAX: {md:?}"
        );
    }
}
