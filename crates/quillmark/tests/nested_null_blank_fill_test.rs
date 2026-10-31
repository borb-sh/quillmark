//! Blank-fill of *nested* nulls in the plate projection. The authored/default/
//! blank ladder itself is owned by `quillmark_core::quill::resolved`; what only
//! shows up through a loaded quill is recursion into object properties and
//! array elements.

use quillmark::Document;

mod common;

#[test]
fn nested_nulls_blank_fill_in_the_plate_data() {
    let quill_yaml = r#"quill:
  name: "test_quill"
  version: "1.0.0"
  backend: "typst"
  description: "Nested null blank-fill"

main:
  fields:
    addr:
      type: object
      properties:
        street: { type: text }
        city: { type: text }
    tags:
      type: array
      items: { type: text }
"#;
    let quill = common::quill_from_files(&[
        ("Quill.yaml", quill_yaml),
        (
            "plate.typ",
            "#import \"@local/quillmark-helper:0.2.0\" as qm\n= Document\n#qm.data",
        ),
    ]);
    let md = "~~~card-yaml\n$quill: test_quill\n$kind: main\n\
              addr:\n  street:\n  city: Pittsburgh\n\
              tags:\n  - alpha\n  - null\n  - gamma\n~~~\n\nbody\n";
    let parsed = Document::parse(md).expect("parse failed").document;
    let data = quill
        .compile_data(&parsed, common::test_date())
        .expect("compile_data should succeed");

    let addr = data
        .get("addr")
        .and_then(|v| v.as_object())
        .expect("addr object");
    assert_eq!(
        addr.get("street")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str()),
        Some(""),
        "null nested property must blank-fill, not leak null: {data}"
    );
    let tags = data
        .get("tags")
        .and_then(|v| v.as_array())
        .expect("tags array");
    assert!(
        !tags.iter().any(|v| v.is_null()),
        "null array element must not leak into the plate: {data}"
    );
}
