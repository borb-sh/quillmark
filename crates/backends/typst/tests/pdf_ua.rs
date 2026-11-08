//! Every PDF render tries PDF/UA-1. A document that conforms carries the
//! PDF/UA identification; one Typst refuses exports plain, with each refusal a
//! located `typst::pdf_ua` warning; one with form fields exports plain and
//! silently.

use quillmark_core::backend::Backend;
use quillmark_core::error::{Diagnostic, Severity};
use quillmark_core::types::{OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;

mod common;

fn render(plate: &str) -> (Vec<u8>, Vec<Diagnostic>) {
    let session = TypstBackend::default()
        .open(
            &common::host_with_plate(plate),
            &serde_json::json!({}),
            common::test_date(),
        )
        .expect("open");
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("render");
    (result.artifacts[0].bytes.clone(), result.warnings)
}

fn claims_ua(pdf: &[u8]) -> bool {
    pdf.windows(b"pdfuaid:part".len())
        .any(|w| w == b"pdfuaid:part")
}

fn ua_warnings(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings
        .iter()
        .filter(|d| d.code.as_deref() == Some("typst::pdf_ua"))
        .collect()
}

#[test]
fn a_conforming_document_exports_as_pdf_ua_1() {
    let (pdf, warnings) = render("= Heading\n\nBody text.\n");
    assert!(claims_ua(&pdf));
    assert!(ua_warnings(&warnings).is_empty(), "{warnings:?}");
}

#[test]
fn a_refused_document_exports_plain_with_a_located_warning() {
    let (pdf, warnings) = render("Body text.\n\n#image(\"/assets/dod_seal.png\", width: 1in)\n");
    assert!(!claims_ua(&pdf));
    let ua = ua_warnings(&warnings);
    assert_eq!(ua.len(), 1, "{warnings:?}");
    assert_eq!(ua[0].severity, Severity::Warning);
    let location = ua[0].location.as_ref().expect("located");
    assert!(location.file.ends_with("plate.typ"), "{location:?}");
    assert_eq!(location.line, 3);
}

#[test]
fn a_document_with_form_fields_exports_plain_without_a_warning() {
    let (pdf, warnings) = render(
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n\nSign below.\n#qm.signature-field(\"a\")\n",
    );
    assert!(!claims_ua(&pdf));
    assert!(ua_warnings(&warnings).is_empty(), "{warnings:?}");
}
