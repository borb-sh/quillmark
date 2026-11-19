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
