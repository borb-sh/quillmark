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
