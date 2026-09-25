//! Placement of the USAF memo's injected AcroForm signature widget. AFH 33-337
//! puts the signature block 4.5 inches from the left page edge, and the package
//! overlays the widget there (offset up into the blank lines above the typed
//! name) so it consumes no flow.

#![cfg(feature = "typst")]

use quillmark::{OutputFormat, RenderOptions};

mod common;

const PT_PER_IN: f32 = 72.0;
const SIG_BLOCK_LEFT_IN: f32 = 4.5;

/// Every `/FT /Sig` widget's `/Rect` as `[x0, y0, x1, y1]` in points. A
/// byte-level scan suffices: the overlay pass appends uncompressed widget dicts.
fn signature_widget_rects(pdf: &[u8]) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let mut cursor = 0;
    while let Some(off) = find(&pdf[cursor..], b"/FT /Sig") {
        let sig_at = cursor + off;
        if let Some(rect) = rect_after(pdf, sig_at) {
            rects.push(rect);
        }
        cursor = sig_at + b"/FT /Sig".len();
    }
    rects
}

fn rect_after(pdf: &[u8], from: usize) -> Option<[f32; 4]> {
    let rect_at = from + find(&pdf[from..], b"/Rect")?;
    let open = rect_at + find(&pdf[rect_at..], b"[")? + 1;
    let close = open + find(&pdf[open..], b"]")?;
    let body = std::str::from_utf8(&pdf[open..close]).ok()?;
    let nums: Vec<f32> = body
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    match nums.as_slice() {
        [x0, y0, x1, y1] => Some([*x0, *y0, *x1, *y1]),
        _ => None,
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn usaf_memo_signature_widget_aligns_with_signature_block() {
    // One card per declared kind, so both `Signature` and `Ind_0_Signature` emit.
    let (engine, quill, parsed) = common::seeded_memo();

    let result = engine.render(
        quill,
        &parsed,
        common::test_date(),
        &RenderOptions::default().with_output_format(OutputFormat::Pdf),
    );

    let rendered = result.expect("usaf_memo should render to PDF");
    let pdf = &rendered.artifacts[0].bytes;

    assert!(
        find(pdf, b"/AcroForm").is_some(),
        "rendered memo should carry an AcroForm with the signature widget(s)"
    );

    let rects = signature_widget_rects(pdf);
    assert!(
        !rects.is_empty(),
        "PDF should contain at least one /FT /Sig widget"
    );

    for [x0, _y0, x1, _y1] in &rects {
        let left_in = x0 / PT_PER_IN;
        // The tolerance covers rounding and the long-name left-shift the
        // package applies only when a line overflows.
        assert!(
            (left_in - SIG_BLOCK_LEFT_IN).abs() < 0.1,
            "signature widget left edge should sit at the {SIG_BLOCK_LEFT_IN}in \
             signature block, but was at {left_in:.2}in (rect x0={x0}pt). A value \
             near 1.0in means the field regressed to the left margin."
        );
        assert!(
            x1 > x0,
            "widget rect should have positive width, got x0={x0} x1={x1}"
        );
    }
}
