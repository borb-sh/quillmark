//! A card no declared kind claims renders: it reaches the plate in `$cards` and
//! the plate's `$kind` dispatch falls through on it. The signal is a
//! `validation::*` warning at the card's bare index, on the render's own
//! `warnings` ahead of the compile's.

#![cfg(feature = "typst")]

use quillmark::{Diagnostic, Document, Engine, OutputFormat, RenderOptions, Severity};
use quillmark_fixtures::quills_path;

mod common;

#[test]
fn unclaimed_cards_render_and_warn() {
    let engine = Engine::new();
    let quill = quillmark::quill_from_path(quills_path("taro")).expect("taro loads");
    let seeded = quill.seed_document().to_markdown();
    let markdown = format!("{seeded}\n~~~\n$kind: ghost\nnote: unknown\n~~~\n\nGhost body.\n");
    let doc = Document::parse(&markdown)
        .unwrap_or_else(|e| panic!("document failed to parse: {e:?}\n---\n{markdown}"))
        .document;
    let ghost = doc.cards().len() - 1;

    let diags = engine
        .render(
            &quill,
            &doc,
            common::test_date(),
            &RenderOptions::default().with_output_format(OutputFormat::Svg),
        )
        .unwrap_or_else(|e| panic!("unclaimed cards must render: {e:?}"))
        .warnings;

    let at = |code: &str| {
        let d = diags
            .iter()
            .find(|d| d.code.as_deref() == Some(code))
            .unwrap_or_else(|| panic!("no {code}; got {diags:?}"));
        assert_eq!(d.severity, Severity::Warning, "{d:?}");
        d.path.clone()
    };
    assert_eq!(
        at("validation::unknown_card"),
        Some(format!("cards[{ghost}]"))
    );

    let validation = |d: &Diagnostic| {
        d.code
            .as_deref()
            .is_some_and(|c| c.starts_with("validation::"))
    };
    assert!(
        diags
            .iter()
            .skip_while(|d| validation(d))
            .all(|d| !validation(d)),
        "validate's warnings lead the compile's: {diags:?}"
    );
}
