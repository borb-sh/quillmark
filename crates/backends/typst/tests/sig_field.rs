//! Compiles each plate through the public `Backend`/`LiveSession` path, parses
//! the output with lopdf, and asserts the AcroForm structure.

use quillmark_core::{Backend, OutputFormat, RenderError, RenderOptions};
use quillmark_typst::TypstBackend;

mod common;
use common::host_with_plate as source_with_plate;

fn compile(plate: &str) -> Result<Vec<u8>, RenderError> {
    // Our plates don't reference data fields, so an empty payload suffices.
    compile_with_data(plate, &serde_json::json!({}))
}

/// [`compile`] with `json_data` threaded to the plate's `data` binding.
fn compile_with_data(plate: &str, json_data: &serde_json::Value) -> Result<Vec<u8>, RenderError> {
    let source = source_with_plate(plate);
    let session = TypstBackend.open(&source, json_data)?;
    let result = session.render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))?;
    Ok(result.artifacts[0].bytes.clone())
}

/// The parsed document plus a map from field name (`/T`) to its widget dict.
fn acroform_widgets(
    plate: &str,
    json_data: &serde_json::Value,
) -> (
    lopdf::Document,
    std::collections::HashMap<String, lopdf::Dictionary>,
) {
    let pdf = compile_with_data(plate, json_data).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    let mut by_name = std::collections::HashMap::new();
    for f in fields {
        let widget = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name =
            String::from_utf8_lossy(widget.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, widget.clone());
    }
    (doc, by_name)
}

#[test]
fn acceptance_two_pages_two_fields() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field

#set page(width: 600pt, height: 400pt, margin: 50pt)

Page 1.
#signature-field("a")

#pagebreak()

Page 2.
#signature-field("b")
"#;
    let pdf = compile(plate).expect("compile ok");

    let doc = lopdf::Document::load_mem(&pdf).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");

    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());

    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 2);
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 2);

    let to_f64 = |o: &lopdf::Object| -> f64 {
        o.as_float()
            .map(|f| f as f64)
            .or_else(|_| o.as_i64().map(|i| i as f64))
            .unwrap()
    };
    let page_refs: Vec<(u32, u16)> = pages.iter().map(|(_, &id)| (id.0, id.1)).collect();

    for f in fields {
        let widget = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name =
            String::from_utf8_lossy(widget.get(b"T").unwrap().as_str().unwrap()).into_owned();
        assert_eq!(widget.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
        assert_eq!(
            widget.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Widget"
        );

        let page_ref = widget.get(b"P").unwrap().as_reference().unwrap();
        let page_index = page_refs.iter().position(|&p| p == page_ref).unwrap();
        let expected = if name == "a" { 0 } else { 1 };
        assert_eq!(page_index, expected, "field {name} on wrong page");

        let rect = widget.get(b"Rect").unwrap().as_array().unwrap();
        let (llx, lly, urx, ury) = (
            to_f64(&rect[0]),
            to_f64(&rect[1]),
            to_f64(&rect[2]),
            to_f64(&rect[3]),
        );
        assert!(
            (urx - llx - 200.0).abs() < 1.0,
            "field {name} width: {}",
            urx - llx
        );
        assert!(
            (ury - lly - 50.0).abs() < 1.0,
            "field {name} height: {}",
            ury - lly
        );
        assert!(
            llx >= 0.0 && urx <= 600.0 && lly >= 0.0 && ury <= 400.0,
            "field {name} rect outside page: [{llx}, {lly}, {urx}, {ury}]"
        );
    }
}

#[test]
fn acceptance_duplicate_name_errors() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field

#set page(width: 600pt, height: 400pt, margin: 50pt)
#signature-field("a")
#signature-field("a")
"#;
    let err = compile(plate).expect_err("expected duplicate-name error");
    let diags = err.diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::duplicate_form_field")),
        "expected typst::duplicate_form_field diagnostic, got {:?}",
        diags
    );
}

/// A user can attach the `<__qm_field__>` label to unrelated metadata; the
/// extractor's `kind` check must filter it without losing the real call.
#[test]
fn user_metadata_on_reserved_label_does_not_clobber() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#metadata((kind: "something-else", note: "user's own metadata")) <__qm_field__>
#signature-field("real_field")
"#;
    let pdf = compile(plate).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let cat = doc.catalog().unwrap();
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(
        fields.len(),
        1,
        "expected exactly 1 real field, got {}",
        fields.len()
    );
    let widget = doc
        .get_object(fields[0].as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(
        widget.get(b"T").unwrap().as_str().unwrap(),
        b"real_field",
        "wrong field name survived extraction"
    );
}

#[test]
fn acceptance_no_fields_no_overlay() {
    let plate = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)

Just a doc.
"#;
    let pdf = compile(plate).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let cat = doc.catalog().unwrap();
    assert!(
        !cat.has(b"AcroForm"),
        "expected no /AcroForm in catalog for sig-field-free plate"
    );

    // No fields, so the overlay is skipped, but the always-on `/Producer` pass
    // still appends one incremental update.
    let startxref_count = pdf
        .windows(b"startxref\n".len())
        .filter(|w| *w == b"startxref\n")
        .count();
    assert_eq!(
        startxref_count, 2,
        "expected 2 startxref markers (one Producer-metadata incremental update); got {}",
        startxref_count
    );
    assert_eq!(
        pdf.windows(b"/Prev".len())
            .filter(|w| *w == b"/Prev")
            .count(),
        1,
        "expected exactly one /Prev (the Producer-metadata incremental update)"
    );
}

// The tests below assert the typst→spec mapping; the spine bytes (`Ff` flag
// bits) belong to `quillmark-pdf/tests/stamp.rs`.

#[test]
fn form_field_text_carries_its_value() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("single", type: "text", value: "hello")
"#;
    let (_doc, widgets) = acroform_widgets(plate, &serde_json::json!({}));

    let single = widgets.get("single").expect("single field");
    assert_eq!(single.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(single.get(b"V").unwrap().as_str().unwrap(), b"hello");
}

#[test]
fn form_field_signature_via_general_helper() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("sig", type: "signature")
"#;
    let (doc, widgets) = acroform_widgets(plate, &serde_json::json!({}));
    let sig = widgets.get("sig").expect("sig field");
    assert_eq!(sig.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    assert!(sig.get(b"V").is_err(), "signature field must carry no /V");

    let cat = doc.catalog().unwrap();
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
}

#[test]
fn form_field_value_binding_from_data() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("name", type: "text", value: data.full_name)
#form-field("count", type: "text", value: str(data.count))
"#;
    let json = serde_json::json!({
        "full_name": "Ada Lovelace",
        "count": 7,
    });
    let (_doc, widgets) = acroform_widgets(plate, &json);

    assert_eq!(
        widgets
            .get("name")
            .unwrap()
            .get(b"V")
            .unwrap()
            .as_str()
            .unwrap(),
        b"Ada Lovelace"
    );
    assert_eq!(
        widgets
            .get("count")
            .unwrap()
            .get(b"V")
            .unwrap()
            .as_str()
            .unwrap(),
        b"7"
    );
}

/// A widget binding no schema field has only a `/T` name, not a schema address,
/// so it surfaces no region.
#[test]
fn form_field_regions_key_on_bound_schema_field() {
    const YAML: &str = r#"
quill:
  name: widget_regions
  version: 0.1.0
  backend: typst
  description: form-field region binding test
typst:
  plate_file: plate.typ
main:
  fields:
    f_txt:
      type: string
      description: text widget binding
    f_chk:
      type: string
      description: second text widget binding
    f_sig:
      type: string
      description: signature widget binding
"#;
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("txt", type: "text", value: "hi", field: "f_txt")
#form-field("chk", type: "text", value: "y", field: "f_chk")
#form-field("sig", type: "signature", field: "f_sig")
#form-field("unbound", type: "text", value: "x")
"#;
    let source = common::quill_with_plate(YAML, plate);
    let session = TypstBackend
        .open(&source, &serde_json::json!({}))
        .expect("open");
    let regions = session.regions();

    let fields: std::collections::HashMap<&str, &quillmark_core::RenderedRegion> =
        regions.iter().map(|r| (r.field.as_str(), r)).collect();

    for field in ["f_txt", "f_chk", "f_sig"] {
        let r = fields
            .get(field)
            .unwrap_or_else(|| panic!("region keyed on bound schema field {field:?}"));
        assert_eq!(r.page, 0);
        assert!(
            r.rect[2] > r.rect[0] && r.rect[3] > r.rect[1],
            "region {field:?} rect is a proper box: {:?}",
            r.rect
        );
    }
    assert!(
        !fields.contains_key("unbound"),
        "an unbound widget exposes no region: {:?}",
        fields.keys().collect::<Vec<_>>()
    );
    for t_name in ["txt", "chk", "sig"] {
        assert!(
            !fields.contains_key(t_name),
            "a bound widget must not also leak its `/T` name {t_name:?}: {:?}",
            fields.keys().collect::<Vec<_>>()
        );
    }
}
