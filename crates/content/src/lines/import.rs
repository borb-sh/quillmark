//! The markdown import lowered to the line form, for the readers and tests
//! still on it.

use crate::carry::Pin;
use crate::lines::model::{Content, ISLAND_SLOT, Line, LineKind, Lines, MarkKind};
#[cfg(test)]
pub(crate) use crate::markdown::import::options;
pub use crate::markdown::import::{DroppedConstruct, ImportError, ImportWarning};

/// A markdown import: the content, and what the markdown spelled that it
/// could not carry, in order of each construct's first occurrence.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "carries the import's warnings; read `.warnings` or bind it"]
pub struct Imported {
    pub content: Lines,
    pub warnings: Vec<ImportWarning>,
}

/// [`crate::markdown::from_markdown`] lowered to the line form, for the
/// readers still on it.
pub fn from_markdown(markdown: &str) -> Result<Imported, ImportError> {
    let imported = crate::markdown::from_markdown(markdown)?;
    Ok(Imported {
        content: crate::migrate::to_lines(&imported.content),
        warnings: imported.warnings,
    })
}

/// [`crate::markdown::from_markdown_unresolved`] lowered to the line form.
pub fn from_markdown_unresolved(markdown: &str) -> Result<Imported, ImportError> {
    let imported = crate::markdown::from_markdown_unresolved(markdown)?;
    Ok(Imported {
        content: crate::migrate::to_lines(&imported.content),
        warnings: imported.warnings,
    })
}

/// A [`from_markdown_against`] import.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "carries the import's warnings and pins; read them or bind it"]
pub struct Against {
    /// The content, each token's island in it and no anchor.
    pub content: Lines,
    /// [`from_markdown`]'s warnings, with each tag and token reported in
    /// place of a token's drop.
    pub warnings: Vec<ImportWarning>,
    /// Each anchor a tag pins, in position order.
    pub pins: Vec<Pin>,
}

/// Import `markdown` as a revise of `stored`, reading the tags of an
/// annotated read (markdown-spec §6.4). A `<qm-anchor ref="ID">` tag in prose
/// pins the anchor `ID` the stored value holds in prose to start where the
/// tag sits; one with an `at` attribute, which the read writes where its tag
/// moved to its line's end, pins nothing. A `<qm-island ref="ID">` token in
/// prose is the stored island `ID`, its type and props as stored but for its
/// cells' anchors, which the revise carries onto the island that continues it
/// ([`crate::carry::carry_onto`]), so a token written twice copies none. A tag or a
/// token naming nothing the stored value holds there, and a second tag naming
/// one anchor, drop reported under `qm-anchor[ref]` or `qm-island[ref]`; a
/// token in a table cell, a note, an image's alt text or a code span drops
/// reported under `qm-island`. An anchor tag outside prose pins nothing.
pub fn from_markdown_against(markdown: &str, stored: &Content) -> Result<Against, ImportError> {
    let anchors = stored
        .marks
        .iter()
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let islands = stored
        .islands
        .iter()
        .map(|i| (i.id.clone(), i.clone()))
        .collect();
    let (mut content, warnings) =
        crate::markdown::import::walk_against(markdown, anchors, islands)?;
    content.split_block_islands();
    let mut content = content.into_lines().into_content();
    let mut pins = Vec::new();
    content.marks.retain(|m| match &m.kind {
        MarkKind::Anchor { id } => {
            pins.push(Pin {
                at: m.start,
                id: id.clone(),
            });
            false
        }
        _ => true,
    });
    Ok(Against {
        content: content.into_lines(),
        warnings,
        pins,
    })
}

/// Import plain text (literal) into a [`Content`]: the literal-codec sibling of
/// [`from_markdown`]. Every character is content, never syntax: `*hi*` is four
/// literal chars, not emphasis. With [`crate::lines::export::to_plaintext`] it pins the
/// fixed point `to_plaintext(from_plaintext(s)) == s` for any `s` free of `\r`,
/// bidi controls, line separators (VT, FF, NEL, U+2028, U+2029), and
/// [`ISLAND_SLOT`].
///
/// Line structure is **derived, not stored**: a lone `\n` between two non-empty
/// segments is a within-paragraph break ([`Line::continues`] `true`); a blank
/// line is a paragraph boundary. The text is stored verbatim, so the round trip
/// is byte-exact however structure is later re-derived.
pub fn from_plaintext(s: &str) -> Lines {
    // Boundary cleanup so the content invariants hold; clean plaintext passes
    // through untouched.
    let text: String = s
        .chars()
        .filter(|&c| c != ISLAND_SLOT)
        .filter_map(crate::model::admit_char)
        .collect();
    // One line per `\n`-separated segment. A single streaming pass carries the
    // prior segment's non-emptiness, so line 0 is `false` and no intermediate
    // segment vector is allocated.
    let mut prev_nonempty = false;
    let lines = text
        .split('\n')
        .map(|seg| {
            let continues = prev_nonempty && !seg.is_empty();
            prev_nonempty = !seg.is_empty();
            Line::new(LineKind::Para).with_continues(continues)
        })
        .collect();
    Content {
        text,
        lines,
        marks: Vec::new(),
        islands: Vec::new(),
    }
    .into_lines()
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) mod generate;
    mod properties;

    use super::*;
    use crate::MAX_NESTING_DEPTH;
    use crate::lines::model::{Container, Island, Mark};
    use crate::model::IslandType;
    use serde_json::json;

    fn imp(md: &str) -> Lines {
        let rt = from_markdown(md).unwrap().content;
        assert_eq!(rt.validate(), Ok(()), "invariants for {md:?}");
        rt
    }

    fn imp_plain(s: &str) -> Lines {
        let rt = from_plaintext(s);
        assert_eq!(rt.validate(), Ok(()), "invariants for {s:?}");
        rt
    }

    #[test]
    fn plaintext_is_literal_and_plain() {
        let rt = imp_plain("a *star* and _under_ #hash");
        assert_eq!(rt.text, "a *star* and _under_ #hash");
        assert!(rt.marks.is_empty());
        assert!(rt.islands.is_empty());
        assert!(rt.is_plain());
        assert!(rt.is_inline(), "one line with no formatting is also inline");
    }

    #[test]
    fn plaintext_round_trip_is_verbatim_and_idempotent() {
        for s in ["", "one line", "a\nb", "a\n\nb", "trailing\n", "*not bold*"] {
            let rt = imp_plain(s);
            assert_eq!(
                crate::lines::export::to_plaintext(&rt),
                s,
                "verbatim for {s:?}"
            );
            let rt2 = from_plaintext(&crate::lines::export::to_plaintext(&rt));
            assert_eq!(rt2.text, rt.text, "idempotent for {s:?}");
            assert_eq!(rt2.lines, rt.lines, "idempotent structure for {s:?}");
        }
    }

    /// The literal codec's half of the slot contract; the markdown half is
    /// `prose_drops_a_stray_slot_before_flanking_is_read` below. A stray
    /// `ISLAND_SLOT` is an invariant violation the mint does not repair —
    /// `validate` reports it — so a codec that admitted one would hand out a
    /// `Lines` that is not, and `imp_plain` would say so. The drop is also
    /// why this codec's fixed point names the character it excludes. A CRLF
    /// collapses to LF beside it.
    #[test]
    fn plaintext_drops_a_stray_slot() {
        let rt = imp_plain("a\u{FFFC}b");
        assert_eq!(rt.text, "ab");
        assert!(rt.islands.is_empty());
        assert_eq!(crate::lines::export::to_plaintext(&rt), "ab");
        assert_eq!(imp_plain("a\r\nb").text, "a\nb", "CRLF collapses to LF");
    }

    #[test]
    fn plaintext_derives_continues_from_line_structure() {
        let rt = imp_plain("a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert!(!rt.lines[0].continues);
        assert!(
            rt.lines[1].continues,
            "lone \\n is a within-paragraph break"
        );

        let rt = imp_plain("a\n\nb");
        assert_eq!(rt.lines.len(), 3);
        assert!(!rt.lines[0].continues);
        assert!(
            !rt.lines[1].continues,
            "the blank line is a paragraph boundary"
        );
        assert!(
            !rt.lines[2].continues,
            "text after a blank line starts a new block"
        );
    }

    #[test]
    fn line_separators_are_spaced_at_every_text_ingress() {
        for sep in ['\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}'] {
            let src = format!("intro{sep}- item");
            assert_eq!(imp_plain(&src).text, "intro - item", "plaintext {sep:?}");
            assert_eq!(imp(&src).text, "intro - item", "markdown {sep:?}");

            let held = Content::new(src, vec![Line::new(LineKind::Para)]);
            assert_eq!(
                held.validate(),
                Err(crate::lines::model::Invariant::LineSeparator(sep))
            );
        }
    }

    #[test]
    fn basic_blocks_import_as_text_line_kinds_and_containers() {
        let quote = || vec![Container::Quote { instance: 0 }];
        let para = |containers| (LineKind::Para, containers);
        let cases: Vec<(&str, &str, Vec<(LineKind, Vec<Container>)>)> = vec![
            ("Hello world", "Hello world", vec![para(vec![])]),
            ("one\n\ntwo", "one\ntwo", vec![para(vec![]), para(vec![])]),
            (
                "## Title",
                "Title",
                vec![(LineKind::Heading { level: 2 }, vec![])],
            ),
            ("> quoted", "quoted", vec![para(quote())]),
            ("> ", "", vec![para(quote())]),
            ("", "", vec![para(vec![])]),
        ];
        for (md, text, lines) in cases {
            let rt = imp(md);
            assert_eq!(rt.text, text, "{md:?}");
            assert!(rt.marks.is_empty(), "{md:?}");
            let got: Vec<_> = rt
                .lines
                .iter()
                .map(|l| (l.kind.clone(), l.containers.clone()))
                .collect();
            assert_eq!(got, lines, "{md:?}");
        }
        assert_eq!(
            imp("- a\n-\n- b").lines.len(),
            3,
            "empty middle list item preserved"
        );
    }

    #[test]
    fn an_inline_span_imports_as_its_mark() {
        for (md, text, mark) in [
            ("a **b** _c_", "a b c", Mark::new(2, 3, MarkKind::Strong)),
            ("a **b** _c_", "a b c", Mark::new(4, 5, MarkKind::Emph)),
            (
                "x <u>y</u> z",
                "x y z",
                Mark::new(2, 3, MarkKind::Underline),
            ),
            (
                "run `cargo test` now",
                "run cargo test now",
                Mark::new(4, 14, MarkKind::Code),
            ),
        ] {
            let rt = imp(md);
            assert_eq!(rt.text, text, "{md}");
            assert!(rt.marks.contains(&mark), "{md}: {:?}", rt.marks);
        }
    }

    /// A `</u>` closes the innermost open `<u>` wherever it sits among the
    /// marks, one carrying an attribute included, so it never closes a `**`
    /// and an underline crosses one freely.
    #[test]
    fn a_u_pair_crosses_other_marks() {
        let cases: &[(&str, &[Mark], &[(&str, usize)])] = &[
            (
                "<u>a **b</u> c**",
                &[
                    Mark::new(0, 3, MarkKind::Underline),
                    Mark::new(2, 5, MarkKind::Strong),
                ],
                &[],
            ),
            (
                "**a <u>b** c</u>",
                &[
                    Mark::new(0, 3, MarkKind::Strong),
                    Mark::new(2, 5, MarkKind::Underline),
                ],
                &[],
            ),
            (
                "**bold <u class=\"x\">a</u> b**",
                &[Mark::new(0, 8, MarkKind::Strong)],
                &[("u", 1)],
            ),
            (
                "<u>a <u class=\"x\">b</u> c</u>",
                &[Mark::new(0, 5, MarkKind::Underline)],
                &[("u", 1)],
            ),
            (
                "<u>a <u>b</u> c</u>",
                &[Mark::new(0, 5, MarkKind::Underline)],
                &[],
            ),
            ("**a </u> b**", &[Mark::new(0, 4, MarkKind::Strong)], &[]),
        ];
        for (md, marks, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.marks, *marks, "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }

    /// A `<u>` still open where its paragraph, heading or item text ends drops
    /// there, reported, and underlines nothing; however many are open, the
    /// import does not fail.
    #[test]
    fn an_unclosed_u_drops_where_its_run_ends() {
        let imported = imp_fixed("<u>a\n\nb **c**");
        assert_eq!(imported.content.text, "a\nb c");
        assert_eq!(imported.content.marks, [Mark::new(4, 5, MarkKind::Strong)]);
        assert_eq!(dropped(&imported), [("u", 1)]);

        let imported = imp_fixed(&format!("{}x</u>", "<u>".repeat(MAX_NESTING_DEPTH + 1)));
        assert_eq!(
            imported.content.marks,
            [Mark::new(0, 1, MarkKind::Underline)]
        );
        assert_eq!(dropped(&imported), [("u", MAX_NESTING_DEPTH)]);

        let imported = imp_fixed("| <u>a **b</u> c** | <u>d |\n|---|---|\n| x | y |");
        let cells = crate::lines::serial::table_cells(&imported.content.islands[0].props);
        assert_eq!(
            cells[0].1,
            [
                Mark::new(0, 3, MarkKind::Underline),
                Mark::new(2, 5, MarkKind::Strong)
            ]
        );
        assert!(cells[1].1.is_empty());
        assert_eq!(dropped(&imported), [("u", 1)]);
    }

    /// A `qm-span` pair sets its run's family, an inner pair overrides the
    /// outer one over its own text, and an attribute it cannot read drops
    /// alone while the pair keeps the family.
    #[test]
    fn a_qm_span_pair_sets_its_runs_family() {
        let family = |name: &str| MarkKind::FontFamily { name: name.into() };
        let cases: Vec<(&str, &str, Vec<Mark>, Vec<(&str, usize)>)> = vec![
            (
                "<qm-span family=\"A\">a <qm-span family=\"B\">b</qm-span> c</qm-span>",
                "a b c",
                vec![
                    Mark::new(0, 2, family("A")),
                    Mark::new(2, 3, family("B")),
                    Mark::new(3, 5, family("A")),
                ],
                vec![],
            ),
            (
                "**a <qm-span family=\"A\">b** c</qm-span>",
                "a b c",
                vec![
                    Mark::new(0, 3, MarkKind::Strong),
                    Mark::new(2, 5, family("A")),
                ],
                vec![],
            ),
            (
                "<qm-span color=\"red\" family=\"A\">a</qm-span>",
                "a",
                vec![Mark::new(0, 1, family("A"))],
                vec![("qm-span[color]", 1)],
            ),
            (
                "<qm-span>a</qm-span> <qm-span color=\"red\">b</qm-span>",
                "a b",
                vec![],
                vec![("qm-span", 2), ("qm-span[color]", 1)],
            ),
            (
                "<qm-span family=\" A\">a</qm-span>",
                "a",
                vec![],
                vec![("qm-span[family]", 1), ("qm-span", 1)],
            ),
            (
                "<qm-span family=\"A\"/>a",
                "a",
                vec![],
                vec![("qm-span", 1)],
            ),
        ];
        for (md, text, marks, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, text, "{md:?}");
            assert_eq!(imported.content.marks, marks, "{md:?}");
            assert_eq!(dropped(&imported), warned, "{md:?}");
        }

        for (md, marks) in [
            (
                "<qm-span family=\"Arimo\">a<qm-span family=\"Carlito\">b<qm-span family=\"Arimo\">c</qm-span>d</qm-span>e</qm-span>",
                vec![
                    Mark::new(0, 1, family("Arimo")),
                    Mark::new(1, 2, family("Carlito")),
                    Mark::new(2, 3, family("Arimo")),
                    Mark::new(3, 4, family("Carlito")),
                    Mark::new(4, 5, family("Arimo")),
                ],
            ),
            (
                "<qm-span family=\"Zilla\"><qm-span family=\"Arimo\">xyz</qm-span></qm-span>",
                vec![Mark::new(0, 3, family("Arimo"))],
            ),
        ] {
            assert_eq!(imp_fixed(md).content.marks, marks, "{md:?}");
        }

        let imported = imp_fixed(
            "| <qm-span family=\"A\">a **b</qm-span> c** |\n|---|\n\na[^1]\n\n[^1]: <qm-span family=\"B\">n</qm-span>",
        );
        let cells = crate::lines::serial::table_cells(&imported.content.islands[0].props);
        assert_eq!(
            cells[0].1,
            [
                Mark::new(0, 3, family("A")),
                Mark::new(2, 5, MarkKind::Strong)
            ]
        );
        let (_, note) = crate::lines::serial::parse_cell(&imported.content.islands[1].props);
        assert_eq!(note, [Mark::new(0, 1, family("B"))]);
    }

    /// A tag line inside an open `qm-span` pair is its text, as one inside an
    /// open `<u>` is.
    #[test]
    fn a_tag_line_inside_an_open_qm_span_pair_is_text() {
        let imported = imp_fixed("<qm-span family=\"A\">a\n<qm-keep>\nb</qm-span>");
        assert_eq!(imported.content.lines.len(), 1);
        assert!(
            imported
                .content
                .marks
                .iter()
                .all(|m| m.kind == MarkKind::FontFamily { name: "A".into() })
        );
    }

    /// `qm-span` tag lines around blocks wrap nothing: the tags drop and warn,
    /// and the blocks stay.
    #[test]
    fn a_qm_span_tag_line_drops() {
        let imported = imp_fixed("<qm-span family=\"A\">\n\nOne.\n\nTwo.\n\n</qm-span>");
        assert_eq!(imported.content.text, "One.\nTwo.");
        assert!(
            imported
                .content
                .lines
                .iter()
                .all(|l| l.containers.is_empty())
        );
        assert!(imported.content.marks.is_empty());
        assert_eq!(dropped(&imported), [("qm-span", 1)]);
    }

    #[test]
    fn other_html_stripped() {
        let rt = imp("a <span>b</span> c");
        assert_eq!(rt.text, "a b c");
    }

    /// `***a**` is a literal `*` followed by strong `a` (CommonMark's rule of
    /// three), and every shape here keeps its stars: a fixer re-segmenting the
    /// run would delete one.
    #[test]
    fn odd_asterisk_runs_keep_their_literal_star() {
        for (src, text) in [
            ("***a**", "*a"),
            ("***aa**", "*aa"),
            ("****a**", "**a"),
            ("a***a**", "a*a"),
        ] {
            assert_eq!(imp(src).text, text, "literal star dropped from {src:?}");
        }
        let rt = imp("***bold italic***");
        assert_eq!(rt.text, "bold italic");
        assert!(rt.marks.iter().any(|m| m.kind == MarkKind::Strong));
        assert!(rt.marks.iter().any(|m| m.kind == MarkKind::Emph));
    }

    /// `<ul>` is not `<u>`, so it is stripped like any other HTML: no
    /// underline, no strong.
    #[test]
    fn ul_lookalike_is_not_underline() {
        let rt = imp("x <ul>y</ul> z");
        assert_eq!(rt.text, "x y z");
        assert!(
            rt.marks
                .iter()
                .all(|m| m.kind != MarkKind::Underline && m.kind != MarkKind::Strong)
        );
    }

    #[test]
    fn code_block_lines() {
        let rt = imp("```rust\nfn a() {}\nfn b() {}\n```");
        assert_eq!(rt.text, "fn a() {}\nfn b() {}");
        assert_eq!(rt.lines.len(), 2);
        assert!(rt.lines.iter().all(|l| l.kind
            == LineKind::Code {
                lang: Some("rust".into())
            }));
    }

    #[test]
    fn code_block_drops_a_stray_slot_and_breaks_on_a_bare_cr() {
        let rt = imp("```\na\u{FFFC}b\nc\rd\n```");
        assert_eq!(rt.text, "ab\nc\nd");
        assert_eq!(rt.lines.len(), 3);
        assert!(
            rt.lines
                .iter()
                .all(|l| l.kind == LineKind::Code { lang: None })
        );
    }

    /// The prose sibling of the code-block case above, on the other door.
    /// Dropping the slot moves what abuts a delimiter, so the emphasis the
    /// *second* pass reads is not the emphasis the first one wrote: `*a\u{FFFC}*`
    /// is a literal pair around the slot (`*` before U+FFFC is not left-flanking),
    /// and once the slot is gone `*a*` is emphasis. The drop is the contract —
    /// `ISLAND_SLOT` with no backing island is an invariant violation, so it
    /// cannot be admitted — and one markdown pass is where it must happen.
    #[test]
    fn prose_drops_a_stray_slot_before_flanking_is_read() {
        let rt = imp("x\u{FFFC}y");
        assert_eq!(rt.text, "xy");
        assert!(rt.islands.is_empty());

        // The flanking move itself: literal in, emphasized out, settled after.
        let once = imp("*a\u{FFFC}*");
        assert_eq!(once.text, "a");
        assert_eq!(once.marks, vec![Mark::new(0, 1, MarkKind::Emph)]);
        let twice = imp(&crate::lines::export::to_markdown(&once));
        assert_eq!(twice, once);
    }

    #[test]
    fn bullet_list_containers() {
        let rt = imp("- a\n- b");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 0,
                checked: None,
                instance: 0,
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 1,
                checked: None,
                instance: 0,
            }]
        );
    }

    #[test]
    fn ordered_list_custom_start() {
        let rt = imp("3. a\n4. b");
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 0,
                checked: None,
                instance: 0,
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
                checked: None,
                instance: 0,
            }]
        );
    }

    #[test]
    fn multi_paragraph_list_item_shares_container() {
        let rt = imp("- first\n\n  second");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.lines[0].containers, rt.lines[1].containers);
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 0,
                checked: None,
                instance: 0,
            }]
        );
    }

    #[test]
    fn thematic_break_is_rule_line() {
        for src in ["---", "***", "___"] {
            let md = format!("one\n\n{src}\n\ntwo");
            let rt = imp(&md);
            assert_eq!(rt.lines.len(), 3, "source: {src}");
            assert_eq!(rt.lines[0].kind, LineKind::Para);
            assert_eq!(rt.lines[1].kind, LineKind::Rule, "source: {src}");
            assert_eq!(rt.lines[2].kind, LineKind::Para);
            assert_eq!(rt.text, "one\n\ntwo");
        }
    }

    #[test]
    fn table_is_block_island() {
        let rt = imp("| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(rt.text, "\u{FFFC}");
        // The block is the slot's markup; the line holding it is prose.
        assert_eq!(rt.lines[0].kind, LineKind::Para);
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, IslandType::Table);
    }

    /// A cell reuses the prose mark machinery through a second `Tag::Strong`
    /// site, so the two must agree on `<u>` vs `**`.
    #[test]
    fn underline_from_u_tag_in_table_cell() {
        let rt = imp("| h |\n|---|\n| <u>a</u> **b** |");
        let cells = crate::lines::serial::table_cells(&rt.islands[0].props);
        let (text, marks) = cells.iter().find(|(t, _)| t == "a b").expect("cell");
        assert_eq!(text, "a b");
        let kinds: Vec<&MarkKind> = marks.iter().map(|m| &m.kind).collect();
        assert_eq!(kinds, [&MarkKind::Underline, &MarkKind::Strong]);
    }

    #[test]
    fn island_ids_are_deterministic_and_positional() {
        let md = "![a](x)\n\n| h |\n|---|\n| c |";
        let a = imp(md);
        let b = imp(md);
        assert_eq!(a.to_canonical_json(), b.to_canonical_json());
        let ids: Vec<&str> = a.islands.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["isl-0", "isl-1"]);
    }

    #[test]
    fn image_is_inline_island() {
        let rt = imp("see ![a cat](cat.png) here");
        assert_eq!(rt.text, "see \u{FFFC} here");
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, IslandType::Image);
        assert_eq!(rt.islands[0].props["url"], "cat.png");
        assert_eq!(rt.islands[0].props["alt"], "a cat");
    }

    /// A task marker is its item's own: a list mixes tasks and plain items,
    /// `[X]` reads as done, and every line of an item carries its marker.
    #[test]
    fn a_task_marker_is_its_items_checked() {
        let checked = |rt: &Lines| -> Vec<Vec<Option<bool>>> {
            rt.lines
                .iter()
                .map(|l| {
                    l.containers
                        .iter()
                        .map(|c| match c {
                            Container::ListItem { checked, .. } => *checked,
                            _ => panic!("a list item: {c:?}"),
                        })
                        .collect()
                })
                .collect()
        };
        let rt = imp_fixed("- [ ] a\n- [X] b\n- c\n\n  more\n  - [x] d").content;
        assert_eq!(rt.text, "a\nb\nc\nmore\nd");
        assert_eq!(
            checked(&rt),
            [
                vec![Some(false)],
                vec![Some(true)],
                vec![None],
                vec![None],
                vec![None, Some(true)],
            ]
        );
        assert!(
            rt.lines.iter().all(|l| l.containers[0].instance() == 0),
            "one list"
        );

        let rt = imp_fixed("- [x]\n  # h\n\n  p").content;
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 1 });
        assert_eq!(checked(&rt), [vec![Some(true)], vec![Some(true)]]);
    }

    /// Each footnote reference is an inline island holding a copy of the note
    /// its label names, however the label is cased and wherever the
    /// definition sits; a note keeps the text of the blocks it holds, a line
    /// each.
    #[test]
    fn a_footnote_reference_is_an_island_holding_its_note() {
        let md = "[^n]: **bold** note\n\nOne[^1] two[^N].\n\n# Head[^1]\n\n\
                  [^1]: first<br>second\n\n    para\n    - item";
        let imported = imp_fixed(md);
        let rt = &imported.content;
        assert_eq!(rt.text, "One\u{FFFC} two\u{FFFC}.\nHead\u{FFFC}");
        let notes: Vec<(String, Vec<Mark>)> = rt
            .islands
            .iter()
            .map(|isl| {
                assert_eq!(isl.island_type, IslandType::Footnote);
                crate::lines::serial::parse_cell(&isl.props)
            })
            .collect();
        let first = ("first\nsecond\npara\nitem".to_string(), vec![]);
        assert_eq!(
            notes,
            [
                first.clone(),
                ("bold note".into(), vec![Mark::new(0, 4, MarkKind::Strong)]),
                first,
            ]
        );
        assert_eq!(dropped(&imported), [("footnote[list]", 1)]);
    }

    /// A definition nothing reads and one repeating a label drop, and so does
    /// a reference where no note can sit; a reference to no definition is
    /// text.
    #[test]
    fn a_footnote_with_no_place_drops() {
        let cases: &[(&str, &str, &[(&str, usize)])] = &[
            (
                "a[^1]\n\n[^1]: x\n\n[^2]: y",
                "a\u{FFFC}",
                &[("footnote", 1)],
            ),
            (
                "a[^1]\n\n[^1]: x\n\n[^1]: y",
                "a\u{FFFC}",
                &[("footnote", 1)],
            ),
            (
                "| h[^1] |\n| --- |\n| c |\n\n[^1]: x",
                "\u{FFFC}",
                &[("footnote", 2)],
            ),
            (
                "a[^1]\n\n[^1]: x[^2]\n\n[^2]: y",
                "a\u{FFFC}",
                &[("footnote", 2)],
            ),
            ("a[^9]", "a[^9]", &[]),
        ];
        for (md, text, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }

    /// A note's first reference copies it free; later copies spend one
    /// budget the document's notes share, and a reference past it stays as
    /// its text, reported. A markdown past the size bound refuses whole.
    #[test]
    fn note_copies_past_the_budget_stay_as_text() {
        let note = "w".repeat(crate::MAX_NOTE_COPIES / 2);
        let imported = imp_fixed(&format!(
            "a[^1] b[^1] c[^1] d[^1] e[^2]\n\n[^1]: {note}\n\n[^2]: {note}"
        ));
        let rt = &imported.content;
        assert_eq!(rt.text, "a\u{FFFC} b\u{FFFC} c\u{FFFC} d[^1] e\u{FFFC}");
        assert_eq!(dropped(&imported), [("footnote:size", 1)]);

        let big = "a".repeat(crate::MAX_MARKDOWN_SIZE + 1);
        assert!(matches!(
            from_markdown(&big),
            Err(ImportError::TooLarge { .. })
        ));
    }

    /// `$…$` is an inline equation and `$$…$$` a display one, each an island
    /// holding its TeX; where no island can sit, the source stays as text.
    #[test]
    fn a_reference_to_a_definition_nested_in_another_reads_as_its_text() {
        let imported = imp_fixed("a[^2] b[^1] c\n\n[^1]: x\n\n    > [^2]: y");
        let rt = &imported.content;
        assert_eq!(rt.text, "a[^2] b\u{FFFC} c");
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].id, "isl-0");
        assert_eq!(rt.islands[0].props["text"], "x\ny");
    }

    /// A definition list's spelling is a paragraph, as CommonMark reads it.
    #[test]
    fn a_definition_list_reads_as_a_paragraph() {
        let rt = imp_fixed("T\n: a\n\nU").content;
        assert_eq!(rt.text, "T : a\nU");
        assert!(
            rt.lines
                .iter()
                .all(|l| l.kind == LineKind::Para && l.containers.is_empty())
        );
    }

    #[test]
    fn math_is_an_island_holding_its_tex() {
        let imported = imp_fixed("Let $x^2$ and $$\\frac{a}{b}$$ cost $5 and $10.");
        let rt = &imported.content;
        assert_eq!(rt.text, "Let \u{FFFC} and \u{FFFC} cost $5 and $10.");
        let props: Vec<&serde_json::Value> = rt.islands.iter().map(|isl| &isl.props).collect();
        assert_eq!(
            props,
            [
                &serde_json::json!({"tex": "x^2"}),
                &serde_json::json!({"tex": "\\frac{a}{b}", "display": true}),
            ]
        );
        assert!(dropped(&imported).is_empty());

        let cases: &[(&str, &str)] = &[
            ("| $a$ |\n| --- |\n| c |", "\u{FFFC}"),
            ("![$a$ b](u)", "\u{FFFC}"),
            ("n[^1]\n\n[^1]: $a$", "n\u{FFFC}"),
        ];
        for (md, text) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(dropped(&imported), [("math", 1)], "{md:?}");
        }
        let rt = imp("| $a$ |\n| --- |\n| c |");
        assert_eq!(rt.islands[0].props["header"][0]["text"], "$a$");
        let rt = imp("![$a$ b](u)");
        assert_eq!(rt.islands[0].props["alt"], "$a$ b");
        let imported = imp_fixed("a $\u{2000}$ b");
        assert_eq!(
            imported.content.text, "a $\u{2000}$ b",
            "a blank equation stays as its text"
        );
        assert_eq!(dropped(&imported), [("math", 1)]);
        let rt = imp("n[^1]\n\n[^1]: $a$");
        assert_eq!(rt.islands[0].props["text"], "$a$");
    }

    /// A paragraph whose inline content is entirely stripped HTML leaves no
    /// line, markdown having no syntax to write one back; an empty heading or
    /// container keeps the line `# `, `- ` and `>` can write. Either way the
    /// import is a fixed point.
    #[test]
    fn empty_paragraph_drops_while_empty_heading_and_container_keep_their_line() {
        let cases: &[(&str, usize)] = &[
            ("a\n\n<span></span>\n\nb", 2),
            ("a\n\n<span></span>", 1),
            ("- a\n\n  <span></span>", 1),
            ("> a\n>\n> <span></span>", 1),
            ("# <span></span>", 1),
            ("- <span></span>", 1),
            ("> <span></span>", 1),
        ];
        for (md, lines) in cases {
            let rt = imp(md);
            assert_eq!(rt.lines.len(), *lines, "{md:?} -> {:?}", rt.lines);
            let rt2 = from_markdown(&crate::lines::export::to_markdown(&rt))
                .unwrap()
                .content;
            assert_eq!(rt2, rt, "{md:?} is not a fixed point");
        }
        assert_eq!(imp("a\n\n<span></span>\n\nb").text, "a\nb");
    }

    #[test]
    fn mark_does_not_swallow_leading_newline() {
        let rt = imp("a\n\n**b**");
        assert_eq!(rt.text, "a\nb");
        let m = &rt.marks[0];
        assert_eq!((m.start, m.end), (2, 3));
        assert_eq!(rt.text.chars().nth(m.start), Some('b'));
    }

    #[test]
    fn import_and_editor_content_same_canonical_bytes() {
        // Equal content → equal bytes, whatever the producer.
        let imported = imp("a\n\n**b**");
        let editor = Content {
            text: "a\nb".into(),
            lines: vec![
                Line {
                    kind: LineKind::Para,
                    containers: vec![],
                    continues: false,
                },
                Line {
                    kind: LineKind::Para,
                    containers: vec![],
                    continues: false,
                },
            ],
            marks: vec![Mark {
                start: 2,
                end: 3,
                kind: MarkKind::Strong,
            }],
            islands: vec![],
        };
        assert_eq!(
            imported.to_canonical_json(),
            editor.into_lines().to_canonical_json()
        );
    }

    #[test]
    fn hard_break_is_a_continuation_line() {
        let rt = imp("line one\\\nline two");
        assert_eq!(rt.text, "line one\nline two");
        assert_eq!(rt.lines.len(), 2);
        assert!(!rt.lines[0].continues);
        assert!(rt.lines[1].continues, "hard break -> continuation line");
    }

    #[test]
    fn inline_br_is_a_hard_break_where_text_precedes_it() {
        let rt = imp("line1<br>line2");
        assert_eq!(rt.text, "line1\nline2");
        assert!(rt.lines[1].continues);

        assert_eq!(imp("# a<br>b").text, "a b");

        let rt = imp("a\n\n<br>b");
        assert_eq!(rt.text, "a\nb");
        assert!(!rt.lines[1].continues, "two paragraphs stay two");

        let rt = imp("# <br>a");
        assert_eq!(rt.text, "a");
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 1 });

        let rt = imp("**<br>a**");
        assert_eq!(rt.text, "a");
        assert_eq!(rt.marks, vec![Mark::new(0, 1, MarkKind::Strong)]);
    }

    #[test]
    fn a_soft_break_beside_a_hard_break_is_nothing() {
        for md in [
            "one<br>\ntwo",
            "one\n<br>two",
            "one<br>\n<br>two",
            "one\n\\\ntwo",
        ] {
            let rt = imp(md);
            assert_eq!(rt.text, "one\ntwo", "{md:?}");
            assert!(rt.lines[1].continues, "{md:?}");
        }
        let rt = imp("a\n**<br>b**");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.marks, vec![Mark::new(2, 3, MarkKind::Strong)]);
        assert_eq!(imp("a<br>\nb\n===").text, "a b");
        assert_eq!(imp("a <br>\nb").text, "a \nb", "an authored space stays");
    }

    /// A mark or `<u>` opens its line ahead of its text, so a line all it would
    /// hold dropped from is an empty paragraph or continuation line.
    #[test]
    fn a_line_emptied_by_what_dropped_drops() {
        for (md, text, depths) in [
            ("a\n\n[](u)", "a", vec![0]),
            ("a\n<qm-keep>\n<u>", "a", vec![0]),
            ("a\\\n[](u)", "a", vec![0]),
            ("a<br><u></u>", "a", vec![0]),
            ("[](u)\nb", "b", vec![0]),
            ("- [x] [](u)\n\n  b", "b", vec![1]),
            ("- [](u)\n  - b", "b", vec![2]),
            ("[](u)\n<qm-keep>\n\nb\n\n</qm-keep>", "b", vec![0]),
        ] {
            let rt = imp_fixed(md).content;
            assert_eq!(rt.text, text, "{md:?}");
            assert_eq!(
                rt.lines
                    .iter()
                    .map(|l| l.containers.len())
                    .collect::<Vec<_>>(),
                depths,
                "{md:?}"
            );
        }
        let rt = imp_fixed("- [x] [](u)").content;
        assert!(
            matches!(rt.lines[..], [Line { ref containers, .. }] if matches!(containers[..], [Container::ListItem { checked: Some(true), .. }])),
            "an item keeps its line: {:?}",
            rt.lines
        );
        assert_eq!(
            imp_fixed("# [](u)").content.lines[0].kind,
            LineKind::Heading { level: 1 }
        );
    }

    #[test]
    fn a_reference_imports_as_text_admits_it_in_a_destination_and_an_alt() {
        let rt = imp_fixed("[x](u&#x202E;&#10;v) ![a&#13;&#10;b](u&#x2028;v) n&#0;\0").content;
        assert_eq!(rt.marks[0].kind, MarkKind::Link { url: "uv".into() });
        assert_eq!(rt.islands[0].props, json!({"alt": "a b", "url": "u v"}));
        assert!(rt.text.ends_with("n\u{FFFD}\u{FFFD}"), "{:?}", rt.text);
    }

    #[test]
    fn atx_heading_cannot_carry_a_hard_break() {
        // A setext heading can, and reaches the heading→space arm.
        let rt = imp("## a  \nb");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 2 });
        assert_eq!(rt.lines[1].kind, LineKind::Para);
        assert!(!rt.lines[1].continues, "separate block, not a continuation");
    }

    /// A tight item's text arrives with no `Paragraph` start, so text after a
    /// block nested in the item opens a line of its own rather than joining
    /// that block's last line.
    #[test]
    fn text_after_a_block_nested_in_a_tight_item_opens_its_own_line() {
        for (md, text) in [
            ("- item\n  ```\n  code\n  ```\n  after", "item\ncode\nafter"),
            ("- item\n  > # head\n  after", "item\nhead\nafter"),
            ("- item\n  <!-- c -->\n  after", "item\nafter"),
            ("- item\n  ***\n  after", "item\n\nafter"),
        ] {
            let rt = imp(md);
            assert_eq!(rt.text, text, "{md:?}");
            let last = rt.lines.last().unwrap();
            assert_eq!(
                (&last.kind, last.containers.len()),
                (&LineKind::Para, 1),
                "{md:?}"
            );
            assert_eq!(imp(&crate::lines::export::to_markdown(&rt)), rt, "{md:?}");
        }
    }

    #[test]
    fn astral_positions_are_usv() {
        let rt = imp("a😀**b**");
        // 'a'(0) '😀'(1) 'b'(2): strong over "b" is 2..3 in scalar.
        assert_eq!(rt.text, "a😀b");
        assert!(
            rt.marks
                .iter()
                .any(|m| m.start == 2 && m.end == 3 && m.kind == MarkKind::Strong)
        );
    }

    /// `from_markdown` and the content fixed point over its result.
    fn imp_fixed(md: &str) -> Imported {
        let imported = from_markdown(md).unwrap();
        assert_eq!(imported.content.validate(), Ok(()), "invariants for {md:?}");
        let back = from_markdown(&crate::lines::export::to_markdown(&imported.content)).unwrap();
        assert_eq!(
            back.content, imported.content,
            "{md:?} is not a fixed point"
        );
        imported
    }

    /// Each warning's construct as `parse::dropped_construct` names it, leaked
    /// so a case compares it against a literal.
    fn dropped(imported: &Imported) -> Vec<(&'static str, usize)> {
        imported
            .warnings
            .iter()
            .map(|ImportWarning { construct, count }| (&*construct.to_string().leak(), *count))
            .collect()
    }

    fn table_rows(rt: &Lines) -> Vec<Vec<String>> {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        assert_eq!(island.island_type, IslandType::Table);
        island.props["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|cell| crate::lines::serial::parse_cell(cell).0)
                    .collect()
            })
            .collect()
    }

    /// An HTML block drops whole, as CommonMark reads it: a tag line other
    /// than a line of carrier tags alone, tight against markdown, takes the
    /// markdown with it, and blank lines set the markdown apart.
    #[test]
    fn a_tag_line_tight_against_markdown_drops_its_block() {
        for (md, tag) in [
            (
                "<div align=\"center\">\n| a | b |\n|---|---|\n| 1 | 2 |\n</div>",
                "div",
            ),
            (
                "<x-keep>\n| a | b |\n|---|---|\n| 1 | 2 |\n</x-keep>",
                "x-keep",
            ),
            ("<center>\n**Signed**\nJ. Doe\n</center>", "center"),
        ] {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, "", "{md:?}");
            assert_eq!(dropped(&imported), [(tag, 1)], "{md:?}");
        }

        let beside = imp_fixed("<span>\n<qm-keep>\nA\n\n</qm-keep>");
        assert_eq!(beside.content.text, "");
        assert_eq!(dropped(&beside), [("span", 1), ("qm-keep", 1)]);

        let imported =
            imp_fixed("<div align=\"center\">\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n</div>");
        assert_eq!(imported.content.text, "\u{FFFC}");
        assert_eq!(table_rows(&imported.content), [["1", "2"]]);
        assert_eq!(dropped(&imported), [("div", 1)]);
    }

    /// A line holding only carrier tags reads as tag lines wherever it
    /// stands, in the containers CommonMark puts it in, lazy continuation
    /// included, and its tags drop.
    #[test]
    fn a_line_of_carrier_tags_reads_as_tag_lines_wherever_it_stands() {
        let cases: &[(&str, &str, &[&[&str]])] = &[
            (
                "<qm-keep>\n| a | b |\n|---|---|\n| 1 | 2 |\n</qm-keep>",
                "\u{FFFC}",
                &[&[]],
            ),
            (
                "<qm-keep>\n**Signed**\nJ. Doe\n</qm-keep>",
                "Signed J. Doe",
                &[&[]],
            ),
            ("<qm-keep>\n\nA\n</qm-keep>\nB", "A\nB", &[&[], &[]]),
            ("A\n<qm-keep>\nB\n</qm-keep>", "A\nB", &[&[], &[]]),
            ("A\n    <qm-keep>\nB\n</qm-keep>", "A\nB", &[&[], &[]]),
            ("A\\\n</qm-keep>\nB", "A\nB", &[&[], &[]]),
            (
                "- a\n  <qm-keep>\n  b\n  </qm-keep>",
                "a\nb",
                &[&["list_item"], &["list_item"]],
            ),
            (
                "> A\n<qm-keep>\n> B\n> </qm-keep>",
                "A\nB",
                &[&["quote"], &["quote"]],
            ),
            (
                "> A\n> <qm-keep>\n> B\n> </qm-keep>",
                "A\nB",
                &[&["quote"], &["quote"]],
            ),
            ("<qm-sig></qm-sig>", "", &[&[]]),
            (
                "<qm-sig name=\"a\"></qm-sig>\n<qm-sig name=\"b\"></qm-sig>",
                "",
                &[&[]],
            ),
            (
                "- [ ] <qm-sig></qm-sig>\n- [x] b",
                "\nb",
                &[&["list_item"], &["list_item"]],
            ),
            (
                "<qm-keep><qm-box>\n\n| a |\n|---|\n| 1 |\n\n</qm-box></qm-keep>",
                "\u{FFFC}",
                &[&[]],
            ),
            (
                "| a |\n|---|\n<qm-sig></qm-sig>\nB",
                "\u{FFFC}\nB",
                &[&[], &[]],
            ),
        ];
        for (md, text, tags) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(container_tags(&imported.content), *tags, "{md:?}");
            assert!(
                dropped(&imported).iter().all(|(t, _)| t.starts_with("qm-")),
                "{md:?}"
            );
        }

        let split = imp_fixed("1. a\n<qm-keep>\n2. b\n</qm-keep>");
        assert_eq!(
            container_tags(&split.content),
            [["list_item"], ["list_item"]]
        );
        assert_eq!(dropped(&split), [("qm-keep", 1)]);
    }

    /// The text under a tag line in a paragraph reads as the paragraph read
    /// it, a line opening a block in a fresh paragraph included, and exports
    /// to markdown that imports the same.
    #[test]
    fn the_text_under_a_tag_line_keeps_its_paragraph_reading() {
        let cases: &[(&str, &str, &[&[&str]])] = &[
            (
                "A\n<qm-keep>\n[x]: /url\nB\n</qm-keep>",
                "A\n[x]: /url B",
                &[&[], &[]],
            ),
            ("A\n<qm-sig></qm-sig>\n    code", "A\ncode", &[&[], &[]]),
            ("A\n<qm-sig></qm-sig>\n2. b", "A\n2. b", &[&[], &[]]),
            ("A\n<qm-sig></qm-sig>\n<span>\nB", "A\nB", &[&[], &[]]),
            (
                "A\n<qm-sig></qm-sig>\n<qm-x>\nB\n</qm-x>",
                "A\nB",
                &[&[], &[]],
            ),
            ("> <qm-sig></qm-sig>\nB", "B", &[&["quote"]]),
            (
                "- a\n  <qm-sig></qm-sig>\nb",
                "a\nb",
                &[&["list_item"], &["list_item"]],
            ),
            (
                "> A\n> <qm-keep>\nB\n> </qm-keep>",
                "A\nB",
                &[&["quote"], &["quote"]],
            ),
        ];
        for (md, text, tags) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(container_tags(&imported.content), *tags, "{md:?}");
            let again = imp_fixed(&crate::lines::export::to_markdown(&imported.content));
            assert_eq!(again.content, imported.content, "{md:?}");
        }
    }

    /// A line of tags that are not all carrier ones, or one inside a mark,
    /// stays inline, its carrier tags dropping as any inline tag does.
    #[test]
    fn a_line_holding_other_tags_stays_inline() {
        let cases: &[(&str, &[(&str, usize)])] = &[
            (
                "A\n<span><qm-keep>\nB\n\n</qm-keep>",
                &[("span", 1), ("qm-keep", 1)],
            ),
            ("*a\n<qm-keep>\nb*\n\n</qm-keep>", &[("qm-keep", 1)]),
            ("<u>a\n<qm-sig></qm-sig>\nb</u>", &[("qm-sig", 1)]),
            ("A\n<qm-sig/>\nB", &[("qm-sig", 1)]),
            ("A\n<qm-anchor ref=\"x\"></qm-anchor>\nB", &[]),
        ];
        for (md, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.lines.len(), 1, "{md:?}");
            assert!(imported.content.lines[0].containers.is_empty(), "{md:?}");
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }

    fn grid(rt: &Lines) -> serde_json::Value {
        let [island] = rt.islands.as_slice() else {
            panic!("one island expected: {:?}", rt.islands);
        };
        assert_eq!(island.island_type, IslandType::Table);
        let mut props = island.props.clone();
        let strip = |row: &mut serde_json::Value| {
            for cell in row.as_array_mut().unwrap() {
                let o = cell.as_object_mut().unwrap();
                if o["marks"].as_array().unwrap().is_empty() {
                    o.remove("marks");
                }
            }
        };
        strip(&mut props["header"]);
        for row in props["rows"].as_array_mut().unwrap() {
            strip(row);
        }
        props
    }

    /// The issue's roster: a title row across the table, a header cell across
    /// two columns and a body cell down two rows, every covered cell an empty
    /// placeholder. It is the export's own spelling.
    #[test]
    fn an_html_table_resolves_its_spans_into_the_grid() {
        use serde_json::json;
        let md = "<table widths=\"1 2 2\" aligns=\"left left right\" align=\"center\">\n<thead>\n\
                  <tr><th colspan=\"3\">FLIGHT X PERSONNEL</th></tr>\n\
                  <tr><th>Date</th><th colspan=\"2\">POC</th></tr>\n</thead>\n<tbody>\n\
                  <tr><td rowspan=\"2\" valign=\"middle\">12 Nov</td><td>SSgt Doe</td><td>555-0101</td></tr>\n\
                  <tr><td>SrA Roe</td><td>555-0102</td></tr>\n</tbody>\n</table>";
        let imported = imp_fixed(md);
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(
            grid(&imported.content),
            json!({
                "aligns": ["left", "left", "right"], "align": "center", "head": 2, "widths": [1, 2, 2],
                "header": [{"text": "FLIGHT X PERSONNEL", "colspan": 3}, {"text": ""}, {"text": ""}],
                "rows": [
                    [{"text": "Date"}, {"text": "POC", "colspan": 2}, {"text": ""}],
                    [{"text": "12 Nov", "rowspan": 2, "valign": "middle"}, {"text": "SSgt Doe"}, {"text": "555-0101"}],
                    [{"text": ""}, {"text": "SrA Roe"}, {"text": "555-0102"}],
                ],
            })
        );
        assert_eq!(crate::lines::export::to_markdown(&imported.content), md);
    }

    /// The subset reads as HTML reads it where a writer leaves things out: an
    /// unquoted value, a row outside any group, an end tag left out, a `tfoot`
    /// after the body, a `th` in a body row, and the `th` rows opening a table
    /// with no `thead`, which head it. Each table here imports as the pipe
    /// table beside it.
    #[test]
    fn an_html_table_reads_as_html_reads_it() {
        let cases = [
            (
                "<table><tr><th>a<th>b<tr><td>1<td>2</table>",
                "| a | b |\n| --- | --- |\n| 1 | 2 |",
            ),
            (
                "<TABLE><THEAD><TR><TH>a</TH><TH>b</TH></TR></THEAD><TR><TD>1</TD><TD>2</TD></TR></TABLE>",
                "| a | b |\n| --- | --- |\n| 1 | 2 |",
            ),
            (
                "<table aligns=left,right>\n<tr><th>a</th><th>b</th></tr>\n</table>",
                "| a | b |\n| --- | --- |",
            ),
            (
                "<table aligns='Left Right'>\n<thead><tr><th>a</th><th>b</th></tr></thead>\n</table>",
                "| a | b |\n| :--- | ---: |",
            ),
            (
                "<table>\n<tr><th>a</th><th>b</th></tr>\n<tfoot><tr><td>f</td><td>g</td></tr></tfoot>\n<tr><td>1</td><td>2</td></tr>\n</table>",
                "| a | b |\n| --- | --- |\n| 1 | 2 |\n| f | g |",
            ),
            (
                "<table>\n<thead><tr><td>a</td><td>b</td></tr></thead>\n<tbody><tr><th>1</th><td/>2</tr></tbody>\n</table>",
                "| a | b |\n| --- | --- |\n| 1 | 2 |",
            ),
            (
                "<table>\n<tr><th>a</th><th>b</th></tr>\n<!-- </table> -->\n<tr><td>1</td><td>2</td></tr>\n</table>",
                "| a | b |\n| --- | --- |\n| 1 | 2 |",
            ),
        ];
        for (html, pipes) in cases {
            let imported = imp_fixed(html);
            let warned = dropped(&imported);
            let want = match html.contains("aligns=left,right") {
                true => vec![("table[aligns]", 1)],
                false => vec![],
            };
            assert_eq!(warned, want, "{html:?}");
            assert_eq!(imported.content, imp_fixed(pipes).content, "{html:?}");
        }
        let headed = imp_fixed(
            "<table>\n<tbody>\n<tr><th>a</th></tr>\n<tr><td>1</td></tr>\n</tbody>\n</table>",
        );
        assert_eq!(headed.content, imp_fixed("| a |\n| --- |\n| 1 |").content);
        let headless = imp_fixed("<table>\n<tbody>\n<tr><td>a</td></tr>\n</tbody>\n</table>");
        assert_eq!(grid(&headless.content)["head"], 0);
        let reaching = imp_fixed(
            "<table>\n<tr><th rowspan=2>a</th><th>b</th></tr>\n<tr><th>c</th></tr>\n<tr><td>1</td><td>2</td></tr>\n</table>",
        );
        assert_eq!(grid(&reaching.content)["head"], 2);
        let past = imp_fixed(
            "<table>\n<tr><th rowspan=2>a</th><th>b</th></tr>\n<tr><td>1</td></tr>\n</table>",
        );
        assert!(past.warnings.is_empty(), "{:?}", past.warnings);
        assert_eq!(grid(&past.content)["head"], 0);
        assert_eq!(grid(&past.content)["header"][0]["rowspan"], 2);
    }

    /// What the grid cannot hold as written drops alone, reported: an
    /// attribute the subset does not read or a value outside its spelling, a
    /// tag outside the subset and text outside every cell, and a row too short
    /// for the grid, which is as wide as its widest row. A span past its row
    /// group or over a slot an earlier span covers stops there silently, and an
    /// attribute that only styles the table drops silently.
    #[test]
    fn what_an_html_table_cannot_hold_drops_alone() {
        use serde_json::json;
        let cases: &[(&str, &[(&str, usize)], serde_json::Value)] = &[
            (
                "<table border=1 bgcolor=red widths=\"0 1\"><tr><td class=x style=\"color: red\" scope=col colspan=0>a</td><td rowspan=two align=justify valign=baseline>b</td></tr></table>",
                &[
                    ("table[bgcolor]", 1),
                    ("table[widths]", 1),
                    ("td[colspan]", 1),
                    ("td[align]", 1),
                    ("td[rowspan]", 1),
                    ("td[valign]", 1),
                ],
                json!({"aligns": ["none", "none"], "head": 0, "headless": true, "header": [{"text": "a"}, {"text": "b"}], "rows": []}),
            ),
            (
                "<table size=3 text=x row=1><tr><td>a</td></tr></table>",
                &[("table[size]", 1), ("table[text]", 1), ("table[row]", 1)],
                json!({"aligns": ["none"], "head": 0, "headless": true, "header": [{"text": "a"}], "rows": []}),
            ),
            (
                "<table><caption>Roster</caption><colgroup><col></colgroup><tr><td>a</td>stray<td>b</td></tr></table>",
                &[
                    ("caption", 1),
                    ("table:text", 2),
                    ("colgroup", 1),
                    ("col", 1),
                ],
                json!({"aligns": ["none", "none"], "head": 0, "headless": true, "header": [{"text": "a"}, {"text": "b"}], "rows": []}),
            ),
            (
                "<table><tr><td>a</td><td>b</td></tr><tr><td>1</td></tr><tr><td>2</td><td>3</td><td>4</td></tr></table>",
                &[("table:row", 2)],
                json!({"aligns": ["none", "none", "none"], "head": 0, "headless": true, "header": [{"text": "a"}, {"text": "b"}, {"text": ""}], "rows": [[{"text": "1"}, {"text": ""}, {"text": ""}], [{"text": "2"}, {"text": "3"}, {"text": "4"}]]}),
            ),
            (
                "<table><tr><td rowspan=2>a</td><td>b</td><td>c</td></tr><tr><td></td><td>x</td><td>y</td></tr></table>",
                &[("table:row", 1)],
                json!({"aligns": ["none", "none", "none", "none"], "head": 0, "headless": true, "header": [{"text": "a", "rowspan": 2}, {"text": "b"}, {"text": "c"}, {"text": ""}], "rows": [[{"text": ""}, {"text": ""}, {"text": "x"}, {"text": "y"}]]}),
            ),
            (
                "<table><tr><td>a</td><td rowspan=2>b</td><td>c</td></tr><tr><td colspan=3>d</td></tr></table>",
                &[],
                json!({"aligns": ["none", "none", "none"], "head": 0, "headless": true, "header": [{"text": "a"}, {"text": "b", "rowspan": 2}, {"text": "c"}], "rows": [[{"text": "d"}, {"text": ""}, {"text": ""}]]}),
            ),
            (
                "<table><thead><tr><th rowspan=5>h</th><th>a</th></tr></thead><tr><td>1</td><td colspan=3>2</td></tr></table>",
                &[("table:row", 1)],
                json!({"aligns": ["none", "none", "none", "none"], "header": [{"text": "h"}, {"text": "a"}, {"text": ""}, {"text": ""}], "rows": [[{"text": "1"}, {"text": "2", "colspan": 3}, {"text": ""}, {"text": ""}]]}),
            ),
        ];
        for (md, warned, want) in cases {
            let imported = imp_fixed(md);
            let mut got = dropped(&imported);
            got.sort();
            let mut warned = warned.to_vec();
            warned.sort();
            assert_eq!(got, warned, "{md:?}");
            assert_eq!(grid(&imported.content), *want, "{md:?}");
        }
    }

    /// Markdown after a table's `</table>` in its block reads as markdown, and
    /// a wrapper's close tag there drops.
    #[test]
    fn markdown_after_a_table_reads_as_its_own() {
        let imported = imp_fixed("<table><tr><td>a</td></tr></table>\nSome **prose**.\n- item");
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(imported.content.text, "\u{FFFC}\nSome prose.\nitem");
        let memo: String = (1..=12).map(|i| format!("### Flight {i}\n<table><tr><th>Name</th></tr><tr><td>SSgt {i}</td></tr></table>\n")).collect();
        let memo = imp_fixed(&memo);
        assert!(memo.warnings.is_empty(), "{:?}", memo.warnings);
        assert_eq!(memo.content.islands.len(), 12);
        assert!(
            memo.content.text.ends_with("Flight 12\n\u{FFFC}"),
            "{:?}",
            memo.content.text
        );
        let kept =
            imp_fixed("<qm-keep>\n\n<table><tr><td>a</td></tr></table>\n</qm-keep>\n\nafter");
        assert_eq!(dropped(&kept), [("qm-keep", 1)]);
        assert_eq!(kept.content.text, "\u{FFFC}\nafter");
        assert!(kept.content.lines.iter().all(|l| l.containers.is_empty()));
    }

    /// A table is as wide as its widest row, so a title row over wider rows
    /// pads, and its `<th>` rows in a `tbody` are its head rows.
    #[test]
    fn a_short_title_row_pads() {
        let imported = imp_fixed(
            "<table><tbody><tr><th>Roster</th></tr><tr><td>a</td><td>b</td><td>c</td></tr></tbody></table>",
        );
        assert_eq!(dropped(&imported), [("table:row", 1)]);
        assert_eq!(table_rows(&imported.content), [["a", "b", "c"]]);
        assert_eq!(grid(&imported.content)["header"][0]["text"], "Roster");
    }

    /// An unclosed comment or code span in a table's block, and `<th>` rows
    /// each reaching past the next, read in time linear in their length.
    #[test]
    fn unclosed_comments_and_code_spans_read_in_linear_time() {
        for md in [
            format!("<table>{}", "<!--".repeat(100_000)),
            format!(
                "<table><tr><td>{}</td></tr></table>",
                (1..1_500).map(|n| "`".repeat(n) + "x").collect::<String>()
            ),
            format!("<table><tr><th>a{}", "<tr><th rowspan=2>a".repeat(30_000)),
        ] {
            assert_eq!(from_markdown(&md).unwrap().content.validate(), Ok(()));
        }
    }

    /// A table stops at [`MAX_TABLE_CELLS`](crate::MAX_TABLE_CELLS) slots, its
    /// rows past them dropping, reported, as an HTML or a pipe table; one whose
    /// first row alone is wider drops whole, read no further than the bound.
    #[test]
    fn a_table_stops_at_its_cell_bound() {
        let cols = 1000;
        let rows = crate::MAX_TABLE_CELLS / cols;
        let html = format!(
            "<table><tr><td colspan={cols}>a</td></tr>{}</table>",
            "<tr>".repeat(rows + 5)
        );
        let pipe = format!(
            "|{}\n|{}\n{}",
            " a |".repeat(cols),
            " - |".repeat(cols),
            "| b |\n".repeat(rows + 5)
        );
        for md in [html, pipe] {
            let imported = imp_fixed(&md);
            assert_eq!(dropped(&imported).last(), Some(&("table:size", 1)));
            assert_eq!(1 + table_rows(&imported.content).len(), rows);
        }
        for n in [101, 20_000] {
            let wide = format!(
                "<table><tr>{}</tr></table>",
                "<td colspan=1000></td>".repeat(n)
            );
            let imported = imp_fixed(&wide);
            assert_eq!(dropped(&imported), [("table:size", 1)]);
            assert!(imported.content.islands.is_empty());
        }
    }

    /// The tables of a document share the one bound, so tables each under
    /// it, as HTML or as pipes, stop together at it, reported, however few
    /// bytes their spans or short rows spend; a document of small tables reads
    /// whole. A cell past the bound in grid order is left out as the scan reads
    /// it, and a head written after it still heads the table.
    #[test]
    fn a_documents_tables_share_the_cell_bound() {
        let slots = |imported: &Imported| -> usize {
            let rows = |isl: &Island| 1 + isl.props["rows"].as_array().map_or(0, Vec::len);
            imported
                .content
                .islands
                .iter()
                .map(|isl| rows(isl) * isl.props["aligns"].as_array().map_or(0, Vec::len))
                .sum()
        };
        let sized = |imported: &Imported| {
            imported
                .warnings
                .iter()
                .any(|w| w.construct == DroppedConstruct::TableSize)
        };
        let html = format!("<table>{}</table>\n\n", "<tr><td colspan=1000>".repeat(100));
        let pipe = format!(
            "{}|\n{}|\n{}\n",
            "|a".repeat(1000),
            "|-".repeat(1000),
            "|x|\n".repeat(99)
        );
        for md in [
            html.repeat(5),
            pipe.repeat(3),
            format!("{html}{pipe}{html}"),
        ] {
            let imported = from_markdown(&md).unwrap();
            assert_eq!(slots(&imported), crate::MAX_TABLE_CELLS);
            assert!(sized(&imported));
        }
        let small = imp_fixed(&"<table><td>x</table>\n\n|a|\n|-|\n|x|\n\n".repeat(1_000));
        assert_eq!((small.content.islands.len(), slots(&small)), (2_000, 3_000));
        assert!(small.warnings.is_empty());
        let late_head = format!(
            "<table><tbody><tr>{}</tbody><thead><tr><th>h</thead></table>",
            "<td>".repeat(crate::MAX_TABLE_CELLS + 1)
        );
        let imported = from_markdown(&late_head).unwrap();
        assert_eq!(
            grid(&imported.content)["header"],
            serde_json::json!([{"text": "h"}])
        );
        assert_eq!(table_rows(&imported.content), Vec::<Vec<String>>::new());
        assert!(sized(&imported));
    }

    /// A code span in a cell never reaches past one of the subset's tags, so a
    /// stray backtick is its text and splits no cell, row or table; a span
    /// holding a tag a comment hides splits around it, the tag its text.
    #[test]
    fn a_stray_backtick_in_a_cell_is_its_text() {
        let texts = |imported: &Imported| -> Vec<String> {
            crate::lines::serial::table_cells(&imported.content.islands[0].props)
                .into_iter()
                .map(|(text, _)| text)
                .collect()
        };
        let imported = imp_fixed("<table><tr><td>`a</td><td>b`</td><td>c</td></tr></table>");
        assert_eq!(texts(&imported), ["`a", "b`", "c"]);
        let imported = imp_fixed("<table><tr><td>`a</td></tr></table> after `x`");
        assert_eq!(texts(&imported), ["`a"]);
        assert!(
            imported
                .content
                .marks
                .iter()
                .any(|m| m.kind == MarkKind::Code)
        );
        assert!(imported.warnings.is_empty());
        let imported = imp_fixed("<table><tr><td>`a <!-- <td> --> b`</td></tr></table>");
        let cells: Vec<(String, Vec<Mark>)> =
            crate::lines::serial::table_cells(&imported.content.islands[0].props);
        assert_eq!(
            cells[0],
            (
                "a <!-- <td> --> b".into(),
                vec![
                    Mark::new(0, 7, MarkKind::Code),
                    Mark::new(11, 17, MarkKind::Code)
                ]
            )
        );
    }

    /// A table is one HTML block: a blank line inside it ends the table there,
    /// in a quote as at the top level, and what follows reads as markdown, an
    /// HTML block of its own that drops. Another `<table>` ends one too, and
    /// one that holds no cell imports nothing.
    #[test]
    fn an_html_table_ends_with_its_block() {
        for md in [
            "<table>\n<tr><td>a</td></tr>\n\n<tr><td>b</td></tr>\n</table>",
            "> <table>\n> <tr><td>a</td></tr>\n>\n> <tr><td>b</td></tr>\n> </table>",
        ] {
            let imported = imp_fixed(md);
            assert_eq!(
                dropped(&imported),
                [("table", 1), ("tr", 1), ("td", 1)],
                "{md:?}"
            );
            assert_eq!(
                table_rows(&imported.content),
                Vec::<Vec<String>>::new(),
                "{md:?}"
            );
            assert_eq!(
                grid(&imported.content)["header"],
                serde_json::json!([{"text": "a"}]),
                "{md:?}"
            );
        }
        let two = imp_fixed(
            "<table><tr><td>a<table><tr><td>b</td></tr></table>\n<table><tr><td>c</td></tr></table>",
        );
        assert_eq!(two.content.islands.len(), 3);
        assert_eq!(two.content.text, "\u{FFFC}\n\u{FFFC}\n\u{FFFC}");
        assert_eq!(dropped(&two), [("table", 1)]);
        let empty = imp_fixed("para\n\n<table>\n<tr></tr>\n</table>");
        assert_eq!(
            (empty.content.text.as_str(), empty.warnings.len()),
            ("para", 0)
        );
    }

    /// A cell's content is one line of inline markdown, a line break in the
    /// source a space: marks, a code span, an escaped `<` shielding a tag, an
    /// angle-bracketed link destination, `<br>` and `<u>`. An equation keeps
    /// its source, as in a pipe table's cell, and a footnote reference, read
    /// apart from the definitions, is its text.
    #[test]
    fn an_html_cell_holds_inline_markdown() {
        let md = "<table><tr><td>**Total**\n  and *more*</td><td>`a` \\<td> [l](<td x>)</td>\
                  <td>a<br><u>b</u> &amp; &#32;</td><td>$x$ y[^1]</td></tr></table>\n\n[^1]: n";
        let imported = imp_fixed(md);
        let props = &imported.content.islands[0].props;
        let cells: Vec<(String, Vec<Mark>)> = crate::lines::serial::table_cells(props);
        assert_eq!(
            cells[0],
            (
                "Total and more".into(),
                vec![
                    Mark::new(0, 5, MarkKind::Strong),
                    Mark::new(10, 14, MarkKind::Emph)
                ]
            )
        );
        assert_eq!(
            cells[1],
            (
                "a <td> l".into(),
                vec![
                    Mark::new(0, 1, MarkKind::Code),
                    Mark::new(7, 8, MarkKind::Link { url: "td x".into() })
                ]
            )
        );
        assert_eq!(
            cells[2],
            (
                "a\nb &  ".into(),
                vec![Mark::new(2, 3, MarkKind::Underline)]
            )
        );
        assert_eq!(cells[3].0, "$x$ y[^1]");
        assert_eq!(dropped(&imported), [("math", 1), ("footnote", 1)]);
    }

    /// A cell reads the formatting tags HTML spells its marks with: `b`,
    /// `strong`, `i`, `em`, `s`, `del` and `code` bare, `a` with an `href`
    /// alone. A `code` pair holds its source verbatim. A close tag ends the
    /// innermost open tag of its name, the tags still open inside it dropping.
    /// A cell a pair reads in holds what its markdown spelling reads back, so
    /// a mark's edge whitespace falls outside it. A tag with another
    /// attribute, one crossing a markdown mark and an `a` inside a link drop as
    /// tags do; prose reads none of them.
    #[test]
    fn an_html_cell_reads_formatting_tags_as_marks() {
        let md = "<table><tr><td><b>a</b> <STRONG>b</STRONG> <i>c</i> <em>d</em></td>\
                  <td><s>e</s> <del>f</del> <code>*g* <b>h</b></code></td>\
                  <td><a href=\"https://x.test/?a&amp;b\"><b>i</b></a></td>\
                  <td><b class=k>j</b> <b>k <i>l</b></i> *<b>m*</b> [n <a href=y>o</a>](z)</td>\
                  <td>q<b> r </b>s<em> </em>t</td></tr></table>\n\n<b>p</b>";
        let imported = imp_fixed(md);
        let cells: Vec<(String, Vec<Mark>)> =
            crate::lines::serial::table_cells(&imported.content.islands[0].props);
        assert_eq!(
            cells[0],
            (
                "a b c d".into(),
                vec![
                    Mark::new(0, 1, MarkKind::Strong),
                    Mark::new(2, 3, MarkKind::Strong),
                    Mark::new(4, 5, MarkKind::Emph),
                    Mark::new(6, 7, MarkKind::Emph)
                ]
            )
        );
        assert_eq!(
            cells[1],
            (
                "e f *g* <b>h</b>".into(),
                vec![
                    Mark::new(0, 1, MarkKind::Strike),
                    Mark::new(2, 3, MarkKind::Strike),
                    Mark::new(4, 16, MarkKind::Code)
                ]
            )
        );
        let url = "https://x.test/?a&b".to_string();
        assert_eq!(
            cells[2],
            (
                "i".into(),
                vec![
                    Mark::new(0, 1, MarkKind::Link { url }),
                    Mark::new(0, 1, MarkKind::Strong)
                ]
            )
        );
        assert_eq!(
            cells[3],
            (
                "j k l m n o".into(),
                vec![
                    Mark::new(2, 5, MarkKind::Strong),
                    Mark::new(6, 7, MarkKind::Emph),
                    Mark::new(8, 11, MarkKind::Link { url: "z".into() })
                ]
            )
        );
        assert_eq!(
            cells[4],
            ("q r s t".into(), vec![Mark::new(2, 3, MarkKind::Strong)])
        );
        assert_eq!(imported.content.text, "\u{FFFC}\np");
        assert!(imported.content.marks.is_empty());
        let unspellable = imp_fixed("<table><td><em>[l](z)</em>x</table>");
        assert_eq!(
            crate::lines::serial::table_cells(&unspellable.content.islands[0].props)[0],
            (
                "lx".into(),
                vec![Mark::new(0, 1, MarkKind::Link { url: "z".into() })]
            )
        );
        assert_eq!(dropped(&imported), [("b", 3), ("i", 1), ("a", 1)]);
    }

    /// A reference in a cell resolves against the document's definitions, as
    /// in a pipe table's cell, and one naming no definition is its text.
    #[test]
    fn an_html_cell_resolves_reference_links() {
        let md = "<table><tr><td>[AFI 36-2903]</td><td>[see][afi  36-2903] [none]</td></tr></table>\n\n\
                  [AFI 36-2903]: https://x.test/afi \"Dress\"";
        let imported = imp_fixed(md);
        let cells: Vec<(String, Vec<Mark>)> =
            crate::lines::serial::table_cells(&imported.content.islands[0].props);
        let link = |start, end| {
            Mark::new(
                start,
                end,
                MarkKind::Link {
                    url: "https://x.test/afi".into(),
                },
            )
        };
        assert_eq!(cells[0], ("AFI 36-2903".into(), vec![link(0, 11)]));
        assert_eq!(cells[1], ("see [none]".into(), vec![link(0, 3)]));
        assert!(imported.warnings.is_empty());
    }

    /// A fence directly under a `</table>` reads as a code block where its
    /// HTML block closes it.
    #[test]
    fn a_fence_under_a_table_imports() {
        let imported = imp_fixed("<table><tr><td>a</td></tr></table>\n```rust\nlet x;\n```\nafter");
        assert_eq!(imported.content.islands.len(), 1);
        assert_eq!(imported.content.text, "\u{FFFC}\nlet x;\nafter");
        assert!(imported.warnings.is_empty());
    }

    /// A note holding an HTML table keeps its cells' text, a line per row, as
    /// it keeps a pipe table's.
    #[test]
    fn an_html_table_in_a_note_is_its_text() {
        let imported = imp_fixed(
            "a[^1]\n\n[^1]: <table><tr><td>x</td><td>**y**</td></tr><tr><td>z</td><td>w</td></tr></table>",
        );
        let note = crate::lines::serial::parse_cell(&imported.content.islands[0].props);
        assert_eq!(
            note,
            ("x y\nz w".into(), vec![Mark::new(2, 3, MarkKind::Strong)])
        );
        assert_eq!(dropped(&imported), [("footnote[table]", 1)]);
    }

    /// A type-7 tag cannot interrupt a pipe table, so the parser reads
    /// `</qm-keep>` after the rows as one more row; the repair ends the
    /// table there instead. A type-6 tag (`</div>`) interrupts it on its own,
    /// and its block drops with the text it holds.
    #[test]
    fn a_row_of_tags_ends_its_table() {
        for close in ["</qm-keep>", "</qm-sig></qm-keep>"] {
            let rt = imp_fixed(&format!("| a | b |\n|---|---|\n| 1 | 2 |\n{close}\nnext")).content;
            assert_eq!(table_rows(&rt), [["1", "2"]], "{close}");
            assert_eq!(rt.text, "\u{FFFC}\nnext", "{close}");
        }
        let imported = imp_fixed("| a | b |\n|---|---|\n| 1 | 2 |\n</div>\nnext");
        assert_eq!(table_rows(&imported.content), [["1", "2"]]);
        assert_eq!(imported.content.text, "\u{FFFC}");
        assert_eq!(dropped(&imported), [("div", 1)]);
        let rt = imp_fixed("> | a |\n> |---|\n> | 1 |\n> </qm-keep>\n> next").content;
        assert_eq!(table_rows(&rt), [["1"]]);
        assert!(
            rt.lines.iter().all(|l| l.containers.len() == 1),
            "{:?}",
            rt.lines
        );
    }

    /// Text after a comment's `-->` on its last line moves to a line of its
    /// own, and ends there rather than joining the line after; one opening
    /// another HTML block opens it there. Mid-line, a comment is inline HTML
    /// and splits nothing.
    #[test]
    fn text_after_a_comment_imports() {
        let cases: &[(&str, &str)] = &[
            ("<!-- comment -->Same line text", "Same line text"),
            (
                "<!--\nmultiline\ncomment\n-->Trailing text",
                "Trailing text",
            ),
            (
                "<!-- first -->Text\n\n<!-- second -->More text",
                "Text\nMore text",
            ),
            ("<!--- comment --->Trailing text", "Trailing text"),
            ("<!-- <!-- -->Trailing", "Trailing"),
            ("   <!-- c -->Text", "Text"),
            ("    <!-- c -->Text", "<!-- c -->Text"),
            ("-->some text", "-->some text"),
            (
                "Some text before <!-- comment -->and after",
                "Some text before and after",
            ),
            ("<!-- a -->Title\n===", "Title\n==="),
            ("<!-- a --><div>\nnext", "next"),
            ("<!-- a --><!-- b\nmore\n-->", "more -->"),
        ];
        for (md, text) in cases {
            assert_eq!(imp_fixed(md).content.text, *text, "{md:?}");
        }
        let rt = imp_fixed("> <!-- a -->text\noutside").content;
        assert_eq!(rt.text, "text\noutside");
        assert_eq!(rt.lines[0].containers.len(), 1);
        assert!(rt.lines[1].containers.is_empty());
    }

    /// A tag alone on its line opens an HTML block, whatever its name: the
    /// inline allowlist does not reach it.
    #[test]
    fn the_allowlist_is_inline_only() {
        let imported = imp_fixed("<u>\ntext\n</u>");
        assert_eq!(imported.content.text, "");
        assert_eq!(dropped(&imported), [("u", 1)]);

        let imported = imp_fixed("para\n\n<br>\n\nnext");
        assert_eq!(imported.content.text, "para\nnext");
        assert_eq!(dropped(&imported), [("br", 1)]);
    }

    /// One count per open or self-closing tag, by lowercase name, in order of
    /// first occurrence: closing tags, comments, a `<pre>` block's content, the
    /// inline allowlist, `qm-anchor` and a block element count nothing. A
    /// tag of the allowlist inside an HTML block drops with it and counts. An
    /// element inside a line, or one that drops unclosed, counts under
    /// `qm-<name>`, and a `qm-*` name outside the grammar under its full
    /// name. A block holding no other opening tag counts its first where it
    /// drops text with it.
    #[test]
    fn dropped_tags_count_once_per_opening() {
        let md = "<div>\n<span>a</span> <SPAN>b</SPAN><br> <u>c</u> <img src=x/>\n</div>\n\n\
                  <!-- <em>not markup</em> -->\n\n<pre><b>x</b></pre>\n\n\
                  x <span>y</span> <u>z</u><br>w\n\n\
                  <qm-anchor id=\"x\">t</qm-anchor> <qm-keep>k</qm-keep>\n\
                  <QM-ANCHOR ref=\"y\"></QM-ANCHOR>\n<Qm-Keep>\n\n\
                  | <span>cell</span> |\n|---|\n| <hr/> <qm-a--b>z</qm-a--b> |\n\n\
                  </Center>\ndropped";
        let imported = imp_fixed(md);
        assert_eq!(
            dropped(&imported),
            [
                ("div", 1),
                ("span", 4),
                ("br", 1),
                ("u", 1),
                ("img", 1),
                ("pre", 1),
                ("qm-keep", 2),
                ("hr", 1),
                ("qm-a--b", 1),
                ("center", 1)
            ]
        );
        assert!(
            imp_fixed("plain **text**, `<code>`, \\<escaped>")
                .warnings
                .is_empty()
        );
        assert!(imp_fixed("</div>\n\npara").warnings.is_empty());
    }

    /// A block that drops markdown reports under its first tag where no
    /// opening tag in it counts: a stray close tag, `qm-anchor`, and a close
    /// whose element then drops unclosed or closes later, reported once.
    #[test]
    fn a_block_dropping_markdown_reports_under_its_first_tag() {
        let cases = [
            ("a\n\n</qm-keep>\n</span>\ntext", ("qm-keep", 1)),
            ("a\n\n</qm-anchor>\ntext", ("qm-anchor", 1)),
            ("<qm-anchor ref=\"x\">\ntext", ("qm-anchor", 1)),
            (
                "<qm-keep>\n\nx\n\n</qm-keep>\n</span>\ntext",
                ("qm-keep", 1),
            ),
            (
                "<qm-keep>\n\nx\n\n</qm-keep>\n</span>\ntext\n\n</qm-keep>",
                ("qm-keep", 1),
            ),
        ];
        for (md, report) in cases {
            let imported = imp_fixed(md);
            assert_eq!(dropped(&imported), [report], "{md:?}");
            assert!(!imported.content.text.contains("text"), "{md:?}");
        }
    }

    /// A `<script>`, `<style>` or `<textarea>` block drops through the line
    /// holding its close tag, blank lines and markdown inside included, and
    /// reports its tag once.
    #[test]
    fn a_raw_text_block_drops_through_its_close_and_reports_its_tag() {
        for (md, tag) in [
            (
                "before\n\n<script>\nlet a = 1;\n\nlet b = 2;\n</script>\n\nafter",
                "script",
            ),
            (
                "before\n\n<style>p { color: red }</style>\n\nafter",
                "style",
            ),
            (
                "before\n\n<textarea>\n**not**\n\n<qm-keep>\n</textarea> tail\n\nafter",
                "textarea",
            ),
        ] {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, "before\nafter", "{md:?}");
            assert!(imported.content.marks.is_empty(), "{md:?}");
            assert_eq!(dropped(&imported), [(tag, 1)], "{md:?}");
        }
    }

    /// A carrier tag among raw tag lines drops as either reads: a block of tag
    /// lines alone or a line CommonMark reads inline.
    #[test]
    fn a_carrier_tag_among_raw_tag_lines_drops() {
        let imported = imp_fixed("<div>\n<qm-keep>\n\npara\n\n</qm-keep>\n</div>\n\nafter");
        assert_eq!(imported.content.text, "para\nafter");
        assert!(
            imported
                .content
                .lines
                .iter()
                .all(|l| l.containers.is_empty())
        );
        assert_eq!(dropped(&imported), [("div", 1), ("qm-keep", 1)]);

        let imported = imp_fixed("<div><qm-keep>\n\npara\n\n</qm-keep></div>\n\nafter");
        assert_eq!(imported.content.text, "para\nafter");
        assert!(
            imported
                .content
                .lines
                .iter()
                .all(|l| l.containers.is_empty())
        );
        assert_eq!(dropped(&imported), [("div", 1), ("qm-keep", 1)]);
    }

    /// A close tag meeting its open tag answers it, so a later close tag that
    /// drops markdown with it reports, in the body as in a note, whichever
    /// comes first.
    #[test]
    fn a_closed_tag_leaves_a_later_dropping_close_to_report() {
        let pair = "<qm-keep>\n\nA\n\n</qm-keep>";
        let stray = "</qm-keep>\n</div>\nC";
        let imported = imp_fixed(&format!("{pair}\n\n{stray}"));
        assert_eq!(dropped(&imported), [("qm-keep", 2)]);
        let note = "x[^1]\n\n[^1]: n\n\n    </qm-keep>\n    </div>\n    lost";
        for markdown in [format!("{pair}\n\n{note}"), format!("{note}\n\n{pair}")] {
            assert_eq!(
                dropped(&imp_fixed(&markdown)),
                [("qm-keep", 2)],
                "{markdown:?}"
            );
        }
    }

    #[test]
    fn a_bidi_control_drops_with_a_report() {
        let imported = imp_fixed("Price \u{200F}123\u{200E} shekel \u{2067}abc\u{2069}");
        assert_eq!(imported.content.text, "Price 123 shekel abc");
        assert_eq!(dropped(&imported), [("bidi", 4)]);
    }

    /// CRLF line endings import as LF ones do, through every block the
    /// carrier, a table cell's break and a fence hold.
    #[test]
    fn crlf_imports_as_lf() {
        let lf = "<qm-keep note=\"a\">\n\n| A | B |\n|---|---|\n| <u>x</u> | y<br>z |\n\n</qm-keep>\n\n\
                  - item\n  more\n\n> quote\n\n```\ncode\n```\n\n<span>x</span>\n";
        let crlf = lf.replace('\n', "\r\n");
        let (lf, crlf) = (imp_fixed(lf), imp_fixed(&crlf));
        assert_eq!(crlf.content, lf.content);
        assert_eq!(crlf.warnings, lf.warnings);
        assert_eq!(dropped(&lf), [("qm-keep", 1), ("span", 1)]);
    }

    fn container_tags(rt: &Lines) -> Vec<Vec<&'static str>> {
        rt.lines
            .iter()
            .map(|l| l.containers.iter().map(Container::tag).collect())
            .collect()
    }

    /// An element tag alone on its line wraps nothing: the blocks up to its
    /// close tag import in the containers around it, its own indentation
    /// placing them, so after a list item they do not continue it.
    #[test]
    fn an_element_tag_line_wraps_nothing() {
        let cases: &[(&str, &str, &[&[&str]])] = &[
            ("<qm-keep>\n\npara\n\n</qm-keep>", "para", &[&[]]),
            (
                "<qm-keep>\n\n- a\n- b\n\n</qm-keep>",
                "a\nb",
                &[&["list_item"], &["list_item"]],
            ),
            (
                "- <qm-keep>\n\n  a\n\n  </qm-keep>\n- b",
                "a\nb",
                &[&["list_item"], &["list_item"]],
            ),
            ("> <qm-keep>\n>\n> q\n>\n> </qm-keep>", "q", &[&["quote"]]),
            ("<qm-keep>\n</qm-keep>", "", &[&[]]),
            (
                "- a\n\n<qm-keep>\n\n- b\n\n</qm-keep>",
                "a\nb",
                &[&["list_item"], &["list_item"]],
            ),
        ];
        for (md, text, tags) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert_eq!(container_tags(&imported.content), *tags, "{md:?}");
            assert!(
                dropped(&imported).iter().all(|(t, _)| t.starts_with("qm-")),
                "{md:?}"
            );
        }
    }

    /// An inline element pair drops its tags and reports `qm-<name>` once
    /// per open tag, in prose, a heading and a table cell, what it holds
    /// importing as written.
    #[test]
    fn an_inline_element_pair_drops() {
        for (md, text) in [
            ("a <qm-hl tone=\"warm\">b</qm-hl> c", "a b c"),
            ("<qm-hl>a **b</qm-hl> c**", "a b c"),
            ("# a <qm-hl>b</qm-hl>", "a b"),
        ] {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, text, "{md:?}");
            assert!(
                imported
                    .content
                    .marks
                    .iter()
                    .all(|m| m.kind == MarkKind::Strong),
                "{md:?}"
            );
            assert_eq!(dropped(&imported), [("qm-hl", 1)], "{md:?}");
        }

        let imported = imp_fixed("| <qm-hl>x</qm-hl> y |\n|---|");
        let (text, marks) =
            crate::lines::serial::parse_cell(&imported.content.islands[0].props["header"][0]);
        assert_eq!(text, "x y");
        assert!(marks.is_empty());
        assert_eq!(dropped(&imported), [("qm-hl", 1)]);
    }

    /// An element the carrier cannot read drops its tags and reports
    /// `qm-<name>`, what it wraps importing: one left open, a self-closing one,
    /// which HTML reads as an open tag, and one inside a line. An attribute
    /// outside the grammar drops alone. A close tag with nothing to close
    /// drops silently.
    #[test]
    fn an_element_drops_where_it_does_not_close() {
        let cases: &[(&str, &str, &[(&str, usize)])] = &[
            ("<qm-keep>\n\na", "a", &[("qm-keep", 1)]),
            ("<qm-keep/>\n\na", "a", &[("qm-keep", 1)]),
            (
                "- <qm-keep>\n\n  a\n- b\n\n</qm-keep>",
                "a\nb",
                &[("qm-keep", 1)],
            ),
            ("> <qm-keep>\n>\n> a\n\n</qm-keep>", "a", &[("qm-keep", 1)]),
            ("</qm-keep>\n\na", "a", &[]),
            ("a\n</qm-keep>", "a", &[]),
            ("a <qm-hl>b", "a b", &[("qm-hl", 1)]),
            ("a<qm-hl></qm-hl>b", "ab", &[("qm-hl", 1)]),
        ];
        for (md, text, warned) in cases {
            let imported = imp_fixed(md);
            assert_eq!(imported.content.text, *text, "{md:?}");
            assert!(imported.content.marks.is_empty(), "{md:?}");
            assert!(
                imported
                    .content
                    .lines
                    .iter()
                    .flat_map(|l| &l.containers)
                    .all(|c| c.tag() != "element"),
                "{md:?}"
            );
            assert_eq!(dropped(&imported), *warned, "{md:?}");
        }
    }
}
