//! Laid-out text no loaded font covers warns `typst::missing_glyph`, naming each
//! character once.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;

fn missing(a: &str) -> Vec<String> {
    let quill = common::quill_with_plate(
        &common::yaml("main:\n  fields:\n    a:\n      type: text\n"),
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n#set text(font: \"Roboto\")\n#qm.data.a\n",
    );
    let session = TypstBackend::default()
        .open(&quill, &serde_json::json!({ "a": a }), common::test_date())
        .expect("the plate compiles");
    session
        .warnings()
        .iter()
        .filter(|d| d.code.as_deref() == Some("typst::missing_glyph"))
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn a_character_no_font_covers_warns_once() {
    let warnings = missing("中文 中 ok");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(
        warnings[0],
        "no loaded font has a glyph for `中` (U+4E2D), `文` (U+6587); \
         the page draws an empty box in its place"
    );
}

#[test]
fn an_emoji_sequence_names_its_pictographs_alone() {
    let warnings = missing("👍🏽 ❤️");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(!warnings[0].contains("U+FE0F"), "{}", warnings[0]);
}

/// One unbroken word lays out as one text item, whose glyph ranges saturate
/// at `u16::MAX` bytes, inside a two-byte character here.
#[test]
fn a_word_past_64_kib_of_uncovered_text_warns() {
    let warnings = missing(&"ա".repeat(33_000));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("U+0561"), "{}", warnings[0]);
}

#[test]
fn covered_text_warns_nothing() {
    assert!(missing("plain text, ünïcödé").is_empty());
}

#[test]
fn text_the_page_never_draws_warns_nothing() {
    let quill = common::quill_with_plate(
        &common::yaml("main:\n  fields:\n    a:\n      type: text\n"),
        "#set text(font: \"Roboto\")\n#link(\"https://例え.jp/パス\")[idn]\n",
    );
    let session = TypstBackend::default()
        .open(
            &quill,
            &serde_json::json!({ "a": "中文" }),
            common::test_date(),
        )
        .expect("the plate compiles");
    assert!(
        session
            .warnings()
            .iter()
            .all(|d| d.code.as_deref() != Some("typst::missing_glyph"))
    );
}
