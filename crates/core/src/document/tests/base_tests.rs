use crate::document::{Card, EditError, StaleBase, Target, content_hash, value_hash};
use crate::quill::{BoundParseError, QuillConfig};
use crate::writer::TypedWriter;
use serde_json::json;

use super::parse;

const QUILL_YAML: &str = "\
quill:
  name: memo
  backend: typst
  version: 1.0.0
  description: Base test quill
main:
  fields:
    subject:
      type: richtext
    note:
      type: text
      shape: lines
    on:
      type: date
";

fn config() -> QuillConfig {
    QuillConfig::from_yaml(QUILL_YAML).expect("valid quill")
}

fn card_with_body(markdown: &str) -> Card {
    let mut card = Card::new("note").unwrap();
    let _ = card.revise_body(markdown).unwrap();
    card
}

fn current_of(err: EditError) -> String {
    match err {
        EditError::StaleBase(StaleBase { current, .. }) => current,
        other => panic!("not a base refusal: {other:?}"),
    }
}

#[test]
fn a_body_revise_lands_under_the_hash_now_and_refuses_a_stale_one() {
    let mut card = card_with_body("Alpha one.\n\nBeta two.");
    let read = card.body_hash();
    let _ = card
        .revise_body_against(&read, "Alpha one, edited.\n\nBeta two.")
        .unwrap();
    assert_eq!(card.body().text, "Alpha one, edited.\nBeta two.");

    let stored = card.body().clone();
    let err = card
        .revise_body_against(&read, "Alpha one.\n\nBeta two, rewritten.")
        .unwrap_err();
    assert_eq!(err.code(), "edit::stale_base");
    assert_eq!(current_of(err), card.body_hash());
    assert_eq!(card.body(), &stored);
}

#[test]
fn a_text_field_under_a_stale_hash_is_refused_at_its_path() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nnote: \"a *b*\"\n~~~\n");
    let mut w = TypedWriter::new(&config, &mut doc);
    let read = w.hash_of("note").unwrap();
    let _ = w.revise(Target::from("note"), "x", Some(&read)).unwrap();
    let err = w
        .revise(Target::from("note"), "y", Some(&read))
        .unwrap_err();
    assert_eq!(
        err.doc_path(&crate::path::DocPath::main())
            .map(|p| p.to_string()),
        Some("main.note".into())
    );
    assert_eq!(current_of(err), w.hash_of("note").unwrap());
    assert_eq!(
        crate::quill::text_of(doc.main().payload().get("note").unwrap().as_json()),
        "x"
    );
}

/// A revise of a field neither `text` nor `richtext` fails as it does with no
/// base, never as a stale read of a fresh hash.
#[test]
fn a_revise_of_a_date_under_a_fresh_hash_fails_as_a_plain_revise() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\non: 2024-01-01\n~~~\n");
    let mut w = TypedWriter::new(&config, &mut doc);
    let plain = w
        .revise(Target::from("on"), "2024-02-02", None)
        .unwrap_err()
        .code();
    let read = w.hash_of("on").unwrap();
    let err = w
        .revise(Target::from("on"), "2024-02-02", Some(&read))
        .unwrap_err();
    assert_eq!(err.code(), plain);
    assert_ne!(plain, "edit::stale_base");
}

#[test]
fn a_richtext_field_hashes_its_content_however_it_rests() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nsubject: Hello *world*\n~~~\n");
    let as_string = doc.main().hash_of("subject").unwrap();
    assert_eq!(
        as_string,
        content_hash(
            &quillmark_content::markdown::from_markdown("Hello *world*")
                .unwrap()
                .content
        )
    );
    let mut w = TypedWriter::new(&config, &mut doc);
    assert_eq!(w.hash_of("subject").unwrap(), as_string);
    let _ = w.revise(Target::Body, "Body.", None).unwrap();
    assert!(config.conform(&mut doc).unwrap().is_empty());
    assert_eq!(doc.main().hash_of("subject").unwrap(), as_string);
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise(
            Target::from("subject"),
            "Hello _world_, hi",
            Some(&as_string),
        )
        .unwrap();
    assert_eq!(
        w.hash_of("missing").unwrap_err().code(),
        "edit::unknown_field"
    );
}

#[test]
fn a_hash_ignores_key_order_and_a_typed_document_hash_ignores_conforming() {
    assert_eq!(
        value_hash(&json!({"b": [{"y": 1, "x": 2}], "a": null})),
        value_hash(&json!({"a": null, "b": [{"x": 2, "y": 1}]}))
    );
    assert_ne!(value_hash(&json!({"a": 1})), value_hash(&json!({"a": 2})));
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nsubject: Hello *world*\n~~~\n\nBody.\n");
    let before = TypedWriter::new(&config, &mut doc).hash().unwrap();
    let stored_before = doc.hash();
    config.conform(&mut doc).unwrap();
    assert_eq!(TypedWriter::new(&config, &mut doc).hash().unwrap(), before);
    assert_ne!(doc.hash(), stored_before);
}

#[test]
fn a_document_revise_lands_under_the_hash_now_and_refuses_a_stale_one() {
    let config = config();
    let mut doc =
        parse("~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nAlpha one.\n\nBeta two.\n");
    let read = doc.to_markdown_annotated().markdown;
    let mut w = TypedWriter::new(&config, &mut doc);
    let hash = w.hash().unwrap();

    let _ = w
        .revise_document_against(&hash, &read.replace("Hello", "Goodbye"))
        .unwrap();
    let stored = doc.to_markdown();
    assert!(stored.contains("subject: Goodbye"), "{stored}");

    let mut w = TypedWriter::new(&config, &mut doc);
    let err = w
        .revise_document_against(&hash, &read.replace("Beta two.", "Beta two, edited."))
        .unwrap_err();
    assert!(
        matches!(&err, BoundParseError::StaleBase(StaleBase { .. })),
        "{err:?}"
    );
    assert_eq!(
        err.to_diagnostics()[0].code.as_deref(),
        Some("edit::stale_base")
    );
    assert_eq!(doc.to_markdown(), stored);
}
