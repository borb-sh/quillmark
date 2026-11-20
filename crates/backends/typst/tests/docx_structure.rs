//! The structure a DOCX render writes: paragraphs, styles, sections and
//! anchors, read back from the package.

mod common;
mod word;

use word::render;

const PAGE: &str = "#set page(width: 300pt, height: 300pt, margin: 30pt)\n";

#[test]
fn a_drawn_rule_or_filled_block_is_reported_anchored_and_a_border_or_decoration_is_not() {
    for plate in [
        "= Experience\n#line(length: 100%)\nSome text.\n",
        "#block(fill: luma(230), inset: 6pt)[A callout.]\n",
    ] {
        let w = render(&format!("{PAGE}{plate}"));
        assert!(w.has("docx::place_anchored"), "{plate}: {:?}", w.codes);
    }
    let w = render(&format!(
        "{PAGE}#underline[under] #strike[struck] #highlight[lit]\n\
         #table(columns: 2, [a], [b])\n"
    ));
    assert!(!w.has("docx::place_anchored"), "{:?}", w.codes);
    assert!(!w.has("docx::shape_dropped"), "{:?}", w.codes);
}

#[test]
fn a_page_that_sets_one_of_foreground_and_background_keeps_its_margins() {
    let fg = "foreground: place(top + right, dx: -5pt, dy: 5pt)[DRAFT]";
    let bg = "background: rect(width: 100%, height: 100%, fill: luma(240))";
    for (sets, margin) in [
        (fg.to_string(), 400),
        (bg.to_string(), 400),
        (format!("{fg}, {bg}"), 1440),
    ] {
        let w = render(&format!(
            "#set page(width: 300pt, height: 300pt, margin: 20pt, {sets})\n\
             Body text.\n\n#pad(left: 20pt)[Indented text.]\n"
        ));
        let s = &w.sections()[0];
        assert_eq!((s.margin_top, s.margin_left), (margin, margin), "{sets}");
        assert_eq!(
            w.has("docx::margins_assumed"),
            margin == 1440,
            "{sets}: {:?}",
            w.codes
        );
        if margin == 400 {
            assert_eq!(w.par("Body text.").ind_left, None, "{sets}");
            assert_eq!(w.par("Indented text.").ind_left, Some(400), "{sets}");
        }
    }
}

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
    assert!(!w.has("docx::link_internal"), "{:?}", w.codes);
}

#[test]
fn the_footnote_rule_is_left_to_word_and_reaches_no_header() {
    let w = render(&format!(
        "{PAGE}First page text.#footnote[A note.]\n#pagebreak()\n\
         Second page text, no note.\n#pagebreak()\n\
         Third page text.#footnote[Another note.]\n"
    ));
    let sections = w.sections();
    assert_eq!(sections.len(), 1);
    assert!(sections[0].headers.is_empty(), "{:?}", sections[0].headers);
    assert!(sections[0].footers.is_empty(), "{:?}", sections[0].footers);
    assert!(!sections[0].title_page);
    assert!(w.body_drawings().is_empty());
    let notes = w.part_paragraphs("word/footnotes.xml");
    let texts: Vec<&str> = notes
        .iter()
        .map(|p| p.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    assert_eq!(texts, ["A note.", "Another note."]);
    assert!(!w.has("docx::shape_dropped"), "{:?}", w.codes);
}

/// The `w:gridCol` widths of the document's first table, in twips.
fn grid_columns(w: &word::Word) -> Vec<i64> {
    let xml = w.xml("word/document.xml");
    let grid = xml
        .split("<w:tblGrid>")
        .nth(1)
        .and_then(|g| g.split("</w:tblGrid>").next())
        .expect("a table grid");
    grid.split("w:w=\"")
        .skip(1)
        .filter_map(|v| v.split('"').next()?.parse().ok())
        .collect()
}

#[test]
fn an_em_column_with_no_block_around_it_resolves_at_the_text_size() {
    let w = render(&format!(
        "{PAGE}#set text(size: 10pt)\n\
         #table(columns: (1fr, 1em + 90pt), [a], [b])\n"
    ));
    let columns = grid_columns(&w);
    assert_eq!(columns.last(), Some(&2000), "{columns:?}");
}

#[test]
fn a_failed_update_leaves_the_word_export_of_the_served_document() {
    use quillmark_core::backend::Backend;
    use quillmark_core::types::{OutputFormat, RenderOptions};
    let quill = common::quill_with_plate(
        &common::yaml("main:\n  fields:\n    title: { type: text }\n"),
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n\
         #set page(width: 300pt, height: 300pt, margin: 30pt)\n\
         #if qm.data.title == \"boom\" { panic(\"boom\") }\n\
         #block[Title #qm.data.title]\n#block[Second block.]\n",
    );
    let mut session = quillmark_typst::TypstBackend::default()
        .open(
            &quill,
            &serde_json::json!({ "title": "ok" }),
            common::test_date(),
        )
        .expect("open");
    let docx = RenderOptions::default().with_output_format(OutputFormat::Docx);
    let before = session.render(&docx).expect("render");
    assert!(
        session
            .update_data(&serde_json::json!({ "title": "boom" }))
            .is_err()
    );
    let after = session.render(&docx).expect("render");
    let codes = |r: &quillmark_core::error::RenderResult| -> Vec<Option<String>> {
        r.warnings.iter().map(|w| w.code.clone()).collect()
    };
    assert_eq!(codes(&after), codes(&before));
    let texts = |r: &quillmark_core::error::RenderResult| {
        word::paragraphs(
            &String::from_utf8(
                word::unzip(&r.artifacts[0].bytes)
                    .into_iter()
                    .find(|(n, _)| n == "word/document.xml")
                    .expect("document")
                    .1,
            )
            .expect("utf-8"),
        )
        .into_iter()
        .map(|p| p.text)
        .collect::<Vec<_>>()
    };
    assert_eq!(texts(&after), ["Title ok", "Second block."]);
}

#[test]
fn a_picture_of_ink_past_the_page_is_clipped_to_the_page() {
    let w = render(&format!(
        "{PAGE}Text.\n#rotate(5deg, reflow: false, rect(width: 30in, height: 30in))\n"
    ));
    let pngs: Vec<(u32, u32)> = w
        .parts
        .iter()
        .filter(|(name, _)| name.starts_with("word/media/") && name.ends_with(".png"))
        .map(|(_, b)| {
            let be = |at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
            (be(16), be(20))
        })
        .collect();
    assert_eq!(pngs.len(), 1);
    let (width, height) = pngs[0];
    assert!(width <= 1250 && height <= 1250, "{width} x {height} px");
    let drawings = w.body_drawings();
    let picture = drawings
        .iter()
        .find(|(_, d)| d.picture)
        .expect("a picture")
        .1
        .clone();
    assert!(picture.width <= 300 * 12700 && picture.height <= 300 * 12700);
    assert!(w.has("docx::rasterized"));
    assert!(!w.has("docx::picture_failed"), "{:?}", w.codes);
}

/// A cell margin side's twips, from the cell's own margins or the table's.
fn cell_margin_top(xml: &str) -> i64 {
    let tag = |name: &str| {
        xml.split(&format!("<w:{name}>"))
            .nth(1)
            .and_then(|m| m.split(&format!("</w:{name}>")).next())
            .and_then(|m| m.split("<w:top w:w=\"").nth(1))
            .and_then(|v| v.split('"').next())
            .and_then(|v| v.parse().ok())
    };
    tag("tcMar").or_else(|| tag("tblCellMar")).unwrap_or(0)
}

#[test]
fn the_space_above_a_table_sets_its_first_line_on_typsts_baseline() {
    let plate = format!("{PAGE}Above.\n#table(columns: 1, inset: 8pt, [Cell])\n");
    let w = render(&plate);
    let quill = common::quill_with_plate(
        &common::yaml(""),
        &format!("#import \"@local/quillmark-helper:0.2.0\" as qm\n{plate}"),
    );
    let doc = quillmark_typst::testing::layout(&quill, &serde_json::json!({}), common::test_date())
        .expect("layout");
    let baseline = |text: &str| {
        doc.nodes
            .iter()
            .flat_map(|n| &n.lines)
            .find(|l| {
                l.items.iter().any(|r| {
                    matches!(&doc.item(*r).kind,
                        quillmark_docx::layout::ItemKind::Text(t) if t.text.trim() == text)
                })
            })
            .map(|l| l.baseline)
            .expect("a line")
    };
    let pars = w.body();
    let above = w.par("Above.");
    let cell = pars.iter().find(|p| p.text == "Cell").expect("the cell");
    let pt = |tw: Option<i64>| tw.unwrap_or(0) as f64 / 20.0;
    let above_baseline = pt(above.space_before) + 0.8 * pt(above.line);
    let row_top = pt(above.space_before) + pt(above.line) + pt(above.space_after);
    let margin = cell_margin_top(&w.xml("word/document.xml")) as f64 / 20.0;
    let word = row_top + margin + 0.8 * pt(cell.line) - above_baseline;
    let typst = baseline("Cell") - baseline("Above.");
    assert!(
        (word - typst).abs() < 0.2,
        "from the line above: Word {word} pt, Typst {typst} pt"
    );
}

#[test]
fn mirrored_content_is_a_picture() {
    let w = render(&format!(
        "{PAGE}Normal line.\n\n#scale(x: -100%)[Mirrored words here]\n\n\
         #scale(y: -100%)[Upside down words]\n"
    ));
    assert_eq!(
        w.texts()
            .iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>(),
        ["Normal line."]
    );
    let pictures = w
        .body_drawings()
        .into_iter()
        .filter(|(_, d)| d.picture)
        .count();
    assert_eq!(pictures, 2);
    assert!(w.has("docx::rasterized"));
}

#[test]
fn a_notice_raised_twice_alike_is_reported_once() {
    let w = render(&format!(
        "{PAGE}#set list(marker: box(width: 4pt, height: 4pt, fill: black))\n\
         - one\n- two\n\nBetween.\n\n- three\n"
    ));
    let replaced = w
        .codes
        .iter()
        .filter(|c| *c == "docx::list_marker_replaced")
        .count();
    assert_eq!(replaced, 1, "{:?}", w.codes);
}

#[test]
fn a_link_to_a_place_in_the_document_keeps_its_text_and_is_reported() {
    let w = render(&format!(
        "{PAGE}= Scope <scope>\n\nAs #link(<scope>)[the scope] says, and #link(\"https://example.com\")[the site].\n"
    ));
    assert!(w.has("docx::link_internal"), "{:?}", w.codes);
    assert!(
        w.texts()
            .iter()
            .any(|t| t == "As the scope says, and the site."),
        "{:?}",
        w.texts()
    );
    let body = w.xml("word/document.xml");
    assert_eq!(body.matches("<w:hyperlink").count(), 1, "{body}");
}

#[test]
fn a_link_that_runs_script_keeps_its_text_and_is_reported() {
    let w = render(&format!(
        "{PAGE}Press #link(\" JavaScript:alert(1)\")[here] or #link(\"https://example.com\")[there].\n"
    ));
    assert!(w.has("docx::link_script"), "{:?}", w.codes);
    assert_eq!(w.texts(), ["Press here or there."]);
    let body = w.xml("word/document.xml");
    assert_eq!(body.matches("<w:hyperlink").count(), 1, "{body}");
    let rels = w.xml("word/_rels/document.xml.rels");
    assert!(!rels.to_ascii_lowercase().contains("javascript"), "{rels}");
}

#[test]
fn a_term_and_its_description_are_one_paragraph_with_the_term_kept() {
    let w = render(&format!(
        "{PAGE}/ Alpha: The first description.\n/ Beta: The second description.\n"
    ));
    let items: Vec<_> = w
        .body()
        .into_iter()
        .filter(|p| p.text.contains("description"))
        .collect();
    assert_eq!(items.len(), 2, "{:?}", w.texts());
    for (item, term) in items.iter().zip(["Alpha", "Beta"]) {
        assert!(item.text.starts_with(term), "{:?}", item.text);
        assert!(
            item.runs.iter().any(|r| r.text == term && r.bold),
            "{:?}",
            item.runs
        );
    }
}

#[test]
fn a_raw_block_is_a_paragraph_per_line_with_proofing_off() {
    let w = render(&format!(
        "{PAGE}Before.\n\n```\nfirst line\nsecond line\n```\n"
    ));
    for text in ["first line", "second line"] {
        let p = w.par(text);
        assert!(p.runs.iter().all(|r| r.no_proof), "{:?}", p.runs);
    }
    assert!(w.par("Before.").runs.iter().all(|r| !r.no_proof));
}

#[test]
fn inline_raw_text_stays_in_its_paragraph() {
    let w = render(&format!("{PAGE}Run `code` inline.\n"));
    assert_eq!(w.texts(), ["Run code inline."]);
}

#[test]
fn a_block_quote_and_a_caption_take_words_own_styles() {
    let w = render(&format!(
        "{PAGE}#quote(block: true, attribution: [Someone])[Quoted words.]\n\n\
         #figure(block[The body.], caption: [The caption.])\n\nAfter.\n"
    ));
    assert_eq!(w.par("Quoted words.").style.as_deref(), Some("Quote"));
    assert_eq!(w.par("\u{2014} Someone").style.as_deref(), Some("Quote"));
    let caption = w.par("Figure\u{a0}1: The caption.");
    assert_eq!(caption.style.as_deref(), Some("Caption"));
    assert!(w.par("The body.").keep_next);
    assert_eq!(w.par("After.").style, None);
    let styles = w.xml("word/styles.xml");
    assert!(styles.contains("w:styleId=\"Quote\""), "{styles}");
    assert!(
        styles.contains("w:styleId=\"Caption\"><w:name w:val=\"caption\"/>"),
        "{styles}"
    );
}

/// The memo's heading block as plain Typst: a date line, then grids
/// for MEMORANDUM FOR (its recipients in a nested grid), FROM and SUBJECT,
/// one blank line apart, then the body.
const HEADING_STACK: &str = "#set page(width: 612pt, height: 792pt, margin: 72pt)\n\
     #set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
     #set par(leading: 0.5em, spacing: 0pt)\n\
     #set block(spacing: 0pt)\n\
     Dated line\n\
     #v(13.95pt)\n\
     #grid(columns: (auto, auto, 1fr), [MEMORANDUM FOR], \"  \", grid([123 OG/CC], [123 MXG/CC]))\n\
     #v(13.95pt)\n\
     #grid(columns: (auto, auto, 1fr), [FROM:], \"  \", [123 CS/SCX])\n\
     #v(13.95pt)\n\
     #grid(columns: (auto, auto, 1fr), [SUBJECT:], \"  \", [Annual Review])\n\
     #v(13.95pt)\n\
     Body text.\n";

/// Word's distance between two lines, by the export's model, against
/// Typst's.
fn gaps(w: &word::Word, quill: &quillmark_core::quill::Quill, pairs: &[(&str, &str)]) {
    for (a, b) in pairs {
        let word = w.word_baseline(b) - w.word_baseline(a);
        let typst = word::typst_baseline(quill, b) - word::typst_baseline(quill, a);
        assert!(
            (word - typst).abs() < 0.25,
            "{a:?} to {b:?}: Word {word:.2} pt, Typst {typst:.2} pt"
        );
    }
}

#[test]
fn two_grids_a_gap_apart_keep_the_gap_and_stay_two_tables() {
    let plate = format!(
        "{PAGE}#grid(columns: (auto, 1fr), [One], [Left])\n#v(20pt)\n\
         #grid(columns: (auto, 1fr), [Two], [Right])\nAfter.\n"
    );
    let w = render(&plate);
    let xml = w.xml("word/document.xml");
    assert_eq!(xml.matches("<w:tbl>").count(), 2);
    assert!(!xml.contains("</w:tbl><w:tbl>"), "the tables touch");
    let quill = common::quill_with_plate(
        &common::yaml(""),
        &format!("#import \"@local/quillmark-helper:0.2.0\" as qm\n{plate}"),
    );
    gaps(&w, &quill, &[("One", "Two"), ("Two", "After.")]);
}

#[test]
fn the_memo_heading_stack_keeps_each_blank_line() {
    let w = word::render_hosted(HEADING_STACK);
    let xml = w.xml("word/document.xml");
    assert!(!xml.contains("</w:tbl><w:tbl>"), "the tables touch");
    gaps(
        &w,
        &word::hosted(HEADING_STACK),
        &[
            ("Dated line", "MEMORANDUM FOR"),
            ("MEMORANDUM FOR", "123 OG/CC"),
            ("123 MXG/CC", "FROM:"),
            ("FROM:", "SUBJECT:"),
            ("SUBJECT:", "Body text."),
        ],
    );
}

/// An indorsement heading as plain Typst: a block that keeps two grids a
/// blank line apart, set a gap below the line before it.
const INDORSEMENT: &str = "#set page(width: 612pt, height: 792pt, margin: 72pt)\n\
     #set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
     #set par(leading: 0.5em, spacing: 0pt)\n\
     #set block(spacing: 0pt)\n\
     Last line.\n\
     #v(40pt)\n\
     #block(breakable: false, sticky: true)[\
     #grid(columns: (auto, 1fr), [1st Ind, 123 OG/CC], align(right)[14 November 2026])\n\
     #v(13.95pt)\n\
     #grid(columns: (auto, auto, 1fr), [MEMORANDUM FOR], \"  \", [123 CS/SCX])]\n\
     #v(13.95pt)\n\
     Body text.\n";

#[test]
fn two_grids_in_a_block_keep_the_gap_above_the_block_and_between_them() {
    let w = word::render_hosted(INDORSEMENT);
    gaps(
        &w,
        &word::hosted(INDORSEMENT),
        &[
            ("Last line.", "1st Ind, 123 OG/CC"),
            ("1st Ind, 123 OG/CC", "MEMORANDUM FOR"),
            ("MEMORANDUM FOR", "Body text."),
        ],
    );
}

/// The `w:gridCol` widths of the table holding `text`, and its tab stops
/// and hanging indent when `text` is a tabbed paragraph instead.
fn grid_of(xml: &str, text: &str) -> Vec<i64> {
    let at = xml
        .find(&format!(">{text}<"))
        .unwrap_or_else(|| panic!("no {text:?}"));
    let table = xml[..at].rfind("<w:tblGrid>").expect("a table");
    xml[table..at]
        .split("<w:gridCol w:w=\"")
        .skip(1)
        .map(|g| g.split('"').next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn a_content_sized_column_is_as_wide_as_its_unkerned_text() {
    let w = word::render_hosted(HEADING_STACK);
    let xml = w.xml("word/document.xml");
    // Times' own advances at 12 pt, each rounded up to a whole twip: "FROM:"
    // 750, "SUBJECT:" 1085. Typst kerns them to 36.80 and 52.93 pt.
    for (label, unkerned) in [("FROM:", 750), ("SUBJECT:", 1085)] {
        let grid = grid_of(&xml, label);
        assert!(grid[0] >= unkerned, "{label} column {grid:?}");
        assert_eq!(
            grid.iter().sum::<i64>(),
            9360,
            "{label} table width {grid:?}"
        );
    }
}

#[test]
fn a_content_sized_column_with_room_past_its_text_keeps_its_width() {
    let w = word::render_hosted(&format!(
        "{PAGE}#set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
         #grid(columns: 2, column-gutter: 1fr, [123 OG/CC], [])\n"
    ));
    let grid = grid_of(&w.xml("word/document.xml"), "123 OG/CC");
    assert_eq!(
        grid[0], 4800,
        "the column and the gutter it holds: {grid:?}"
    );
}

#[test]
fn a_tabbed_label_has_room_for_its_unkerned_text() {
    let plate = "#set page(width: 612pt, height: 792pt, margin: 72pt)\n\
         #set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
         #qm.hints.hint(\"layout\", form: \"tabbed\")[#grid(columns: (auto, auto, 1fr), \
         [SUBJECT:], [], [Annual Review of Every Office of Record in the Wing, with a \
         subject long enough to wrap under itself])]\n";
    let w = word::render_hosted(plate);
    let xml = w.xml("word/document.xml");
    let stops: Vec<i64> = xml
        .split("<w:tab w:val=\"left\" w:pos=\"")
        .skip(1)
        .map(|g| g.split('"').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(stops.len(), 2, "{xml}");
    assert!(stops[0] >= 1085, "{stops:?}");
    assert!(stops[1] > stops[0], "two stops share a position: {stops:?}");
    let hanging = xml
        .split("w:hanging=\"")
        .nth(1)
        .and_then(|v| v.split('"').next())
        .and_then(|v| v.parse::<i64>().ok());
    assert_eq!(hanging, Some(stops[1]));
}

fn starting(w: &word::Word, prefix: &str) -> word::Par {
    let all = w.body();
    all.iter()
        .find(|p| p.text.starts_with(prefix))
        .cloned()
        .unwrap_or_else(|| panic!("no paragraph {prefix:?} in {:?}", word::texts(&all)))
}

#[test]
fn a_superscript_joins_its_line_and_is_set_at_the_lines_size() {
    let w = render(&format!(
        "{PAGE}The 1#super[st] Indorsement sits in a paragraph long enough to wrap onto a second line.\n\n\
         A plain paragraph that is also long enough to wrap onto a second line.\n"
    ));
    let raised = starting(&w, "The 1st Indorsement");
    let plain = starting(&w, "A plain paragraph");
    assert_eq!(raised.line, plain.line, "one pitch for both");
    let st = raised.runs.iter().find(|r| r.text == "st").expect("a run");
    assert_eq!(st.vert.as_deref(), Some("superscript"));
    assert_eq!(st.size, raised.runs[0].size, "Word shrinks it itself");
}

#[test]
fn inline_math_is_a_picture_on_its_line_and_the_next_paragraph_keeps_its_pitch() {
    let w = render(&format!(
        "{PAGE}Inline $x_i + y^2$ in a line.\n\nThe next paragraph.\n"
    ));
    let math = w.par("Inline  in a line.");
    let next = w.par("The next paragraph.");
    assert_eq!(math.line, next.line);
    assert!(math.line.is_some_and(|l| l > 0), "{:?}", math.line);
    let [picture] = &math.drawings[..] else {
        panic!("{:?}", math.drawings)
    };
    assert!(picture.picture && !picture.anchored);
    assert_eq!(picture.descr.as_deref(), Some("x_i + y^2"));
    assert!(
        picture.position.is_some_and(|p| p < 0),
        "lowered to the baseline"
    );
    assert!(w.has("docx::rasterized"), "{:?}", w.codes);
    assert!(!w.has("docx::equation_text"), "{:?}", w.codes);
}

#[test]
fn display_math_is_one_centred_picture() {
    let w = render(&format!(
        "{PAGE}Before.\n\n$ x = (a + b) / 2 $\n\nAfter the display.\n"
    ));
    assert_eq!(
        w.texts()
            .iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>(),
        ["Before.", "After the display."]
    );
    let display = w
        .body()
        .into_iter()
        .find(|p| !p.drawings.is_empty())
        .expect("a picture paragraph");
    assert_eq!(display.jc.as_deref(), Some("center"));
    assert_eq!(
        display.drawings[0].descr.as_deref(),
        Some("x = (a + b) / 2")
    );
}

#[test]
fn an_equation_that_wraps_keeps_its_text_and_is_reported() {
    let w = render(&format!(
        "{PAGE}Before $a^2 + b^2 + c^2 + d^2 + e^2 + f^2 + g^2 + h^2 + i^2 + j^2 + k^2 + l^2$ after.\n\n\
         The next paragraph, long enough that it wraps onto a second line too.\n"
    ));
    assert!(w.has("docx::equation_text"), "{:?}", w.codes);
    let math = starting(&w, "Before");
    assert!(math.drawings.is_empty());
    let (line, next) = (
        math.line.unwrap(),
        starting(&w, "The next paragraph").line.unwrap(),
    );
    assert!((line - next).abs() <= 2, "{line} against {next}");
}

#[test]
fn a_one_line_paragraph_takes_the_indent_kind_of_its_wrapped_sibling() {
    let w = render(&format!(
        "{PAGE}#h(2em)One line gap.\n\n\
         #h(2em)A gap that opens a paragraph long enough to wrap onto a second line.\n\n\
         #pad(left: 2em)[One line padded.]\n\n\
         #pad(left: 2em)[A padded paragraph that is long enough to wrap onto a second line.]\n\n\
         #set par(first-line-indent: 1.5em)\nPlain first.\n\nIndented one line.\n\n\
         Indented and long enough that it wraps onto a second line as well.\n"
    ));
    let indents = |prefix: &str| {
        let p = starting(&w, prefix);
        (p.ind_left, p.ind_first)
    };
    assert_eq!(indents("One line gap"), (None, Some(440)));
    assert_eq!(indents("A gap that opens"), (None, Some(440)));
    assert_eq!(indents("One line padded"), (Some(440), None));
    assert_eq!(indents("A padded paragraph"), (Some(440), None));
    assert_eq!(indents("Indented one line"), (None, Some(330)));
    assert_eq!(indents("Indented and long"), (None, Some(330)));
}

#[test]
fn a_heading_shown_through_a_centred_block_is_centred_not_indented() {
    let w = render(&format!(
        "{PAGE}#show heading: it => {{\n  set align(center)\n  \
         block(width: 100%, text(weight: \"bold\", it.body))\n}}\n\
         = Summary\n\nA body paragraph.\n"
    ));
    let heading = w.par("Summary");
    assert_eq!(heading.style.as_deref(), Some("Heading1"));
    assert_eq!(heading.jc.as_deref(), Some("center"));
    assert_eq!(heading.ind_left, None);
    assert_eq!(w.par("A body paragraph.").jc, None);
}

/// The first of `texts` Typst sets on its second page.
fn first_on_second_page(quill: &quillmark_core::quill::Quill, texts: &[String]) -> String {
    texts
        .iter()
        .find(|t| word::typst_line(quill, t).0 == 1)
        .expect("a second page")
        .clone()
}

#[test]
fn a_numbered_paragraph_at_a_page_top_keeps_its_siblings_space_before() {
    let body: String = (1..=16)
        .map(|i| {
            format!("#hints.hint(\"label\", level: 1, value: {i})[{i}.#h(4pt)]Paragraph {i}.\n\n#v(8pt)\n\n")
        })
        .collect();
    let quill = word::plain(&format!(
        "#let hints = qm.hints\n{PAGE}#set par(spacing: 6pt)\n\
         #hints.hint(\"list\", v: 1, levels: ((format: \"decimal\", suffix: \". \"),))[\n{body}]\n"
    ));
    let texts: Vec<String> = (1..=16).map(|i| format!("Paragraph {i}.")).collect();
    let top = first_on_second_page(&quill, &texts);
    let w = word::render_quill(&quill);
    let mid = w.par("Paragraph 3.").space_before;
    assert!(mid.is_some_and(|b| b > 120), "{mid:?}");
    assert_eq!(w.par(&top).space_before, mid, "{top}");
}

#[test]
fn a_list_item_at_a_page_top_keeps_its_siblings_space_before() {
    let items: String = (1..=30).map(|i| format!("- Item {i}\n")).collect();
    let quill = word::plain(&format!("{PAGE}Before the list.\n\n{items}"));
    let texts: Vec<String> = (1..=30).map(|i| format!("Item {i}")).collect();
    let top = first_on_second_page(&quill, &texts);
    let w = word::render_quill(&quill);
    let mid = w.par("Item 3").space_before;
    assert_eq!(w.par(&top).space_before, mid, "{top}");
}

#[test]
fn widow_and_orphan_control_is_on_for_every_paragraph() {
    let w = render(&format!("{PAGE}A paragraph.\n"));
    let xml = w.xml("word/styles.xml");
    let doc = roxmltree::Document::parse(&xml).expect("well-formed");
    let control = doc
        .descendants()
        .find(|n| n.tag_name().name() == "pPrDefault")
        .and_then(|d| {
            d.descendants()
                .find(|n| n.tag_name().name() == "widowControl")
        })
        .expect("a default widowControl");
    let main = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    assert!(
        control
            .attribute((main, "val"))
            .is_none_or(|v| v != "0" && v != "false"),
        "{xml}"
    );
    assert!(!w.xml("word/document.xml").contains("w:widowControl"));
}

#[test]
fn a_place_after_a_table_that_fills_its_page_adds_no_page_break() {
    let w = render(
        "#set page(width: 300pt, height: 200pt, margin: 36pt)\n\
         #table(columns: 1fr, rows: 110pt, [A tall cell.])\n\
         #place(top + right)[Placed.]\n\
         Next paragraph after the place, long enough to need a few lines so that it cannot \
         fit at the foot of the first page.\n\nSecond paragraph.\n",
    );
    assert!(
        w.body_drawings()
            .iter()
            .any(|(_, d)| d.anchored && d.text == "Placed.")
    );
    let next = starting(&w, "Next paragraph after the place");
    assert!(!next.page_break_before);
    assert!(w.body().iter().all(|p| !p.page_break_before));
}

/// The text a header or footer part shows, in its paragraphs and text
/// boxes.
fn chrome_text(w: &word::Word, part: &str) -> String {
    w.part_paragraphs(part)
        .iter()
        .map(|p| {
            p.text.clone()
                + &p.drawings
                    .iter()
                    .map(|d| d.text.clone())
                    .collect::<String>()
        })
        .collect()
}

#[test]
fn a_section_with_no_footer_writes_an_empty_one_rather_than_inherit() {
    let w = render(&format!(
        "{PAGE}#set page(footer: [Foot A])\nFirst page.\n#pagebreak()\nSecond page.\n\
         #set page(footer: none)\nThird page, with no footer.\n"
    ));
    let sections = w.sections();
    assert_eq!(sections.len(), 2);
    let text = |part: &str| chrome_text(&w, part);
    let first: Vec<String> = sections[0].footers.iter().map(|(_, p)| text(p)).collect();
    assert_eq!(first, ["Foot A"]);
    let [(kind, part)] = &sections[1].footers[..] else {
        panic!("{:?}", sections[1].footers)
    };
    assert_eq!(kind, "default");
    assert_eq!(text(part), "");
    assert_eq!(sections[1].start.as_deref(), Some("nextPage"));
}

#[test]
fn a_running_header_that_changes_mid_flow_breaks_the_section_on_the_same_page() {
    let w = render(
        "#set page(width: 300pt, height: 200pt, margin: 36pt, header: context {\n  \
         let hs = query(heading.where(level: 1).before(here()))\n  \
         if hs.len() > 0 [Running #hs.last().body] else [Front]\n})\n\
         = Alpha\n#lorem(70)\n\n= Beta\n#lorem(60)\n\n= Gamma\nShort.\n",
    );
    let sections = w.sections();
    assert!(sections.len() >= 2, "{} sections", sections.len());
    for s in &sections[1..] {
        assert_eq!(s.start.as_deref(), Some("continuous"));
    }
    assert!(w.body().iter().all(|p| !p.page_break_before));
    let headers: Vec<String> = sections
        .iter()
        .flat_map(|s| &s.headers)
        .map(|(_, p)| chrome_text(&w, p))
        .collect();
    assert!(headers.iter().any(|h| h == "Running Beta"), "{headers:?}");
}

#[test]
fn a_hint_on_what_it_cannot_apply_to_is_reported() {
    let misused = |body: &str| {
        let w = render(&format!("#let hints = qm.hints\n{PAGE}{body}\n"));
        w.has("docx::hint_unchecked")
    };
    assert!(misused(
        "#hints.hint(\"label\", level: 1, value: 1)[1.#h(4pt)]A label outside any list."
    ));
    assert!(misused(
        "#hints.hint(\"layout\", form: \"tabbed\")[A paragraph, not a grid.]"
    ));
    assert!(misused(
        "#hints.hint(\"chrome\")[A paragraph, not a place.]"
    ));
    assert!(!misused(
        "#hints.hint(\"layout\", form: \"tabbed\")[#grid(columns: (auto, 1fr), [FROM:], [Here])]\n\
         #hints.hint(\"chrome\")[#place(top + right)[Mark]]\n\
         #hints.hint(\"list\", v: 1, levels: ((format: \"decimal\", suffix: \". \"),))[\n\
         #hints.hint(\"label\", level: 1, value: 1)[1.#h(4pt)]In a list.\n]"
    ));
}

#[test]
fn form_and_signature_fields_are_reported_as_left_out() {
    let w = render(&format!(
        "{PAGE}Name: #qm.form-field(\"name\", type: \"text\", value: \"Ann\")\n\n\
         Signed: #qm.form-field(\"sig\", type: \"signature\")\n"
    ));
    assert!(w.has("docx::form_field_declined"), "{:?}", w.codes);
    assert!(!render(&format!("{PAGE}No fields.\n")).has("docx::form_field_declined"));
}

#[test]
fn columns_in_the_flow_are_reported_as_one_column() {
    let w = render(&format!(
        "{PAGE}Before.\n\n#columns(2)[Left column text.\n#colbreak()\nRight column text.]\n"
    ));
    assert!(w.has("docx::columns_flattened"), "{:?}", w.codes);
    assert_eq!(w.sections().len(), 1);
    assert!(!render(&format!("{PAGE}#columns(1)[One.]\n")).has("docx::columns_flattened"));
}

#[test]
fn right_to_left_text_is_reported() {
    let w = word::render_hosted(&format!(
        "{PAGE}Left to right.\n\n#text(font: \"Liberation Mono\", lang: \"he\")[\u{5e9}\u{5dc}\u{5d5}\u{5dd}]\n"
    ));
    assert!(w.texts().iter().any(|t| t.contains('\u{5e9}')));
    assert!(w.has("docx::direction_dropped"), "{:?}", w.codes);
    assert!(
        !word::render_hosted(&format!("{PAGE}Left to right.\n")).has("docx::direction_dropped")
    );
}

#[test]
fn a_break_to_an_odd_or_even_page_starts_a_section_of_that_parity() {
    let w = render(&format!(
        "{PAGE}One.\n#pagebreak(to: \"odd\")\nThree.\n#pagebreak(to: \"even\")\n\
         #table(columns: 2)[A][B]\n"
    ));
    let starts: Vec<Option<String>> = w.sections().into_iter().map(|s| s.start).collect();
    assert_eq!(
        starts,
        [
            Some("nextPage".to_string()),
            Some("oddPage".to_string()),
            Some("evenPage".to_string())
        ]
    );
    assert!(!w.xml("word/document.xml").contains("pageBreakBefore"));
    assert_eq!(w.par("Three.").space_before, Some(0));
}

/// The paragraph before the first one whose text is `text`.
fn before(w: &word::Word, text: &str) -> word::Par {
    let body = w.body();
    let at = body
        .iter()
        .position(|p| p.text == text)
        .unwrap_or_else(|| panic!("{text:?} in {:?}", w.texts()));
    body[at.checked_sub(1).expect("a paragraph before it")].clone()
}

#[test]
fn a_keep_or_a_tables_gap_in_a_list_or_contents_goes_to_the_block_right_before_it() {
    let w = render(&format!(
        "{PAGE}Before.\n\n- one\n- #qm.hints.keep-with-next[two]\n- three\n\n  \
         #table(columns: 1, [T1])\n  #table(columns: 1, [T2])\n- four\n"
    ));
    assert!(w.par("two").keep_next);
    assert!(!w.par("Before.").keep_next);
    assert_eq!(w.par("Before.").space_after, None);
    assert!(w.par("three").space_after.is_some(), "the gap above T1");
    let spacer = before(&w, "T2");
    assert!(spacer.text.is_empty() && !spacer.in_table, "{spacer:?}");
    assert!(!w.xml("word/document.xml").contains("</w:tbl><w:tbl>"));

    let w = render(&format!(
        "{PAGE}#show outline: it => {{ it; table(columns: 1, [T]) }}\n\
         Before.\n#outline()\n= A\n"
    ));
    assert_eq!(w.par("Before.").space_after, None);
    let spacer = before(&w, "T");
    assert!(
        spacer.text.is_empty() && spacer.line.is_some(),
        "{spacer:?}"
    );
}

#[test]
fn a_keep_opening_a_footnote_cell_or_text_box_keeps_nothing_outside_it() {
    let keep = "#qm.hints.hint-point(\"keep\")";
    let w = render(&format!(
        "{PAGE}First.\n\nSecond.#footnote[{keep}The note.]\n\n\
         #table(columns: 1, [A], [{keep}Cell text.])\n\n\
         Third #place(top + right)[{keep}P]\n\nNext.\n"
    ));
    let kept: Vec<String> = w
        .body()
        .into_iter()
        .filter(|p| p.keep_next)
        .map(|p| p.text)
        .collect();
    assert!(kept.is_empty(), "{kept:?}");
}
