//! A card no declared kind claims renders: it reaches the plate in `$cards` and
//! the plate's `$kind` dispatch falls through on it. The signal is a
//! `validation::*` warning at the card's bare index.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions, Severity};
use quillmark_fixtures::quills_path;

mod common;

fn unclaimed_cards_render_and_warn(quill_name: &str) {
    let engine = Quillmark::new();
    let quill = quillmark::quill_from_path(quills_path(quill_name))
        .unwrap_or_else(|e| panic!("{quill_name} should load: {e:?}"));
    let seeded = quill.seed_document().to_markdown();
    let markdown = format!(
        "{seeded}\n~~~\n$kind: ghost\nnote: unknown\n~~~\n\nGhost body.\n"
    );
    let doc = Document::parse(&markdown)
        .unwrap_or_else(|e| panic!("document failed to parse: {e:?}\n---\n{markdown}"))
        .document;
    let ghost = doc.cards().len() - 1;

    engine
        .render(
            &quill,
            &doc,
            common::test_date(),
            &RenderOptions::default().with_output_format(OutputFormat::Svg),
        )
        .unwrap_or_else(|e| panic!("{quill_name}: unclaimed cards must render: {e:?}"));

    let diags = quill.validate(&doc);
    let at = |code: &str| {
        let d = diags
            .iter()
            .find(|d| d.code.as_deref() == Some(code))
            .unwrap_or_else(|| panic!("{quill_name}: no {code}; got {diags:?}"));
        assert_eq!(d.severity, Severity::Warning, "{d:?}");
        d.path.clone()
    };
    assert_eq!(at("validation::unknown_card"), Some(format!("cards[{ghost}]")));
}

#[test]
fn taro_renders_unclaimed_cards() {
    unclaimed_cards_render_and_warn("taro");
}

#[test]
fn usaf_memo_renders_unclaimed_cards() {
    unclaimed_cards_render_and_warn("usaf_memo");
}
