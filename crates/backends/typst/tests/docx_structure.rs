//! The structure a DOCX render writes: paragraphs, styles, sections and
//! anchors, read back from the package.

mod common;
mod word;

use word::render;

const PAGE: &str = "#set page(width: 300pt, height: 300pt, margin: 30pt)\n";

#[test]
fn sibling_blocks_are_paragraphs_of_their_own() {
    let w = render(&format!(
        "{PAGE}#block[First paragraph in its own block.]\n\
         #block[Second paragraph in its own block.]\n\
         #block[A third, #underline[underlined] in its block.]\n"
    ));
    assert_eq!(
        w.texts(),
        [
            "First paragraph in its own block.",
            "Second paragraph in its own block.",
            "A third, underlined in its block.",
        ]
    );
    assert!(
        w.xml("word/document.xml")
            .contains("<w:u w:val=\"single\"/>")
    );
    assert!(!w.has("docx::shape_dropped"), "{:?}", w.codes);
}

#[test]
fn an_aligned_box_in_a_grid_cell_is_aligned_not_indented() {
    let w = render(&format!(
        "{PAGE}#grid(columns: (1fr, 2in), [Left], align(right, box[18 November 2026]))\n\
         #align(center)[Centred alone]\n"
    ));
    let date = w.par("18\u{a0}November\u{a0}2026");
    assert!(date.in_table);
    assert_eq!(date.jc.as_deref(), Some("right"));
    assert_eq!(date.ind_left, None);
    let centred = w.par("Centred alone");
    assert_eq!(centred.jc.as_deref(), Some("center"));
    assert_eq!(centred.ind_left, None);
}

#[test]
fn a_footer_of_two_lines_is_two_paragraphs() {
    let w = render(
        "#set page(width: 300pt, height: 300pt, margin: 40pt, numbering: \"1\", \
         footer: [#align(center)[#context counter(page).display()] #align(center)[MARKING]])\n\
         Body text.\n",
    );
    let sections = w.sections();
    let (_, footer) = &sections[0].footers[0];
    let pars = w.part_paragraphs(footer);
    let boxes: Vec<Vec<String>> = pars
        .iter()
        .flat_map(|p| &p.drawings)
        .map(|d| d.lines.clone())
        .collect();
    assert_eq!(boxes, [["1", "MARKING"]]);
}

#[test]
fn a_heading_whose_show_rule_breaks_the_page_keeps_its_style() {
    let w = render(&format!(
        "{PAGE}#show heading.where(level: 1): it => {{ pagebreak(weak: true); it }}\n\
         = First chapter\n\
         Some text.\n\
         = Second chapter\n\
         More text.\n\
         == A section\n\
         Even more.\n"
    ));
    let pars = w.body();
    let shape: Vec<(&str, Option<&str>, bool, usize)> = pars
        .iter()
        .map(|p| {
            (
                p.text.as_str(),
                p.style.as_deref(),
                p.page_break_before,
                p.page,
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("First chapter", Some("Heading1"), false, 0),
            ("Some text.", None, false, 0),
            ("Second chapter", Some("Heading1"), true, 1),
            ("More text.", None, false, 1),
            ("A section", Some("Heading2"), false, 1),
            ("Even more.", None, false, 1),
        ]
    );
}

/// Each section's number format and restart, and what its default footer
/// prints: each text box's lines and fields.
fn numbering(w: &word::Word) -> Vec<(Option<String>, Option<i64>, Vec<String>, Vec<String>)> {
    w.sections()
        .into_iter()
        .map(|s| {
            let footer = s
                .footers
                .iter()
                .find(|(kind, _)| kind == "default")
                .map(|(_, part)| w.part_paragraphs(part))
                .unwrap_or_default();
            let boxes: Vec<_> = footer.iter().flat_map(|p| p.drawings.clone()).collect();
            (
                s.number_format,
                s.number_start,
                boxes.iter().flat_map(|d| d.lines.clone()).collect(),
                boxes.iter().flat_map(|d| d.fields.clone()).collect(),
            )
        })
        .collect()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn roman_front_matter_and_an_arabic_body_are_two_sections() {
    let w = render(
        "#set page(width: 300pt, height: 300pt, margin: 30pt, numbering: \"i\")\n\
         Front one.\n#pagebreak()\nFront two.\n\
         #set page(numbering: \"1\")\n#counter(page).update(1)\n\
         Body one.\n#pagebreak()\nBody two.\n",
    );
    assert_eq!(
        numbering(&w),
        [
            (
                Some("lowerRoman".into()),
                None,
                strings(&["i"]),
                strings(&["PAGE"])
            ),
            (
                Some("decimal".into()),
                Some(1),
                strings(&["1"]),
                strings(&["PAGE"])
            ),
        ]
    );
    let ends: Vec<(String, bool)> = w
        .body()
        .into_iter()
        .map(|p| (p.text, p.ends_section))
        .collect();
    assert_eq!(
        ends,
        [
            ("Front one.".into(), false),
            ("Front two.".into(), true),
            ("Body one.".into(), false),
            ("Body two.".into(), false),
        ]
    );
}

#[test]
fn chapter_numbers_restart_a_section_each_with_its_prefix_as_text() {
    let w = render(
        "#set page(width: 300pt, height: 300pt, margin: 30pt)\n\
         #let chapter(c) = (..n) => str(c) + \"-\" + str(n.pos().first())\n\
         #set page(numbering: chapter(1))\n\
         One A.\n#pagebreak()\nOne B.\n\
         #set page(numbering: chapter(2))\n#counter(page).update(1)\n\
         Two A.\n#pagebreak()\nTwo B.\n",
    );
    assert_eq!(
        numbering(&w),
        [
            (
                Some("decimal".into()),
                None,
                strings(&["1-1"]),
                strings(&["PAGE"])
            ),
            (
                Some("decimal".into()),
                Some(1),
                strings(&["2-1"]),
                strings(&["PAGE"])
            ),
        ]
    );
    assert!(!w.has("docx::page_number_frozen"), "{:?}", w.codes);
}

#[test]
fn an_appendix_number_is_its_letter_and_a_page_field() {
    let w = render(
        "#set page(width: 300pt, height: 300pt, margin: 30pt, \
         numbering: (..n) => \"A-\" + str(n.pos().first()))\n\
         Appendix page.\n#pagebreak()\nAnother.\n",
    );
    assert_eq!(
        numbering(&w),
        [(
            Some("decimal".into()),
            None,
            strings(&["A-1"]),
            strings(&["PAGE"])
        )]
    );
}

#[test]
fn a_number_word_cannot_count_stays_text_and_is_reported() {
    let w = render(
        "#set page(width: 300pt, height: 300pt, margin: 30pt, numbering: \"α\")\n\
         One.\n#pagebreak()\nTwo.\n",
    );
    let sections = numbering(&w);
    assert_eq!(sections.len(), 1);
    assert!(sections[0].3.is_empty(), "{sections:?}");
    assert!(w.has("docx::page_number_frozen"), "{:?}", w.codes);
}
