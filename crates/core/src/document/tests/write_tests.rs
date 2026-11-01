use quillmark_content::change::{AnchorStep, Change, Splice, Step, apply, invert};
use quillmark_content::model::{Anchor, Block, BlockKind};

use crate::document::{Card, CardRef, EditError, LOG_WRITES, Target, Write};

use super::parse;

const SOURCE: &str = "~~~\n$quill: q\n~~~\n\nRevenue grew 12%.\n\nShip v2\n";

fn split_at(at: usize, id: &str) -> Change {
    Change::new(vec![Step::Splice(Splice {
        blocks: vec![Block::new(id.to_string(), BlockKind::Para)],
        ..Splice::new(at, " ".to_string(), "\n".to_string())
    })])
}

/// A split names the right half and the left keeps its record; the receipt's
/// hash is the next write's base, and its change undoes the write.
#[test]
fn apply_lands_a_change_and_answers_its_inverse() {
    let mut doc = parse(SOURCE);
    let before = doc.main().read(Target::Body).unwrap();
    let base = doc.main().hash_of(Target::Body).unwrap();
    let receipt = doc
        .main_mut()
        .apply(Target::Body, &split_at(12, "b9"), Some(&base))
        .unwrap();
    let body = doc.main().body();
    assert_eq!(body.text, "Revenue grew\n12%.\nShip v2");
    let ids: Vec<&str> = body.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b9", "b1"]);
    assert_eq!(receipt.hash, doc.main().hash_of(Target::Body).unwrap());

    let undo = invert(&receipt.applied, &before).unwrap();
    assert_eq!(apply(body, &undo).unwrap().content, before);
}

/// A base the card's log no longer reaches refuses the write, which changes
/// nothing; a refused change does the same.
#[test]
fn a_base_older_than_the_log_or_a_refused_change_writes_nothing() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    for n in 0..=LOG_WRITES {
        let typed = Change::new(vec![Step::Splice(Splice::new(n, String::new(), "x".to_string()))]);
        doc.main_mut().apply(Target::Body, &typed, None).unwrap();
    }
    let held = doc.clone();
    let err = doc
        .main_mut()
        .apply(Target::Body, &split_at(7, "b8"), Some(&base))
        .unwrap_err();
    assert_eq!(err.code(), "edit::stale_base");
    let err = doc.main_mut().apply(Target::Body, &split_at(2, "b8"), None).unwrap_err();
    assert!(matches!(err, EditError::Change(_)), "{err:?}");
    assert_eq!(doc, held);
}

/// A base the value has moved on from rebases the write over what changed
/// since, and the receipt says so; the log it rebases from is no part of the
/// document's value.
#[test]
fn a_stale_base_rebases_over_what_changed_since() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().apply(Target::Body, &split_at(12, "b9"), None).unwrap();
    let receipt = doc
        .main_mut()
        .apply(Target::Body, &split_at(7, "b8"), Some(&base))
        .unwrap();
    assert!(receipt.rebased);
    assert_eq!(doc.main().body().text, "Revenue\ngrew\n12%.\nShip v2");
    let stored = serde_json::to_value(&doc).unwrap();
    assert_eq!(doc, serde_json::from_value(stored).unwrap(), "the log is no part of the value");
}

/// A writer that read, thought, and placed a comment lands it on the text it
/// read, however much was typed ahead of it since.
#[test]
fn a_slow_anchor_lands_on_its_text_over_live_typing() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    for (at, typed) in [(0, "Q3: "), (4, "net "), (25, " fast")] {
        let key = Change::new(vec![Step::Splice(Splice::new(at, String::new(), typed.to_string()))]);
        doc.main_mut().apply(Target::Body, &key, None).unwrap();
    }
    assert_eq!(doc.main().body().text, "Q3: net Revenue grew 12%. fast\nShip v2");
    let place = Change::new(vec![Step::Anchor(AnchorStep::Set {
        anchor: Anchor::new("c1".to_string(), 13, 16),
        quote: Some("12%".to_string()),
    })]);
    let receipt = doc.main_mut().apply(Target::Body, &place, Some(&base)).unwrap();
    assert!(receipt.rebased);
    assert_eq!(doc.main().body().anchors, [Anchor::new("c1".to_string(), 21, 24)]);
}

/// A revise from a stale read diffs against what its writer read, so an edit
/// made since to another paragraph stays.
#[test]
fn a_slow_revise_keeps_a_concurrent_edit_elsewhere() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().revise(Target::Body, "Revenue grew 12%.\n\nShip v2 on Friday", None).unwrap();
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "Revenue grew 14%.\n\nShip v2", Some(&base))
        .unwrap();
    assert!(receipt.rebased);
    assert_eq!(doc.main().body().text, "Revenue grew 14%.\nShip v2 on Friday");
    let ids: Vec<&str> = doc.main().body().blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b1"]);
}

/// Two stale revises that each add a paragraph elsewhere both land: the
/// second mints its block past the ids the value now holds, not only those
/// its base held.
#[test]
fn two_stale_revises_that_each_add_a_paragraph_elsewhere_both_land() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nAlpha\n\nBeta\n\nGamma\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().revise(Target::Body, "Alpha\n\nInserted one\n\nBeta\n\nGamma\n", Some(&base)).unwrap();
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "Alpha\n\nBeta\n\nGamma\n\nInserted two\n", Some(&base))
        .unwrap();
    assert!(receipt.rebased);
    assert_eq!(doc.main().body().text, "Alpha\nInserted one\nBeta\nGamma\nInserted two");
}

/// A stale replace whose content carries the list, item and footnote ids a
/// revise since introduced lands beside it under ids of its own.
#[test]
fn a_stale_replace_reusing_ids_introduced_since_lands_beside_them() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nAlpha\n\nGamma\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().revise(Target::Body, "Alpha[^a]\n\n- one\n\nGamma\n\n[^a]: first", Some(&base)).unwrap();
    let mine = quillmark_content::markdown::from_markdown("Alpha\n\nGamma[^b]\n\n- two\n\n[^b]: second").unwrap().content;
    let receipt = doc.main_mut().replace(Target::Body, &mine, Some(&base)).unwrap();
    assert!(receipt.rebased);
    let body = doc.main().body();
    assert_eq!((body.containers.len(), body.islands.len()), (4, 2));
    assert_eq!(quillmark_content::markdown::to_markdown(body), "Alpha[^1]\n\n- one\n\nGamma[^2]\n\n- two\n\n[^1]: first\n\n[^2]: second");
}

/// Two writers who rewrote the same text conflict, and the later one is
/// refused with the step that conflicts and why, writing nothing.
#[test]
fn overlapping_edits_refuse_with_a_rebase_conflict() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().revise(Target::Body, "Revenue grew 15%.\n\nShip v2", None).unwrap();
    let held = doc.clone();
    let err = doc
        .main_mut()
        .revise(Target::Body, "Revenue grew 14%.\n\nShip v2", Some(&base))
        .unwrap_err();
    assert_eq!(err.code(), "edit::rebase_conflict");
    let args = err.args();
    assert_eq!(args["current"], serde_json::json!(doc.main().hash_of(Target::Body).unwrap()));
    assert_eq!(args["step"], serde_json::json!(0));
    assert!(args.contains_key("reason"), "{args:?}");
    assert_eq!(doc, held);
}

/// A stale revise that quotes a block keeps the heading another writer made
/// of it since, and a stale restyle of a block another writer re-kinded
/// since refuses rather than undoing it.
#[test]
fn a_stale_write_keeps_a_record_changed_since_or_refuses() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    let heading = Change::new(vec![Step::SetBlock(quillmark_content::change::SetBlock {
        id: "b1".into(),
        kind: Some("heading".into()),
        attrs: serde_json::json!({"level": 2}).as_object().cloned(),
        path: None,
    })]);
    doc.main_mut().apply(Target::Body, &heading, Some(&base)).unwrap();
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "Revenue grew 12%.\n\n> Ship v2", Some(&base))
        .unwrap();
    assert!(receipt.rebased);
    let b1 = doc.main().body().blocks[1].clone();
    assert_eq!((b1.kind, b1.path.len()), (BlockKind::Heading { level: 2 }, 1));

    let held = doc.clone();
    let err = doc
        .main_mut()
        .revise(Target::Body, "Revenue grew 12%.\n\n### Ship v2", Some(&base))
        .unwrap_err();
    assert_eq!(err.code(), "edit::rebase_conflict");
    assert_eq!(err.args()["reason"], serde_json::json!("changed"));
    assert_eq!(doc, held);
}

/// A revise keeps the ids of the blocks it kept and rides an anchor on the
/// text it kept.
#[test]
fn revise_keeps_what_it_kept() {
    let mut doc = parse(SOURCE);
    let mut body = doc.main().read(Target::Body).unwrap();
    body.anchors.push(Anchor::new("c1".to_string(), 8, 12));
    doc.main_mut().replace(Target::Body, &body, None).unwrap();
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "Revenue grew 14%.\n\nShip v2 now", None)
        .unwrap();
    assert!(receipt.dropped_anchors.is_empty(), "{:?}", receipt.dropped_anchors);
    let body = doc.main().body();
    assert_eq!(body.text, "Revenue grew 14%.\nShip v2 now");
    let ids: Vec<&str> = body.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b1"]);
    assert_eq!(body.anchors, [Anchor::new("c1".to_string(), 8, 12)]);
}

/// A replace carries each anchor the new value does not name onto the text
/// the diff keeps, and names the one whose text it deleted.
#[test]
fn replace_carries_the_anchors_it_does_not_name() {
    let mut doc = parse(SOURCE);
    let mut body = doc.main().read(Target::Body).unwrap();
    body.anchors.push(Anchor::new("kept".to_string(), 0, 7));
    body.anchors.push(Anchor::new("gone".to_string(), 23, 25));
    doc.main_mut().replace(Target::Body, &body, None).unwrap();
    let fresh = quillmark_content::markdown::from_markdown("Revenue grew 12%.\n\nShip").unwrap().content;
    let receipt = doc.main_mut().replace(Target::Body, &fresh, None).unwrap();
    let dropped: Vec<&str> = receipt.dropped_anchors.iter().map(|a| a.context.id.as_str()).collect();
    assert_eq!(dropped, ["gone"]);
    assert_eq!(doc.main().body().anchors, [Anchor::new("kept".to_string(), 0, 7)]);
}

/// A richtext field is a target like the body: an absent one is the empty
/// content, and the write stores the model's value.
#[test]
fn a_field_takes_a_change_from_empty() {
    let mut doc = parse(SOURCE);
    let insert = Change::new(vec![Step::Splice(Splice::new(0, String::new(), "Hello".to_string()))]);
    doc.main_mut().apply(Target::Field("subject"), &insert, None).unwrap();
    let stored = doc.main().payload().get("subject").unwrap().as_json();
    assert_eq!(stored, &serde_json::json!({"text": "Hello", "blocks": [{"id": "b0", "kind": "para"}]}));
    assert_eq!(doc.main().read(Target::Field("subject")).unwrap().text, "Hello");
}

/// `apply_all` lands every write or none, and resolves every card id before
/// it applies any write: an unknown id refuses ahead of a stale base.
#[test]
fn apply_all_is_all_or_nothing() {
    let mut doc = parse(SOURCE);
    let held = doc.clone();
    let writes = [
        Write { card: CardRef::Main, target: Target::Body, change: split_at(12, "b9"), base: None },
        Write { card: CardRef::Main, target: Target::Field("subject"), change: Change::new(Vec::new()), base: Some("stale") },
    ];
    let (at, err) = doc.apply_all(&writes).unwrap_err();
    assert_eq!((at, err.code()), (1, "edit::stale_base"));
    assert_eq!(doc, held);
    let unknown = Write { card: CardRef::Id("nope"), ..writes[0].clone() };
    let (at, err) = doc.apply_all(&[writes[1].clone(), unknown]).unwrap_err();
    assert_eq!((at, err.code()), (1, "edit::unknown_card_id"));

    let receipts = doc.apply_all(&writes[..1]).unwrap();
    assert_eq!(receipts[0].hash, doc.main().hash_of(Target::Body).unwrap());
}

/// A write names its card by id, so a card moved or inserted ahead of it
/// between the read and the write, its body equal to the one read, leaves
/// the write on the card its writer read.
#[test]
fn a_write_lands_on_the_card_its_writer_read() {
    let three = "~~~\n$quill: q\n~~~\n\n~~~card-yaml\n$kind: note\n$id: aaa\n~~~\n\nHello\n\n\
                 ~~~card-yaml\n$kind: note\n$id: bbb\n~~~\n\nHello\n\n~~~card-yaml\n$kind: note\n$id: ccc\n~~~\n\nHello\n";
    let bodies = |doc: &crate::document::Document| -> Vec<(String, String)> {
        doc.cards().iter().map(|c| (c.id().unwrap().to_string(), c.body().text.clone())).collect()
    };
    let shuffles: [fn(&mut crate::document::Document); 2] = [
        |doc| doc.move_card(2, 1).unwrap(),
        |doc| {
            let mut card = Card::new("note").unwrap();
            card.overwrite_body(quillmark_content::lines::import::from_markdown("Hello").unwrap().content);
            doc.insert_card(1, card).unwrap();
        },
    ];
    for shuffle in shuffles {
        let mut doc = parse(three);
        let base = doc.card_by_id("bbb").unwrap().hash_of(Target::Body).unwrap();
        shuffle(&mut doc);
        let write = Write {
            card: CardRef::Id("bbb"),
            target: Target::Body,
            change: Change::new(vec![Step::Splice(Splice::new(5, "", " world"))]),
            base: Some(&base),
        };
        doc.apply_all(&[write]).unwrap();
        let written: Vec<String> = bodies(&doc).into_iter().filter(|(_, t)| t != "Hello").map(|(id, _)| id).collect();
        assert_eq!(written, ["bbb"]);
        assert_eq!(doc.card_by_id("bbb").unwrap().body().text, "Hello world");
    }
}

/// Markdown cannot spell an `x-` island, so a revise from a plain read, or a
/// replace from a model that holds none, refuses to remove one; a change of
/// its own through `apply` removes it.
#[test]
fn a_revise_or_replace_refuses_to_remove_an_x_island() {
    use quillmark_content::change::IslandStep;
    use quillmark_content::lines::island::IslandType;
    use quillmark_content::model::Island;

    let mut doc = parse(SOURCE);
    let island = Island::new("isl-x", IslandType::Ext("x-chart".into())).with_props(serde_json::json!({"series": [1]}));
    let insert = Change::new(vec![Step::Island(IslandStep::Insert { at: 7, island, block: None })]);
    doc.main_mut().apply(Target::Body, &insert, None).unwrap();
    let held = doc.clone();
    let plain = quillmark_content::markdown::to_markdown(doc.main().body());
    let err = doc.main_mut().revise(Target::Body, &plain, None).unwrap_err();
    assert_eq!(err.code(), "edit::drops_ext_island");
    let without = quillmark_content::markdown::from_markdown(&plain).unwrap().content;
    let err = doc.main_mut().replace(Target::Body, &without, None).unwrap_err();
    assert_eq!((err.code(), err.args()["island"].as_str()), ("edit::drops_ext_island", Some("isl-x")));
    assert_eq!(doc, held);

    let removed = invert(&insert, &parse(SOURCE).main().read(Target::Body).unwrap()).unwrap();
    doc.main_mut().apply(Target::Body, &removed, None).unwrap();
    assert!(doc.main().body().islands.is_empty());
}
