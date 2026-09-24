//! A GFM table in a `$body` renders through the full markdown -> Content ->
//! Typst -> artifact pipeline, exercising the emitter's `#table(...)` lowering
//! with column alignment, formatted cells, and a ragged row the model layer
//! normalizes before the backend sees it.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use quillmark_fixtures::quills_path;

fn render(markdown: &str, format: OutputFormat) -> quillmark::RenderResult {
    let engine = Quillmark::new();
    let quill =
        quillmark::quill_from_path(quills_path("table_demo")).expect("table_demo should load");
    let parsed = Document::parse(markdown)
        .unwrap_or_else(|e| panic!("document failed to parse: {e:?}\n---\n{markdown}"))
        .document;
    engine
        .render(
            &quill,
            &parsed,
            None,
            &RenderOptions::default().with_output_format(format),
        )
        .unwrap_or_else(|e| panic!("render failed: {e:?}\n---\n{markdown}"))
}

const FRONTMATTER: &str = "\
~~~card-yaml
$quill: table_demo@0.1.0
$kind: main
title: Table Demo
~~~
";

fn doc(body: &str) -> String {
    format!("{FRONTMATTER}\n{body}\n")
}

/// The successful render is the signal: a malformed `#table(...)` lowering
/// fails Typst compilation and the helper panics.
#[test]
fn table_body_renders_through_typst() {
    let table = render(
        &doc(
            "| Fruit | **Rank** | Note |\n\
             | :--- | :---: | ---: |\n\
             | Taro | 1 | best |\n\
             | Vanilla | 2 |",
        ),
        OutputFormat::Svg,
    );
    assert!(
        table.artifacts.first().is_some_and(|a| !a.bytes.is_empty()),
        "render produced no artifacts"
    );
}

