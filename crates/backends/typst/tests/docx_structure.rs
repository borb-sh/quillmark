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

/// Each body paragraph's text, the page it starts, and the text of the
/// objects anchored to it.
fn anchors(w: &word::Word) -> Vec<(String, usize, Vec<String>)> {
    w.body()
        .into_iter()
        .map(|p| {
            (
                p.text,
                p.page,
                p.drawings.into_iter().map(|d| d.text).collect(),
            )
        })
        .collect()
}

fn anchored(text: &str, page: usize, boxes: &[&str]) -> (String, usize, Vec<String>) {
    (text.into(), page, strings(boxes))
}

#[test]
fn a_fixed_page_alone_is_one_anchor_paragraph_in_paint_order() {
    let w = render(&format!(
        "{PAGE}#place(top + left, dx: 20pt, dy: 30pt)[Cover title]\n\
         #place(top + left, dx: 25pt, dy: 32pt)[Over it]\n"
    ));
    assert_eq!(anchors(&w), [anchored("", 0, &["Cover title", "Over it"])]);
    let par = &w.body()[0];
    assert_eq!(par.line, Some(20));
    let z: Vec<i64> = par.drawings.iter().map(|d| d.z.expect("z")).collect();
    assert!(z[0] < z[1], "{z:?}");
    for d in &par.drawings {
        assert!(d.anchored);
        assert_eq!(d.y.as_ref().map(|y| y.0.as_str()), Some("page"));
    }
}

#[test]
fn two_fixed_pages_in_a_row_each_keep_their_objects() {
    let w = render(&format!(
        "{PAGE}#place(top + left, dx: 20pt, dy: 30pt)[First page]\n\
         #pagebreak()\n\
         #place(top + left, dx: 20pt, dy: 30pt)[Second page]\n"
    ));
    assert_eq!(
        anchors(&w),
        [
            anchored("", 0, &["First page"]),
            anchored("", 1, &["Second page"]),
        ]
    );
    assert!(w.body()[1].page_break_before);
}

#[test]
fn a_fixed_page_between_flowing_pages_holds_its_own_objects() {
    let w = render(&format!(
        "{PAGE}Before.\n#pagebreak()\n\
         #place(top + left, dx: 20pt, dy: 30pt)[Fixed]\n\
         #pagebreak()\nAfter.\n"
    ));
    assert_eq!(
        anchors(&w),
        [
            anchored("Before.", 0, &[]),
            anchored("", 1, &["Fixed"]),
            anchored("After.", 2, &[]),
        ]
    );
}

#[test]
fn a_fixed_cover_ends_its_own_section_before_new_margins() {
    let w = render(&format!(
        "{PAGE}#place(top + left, dx: 20pt, dy: 30pt)[Cover]\n\
         #set page(margin: 60pt)\nBody.\n"
    ));
    assert_eq!(
        anchors(&w),
        [anchored("", 0, &["Cover"]), anchored("Body.", 1, &[])]
    );
    assert!(w.body()[0].ends_section);
    let margins: Vec<i64> = w.sections().iter().map(|s| s.margin_top).collect();
    assert_eq!(margins, [600, 1200]);
}

#[test]
fn a_custom_outline_entry_leads_to_its_prefixed_number_and_is_locked() {
    let w = render(
        "#set page(width: 300pt, height: 300pt, margin: 30pt, \
         numbering: (..n) => \"1-\" + str(n.pos().first()))\n\
         #show outline.entry: it => block(link(it.element.location(), \
         [#it.body() #box(width: 1fr, repeat[.]) #it.page()]))\n\
         #outline()\n= Intro\nText.\n#pagebreak()\n= Second\n",
    );
    let entries: Vec<(String, Option<String>)> = w
        .body()
        .into_iter()
        .filter(|p| p.style.as_deref().is_some_and(|s| s.starts_with("TOC")))
        .map(|p| (p.text, p.style))
        .collect();
    assert_eq!(
        entries,
        [
            ("Intro\t1-1".to_string(), Some("TOC1".to_string())),
            ("Second\t1-2".to_string(), Some("TOC1".to_string())),
        ]
    );
    let xml = w.xml("word/document.xml");
    assert!(xml.contains("w:fldLock=\"1\""));
    assert!(w.has("docx::toc_frozen"), "{:?}", w.codes);
}
