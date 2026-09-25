//! The three type dials on `form-field` (`font`, `size`, `align`), asserted on
//! the stamped PDF: each widget's `/DA` and `/Q`, and the `/DR` `/Font` the
//! `/DA` names resolve against.

use quillmark_core::{backend::Backend, error::RenderError, types::{OutputFormat, RenderOptions}};
use quillmark_typst::TypstBackend;

mod common;
use common::host_with_plate as source_with_plate;

fn compile(plate: &str) -> Result<Vec<u8>, RenderError> {
    let source = source_with_plate(plate);
    let session = TypstBackend.open(&source, &serde_json::json!({}), common::test_date())?;
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

/// The base fonts `/DR` `/Font` registers, sorted.
fn dr_base_fonts(doc: &lopdf::Document, af: &lopdf::Dictionary) -> Vec<String> {
    let fonts = af.get(b"DR").unwrap().as_dict().unwrap().get(b"Font").unwrap().as_dict().unwrap();
    let mut base_fonts: Vec<String> = fonts
        .iter()
        .map(|(_, v)| {
            let f = doc.get_object(v.as_reference().unwrap()).unwrap().as_dict().unwrap();
            String::from_utf8_lossy(f.get(b"BaseFont").unwrap().as_name().unwrap()).into_owned()
        })
        .collect();
    base_fonts.sort();
    base_fonts
}

/// Quadding is written only when it moves the text, left being the PDF
/// default. A `/DA` naming a face absent from `/DR` `/Font` is undefined
/// behavior, so every face used must resolve, and Helvetica must be there for
/// the form-level `/DA` even when no widget asks for it.
#[test]
fn the_dials_reach_da_q_and_dr() {
    let (doc, af, w) = acroform(PLATE);
    assert_eq!(da(&w["plain"]), "/Helv 0 Tf 0 g");
    assert_eq!(da(&w["dated"]), "/TiRo 12 Tf 0 g");
    assert_eq!(da(&w["centred"]), "/Cour 0 Tf 0 g");

    assert!(w["plain"].get(b"Q").is_err());
    assert_eq!(w["dated"].get(b"Q").unwrap().as_i64().unwrap(), 2);
    assert_eq!(w["centred"].get(b"Q").unwrap().as_i64().unwrap(), 1);

    assert_eq!(dr_base_fonts(&doc, &af), ["Courier", "Helvetica", "Times-Roman"]);
    let fonts = af.get(b"DR").unwrap().as_dict().unwrap().get(b"Font").unwrap().as_dict().unwrap();
    for key in ["Helv", "TiRo", "Cour"] {
        assert!(fonts.has(key.as_bytes()), "/DR /Font is missing /{key}");
    }
}

/// A quill that never touches the dials registers one Helvetica in `/DR`.
#[test]
fn untouched_fields_register_helvetica_alone() {
    let (doc, af, _) = acroform(
        r#"
#import "@local/quillmark-helper:0.1.0": form-field

#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("a", type: "text")
"#,
    );
    assert_eq!(dr_base_fonts(&doc, &af), ["Helvetica"]);
}

/// `0pt` reaches the PDF as `0 Tf`, which *is* auto-size, so it has to be
/// refused rather than granted as the opposite of what it asks for. The dials
/// are meaningless where there is no variable text, so a signature refuses
/// them rather than accepting a call whose styling silently vanishes.
#[test]
fn a_dial_the_widget_cannot_honour_is_rejected() {
    for (call, assert) in [
        ("type: \"text\", size: 0pt", "positive length"),
        ("type: \"text\", size: -4pt", "positive length"),
        ("type: \"signature\", align: \"right\"", "text fields only"),
    ] {
        let e = compile(&format!(
            "#import \"@local/quillmark-helper:0.1.0\": form-field\n#form-field(\"t\", {call})\n"
        ))
        .expect_err(&format!("{call} must not compile"));
        assert!(
            format!("{e:?}").contains(assert),
            "expected the helper's assert for {call}, got {e:?}"
        );
    }
}
