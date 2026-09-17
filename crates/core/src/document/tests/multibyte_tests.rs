//! Regression coverage for the "string index N is not a character boundary"
//! panic class on the YAML scanner paths that run *after* the prescan.

use crate::document::Document;

fn decompose(markdown: &str) -> Result<Document, crate::error::ParseError> {
    Document::parse(markdown).map(|p| p.document)
}

#[test]
fn multibyte_in_quoted_scalar_parses() {
    let single = "~~~card-yaml\n$quill: q@0.1\n$kind: main\nbluf: '\u{2014}leading em-dash'\n~~~\n";
    let double =
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\nbluf: \"\u{201C}smart-quoted\u{201D}\"\n~~~\n";
    for input in [single, double] {
        decompose(input).unwrap_or_else(|e| panic!("expected parse to succeed, got: {e}"));
    }
}

#[test]
fn multibyte_yaml_errors_reach_the_renderer_char_bounded() {
    // `assemble` must hand the caret renderer a char-bounded slice. A parse error
    // is the expected outcome; a panic is not.
    // The renderer's own multibyte guarantee is pinned at its altitude
    let inputs = [
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\nf\u{2014}o: 1\nf\u{2014}o: 2\n~~~\n",
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\nx: hello \u{2014} world\nbluf: *bad-alias\n~~~\n",
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\nsystem_name: \u{201C}Service\u{201D}: Order API\n~~~\n",
    ];
    for input in inputs {
        let _ = decompose(input);
    }
}
