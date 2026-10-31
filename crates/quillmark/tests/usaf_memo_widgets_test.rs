//! Placement and styling of the USAF memo's injected AcroForm widgets, read
//! from the seeded memo: one card per declared kind, each field blank, so both
//! dates are unset and both `Signature` and `Ind_0_Signature` emit.
//!
//! A byte-level scan suffices: the overlay pass appends uncompressed widget
//! dicts.

#![cfg(feature = "typst")]

use std::sync::LazyLock;

use quillmark::{OutputFormat, RenderOptions};

mod common;

const PT_PER_IN: f32 = 72.0;

static PDF: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let (engine, quill, parsed) = common::seeded_memo();
    engine
        .render(
            quill,
            &parsed,
            common::test_date(),
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("usaf_memo should render to PDF")
        .artifacts[0]
        .bytes
        .clone()
});

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The numbers of the first `key [ ... ]` array in `bytes`.
fn array(bytes: &[u8], key: &[u8]) -> Vec<f32> {
    let key_name = String::from_utf8_lossy(key);
    let at = find(bytes, key).unwrap_or_else(|| panic!("no {key_name}"));
    let open = at + find(&bytes[at..], b"[").expect("[") + 1;
    let close = open + find(&bytes[open..], b"]").expect("]");
    std::str::from_utf8(&bytes[open..close])
        .unwrap()
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn rect(bytes: &[u8]) -> [f32; 4] {
    let n = array(bytes, b"/Rect");
    [n[0], n[1], n[2], n[3]]
}

/// The indirect object carrying `/T (name)`, bounded so a neighbour's keys are
/// not read as this widget's.
fn widget_object<'a>(pdf: &'a [u8], name: &str) -> &'a [u8] {
    let t = format!("/T ({name})");
    let at = find(pdf, t.as_bytes()).unwrap_or_else(|| panic!("no widget named {name:?}"));
    let start = pdf[..at]
        .windows(3)
        .rposition(|w| w == b"obj")
        .expect("widget object header");
    let end = at + find(&pdf[at..], b"endobj").expect("widget object trailer");
    &pdf[start..end]
}

fn dict_str(obj: &[u8], key: &str) -> Option<String> {
    let at = find(obj, key.as_bytes())?;
    let open = at + find(&obj[at..], b"(")? + 1;
    let close = open + find(&obj[open..], b")")?;
    Some(String::from_utf8_lossy(&obj[open..close]).into_owned())
}

/// Advance width of `"September 28, 2026"` in 12pt Times-Roman, the face the
/// widget's `/DA` names: the longest date either memo style produces (the DAF
/// ordering; USAF's `"28 September 2026"` is 93.32pt). A fixed font size clips
/// an overlong value rather than shrinking it, so the box is what fits it.
const LONGEST_DATE_PT: f32 = 96.32;

/// The seeded document leaves `font_size` at its schema default.
const DEFAULT_FONT_SIZE_PT: f32 = 12.0;

/// A blank date is dated by hand at signing, so the widget stands in for a date
/// the memo would otherwise typeset and is set like one: AFH 33-337's 12 point
/// Times body face, its right edge one inch from the page edge, where `/Q 2`
/// right-justifies the value and `display-date` would have ended.
///
/// The width is asserted as a multiple of the body size, not as points, because
/// `font_size` is a document field with no declared ceiling: a width that merely
/// cleared 96pt would still clip once a memo raised the size under it.
#[test]
fn date_widgets_are_set_like_the_date_they_replace() {
    let pdf = PDF.as_slice();
    let obj = widget_object(pdf, "Ind_0_Date");

    assert_eq!(
        dict_str(obj, "/DA").as_deref(),
        Some("/TiRo 12 Tf 0 g"),
        "the widget sets its value in the memo's 12pt Times body face, not the \
         auto-sized Helvetica default"
    );
    assert!(
        find(obj, b"/Q 2").is_some(),
        "the value is right-justified, so a typed date ends where a printed one \
         would; got {}",
        String::from_utf8_lossy(obj)
    );

    let [x0, _, x1, _] = rect(obj);
    let ems = (x1 - x0) / DEFAULT_FONT_SIZE_PT;
    let needed = LONGEST_DATE_PT / DEFAULT_FONT_SIZE_PT;
    assert!(
        ems >= needed,
        "widget is {ems:.2}em wide, under the {needed:.2}em \
         \"September 28, 2026\" sets in Times: a value that long would clip"
    );

    let page_width = array(pdf, b"/MediaBox")[2];
    for name in ["Date", "Ind_0_Date"] {
        let [_, _, x1, _] = rect(widget_object(pdf, name));
        let gap = page_width - x1;
        assert!(
            (gap - PT_PER_IN).abs() < 0.5,
            "{name}'s right edge should sit 1in from the page edge, sits {gap}pt"
        );
    }
}

/// AFH 33-337 puts the signature block 4.5 inches from the left page edge, and
/// the package overlays the widget there, offset up into the blank lines above
/// the typed name, so it consumes no flow.
#[test]
fn signature_widgets_align_with_the_signature_block() {
    const SIG_BLOCK_LEFT_IN: f32 = 4.5;
    let pdf = PDF.as_slice();
    assert!(
        find(pdf, b"/AcroForm").is_some(),
        "rendered memo should carry an AcroForm with the signature widget(s)"
    );

    let mut rects = Vec::new();
    let mut cursor = 0;
    while let Some(off) = find(&pdf[cursor..], b"/FT /Sig") {
        let sig_at = cursor + off;
        rects.push(rect(&pdf[sig_at..]));
        cursor = sig_at + b"/FT /Sig".len();
    }
    assert!(!rects.is_empty(), "PDF should contain at least one /FT /Sig widget");

    for [x0, _, x1, _] in rects {
        let left_in = x0 / PT_PER_IN;
        // The tolerance covers rounding and the long-name left-shift the
        // package applies only when a line overflows.
        assert!(
            (left_in - SIG_BLOCK_LEFT_IN).abs() < 0.1,
            "signature widget left edge should sit at the {SIG_BLOCK_LEFT_IN}in \
             signature block, but was at {left_in:.2}in (rect x0={x0}pt). A value \
             near 1.0in means the field regressed to the left margin."
        );
        assert!(x1 > x0, "widget rect should have positive width, got x0={x0} x1={x1}");
    }
}
