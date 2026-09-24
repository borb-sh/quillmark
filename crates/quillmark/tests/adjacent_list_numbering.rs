//! Two adjacent ordered lists keep their own numbering on the page.
//!
//! They reach the emitter as two runs carrying different
//! `Container::instance`, and the first item of each states its number so
//! Typst's running counter (`typst-layout::lists`, `item.number` overriding
//! the running one) restarts with it rather than counting 1 2 3 4.

#![cfg(feature = "typst")]
use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use quillmark_fixtures::quills_path;

fn svg(body: &str) -> String {
    let engine = Quillmark::new();
    let quill = quillmark::quill_from_path(quills_path("table_demo")).expect("load");
    let md = format!("~~~card-yaml\n$quill: table_demo@0.1.0\n$kind: main\ntitle: T\n~~~\n\n{body}\n");
    let parsed = Document::parse(&md).expect("parse").document;
    let r = engine.render(&quill, &parsed, None,
        &RenderOptions::default().with_output_format(OutputFormat::Svg)).expect("render");
    String::from_utf8_lossy(&r.artifacts[0].bytes).to_string()
}

/// Distinct glyph symbols the page references, as a proxy for which digits were
/// typeset: 1,2,1,2 uses two digit glyphs; 1,2,3,4 uses four.
fn glyphs(s: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for (i, _) in s.match_indices("<use ") {
        if let Some(h) = s[i..].find("href=\"#") {
            let rest = &s[i + h + 7..];
            if let Some(e) = rest.find('"') { out.insert(rest[..e].to_string()); }
        }
    }
    out
}

/// Digit glyphs stand in for the numbers on the page: a restart typesets only
/// `1` and `2`, while one run of four also pulls in `3` and `4`.
#[test]
fn adjacent_ordered_lists_number_apart() {
    // Two adjacent ordered lists, the CommonMark way.
    let two = svg("1. alpha\n2. bravo\n\n<!-- -->\n\n1. charlie\n2. delta");
    // One list of four.
    let four = svg("1. alpha\n2. bravo\n3. charlie\n4. delta");
    let (g2, g4) = (glyphs(&two), glyphs(&four));
    assert_eq!(
        g4.difference(&g2).count(),
        2,
        "one run of four should typeset two digits the restart never reaches"
    );
    assert!(
        g2.difference(&g4).next().is_none(),
        "the restart should typeset no digit the four-item run lacks"
    );
}
