use quillmark_content::export::to_markdown_annotated;
use quillmark_content::import::from_markdown;

use crate::document::{content_hash, value_hash, Base, BaseError, Card, Document, EditError};
use crate::quill::{BoundParseError, QuillConfig};
use serde_json::json;
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
card_kinds:
  item:
    fields:
      summary:
        type: richtext
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
        EditError::Base(BaseError::Stale { current, .. }) => ("stale", current),
        EditError::Base(BaseError::Conflict { current, .. }) => ("conflict", current),
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

    let mut w = TypedWriter::new(&config, &mut doc);
    let read = w.field_hash("note").unwrap();
    let _ = w.revise_field("note", "x").unwrap();
    let err = w.revise_field_against("note", Base::Hash(&read), "y").unwrap_err();
    assert_eq!(err.doc_path(&crate::path::DocPath::main()).map(|p| p.to_string()), Some("main.note".into()));
    let w = TypedWriter::new(&config, &mut doc);
    assert_eq!(current_of(err), ("stale", w.field_hash("note").unwrap()));
}

#[test]
fn a_richtext_field_hashes_its_content_however_it_rests() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nsubject: Hello *world*\n~~~\n");
    let as_string = doc.main().field_hash("subject").unwrap();
    assert_eq!(as_string, content_hash(&from_markdown("Hello *world*").unwrap().content));
    let mut w = TypedWriter::new(&config, &mut doc);
    assert_eq!(w.field_hash("subject").unwrap(), as_string);
    let _ = w.revise_body("Body.").unwrap();
    assert!(config.conform(&mut doc).unwrap().is_empty());
    assert_eq!(doc.main().field_hash("subject").unwrap(), as_string);
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w.revise_field_against("subject", Base::Hash(&as_string), "Hello _world_, hi").unwrap();
    assert_eq!(w.field_hash("missing").unwrap_err().code(), "edit::unknown_field");
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
fn an_annotated_document_read_with_a_field_anchor_is_a_fresh_read() {
    let config = config();
    let mut doc = parse(
        "~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nBody.\n\n~~~\n$kind: item\nsummary: |\n  First para.\n\n  Second para here.\n~~~\n",
    );
    config.conform(&mut doc).unwrap();
    let summary = doc
        .card(0)
        .unwrap()
        .field_content("summary", crate::document::Codec::Richtext)
        .unwrap()
        .unwrap();
    let mut card = doc.card_mut(0).unwrap();
    card.overwrite_field("summary", anchored(&summary, "Second", "s1")).unwrap();
    let read = doc.to_markdown_annotated().markdown;
    assert!(read.contains("<qm-anchor"), "{read}");

    let mut w = TypedWriter::new(&config, &mut doc);
    let revised = w
        .revise_document_against(Base::Read(&read), &read.replace("Second para here", "Second paragraph here"))
        .unwrap();
    assert!(revised.dropped_anchors.is_empty());
    let summary = doc.card(0).unwrap().field_content("summary", crate::document::Codec::Richtext).unwrap().unwrap();
    assert!(summary.text.contains("Second paragraph here"));
    assert_eq!(anchor_ids(&summary), ["s1"]);
}

#[test]
fn a_merge_that_does_not_parse_is_a_conflict_and_warnings_follow_their_card() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nBody.\n\n~~~\n$kind: item\n~~~\n\nOne.\n");
    let read = doc.to_markdown();

    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w.revise_document(&read.replace("subject: Hello\n", "subject: Hello\nnote: theirs\n")).unwrap();
    let err = w
        .revise_document_against(Base::Read(&read), &read.replace("$quill: memo@1.0.0\n", "$quill: memo@1.0.0\nnote: mine\n"))
        .unwrap_err();
    assert!(matches!(err, BoundParseError::Base(BaseError::Conflict { .. })), "{err:?}");

    let mut doc = parse(
        "~~~\n$quill: memo@1.0.0\n~~~\n\nBody.\n\n~~~\n$kind: item\n~~~\n\nOne.\n\n~~~\n$kind: item\n~~~\n\nTwo.\n",
    );
    let read = doc.to_markdown();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise_document(&read.replace("Body.\n", "Body.\n\n~~~\n$kind: item\n~~~\n\nZero.\n"))
        .unwrap();
    let revised = w
        .revise_document_against(Base::Read(&read), &read.replace("Two.", "Two <span>x</span>."))
        .unwrap();
    assert_eq!(
        revised.warnings.iter().map(|w| w.path.as_deref()).collect::<Vec<_>>(),
        [Some("cards.item[2].body")]
    );
    assert_eq!(doc.card(2).unwrap().body().text, "Two x.");
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

/// A merge compares the three sides with their islands that have no markdown
/// as tokens, so a token the writer kept survives a change made since its
/// read, in a body and in a document.
/// A plain read of a value whose markup loses an island is still a read of
/// it now: the writer's own markdown lands, rather than a merge that could
/// only conflict on the island's line.
#[test]
fn a_plain_read_of_an_unchanged_value_writes_the_writers_own() {
    let mut card = Card::new("note").unwrap();
    let mut body = from_markdown("Lead.\n\n| h |\n| --- |\n| cell |").unwrap().content.into_content();
    body.islands[0].props["note"] = "kept".into();
    card.overwrite_body(body.into_normalized());
    let read = card.body_markdown();
    let edited = read.replace("cell", "cell, edited");
    let revised = card.revise_body_against(Base::Read(&read), &edited).unwrap();
    assert!(revised.warnings.is_empty(), "{:?}", revised.warnings);
    assert_eq!(card.body_markdown(), edited);

    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\n~~~\n\nLead.\n");
    doc.main_mut().overwrite_body(card.body().clone());
    let read = doc.to_markdown();
    let mut w = TypedWriter::new(&config, &mut doc);
    w.revise_document_against(Base::Read(&read), &read.replace("edited", "edited twice")).unwrap();
    assert!(doc.main().body_markdown().contains("edited twice"));
}

#[test]
fn a_token_survives_a_merge() {
    let mut card = Card::new("note").unwrap();
    let body = super::revise_tests::titled(&from_markdown("Alpha ![a](u.png).\n\nBeta two.").unwrap().content);
    card.overwrite_body(body.clone());
    let read = to_markdown_annotated(card.body()).markdown;
    assert_eq!(read, "Alpha <qm-island ref=\"isl-0\"></qm-island>.\n\nBeta two.");

    let _ = card.revise_body(read.replace("Beta two.", "Beta two, theirs.")).unwrap();
    let revised = card
        .revise_body_against(Base::Read(&read), read.replace("Alpha", "Alpha, mine,"))
        .unwrap();
    assert!(revised.warnings.is_empty(), "{:?}", revised.warnings);
    assert_eq!(card.body().text, "Alpha, mine, \u{FFFC}.\nBeta two, theirs.");
    assert_eq!(card.body().islands, body.islands);

    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\n~~~\n\nAlpha ![a](u.png).\n\nBeta two.\n");
    doc.main_mut().overwrite_body(body.clone());
    let read = doc.to_markdown_annotated().markdown;
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w.revise_document(&read.replace("Beta two.", "Beta two, theirs.")).unwrap();
    let revised = w
        .revise_document_against(Base::Read(&read), &read.replace("Alpha", "Alpha, mine,"))
        .unwrap();
    assert!(revised.warnings.is_empty(), "{:?}", revised.warnings);
    assert_eq!(doc.main().body().text, "Alpha, mine, \u{FFFC}.\nBeta two, theirs.");
    assert_eq!(doc.main().body().islands, body.islands);
}
