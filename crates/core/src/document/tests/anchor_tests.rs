use std::collections::HashMap;

use quillmark_content::model::{MarkKind, Normalized};
use quillmark_content::serial::to_canonical_value;
use serde_json::{json, Value as JsonValue};

use crate::document::{AnchorSelector, Card, Codec, Document};
use crate::path::DocPath;
use crate::quill::{FieldSchema, FieldType};
use crate::value::QuillValue;

use super::parse;
use super::revise_tests::{anchor_ids, anchored};

const SOURCE: &str = "~~~\n$quill: q\n~~~\n\nThe committee met on Monday to plan the roads budget.\n\n\
~~~\n$kind: note\n~~~\n\nA note about apples and the orchard rows.\n";

/// [`SOURCE`] with an anchor in the main body, the note's body, and the second
/// item of the note's `items` list.
fn document() -> Document {
    let mut doc = parse(SOURCE);
    let body = anchored(doc.main().body(), "met on Monday", "m1");
    doc.main_mut().overwrite_body(body);
    let mut note = doc.card_mut(0).unwrap();
    let body = anchored(note.body(), "orchard rows", "n1");
    note.overwrite_body(body);
    let item = crate::document::import_body("an item worth a flag").unwrap();
    let items = json!([
        to_canonical_value(&crate::document::import_body("a plain item").unwrap()),
        to_canonical_value(&anchored(&item, "worth a flag", "i1")),
    ]);
    note.store_field("items", QuillValue::from_json(items)).unwrap();
    doc
}

/// `doc` with every prose anchor taken out, through the stored form.
fn without_anchors(doc: &Document) -> Document {
    fn strip(value: &mut JsonValue) {
        match value {
            JsonValue::Object(map) => {
                if let Some(JsonValue::Array(marks)) = map.get_mut("marks") {
                    marks.retain(|m| m["type"] != "anchor");
                }
                map.values_mut().for_each(strip);
            }
            JsonValue::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut stored = serde_json::to_value(doc).unwrap();
    strip(&mut stored);
    serde_json::from_value(stored).unwrap()
}

fn ids(selectors: &[AnchorSelector]) -> Vec<&str> {
    selectors.iter().map(|s| s.id.as_str()).collect()
}

#[test]
fn contexts_list_every_anchor_at_its_path_in_annotated_order() {
    let doc = document();
    let selectors = doc.anchor_contexts();
    let listed: Vec<(String, String)> = doc
        .to_markdown_annotated()
        .anchors
        .into_iter()
        .map(|a| (a.id, a.path.to_string()))
        .collect();
    let read: Vec<(String, String)> =
        selectors.iter().map(|s| (s.id.clone(), s.path.to_string())).collect();
    assert_eq!(read, listed);
    assert_eq!(read[1], ("i1".into(), "cards.note[0].items[1]".into()));

    let item = doc.anchor_context("i1").unwrap();
    assert_eq!((item.quote.as_str(), item.prefix.as_str(), item.suffix.as_str()), ("worth a flag", "an item ", ""));
    assert_eq!(doc.anchor_context("nope"), None);
}

#[test]
fn contexts_then_reattach_is_the_identity_and_restores_what_a_store_lost() {
    let doc = document();
    let selectors = doc.anchor_contexts();

    let mut same = doc.clone();
    let out = same.reattach(&selectors, &HashMap::new());
    assert_eq!(same, doc, "a live anchor is not written");
    assert_eq!(out.reattached, selectors);
    assert!(out.orphaned.is_empty());

    let mut bare = without_anchors(&doc);
    assert!(bare.anchor_contexts().is_empty());
    let again = AnchorSelector { path: DocPath::main_body(), ..selectors[2].clone() };
    let out = bare.reattach(&[selectors.clone(), vec![again]].concat(), &HashMap::new());
    assert!(out.orphaned.is_empty(), "an id repeated is answered once: {out:?}");
    assert_eq!(out.reattached, selectors);
    assert_eq!(bare, doc);
}

#[test]
fn a_cold_reopen_recovers_the_body_anchors_from_their_selectors() {
    let doc = document();
    let selectors = doc.anchor_contexts();
    let mut reopened = parse(&doc.to_markdown());
    let out = reopened.reattach(&selectors, &HashMap::new());
    assert_eq!(ids(&out.reattached), ["m1", "n1"]);
    assert_eq!(ids(&out.orphaned), ["i1"], "the reopened list holds markdown, not content");
    assert_eq!(anchor_ids(reopened.main().body()), ["m1"]);
}

#[test]
fn a_path_naming_no_body_or_content_value_orphans_its_selector() {
    let doc = document();
    let n1 = doc.anchor_context("n1").unwrap();
    let mut bare = without_anchors(&doc);
    let elsewhere = |path: &str| AnchorSelector { id: path.into(), path: path.parse().unwrap(), ..n1.clone() };
    let selectors = [
        elsewhere("cards.memo[0].body"),
        elsewhere("cards.note[3].body"),
        elsewhere("cards.note[0].items[7]"),
        elsewhere("cards.note[0].missing"),
        elsewhere("main.body"),
    ];
    let out = bare.reattach(&selectors, &HashMap::new());
    assert_eq!(out.orphaned, selectors);
    assert_eq!(bare, without_anchors(&doc));
}

#[test]
fn an_id_live_anywhere_is_answered_and_not_written() {
    let mut doc = document();
    let table = crate::document::import_body("Lead line.\n\n| a | b |\n|---|---|\n| 1 | 2 |").unwrap();
    let mut table = table.into_content();
    table.islands[0].props["rows"][0][0]["marks"] =
        json!([{ "start": 0, "end": 1, "type": "anchor", "id": "cell1" }]);
    doc.main_mut().overwrite_field("grid", Normalized::from(table)).unwrap();
    let held = doc.clone();

    let cell = AnchorSelector::new("cell1", DocPath::main_body(), "met on Monday", "The committee ", " to plan the roads");
    let elsewhere = AnchorSelector { path: DocPath::main_body(), ..doc.anchor_context("n1").unwrap() };
    let out = doc.reattach(&[cell.clone(), elsewhere], &HashMap::new());
    assert_eq!(doc, held);
    assert_eq!(out.reattached, [cell, doc.anchor_context("n1").unwrap()]);
}

#[test]
fn a_delta_binds_only_the_path_it_is_keyed_by() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nThe committee met on Monday to plan.\n");
    let body = anchored(&anchored(doc.main().body(), "met on Monday", "m1"), "to plan", "m2");
    doc.main_mut().overwrite_body(body);
    let before = doc.anchor_contexts();
    let mut bare = without_anchors(&doc);
    let revised = bare.main_mut().revise_body("Minutes.\n\nThe committee met on Monday to plan.").unwrap();

    let mut strict = bare.clone();
    let here = HashMap::from([(DocPath::main_body(), revised.delta.clone())]);
    assert_eq!(strict.reattach(&before, &here).orphaned, before, "both quotes sit in kept text");

    let elsewhere = HashMap::from([("cards.note[0].body".parse().unwrap(), revised.delta)]);
    let out = bare.reattach(&before, &elsewhere);
    assert_eq!(ids(&out.reattached), ["m1", "m2"]);
    assert_eq!(anchor_ids(bare.main().body()), ["m1", "m2"]);
}

#[test]
fn a_quote_found_again_in_inserted_text_is_placed_with_the_delta() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nIntro.\n\nOver the fiscal year we approved the budget for roads.\n");
    let body = anchored(doc.main().body(), "approved the budget", "c1");
    doc.main_mut().overwrite_body(body);
    let before = doc.anchor_contexts();
    let rewritten = "Intro.\n\nEverything else changed: one two three four five six seven eight \
                     nine ten eleven twelve thirteen fiscal year we approved the budget for roads.";
    let mut bare = without_anchors(&doc);
    let revised = bare.main_mut().revise_body(rewritten).unwrap();
    let deltas = HashMap::from([(DocPath::main_body(), revised.delta)]);
    let out = bare.reattach(&before, &deltas);
    assert!(out.orphaned.is_empty(), "{out:?}");
    assert_eq!(out.reattached[0].quote, "approved the budget");

    let revised = doc.main_mut().revise_body(rewritten).unwrap();
    assert!(revised.dropped_anchors.is_empty(), "revise relocates it itself");
    assert_eq!(doc, bare);
}

/// Every rung that revises one body or field names the anchor it drops, with
/// the line it stood on.
#[test]
fn every_field_revise_names_the_anchor_it_drops() {
    let base = crate::document::import_body("Keep this line.\n\nDrop this anchored line.").unwrap();
    let base = anchored(&base, "anchored", "c1");
    let next = crate::document::import_body("Keep this line.").unwrap();
    let schema = FieldSchema::new("f".to_string(), FieldType::RichText { multiline: true }, None);
    let fresh = || {
        let mut card = Card::new("note").unwrap();
        card.overwrite_body(base.clone());
        card.overwrite_field("f", base.clone()).unwrap();
        card
    };
    type Rung = fn(&mut Card, &Normalized, &FieldSchema) -> crate::document::Revised;
    let rungs: [(&str, Rung); 5] = [
        ("revise_body", |c, _, _| c.revise_body("Keep this line.").unwrap()),
        ("revise_field", |c, _, _| c.revise_field("f", "Keep this line.").unwrap()),
        ("revise_body_content", |c, n, _| c.revise_body_content(n.clone())),
        ("revise_field_content", |c, n, _| c.revise_field_content("f", n.clone()).unwrap()),
        ("revise_field_checked", |c, _, s| c.revise_field_checked("f", "Keep this line.", s).unwrap()),
    ];
    for (rung, revise) in rungs {
        let mut card = fresh();
        let revised = revise(&mut card, &next, &schema);
        let dropped: Vec<(&str, &str)> =
            revised.dropped_anchors.iter().map(|a| (a.id.as_str(), a.line.as_str())).collect();
        assert_eq!(dropped, [("c1", "Drop this anchored line.")], "{rung}");
    }

    let mut card = fresh();
    let kept = card.revise_field("f", "Keep this line.\n\nDrop this anchored line, edited.").unwrap();
    assert!(kept.dropped_anchors.is_empty(), "{kept:?}");
    let read = card.field_content("f", Codec::Richtext).unwrap().unwrap();
    assert!(read.marks.iter().any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")));
}
