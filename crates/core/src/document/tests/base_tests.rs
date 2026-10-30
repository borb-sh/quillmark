use quillmark_content::export::to_markdown_annotated;
use quillmark_content::import::from_markdown;

use crate::document::{Base, BaseError, Card, Document, EditError};
use crate::quill::{BoundParseError, QuillConfig};
use crate::writer::TypedWriter;

use super::parse;
use super::revise_tests::{anchor_ids, anchored};

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
      multiline: false
    note:
      type: text
      multiline: true
";

fn config() -> QuillConfig {
    QuillConfig::from_yaml(QUILL_YAML).expect("valid quill")
}

fn card_with_body(markdown: &str) -> Card {
    let mut card = Card::new("note").unwrap();
    let _ = card.revise_body(markdown).unwrap();
    card
}

fn current_of(err: EditError) -> (&'static str, String) {
    match err {
        EditError::Base(BaseError::Stale { current }) => ("stale", current),
        EditError::Base(BaseError::Conflict { current }) => ("conflict", current),
        other => panic!("not a base refusal: {other:?}"),
    }
}

#[test]
fn a_stale_hash_refuses_the_revise_and_names_the_hash_now() {
    let mut card = card_with_body("Alpha one.\n\nBeta two.");
    let read = card.body_hash();
    let _ = card.revise_body("Alpha one, edited.\n\nBeta two.").unwrap();
    let stored = card.body().clone();

    let err = card
        .revise_body_against(Base::Hash(&read), "Alpha one.\n\nBeta two, rewritten.")
        .unwrap_err();
    assert_eq!(current_of(err), ("stale", card.body_hash()));
    assert_eq!(card.body(), &stored);

    let fresh = card.body_hash();
    let _ = card
        .revise_body_against(Base::Hash(&fresh), "Alpha one, edited.\n\nBeta two, rewritten.")
        .unwrap();
    assert_eq!(card.body().text, "Alpha one, edited.\nBeta two, rewritten.");
}

#[test]
fn a_read_lands_the_writer_edit_beside_the_change_since_and_keeps_anchors() {
    let content = from_markdown("Alpha one.\n\nBeta two.\n\nGamma three.").unwrap().content;
    let mut card = Card::new("note").unwrap();
    card.overwrite_body(anchored(&content, "Beta", "c1"));
    let read = to_markdown_annotated(card.body()).markdown;
    assert!(read.contains("<qm-anchor"), "{read}");

    let _ = card.revise_body("Alpha one, edited.\n\nBeta two.\n\nGamma three.").unwrap();
    let revised = card
        .revise_body_against(Base::Read(&read), "Alpha one.\n\nBeta two.\n\nGamma three, rewritten.")
        .unwrap();
    assert_eq!(card.body().text, "Alpha one, edited.\nBeta two.\nGamma three, rewritten.");
    assert_eq!(anchor_ids(card.body()), ["c1"]);
    assert!(revised.dropped_anchors.is_empty());

    let stored = card.body().clone();
    let err = card
        .revise_body_against(Base::Read(&read), "Alpha one, mine.\n\nBeta two.\n\nGamma three.")
        .unwrap_err();
    assert_eq!(current_of(err), ("conflict", card.body_hash()));
    assert_eq!(card.body(), &stored);
}

#[test]
fn a_read_of_the_value_now_is_a_plain_revise() {
    let mut card = card_with_body("Alpha *one*.");
    let read = to_markdown_annotated(card.body()).markdown;
    let mut plain = card.clone();
    let _ = plain.revise_body("Alpha _one_, then <span>two</span>.").unwrap();
    let revised = card
        .revise_body_against(Base::Read(&read), "Alpha _one_, then <span>two</span>.")
        .unwrap();
    assert_eq!(card.body(), plain.body());
    assert_eq!(revised.warnings.len(), 1);
}

#[test]
fn a_text_field_merges_its_literal_text() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nnote: \"a *b*\\n\\nc\\n\\nd\"\n~~~\n");
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w.revise_field("note", "a *b*, edited\n\nc\n\nd").unwrap();
    let _ = w
        .revise_field_against("note", Base::Read("a *b*\n\nc\n\nd"), "a *b*\n\nc\n\nd, too")
        .unwrap();
    let note = doc.main().payload().get("note").unwrap().as_json().clone();
    assert_eq!(note, "a *b*, edited\n\nc\n\nd, too");

    let read = doc.main().field_hash("note").unwrap();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w.revise_field("note", "x").unwrap();
    let err = w.revise_field_against("note", Base::Hash(&read), "y").unwrap_err();
    assert_eq!(current_of(err), ("stale", doc.main().field_hash("note").unwrap()));
}

#[test]
fn a_document_revise_merges_a_read_and_refuses_a_stale_hash() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nAlpha one.\n\nBeta two.\n");
    let read = doc.to_markdown_annotated().markdown;
    let hash = doc.hash();

    let _ = doc.main_mut().revise_body("Alpha one.\n\nBeta two, edited.").unwrap();
    let mut w = TypedWriter::new(&config, &mut doc);
    let err = w
        .revise_document_against(Base::Hash(&hash), &read.replace("Hello", "Goodbye"))
        .unwrap_err();
    assert!(matches!(&err, BoundParseError::Base(BaseError::Stale { .. })), "{err:?}");
    assert_eq!(err.to_diagnostics()[0].code.as_deref(), Some("edit::stale_base"));

    let _ = w
        .revise_document_against(Base::Read(&read), &read.replace("Hello", "Goodbye"))
        .unwrap();
    let expected: Document = parse(
        "~~~\n$quill: memo@1.0.0\nsubject: Goodbye\n~~~\n\nAlpha one.\n\nBeta two, edited.\n",
    );
    assert_eq!(doc.to_markdown(), expected.to_markdown());
}
