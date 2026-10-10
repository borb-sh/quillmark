use quillmark_content::model::{Mark, MarkKind, Normalized};
use quillmark_content::serial::to_canonical_value;

use crate::document::{Codec, Document};
use crate::value::QuillValue;

use super::parse;

/// `content` with an anchor `id` over the first occurrence of `word`.
pub(super) fn anchored(content: &Normalized, word: &str, id: &str) -> Normalized {
    let byte = content.text.find(word).expect("word in content");
    let start = content.text[..byte].chars().count();
    let end = start + word.chars().count();
    let mut content = content.clone().into_content();
    content
        .marks
        .push(Mark::new(start, end, MarkKind::Anchor { id: id.into() }));
    content.into_normalized()
}

pub(super) fn anchor_ids(content: &Normalized) -> Vec<String> {
    content
        .marks
        .iter()
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn anchor_card_body(doc: &mut Document, index: usize, word: &str, id: &str) {
    let mut card = doc.card_mut(index).unwrap();
    let body = anchored(card.body(), word, id);
    card.overwrite_body(body);
}

const STORED: &str = "~~~\n$quill: q\nsubject: placeholder\n~~~\n\nMain prose stays here.\n\n\
~~~\n$kind: note\n$ext:\n  app:\n    key: n1\n~~~\n\nFirst note about apples.\n\n\
~~~\n$kind: note\n$ext:\n  app:\n    key: n2\n~~~\n\nSecond note about pears.\n\n\
~~~\n$kind: memo\n~~~\n\nA memo to drop.\n";

fn stored() -> Document {
    let mut doc = parse(STORED);
    let subject = crate::document::import_body("The subject line").unwrap();
    doc.main_mut()
        .overwrite_field("subject", anchored(&subject, "subject", "s1"))
        .unwrap();
    let body = anchored(doc.main().body(), "prose", "m1");
    doc.main_mut().overwrite_body(body);
    anchor_card_body(&mut doc, 0, "apples", "a1");
    anchor_card_body(&mut doc, 1, "pears", "p1");
    anchor_card_body(&mut doc, 2, "memo", "x1");
    doc
}

/// [`stored`], its first note holding an `items` list whose second item is
/// anchored `i1`.
fn stored_with_items() -> Document {
    let mut doc = stored();
    let item = crate::document::import_body("an item to flag").unwrap();
    let items = serde_json::json!([
        to_canonical_value(&crate::document::import_body("a plain item").unwrap()),
        to_canonical_value(&anchored(&item, "flag", "i1")),
    ]);
    doc.card_mut(0)
        .unwrap()
        .store_field("items", QuillValue::from_json(items))
        .unwrap();
    doc
}

#[test]
fn aligned_cards_keep_their_anchors_and_the_receipt_names_the_rest() {
    let mut doc = stored();
    let receipt = doc
        .revise(
            "~~~\n$quill: q\nsubject: The subject line, edited\n~~~\n\nMain prose stays here.\n\n\
~~~\n$kind: aside\n~~~\n\nAn inserted aside.\n\n\
~~~\n$kind: note\n~~~\n\nSecond note about pears.\n\n\
~~~\n$kind: note\n~~~\n\nFirst note about kiwi.\n",
        )
        .unwrap();

    assert_eq!(receipt.alignment, vec![None, Some(1), Some(0)]);
    assert_eq!(anchor_ids(doc.main().body()), ["m1"]);
    let subject = doc
        .main()
        .field_content("subject", Codec::Richtext)
        .unwrap()
        .unwrap();
    assert_eq!(anchor_ids(&subject), ["s1"]);
    assert_eq!(anchor_ids(doc.cards()[1].body()), ["p1"]);
    assert!(anchor_ids(doc.cards()[2].body()).is_empty());

    let dropped: Vec<(String, &str)> = receipt
        .dropped_anchors
        .iter()
        .map(|d| (d.path.to_string(), d.id.as_str()))
        .collect();
    assert_eq!(
        dropped,
        [
            ("cards.note[0].body".to_string(), "a1"),
            ("cards.memo[2].body".to_string(), "x1"),
        ]
    );
}

#[test]
fn an_omitted_ext_carries_when_the_card_aligns_by_text_or_the_kind_sequences_match() {
    let same = "~~~\n$quill: q\n~~~\n\n~~~\n$kind: note\n~~~\n\nFirst.\n\n~~~\n$kind: note\n~~~\n\nSecond.\n\n~~~\n$kind: memo\n~~~\n";
    let mut doc = stored();
    let _ = doc.revise(same).unwrap();
    let keys: Vec<_> = doc.cards().iter().map(|c| c.ext().cloned()).collect();
    assert_eq!(keys[0].as_ref().unwrap()["app"]["key"], "n1");
    assert_eq!(keys[1].as_ref().unwrap()["app"]["key"], "n2");

    let mut doc = stored();
    let main_ext = serde_json::Map::from_iter([("k".to_string(), serde_json::json!(1))]);
    doc.main_mut().store_ext(main_ext.clone()).unwrap();
    let _ = doc
        .revise("~~~\n$quill: q\n~~~\n\n~~~\n$kind: note\n~~~\n\nFirst note about apples.\n")
        .unwrap();
    assert_eq!(doc.main().ext(), Some(&main_ext));
    assert_eq!(doc.cards()[0].ext().unwrap()["app"]["key"], "n1");

    let mut doc = stored();
    let receipt = doc
        .revise("~~~\n$quill: q\n~~~\n\n~~~\n$kind: note\n~~~\n\nUnrelated words entirely.\n")
        .unwrap();
    assert_eq!(receipt.alignment, vec![Some(0)]);
    assert_eq!(doc.cards()[0].ext(), None);

    let mut doc = stored();
    let _ = doc
        .revise(
            "~~~\n$quill: q\n~~~\n\n~~~\n$kind: note\n$ext:\n  app:\n    key: fresh\n~~~\n\n\
~~~\n$kind: note\n$ext: {}\n~~~\n\n~~~\n$kind: memo\n~~~\n",
        )
        .unwrap();
    assert_eq!(doc.cards()[0].ext().unwrap()["app"]["key"], "fresh");
    assert!(doc.cards()[1].ext().unwrap().is_empty());
}

#[test]
fn everything_else_lands_as_the_markdown_spells_it() {
    let mut doc = stored();
    doc.main_mut()
        .store_field("qty", QuillValue::from_json(serde_json::json!(3)))
        .unwrap();
    let incoming = "~~~\n$quill: other@1\n# kept comment\nsubject: plain now\nqty: 4\n~~~\n";
    let _ = doc.revise(incoming).unwrap();
    assert_eq!(doc.quill_reference().to_string(), "other@1");
    assert_eq!(doc.main().payload().get("qty").unwrap().as_json(), &serde_json::json!(4));
    assert!(doc.cards().is_empty());
    let mut expected = parse(incoming);
    let subject = doc
        .main()
        .field_content("subject", Codec::Richtext)
        .unwrap()
        .unwrap();
    expected.main_mut().overwrite_field("subject", subject).unwrap();
    assert_eq!(doc.to_markdown(), expected.to_markdown());
    assert!(doc.to_markdown().contains("# kept comment"));
}

#[test]
fn a_parse_failure_leaves_the_document_unchanged() {
    let mut doc = stored();
    let before = doc.clone();
    assert!(doc.revise("~~~\n$kind: note\n~~~\n").is_err());
    assert_eq!(doc, before);
}

#[test]
fn dropped_anchor_paths_name_the_stored_address() {
    let mut doc = stored();
    let receipt = doc
        .revise("~~~\n$quill: q\nsubject: Hmm\n~~~\n\nMain stays here.\n")
        .unwrap();
    let dropped: Vec<String> = receipt
        .dropped_anchors
        .iter()
        .map(|d| format!("{}#{}", d.path, d.id))
        .collect();
    assert_eq!(
        dropped,
        [
            "main.subject#s1",
            "main.body#m1",
            "cards.note[0].body#a1",
            "cards.note[1].body#p1",
            "cards.memo[2].body#x1",
        ]
    );
}

#[test]
fn a_dropped_anchor_inside_a_field_names_the_content_holding_it() {
    let mut doc = stored_with_items();
    let read = doc.to_markdown_annotated();
    let listed = read.anchors.iter().find(|a| a.id == "i1").unwrap();
    assert_eq!(listed.path.to_string(), "cards.note[0].items[1]");

    let markdown = read
        .markdown
        .replace("  - an item to <qm-anchor ref=\"i1\"></qm-anchor>flag\n", "");
    assert_ne!(markdown, read.markdown);
    let receipt = doc.revise(&markdown).unwrap();
    let dropped: Vec<String> = receipt
        .dropped_anchors
        .iter()
        .map(|d| format!("{}#{}", d.path, d.id))
        .collect();
    assert_eq!(dropped, ["cards.note[0].items[1]#i1"]);
}

/// An anchor in content nested in a field revises with the item holding it,
/// matched by index, so an edit elsewhere in the item keeps it.
#[test]
fn an_anchor_in_a_fields_list_item_survives_an_edit_to_its_item() {
    let mut doc = stored_with_items();

    let markdown = doc.to_markdown().replace("an item to flag", "an edited item to flag");
    let receipt = doc.revise(&markdown).unwrap();
    assert!(receipt.dropped_anchors.iter().all(|d| d.id != "i1"), "{receipt:?}");
    let items = doc.cards()[0].payload().get("items").unwrap().as_json().clone();
    let item = quillmark_content::serial::from_canonical_value(&items[1]).unwrap();
    assert_eq!(item.text, "an edited item to flag");
    assert_eq!(anchor_ids(&item), ["i1"]);
    assert!(items[0].is_object(), "an unanchored item revises to content too");
}

#[test]
fn a_deleted_card_never_hands_its_ext_to_an_edited_neighbour() {
    let mut doc = parse(
        "~~~\n$quill: q\n~~~\n\n\
~~~\n$kind: note\nowner: Ann\nstatus: done\n$ext:\n  app:\n    key: ann\n~~~\n\nWrite the intro section.\n\n\
~~~\n$kind: note\nowner: Bob\nstatus: open\n$ext:\n  app:\n    key: bob\n~~~\n\nReview the budget table.\n",
    );
    let receipt = doc
        .revise(
            "~~~\n$quill: q\n~~~\n\n\
~~~\n$kind: note\nowner: Bob\nstatus: done\n~~~\n\nReview the budget table and sign off.\n\n\
~~~\n$kind: note\nowner: Cy\nstatus: open\n~~~\n\nDraft the appendix.\n",
        )
        .unwrap();
    assert_eq!(receipt.alignment, vec![Some(1), None]);
    assert_eq!(doc.cards()[0].ext().unwrap()["app"]["key"], "bob");
    assert_eq!(doc.cards()[1].ext(), None);
}

#[test]
fn the_annotated_read_lists_each_anchor_at_its_field() {
    let doc = stored_with_items();

    let read = doc.to_markdown_annotated();
    let listed: Vec<(String, &str, &str)> = read
        .anchors
        .iter()
        .map(|a| (a.path.to_string(), a.id.as_str(), a.line.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            ("main.subject".to_string(), "s1", "The subject line"),
            ("main.body".to_string(), "m1", "Main prose stays here."),
            ("cards.note[0].items[1]".to_string(), "i1", "an item to flag"),
            ("cards.note[0].body".to_string(), "a1", "First note about apples."),
            ("cards.note[1].body".to_string(), "p1", "Second note about pears."),
            ("cards.memo[2].body".to_string(), "x1", "A memo to drop."),
        ]
    );
    for id in ["s1", "m1", "i1", "a1", "p1", "x1"] {
        let tag = format!("<qm-anchor ref=\"{id}\"></qm-anchor>");
        assert_eq!(read.markdown.matches(&tag).count(), 1, "{id}:\n{}", read.markdown);
    }
}

#[test]
fn revising_with_the_annotated_read_keeps_every_anchor() {
    let mut doc = stored();
    let before = doc.clone();
    let markdown = doc.to_markdown_annotated().markdown;
    assert!(markdown.contains("<qm-anchor ref=\"s1\"></qm-anchor>"));
    let receipt = doc.revise(&markdown).unwrap();
    assert!(receipt.dropped_anchors.is_empty(), "{:?}", receipt.dropped_anchors);
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);
    assert_eq!(doc, before);
}

#[test]
fn a_revised_fields_nested_comments_stay() {
    let mut doc = stored();
    let sections = serde_json::json!([{
        "body": to_canonical_value(&crate::document::import_body("hello world").unwrap()),
        "note": "x",
    }]);
    doc.main_mut()
        .store_field("sections", QuillValue::from_json(sections))
        .unwrap();
    let markdown = doc
        .to_markdown()
        .replace("sections:\n", "sections:\n  # lead comment\n")
        .replace("    note: x", "    # inner\n    note: x");
    assert!(markdown.contains("# inner"), "{markdown}");
    let _ = doc.revise(&markdown).unwrap();
    let out = doc.to_markdown();
    assert!(out.contains("# lead comment") && out.contains("# inner"), "{out}");
}
