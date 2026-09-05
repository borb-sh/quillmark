//! `update` swaps new document data into the helper package, recompiles, and
//! reports the dirty page set. Commit is transactional: a failed recompile
//! leaves every read serving the last-good compile.

use quillmark_core::{Backend, OutputFormat, Quill, RenderOptions};
use quillmark_typst::TypstBackend;
use serde_json::json;

mod common;

const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#data.at("msg")
"#;

fn quill() -> Quill {
    let yaml = "quill:\n  name: live\n  version: 0.1.0\n  backend: typst\n  description: update acceptance quill\n\ntypst:\n  plate_file: plate.typ\n\nmain:\n  fields:\n    msg:\n      description: message\n      type: string\n";
    common::quill_with_plate(yaml, PLATE)
}

/// `n` sentences; `marker` appended after sentence `edit_at`.
fn msg(n: usize, edit_at: Option<usize>, marker: &str) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str("Sentence ");
        s.push_str(&i.to_string());
        s.push_str(" lorem ipsum dolor sit amet consectetur adipiscing elit. ");
        if edit_at == Some(i) {
            s.push_str(marker);
            s.push(' ');
        }
    }
    s
}

#[test]
fn update_commits_and_dirties_only_the_touched_suffix() {
    let backend = TypstBackend;
    let q = quill();
    let n = 60;

    let mut session = backend
        .open(&q, &json!({ "msg": msg(n, None, "") }))
        .expect("open");
    let pages = session.page_count();
    assert!(pages >= 3, "fixture must span several pages, got {pages}");

    let cs = session
        .update_data(&json!({ "msg": msg(n, Some(n - 1), "EDITED") }))
        .expect("update");
    assert_eq!(cs.page_count, pages);
    assert_eq!(cs.dirty_pages, vec![pages - 1]);

    // A front edit dirties the first page and possibly shifted successors.
    let cs = session
        .update_data(&json!({ "msg": msg(n, Some(0), "EDITED") }))
        .expect("update");
    assert!(cs.dirty_pages.contains(&0), "dirty: {:?}", cs.dirty_pages);

    let cs = session
        .update_data(&json!({ "msg": msg(n, Some(0), "EDITED") }))
        .expect("update");
    assert!(cs.dirty_pages.is_empty(), "dirty: {:?}", cs.dirty_pages);
}

/// A content field routes its glyph spans into the helper `lib.typ`, which is
/// regenerated per `update`: the dirty-every-re-update shape.
fn markdown_quill() -> Quill {
    const YAML: &str = r#"quill:
  name: live_markdown
  version: 0.1.0
  backend: typst
  description: markdown-content no-op re-update quill
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a markdown body
"#;
    const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#data.body
"#;
    common::quill_with_plate(YAML, PLATE)
}

#[test]
fn identical_re_update_of_markdown_content_is_clean() {
    // A page's fingerprint must not fold in `Span`s, so a byte-identical re-update
    // reports nothing dirty, every round, not just once.
    let backend = TypstBackend;
    let q = markdown_quill();
    let body = "This is a **markdown** paragraph that renders some real ink. ".repeat(3);

    let mut session = backend.open(&q, &json!({ "body": body })).expect("open");
    let pages = session.page_count();
    assert!(pages >= 1);

    for round in 0..3 {
        let cs = session
            .update_data(&json!({ "body": body }))
            .expect("update identical");
        assert_eq!(cs.page_count, pages);
        assert!(
            cs.dirty_pages.is_empty(),
            "round {round}: identical markdown re-update must be clean, got {:?}",
            cs.dirty_pages
        );
    }

    // A real change still dirties: the fingerprint didn't go blind.
    let cs = session
        .update_data(&json!({ "body": format!("{body} plus a genuinely new sentence.") }))
        .expect("update changed");
    assert!(
        !cs.dirty_pages.is_empty(),
        "a real edit must still dirty a page"
    );
}

fn two_field_quill() -> Quill {
    const YAML: &str = r#"quill:
  name: live_two_field
  version: 0.1.0
  backend: typst
  description: two markdown fields
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a markdown body
    note:
      type: richtext
      description: a markdown note
"#;
    const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#data.body

#data.note
"#;
    common::quill_with_plate(YAML, PLATE)
}

#[test]
fn update_with_reordered_fields_same_content_is_clean() {
    // `serde_json` is built with `preserve_order`, so field insertion order
    // survives on the wire and an editor can hand `update` the same content in a
    // different key order. Canonical codegen and span-free page hashes must
    // both hold for that to stay clean.
    let backend = TypstBackend;
    let q = two_field_quill();

    let opened: serde_json::Value = serde_json::from_str(
        r#"{"body":"**Body** paragraph with real ink.","note":"A note with ink too."}"#,
    )
    .unwrap();
    let reordered: serde_json::Value = serde_json::from_str(
        r#"{"note":"A note with ink too.","body":"**Body** paragraph with real ink."}"#,
    )
    .unwrap();

    let mut session = backend.open(&q, &opened).expect("open");
    let cs = session.update_data(&reordered).expect("update reordered");
    assert!(
        cs.dirty_pages.is_empty(),
        "same content in a different field order moved no ink; got dirty {:?}",
        cs.dirty_pages
    );

    let mut edited = reordered.clone();
    edited["body"] = json!("**Body** paragraph with real ink, now extended further.");
    let cs = session.update_data(&edited).expect("update edited");
    assert!(!cs.dirty_pages.is_empty(), "a real edit must still dirty");
}

#[test]
fn update_is_transactional_on_compile_failure() {
    let backend = TypstBackend;
    let q = quill();

    let mut session = backend
        .open(&q, &json!({ "msg": "last good" }))
        .expect("open");
    let pages = session.page_count();

    // No `msg` key → the plate's `data.at("msg")` fails at eval.
    let err = session.update_data(&json!({})).expect_err("compile must fail");
    assert!(!err.diagnostics().is_empty());

    assert_eq!(session.page_count(), pages);
    session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("render serves last-good");

    let cs = session
        .update_data(&json!({ "msg": "recovered" }))
        .expect("update after failure");
    assert_eq!(cs.page_count, session.page_count());
}

#[test]
fn update_tracks_page_count_growth_and_shrink() {
    let backend = TypstBackend;
    let q = quill();

    let mut session = backend
        .open(&q, &json!({ "msg": msg(4, None, "") }))
        .expect("open");
    let small = session.page_count();

    let cs = session
        .update_data(&json!({ "msg": msg(120, None, "") }))
        .expect("grow");
    assert!(cs.page_count > small);
    assert_eq!(cs.page_count, session.page_count());
    assert!(cs.dirty_pages.contains(&(cs.page_count - 1)));

    let cs = session
        .update_data(&json!({ "msg": msg(4, None, "") }))
        .expect("shrink");
    assert_eq!(cs.page_count, small);
    assert_eq!(cs.page_count, session.page_count());
    assert!(cs.dirty_pages.iter().all(|&p| p < cs.page_count));
}
