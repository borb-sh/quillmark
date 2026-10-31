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
    let md = "~~~\n$quill: q\nsubject: The subject line, edited\n~~~\n\nMain prose stays here.\n\n\
~~~\n$kind: aside\n~~~\n\nAn inserted aside.\n\n\
~~~\n$kind: note\n~~~\n\nSecond note about pears.\n\n\
~~~\n$kind: note\n~~~\n\nFirst note about kiwi.\n";
    let receipt = doc.revise(md).unwrap();
    assert_eq!(receipt.from, [None, Some(1), Some(0)]);

    let keys: Vec<_> = doc.cards().iter().map(|c| c.ext().map(|e| e["app"]["key"].clone())).collect();
    assert_eq!(keys, [None, Some(serde_json::json!("n2")), Some(serde_json::json!("n1"))]);
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
    let md = "~~~\n$quill: q\n~~~\n\n~~~\n$kind: note\n~~~\n\nUnrelated words entirely.\n";
    assert_eq!(doc.revise(md).unwrap().from, [Some(0)]);
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
    let incoming = "~~~\n$quill: other@1.2\n# kept comment\nsubject: plain now\nqty: 4\n~~~\n";
    let _ = doc.revise(incoming).unwrap();
    assert_eq!(doc.quill_reference().to_string(), "other@1.2");
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

/// A content object stored out of canonical order emits as the structure it
/// is and revises as authored, and an anchor in it is named when a revise
/// drops it.
#[test]
fn an_anchor_in_content_stored_out_of_canonical_order_is_named_when_it_drops() {
    let note = crate::document::import_body("a note to flag").unwrap();
    let canonical = to_canonical_value(&anchored(&note, "flag", "n1"));
    let reordered: serde_json::Map<String, serde_json::Value> =
        canonical.as_object().unwrap().clone().into_iter().rev().collect();
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nBody.\n");
    doc.main_mut()
        .store_field("note", QuillValue::from_json(reordered.into()))
        .unwrap();

    let receipt = doc.clone().revise(&doc.to_markdown()).unwrap();
    assert!(receipt.dropped_anchors.is_empty(), "{receipt:?}");

    let receipt = doc
        .revise("~~~\n$quill: q\nnote: a note to flag\n~~~\n\nBody.\n")
        .unwrap();
    let dropped: Vec<String> = receipt
        .dropped_anchors
        .iter()
        .map(|d| format!("{}#{}", d.path, d.id))
        .collect();
    assert_eq!(dropped, ["main.note#n1"]);
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
    let md = "~~~\n$quill: q\n~~~\n\n\
~~~\n$kind: note\nowner: Bob\nstatus: done\n~~~\n\nReview the budget table and sign off.\n\n\
~~~\n$kind: note\nowner: Cy\nstatus: open\n~~~\n\nDraft the appendix.\n";
    assert_eq!(doc.revise(md).unwrap().from, [Some(1), None]);
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

/// A card spelling a stored card's `$id` revises it wherever it moved and
/// however its text changed, keeping its `$ext` and its anchors.
#[test]
fn a_card_spelling_its_id_revises_its_stored_card() {
    let mut doc = stored();
    let first = doc.cards()[0].id().unwrap().to_string();
    let second = doc.cards()[1].id().unwrap().to_string();
    let md = format!(
        "~~~\n$quill: q\n~~~\n\n\
~~~\n$kind: note\n$id: {second}\n~~~\n\nPears, rewritten from the ground up.\n\n\
~~~\n$kind: note\n$id: {first}\n~~~\n\nNothing like apples now.\n"
    );
    let receipt = doc.revise(&md).unwrap();
    assert_eq!(receipt.from, [Some(1), Some(0)]);
    assert_eq!(doc.cards()[0].id(), Some(second.as_str()));
    assert_eq!(doc.cards()[0].ext().unwrap()["app"]["key"], "n2");
    assert_eq!(anchor_ids(doc.cards()[0].body()), ["p1"]);
    assert_eq!(doc.cards()[1].ext().unwrap()["app"]["key"], "n1");
}

/// A writer keeping the ids it read that leaves a card out and writes a new
/// one in its place removed the card: the new one takes neither its id nor
/// its `$ext`.
#[test]
fn a_new_card_in_a_removed_cards_place_takes_none_of_its_identity() {
    let mut doc = stored();
    let first = doc.cards()[0].id().unwrap().to_string();
    let second = doc.cards()[1].id().unwrap().to_string();
    let memo = doc.cards()[2].id().unwrap().to_string();
    let md = format!(
        "~~~\n$quill: q\n~~~\n\n\
~~~\n$kind: note\n$id: {first}\n~~~\n\nFirst note about apples.\n\n\
~~~\n$kind: note\n~~~\n\nA new note about plums.\n\n\
~~~\n$kind: memo\n$id: {memo}\n~~~\n\nA memo to drop.\n"
    );
    let receipt = doc.revise(&md).unwrap();
    assert_eq!(receipt.from, [Some(0), None, Some(2)]);
    assert_ne!(doc.cards()[1].id(), Some(second.as_str()));
    assert!(doc.cards()[1].ext().is_none());
    let dropped: Vec<_> = receipt.dropped_anchors.iter().filter_map(|a| Some((a.id.as_str(), a.card_id.as_deref()?))).collect();
    assert_eq!(dropped, [("p1", second.as_str())]);
}

/// A pasted copy spells the id of the card it copies: the one more like the
/// stored card keeps it, and the other is an inserted card with a new id.
#[test]
fn a_pasted_copy_of_a_card_is_inserted_with_a_new_id() {
    let mut doc = stored();
    let id = doc.cards()[0].id().unwrap().to_string();
    let card = doc.to_markdown();
    let copy = format!("~~~\n$kind: note\n$id: {id}\n~~~\n\nA copy, then changed.\n\n");
    let md = card.replacen("~~~\n$kind: note", &format!("{copy}~~~\n$kind: note"), 1);
    let receipt = doc.revise(&md).unwrap();
    assert_eq!(&receipt.from[..2], [None, Some(0)]);
    assert_eq!(doc.cards()[1].id(), Some(id.as_str()));
    let minted = doc.cards()[0].id().unwrap();
    assert_ne!(minted, id);
    let warned: Vec<_> = receipt
        .warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some(crate::document::card_id::CARD_ID_REMINTED))
        .map(|w| w.path.clone())
        .collect();
    assert_eq!(warned, [Some("cards.note[0]".to_string())]);
}

/// An inserted card keeps a valid `$id` no stored card holds, and a minted
/// one never repeats a stored card's, though both mints start alike.
#[test]
fn an_inserted_card_keeps_a_fresh_id_and_a_mint_skips_the_stored_ids() {
    let mut doc = stored();
    let stored_ids: Vec<String> = doc.cards().iter().map(|c| c.id().unwrap().to_string()).collect();
    let md = format!(
        "{}\n~~~\n$kind: aside\n$id: my-aside\n~~~\n\nNamed.\n\n~~~\n$kind: aside\n~~~\n\nUnnamed.\n",
        doc.to_markdown()
    );
    let receipt = doc.revise(&md).unwrap();
    assert_eq!(receipt.from, [Some(0), Some(1), Some(2), None, None]);
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);
    assert_eq!(doc.cards()[3].id(), Some("my-aside"));
    let minted = doc.cards()[4].id().unwrap();
    assert!(!stored_ids.iter().any(|id| id == minted), "{minted}");
}

/// An item inserted ahead of an anchored one leaves the anchor on its own
/// item, and the receipt names no drop.
#[test]
fn an_item_inserted_ahead_keeps_each_anchor_on_its_own_item() {
    let mut doc = stored_with_items();
    let markdown = doc
        .to_markdown()
        .replace("  - a plain item\n", "  - a new first item\n  - a plain item\n");
    assert_ne!(markdown, doc.to_markdown());
    let receipt = doc.revise(&markdown).unwrap();
    assert!(receipt.dropped_anchors.is_empty(), "{receipt:?}");
    let items = doc.cards()[0].payload().get("items").unwrap().as_json().clone();
    assert_eq!(items[0], "a new first item");
    let item = quillmark_content::serial::from_canonical_value(&items[2]).unwrap();
    assert_eq!(item.text, "an item to flag");
    assert_eq!(anchor_ids(&item), ["i1"]);

    let markdown = doc.to_markdown().replace("  - a plain item\n", "");
    let receipt = doc.revise(&markdown).unwrap();
    assert!(receipt.dropped_anchors.is_empty(), "{receipt:?}");
}

/// `content` with its first island holding a `title` its markdown cannot
/// spell, so an annotated read writes the island as its token.
pub(super) fn titled(content: &Normalized) -> Normalized {
    let mut content = content.clone().into_content();
    content.islands[0].props["title"] = "kept".into();
    content.into_normalized()
}

/// The annotated read is two-way: a writer that moves a tag moves its anchor's
/// start, and a token keeps its island as stored through a rewrite of the text
/// around it.
#[test]
fn an_annotated_read_revises_back_through_its_tags_and_tokens() {
    let mut doc = parse("~~~\n$quill: memo@1.0.0\n~~~\n\nSee ![a](u.png) and the plan.\n\n~~~\n$kind: note\n~~~\n\nA note.\n");
    let body = titled(&anchored(doc.main().body(), "plan", "p1"));
    doc.main_mut().overwrite_body(body.clone());
    let read = doc.to_markdown_annotated().markdown;
    assert!(read.contains("See <qm-island ref=\"isl-0\"></qm-island> and the <qm-anchor ref=\"p1\"></qm-anchor>plan."), "{read}");

    let written = read.replace(
        "See <qm-island ref=\"isl-0\"></qm-island> and the <qm-anchor ref=\"p1\"></qm-anchor>plan.",
        "Now <qm-anchor ref=\"p1\"></qm-anchor>the plan, beside <qm-island ref=\"isl-0\"></qm-island>.",
    );
    let receipt = doc.revise(&written).unwrap();
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);
    assert!(receipt.dropped_anchors.is_empty(), "{:?}", receipt.dropped_anchors);
    let revised = doc.main().body();
    assert_eq!(revised.text, "Now the plan, beside \u{FFFC}.");
    assert_eq!(revised.islands, body.islands);
    let p1 = revised.marks.iter().find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "p1")).unwrap();
    assert_eq!((p1.start, p1.end), (4, 12));
}

/// A tag or token naming nothing the card it lands in holds, an inserted
/// card's among them, drops reported at that card's body; a cold parse drops
/// a token reported and an anchor tag silently.
#[test]
fn a_tag_naming_nothing_stored_drops_reported_at_its_card() {
    let mut doc = parse("~~~\n$quill: memo@1.0.0\n~~~\n\nBody.\n");
    let written = "~~~\n$quill: memo@1.0.0\n~~~\n\nBody <qm-anchor ref=\"x\"></qm-anchor>.\n\n\
                   ~~~\n$kind: note\n~~~\n\nNew <qm-island ref=\"isl-0\"></qm-island>.\n";
    let receipt = doc.revise(written).unwrap();
    let reported: Vec<_> = receipt
        .warnings
        .iter()
        .map(|w| (w.path.as_deref().unwrap_or_default(), w.args["construct"].as_str().unwrap_or_default()))
        .collect();
    assert_eq!(reported, [("main.body", "qm-anchor[ref]"), ("cards.note[0].body", "qm-island[ref]")]);
    assert_eq!(doc.cards()[0].body().text, "New .");

    let cold = Document::parse(written).unwrap();
    let constructs: Vec<_> = cold.warnings.iter().filter_map(|w| w.args.get("construct")).collect();
    assert_eq!(constructs, [&serde_json::json!("qm-island")]);
}

/// A token in a richtext field keeps the field's island as stored.
#[test]
fn a_token_in_a_field_keeps_its_island() {
    let mut doc = parse("~~~\n$quill: memo@1.0.0\nsummary: x\n~~~\n");
    let summary = titled(&crate::document::import_body("A ![b](v.png) here.").unwrap());
    doc.main_mut().overwrite_field("summary", summary.clone()).unwrap();
    let read = doc.to_markdown_annotated().markdown;
    assert!(read.contains("summary: A <qm-island ref=\"isl-0\"></qm-island> here."), "{read}");

    let receipt = doc.revise(&read.replace(" here.", " there.")).unwrap();
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);
    let now = doc.main().field_content("summary", Codec::Richtext).unwrap().unwrap();
    assert_eq!(now.text, "A \u{FFFC} there.");
    assert_eq!(now.islands, summary.islands);
}
