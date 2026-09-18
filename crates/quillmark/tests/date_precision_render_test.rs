//! A `date` narrower than a day, end to end: the plate reads the components the
//! document knows, and `display` places ink for them without the backend
//! fabricating the ones it does not.
//!
//! Engine altitude rather than `helper.rs`: the generated closure is Typst
//! source, so only a compile says it parses.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use std::fs;
use tempfile::TempDir;

fn precision_quill(temp_dir: &TempDir) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("precision_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        r#"quill:
  name: "precision_quill"
  version: "1.0"
  backend: "typst"
  description: "date precision lowering"

typst:
  plate_file: plate.typ

main:
  body:
    enabled: false
  fields:
    since:
      type: date
      precision: month
    class_of:
      type: date
      precision: year
    signed_on:
      type: date
"#,
    )
    .unwrap();
    fs::write(
        quill_path.join("plate.typ"),
        "#import \"@local/quillmark-helper:0.1.0\": data, display\n\
         #set page(width: 612pt, height: 792pt, margin: 72pt)\n\
         // The partial date is the dict of what it carries: no day to read.\n\
         #data.since.year - #data.since.month / #data.class_of.year\n\
         // The floor pattern prints the declared components alone…\n\
         #display(\"since\") | #display(\"class_of\")\n\
         // …and a pattern the plate names is the plate's.\n\
         #display(\"since\", \"[month repr:long] [year]\")\n\
         #data.signed_on.display()\n",
    )
    .unwrap();
    quill_path
}

#[test]
fn a_partial_date_lowers_its_components_and_displays_at_its_precision() {
    let temp_dir = TempDir::new().unwrap();
    let quill = quillmark::quill_from_path(precision_quill(&temp_dir)).expect("load quill");
    let md = "~~~card-yaml\n$quill: precision_quill\n$kind: main\n\
              since: \"2024-08\"\nclass_of: \"2026\"\nsigned_on: \"2024-08-15\"\n~~~\n";
    let parsed = Document::parse(md).expect("parse").document;

    let plate = quill.compile_data(&parsed).expect("compile_data");
    assert_eq!(plate["since"], "2024-08", "stored verbatim");

    let svg = Quillmark::new()
        .open(&quill, &parsed)
        .expect("open")
        .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        .expect("a partial date must compile");
    assert_eq!(svg.artifacts.len(), 1);
}

/// The grammar is the precision's: a value carrying more than the field
/// declares is as malformed as one carrying less, so nothing has to guess which
/// components are real.
#[test]
fn a_value_off_its_declared_precision_is_a_format_violation() {
    let temp_dir = TempDir::new().unwrap();
    let quill = quillmark::quill_from_path(precision_quill(&temp_dir)).expect("load quill");
    let md = "~~~card-yaml\n$quill: precision_quill\n$kind: main\nsince: \"2024-08-15\"\n~~~\n";
    let parsed = Document::parse(md).expect("parse").document;

    let diag = quill
        .validate(&parsed)
        .into_iter()
        .find(|d| d.path.as_deref() == Some("main.since"))
        .expect("a month-precision field rejects a full date");
    assert_eq!(diag.code.as_deref(), Some("validation::format_violation"));
    assert_eq!(
        diag.args.get("format"),
        Some(&serde_json::json!("date(month)"))
    );
}
