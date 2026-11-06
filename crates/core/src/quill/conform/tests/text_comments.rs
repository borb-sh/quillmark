//! A `text` field rests as a plain content, so a comment on it is an anchor
//! as on a `richtext` field, and a comment on a field holding no content is a
//! whole-field selector.

use std::collections::HashMap;

use serde_json::json;

use super::{MD, bytes, parse_bound, quill};
use crate::document::{AnchorSelector, Document, StoredDocument};
use crate::path::DocPath;
use crate::value::QuillValue;

fn note() -> DocPath {
    "main.note".parse().unwrap()
}

fn selector(id: &str, path: DocPath, quote: &str, prefix: &str, suffix: &str) -> AnchorSelector {
    AnchorSelector::new(id, path, quote, prefix, suffix)
}

/// [`MD`] bound, with anchor `c1` over `literal` in the `text` field `note`.
fn anchored() -> Document {
    let (mut doc, _) = parse_bound(&quill(), MD);
    let out = doc.reattach(
        &[selector("c1", note(), "literal", "a *", "* line")],
        &HashMap::new(),
    );
    assert_eq!(out.orphaned, [], "the quote is found in the text field");
    doc
}

#[test]
fn a_text_field_rests_as_plain_content_and_emits_its_literal_text() {
    let (doc, _) = parse_bound(&quill(), MD);
    let note = doc.main().payload().get("note").unwrap().as_json();
    assert_eq!(note["codec"], json!("plain"));
    assert_eq!(note["text"], json!("a *literal* line"));

    let md = doc.to_markdown();
    assert!(md.contains("note: a *literal* line\n"), "{md}");
    assert!(md.contains("  label: keep *this*\n"), "{md}");
    let (back, warnings) = parse_bound(&quill(), &md);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(bytes(&back), bytes(&doc));
}

#[test]
fn a_string_at_rest_reads_as_authored_and_conforms_to_the_object() {
    let mut doc = Document::parse(MD).expect("transport parse").document;
    assert_eq!(
        doc.main().payload().get("note").unwrap().as_json(),
        &json!("a *literal* line"),
        "the transport door keeps the authored string"
    );
    let quill = quill();
    let text = quill.reader(&doc).get("note").unwrap().unwrap();
    assert_eq!(text.as_str(), Some("a *literal* line"));

    let diags = quill.conform(&mut doc).expect("conform");
    assert!(diags.is_empty(), "{diags:?}");
    let (bound, _) = parse_bound(&quill, MD);
    assert_eq!(bytes(&doc), bytes(&bound));
}

#[test]
fn a_text_anchor_reads_rests_and_round_trips_as_a_richtext_one() {
    let doc = anchored();
    let contexts = doc.anchor_contexts();
    let c1 = contexts
        .iter()
        .find(|s| s.id == "c1")
        .expect("c1 is listed");
    assert_eq!((c1.path.clone(), c1.quote.as_str()), (note(), "literal"));

    let stored = serde_json::to_string(&StoredDocument::from(doc.clone())).unwrap();
    let back: StoredDocument = serde_json::from_str(&stored).unwrap();
    assert_eq!(
        Document::try_from(back).unwrap(),
        doc,
        "the anchor rests in the stored row"
    );

    let read = doc.to_markdown_annotated();
    assert!(
        read.markdown.contains("note: a *literal* line\n"),
        "no tag in a text field: {}",
        read.markdown
    );
    let listed = read
        .anchors
        .iter()
        .find(|a| a.id == "c1")
        .expect("the read lists c1");
    assert_eq!(listed.path, note());

    assert_eq!(doc.to_markdown(), parse_bound(&quill(), MD).0.to_markdown());
}

#[test]
fn a_text_revise_carries_its_anchors_by_the_diff() {
    let mut doc = anchored();
    let revised = quill()
        .writer(&mut doc)
        .revise(
            crate::document::Target::from("note"),
            "now a *literal* line, longer",
            None,
        )
        .expect("revise");
    assert_eq!(revised.dropped_anchors, []);
    let c1 = doc.anchor_context("c1").expect("c1 survives");
    assert_eq!(
        (c1.quote.as_str(), c1.prefix.as_str()),
        ("literal", "now a *")
    );

    let revised = quill()
        .writer(&mut doc)
        .revise(
            crate::document::Target::from("note"),
            "now a line, longer",
            None,
        )
        .expect("revise");
    let dropped: Vec<&str> = revised
        .dropped_anchors
        .iter()
        .map(|a| a.context.id.as_str())
        .collect();
    assert_eq!(
        dropped,
        ["c1"],
        "the receipt names the anchor the edit took"
    );
}

#[test]
fn a_document_revise_keeps_a_text_anchor() {
    let mut doc = anchored();
    let md = doc
        .to_markdown()
        .replace("note: a *literal* line", "note: still a *literal* line");
    let revised = quill()
        .writer(&mut doc)
        .revise_document(&md)
        .expect("revise");
    assert_eq!(revised.dropped_anchors, []);
    let c1 = doc.anchor_context("c1").expect("c1 survives");
    assert_eq!(
        (c1.quote.as_str(), c1.prefix.as_str()),
        ("literal", "still a *")
    );
    let note = doc.main().payload().get("note").unwrap().as_json();
    assert_eq!(note["codec"], json!("plain"));
}

#[test]
fn a_dropped_text_anchor_reattaches_by_its_selector() {
    let mut doc = anchored();
    let selector = doc.anchor_context("c1").unwrap();
    let _ = quill()
        .writer(&mut doc)
        .revise(crate::document::Target::from("note"), "gone", None)
        .unwrap();
    let _ = quill()
        .writer(&mut doc)
        .revise(
            crate::document::Target::from("note"),
            "a *literal* line",
            None,
        )
        .unwrap();
    assert!(doc.anchor_context("c1").is_none());
    let out = doc.reattach(&[selector], &HashMap::new());
    assert_eq!(out.orphaned, []);
    assert_eq!(doc.anchor_context("c1").unwrap().quote, "literal");
}

#[test]
fn a_text_value_refuses_formatting_at_the_content_level() {
    let (mut doc, _) = parse_bound(&quill(), MD);
    let mut value = doc.main().payload().get("note").unwrap().as_json().clone();
    value["marks"] = json!([{ "start": 0, "end": 1, "type": "strong" }]);
    let refused = quillmark_content::model::Content::from_value(&value);
    assert_eq!(
        refused,
        Err(quillmark_content::wire::ParseError::Invalid(
            quillmark_content::model::Invariant::NotPlain
        ))
    );
    let quill = quill();
    let mut w = quill.writer(&mut doc);
    assert!(
        w.set("note", QuillValue::from_json(value).into_json())
            .is_err()
    );
}

#[test]
fn a_whole_field_selector_attaches_while_the_field_exists() {
    let (mut doc, _) = parse_bound(&quill(), MD);
    let qty = selector("q1", "main.qty".parse().unwrap(), "", "", "");
    let missing = selector("q2", "main.absent".parse().unwrap(), "", "", "");
    let out = doc.reattach(&[qty.clone(), missing.clone()], &HashMap::new());
    assert_eq!(out.reattached, [qty.clone()]);
    assert_eq!(out.orphaned, [missing]);
    assert_eq!(
        doc,
        parse_bound(&quill(), MD).0,
        "a whole selector writes nothing"
    );

    let quoted = selector("q3", "main.qty".parse().unwrap(), "3", "", "");
    assert_eq!(
        doc.reattach(&[quoted.clone()], &HashMap::new()).orphaned,
        [quoted]
    );
}

#[test]
fn a_whole_field_selector_follows_its_card_by_id() {
    let md = MD.replace("$kind: entry\n", "$kind: entry\n$id: e1\nweight: 4\n");
    let mut doc = Document::parse(&md).unwrap().document;
    let at = |i: usize| DocPath::card(Some("entry"), i).field("weight");
    let given = selector("w1", at(3), "", "", "").with_card_id(Some("e1".into()));
    let out = doc.reattach(&[given], &HashMap::new());
    assert_eq!(
        out.reattached,
        [selector("w1", at(0), "", "", "").with_card_id(Some("e1".into()))]
    );
}
