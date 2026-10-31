//! `LiveSession::coverage` over a real compile: what a plate's prints and
//! claims reach, and what no click does.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;

const SCHEMA: &str = r#"
main:
  fields:
    name: { type: enum, values: [Ada, Grace] }
    title: { type: enum, values: [Roster, Register] }
    stratifications:
      type: array
      items:
        type: object
        properties:
          year: { type: integer }
          rank: { type: enum, values: [Capt, Maj] }
"#;

/// A claim built inside a closure over computed rows reaches its cell, a bare
/// print of a value reaches nothing, and a row cell the plate never claims is
/// reported rather than hidden.
#[test]
fn coverage_names_what_claims_reach_and_what_nothing_does() {
    let plate = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 300pt, margin: 20pt)
#qm.data.name
#qm.ink(qm.data, "title")
#let rr = qm.data.stratifications.sorted(key: s => s.year)
#table(columns: 2, ..rr.map(s => (qm.region(s.at("$path") + "year")[#s.year], [#s.rank])).flatten())
"#;
    let session = TypstBackend::default()
        .open(
            &common::quill_with_plate(&common::yaml(SCHEMA), plate),
            &serde_json::json!({
                "name": "Ada",
                "title": "Roster",
                "stratifications": [
                    { "year": 2027, "rank": "Maj" },
                    { "year": 2026, "rank": "Capt" },
                ],
            }),
            common::test_date(),
        )
        .expect("open");
    let got: Vec<(String, Option<String>)> = session
        .coverage()
        .into_iter()
        .map(|c| (c.field, c.covered_by))
        .collect();
    let reached = |field: &str| Some(field.to_string());
    assert_eq!(
        got,
        [
            ("name".to_string(), None),
            ("title".to_string(), reached("title")),
            ("stratifications.0.year".to_string(), reached("stratifications.0.year")),
            ("stratifications.0.rank".to_string(), None),
            ("stratifications.1.year".to_string(), reached("stratifications.1.year")),
            ("stratifications.1.rank".to_string(), None),
        ]
    );
}
