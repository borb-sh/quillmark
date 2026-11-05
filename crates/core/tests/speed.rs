//! The interactive target's keystroke: one splice through `CardMut::write`
//! on a body, and on a rich text field, of 100000 characters lands within
//! [`BUDGET`] milliseconds, and a peer's one splice merged in takes no more
//! than that past what Automerge takes to apply it, checked at [`HEADROOM`]
//! times that for a CI runner. A release-build timing, run by the `speed`
//! job with `--ignored`.

use std::time::{Duration, Instant};

use quillmark_automerge::content::change::{Change, Splice, Step};
use quillmark_automerge::model::Doc;
use quillmark_core::document::{Document, Target};

const BUDGET: f64 = 5.0;

const HEADROOM: f64 = 2.0;

/// A document whose body, or field `target` names, holds 100000 characters,
/// and the offset of its middle.
fn long(target: Target<'_>) -> (Document, usize) {
    let paragraphs: Vec<String> = (0..2000).map(|i| format!("Paragraph {i:04} says what the forty-odd chars say.")).collect();
    let body = paragraphs.join("\n\n");
    assert!(body.chars().count() >= 100_000, "{} chars", body.chars().count());
    let mut doc = Document::parse(&format!("~~~\n$quill: q\n~~~\n\n{body}\n")).unwrap().document;
    if let Target::Field(_) = target {
        doc.main_mut().revise(target, &body, None).unwrap();
    }
    (doc, body.chars().count() / 2)
}

fn one_splice_in_time(target: Target<'_>) {
    let (mut doc, at) = long(target);
    let mut best = Duration::MAX;
    for _ in 0..20 {
        let key = Change::new(vec![Step::Splice(Splice::new(at, "", "x"))]);
        let t = Instant::now();
        doc.main_mut().apply(target, &key, None).unwrap();
        best = best.min(t.elapsed());
    }
    let ms = best.as_secs_f64() * 1000.0;
    println!("  {target:?} splice {ms:.2} ms at 100000 chars (target {BUDGET} ms)");
    assert!(ms <= BUDGET * HEADROOM, "{ms:.2} ms > {BUDGET} ms x {HEADROOM}");
}

fn one_peer_splice_in_time(target: Target<'_>) {
    let (mut doc, at) = long(target);
    let mut peer = doc.fork();
    let (mut best, mut floor) = (Duration::MAX, Duration::MAX);
    for _ in 0..10 {
        let heads = peer.heads();
        peer.main_mut().apply(target, &Change::new(vec![Step::Splice(Splice::new(at, "", "x"))]), None).unwrap();
        let bytes = peer.save_since(&heads);
        let mut automerge = Doc::load(&doc.save()).unwrap();
        let t = Instant::now();
        automerge.merge(&bytes).unwrap();
        floor = floor.min(t.elapsed());
        let t = Instant::now();
        doc.merge(&bytes).unwrap();
        best = best.min(t.elapsed());
    }
    assert_eq!(doc.main().read(target).unwrap(), peer.main().read(target).unwrap());
    let (ms, floor) = (best.as_secs_f64() * 1000.0, floor.as_secs_f64() * 1000.0);
    println!("  {target:?} peer splice merged {ms:.2} ms at 100000 chars, Automerge's apply {floor:.2} ms of it (target {BUDGET} ms past it)");
    assert!(ms - floor <= BUDGET * HEADROOM, "{ms:.2} ms - {floor:.2} ms > {BUDGET} ms x {HEADROOM}");
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn one_splice_writes_a_long_body_in_time() {
    one_splice_in_time(Target::Body);
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn one_splice_writes_a_long_rich_text_field_in_time() {
    one_splice_in_time(Target::Field("note"));
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn a_peer_splice_merges_into_a_long_body_in_time() {
    one_peer_splice_in_time(Target::Body);
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn a_peer_splice_merges_into_a_long_rich_text_field_in_time() {
    one_peer_splice_in_time(Target::Field("note"));
}
