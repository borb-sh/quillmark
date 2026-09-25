//! `today` in a `date` cell: stored as written, rendered as the date the host
//! supplies to the compile.

use crate::document::Document;
use crate::quill::{quill_from_yaml, CalendarDate, FieldSource, QuillConfig};
use serde_json::json;

const QUILL_YAML: &str = r#"
quill:
  name: today_probe
  version: "0.1.0"
  backend: typst
  description: Today probe

main:
  fields:
    issued: { type: date, default: today }
    signed: { type: date }
    stamps: { type: array, items: { type: date } }
    review:
      type: object
      properties:
        due: { type: date }
    revoked: { type: date? }
"#;

fn document() -> Document {
    let markdown = "~~~\n$quill: today_probe@0.1.0\n$kind: main\n\
                    signed: today\nstamps: [today, 2026-01-02]\nreview: { due: today }\n~~~\n";
    Document::parse(markdown).expect("parses").document
}

fn day() -> CalendarDate {
    "2026-03-14".parse().expect("a date")
}

#[test]
fn a_today_cell_renders_the_supplied_date_at_every_depth() {
    let quill = quill_from_yaml(QUILL_YAML);
    let doc = document();
    let plate = quill.compile_data(&doc, day()).expect("compiles");

    assert_eq!(plate["issued"], json!("2026-03-14"));
    assert_eq!(plate["signed"], json!("2026-03-14"));
    assert_eq!(plate["stamps"], json!(["2026-03-14", "2026-01-02"]));
    assert_eq!(plate["review"], json!({ "due": "2026-03-14" }));
    assert_eq!(plate["revoked"], json!(null));

    assert_eq!(
        doc.main().payload().get("signed").and_then(|v| v.as_str()),
        Some("today"),
        "the document keeps the keyword"
    );

    let resolved = quill.resolve(&doc, day());
    for row in &resolved.main.fields {
        assert_eq!(row.value.as_json(), &plate[&row.name], "{}", row.name);
    }
    let source = |name: &str| {
        resolved.main.fields.iter().find(|r| r.name == name).unwrap().source
    };
    assert_eq!(source("issued"), FieldSource::Default);
    assert_eq!(source("signed"), FieldSource::Authored);
}

#[test]
fn today_is_a_date_value_only() {
    let with_default = |field: &str| {
        QuillConfig::from_yaml(&format!(
            "quill:\n  name: q\n  version: \"1.0\"\n  backend: typst\n  description: q\n\
             main:\n  fields:\n    f: {field}\n"
        ))
    };
    assert!(with_default("{ type: date, example: today }").is_ok());
    assert!(with_default("{ type: datetime, default: today }").is_err());
    assert!(with_default("{ type: string, default: today }").is_ok());

    assert!("today".parse::<CalendarDate>().is_err());
    assert!("2026-02-30".parse::<CalendarDate>().is_err());
    assert_eq!(day().to_string(), "2026-03-14");
}
