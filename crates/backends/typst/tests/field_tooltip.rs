//! A widget's `/TU`: the `tooltip:` argument where the plate passes one, else
//! the `description` of the schema field its `field:` address names.

use quillmark_core::backend::Backend;
use quillmark_core::types::{OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;

mod common;

const SCHEMA: &str = r#"main:
  fields:
    approver:
      type: text
      description: Name of the approving official
    bare:
      type: text
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      default: ""
      description: Banner marking
      variants:
        CUI:
          poc:
            type: text
            description: CUI point of contact
card_kinds:
  endorsement:
    fields:
      office:
        type: text
        description: Endorsing office symbol
"#;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 600pt, height: 600pt, margin: 20pt)
#qm.signature-field("Inherited", height: 20pt, field: "approver")

#qm.signature-field("Overridden", height: 20pt, field: "approver", tooltip: "Sign as approver")

#qm.form-field("Unbound", tooltip: "No schema field")

#qm.form-field("Silent")

#qm.form-field("Undescribed", field: "bare")

#qm.form-field("Cell", field: "classification.poc")

#qm.form-field("Level", field: "classification.value")

#for card in qm.data.at("$cards") {
  qm.form-field("Office", field: card.at("$path") + "office")
}
"#;

#[test]
fn tooltip_overrides_the_description_it_otherwise_inherits() {
    let quill = common::quill_with_plate(&common::yaml(SCHEMA), PLATE);
    let data = serde_json::json!({
        "approver": common::content("Col A. Smith"),
        "bare": common::content(""),
        "classification": { "value": "CUI", "poc": common::content("Capt B") },
        "$cards": [ { "$kind": "endorsement", "office": common::content("SAF/AA") } ],
    });
    let session = TypstBackend::default()
        .open(&quill, &data, common::test_date())
        .expect("open");
    let pdf = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("render")
        .artifacts[0]
        .bytes
        .clone();
    let (_, _, widgets) = quillmark_fixtures::acroform::parse(&pdf);
    let tooltip = |name: &str| {
        widgets[name]
            .get(b"TU")
            .ok()
            .map(|tu| String::from_utf8(tu.as_str().unwrap().to_vec()).unwrap())
    };

    assert_eq!(
        tooltip("Inherited").as_deref(),
        Some("Name of the approving official")
    );
    assert_eq!(tooltip("Overridden").as_deref(), Some("Sign as approver"));
    assert_eq!(tooltip("Unbound").as_deref(), Some("No schema field"));
    assert_eq!(tooltip("Silent"), None);
    assert_eq!(tooltip("Undescribed"), None);
    assert_eq!(tooltip("Cell").as_deref(), Some("CUI point of contact"));
    assert_eq!(tooltip("Level").as_deref(), Some("Banner marking"));
    assert_eq!(
        tooltip("Office").as_deref(),
        Some("Endorsing office symbol")
    );
}

#[test]
fn a_non_string_tooltip_is_an_assert() {
    let plate = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#qm.form-field("A", tooltip: [content])
"#;
    let quill = common::quill_with_plate(&common::yaml(SCHEMA), plate);
    let err =
        match TypstBackend::default().open(&quill, &serde_json::json!({}), common::test_date()) {
            Ok(_) => panic!("a content tooltip must fail the compile"),
            Err(err) => err.to_string(),
        };
    assert!(err.contains("tooltip must be a string"), "{err}");
}
