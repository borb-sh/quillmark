//! A `today` date and a plate's `datetime.today()` read the same clock.

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use std::fs;
use tempfile::TempDir;

#[test]
fn a_today_date_renders_as_the_plates_today() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = temp_dir.path().join("dated");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        "quill:\n  name: dated\n  version: \"1.0\"\n  backend: typst\n  description: dated\n\
         typst:\n  plate_file: plate.typ\n\
         main:\n  fields:\n    issued: { type: date, default: today }\n",
    )
    .unwrap();
    fs::write(
        quill_path.join("plate.typ"),
        "#import \"@local/quillmark-helper:0.1.0\": data\n\
         #assert.eq(data.issued, datetime.today())\n\
         Issued\n",
    )
    .unwrap();
    let quill = quillmark::quill_from_path(&quill_path).unwrap();
    let doc = Document::parse("~~~\n$quill: dated\n$kind: main\n~~~\n")
        .unwrap()
        .document;

    Quillmark::new()
        .render(
            &quill,
            &doc,
            &RenderOptions::default().with_output_format(OutputFormat::Svg),
        )
        .expect("the field and the plate agree on today");
}
