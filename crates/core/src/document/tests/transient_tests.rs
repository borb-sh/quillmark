use serde_json::json;

use crate::document::Document;
use crate::value::QuillValue;

use super::parse;
use super::revise_tests::anchored;

const SOURCE: &str = "~~~\n$quill: q\n~~~\n\nThe committee met on Monday to plan the roads budget.\n\n\
~~~\n$kind: note\n~~~\n\nA note about apples and the orchard rows.\n";

/// [`SOURCE`] with a stored anchor `c1` and a transient `~w1` in the main
/// body, and a transient `~w2` in the note's `items` field.
fn document() -> Document {
    let mut doc = parse(SOURCE);
    let body = anchored(doc.main().body_lines(), "met on Monday", "c1");
    let body = anchored(&body, "roads budget", "~w1");
    doc.main_mut().overwrite_body(body);
    let item = crate::document::import_body_lines("an item worth a flag").unwrap();
    let items = json!([crate::document::stored_over(
        None,
        &anchored(&item, "worth a flag", "~w2")
    )]);
    doc.unchecked()
        .store_field(Some(0), "items", QuillValue::from_json(items))
        .unwrap();
    doc
}

/// [`document`] with no transient anchor.
fn stored_only() -> Document {
    let mut doc = parse(SOURCE);
    let body = anchored(doc.main().body_lines(), "met on Monday", "c1");
    doc.main_mut().overwrite_body(body);
    let item = crate::document::import_body_lines("an item worth a flag").unwrap();
    let items = json!([crate::document::stored_over(None, &item)]);
    doc.unchecked()
        .store_field(Some(0), "items", QuillValue::from_json(items))
        .unwrap();
    doc
}

fn quote(doc: &Document, id: &str) -> Option<String> {
    doc.anchor_context(id).map(|c| c.quote)
}

#[test]
fn the_stored_form_and_the_hash_carry_no_transient_anchor() {
    let doc = document();
    let stored = serde_json::to_string(&doc).unwrap();
    assert!(stored.contains("\"c1\""));
    assert!(!stored.contains("~w"), "{stored}");
    assert_eq!(doc.hash(), stored_only().hash());

    let reopened: Document = serde_json::from_str(&stored).unwrap();
    assert_eq!(quote(&reopened, "c1").as_deref(), Some("met on Monday"));
    assert_eq!(quote(&reopened, "~w1"), None);
    assert_eq!(reopened, stored_only());
}

#[test]
fn the_annotated_read_neither_spells_nor_lists_a_transient_anchor() {
    let doc = document();
    let read = doc.to_markdown_annotated();
    let ids: Vec<&str> = read.anchors.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["c1"]);
    assert!(!read.markdown.contains("~w"), "{}", read.markdown);
    assert_eq!(
        read.markdown,
        stored_only().to_markdown_annotated().markdown
    );
}

#[test]
fn a_clone_keeps_transient_anchors_and_equality_sees_them() {
    let doc = document();
    let copy = doc.clone();
    assert_eq!(quote(&copy, "~w1").as_deref(), Some("roads budget"));
    assert_eq!(quote(&copy, "~w2").as_deref(), Some("worth a flag"));
    assert_ne!(doc, stored_only());
}

#[test]
fn a_transient_anchor_moves_with_its_text_through_a_body_revise() {
    let mut doc = document();
    let _ = doc
        .main_mut()
        .revise(
            crate::document::Target::Body,
            "Last week the committee met on Monday to plan the roads budget for spring.",
            None,
        )
        .unwrap();
    assert_eq!(quote(&doc, "~w1").as_deref(), Some("roads budget"));
    assert_eq!(quote(&doc, "c1").as_deref(), Some("met on Monday"));
}

#[test]
fn a_transient_anchor_survives_a_revise_from_the_annotated_read() {
    let mut doc = document();
    let source = doc
        .to_markdown_annotated()
        .markdown
        .replace("The committee", "Our committee");
    let _ = doc.revise(&source).unwrap();
    assert_eq!(quote(&doc, "~w1").as_deref(), Some("roads budget"));
    assert_eq!(quote(&doc, "~w2").as_deref(), Some("worth a flag"));
}

#[test]
fn a_revise_that_deletes_the_text_names_the_transient_anchor_it_drops() {
    let mut doc = document();
    let receipt = doc
        .main_mut()
        .revise(
            crate::document::Target::Body,
            "The committee met on Monday.",
            None,
        )
        .unwrap();
    let dropped: Vec<&str> = receipt
        .dropped_anchors
        .iter()
        .map(|a| a.context.id.as_str())
        .collect();
    assert_eq!(dropped, ["~w1"]);
}
