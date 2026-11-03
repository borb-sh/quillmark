use quillmark_content::lines::export::to_markdown_annotated;
use quillmark_content::lines::import::from_markdown;

use crate::document::{
    Base, BaseError, Card, Document, DocumentBase, EditError, MergeConflict, content_hash, value_hash,
};
use crate::quill::{BoundParseError, QuillConfig};
use crate::writer::TypedWriter;
use serde_json::json;

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
    on:
      type: date
    jobs:
      type: array
      items:
        type: object
        properties:
          title: { type: text }
          dates: { type: text }
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
fn a_stale_hash_rebases_the_revise_and_an_unknown_one_names_the_hash_now() {
    let mut card = card_with_body("Alpha one.\n\nBeta two.");
    let read = card.body_hash();
    let _ = card.revise_body("Alpha one, edited.\n\nBeta two.").unwrap();

    let receipt = card
        .revise_body_against(Base::Hash(&read), "Alpha one.\n\nBeta two, rewritten.")
        .unwrap();
    assert!(receipt.rebased);
    assert_eq!(card.body_lines().text, "Alpha one, edited.\nBeta two, rewritten.");

    let stored = card.body_lines().clone();
    let err = card
        .revise_body_against(Base::Hash("not-a-read"), "Alpha.")
        .unwrap_err();
    assert_eq!(current_of(err), ("stale", card.body_hash()));
    assert_eq!(card.body_lines(), &stored);
}

#[test]
fn a_read_lands_the_writer_edit_beside_the_change_since_and_keeps_anchors() {
    let content = from_markdown("Alpha one.\n\nBeta two.\n\nGamma three.")
        .unwrap()
        .content;
    let mut card = Card::new("note").unwrap();
    card.overwrite_body(anchored(&content, "Beta", "c1"));
    let read = to_markdown_annotated(card.body_lines()).markdown;
    let base = card.read(crate::document::Target::Body).unwrap();
    assert!(read.contains("<qm-anchor"), "{read}");

    let _ = card
        .revise_body("Alpha one, edited.\n\nBeta two.\n\nGamma three.")
        .unwrap();
    let revised = card
        .revise_body_against(
            Base::Content(&base),
            "Alpha one.\n\nBeta two.\n\nGamma three, rewritten.",
        )
        .unwrap();
    assert_eq!(
        card.body_lines().text,
        "Alpha one, edited.\nBeta two.\nGamma three, rewritten."
    );
    assert_eq!(anchor_ids(card.body_lines()), ["c1"]);
    assert!(revised.dropped_anchors.is_empty());

    let revised = card
        .revise_body_against(
            Base::Content(&base),
            "Alpha uno!\n\nBeta two.\n\nGamma three.",
        )
        .unwrap();
    assert!(revised.rebased);
    let text = card.body_lines().text.clone();
    assert!(text.contains("uno!") && text.contains("edited") && text.contains("rewritten"), "{text}");
    assert_eq!(anchor_ids(card.body_lines()), ["c1"]);
}

#[test]
fn a_read_of_the_value_now_is_a_plain_revise() {
    let mut card = card_with_body("Alpha *one*.");
    let base = card.read(crate::document::Target::Body).unwrap();
    let mut plain = card.clone();
    let _ = plain
        .revise_body("Alpha _one_, then <span>two</span>.")
        .unwrap();
    let revised = card
        .revise_body_against(Base::Content(&base), "Alpha _one_, then <span>two</span>.")
        .unwrap();
    assert_eq!(card.body_lines(), plain.body_lines());
    assert_eq!(revised.warnings.len(), 1);
}

#[test]
fn a_text_field_merges_its_literal_text() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nnote: \"a *b*\\n\\nc\\n\\nd\"\n~~~\n");
    let base = doc.main().read("note").unwrap();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w.revise(crate::document::Target::from("note"), "a *b*, edited\n\nc\n\nd", None).unwrap();
    let _ = w
        .revise(crate::document::Target::from("note"), "a *b*\n\nc\n\nd, too", Some(Base::Content(&base)))
        .unwrap();
    let note = doc.main().payload().get("note").unwrap().as_json().clone();
    assert_eq!(crate::quill::text_of(&note), "a *b*, edited\n\nc\n\nd, too");

    let mut w = TypedWriter::new(&config, &mut doc);
    let read = w.hash_of("note").unwrap();
    let _ = w.revise(crate::document::Target::from("note"), "x", None).unwrap();
    let _ = w.revise(crate::document::Target::from("note"), "y", Some(Base::Hash(&read))).unwrap();
    let note = doc.main().payload().get("note").unwrap().as_json().clone();
    assert_eq!(crate::quill::text_of(&note), "yx");

    let mut w = TypedWriter::new(&config, &mut doc);
    let err = w
        .revise(crate::document::Target::from("note"), "z", Some(Base::Hash("not-a-read")))
        .unwrap_err();
    assert_eq!(
        err.doc_path(&crate::path::DocPath::main())
            .map(|p| p.to_string()),
        Some("main.note".into())
    );
    let w = TypedWriter::new(&config, &mut doc);
    assert_eq!(current_of(err), ("stale", w.hash_of("note").unwrap()));
}

/// A revise of a field neither `text` nor `richtext` fails as it does with no
/// base, never as a stale read of a fresh hash.
#[test]
fn a_revise_of_a_date_under_a_fresh_hash_fails_as_a_plain_revise() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\non: 2024-01-01\n~~~\n");
    let mut w = TypedWriter::new(&config, &mut doc);
    let plain = w.revise(crate::document::Target::from("on"), "2024-02-02", None).unwrap_err().code();
    let read = w.hash_of("on").unwrap();
    let err = w
        .revise(crate::document::Target::from("on"), "2024-02-02", Some(Base::Hash(&read)))
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
        content_hash(&quillmark_content::markdown::from_markdown("Hello *world*").unwrap().content)
    );
    let mut w = TypedWriter::new(&config, &mut doc);
    assert_eq!(w.hash_of("subject").unwrap(), as_string);
    let _ = w.revise(crate::document::Target::Body, "Body.", None).unwrap();
    assert!(config.conform(&mut doc).unwrap().is_empty());
    assert_eq!(doc.main().hash_of("subject").unwrap(), as_string);
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise(crate::document::Target::from("subject"), "Hello _world_, hi", Some(Base::Hash(&as_string)))
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
    card.overwrite_field("summary", anchored(&summary, "Second", "s1"))
        .unwrap();
    let read = doc.to_markdown_annotated().markdown;
    let stored = doc.clone();
    assert!(read.contains("<qm-anchor"), "{read}");

    let mut w = TypedWriter::new(&config, &mut doc);
    let revised = w
        .revise_document_against(
            DocumentBase::Stored(&stored),
            &read.replace("Second para here", "Second paragraph here"),
        )
        .unwrap();
    assert!(revised.dropped_anchors.is_empty());
    let summary = doc
        .card(0)
        .unwrap()
        .field_content("summary", crate::document::Codec::Richtext)
        .unwrap()
        .unwrap();
    assert!(summary.text.contains("Second paragraph here"));
    assert_eq!(anchor_ids(&summary), ["s1"]);
}

#[test]
fn a_field_both_sides_wrote_keeps_its_value_now_and_warnings_follow_their_card() {
    let config = config();
    let mut doc = parse(
        "~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nBody.\n\n~~~\n$kind: item\n~~~\n\nOne.\n",
    );
    let read = doc.to_markdown();
    let stored = doc.clone();

    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise_document(&read.replace("subject: Hello\n", "subject: Hello\nnote: theirs\n"))
        .unwrap();
    let revised = w
        .revise_document_against(
            DocumentBase::Stored(&stored),
            &read
                .replace("$quill: memo@1.0.0\n", "$quill: memo@1.0.0\nnote: mine\n")
                .replace("Body.", "Body, mine."),
        )
        .unwrap();
    assert_eq!(
        revised.conflicts,
        [MergeConflict {
            path: "main.note".parse().unwrap(),
            base: None,
            current: Some(json!("theirs")),
            proposed: Some(json!("mine")),
        }]
    );
    assert_eq!(
        crate::quill::text_of(doc.main().payload().get("note").unwrap().as_json()),
        "theirs"
    );
    assert_eq!(doc.main().body_markdown(), "Body, mine.");

    let mut doc = parse(
        "~~~\n$quill: memo@1.0.0\n~~~\n\nBody.\n\n~~~\n$kind: item\n~~~\n\nOne.\n\n~~~\n$kind: item\n~~~\n\nTwo.\n",
    );
    let read = doc.to_markdown();
    let stored = doc.clone();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise_document(&read.replace("Body.\n", "Body.\n\n~~~\n$kind: item\n~~~\n\nZero.\n"))
        .unwrap();
    let revised = w
        .revise_document_against(
            DocumentBase::Stored(&stored),
            &read.replace("Two.", "Two <span>x</span>."),
        )
        .unwrap();
    assert_eq!(
        revised
            .warnings
            .iter()
            .map(|w| w.path.as_deref())
            .collect::<Vec<_>>(),
        [Some("cards.item[2].body")]
    );
    assert_eq!(doc.card(2).unwrap().body_lines().text, "Two x.");
}

#[test]
fn a_document_revise_merges_a_read_and_refuses_a_stale_hash() {
    let config = config();
    let mut doc =
        parse("~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nAlpha one.\n\nBeta two.\n");
    let read = doc.to_markdown_annotated().markdown;
    let stored = doc.clone();
    let hash = doc.hash();

    let _ = doc
        .main_mut()
        .revise(crate::document::Target::Body, "Alpha one.\n\nBeta two, edited.", None)
        .unwrap();
    let mut w = TypedWriter::new(&config, &mut doc);
    let err = w
        .revise_document_against(DocumentBase::Hash(&hash), &read.replace("Hello", "Goodbye"))
        .unwrap_err();
    assert!(
        matches!(&err, BoundParseError::Base(BaseError::Stale { .. })),
        "{err:?}"
    );
    assert_eq!(
        err.to_diagnostics()[0].code.as_deref(),
        Some("edit::stale_base")
    );

    let _ = w
        .revise_document_against(DocumentBase::Stored(&stored), &read.replace("Hello", "Goodbye"))
        .unwrap();
    let expected: Document = parse(
        "~~~\n$quill: memo@1.0.0\nsubject: Goodbye\n~~~\n\nAlpha one.\n\nBeta two, edited.\n",
    );
    assert_eq!(doc.to_markdown(), expected.to_markdown());
}

/// A merge compares the three sides with their islands that have no markdown
/// as tokens, so a token the writer kept survives a change made since its
/// read, in a body and in a document.
/// A tag the writer moved pins its anchor through a merge, as it does with no
/// change since the read.
#[test]
fn a_moved_tag_pins_through_a_merge() {
    let mut card = card_with_body("Alpha beta gamma delta.\n\nSecond.\n\nThird.");
    let body = anchored(card.body_lines(), "delta", "c1");
    card.overwrite_body(body);
    let read = to_markdown_annotated(card.body_lines()).markdown;
    let base = card.read(crate::document::Target::Body).unwrap();
    assert!(
        read.contains("<qm-anchor ref=\"c1\"></qm-anchor>delta"),
        "{read}"
    );
    let moved = read
        .replace("<qm-anchor ref=\"c1\"></qm-anchor>delta", "delta")
        .replace("beta", "<qm-anchor ref=\"c1\"></qm-anchor>beta");

    let _ = card
        .revise_body(read.replace("Third.", "Third, theirs."))
        .unwrap();
    let revised = card.revise_body_against(Base::Content(&base), &moved).unwrap();
    assert!(revised.dropped_anchors.is_empty());
    let ctx = quillmark_content::lines::anchor::anchor_context(card.body_lines(), "c1").unwrap();
    assert!(ctx.quote.starts_with("beta"), "{ctx:?}");
    assert!(card.body_markdown().contains("Third, theirs."));
}

/// A plain read of a value whose markup loses an island is still a read of
/// it now: the writer's own markdown lands, rather than a merge that could
/// only conflict on the island's line.
#[test]
fn a_plain_read_of_an_unchanged_value_writes_the_writers_own() {
    let mut card = Card::new("note").unwrap();
    let mut body = from_markdown("Lead.\n\n| h |\n| --- |\n| cell |")
        .unwrap()
        .content
        .into_content();
    body.islands[0].props["note"] = "kept".into();
    card.overwrite_body(body.into_lines());
    let read = card.body_markdown();
    let base = card.read(crate::document::Target::Body).unwrap();
    let edited = read.replace("cell", "cell, edited");
    let revised = card
        .revise_body_against(Base::Content(&base), &edited)
        .unwrap();
    assert!(revised.warnings.is_empty(), "{:?}", revised.warnings);
    assert_eq!(card.body_markdown(), edited);

    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\n~~~\n\nLead.\n");
    doc.main_mut().overwrite_body(card.body_lines().clone());
    let read = doc.to_markdown();
    let stored = doc.clone();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise_document_against(DocumentBase::Stored(&stored), &read.replace("edited", "edited twice"))
        .unwrap();
    assert!(doc.main().body_markdown().contains("edited twice"));
}

#[test]
fn a_token_survives_a_merge() {
    let mut card = Card::new("note").unwrap();
    let body = super::revise_tests::titled(
        &from_markdown("Alpha ![a](u.png).\n\nBeta two.")
            .unwrap()
            .content,
    );
    card.overwrite_body(body.clone());
    let read = to_markdown_annotated(card.body_lines()).markdown;
    let base = card.read(crate::document::Target::Body).unwrap();
    assert_eq!(
        read,
        "Alpha <qm-island ref=\"isl-0\"></qm-island>.\n\nBeta two."
    );

    let _ = card
        .revise_body(read.replace("Beta two.", "Beta two, theirs."))
        .unwrap();
    let revised = card
        .revise_body_against(Base::Content(&base), read.replace("Alpha", "Alpha, mine,"))
        .unwrap();
    assert!(revised.warnings.is_empty(), "{:?}", revised.warnings);
    assert_eq!(
        card.body_lines().text,
        "Alpha, mine, \u{FFFC}.\nBeta two, theirs."
    );
    assert_eq!(card.body_lines().islands, body.islands);

    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\n~~~\n\nAlpha ![a](u.png).\n\nBeta two.\n");
    doc.main_mut().overwrite_body(body.clone());
    let read = doc.to_markdown_annotated().markdown;
    let stored = doc.clone();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise_document(&read.replace("Beta two.", "Beta two, theirs."))
        .unwrap();
    let revised = w
        .revise_document_against(DocumentBase::Stored(&stored), &read.replace("Alpha", "Alpha, mine,"))
        .unwrap();
    assert!(revised.warnings.is_empty(), "{:?}", revised.warnings);
    assert_eq!(
        doc.main().body_lines().text,
        "Alpha, mine, \u{FFFC}.\nBeta two, theirs."
    );
    assert_eq!(doc.main().body_lines().islands, body.islands);
}

fn jobs(doc: &Document) -> serde_json::Value {
    crate::quill::plain_leaves(doc.main().payload().get("jobs").unwrap().as_json())
}

/// The person edits one row while the writer edits another: both land.
#[test]
fn edits_to_two_rows_of_one_array_both_land() {
    let config = config();
    let mut doc = parse(
        "~~~\n$quill: memo@1.0.0\njobs:\n  - title: Clerk\n    dates: 2019\n  - title: Analyst\n    dates: 2021\n~~~\n\nBody.\n",
    );
    config.conform(&mut doc).unwrap();
    let read = doc.to_markdown_annotated().markdown;
    let stored = doc.clone();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise_document(&read.replace("2019", "2019-2020"))
        .unwrap();
    let revised = w
        .revise_document_against(
            DocumentBase::Stored(&stored),
            &read.replace("Analyst", "Senior analyst"),
        )
        .unwrap();
    assert!(revised.conflicts.is_empty(), "{:?}", revised.conflicts);
    assert_eq!(
        jobs(&doc),
        json!([
            {"title": "Clerk", "dates": "2019-2020"},
            {"title": "Senior analyst", "dates": "2021"},
        ])
    );
}

/// A card the person removed while the writer edited it stays removed and is
/// reported whole; the writer's other edits land.
#[test]
fn a_card_removed_since_the_read_and_edited_by_the_writer_stays_removed() {
    let config = config();
    let mut doc = parse(
        "~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nBody.\n\n~~~\n$kind: item\n~~~\n\nOne.\n\n~~~\n$kind: item\n~~~\n\nTwo.\n",
    );
    let read = doc.to_markdown_annotated().markdown;
    let stored = doc.clone();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w.remove_card(1);
    let revised = w
        .revise_document_against(
            DocumentBase::Stored(&stored),
            &read
                .replace("Two.", "Two, mine.")
                .replace("Hello", "Goodbye"),
        )
        .unwrap();
    assert_eq!(revised.conflicts.len(), 1, "{:?}", revised.conflicts);
    let conflict = &revised.conflicts[0];
    assert_eq!(conflict.path.to_string(), "cards.item[1]");
    assert_eq!(conflict.current, None);
    assert_eq!(
        conflict.proposed.as_ref().unwrap()["$body"],
        json!("Two, mine.")
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.card(0).unwrap().body_markdown(), "One.");
    assert!(doc.to_markdown().contains("subject: Goodbye"));
}

/// Two edits to one line of a body keep the body now and report it, while the
/// writer's edits elsewhere land.
#[test]
fn a_body_line_both_sides_changed_keeps_the_body_now() {
    let config = config();
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nsubject: Hello\n~~~\n\nAlpha one.\n");
    let read = doc.to_markdown_annotated().markdown;
    let stored = doc.clone();
    let mut w = TypedWriter::new(&config, &mut doc);
    let _ = w
        .revise_document(&read.replace("Alpha one.", "Alpha, theirs."))
        .unwrap();
    let revised = w
        .revise_document_against(
            DocumentBase::Stored(&stored),
            &read
                .replace("Alpha one.", "Alpha, mine.")
                .replace("Hello", "Goodbye"),
        )
        .unwrap();
    assert_eq!(
        revised.conflicts,
        [MergeConflict {
            path: "main.body".parse().unwrap(),
            base: Some(json!("Alpha one.")),
            current: Some(json!("Alpha, theirs.")),
            proposed: Some(json!("Alpha, mine.")),
        }]
    );
    assert_eq!(doc.main().body_markdown(), "Alpha, theirs.");
    assert!(doc.to_markdown().contains("subject: Goodbye"));
}
