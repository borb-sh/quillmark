//! The interactive target's keystroke: one splice through `Card::write` on a
//! body of 100000 characters lands within [`BUDGET`] milliseconds, and a card
//! block of sixty nested long keys over a long flow list parses within
//! [`PARSE_BUDGET`], each checked at [`HEADROOM`] times that for a CI runner.
//! Release-build timings, run by the `speed` job with `--ignored`.

use std::time::{Duration, Instant};

use quillmark_content::change::{Change, Splice, Step};
use quillmark_core::document::{Document, Target};

const BUDGET: f64 = 5.0;

const HEADROOM: f64 = 2.0;

#[test]
#[ignore = "a release-build timing, run on its own"]
fn one_splice_writes_a_long_body_in_time() {
    let paragraphs: Vec<String> = (0..2000)
        .map(|i| format!("Paragraph {i:04} says what the forty-odd chars say."))
        .collect();
    let body = paragraphs.join("\n\n");
    assert!(
        body.chars().count() >= 100_000,
        "{} chars",
        body.chars().count()
    );
    let mut doc = Document::parse(&format!("~~~\n$quill: q\n~~~\n\n{body}\n"))
        .unwrap()
        .document;
    let at = body.chars().count() / 2;
    let mut best = Duration::MAX;
    for _ in 0..20 {
        let key = Change::new(vec![Step::Splice(Splice::new(at, "", "x"))]);
        let t = Instant::now();
        doc.main_mut().apply(Target::Body, &key, None).unwrap();
        best = best.min(t.elapsed());
    }
    let ms = best.as_secs_f64() * 1000.0;
    println!(
        "  splice {ms:.2} ms at {} chars (target {BUDGET} ms)",
        body.chars().count()
    );
    assert!(
        ms <= BUDGET * HEADROOM,
        "{ms:.2} ms > {BUDGET} ms x {HEADROOM}"
    );
}

const PARSE_BUDGET: f64 = 400.0;

#[test]
#[ignore = "a release-build timing, run on its own"]
fn sixty_long_keys_deep_over_a_long_flow_list_parse_in_time() {
    let mut yaml = String::from("$quill: q\n");
    for depth in 0..60u8 {
        let key = char::from(b'a' + depth % 26).to_string().repeat(1000);
        yaml.push_str(&format!("{}{key}:\n", "  ".repeat(depth.into())));
    }
    let items = vec!["x"; 200_000].join(", ");
    yaml.push_str(&format!("{}v: [{items}]\n", "  ".repeat(60)));
    let md = format!("~~~\n{yaml}~~~\n");
    let t = Instant::now();
    let _ = Document::parse(&md).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "  parse {ms:.0} ms at {} bytes (target {PARSE_BUDGET} ms)",
        md.len()
    );
    assert!(
        ms <= PARSE_BUDGET * HEADROOM,
        "{ms:.0} ms > {PARSE_BUDGET} ms x {HEADROOM}"
    );
}
