//! Compiles plates to PDF, reparses with lopdf, and asserts the `/Info`
//! `/Producer` stamp and `/Title`.

use lopdf::Object;
use quillmark_core::{backend::Backend, types::{OutputFormat, RenderOptions}};
use quillmark_typst::TypstBackend;

mod common;
use common::host_with_plate as source_with_plate;

const PLATE: &str = "#set page(width: 400pt, height: 300pt)\n= Hello\n";

fn render_pdf(plate: &str) -> Vec<u8> {
    render_titled(plate, None)
}

fn render_titled(plate: &str, title: Option<&str>) -> Vec<u8> {
    let source = source_with_plate(plate);
    let session = TypstBackend::default()
        .open(&source, &serde_json::json!({}), common::test_date())
        .expect("open session");
    let opts = RenderOptions::default().with_output_format(OutputFormat::Pdf);
    let result = session
        .render(&match title {
            Some(title) => opts.with_title(title),
            None => opts,
        })
        .expect("render ok");
    result.artifacts[0].bytes.clone()
}

fn info_string(pdf: &[u8], key: &[u8]) -> Vec<u8> {
    let doc = lopdf::Document::load_mem(pdf).expect("reparse pdf");
    let info_ref = doc
        .trailer
        .get(b"Info")
        .expect("/Info in trailer")
        .as_reference()
        .expect("/Info is a reference");
    let info = doc.get_object(info_ref).unwrap().as_dict().unwrap();
    match info.get(key).expect("key present in /Info") {
        Object::String(bytes, _) => bytes.clone(),
        other => panic!("{key:?} not a string: {other:?}"),
    }
}

/// The stamp reads `quillmark-pdf`'s version; `CARGO_PKG_VERSION` here is this
/// crate's. One `version.workspace = true` gives both the same string. The
/// pass rewrites `/Producer` alone, so Typst's `/Creator` survives it.
#[test]
fn producer_is_quillmark_and_creator_stays_typst() {
    let pdf = render_pdf(PLATE);
    let expected = format!("Quillmark {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(info_string(&pdf, b"Producer"), expected.as_bytes());
    let creator = info_string(&pdf, b"Creator");
    assert!(
        creator.starts_with(b"Typst"),
        "expected Typst /Creator, got {:?}",
        String::from_utf8_lossy(&creator)
    );
}

const TITLED_PLATE: &str = "#set document(title: \"Plate Title\")\n= Hello\n";

#[test]
fn title_falls_back_to_the_quill_description() {
    let description = source_with_plate(PLATE).config().description.clone();
    assert_eq!(info_string(&render_pdf(PLATE), b"Title"), description.as_bytes());
}

#[test]
fn plate_title_overrides_the_description() {
    assert_eq!(info_string(&render_pdf(TITLED_PLATE), b"Title"), b"Plate Title");
}

#[test]
fn render_title_overrides_the_plate() {
    let pdf = render_titled(TITLED_PLATE, Some("Render Title"));
    assert_eq!(info_string(&pdf, b"Title"), b"Render Title");
}

#[test]
fn blank_render_title_counts_as_absent() {
    let pdf = render_titled(TITLED_PLATE, Some("  "));
    assert_eq!(info_string(&pdf, b"Title"), b"Plate Title");
}

#[test]
fn blank_plate_title_falls_back_to_the_description() {
    let plate = "#set document(title: \" \")\n= Hello\n";
    let description = source_with_plate(plate).config().description.clone();
    assert_eq!(info_string(&render_pdf(plate), b"Title"), description.as_bytes());
}

#[test]
fn render_title_is_trimmed() {
    let pdf = render_titled(PLATE, Some(" Render Title "));
    assert_eq!(info_string(&pdf, b"Title"), b"Render Title");
}
