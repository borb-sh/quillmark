//! A document's cards held in its Automerge document read back as
//! themselves: payloads exactly, bodies up to the ids the document gives
//! their parts.

use super::parse;
use crate::document::held;
use crate::document::{Card, Document};

fn same(a: &Card, b: &Card) {
    assert_eq!(a.payload(), b.payload());
    assert_eq!(a.body_markdown(), b.body_markdown());
    assert_eq!(a.body().text, b.body().text);
    assert_eq!(a.body().anchors, b.body().anchors);
    assert_eq!(a.body().marks, b.body().marks);
}

fn round_trip(doc: &Document) {
    let mut held = held::build(doc.main(), doc.cards());
    let (main, cards) = held::read(&held).unwrap();
    same(&main, doc.main());
    assert_eq!(cards.len(), doc.cards().len());
    for (a, b) in cards.iter().zip(doc.cards()) {
        same(a, b);
    }
    let bytes = held.save();
    let back = quillmark_automerge::model::Doc::load(&bytes).unwrap();
    let (main2, cards2) = held::read(&back).unwrap();
    assert_eq!(main2, main);
    assert_eq!(cards2, cards);
}

const DOCS: [&str; 3] = [
    "~~~\n$quill: q@0.1\ntitle: Hi\n~~~\n\n# Head\n\n- one\n- two\n  - nested\n\n> quote *em*\n\n| a | b |\n| - | - |\n| c | d |\n\nText[^n].\n\n[^n]: a note\n",
    "~~~\n$quill: q@0.1\n~~~\n",
    "~~~\n$quill: q@0.1\nrecipients:\n  - name: A\n    org: B\n~~~\n\n```rust\nfn x() {}\n```\n\n~~~\n$kind: a\n~~~\n\n~~~\n$kind: b\nn: 3\n~~~\n\nlast\n",
];

#[test]
fn documents_read_back_as_themselves() {
    for src in DOCS {
        round_trip(&parse(src));
    }
}

#[test]
fn comments_ext_and_nested_values_read_back() {
    let src = "~~~\n$quill: q@0.1\n# a comment\ntitle: Hi # inline\nlist:\n  # inside\n  - 1\n  - b\nobj: { a: 1, b: [true, null] }\n$ext: { x: 1 }\n~~~\n\nBody **bold** text.\n\n~~~\n$kind: note\n$id: n1\nwho: me\n~~~\n\nA note.\n";
    round_trip(&parse(src));
}

#[test]
fn equal_documents_build_equal_bytes() {
    let doc = parse(DOCS[0]);
    let (mut a, mut b) = (held::build(doc.main(), doc.cards()), held::build(doc.main(), doc.cards()));
    assert_eq!(a.heads(), b.heads());
    assert_eq!(a.save(), b.save());
}

#[test]
fn writing_over_the_cards_writes_what_changed() {
    let doc = parse("~~~\n$quill: q@0.1\ntitle: Hi\n~~~\n\nOne.\n\n~~~\n$kind: note\n$id: n1\n~~~\n\nTwo.\n");
    let mut held = held::build(doc.main(), doc.cards());
    let (main, cards) = held::read(&held).unwrap();
    let edited = parse("~~~\n$quill: q@0.1\ntitle: Bye\nmore: 2\n~~~\n\nOne and more.\n\n~~~\n$kind: note\n$id: n2\n~~~\n\nThree.\n\n~~~\n$kind: note\n$id: n1\n~~~\n\nTwo.\n");
    held::write_over(&mut held, (&main, &cards), (edited.main(), edited.cards()));
    let (m2, c2) = held::read(&held).unwrap();
    same(&m2, edited.main());
    assert_eq!(c2.len(), 2);
    for (a, b) in c2.iter().zip(edited.cards()) {
        same(a, b);
    }
}
