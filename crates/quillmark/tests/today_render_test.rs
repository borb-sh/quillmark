//! The render date reaches a `today` field and a plate's `datetime.today()` as
//! one value. The plate asserts, so a render that succeeds is the plate having
//! seen what each case states.

#![cfg(feature = "typst")]

use quillmark::{CalendarDate, Document, Engine, OutputFormat, RenderOptions};

mod common;

const QUILL_YAML: &str = r#"quill:
  name: today_quill
  version: "1.0.0"
  backend: typst
  description: A today field and the plate's date agree

typst:
  plate_file: plate.typ

main:
  fields:
    issued: { type: date, default: today }
    signed: { type: date }
"#;

fn quill(plate: &str) -> quillmark::Quill {
    let plate = format!("#import \"@local/quillmark-helper:0.2.0\" as qm\n{plate}\nok\n");
    common::quill_from_files(&[("Quill.yaml", QUILL_YAML), ("plate.typ", &plate)])
}

fn doc(fields: &str) -> Document {
    let md = format!("~~~card-yaml\n$quill: today_quill\n$kind: main\n{fields}~~~\n");
    Document::parse(&md).expect("parse").document
}

fn svg() -> RenderOptions {
    RenderOptions::default().with_output_format(OutputFormat::Svg)
}

#[test]
fn the_field_and_the_plate_read_the_supplied_date() {
    let quill = quill(
        r#"
#assert.eq(qm.data.issued, datetime.today())
#assert.eq(qm.data.issued, datetime(year: 2026, month: 3, day: 14))
#assert.eq(qm.data.signed, none)
"#,
    );
    let today: CalendarDate = "2026-03-14".parse().unwrap();
    let mut session = Engine::new()
        .open(&quill, &doc(""), today)
        .expect("the plate saw the supplied date");
    session.render(&svg()).expect("renders");

    session
        .update(&doc("issued: today\n"))
        .expect("an update compiles against the session's date");
}
