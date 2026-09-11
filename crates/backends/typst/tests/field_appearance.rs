//! The three type dials on `form-field` (`font`, `size`, `align`), asserted on
//! the stamped PDF: each widget's `/DA` and `/Q`, and the `/DR` `/Font` the
//! `/DA` names resolve against.

use quillmark_core::{Backend, OutputFormat, RenderError, RenderOptions};
use quillmark_typst::TypstBackend;

mod common;
use common::host_with_plate as source_with_plate;

fn compile(plate: &str) -> Result<Vec<u8>, RenderError> {
    let source = source_with_plate(plate);
    let session = TypstBackend.open(&source, &serde_json::json!({}))?;
    let result = session.render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))?;
    Ok(result.artifacts[0].bytes.clone())
}

/// The parsed document, its AcroForm dict, and a `/T` → widget map.
fn acroform(
    plate: &str,
) -> (
    lopdf::Document,
    lopdf::Dictionary,
    std::collections::HashMap<String, lopdf::Dictionary>,
) {
    let pdf = compile(plate).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    let cat = doc.catalog().expect("catalog");
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap()
        .clone();
    let mut by_name = std::collections::HashMap::new();
    for f in af.get(b"Fields").unwrap().as_array().unwrap() {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap()
            .clone();
        let name = String::from_utf8_lossy(w.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, w);
    }
    (doc, af, by_name)
}

fn da(w: &lopdf::Dictionary) -> String {
    String::from_utf8_lossy(w.get(b"DA").expect("/DA").as_str().unwrap()).into_owned()
}

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field

#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("plain", type: "text")
#form-field("dated", type: "text", font: "times", size: 12pt, align: "right")
#form-field("centred", type: "text", font: "courier", align: "center")
"#;

#[test]
fn da_carries_the_requested_face_and_size() {
    let (_, _, w) = acroform(PLATE);
    assert_eq!(da(&w["plain"]), "/Helv 0 Tf 0 g");
    assert_eq!(da(&w["dated"]), "/TiRo 12 Tf 0 g");
    assert_eq!(da(&w["centred"]), "/Cour 0 Tf 0 g");
}

#[test]
fn quadding_is_written_only_when_it_moves_the_text() {
    let (_, _, w) = acroform(PLATE);
    assert!(
        w["plain"].get(b"Q").is_err(),
        "left is the PDF default and stays unwritten"
    );
    assert_eq!(w["dated"].get(b"Q").unwrap().as_i64().unwrap(), 2);
    assert_eq!(w["centred"].get(b"Q").unwrap().as_i64().unwrap(), 1);
}

/// A `/DA` naming a face absent from `/DR` `/Font` is undefined behavior, so
/// every face used must resolve, and Helvetica must be there for the
/// form-level `/DA` even when no widget asks for it.
#[test]
fn dr_font_carries_every_face_named_by_a_da() {
    let (doc, af, _) = acroform(PLATE);
    let dr = af.get(b"DR").unwrap().as_dict().unwrap();
    let fonts = dr.get(b"Font").unwrap().as_dict().unwrap();

    let mut base_fonts: Vec<String> = fonts
        .iter()
        .map(|(_, v)| {
            let f = doc
                .get_object(v.as_reference().unwrap())
                .unwrap()
                .as_dict()
                .unwrap();
            String::from_utf8_lossy(f.get(b"BaseFont").unwrap().as_name().unwrap()).into_owned()
        })
        .collect();
    base_fonts.sort();

    assert_eq!(base_fonts, ["Courier", "Helvetica", "Times-Roman"]);
    for key in ["Helv", "TiRo", "Cour"] {
        assert!(fonts.has(key.as_bytes()), "/DR /Font is missing /{key}");
    }
}

/// A quill that never touches the dials must stamp exactly as it did before
/// they existed: one Helvetica in `/DR`, auto-size `/DA`, no `/Q`.
#[test]
fn untouched_fields_keep_the_house_style() {
    let (doc, af, w) = acroform(
        r#"
#import "@local/quillmark-helper:0.1.0": form-field

#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("a", type: "text")
"#,
    );
    assert_eq!(da(&w["a"]), "/Helv 0 Tf 0 g");
    assert!(w["a"].get(b"Q").is_err());

    let fonts = af
        .get(b"DR")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(fonts.len(), 1);
    let f = doc
        .get_object(fonts.get(b"Helv").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(f.get(b"BaseFont").unwrap().as_name().unwrap(), b"Helvetica");
}

/// `0pt` reaches the PDF as `0 Tf`, which *is* auto-size, so it has to be
/// refused rather than granted as the opposite of what it asks for.
#[test]
fn a_non_positive_size_is_rejected() {
    for bad in ["0pt", "-4pt"] {
        let e = compile(&format!(
            r#"
#import "@local/quillmark-helper:0.1.0": form-field

#form-field("t", type: "text", size: {bad})
"#
        ))
        .expect_err(&format!("size: {bad} must not compile"));
        assert!(
            format!("{e:?}").contains("positive length"),
            "expected the helper's assert for size: {bad}, got {e:?}"
        );
    }
}

/// The dials are meaningless where there is no variable text, so the helper
/// rejects them rather than accepting a call whose styling silently vanishes.
#[test]
fn dials_are_rejected_on_fields_without_variable_text() {
    let e = compile(
        r#"
#import "@local/quillmark-helper:0.1.0": form-field

#form-field("sig", type: "signature", align: "right")
"#,
    )
    .expect_err("a styled signature field is an error");
    assert!(
        format!("{e:?}").contains("text fields only"),
        "expected the helper's assert, got {e:?}"
    );
}
