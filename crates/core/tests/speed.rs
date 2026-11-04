//! The interactive target's keystroke: one splice through `Card::write` on a
//! body of 100000 characters lands within [`BUDGET`] milliseconds, checked at
//! [`HEADROOM`] times that for a CI runner. A release-build timing, run by the
//! `speed` job with `--ignored`.

use std::time::{Duration, Instant};

use quillmark_automerge::content::change::{Change, Splice, Step};
use quillmark_core::document::{Document, Target};

const BUDGET: f64 = 5.0;

const HEADROOM: f64 = 2.0;

#[test]
#[ignore = "a release-build timing, run on its own"]
fn one_splice_writes_a_long_body_in_time() {
    let paragraphs: Vec<String> = (0..2000).map(|i| format!("Paragraph {i:04} says what the forty-odd chars say.")).collect();
    let body = paragraphs.join("\n\n");
    assert!(body.chars().count() >= 100_000, "{} chars", body.chars().count());
    let mut doc = Document::parse(&format!("~~~\n$quill: q\n~~~\n\n{body}\n")).unwrap().document;
    let at = body.chars().count() / 2;
    let mut best = Duration::MAX;
    for _ in 0..20 {
        let key = Change::new(vec![Step::Splice(Splice::new(at, "", "x"))]);
        let t = Instant::now();
        doc.main_mut().apply(Target::Body, &key, None).unwrap();
        best = best.min(t.elapsed());
    }
    let ms = best.as_secs_f64() * 1000.0;
    println!("  splice {ms:.2} ms at {} chars (target {BUDGET} ms)", body.chars().count());
    assert!(ms <= BUDGET * HEADROOM, "{ms:.2} ms > {BUDGET} ms x {HEADROOM}");
}
