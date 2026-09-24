//! An optional cell (`type: t?`) crosses the whole render path as Typst `none`.
//! The plate asserts, so a render that succeeds is the plate having seen what
//! each case states.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use std::fs;
use tempfile::TempDir;

const QUILL_YAML: &str = r#"quill:
  name: optional_quill
  version: "1.0"
  backend: typst
  description: Optional cells reach the plate as none

typst:
  plate_file: plate.typ

main:
  fields:
    quorum: { type: integer? }
    attendees: { type: array?, items: { type: string } }
    adjourned: { type: date? }
    minutes: { type: richtext? }
    tally:
      type: object
      properties:
        votes_for: { type: integer? }
        votes_against: { type: integer }
"#;

fn render(plate: &str, fields: &str) -> Result<(), String> {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = temp_dir.path().join("optional_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(quill_path.join("Quill.yaml"), QUILL_YAML).unwrap();
    fs::write(
        quill_path.join("plate.typ"),
        format!(
            "#import \"@local/quillmark-helper:0.1.0\": data\n{plate}\nok\n"
        ),
    )
    .unwrap();
    let quill = quillmark::quill_from_path(&quill_path).expect("load quill");
    let md = format!("~~~card-yaml\n$quill: optional_quill\n$kind: main\n{fields}~~~\n");
    let parsed = Document::parse(&md).expect("parse").document;
    Quillmark::new()
        .open(&quill, &parsed)
        .and_then(|session| {
            session.render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        })
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn an_unanswered_optional_cell_is_none() {
    let plate = r#"
#assert.eq(data.quorum, none)
#assert.eq(data.attendees, none)
#assert.eq(data.adjourned, none)
#assert.eq(data.minutes, none)
#assert.eq(data.tally, (votes_against: 0, votes_for: none))
#assert.eq("n" + data.quorum, "n")
"#;
    render(plate, "").expect("the plate saw every unanswered cell as none");
}

#[test]
fn an_authored_zero_is_an_answer() {
    let plate = r#"
#assert.eq(data.quorum, 0)
#assert.eq(data.attendees, ())
#assert.eq(data.adjourned.year(), 2026)
"#;
    render(plate, "quorum: 0\nattendees: []\nadjourned: 2026-09-24\n")
        .expect("the plate saw each authored value as written");
}
