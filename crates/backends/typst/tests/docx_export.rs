//! A DOCX render end to end: one package that carries the document's text.

use quillmark_core::backend::Backend;
use quillmark_core::types::{OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;
use serde_json::json;

mod common;

/// Each part of the one artifact `plate` renders to as DOCX.
fn docx_parts(plate: &str, data: serde_json::Value) -> Vec<(String, Vec<u8>)> {
    let quill = common::quill_with_plate(
        &common::yaml("main:\n  fields:\n    prose: { type: richtext, shape: blocks }\n"),
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
    quillmark_docx::unzip(&result.artifacts[0].bytes).expect("a zip package")
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
    (
        quillmark_docx::unzip(&result.artifacts[0].bytes).unwrap(),
        codes,
    )
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
            let parts = quillmark_docx::unzip(&result.artifacts[0].bytes).expect("zip");
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
    assert!(xml.contains("w:fill=\"0074D9\""), "the highlight's own fill");
    assert!(xml.contains("prst=\"line\""), "the rule is a shape");
    let texts: Vec<String> = paragraphs(&xml).into_iter().map(|p| p.0).collect();
    assert!(texts.iter().any(|t| t == "Before blue and  after."), "{texts:?}");
    assert!(xml.contains("<wp:inline"), "the circle is a picture in its paragraph");
    assert!(parts.iter().any(|(n, _)| n.starts_with("word/media/")));
    assert!(codes.iter().any(|c| c == "docx::rasterized"), "{codes:?}");
    assert!(!codes.iter().any(|c| c == "docx::ink_dropped" || c == "docx::shape_dropped"), "{codes:?}");
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
        .filter(|n| n.tag_name().name() == "spacing" && n.parent().unwrap().parent().unwrap().parent().unwrap().tag_name().name() == "body")
        .filter_map(|n| n.attributes().find(|a| a.name() == "line").map(|a| a.value()))
        .collect();
    assert_eq!(lines.len(), 1, "a one-line paragraph takes the stride of its face: {lines:?}");
    assert!(xml.contains("w:header=\"400\""), "the header sits at the margin less its ascent");
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
    assert!(spacing.attribute((NS_W, "after")).is_some_and(|v| v.parse::<i32>().unwrap() > 0));
    assert!(xml.contains("w:fill=\"AAAAAA\""), "the header row's fill");
    assert!(xml.contains("<w:tcBorders><w:top w:val=\"single\" w:sz=\"4\""), "{xml}");
    assert_eq!(xml.matches("<w:noWrap/>").count(), 1, "the auto column does not wrap");
    assert!(xml.contains("<w:cantSplit/>"));
    assert!(xml.contains("<w:titlePg/>"));
    let footers: String = parts
        .iter()
        .filter(|(n, _)| n.starts_with("word/footer"))
        .map(|(_, b)| String::from_utf8_lossy(b).to_string())
        .collect();
    assert!(footers.contains("Reserved band"), "the float sits in the first-page footer");
    assert!(!xml.contains("Reserved band"));
    assert!(!codes.iter().any(|c| c == "docx::ink_dropped" || c == "docx::shape_dropped"), "{codes:?}");
}

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
