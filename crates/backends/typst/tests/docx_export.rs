//! A DOCX render end to end: one package that carries the document's text.

use quillmark_core::backend::Backend;
use quillmark_core::types::{OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;
use serde_json::json;

mod common;
mod word;

/// Each part of the one artifact `plate` renders to as DOCX.
fn docx_parts(plate: &str, data: serde_json::Value) -> Vec<(String, Vec<u8>)> {
    let quill = common::quill_with_plate(
        &common::yaml(
            "schema:\n  main:\n    fields:\n      prose: { type: richtext, shape: blocks }\n",
        ),
        plate,
    );
    let session = TypstBackend::default()
        .open(&quill, &data, common::test_date())
        .expect("open");
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Docx))
        .expect("render");
    assert_eq!(result.artifacts.len(), 1, "a DOCX render is one artifact");
    assert_eq!(result.artifacts[0].output_format, OutputFormat::Docx);
    word::unzip(&result.artifacts[0].bytes)
}

fn document_text(parts: &[(String, Vec<u8>)]) -> String {
    let (_, xml) = parts
        .iter()
        .find(|(n, _)| n == "word/document.xml")
        .expect("main part");
    let xml = String::from_utf8(xml.clone()).unwrap();
    let doc = roxmltree::Document::parse(&xml).expect("well-formed");
    doc.descendants()
        .filter(|n| n.tag_name().name() == "t")
        .filter_map(|n| n.text())
        .collect()
}

#[test]
fn a_docx_render_carries_the_documents_text() {
    let parts = docx_parts(
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n#set page(width: 300pt, height: 200pt)\n#qm.data.prose\n",
        json!({ "prose": common::content("Hello *there* & welcome.") }),
    );
    assert!(document_text(&parts).contains("Hello there & welcome."));
}

fn part(parts: &[(String, Vec<u8>)], name: &str) -> String {
    let (_, xml) = parts
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name}"));
    String::from_utf8(xml.clone()).unwrap()
}

fn render(plate: &str) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let plate = format!("#import \"@local/quillmark-helper:0.2.0\" as qm\n{plate}");
    let quill = common::quill_with_plate(&common::yaml(""), &plate);
    let session = TypstBackend::default()
        .open(&quill, &json!({}), common::test_date())
        .expect("open");
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Docx))
        .expect("render");
    let codes = result
        .warnings
        .iter()
        .filter_map(|w| w.code.clone())
        .collect();
    (word::unzip(&result.artifacts[0].bytes), codes)
}

/// The `w:p` elements of `xml` with the text each holds.
fn paragraphs(xml: &str) -> Vec<(String, roxmltree::NodeId)> {
    let doc = roxmltree::Document::parse(xml).expect("well-formed");
    doc.descendants()
        .filter(|n| {
            n.tag_name().name() == "p" && n.ancestors().skip(1).all(|a| a.tag_name().name() != "p")
        })
        .map(|p| {
            let text: String = p
                .descendants()
                .filter(|n| n.tag_name().name() == "t")
                .filter_map(|n| n.text())
                .collect();
            (text, p.id())
        })
        .collect()
}

const PAGE: &str =
    "#set page(width: 300pt, height: 300pt, margin: 30pt, numbering: \"1\", header: [Head])\n";

#[test]
fn a_wrapped_paragraph_is_one_word_paragraph_and_a_heading_takes_its_style() {
    let (parts, codes) = render(&format!(
        "{PAGE}= Title\nA paragraph with words enough that it wraps onto a second line of text \
         and then a third line here.\n\nLine one \\\nline two\n"
    ));
    let xml = part(&parts, "word/document.xml");
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(texts.contains(&"Title".to_string()), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("A paragraph with") && t.ends_with("third line here.")),
        "{texts:?}"
    );
    assert!(texts.contains(&"Line oneline two".to_string()), "{texts:?}");
    assert!(xml.contains("<w:pStyle w:val=\"Heading1\"/>"));
    assert_eq!(xml.matches("<w:br/>").count(), 1);
    assert!(part(&parts, "word/styles.xml").contains("w:styleId=\"Heading1\""));
    assert!(part(&parts, "word/fontTable.xml").contains("<w:font w:name="));
    assert!(!codes.iter().any(|c| c == "docx::ink_dropped"), "{codes:?}");
}

#[test]
fn lists_tables_and_footnotes_lower_to_their_word_structures() {
    let (parts, codes) = render(&format!(
        "{PAGE}- one\n- two\n\n+ first\n+ second\n\n\
         #table(columns: 2, table.header[H1][H2], [a], [b])\nNoted#footnote[The note.].\n"
    ));
    let xml = part(&parts, "word/document.xml");
    assert_eq!(xml.matches("<w:numPr>").count(), 4);
    let numbering = part(&parts, "word/numbering.xml");
    assert!(numbering.contains("w:val=\"bullet\""));
    assert!(numbering.contains("w:val=\"decimal\"") && numbering.contains("w:val=\"%1.\""));
    assert_eq!(xml.matches("<w:tc>").count(), 4);
    assert_eq!(xml.matches("<w:tblHeader/>").count(), 1);
    assert_eq!(xml.matches("<w:footnoteReference").count(), 1);
    let notes = part(&parts, "word/footnotes.xml");
    let entry: Vec<String> = paragraphs(&notes).into_iter().map(|p| p.0).collect();
    assert!(entry.iter().any(|t| t == " The note."), "{entry:?}");
    assert!(!codes.iter().any(|c| c == "docx::ink_dropped"), "{codes:?}");
}

#[test]
fn page_setup_chrome_and_numbers_become_section_properties_and_fields() {
    let (parts, codes) = render(&format!("{PAGE}one\n#pagebreak()\ntwo\n"));
    let xml = part(&parts, "word/document.xml");
    assert!(xml.contains("w:w=\"6000\" w:h=\"6000\""), "page size");
    assert!(xml.contains("w:top=\"600\""), "margins");
    assert!(xml.contains("<w:pageBreakBefore/>"));
    assert!(xml.contains("<w:pgNumType w:fmt=\"decimal\"/>"));
    let header = part(&parts, "word/header1.xml");
    assert!(header.contains("Head"));
    let footer = part(&parts, "word/footer1.xml");
    assert!(footer.contains("PAGE"), "{footer}");
    assert!(!codes.iter().any(|c| c == "docx::ink_dropped"), "{codes:?}");
}

#[test]
fn every_fixture_exports_with_all_its_text_placed() {
    for name in quillmark_fixtures::quill_names() {
        let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path(&name))
            .unwrap_or_else(|e| panic!("{name} loads: {e:?}"));
        if quill.backend_id() != "typst" {
            continue;
        }
        for doc in std::iter::once(quill.seed_document()).chain(quill.example_document()) {
            let data = quill
                .compile_checked(&doc, common::test_date())
                .expect("data");
            let session = TypstBackend::default()
                .open(&quill, &data, common::test_date())
                .expect("open");
            let result = session
                .render(&RenderOptions::default().with_output_format(OutputFormat::Docx))
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let codes: Vec<_> = result
                .warnings
                .iter()
                .filter_map(|w| w.code.clone())
                .collect();
            assert!(
                !codes
                    .iter()
                    .any(|c| c == "docx::ink_dropped" || c == "docx::probes_disabled"),
                "{name}: {:?}",
                result.warnings
            );
            let parts = word::unzip(&result.artifacts[0].bytes);
            for (n, bytes) in &parts {
                if n.ends_with(".xml") || n.ends_with(".rels") {
                    let s = String::from_utf8(bytes.clone()).unwrap();
                    roxmltree::Document::parse(&s).unwrap_or_else(|e| panic!("{name} {n}: {e}"));
                }
            }
        }
    }
}

#[test]
fn a_highlight_rule_and_picture_keep_their_ink() {
    let (parts, codes) = render(&format!(
        "{PAGE}Before #highlight(fill: blue)[blue] and #box(circle(radius: 3pt, fill: red)) after.\n\
         #line(length: 100%)\n"
    ));
    let xml = part(&parts, "word/document.xml");
    assert!(
        xml.contains("w:fill=\"0074D9\""),
        "the highlight's own fill"
    );
    assert!(xml.contains("prst=\"line\""), "the rule is a shape");
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(
        texts.iter().any(|t| t == "Before blue and  after."),
        "{texts:?}"
    );
    assert!(
        xml.contains("<wp:inline"),
        "the circle is a picture in its paragraph"
    );
    assert!(parts.iter().any(|(n, _)| n.starts_with("word/media/")));
    assert!(codes.iter().any(|c| c == "docx::rasterized"), "{codes:?}");
    assert!(
        !codes
            .iter()
            .any(|c| c == "docx::ink_dropped" || c == "docx::shape_dropped"),
        "{codes:?}"
    );
}

#[test]
fn measured_pitch_distances_tables_and_a_first_page_float() {
    let (parts, codes) = render(&format!(
        "{PAGE}A paragraph with words enough that it wraps onto a second line of text here.\n\n\
         One line.\n\n\
         #table(columns: (auto, 1fr), fill: (x, y) => if y == 0 {{ gray }}, stroke: 0.5pt + red,\n\
         [Key], [Value])\n\
         #place(bottom, float: true)[Reserved band]\n"
    ));
    let xml = part(&parts, "word/document.xml");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let lines: std::collections::BTreeSet<&str> = doc
        .descendants()
        .filter(|n| {
            n.tag_name().name() == "spacing"
                && n.parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .tag_name()
                    .name()
                    == "body"
        })
        .filter_map(|n| {
            n.attributes()
                .find(|a| a.name() == "line")
                .map(|a| a.value())
        })
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "a one-line paragraph takes the stride of its face: {lines:?}"
    );
    assert!(
        xml.contains("w:header=\"400\""),
        "the header sits at the margin less its ascent"
    );
    let before_table = paragraphs(&xml)
        .into_iter()
        .find(|p| p.0 == "One line.")
        .map(|p| p.1)
        .unwrap();
    let spacing = doc
        .get_node(before_table)
        .unwrap()
        .descendants()
        .find(|n| n.tag_name().name() == "spacing")
        .unwrap();
    assert!(
        spacing
            .attribute((NS_W, "after"))
            .is_some_and(|v| v.parse::<i32>().unwrap() > 0)
    );
    assert!(xml.contains("w:fill=\"AAAAAA\""), "the header row's fill");
    assert!(
        xml.contains("<w:tcBorders><w:top w:val=\"single\" w:sz=\"4\""),
        "{xml}"
    );
    assert_eq!(
        xml.matches("<w:noWrap/>").count(),
        1,
        "the auto column does not wrap"
    );
    assert!(xml.contains("<w:cantSplit/>"));
    assert!(xml.contains("<w:titlePg/>"));
    let footers: String = parts
        .iter()
        .filter(|(n, _)| n.starts_with("word/footer"))
        .map(|(_, b)| String::from_utf8_lossy(b).to_string())
        .collect();
    assert!(
        footers.contains("Reserved band"),
        "the float sits in the first-page footer"
    );
    assert!(!xml.contains("Reserved band"));
    assert!(
        !codes
            .iter()
            .any(|c| c == "docx::ink_dropped" || c == "docx::shape_dropped"),
        "{codes:?}"
    );
}

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn fixture_parts(name: &str) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path(name)).expect("loads");
    let doc = quill.example_document().expect("an example");
    let data = quill
        .compile_checked(&doc, common::test_date())
        .expect("data");
    let result = TypstBackend::default()
        .open(&quill, &data, common::test_date())
        .expect("open")
        .render(&RenderOptions::default().with_output_format(OutputFormat::Docx))
        .expect("render");
    let codes = result
        .warnings
        .iter()
        .filter_map(|w| w.code.clone())
        .collect();
    (word::unzip(&result.artifacts[0].bytes), codes)
}

fn style_of(xml: &str, text: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let (_, id) = paragraphs(xml).into_iter().find(|p| p.0 == text)?;
    doc.get_node(id)?
        .descendants()
        .find(|n| n.tag_name().name() == "pStyle")
        .and_then(|n| n.attribute((NS_W, "val")))
        .map(str::to_string)
}

#[test]
fn hints_number_paragraphs_live_and_name_their_roles_tabs_chrome_keep_and_font() {
    let (parts, codes) = fixture_parts("numbered_letter");
    let xml = part(&parts, "word/document.xml");
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(
        texts.contains(&"The second paragraph is the last one.".to_string()),
        "{texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|t| t.starts_with("2.") || t.starts_with("a.")),
        "{texts:?}"
    );
    assert_eq!(
        style_of(&xml, "The second paragraph is the last one.").as_deref(),
        Some("LetterParagraph1")
    );
    assert_eq!(
        style_of(&xml, "A sub-paragraph sits under its parent\u{2019}s text.").as_deref(),
        Some("LetterParagraph2")
    );
    let numbering = part(&parts, "word/numbering.xml");
    assert!(
        numbering.contains("<w:lvlText w:val=\"%1.  \"/>"),
        "{numbering}"
    );
    assert!(numbering.contains("<w:numFmt w:val=\"lowerLetter\"/><w:pStyle w:val=\"LetterParagraph2\"/><w:suff w:val=\"nothing\"/><w:lvlText w:val=\"%2.  \"/>"));
    assert_eq!(
        xml.matches("<w:numPr>").count(),
        0,
        "the number comes from the level style"
    );

    let from = paragraphs(&xml)
        .into_iter()
        .find(|p| p.0 == "FROM:123 Example Office");
    assert!(from.is_some(), "the grid line is one paragraph: {texts:?}");
    assert_eq!(
        style_of(&xml, "FROM:123 Example Office").as_deref(),
        Some("LetterHeading")
    );
    assert!(!xml.contains("<w:tbl>"), "a tabbed grid is not a table");
    let styles = part(&parts, "word/styles.xml");
    assert!(styles.contains("w:styleId=\"LetterHeading\"><w:name w:val=\"Letter Heading\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"LetterBody\"/>"), "{styles}");
    assert!(
        styles.contains("<w:tab w:val=\"left\" w:pos=\"1440\"/>"),
        "the label column's edge"
    );
    assert!(styles.contains("w:styleId=\"LetterBody\""));

    let header = part(&parts, "word/header1.xml");
    assert!(header.contains("NUMBERED LETTER"));
    assert!(!xml.contains("NUMBERED LETTER"));
    let keep = paragraphs(&xml)
        .into_iter()
        .find(|p| p.0 == "Respectfully,")
        .unwrap()
        .1;
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert!(
        doc.get_node(keep)
            .unwrap()
            .descendants()
            .any(|n| n.tag_name().name() == "keepNext")
    );
    let fonts = part(&parts, "word/fontTable.xml");
    assert!(
        fonts.contains("<w:font w:name=\"Arial\"><w:altName w:val=\"Roboto\"/>"),
        "a hinted face with no clone is named as its alt: {fonts}"
    );
    assert!(!xml.contains("w:ascii=\"Roboto\""), "the runs name the alt");
    assert!(
        !codes.iter().any(|c| [
            "docx::font_not_embedded",
            "docx::hint_unknown",
            "docx::hint_unclosed",
            "docx::hint_unchecked",
            "docx::label_mismatch",
            "docx::ink_dropped"
        ]
        .contains(&c.as_str())),
        "{codes:?}"
    );
}

const HINTS: &str = "#let hints = qm.hints\n#set page(width: 300pt, height: 300pt, margin: 30pt)\n";

#[test]
fn a_label_that_is_not_what_word_would_print_stays_printed_and_is_reported() {
    for (suffix, label) in [(". ", "3."), (") ", "1.")] {
        let (parts, codes) = render(&format!(
            "{HINTS}#hints.hint(\"list\", levels: ((format: \"decimal\", suffix: \"{suffix}\"),))[\n\
             #hints.hint(\"label\", level: 1)[{label}#h(4pt)]Printed.\n]\n"
        ));
        let xml = part(&parts, "word/document.xml");
        let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with(label) && t.ends_with("Printed.")),
            "{texts:?}"
        );
        assert!(
            codes.iter().any(|c| c == "docx::label_mismatch"),
            "{codes:?}"
        );
    }
}

#[test]
fn a_level_with_no_indent_takes_its_first_labels_place() {
    let (parts, _) = render(&format!(
        "{HINTS}#hints.hint(\"list\", levels: ((format: \"decimal\", suffix: \". \"),))[\n\
         #h(36pt)#hints.hint(\"label\", level: 1)[1.#h(4pt)]Indented.\n]\n"
    ));
    let numbering = part(&parts, "word/numbering.xml");
    assert!(numbering.contains("w:firstLine=\"720\""), "{numbering}");
    let xml = part(&parts, "word/document.xml");
    assert!(!xml.contains("<w:ind "), "the level carries it: {xml}");
}

#[test]
fn a_printed_label_in_a_drawn_enum_is_not_numbered_twice() {
    let (parts, codes) = render(&format!(
        "{HINTS}#show enum: it => hints.hint(\"list\", levels: ((format: \"decimal\", suffix: \". \"),), {{\n\
         for item in it.children {{ block[#hints.hint(\"label\", level: 1)[#(if it.start == auto {{ 1 }} else {{ it.start }}).#h(4pt)]#item.body] }}\n\
         }})\n\
         + First.\n\nBetween.\n\n#enum(start: 3)[Third.]\n"
    ));
    let xml = part(&parts, "word/document.xml");
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(texts.contains(&"First.".to_string()), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("3.") && t.ends_with("Third.")),
        "{texts:?}"
    );
    assert_eq!(
        xml.matches("<w:numPr>").count(),
        0,
        "the live number comes from the level style, and a printed one has none: {xml}"
    );
    assert!(
        codes.iter().any(|c| c == "docx::label_mismatch"),
        "{codes:?}"
    );
}

#[test]
fn an_unknown_hint_and_one_left_open_are_reported_and_cost_only_their_paragraph() {
    let (parts, codes) = render(&format!(
        "{HINTS}#metadata((kind: \"__qm_hint__\", hint: \"mystery\"))<__qm_hint__>\
         Open #metadata((kind: \"__qm_hint__\", hint: \"role\", edge: \"open\", name: \"Lost\"))<__qm_hint__> here.\n\n\
         After it.\n"
    ));
    let xml = part(&parts, "word/document.xml");
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(texts.contains(&"After it.".to_string()), "{texts:?}");
    assert_eq!(style_of(&xml, "After it."), None);
    assert!(codes.iter().any(|c| c == "docx::hint_unknown"), "{codes:?}");
    assert!(
        codes.iter().any(|c| c == "docx::hint_unclosed"),
        "{codes:?}"
    );
}

#[test]
fn a_misspelt_or_mistyped_hint_fails_the_compile_with_its_message() {
    for (hint, message) in [
        (
            "#qm.hints.hint(\"role\", nme: \"X\")[text]",
            "hint role: unknown field \\\"nme\\\"",
        ),
        (
            "#qm.hints.hint(\"list\", levels: ((suffix: [.]),))[text]",
            "hint list level: suffix must be str, got content",
        ),
        (
            "#qm.hints.hint-point(\"font\", family: \"A\", alt: 3)",
            "hint font: alt must be str, got int",
        ),
    ] {
        let plate = format!("#import \"@local/quillmark-helper:0.2.0\" as qm\n{HINTS}{hint}\n");
        let quill = common::quill_with_plate(&common::yaml(""), &plate);
        let err = TypstBackend::default()
            .open(&quill, &json!({}), common::test_date())
            .and_then(|s| s.render(&RenderOptions::default().with_output_format(OutputFormat::Pdf)))
            .err()
            .expect("a compile error");
        assert!(format!("{err:?}").contains(message), "{hint}: {err:?}");
    }
}

#[test]
fn an_outline_is_a_toc_field_over_its_entries_and_its_title_is_unlisted() {
    let (parts, codes) = render(&format!(
        "{PAGE}#outline()\n= Intro\nText.\n== Sub\n#pagebreak()\n= Second\n"
    ));
    let xml = part(&parts, "word/document.xml");
    assert!(
        xml.contains(
            "<w:instrText xml:space=\"preserve\"> TOC \\o \"1-9\" \\h \\z \\u </w:instrText>"
        ),
        "{xml}"
    );
    assert!(!xml.contains("w:fldLock"));
    assert_eq!(
        style_of(&xml, "Contents").as_deref(),
        Some("UnlistedHeading1")
    );
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(
        texts.contains(&"Intro1".to_string()) && texts.contains(&"Second2".to_string()),
        "{texts:?}"
    );
    assert_eq!(style_of(&xml, "Sub1").as_deref(), Some("TOC2"));
    assert_eq!(
        xml.matches("<w:tab w:val=\"right\" w:leader=\"dot\"")
            .count(),
        3
    );
    let styles = part(&parts, "word/styles.xml");
    assert!(styles.contains("<w:name w:val=\"toc 2\"/>"));
    let unlisted = styles
        .split("<w:style ")
        .find(|s| s.contains("w:styleId=\"UnlistedHeading1\""))
        .expect("the unlisted style");
    assert!(
        unlisted.contains("<w:keepNext/>") && !unlisted.contains("outlineLvl"),
        "{unlisted}"
    );
    assert!(
        !codes
            .iter()
            .any(|c| c == "docx::toc_frozen" || c == "docx::ink_dropped"),
        "{codes:?}"
    );
}

#[test]
fn an_outline_whose_page_numbers_carry_a_prefix_is_locked() {
    let (parts, codes) = render(
        "#set page(width: 300pt, height: 300pt, margin: 30pt, numbering: (..n) => \"A-\" + str(n.pos().first()))\n\
         #outline()\n= Intro\n",
    );
    let xml = part(&parts, "word/document.xml");
    assert!(xml.contains("w:fldLock=\"1\""), "{xml}");
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(texts.contains(&"IntroA-1".to_string()), "{texts:?}");
    assert!(codes.iter().any(|c| c == "docx::toc_frozen"), "{codes:?}");
}

#[test]
fn a_document_the_probes_would_move_is_read_without_them_and_keeps_its_text() {
    let w = word::render(
        "#set page(width: 300pt, height: 300pt, margin: 30pt)\n\
         = Heading\n\
         One two three four five #linebreak(justify: true) six seven.\n\n\
         A second paragraph.\n",
    );
    assert!(w.has("docx::probes_disabled"), "{:?}", w.codes);
    assert!(w.has("docx::margins_assumed"), "{:?}", w.codes);
    assert!(!w.has("docx::ink_dropped"), "{:?}", w.codes);
    let body = w.body();
    assert_eq!(body.len(), 3, "{:?}", word::texts(&body));
    assert_eq!(body[0].style.as_deref(), Some("Heading1"));
}
