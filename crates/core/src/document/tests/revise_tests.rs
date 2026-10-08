use quillmark_content::model::{Mark, MarkKind, Normalized};

use crate::document::{Codec, Document};
use crate::path::DocPath;
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

    let paths: Vec<String> = receipt.deltas.iter().map(|d| d.path.to_string()).collect();
    assert_eq!(
        paths,
        [
            "main.body",
            "main.subject",
            "cards.note[1].body",
            "cards.note[2].body"
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
            "main.body#m1",
            "main.subject#s1",
            "cards.note[0].body#a1",
            "cards.note[1].body#p1",
            "cards.memo[2].body#x1",
        ]
    );
    assert_eq!(receipt.deltas[0].path, DocPath::main_body());
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
