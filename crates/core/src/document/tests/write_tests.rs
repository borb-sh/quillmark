use quillmark_content::change::{AnchorStep, Change, Splice, Step, apply, invert};
use quillmark_content::model::{Anchor, Block, BlockKind};

use crate::document::{Card, Document, EditError, Target};

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
        .apply(Target::Body, &split_at(12, "b9"), Some(base.as_str()))
        .unwrap();
    let body = doc.main().body();
    assert_eq!(body.text, "Revenue grew\n12%.\nShip v2");
    let ids: Vec<&str> = body.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b9", "b1"]);
    assert_eq!(receipt.hash, doc.main().hash_of(Target::Body).unwrap());

    let undo = invert(&receipt.applied, &before).unwrap();
    assert_eq!(apply(body, &undo).unwrap().content, before);
}

/// Writes, with no base, each `(at, keys)` as `keys` steps that each type
/// one character at `at`, answering the hash each write left.
fn type_keys(doc: &mut crate::document::Document, writes: impl IntoIterator<Item = (usize, usize)>) -> Vec<String> {
    writes
        .into_iter()
        .map(|(at, keys)| {
            let key = || Step::Splice(Splice::new(at, String::new(), "x".to_string()));
            doc.main_mut().apply(Target::Body, &Change::new((0..keys).map(|_| key()).collect()), None).unwrap().hash
        })
        .collect()
}

/// A base read before a thousand keystrokes at two places merges a write
/// onto the text it read, and `read_at` answers the value it named, as does
/// a base read halfway through.
#[test]
fn a_base_from_before_a_thousand_keystrokes_merges() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    let hashes = type_keys(&mut doc, (0..500).chain(525..1025).map(|at| (at, 1)));
    let typed = "x".repeat(500);
    assert_eq!(doc.main_mut().read_at(Target::Body, Some(&base)).unwrap(), parse(SOURCE).main().read(Target::Body).unwrap());
    assert_eq!(doc.main_mut().read_at(Target::Body, Some(&hashes[499])).unwrap().text, format!("{typed}Revenue grew 12%.\nShip v2"));
    let receipt = doc
        .main_mut()
        .apply(Target::Body, &split_at(7, "b8"), Some(base.as_str()))
        .unwrap();
    assert!(receipt.rebased);
    assert_eq!(doc.main().body().text, format!("{typed}Revenue\ngrew 12%.\nShip v2{typed}"));
}

/// A document read from storage holds no history, so a base it was left at
/// before the store reaches no heads and refuses the write, which changes
/// nothing; a refused change writes nothing either.
#[test]
fn a_base_from_before_storage_or_a_refused_change_writes_nothing() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    type_keys(&mut doc, [(0, 3)]);
    let mut doc = Document::from_stored(&serde_json::to_string(&doc).unwrap()).unwrap().document;
    let held = doc.clone();
    let err = doc
        .main_mut()
        .apply(Target::Body, &split_at(7, "b8"), Some(base.as_str()))
        .unwrap_err();
    assert_eq!(err.code(), "edit::stale_base");
    let err = doc.main_mut().apply(Target::Body, &split_at(2, "b8"), None).unwrap_err();
    assert!(matches!(err, EditError::Change(_)), "{err:?}");
    assert_eq!(doc, held);
    assert_eq!(doc.main().body().text, "xxxRevenue grew 12%.\nShip v2");
}

/// A base read before a write of more steps than a keystroke's thousands
/// merges with it.
#[test]
fn a_base_merges_with_a_write_of_many_steps() {
    let mut doc = parse(SOURCE);
    type_keys(&mut doc, (0..3).map(|at| (at, 1)));
    let base = doc.main().hash_of(Target::Body).unwrap();
    type_keys(&mut doc, [(0, 8193)]);
    let receipt = doc
        .main_mut()
        .apply(Target::Body, &split_at(10, "b8"), Some(base.as_str()))
        .unwrap();
    assert!(receipt.rebased);
    assert_eq!(doc.main().body().text, format!("{}Revenue\ngrew 12%.\nShip v2", "x".repeat(8196)));
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
        .apply(Target::Body, &split_at(7, "b8"), Some(base.as_str()))
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
    let place = Change::new(vec![Step::Anchor(AnchorStep::Place {
        anchor: Anchor::new("c1".to_string(), 13, 16),
        quote: Some("12%".to_string()),
    })]);
    let receipt = doc.main_mut().apply(Target::Body, &place, Some(base.as_str())).unwrap();
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
        .revise(Target::Body, "Revenue grew 14%.\n\nShip v2", Some(base.as_str()))
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
    doc.main_mut().revise(Target::Body, "Alpha\n\nInserted one\n\nBeta\n\nGamma\n", Some(base.as_str())).unwrap();
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "Alpha\n\nBeta\n\nGamma\n\nInserted two\n", Some(base.as_str()))
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
    doc.main_mut().revise(Target::Body, "Alpha[^a]\n\n- one\n\nGamma\n\n[^a]: first", Some(base.as_str())).unwrap();
    let mine = quillmark_content::markdown::from_markdown("Alpha\n\nGamma[^b]\n\n- two\n\n[^b]: second").unwrap().content;
    let receipt = doc.main_mut().replace(Target::Body, &mine, Some(base.as_str())).unwrap();
    assert!(receipt.rebased);
    let body = doc.main().body();
    assert_eq!((body.containers.len(), body.islands.len()), (4, 2));
    assert_eq!(quillmark_content::markdown::to_markdown(body), "Alpha[^1]\n\n- one\n\nGamma[^2]\n\n- two\n\n[^1]: first\n\n[^2]: second");
}

/// Two writers who rewrote the same text both land, as Automerge merges
/// them; two who set one block record conflict, and the later one is
/// refused with the step that conflicts and why, writing nothing.
#[test]
fn overlapping_text_merges_and_a_record_both_set_refuses() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().revise(Target::Body, "Revenue grew 15%.\n\nShip v2", None).unwrap();
    let receipt = doc.main_mut().revise(Target::Body, "Revenue grew 14%.\n\nShip v2", Some(base.as_str())).unwrap();
    assert!(receipt.rebased);
    let text = doc.main().body().text.clone();
    assert!(text.contains('4') && text.contains('5') && !text.contains("12"), "{text}");

    let base = doc.main().hash_of(Target::Body).unwrap();
    let heading = |level: u8| {
        Change::new(vec![Step::SetBlock(quillmark_content::change::SetBlock {
            id: "b1".into(),
            kind: Some("heading".into()),
            attrs: serde_json::json!({ "level": level }).as_object().cloned(),
            path: None,
        })])
    };
    doc.main_mut().apply(Target::Body, &heading(2), None).unwrap();
    let held = doc.clone();
    let err = doc.main_mut().apply(Target::Body, &heading(3), Some(base.as_str())).unwrap_err();
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
    doc.main_mut().apply(Target::Body, &heading, Some(base.as_str())).unwrap();
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "Revenue grew 12%.\n\n> Ship v2", Some(base.as_str()))
        .unwrap();
    assert!(receipt.rebased);
    let b1 = doc.main().body().blocks[1].clone();
    assert_eq!((b1.kind, b1.path.len()), (BlockKind::Heading { level: 2 }, 1));

    let held = doc.clone();
    let err = doc
        .main_mut()
        .revise(Target::Body, "Revenue grew 12%.\n\n### Ship v2", Some(base.as_str()))
        .unwrap_err();
    assert_eq!(err.code(), "edit::rebase_conflict");
    assert_eq!(err.args()["reason"], serde_json::json!("changed"));
    assert_eq!(doc, held);
}

/// A write mints from the value it lands on and the value its base names
/// alone, so equal values take one write to equal bytes whatever the card's
/// history; a stale step on the block it read, gone since, refuses as
/// `missing` rather than landing on the paragraph minted since under its
/// freed id.
#[test]
fn a_write_mints_from_the_values_alone_and_a_stale_step_on_a_freed_id_refuses() {
    let source = "~~~\n$quill: q\n~~~\n\nAlpha\n\nBeta\n\nGamma\n";
    let mut doc = parse(source);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().revise(Target::Body, "Alpha\n\nBeta", None).unwrap();
    let fresh = parse("~~~\n$quill: q\n~~~\n\nAlpha\n\nBeta\n");
    assert_eq!(doc.main().body(), fresh.main().body());
    let mut fresh = fresh;
    let delta = quillmark_content::markdown::from_markdown("Alpha\n\nBeta\n\nDelta").unwrap().content;
    let receipts = [&mut doc, &mut fresh].map(|d| d.main_mut().replace(Target::Body, &delta, None).unwrap());
    assert_eq!(receipts[0].hash, receipts[1].hash);
    let ids: Vec<&str> = doc.main().body().blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b1", "b2"]);

    let held = doc.clone();
    let err = doc.main_mut().revise(Target::Body, "Alpha\n\nBeta\n\n## Gamma", Some(base.as_str())).unwrap_err();
    assert_eq!((err.code(), &err.args()["reason"]), ("edit::rebase_conflict", &serde_json::json!("missing")));
    let heading = Change::new(vec![Step::SetBlock(quillmark_content::change::SetBlock {
        id: "b2".into(),
        kind: Some("heading".into()),
        attrs: serde_json::json!({"level": 1}).as_object().cloned(),
        path: None,
    })]);
    let err = doc.main_mut().apply(Target::Body, &heading, Some(base.as_str())).unwrap_err();
    assert_eq!((err.code(), &err.args()["reason"]), ("edit::rebase_conflict", &serde_json::json!("missing")));
    assert_eq!(doc, held);
}

/// A live editor that deletes a block and mints its id again for a new one
/// writes through `apply`; a stale step by that id refuses there too.
#[test]
fn a_stale_step_on_an_id_a_live_editor_minted_again_refuses() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nalpha\n\nbravo\n\ncharlie\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(11, "\ncharlie", ""))]), None).unwrap();
    let enter = Splice { blocks: vec![Block::new("b2".to_string(), BlockKind::Para)], ..Splice::new(5, "", "\nnew") };
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(enter)]), None).unwrap();
    assert_eq!(doc.main().body().blocks[1].id, "b2");
    let held = doc.clone();
    let heading = Change::new(vec![Step::SetBlock(quillmark_content::change::SetBlock {
        id: "b2".into(),
        kind: Some("heading".into()),
        attrs: serde_json::json!({"level": 1}).as_object().cloned(),
        path: None,
    })]);
    for err in [
        doc.main_mut().apply(Target::Body, &heading, Some(base.as_str())).unwrap_err(),
        doc.main_mut().revise(Target::Body, "alpha\n\nbravo\n\n# charlie", Some(base.as_str())).unwrap_err(),
    ] {
        assert_eq!((err.code(), &err.args()["reason"]), ("edit::rebase_conflict", &serde_json::json!("missing")));
    }
    assert_eq!(doc, held);
}

/// A stale revise that moves a block another writer re-kinded since refuses
/// rather than writing it back as it read it.
#[test]
fn a_stale_move_of_a_block_re_kinded_since_refuses() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nAlpha\n\nBeta\n\nGamma\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().revise(Target::Body, "Alpha\n\nBeta\n\n## Gamma", Some(base.as_str())).unwrap();
    let held = doc.clone();
    let err = doc.main_mut().revise(Target::Body, "Alpha\n\nGamma\n\nBeta", Some(base.as_str())).unwrap_err();
    assert_eq!(err.code(), "edit::rebase_conflict");
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

/// A write that finds its card by id, after a card moved or was inserted
/// ahead of it since the read, its body equal to the one read, lands on the
/// card its writer read.
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
            doc.unchecked().insert_card(Some(1), card, None).unwrap();
        },
    ];
    for shuffle in shuffles {
        let mut doc = parse(three);
        let base = doc.card_by_id("bbb").unwrap().hash_of(Target::Body).unwrap();
        shuffle(&mut doc);
        let at = doc.card_index("bbb").unwrap();
        let change = Change::new(vec![Step::Splice(Splice::new(5, "", " world"))]);
        doc.card_mut(at).unwrap().apply(Target::Body, &change, Some(base.as_str())).unwrap();
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

fn content(v: serde_json::Value) -> quillmark_content::model::Content {
    quillmark_content::model::Content::from_value(&v).unwrap_or_else(|e| panic!("{e}: {v}"))
}

fn paras(text: &str) -> serde_json::Value {
    let blocks: Vec<_> = (0..text.split('\n').count()).map(|k| serde_json::json!({"id": format!("b{k}"), "kind": "para"})).collect();
    serde_json::json!({"text": text, "blocks": blocks})
}

/// Valid values whose markdown read spells less than they hold: an editor's
/// empty paragraph (between blocks, at either end, holding a break, ending a
/// note, between two lists), formatting inside link text, a lone or trailing
/// hard break, a cell holding two breaks in a row, a strong mark inside
/// a code mark, a point anchor at either edge.
fn unspelled() -> Vec<(&'static str, quillmark_content::model::Content)> {
    use serde_json::json;
    let list = |id: &str, item: &str, ordered: bool| {
        [json!({"id": id, "kind": "list", "attrs": {"ordered": ordered, "start": 1}}), json!({"id": item, "kind": "item"})]
    };
    let [l0, i0] = list("l0", "i0", true);
    let [l1, i1] = list("l1", "i1", true);
    let mut anchored = content(paras("a\nb"));
    anchored.anchors = vec![Anchor::new("p0", 0, 0), Anchor::new("p1", 3, 3)];
    vec![
        ("empty paragraph between", content(paras("Alpha.\n\nBeta."))),
        ("empty paragraph first", content(paras("\nBeta."))),
        ("empty paragraph last", content(paras("Alpha.\n"))),
        ("break-only paragraph", content(paras("Alpha.\n\u{2028}\nBeta."))),
        ("lone break", content(paras("\u{2028}"))),
        ("trailing break", content(paras("a\u{2028}"))),
        ("two lists around an empty paragraph", content(json!({
            "text": "a\n\nb",
            "blocks": [{"id": "b0", "kind": "para", "path": ["l0", "i0"]}, {"id": "b1", "kind": "para"}, {"id": "b2", "kind": "para", "path": ["l1", "i1"]}],
            "containers": [l0, i0, l1, i1],
        }))),
        ("emphasis in link text", content(json!({
            "text": "see the official docs now", "blocks": [{"id": "b0", "kind": "para"}],
            "marks": [{"start": 4, "end": 21, "type": "link", "attrs": {"url": "https://x.io"}}, {"start": 8, "end": 16, "type": "emph"}],
        }))),
        ("code in link text", content(json!({
            "text": "see the code docs", "blocks": [{"id": "b0", "kind": "para"}],
            "marks": [{"start": 4, "end": 17, "type": "link", "attrs": {"url": "https://x.io"}}, {"start": 8, "end": 12, "type": "code"}],
        }))),
        ("cell: two breaks", content(json!({
            "text": "\u{FFFC}", "blocks": [{"id": "b0", "kind": "para"}],
            "islands": [{"id": "t0", "type": "table",
                "props": {"columns": ["k0"], "rows": [{"id": "r0", "cells": {"k0": "s0"}}, {"id": "r1", "cells": {"k0": "s1"}}]},
                "slots": {"s0": paras("H"), "s1": paras("a\u{2028}\u{2028}b")}}],
        }))),
        ("strong inside code", content(json!({
            "text": "abcdef", "blocks": [{"id": "b0", "kind": "para"}],
            "marks": [{"start": 0, "end": 6, "type": "code"}, {"start": 2, "end": 4, "type": "strong"}],
        }))),
        ("point anchors at the edges", anchored),
    ]
}

/// A revise with the value's own annotated read writes nothing, whatever the
/// markdown cannot spell; one that edits another paragraph keeps it too.
#[test]
fn a_revise_with_the_annotated_read_keeps_what_markdown_cannot_spell() {
    for (name, value) in unspelled() {
        let mut doc = parse(SOURCE);
        doc.main_mut().replace(Target::Body, &value, None).unwrap();
        let stored = doc.main().body().clone();
        let read = quillmark_content::markdown::to_markdown_annotated(&stored).markdown;
        let receipt = doc.main_mut().revise(Target::Body, &read, None).unwrap();
        assert_eq!(doc.main().body(), &stored, "{name}: {read:?}");
        assert!(receipt.applied.is_empty(), "{name}: {:?}", receipt.applied);
    }
    let mut doc = parse(SOURCE);
    for (name, value) in unspelled().into_iter().filter(|(n, _)| n.starts_with("empty paragraph between") || n.contains("link")) {
        let mut edited = value.clone();
        edited.text.push_str("\nTail.");
        edited.blocks.push(Block::new("tail", BlockKind::Para));
        doc.main_mut().replace(Target::Body, &edited, None).unwrap();
        let stored = doc.main().body().clone();
        let read = quillmark_content::markdown::to_markdown_annotated(&stored).markdown;
        doc.main_mut().revise(Target::Body, &read.replace("Tail.", "Tail, edited."), None).unwrap();
        assert_eq!(doc.main().body().text, stored.text.replace("Tail.", "Tail, edited."), "{name}");
        assert_eq!(doc.main().body().marks, stored.marks, "{name}");
        assert_eq!(doc.main().body().blocks, stored.blocks, "{name}");
    }
}

/// A revise that rewrites the text under what its read could not spell
/// lands as written, and one that rewrites what the read re-spelled lands
/// merged with what the read lost.
#[test]
fn a_revise_over_what_its_read_could_not_spell_lands_as_written_or_conflicts() {
    let shape = |name: &str| unspelled().into_iter().find(|(n, _)| *n == name).unwrap().1;
    let mut doc = parse(SOURCE);
    doc.main_mut().replace(Target::Body, &shape("emphasis in link text"), None).unwrap();
    doc.main_mut().revise(Target::Body, "see [the unofficial docs](https://x.io) now", None).unwrap();
    let body = doc.main().body();
    assert_eq!(body.text, "see the unofficial docs now");
    assert!(body.marks.iter().all(|m| m.kind != quillmark_content::model::MarkKind::Emph), "{:?}", body.marks);

    let mut doc = parse(SOURCE);
    doc.main_mut().replace(Target::Body, &shape("cell: two breaks"), None).unwrap();
    let read = quillmark_content::markdown::to_markdown_annotated(doc.main().body()).markdown;
    doc.main_mut().revise(Target::Body, &read.replace("a<br><br>b", "z"), None).unwrap();
    let island = &doc.main().body().islands[0];
    let cell = &island.slots[&island.slot_order()[1]].text;
    assert!(cell.starts_with('z') && !cell.contains(['a', 'b']), "{cell:?}");
}

fn anchored(body: &str, anchors: &[(&str, &str)]) -> crate::document::Document {
    let mut doc = parse(&format!("~~~\n$quill: q\n~~~\n\n{body}"));
    let text = doc.main().body().text.clone();
    let steps = anchors
        .iter()
        .map(|(id, quote)| {
            let start = text[..text.find(quote).unwrap()].chars().count();
            Step::Anchor(AnchorStep::Place { anchor: Anchor::new(*id, start, start + quote.chars().count()), quote: None })
        })
        .collect();
    doc.main_mut().apply(Target::Body, &Change::new(steps), None).unwrap();
    doc
}

fn quote_of(doc: &crate::document::Document, id: &str) -> Option<String> {
    let body = doc.main().body();
    let chars: Vec<char> = body.text.chars().collect();
    body.anchors.iter().find(|a| a.id == id).map(|a| chars[a.start..a.end].iter().collect())
}

/// A revise and a replace carry an anchor through the one word diff: a word
/// rewritten under the anchor's end leaves the anchor on the words kept, and
/// a punctuation mark changed beside the anchored word leaves the word.
#[test]
fn a_revise_and_a_replace_carry_an_anchor_through_one_word_diff() {
    let doc = anchored("The quick brown fox jumps over the lazy dog.\n", &[("c1", "brown fox"), ("c2", "dog")]);
    let md = "The quick brown wolf jumps over the lazy dog!";
    let mut revised = doc.clone();
    let receipt = revised.main_mut().revise(Target::Body, md, None).unwrap();
    let mut replaced = doc.clone();
    replaced.main_mut().replace(Target::Body, &quillmark_content::markdown::from_markdown(md).unwrap().content, None).unwrap();
    assert_eq!(quote_of(&revised, "c1").as_deref(), Some("brown "));
    assert_eq!(quote_of(&revised, "c2").as_deref(), Some("dog"));
    assert_eq!(revised.main().body().anchors, replaced.main().body().anchors);
    let moved: Vec<_> = receipt.moved_anchors.iter().map(|m| (m.id.as_str(), m.reason.name())).collect();
    assert_eq!(moved, [("c1", "context"), ("c2", "context")]);
}

/// A replaced run's lines pair by best similarity, so a paragraph the writer
/// inserts that resembles the one it edited takes neither that paragraph's
/// id nor its anchor.
#[test]
fn an_inserted_look_alike_paragraph_takes_no_id_or_anchor_from_the_edited_one() {
    let mut doc = anchored("Intro.\n\nThe meeting is on Monday at noon.\n\nOther text here.\n", &[("c1", "The meeting")]);
    let edited = doc.main().body().blocks[1].id.clone();
    let md = "Intro.\n\nThe meeting room is on floor three.\n\nThe meeting is on Tuesday at noon.\n\nOther text here.";
    doc.main_mut().revise(Target::Body, md, None).unwrap();
    let body = doc.main().body();
    assert_eq!(body.blocks[2].id, edited, "{:?}", body.blocks);
    assert_eq!(body.block_at(body.anchors[0].start), 2);
}

/// One receipt spells anchor text one way, as stored: a dropped anchor's
/// quote and a moved anchor's `before` both hold a hard break as `U+2028`,
/// as the selector `anchor_contexts` read before the write did.
#[test]
fn a_receipt_spells_a_dropped_and_a_moved_anchor_as_stored() {
    let mut doc = anchored("Alpha\\\nbeta gamma\\\ndelta.\n", &[("c1", "Alpha\u{2028}beta"), ("c2", "gamma\u{2028}delta")]);
    let read = doc.anchor_contexts();
    assert_eq!(read[0].quote, "Alpha\u{2028}beta");
    let receipt = doc.main_mut().revise(Target::Body, "gamma\\\ndelt.", None).unwrap();
    assert_eq!(receipt.dropped_anchors.iter().map(|d| d.context.quote.as_str()).collect::<Vec<_>>(), ["Alpha\u{2028}beta"]);
    assert_eq!(receipt.dropped_anchors[0].context, read[0].context());
    assert_eq!(receipt.moved_anchors.iter().map(|m| (m.id.as_str(), m.before.as_str())).collect::<Vec<_>>(), [("c2", "gamma\u{2028}delta")]);
}

/// A write past an input bound refuses under the bound's code before any of
/// it runs, and writes nothing: markdown past 10 MiB, a change past
/// `MAX_CHANGE_STEPS`.
#[test]
fn a_write_past_an_input_bound_refuses_under_its_code() {
    let mut doc = parse(SOURCE);
    let held = doc.clone();
    let huge = "a".repeat(quillmark_content::MAX_MARKDOWN_SIZE + 1);
    let err = doc.main_mut().revise(Target::Body, &huge, None).unwrap_err();
    assert_eq!(err.code(), "edit::input_too_large");
    let steps = (0..=crate::document::MAX_CHANGE_STEPS).map(|_| Step::Splice(Splice::new(0, String::new(), "x".to_string())));
    let err = doc.main_mut().apply(Target::Body, &Change::new(steps.collect()), None).unwrap_err();
    assert_eq!(err.code(), "edit::too_many_steps");
    assert_eq!(doc, held);
}

/// The hash a write leaves is the one the next write reads, and a write
/// around the content lane, to the body or a field, leaves none behind: the
/// hash read after it is the value's own.
#[test]
fn a_remembered_hash_is_the_values_own() {
    use crate::document::base::content_hash;
    let mut doc = parse(SOURCE);
    let notes = Target::Field("notes");
    for target in [Target::Body, notes] {
        let receipt = doc.main_mut().apply(target, &Change::new(vec![Step::Splice(Splice::new(0, String::new(), "x".to_string()))]), None).unwrap();
        assert_eq!(receipt.hash, content_hash(&doc.main().read(target).unwrap()));
    }
    doc.unchecked().store_field(None, "notes", crate::value::QuillValue::from_json(serde_json::json!("plain"))).unwrap();
    doc.main_mut().revise(crate::document::Target::Body, "Fresh body.", None).unwrap();
    for target in [Target::Body, notes] {
        assert_eq!(doc.main().hash_of(target).unwrap(), content_hash(&doc.main().read(target).unwrap()));
    }
}

/// Two live writers make structural edits from one read under their own actors: each
/// verb answers its receipt, the second rebased, and their mints differ. A
/// placeholder in a stale apply is filled past the value it lands on and named
/// in `minted`; a revise under an actor mints under it, and a bad actor
/// writes nothing.
#[test]
fn structural_edits_and_placeholders_mint_under_the_writers_actor() {
    use crate::document::WriteOptions;
    use quillmark_content::structural::Wrap;
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nalpha\n\nbravo\n\ncharlie\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    let ann = WriteOptions { base: Some(crate::document::Base::Hash(&base)), actor: Some("ann") };
    let bo = WriteOptions { base: Some(crate::document::Base::Hash(&base)), actor: Some("bo") };
    let quoted = doc.main_mut().wrap(Target::Body, "b0", "b1", Wrap::Quote, ann).unwrap();
    assert_eq!(quoted.minted.get("?0").map(String::as_str), Some("ann:q0"));
    let split = doc.main_mut().split_block(Target::Body, 14, None, bo).unwrap();
    assert!(split.rebased);
    assert_eq!(split.minted.get("?0").map(String::as_str), Some("bo:b0"));
    let listed = doc.main_mut().wrap(Target::Body, "b2", "b2", Wrap::List { ordered: true, checked: None }, bo).unwrap();
    assert!(listed.rebased);
    assert_eq!(listed.minted.values().map(String::as_str).collect::<Vec<_>>(), ["bo:l0", "bo:i0"]);
    let body = doc.main().body();
    let ids: Vec<&str> = body.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b1", "b2", "bo:b0"]);
    assert_eq!(body.blocks[2].path, ["bo:l0", "bo:i0"]);
    let located = doc.main().block_at(Target::Body, 7).unwrap();
    assert_eq!((located.id.as_str(), located.index, located.path.as_slice()), ("b1", 1, &["ann:q0".to_string()][..]));

    let insert = Change::new(vec![
        Step::Splice(Splice { blocks: vec![Block::new("?p".to_string(), BlockKind::Para)], ..Splice::new(19, "", "\nnew") }),
        Step::SetBlock(quillmark_content::change::SetBlock {
            id: "?p".into(),
            kind: Some("heading".into()),
            attrs: serde_json::json!({"level": 2}).as_object().cloned(),
            path: None,
        }),
    ]);
    let filled = doc.main_mut().apply(Target::Body, &insert, Some(base.as_str())).unwrap();
    assert!(filled.rebased);
    assert_eq!(filled.minted.get("?p").map(String::as_str), Some("b3"));
    assert!(doc.main().body().blocks.iter().any(|b| b.id == "b3" && matches!(b.kind, BlockKind::Heading { level: 2 })));

    let now = doc.main().hash_of(Target::Body).unwrap();
    let more = format!("{}\n\nend\n", doc.main().body_annotated().markdown.trim_end());
    let revised = doc.main_mut().revise(Target::Body, &more, WriteOptions { base: Some(crate::document::Base::Hash(&now)), actor: Some("cy") }).unwrap();
    assert_eq!(doc.main().body().blocks.last().unwrap().id, "cy:b0");
    assert!(revised.minted.is_empty());

    let held = doc.clone();
    let err = doc.main_mut().join_blocks(Target::Body, "b1", WriteOptions { base: None, actor: Some("a:b") }).unwrap_err();
    assert_eq!(err.code(), "edit::invalid_actor");
    assert_eq!(doc, held);
}


/// A stale write and a change since that each delete part of an anchor's
/// text take it out together, and the receipt names it though neither alone
/// dropped it.
#[test]
fn a_merge_names_an_anchor_two_deletions_took_out_together() {
    let mut doc = anchored("Hello brave new world", &[("c-1", "brave new")]);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(6, "brave ", ""))]), None).unwrap();
    assert!(doc.main().body().anchor("c-1").is_some());
    let receipt = doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(11, " new", ""))]), Some(base.as_str())).unwrap();
    assert!(receipt.rebased);
    assert_eq!(doc.main().body().text, "Hello  world");
    assert_eq!(receipt.dropped_anchors.iter().map(|d| d.context.id.as_str()).collect::<Vec<_>>(), ["c-1"]);
}

/// A value's live document saves to bytes another document loads, where a
/// base read before the load is stale and one read after it merges, and two
/// documents synced by messages read as one value.
#[test]
fn a_live_document_saves_loads_and_syncs() {
    use quillmark_automerge::automerge::sync::State;
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    type_keys(&mut doc, [(0, 2)]);
    let bytes = doc.main_mut().save(Target::Body).unwrap();

    let mut other = parse("~~~\n$quill: q\n~~~\n\nelse\n");
    let receipt = other.main_mut().load(Target::Body, &bytes).unwrap();
    assert_eq!(other.main().body(), doc.main().body());
    assert_eq!(receipt.hash, doc.main().hash_of(Target::Body).unwrap());
    let err = other.main_mut().apply(Target::Body, &split_at(7, "b8"), Some(base.as_str())).unwrap_err();
    assert_eq!(err.code(), "edit::stale_base");
    let loaded = other.main().hash_of(Target::Body).unwrap();
    type_keys(&mut other, [(0, 1)]);
    other.main_mut().apply(Target::Body, &split_at(9, "b8"), Some(loaded.as_str())).unwrap();
    assert_eq!(other.main().body().text, "xxxRevenue\ngrew 12%.\nShip v2");

    let (mut ours, mut theirs) = (State::new(), State::new());
    for _ in 0..8 {
        if let Some(m) = other.main_mut().generate_sync_message(Target::Body, &mut ours).unwrap() {
            doc.main_mut().receive_sync_message(Target::Body, &mut theirs, m).unwrap();
        }
        if let Some(m) = doc.main_mut().generate_sync_message(Target::Body, &mut theirs).unwrap() {
            other.main_mut().receive_sync_message(Target::Body, &mut ours, m).unwrap();
        }
    }
    assert_eq!(doc.main().body(), other.main().body());
    assert!(matches!(doc.main_mut().load(Target::Body, b"not a document"), Err(EditError::BadLiveBytes { .. })));
}

/// Runs the sync protocol between the bodies of `a` and `b` until neither
/// has a message left.
fn sync(a: &mut Document, b: &mut Document) {
    use quillmark_automerge::automerge::sync::State;
    let (mut ours, mut theirs) = (State::new(), State::new());
    for _ in 0..16 {
        let mut quiet = true;
        if let Some(m) = a.main_mut().generate_sync_message(Target::Body, &mut ours).unwrap() {
            b.main_mut().receive_sync_message(Target::Body, &mut theirs, m).unwrap();
            quiet = false;
        }
        if let Some(m) = b.main_mut().generate_sync_message(Target::Body, &mut theirs).unwrap() {
            a.main_mut().receive_sync_message(Target::Body, &mut ours, m).unwrap();
            quiet = false;
        }
        if quiet {
            return;
        }
    }
    panic!("the peers still have messages after sixteen rounds");
}

/// Two peers writing with no actor sync and converge, one loaded from the
/// other's save or both built from one value, since each live document
/// writes under its own actor and equal values build one first change.
#[test]
fn peers_writing_with_no_actor_sync_from_one_origin() {
    let typed = |doc: &mut Document, at: usize, s: &str| {
        doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(at, "", s))]), None).unwrap();
    };
    let mut a = parse(SOURCE);
    typed(&mut a, 0, "A");
    let mut b = parse("~~~\n$quill: q\n~~~\n\nelse\n");
    b.main_mut().load(Target::Body, &a.main_mut().save(Target::Body).unwrap()).unwrap();
    typed(&mut a, 0, "1");
    typed(&mut b, 0, "2");
    sync(&mut a, &mut b);
    assert_eq!(a.main().body(), b.main().body());
    assert!(["12ARevenue", "21ARevenue"].iter().any(|s| a.main().body().text.starts_with(s)), "{:?}", a.main().body().text);

    let (mut a, mut b) = (parse(SOURCE), parse(SOURCE));
    typed(&mut a, 0, "1");
    typed(&mut b, 7, "2");
    sync(&mut a, &mut b);
    assert_eq!(a.main().body(), b.main().body());
    assert_eq!(a.main().body().text, "1Revenue2 grew 12%.\nShip v2");
}

/// Bytes or a sync message that leave the live document holding no value
/// in its shape are refused as `edit::bad_live_bytes` with their reason, and
/// the value, its document and the sync state stay as they were.
#[test]
fn a_live_document_refuses_bytes_or_a_message_it_cannot_read() {
    use quillmark_automerge::automerge::sync::{State, SyncDoc};
    use quillmark_automerge::automerge::transaction::Transactable;
    use quillmark_automerge::automerge::{AutoCommit, ObjType, ROOT, ReadDoc};
    let mut bad = AutoCommit::new();
    let v = bad.put_object(&ROOT, "value", ObjType::Map).unwrap();
    let t = bad.put_object(&v, "text", ObjType::Text).unwrap();
    bad.splice_text(&t, 0, 0, "hi\nthere").unwrap();
    let address = crate::document::write::address(None, Target::Body);
    bad.commit_with(quillmark_automerge::automerge::transaction::CommitOptions::default().with_message(address));

    let mut doc = parse(SOURCE);
    let before = doc.main().body().clone();
    let err = doc.main_mut().load(Target::Body, &bad.save()).unwrap_err();
    assert_eq!((err.code(), err.args()["reason"].as_str()), ("edit::bad_live_bytes", Some("shape")));
    let err = doc.main_mut().load(Target::Body, b"not a document").unwrap_err();
    assert_eq!(err.args()["reason"].as_str(), Some("decode"));

    let mut peer = AutoCommit::load(&doc.main_mut().save(Target::Body).unwrap()).unwrap();
    let v = peer.get(&ROOT, "value").unwrap().unwrap().1;
    peer.delete(&v, "containers").unwrap();
    let (mut ours, mut theirs) = (State::new(), State::new());
    let refused = (0..8).find_map(|_| {
        if let Some(m) = doc.main_mut().generate_sync_message(Target::Body, &mut ours).unwrap() {
            peer.sync().receive_sync_message(&mut theirs, m).unwrap();
        }
        let m = peer.sync().generate_sync_message(&mut theirs)?;
        let held = ours.clone();
        doc.main_mut().receive_sync_message(Target::Body, &mut ours, m).err().map(|e| (e, held))
    });
    let (err, held) = refused.expect("the message that brings the bad value is refused");
    assert_eq!(err.args()["reason"].as_str(), Some("shape"));
    assert_eq!((doc.main().body(), &ours), (&before, &held));
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(0, "", "x"))]), None).unwrap();
    assert_eq!(doc.main().body().text, "xRevenue grew 12%.\nShip v2");
}

/// A merge whose repair inserts stores the repaired value and leaves the
/// repair unwritten, so the live document reads through it; the next write
/// lowers into the document as a diff and lands where its offsets say.
#[test]
fn a_merge_whose_repair_inserts_leaves_it_unwritten() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nabc\n\n| aa | bb |\n| - | - |\n| cc | dd |\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(0, "abc\n", ""))]), None).unwrap();
    let receipt = doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(3, "", "d"))]), Some(base.as_str())).unwrap();
    assert!(receipt.rebased);
    let merged = doc.main().body().clone();
    let address = crate::document::write::address(None, Target::Body);
    let (_, read) = quillmark_automerge::Held::load(&doc.main_mut().save(Target::Body).unwrap(), &address).unwrap();
    assert_eq!(read.content, merged);
    assert!(read.repairs.iter().any(|r| r.inserts()), "{:?}", read.repairs);

    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(0, "", "x"))]), None).unwrap();
    assert_eq!(doc.main().body().text, format!("x{}", merged.text));
}

/// A value's live document is made at its address, the card and the field
/// it sits at: bytes or a sync message from the live document of another
/// address are refused as `address`, however equal the two values.
#[test]
fn a_live_document_refuses_the_history_of_another_address() {
    use quillmark_automerge::automerge::sync::State;
    let two = "~~~\n$quill: q\n~~~\n\n~~~card-yaml\n$kind: note\n$id: aaa\n~~~\n\nHello\n\n~~~card-yaml\n$kind: note\n$id: bbb\n~~~\n\nHello\n";
    let mut doc = parse(two);
    let held = doc.clone();
    let bytes = doc.card_mut(0).unwrap().save(Target::Body).unwrap();
    let err = doc.card_mut(1).unwrap().load(Target::Body, &bytes).unwrap_err();
    assert_eq!((err.code(), err.args()["reason"].as_str()), ("edit::bad_live_bytes", Some("address")));

    let (mut ours, mut theirs) = (State::new(), State::new());
    let mut peer = parse(two);
    let refused = (0..8).find_map(|_| {
        if let Some(m) = doc.card_mut(1).unwrap().generate_sync_message(Target::Body, &mut theirs).unwrap() {
            peer.card_mut(0).unwrap().receive_sync_message(Target::Body, &mut ours, m).err()
        } else {
            let m = peer.card_mut(0).unwrap().generate_sync_message(Target::Body, &mut ours).unwrap()?;
            doc.card_mut(1).unwrap().receive_sync_message(Target::Body, &mut theirs, m).err()
        }
    });
    let err = refused.expect("a message from another address is refused");
    assert_eq!(err.args()["reason"].as_str(), Some("address"));
    assert_eq!((&doc, &peer), (&held, &held));
}

/// A stale write that puts an anchor on text a write since deleted refuses
/// as `deleted` rather than landing with the anchor gone, a point or a
/// range; one whose text stayed lands.
#[test]
fn a_stale_anchor_on_text_deleted_since_refuses() {
    let place = |anchor: Anchor, quote: Option<&str>| Change::new(vec![Step::Anchor(AnchorStep::Place { anchor, quote: quote.map(str::to_string) })]);
    for (cut, anchor, quote) in [
        (Splice::new(10, "d", ""), Anchor::new("c-1".to_string(), 11, 11), None),
        (Splice::new(5, " brave", ""), Anchor::new("c-1".to_string(), 6, 11), Some("brave")),
    ] {
        let mut doc = parse("~~~\n$quill: q\n~~~\n\nHello brave world\n");
        let base = doc.main().hash_of(Target::Body).unwrap();
        let cut = if quote.is_some() { cut } else { Splice::new(16, "d", "") };
        let anchor = if quote.is_some() { anchor } else { Anchor::new("c-1".to_string(), 17, 17) };
        doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(cut)]), None).unwrap();
        let held = doc.clone();
        let err = doc.main_mut().apply(Target::Body, &place(anchor, quote), Some(base.as_str())).unwrap_err();
        assert_eq!((err.code(), &err.args()["reason"]), ("edit::rebase_conflict", &serde_json::json!("deleted")));
        assert_eq!(doc, held);
    }
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nHello brave world\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(0, "", "Oh "))]), None).unwrap();
    let receipt = doc.main_mut().apply(Target::Body, &place(Anchor::new("c-1".to_string(), 6, 11), Some("brave")), Some(base.as_str())).unwrap();
    assert_eq!(doc.main().body().anchors, [Anchor::new("c-1".to_string(), 9, 14)]);
    assert!(receipt.dropped_anchors.is_empty());
}

/// A stale anchor and a stale mark keep their edges over text a write since
/// typed at them, whichever of the two writers' actors sorts first.
#[test]
fn a_stale_anchor_and_mark_keep_their_edges_over_typing_at_them() {
    use quillmark_content::change::MarkStep;
    use quillmark_content::model::{Mark, MarkKind};
    for (typist, writer) in [("a", "z"), ("z", "a")] {
        let mut doc = parse("~~~\n$quill: q\n~~~\n\nHello world\n");
        let base = doc.main().hash_of(Target::Body).unwrap();
        let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "XX ")), Step::Splice(Splice::new(9, "", "YY"))]);
        doc.main_mut().apply(Target::Body, &typed, crate::document::WriteOptions { base: None, actor: Some(typist) }).unwrap();
        let put = Change::new(vec![
            Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1".to_string(), 6, 11), quote: Some("world".into()) }),
            Step::Mark(MarkStep::Add { mark: Mark::new(0, 5, MarkKind::Strong), quote: Some("Hello".into()) }),
        ]);
        let opts = crate::document::WriteOptions { base: Some(crate::document::Base::Hash(&base)), actor: Some(writer) };
        doc.main_mut().apply(Target::Body, &put, opts).unwrap();
        let body = doc.main().body();
        assert_eq!(body.text, "XX Hello YYworld", "{typist}");
        assert_eq!(body.anchors, [Anchor::new("c-1".to_string(), 11, 16)], "{typist}");
        assert_eq!(body.marks, [Mark::new(3, 8, MarkKind::Strong)], "{typist}");
    }
}

/// A stale revise from an annotated read that kept the tag of a comment
/// resolved since refuses as `missing` rather than bringing it back.
#[test]
fn a_stale_revise_does_not_resurrect_an_anchor_resolved_since() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nThe quick brown fox\n");
    let place = Change::new(vec![Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1".to_string(), 10, 15), quote: None })]);
    doc.main_mut().apply(Target::Body, &place, None).unwrap();
    let base = doc.main().hash_of(Target::Body).unwrap();
    let read = quillmark_content::markdown::to_markdown_annotated(doc.main().body()).markdown;
    assert!(read.contains("qm-anchor"), "{read}");
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Anchor(AnchorStep::Remove { id: "c-1".into() })]), None).unwrap();
    let held = doc.clone();
    let err = doc.main_mut().revise(Target::Body, &read.replace("brown", "red"), Some(base.as_str())).unwrap_err();
    assert_eq!((err.code(), &err.args()["reason"]), ("edit::rebase_conflict", &serde_json::json!("missing")));
    assert_eq!(doc, held);
}

/// A live document notes the heads of its latest 1024 hashes: a base older
/// than those is stale, and the one just inside them merges.
#[test]
fn a_base_past_the_noted_hashes_is_stale() {
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nx\n");
    let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "a"))]);
    let mut bases = vec![doc.main().hash_of(Target::Body).unwrap()];
    for _ in 0..1024 {
        doc.main_mut().apply(Target::Body, &typed, None).unwrap();
        bases.push(doc.main().hash_of(Target::Body).unwrap());
    }
    let at_end = Change::new(vec![Step::Splice(Splice::new(1, "", "!"))]);
    let err = doc.main_mut().apply(Target::Body, &at_end, Some(bases[0].as_str())).unwrap_err();
    assert_eq!(err.code(), "edit::stale_base");
    let at_end = Change::new(vec![Step::Splice(Splice::new(2, "", "!"))]);
    assert!(doc.main_mut().apply(Target::Body, &at_end, Some(bases[1].as_str())).unwrap().rebased);
}

/// A stale revise that rewrites a word another writer formatted since
/// refuses as `changed` rather than dropping the formatting, whichever
/// writer's actor sorts first; one over text typed elsewhere since lands.
#[test]
fn a_stale_revise_does_not_strip_formatting_put_since() {
    use crate::document::{Base, WriteOptions};
    use quillmark_content::change::MarkStep;
    use quillmark_content::model::{Mark, MarkKind};
    let formats = [MarkKind::Strong, MarkKind::Link { url: "https://x.io".into() }];
    for (kind, (typist, writer), reworded) in formats
        .iter()
        .flat_map(|k| [("a", "z"), ("z", "a")].map(|o| (k, o)))
        .flat_map(|(k, o)| ["Hello World, again", "Hello wOrld, again"].map(|r| (k.clone(), o, r)))
    {
        let mut doc = parse("~~~\n$quill: q\n~~~\n\nHello world, again\n");
        let base = doc.main().hash_of(Target::Body).unwrap();
        let mark = Change::new(vec![Step::Mark(MarkStep::Add { mark: Mark::new(6, 11, kind), quote: None })]);
        doc.main_mut().apply(Target::Body, &mark, WriteOptions { base: None, actor: Some(typist) }).unwrap();
        let held = doc.clone();
        let opts = WriteOptions { base: Some(Base::Hash(&base)), actor: Some(writer) };
        let err = doc.main_mut().revise(Target::Body, reworded, opts).unwrap_err();
        assert_eq!((err.code(), &err.args()["reason"]), ("edit::rebase_conflict", &serde_json::json!("changed")), "{reworded}");
        assert_eq!(doc, held);
    }
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nHello world, again\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().apply(Target::Body, &Change::new(vec![Step::Splice(Splice::new(0, "", "Oh, "))]), None).unwrap();
    doc.main_mut().revise(Target::Body, "Hello World, again", Some(base.as_str())).unwrap();
    assert_eq!(doc.main().body().text, "Oh, Hello World, again");
}
