use quillmark_content::change::{AnchorStep, Change, Step, apply};
use quillmark_content::markdown::from_markdown;
use quillmark_content::model::{Anchor, Content};
use serde_json::json;

use crate::document::{Document, Target};
use crate::value::QuillValue;

use super::parse;

const SOURCE: &str = "~~~\n$quill: q\n~~~\n\nThe committee met on Monday to plan the roads budget.\n\n\
~~~\n$kind: note\n~~~\n\nA note about apples.\n";

fn place(content: &Content, quote: &str, id: &str) -> Change {
    let start = content.text[..content.text.find(quote).expect("quote in text")]
        .chars()
        .count();
    Change::new(vec![Step::Anchor(AnchorStep::Place {
        anchor: Anchor::new(id, start, start + quote.chars().count()),
        quote: None,
    })])
}

fn anchored(content: &Content, quote: &str, id: &str) -> Content {
    apply(content, &place(content, quote, id)).unwrap().content
}

fn content(markdown: &str) -> Content {
    from_markdown(markdown).unwrap().content
}

/// A stored anchor `c1` in the main body, and with `transient` a `~w1` beside
/// it, a `~w2` in the note's richtext `summary` and a `~w3` in a value inside
/// its `items` list.
fn document(transient: bool) -> Document {
    let mark = |c: Content, quote: &str, id: &str| match transient {
        true => anchored(&c, quote, id),
        false => c,
    };
    let mut doc = parse(SOURCE);
    let body = anchored(doc.main().body(), "met on Monday", "c1");
    let body = mark(body, "roads budget", "~w1");
    doc.main_mut().replace(Target::Body, &body, None).unwrap();
    let summary = mark(content("A summary worth a flag"), "worth a flag", "~w2");
    let item = mark(content("an item to check"), "to check", "~w3");
    let mut unchecked = doc.unchecked();
    unchecked
        .store_field(
            Some(0),
            "summary",
            QuillValue::from_json(summary.to_value()),
        )
        .unwrap();
    unchecked
        .store_field(
            Some(0),
            "items",
            QuillValue::from_json(json!([{ "line": item.to_value() }])),
        )
        .unwrap();
    doc
}

fn quote(doc: &Document, id: &str) -> Option<String> {
    doc.anchor_context(id).map(|c| c.quote)
}

#[test]
fn storage_and_every_hash_leave_out_a_transient_anchor() {
    let (doc, plain) = (document(true), document(false));
    let stored = serde_json::to_string(&doc).unwrap();
    assert!(stored.contains("\"c1\""), "{stored}");
    assert!(!stored.contains("~w"), "{stored}");
    assert_eq!(serde_json::from_str::<Document>(&stored).unwrap(), plain);

    assert_eq!(doc.hash(), plain.hash());
    assert_eq!(
        doc.main().hash_of(Target::Body).unwrap(),
        plain.main().hash_of(Target::Body).unwrap()
    );
    assert_eq!(
        doc.cards()[0].hash_of(Target::Field("summary")).unwrap(),
        plain.cards()[0].hash_of(Target::Field("summary")).unwrap()
    );
    assert_ne!(doc, plain, "equality sees a transient anchor");
}

#[test]
fn the_annotated_read_neither_spells_nor_lists_a_transient_anchor() {
    let (doc, plain) = (document(true), document(false));
    let read = doc.to_markdown_annotated();
    let ids: Vec<&str> = read.anchors.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["c1"]);
    assert_eq!(read, plain.to_markdown_annotated());
    assert_eq!(
        doc.main().annotated(Target::Body).unwrap(),
        plain.main().annotated(Target::Body).unwrap()
    );
    assert_eq!(
        doc.cards()[0].annotated(Target::Field("summary")).unwrap(),
        plain.cards()[0]
            .annotated(Target::Field("summary"))
            .unwrap()
    );
}

/// A host placing a suggestion between a writer's read and its write: the
/// placement lands without moving the hash, so the writer's base still holds,
/// and the anchor rides the writer's revise.
#[test]
fn a_transient_anchor_lands_without_staling_a_base() {
    let mut doc = document(false);
    let base = doc.main().hash_of(Target::Body).unwrap();
    let change = place(doc.main().body(), "roads budget", "~w1");
    let placed = doc
        .main_mut()
        .apply(Target::Body, &change, Some(base.as_str()))
        .unwrap();
    assert_eq!(placed.hash, base);
    assert_eq!(quote(&doc, "~w1").as_deref(), Some("roads budget"));

    let summary = doc.cards()[0].read(Target::Field("summary")).unwrap();
    let field_base = doc.cards()[0].hash_of(Target::Field("summary")).unwrap();
    let placed = doc
        .card_mut(0)
        .unwrap()
        .apply(
            Target::Field("summary"),
            &place(&summary, "worth a flag", "~w2"),
            Some(field_base.as_str()),
        )
        .unwrap();
    assert_eq!(placed.hash, field_base);
    assert_eq!(quote(&doc, "~w2").as_deref(), Some("worth a flag"));

    doc.main_mut()
        .revise(
            Target::Body,
            "Last week the committee met on Monday to plan the roads budget for spring.",
            Some(base.as_str()),
        )
        .unwrap();
    assert_eq!(quote(&doc, "~w1").as_deref(), Some("roads budget"));
    assert_eq!(quote(&doc, "c1").as_deref(), Some("met on Monday"));
}

/// The annotated read spells no transient anchor, and a revise keeps an
/// anchor its markdown does not spell, so a round trip through the read keeps
/// every one.
#[test]
fn a_transient_anchor_survives_a_revise_from_the_annotated_read() {
    let mut doc = document(true);
    let source = doc
        .to_markdown_annotated()
        .markdown
        .replace("The committee", "Our committee");
    assert!(doc.revise(&source).unwrap().dropped_anchors.is_empty());
    assert_eq!(quote(&doc, "~w1").as_deref(), Some("roads budget"));
    assert_eq!(quote(&doc, "~w2").as_deref(), Some("worth a flag"));
    assert_eq!(quote(&doc, "~w3").as_deref(), Some("to check"));
}

#[test]
fn a_revise_that_deletes_the_text_names_the_transient_anchor_it_drops() {
    let mut doc = document(true);
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "The committee met on Monday.", None)
        .unwrap();
    let dropped: Vec<&str> = receipt
        .dropped_anchors
        .iter()
        .map(|a| a.context.id.as_str())
        .collect();
    assert_eq!(dropped, ["~w1"]);
}
