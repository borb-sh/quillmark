//! Compiles each plate through the public `Backend`/`LiveSession` path, parses
//! the output with lopdf, and asserts the AcroForm structure.

use std::collections::HashMap;

use quillmark_core::{backend::Backend, error::RenderError, types::{OutputFormat, RenderOptions}};
use quillmark_typst::TypstBackend;

mod common;
use common::host_with_plate as source_with_plate;

fn compile(plate: &str, json_data: &serde_json::Value) -> Result<Vec<u8>, RenderError> {
    let source = source_with_plate(plate);
    let session = TypstBackend.open(&source, json_data, None)?;
    let result = session.render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))?;
    Ok(result.artifacts[0].bytes.clone())
}

/// The parsed document, its AcroForm dict, and a `/T` → widget map.
fn acroform(
    plate: &str,
    json_data: &serde_json::Value,
) -> (lopdf::Document, lopdf::Dictionary, HashMap<String, lopdf::Dictionary>) {
    let pdf = compile(plate, json_data).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    let af_ref = doc
        .catalog()
        .expect("catalog")
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap().clone();
    let mut by_name = HashMap::new();
    for f in af.get(b"Fields").unwrap().as_array().unwrap() {
        let widget = doc.get_object(f.as_reference().unwrap()).unwrap().as_dict().unwrap();
        let name =
            String::from_utf8_lossy(widget.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, widget.clone());
    }
    (doc, af, by_name)
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
    let (doc, af, widgets) = acroform(plate, &serde_json::json!({}));
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert_eq!(widgets.len(), 2);

    let to_f64 = |o: &lopdf::Object| -> f64 {
        o.as_float()
            .map(|f| f as f64)
            .or_else(|_| o.as_i64().map(|i| i as f64))
            .unwrap()
    };
    let page_refs: Vec<(u32, u16)> = doc.get_pages().values().copied().collect();
    assert_eq!(page_refs.len(), 2);

    for (name, expected_page) in [("a", 0), ("b", 1)] {
        let widget = &widgets[name];
        assert_eq!(widget.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
        assert_eq!(
            widget.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Widget"
        );

        let page_ref = widget.get(b"P").unwrap().as_reference().unwrap();
        let page_index = page_refs.iter().position(|&p| p == page_ref).unwrap();
        assert_eq!(page_index, expected_page, "field {name} on wrong page");

        let rect = widget.get(b"Rect").unwrap().as_array().unwrap();
        let [llx, lly, urx, ury] = [0, 1, 2, 3].map(|i| to_f64(&rect[i]));
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
    let err = compile(plate, &serde_json::json!({})).expect_err("expected duplicate-name error");
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
    let (_, _, widgets) = acroform(plate, &serde_json::json!({}));
    assert_eq!(
        widgets.keys().collect::<Vec<_>>(),
        ["real_field"],
        "exactly the real field survives extraction"
    );
}

#[test]
fn acceptance_no_fields_no_overlay() {
    let plate = "#set page(width: 600pt, height: 400pt, margin: 50pt)\n\nJust a doc.\n";
    let pdf = compile(plate, &serde_json::json!({})).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    assert!(
        !doc.catalog().unwrap().has(b"AcroForm"),
        "expected no /AcroForm in catalog for sig-field-free plate"
    );
}

/// The typst→spec mapping for each `form-field` type and a value bound from
/// `data`; the spine bytes (`Ff` flag bits) belong to `quillmark-pdf/tests/stamp.rs`.
#[test]
fn form_field_maps_each_type_and_binds_values_from_data() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("single", type: "text", value: "hello")
#form-field("multi", type: "text", value: "a\nb", multiline: true)
#form-field("sig", type: "signature")
#form-field("name", type: "text", value: data.full_name)
#form-field("count", type: "text", value: str(data.count))
"#;
    let (_, af, w) = acroform(plate, &serde_json::json!({ "full_name": "Ada Lovelace", "count": 7 }));

    let ft = |name: &str| w[name].get(b"FT").unwrap().as_name().unwrap().to_vec();
    let v = |name: &str| w[name].get(b"V").unwrap().as_str().unwrap().to_vec();
    assert_eq!(ft("single"), b"Tx");
    assert_eq!(ft("multi"), b"Tx");
    assert_eq!(ft("sig"), b"Sig");
    assert_eq!(v("single"), b"hello");
    assert_eq!(v("name"), b"Ada Lovelace");
    assert_eq!(v("count"), b"7");
    assert!(w["sig"].get(b"V").is_err(), "signature field must carry no /V");
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
}

/// A widget binding no schema field has only a `/T` name, not a schema address,
/// so it surfaces no region. A field bound to a widget *and* read as content
/// surfaces both, widget first — `SessionHandle::regions`' order contract.
#[test]
fn form_field_regions_key_on_bound_schema_field() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#data.f_txt
#form-field("txt", type: "text", value: data.f_txt, field: "f_txt", width: 137pt)
#form-field("sig", type: "signature", field: "f_sig")
#form-field("unbound", type: "text", value: "x")
"#;
    let source = common::quill_with_plate(
        &common::yaml("main:\n  fields:\n    f_txt: { type: string }\n    f_sig: { type: string }\n"),
        plate,
    );
    let session = TypstBackend
        .open(&source, &serde_json::json!({ "f_txt": "FIRST M. LAST", "f_sig": "" }), None)
        .expect("open");
    let regions = session.regions();
    let of = |field: &str| -> Vec<&quillmark_core::region::RenderedRegion> {
        regions.iter().filter(|r| r.field == field).collect()
    };

    for field in ["f_txt", "f_sig"] {
        let [r, ..] = of(field)[..] else {
            panic!("region keyed on bound schema field {field:?}: {regions:?}");
        };
        assert_eq!(r.page, 0);
        assert!(
            r.rect[2] > r.rect[0] && r.rect[3] > r.rect[1],
            "region {field:?} rect is a proper box: {:?}",
            r.rect
        );
    }

    // The widget is a fixed-size box at the test-owned 137pt width; the content
    // region is the ink of the placed string, telling which entry is which
    // without a `source` field the type does not carry.
    let txt = of("f_txt");
    assert_eq!(txt.len(), 2, "widget and content both surface: {regions:?}");
    assert!(
        (txt[0].rect[2] - txt[0].rect[0] - 137.0).abs() < 0.01,
        "the widget region sorts first: {regions:?}"
    );

    for name in ["unbound", "txt", "sig"] {
        assert!(
            of(name).is_empty(),
            "a widget's `/T` name {name:?} is not a region key: {regions:?}"
        );
    }
}
