use quillmark_content::change::{Change, Splice, Step, apply, invert};
use quillmark_content::model::{Anchor, Block, BlockKind};

use crate::document::{EditError, Target, Write};

use super::parse;

const SOURCE: &str = "~~~\n$quill: q\n~~~\n\nRevenue grew 12%.\n\nShip v2\n";

fn split_at(at: usize, id: &str) -> Change {
    Change::new(vec![Step::Splice(Splice {
        blocks: vec![Block::new(id.to_string(), BlockKind::Para)],
        ..Splice::new(at, " ".to_string(), "\n".to_string())
    })])
}

/// A split names the left half and the right keeps its record; the receipt's
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
    assert_eq!(ids, ["b9", "b0", "b1"]);
    assert_eq!(receipt.hash, doc.main().hash_of(Target::Body).unwrap());

    let undo = invert(&receipt.applied, &before).unwrap();
    assert_eq!(apply(body, &undo).unwrap().content, before);
}

/// A base the value has moved on from refuses the write, which changes
/// nothing; a refused change does the same.
#[test]
fn a_stale_base_or_a_refused_change_writes_nothing() {
    let mut doc = parse(SOURCE);
    let base = doc.main().hash_of(Target::Body).unwrap();
    doc.main_mut().apply(Target::Body, &split_at(12, "b9"), None).unwrap();
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
    let dropped: Vec<&str> = receipt.dropped_anchors.iter().map(|a| a.id.as_str()).collect();
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

/// `apply_all` lands every write or none: a stale second write leaves the
/// first unwritten.
#[test]
fn apply_all_is_all_or_nothing() {
    let mut doc = parse(SOURCE);
    let held = doc.clone();
    let writes = [
        Write { card: None, target: Target::Body, change: split_at(12, "b9"), base: None },
        Write { card: None, target: Target::Field("subject"), change: Change::new(Vec::new()), base: Some("stale") },
    ];
    let (at, err) = doc.apply_all(&writes).unwrap_err();
    assert_eq!((at, err.code()), (1, "edit::stale_base"));
    assert_eq!(doc, held);
    let (at, err) = doc
        .apply_all(&[Write { card: Some(3), ..writes[0].clone() }])
        .unwrap_err();
    assert_eq!((at, err.code()), (0, "edit::index_out_of_range"));

    let receipts = doc.apply_all(&writes[..1]).unwrap();
    assert_eq!(receipts[0].hash, doc.main().hash_of(Target::Body).unwrap());
}
