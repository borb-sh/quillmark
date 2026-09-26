//! Blank-fill of *nested* nulls in the plate projection. The authored/default/
//! blank ladder itself is owned by `quillmark_core::quill::resolved`; what only
//! shows up through a loaded quill is recursion into object properties and
//! array elements.

use quillmark::Document;
use std::fs;
use tempfile::TempDir;

mod common;

fn create_test_quill(temp_dir: &TempDir, quill_yaml: &str) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(quill_path.join("Quill.yaml"), quill_yaml).unwrap();
    fs::write(
        quill_path.join("plate.typ"),
        "#import \"@local/quillmark-helper:0.1.0\": data\n= Document\n#data",
    )
    .unwrap();
    quill_path
}

#[test]
fn test_nested_null_blank_fills_in_plate() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  description: "Nested null blank-fill"

main:
  fields:
    addr:
      type: object
      properties:
        street: { type: string }
        city: { type: string }
    tags:
      type: array
      items: { type: string }
"#,
    );
    let quill = quillmark::quill_from_path(&quill_path).expect("from_path failed");
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
        addr.get("street").and_then(|v| v.as_str()),
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
