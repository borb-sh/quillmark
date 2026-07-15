use quillmark_core::quill::QuillConfig;

#[test]
fn test_markdown_type_is_a_load_error() {
    // `type: markdown` was a deprecated alias for block `richtext`; PR-G retires
    // it outright. A Quill.yaml that still declares it fails to load — no silent
    // alias, no parallel accepted spelling.
    let err = QuillConfig::from_yaml(
        r#"
quill:
  name: markdown_schema
  version: "1.0"
  backend: typst
  description: markdown schema test

main:
  fields:
    description:
      type: markdown
"#,
    )
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("markdown"),
        "load error should name the offending type: {msg}"
    );
}

#[test]
fn test_richtext_field_schema_emission() {
    let config = QuillConfig::from_yaml(
        r#"
quill:
  name: richtext_schema
  version: "1.0"
  backend: typst
  description: richtext schema test

main:
  fields:
    description:
      type: richtext
"#,
    )
    .unwrap();

    let yaml = config.schema_yaml().unwrap();
    let value: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        value
            .get("main")
            .and_then(|v| v.get("fields"))
            .and_then(|v| v.get("description"))
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("richtext")
    );
}
