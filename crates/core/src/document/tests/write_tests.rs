use quillmark_content::change::{AnchorStep, Change, Splice, Step, apply, invert};
use quillmark_content::model::{Anchor, Block, BlockKind};

use crate::document::{Card, EditError, Target};

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

/// A base the value has moved on from refuses each verb with the hash now and
/// writes nothing, as does a change that does not apply; a write under the
/// hash now lands.
#[test]
fn a_stale_base_or_a_refused_change_writes_nothing() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut()
        .apply(Target::Body, &split_at(12, "b9"), None)
        .unwrap();
    let held = doc.clone();
    let now = doc.main().hash_of(Target::Body).unwrap();
    let mine = quillmark_content::markdown::from_markdown("Revenue grew 14%.\n\nShip v2")
        .unwrap()
        .content;
    let errs = [
        doc.main_mut()
            .apply(Target::Body, &split_at(7, "b8"), Some(base.as_str()))
            .unwrap_err(),
        doc.main_mut()
            .revise(
                Target::Body,
                "Revenue grew 14%.\n\nShip v2",
                Some(base.as_str()),
            )
            .unwrap_err(),
        doc.main_mut()
            .replace(Target::Body, &mine, Some(base.as_str()))
            .unwrap_err(),
    ];
    for err in errs {
        assert_eq!(err.code(), "edit::stale_base");
        assert_eq!(err.args()["current"], serde_json::json!(now));
    }
    let err = doc
        .main_mut()
        .apply(Target::Body, &split_at(2, "b8"), None)
        .unwrap_err();
    assert!(matches!(err, EditError::Change(_)), "{err:?}");
    assert_eq!(doc, held);
    doc.main_mut()
        .apply(Target::Body, &split_at(7, "b8"), Some(now.as_str()))
        .unwrap();
    assert_eq!(doc.main().body().text, "Revenue\ngrew\n12%.\nShip v2");
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
    assert!(
        receipt.dropped_anchors.is_empty(),
        "{:?}",
        receipt.dropped_anchors
    );
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
    let fresh = quillmark_content::markdown::from_markdown("Revenue grew 12%.\n\nShip")
        .unwrap()
        .content;
    let receipt = doc.main_mut().replace(Target::Body, &fresh, None).unwrap();
    let dropped: Vec<&str> = receipt
        .dropped_anchors
        .iter()
        .map(|a| a.context.id.as_str())
        .collect();
    assert_eq!(dropped, ["gone"]);
    assert_eq!(
        doc.main().body().anchors,
        [Anchor::new("kept".to_string(), 0, 7)]
    );
}

/// A richtext field is a target like the body: an absent one is the empty
/// content, and the write stores the model's value.
#[test]
fn a_field_takes_a_change_from_empty() {
    let mut doc = parse(SOURCE);
    let insert = Change::new(vec![Step::Splice(Splice::new(
        0,
        String::new(),
        "Hello".to_string(),
    ))]);
    doc.main_mut()
        .apply(Target::Field("subject"), &insert, None)
        .unwrap();
    let stored = doc.main().payload().get("subject").unwrap().as_json();
    assert_eq!(
        stored,
        &serde_json::json!({"text": "Hello", "blocks": [{"id": "b0", "kind": "para"}]})
    );
    assert_eq!(
        doc.main().read(Target::Field("subject")).unwrap().text,
        "Hello"
    );
}

/// A write that finds its card by id, after a card moved or was inserted
/// ahead of it since the read, its body equal to the one read, lands on the
/// card its writer read.
#[test]
fn a_write_lands_on_the_card_its_writer_read() {
    let three = "~~~\n$quill: q\n~~~\n\n~~~card-yaml\n$kind: note\n$id: aaa\n~~~\n\nHello\n\n\
                 ~~~card-yaml\n$kind: note\n$id: bbb\n~~~\n\nHello\n\n~~~card-yaml\n$kind: note\n$id: ccc\n~~~\n\nHello\n";
    let bodies = |doc: &crate::document::Document| -> Vec<(String, String)> {
        doc.cards()
            .iter()
            .map(|c| (c.id().unwrap().to_string(), c.body().text.clone()))
            .collect()
    };
    let shuffles: [fn(&mut crate::document::Document); 2] = [
        |doc| doc.move_card(2, 1).unwrap(),
        |doc| {
            let mut card = Card::new("note").unwrap();
            card.overwrite_body(
                quillmark_content::markdown::from_markdown("Hello")
                    .unwrap()
                    .content,
            );
            doc.unchecked().insert_card(Some(1), card).unwrap();
        },
    ];
    for shuffle in shuffles {
        let mut doc = parse(three);
        let base = doc
            .card_by_id("bbb")
            .unwrap()
            .hash_of(Target::Body)
            .unwrap();
        shuffle(&mut doc);
        let at = doc.card_index("bbb").unwrap();
        let change = Change::new(vec![Step::Splice(Splice::new(5, "", " world"))]);
        doc.card_mut(at)
            .unwrap()
            .apply(Target::Body, &change, Some(base.as_str()))
            .unwrap();
        let written: Vec<String> = bodies(&doc)
            .into_iter()
            .filter(|(_, t)| t != "Hello")
            .map(|(id, _)| id)
            .collect();
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
    use quillmark_content::model::Island;
    use quillmark_content::model::IslandType;

    let mut doc = parse(SOURCE);
    let island = Island::new("isl-x", IslandType::Ext("x-chart".into()))
        .with_props(serde_json::json!({"series": [1]}));
    let insert = Change::new(vec![Step::Island(IslandStep::Insert {
        at: 7,
        island,
        block: None,
    })]);
    doc.main_mut().apply(Target::Body, &insert, None).unwrap();
    let held = doc.clone();
    let plain = quillmark_content::markdown::to_markdown(doc.main().body());
    let err = doc
        .main_mut()
        .revise(Target::Body, &plain, None)
        .unwrap_err();
    assert_eq!(err.code(), "edit::drops_ext_island");
    let without = quillmark_content::markdown::from_markdown(&plain)
        .unwrap()
        .content;
    let err = doc
        .main_mut()
        .replace(Target::Body, &without, None)
        .unwrap_err();
    assert_eq!(
        (err.code(), err.args()["island"].as_str()),
        ("edit::drops_ext_island", Some("isl-x"))
    );
    assert_eq!(doc, held);

    let removed = invert(&insert, &parse(SOURCE).main().read(Target::Body).unwrap()).unwrap();
    doc.main_mut().apply(Target::Body, &removed, None).unwrap();
    assert!(doc.main().body().islands.is_empty());
}

fn content(v: serde_json::Value) -> quillmark_content::model::Content {
    quillmark_content::model::Content::from_value(&v).unwrap_or_else(|e| panic!("{e}: {v}"))
}

fn paras(text: &str) -> serde_json::Value {
    let blocks: Vec<_> = (0..text.split('\n').count())
        .map(|k| serde_json::json!({"id": format!("b{k}"), "kind": "para"}))
        .collect();
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
        [
            json!({"id": id, "kind": "list", "attrs": {"ordered": ordered, "start": 1}}),
            json!({"id": item, "kind": "item"}),
        ]
    };
    let [l0, i0] = list("l0", "i0", true);
    let [l1, i1] = list("l1", "i1", true);
    let mut anchored = content(paras("a\nb"));
    anchored.anchors = vec![Anchor::new("p0", 0, 0), Anchor::new("p1", 3, 3)];
    vec![
        ("empty paragraph between", content(paras("Alpha.\n\nBeta."))),
        ("empty paragraph first", content(paras("\nBeta."))),
        ("empty paragraph last", content(paras("Alpha.\n"))),
        (
            "break-only paragraph",
            content(paras("Alpha.\n\u{2028}\nBeta.")),
        ),
        ("lone break", content(paras("\u{2028}"))),
        ("trailing break", content(paras("a\u{2028}"))),
        (
            "two lists around an empty paragraph",
            content(json!({
                "text": "a\n\nb",
                "blocks": [{"id": "b0", "kind": "para", "path": ["l0", "i0"]}, {"id": "b1", "kind": "para"}, {"id": "b2", "kind": "para", "path": ["l1", "i1"]}],
                "containers": [l0, i0, l1, i1],
            })),
        ),
        (
            "emphasis in link text",
            content(json!({
                "text": "see the official docs now", "blocks": [{"id": "b0", "kind": "para"}],
                "marks": [{"start": 4, "end": 21, "type": "link", "attrs": {"url": "https://x.io"}}, {"start": 8, "end": 16, "type": "emph"}],
            })),
        ),
        (
            "code in link text",
            content(json!({
                "text": "see the code docs", "blocks": [{"id": "b0", "kind": "para"}],
                "marks": [{"start": 4, "end": 17, "type": "link", "attrs": {"url": "https://x.io"}}, {"start": 8, "end": 12, "type": "code"}],
            })),
        ),
        (
            "cell: two breaks",
            content(json!({
                "text": "\u{FFFC}", "blocks": [{"id": "b0", "kind": "para"}],
                "islands": [{"id": "t0", "type": "table",
                    "props": {"columns": ["k0"], "rows": [{"id": "r0", "cells": {"k0": "s0"}}, {"id": "r1", "cells": {"k0": "s1"}}]},
                    "slots": {"s0": paras("H"), "s1": paras("a\u{2028}\u{2028}b")}}],
            })),
        ),
        (
            "strong inside code",
            content(json!({
                "text": "abcdef", "blocks": [{"id": "b0", "kind": "para"}],
                "marks": [{"start": 0, "end": 6, "type": "code"}, {"start": 2, "end": 4, "type": "strong"}],
            })),
        ),
        ("point anchors at the edges", anchored),
    ]
}

/// A revise with the value's own annotated read drops what markdown cannot
/// spell and lands what it reads, so the next revise with the read of that
/// writes nothing. Markdown spells formatting inside link text, so an edit of another
/// paragraph keeps it.
#[test]
fn a_revise_with_the_annotated_read_keeps_what_markdown_spells() {
    for (name, value) in unspelled() {
        let mut doc = parse(SOURCE);
        doc.main_mut().replace(Target::Body, &value, None).unwrap();
        let read = quillmark_content::markdown::to_markdown_annotated(doc.main().body()).markdown;
        doc.main_mut().revise(Target::Body, &read, None).unwrap();
        let reread = quillmark_content::markdown::to_markdown_annotated(doc.main().body()).markdown;
        let receipt = doc.main_mut().revise(Target::Body, &reread, None).unwrap();
        assert!(
            receipt.applied.is_empty(),
            "{name}: {read:?} then {reread:?}: {:?}",
            receipt.applied
        );
    }
    let mut doc = parse(SOURCE);
    for (name, value) in unspelled().into_iter().filter(|(n, _)| n.contains("link")) {
        let mut edited = value.clone();
        edited.text.push_str("\nTail.");
        edited.blocks.push(Block::new("tail", BlockKind::Para));
        doc.main_mut().replace(Target::Body, &edited, None).unwrap();
        let stored = doc.main().body().clone();
        let read = quillmark_content::markdown::to_markdown_annotated(&stored).markdown;
        doc.main_mut()
            .revise(Target::Body, &read.replace("Tail.", "Tail, edited."), None)
            .unwrap();
        assert_eq!(
            doc.main().body().text,
            stored.text.replace("Tail.", "Tail, edited."),
            "{name}"
        );
        assert_eq!(doc.main().body().marks, stored.marks, "{name}");
        assert_eq!(doc.main().body().blocks, stored.blocks, "{name}");
    }
}

/// A revise that rewrites the text under what its read could not spell, or
/// what the read re-spelled, lands as written.
#[test]
fn a_revise_over_what_its_read_could_not_spell_lands_as_written() {
    let shape = |name: &str| unspelled().into_iter().find(|(n, _)| *n == name).unwrap().1;
    let mut doc = parse(SOURCE);
    doc.main_mut()
        .replace(Target::Body, &shape("emphasis in link text"), None)
        .unwrap();
    doc.main_mut()
        .revise(
            Target::Body,
            "see [the unofficial docs](https://x.io) now",
            None,
        )
        .unwrap();
    let body = doc.main().body();
    assert_eq!(body.text, "see the unofficial docs now");
    assert!(
        body.marks
            .iter()
            .all(|m| m.kind != quillmark_content::model::MarkKind::Emph),
        "{:?}",
        body.marks
    );

    let mut doc = parse(SOURCE);
    doc.main_mut()
        .replace(Target::Body, &shape("cell: two breaks"), None)
        .unwrap();
    let read = quillmark_content::markdown::to_markdown_annotated(doc.main().body()).markdown;
    doc.main_mut()
        .revise(Target::Body, &read.replace("a<br><br>b", "z"), None)
        .unwrap();
    let cell = &doc.main().body().islands[0].slots["s1"];
    assert_eq!(cell.text, "z", "{read:?}");
}

fn anchored(body: &str, anchors: &[(&str, &str)]) -> crate::document::Document {
    let mut doc = parse(&format!("~~~\n$quill: q\n~~~\n\n{body}"));
    let text = doc.main().body().text.clone();
    let steps = anchors
        .iter()
        .map(|(id, quote)| {
            let start = text[..text.find(quote).unwrap()].chars().count();
            Step::Anchor(AnchorStep::Place {
                anchor: Anchor::new(*id, start, start + quote.chars().count()),
                quote: None,
            })
        })
        .collect();
    doc.main_mut()
        .apply(Target::Body, &Change::new(steps), None)
        .unwrap();
    doc
}

fn quote_of(doc: &crate::document::Document, id: &str) -> Option<String> {
    let body = doc.main().body();
    let chars: Vec<char> = body.text.chars().collect();
    body.anchors
        .iter()
        .find(|a| a.id == id)
        .map(|a| chars[a.start..a.end].iter().collect())
}

/// A revise and a replace carry an anchor through the one word diff: a word
/// rewritten under the anchor's end leaves the anchor on the words kept, and
/// a punctuation mark changed beside the anchored word leaves the word.
#[test]
fn a_revise_and_a_replace_carry_an_anchor_through_one_word_diff() {
    let doc = anchored(
        "The quick brown fox jumps over the lazy dog.\n",
        &[("c1", "brown fox"), ("c2", "dog")],
    );
    let md = "The quick brown wolf jumps over the lazy dog!";
    let mut revised = doc.clone();
    let receipt = revised.main_mut().revise(Target::Body, md, None).unwrap();
    let mut replaced = doc.clone();
    replaced
        .main_mut()
        .replace(
            Target::Body,
            &quillmark_content::markdown::from_markdown(md)
                .unwrap()
                .content,
            None,
        )
        .unwrap();
    assert_eq!(quote_of(&revised, "c1").as_deref(), Some("brown "));
    assert_eq!(quote_of(&revised, "c2").as_deref(), Some("dog"));
    assert_eq!(
        revised.main().body().anchors,
        replaced.main().body().anchors
    );
    let moved: Vec<_> = receipt
        .moved_anchors
        .iter()
        .map(|m| (m.id.as_str(), m.reason.name()))
        .collect();
    assert_eq!(moved, [("c1", "context"), ("c2", "context")]);
}

/// A replaced run's lines pair by best similarity, so a paragraph the writer
/// inserts that resembles the one it edited takes neither that paragraph's
/// id nor its anchor.
#[test]
fn an_inserted_look_alike_paragraph_takes_no_id_or_anchor_from_the_edited_one() {
    let mut doc = anchored(
        "Intro.\n\nThe meeting is on Monday at noon.\n\nOther text here.\n",
        &[("c1", "The meeting")],
    );
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
    let mut doc = anchored(
        "Alpha\\\nbeta gamma\\\ndelta.\n",
        &[("c1", "Alpha\u{2028}beta"), ("c2", "gamma\u{2028}delta")],
    );
    let read = doc.anchor_contexts();
    assert_eq!(read[0].quote, "Alpha\u{2028}beta");
    let receipt = doc
        .main_mut()
        .revise(Target::Body, "gamma\\\ndelt.", None)
        .unwrap();
    assert_eq!(
        receipt
            .dropped_anchors
            .iter()
            .map(|d| d.context.quote.as_str())
            .collect::<Vec<_>>(),
        ["Alpha\u{2028}beta"]
    );
    assert_eq!(receipt.dropped_anchors[0].context, read[0].context());
    assert_eq!(
        receipt
            .moved_anchors
            .iter()
            .map(|m| (m.id.as_str(), m.before.as_str()))
            .collect::<Vec<_>>(),
        [("c2", "gamma\u{2028}delta")]
    );
}

/// A write past an input bound refuses under the bound's code before any of
/// it runs, and writes nothing: markdown past 10 MiB, a change past
/// `MAX_CHANGE_STEPS`.
#[test]
fn a_write_past_an_input_bound_refuses_under_its_code() {
    let mut doc = parse(SOURCE);
    let held = doc.clone();
    let huge = "a".repeat(quillmark_content::MAX_MARKDOWN_SIZE + 1);
    let err = doc
        .main_mut()
        .revise(Target::Body, &huge, None)
        .unwrap_err();
    assert_eq!(err.code(), "edit::input_too_large");
    let steps = (0..=crate::document::MAX_CHANGE_STEPS)
        .map(|_| Step::Splice(Splice::new(0, String::new(), "x".to_string())));
    let err = doc
        .main_mut()
        .apply(Target::Body, &Change::new(steps.collect()), None)
        .unwrap_err();
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
        let receipt = doc
            .main_mut()
            .apply(
                target,
                &Change::new(vec![Step::Splice(Splice::new(
                    0,
                    String::new(),
                    "x".to_string(),
                ))]),
                None,
            )
            .unwrap();
        assert_eq!(
            receipt.hash,
            content_hash(&doc.main().read(target).unwrap())
        );
    }
    doc.unchecked()
        .store_field(
            None,
            "notes",
            crate::value::QuillValue::from_json(serde_json::json!("plain")),
        )
        .unwrap();
    doc.main_mut()
        .revise(crate::document::Target::Body, "Fresh body.", None)
        .unwrap();
    for target in [Target::Body, notes] {
        assert_eq!(
            doc.main().hash_of(target).unwrap(),
            content_hash(&doc.main().read(target).unwrap())
        );
    }
}

/// Structural edits made in turn, each from the hash the last receipt
/// answered, each answer their receipt and mint past every id the value
/// holds; one from a stale hash writes nothing. A placeholder in an apply is
/// filled past the value it lands on and named in `minted`.
#[test]
fn structural_edits_and_placeholders_mint_past_the_value() {
    use quillmark_content::structural::Wrap;
    let mut doc = parse("~~~\n$quill: q\n~~~\n\nalpha\n\nbravo\n\ncharlie\n");
    let base = doc.main().hash_of(Target::Body).unwrap();
    let quoted = doc
        .main_mut()
        .wrap(Target::Body, "b0", "b1", Wrap::Quote, Some(base.as_str()))
        .unwrap();
    assert_eq!(quoted.minted.get("?0").map(String::as_str), Some("q0"));
    let held = doc.clone();
    let err = doc
        .main_mut()
        .split_block(Target::Body, 14, None, Some(base.as_str()))
        .unwrap_err();
    assert_eq!(err.code(), "edit::stale_base");
    assert_eq!(doc, held);
    let split = doc
        .main_mut()
        .split_block(Target::Body, 14, None, Some(quoted.hash.as_str()))
        .unwrap();
    let right = split.minted["?0"].clone();
    assert!(right.starts_with('b') && right[1..].parse::<u64>().is_err());
    let listed = doc
        .main_mut()
        .wrap(
            Target::Body,
            "b2",
            "b2",
            Wrap::List {
                ordered: true,
                checked: None,
            },
            Some(split.hash.as_str()),
        )
        .unwrap();
    assert_eq!(
        listed
            .minted
            .values()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["l0", "i0"]
    );
    let body = doc.main().body();
    let ids: Vec<&str> = body.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b1", "b2", right.as_str()]);
    assert_eq!(body.blocks[2].path, ["l0", "i0"]);
    let located = doc.main().block_at(Target::Body, 7).unwrap();
    assert_eq!(
        (located.id.as_str(), located.index, located.path.as_slice()),
        ("b1", 1, &["q0".to_string()][..])
    );

    let insert = Change::new(vec![
        Step::Splice(Splice {
            blocks: vec![Block::new("?p".to_string(), BlockKind::Para)],
            ..Splice::new(20, "", "\nnew")
        }),
        Step::SetBlock(quillmark_content::change::SetBlock {
            id: "?p".into(),
            kind: Some("heading".into()),
            attrs: serde_json::json!({"level": 2}).as_object().cloned(),
            path: None,
        }),
    ]);
    let filled = doc
        .main_mut()
        .apply(Target::Body, &insert, Some(listed.hash.as_str()))
        .unwrap();
    let new = filled.minted["?p"].clone();
    assert!(
        doc.main()
            .body()
            .blocks
            .iter()
            .any(|b| b.id == new && matches!(b.kind, BlockKind::Heading { level: 2 }))
    );
}
