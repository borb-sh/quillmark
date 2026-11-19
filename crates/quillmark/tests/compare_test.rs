//! `Quill::compare` against the move it describes: where the delta says a
//! document carries, retargeting `$quill` is the whole move, there and back.

use std::str::FromStr;

use quillmark::{Document, Quill, QuillReference, Severity};

mod common;

const PLAIN: &str = r#"quill:
  name: resume
  version: "1.2.0"
  backend: typst
  description: A resume
main:
  fields:
    name: { type: text }
    level: { type: enum, values: [junior, senior] }
card_kinds:
  job:
    fields:
      employer: { type: text }
"#;

const MODERN: &str = r#"quill:
  name: resume_modern
  version: "0.1.0"
  backend: typst
  description: A resume with an accent
main:
  fields:
    name: { type: text, title: Full name }
    level: { type: enum, values: [junior, senior, principal] }
    accent: { type: text, default: teal }
card_kinds:
  job:
    fields:
      employer: { type: text }
      logo: { type: text }
"#;

const DOCUMENT: &str = "~~~\n$quill: resume@1.2\nname: Ada\nlevel: senior\n~~~\n\n\
                        ~~~\n$kind: job\nemployer: Analytical Engines\n~~~\n";

fn problems(quill: &Quill, document: &Document) -> Vec<String> {
    quill
        .validate(document)
        .into_iter()
        .map(|d| d.message)
        .collect()
}

#[test]
fn a_document_the_delta_carries_moves_and_returns_unchanged() {
    let plain = common::quill_from_files(&[("Quill.yaml", PLAIN)]);
    let modern = common::quill_from_files(&[("Quill.yaml", MODERN)]);
    assert!(Quill::compare(&plain, &modern).carries());
    assert!(!Quill::compare(&modern, &plain).carries());

    let mut document = plain.parse(DOCUMENT).unwrap().document;
    let written = document.to_markdown();

    document.set_quill_ref(QuillReference::from_str("resume_modern@0.1").unwrap());
    let conformed = modern.conform(&mut document).unwrap();
    assert!(conformed.iter().all(|d| d.severity != Severity::Error));
    assert_eq!(problems(&modern, &document), Vec::<String>::new());

    document.set_quill_ref(QuillReference::from_str("resume@1.2").unwrap());
    plain.conform(&mut document).unwrap();
    assert_eq!(document.to_markdown(), written);
}
