//! Styling and placement of the USAF memo's injected indorsement-date widget.
//!
//! An indorsement is dated when the endorser signs it, so the date is normally
//! blank at compile time and the widget is the only thing occupying its slot.
//! It stands in for a date the memo would otherwise typeset, so it has to be set
//! like one: AFH 33-337's 12 point Times body face, ending at the same right
//! margin `display-date` would have ended at.

#![cfg(feature = "typst")]

use quillmark::{OutputFormat, RenderOptions};

mod common;

const PT_PER_IN: f32 = 72.0;

/// Advance width of `"September 28, 2026"` in 12pt Times-Roman, the face the
/// widget's `/DA` names: the longest date either memo style produces (the DAF
/// ordering; USAF's `"28 September 2026"` is 93.32pt). A fixed font size clips
/// an overlong value rather than shrinking it, so the box is what fits it.
const LONGEST_DATE_PT: f32 = 96.32;

/// The seeded document leaves `font_size` at its schema default.
const DEFAULT_FONT_SIZE_PT: f32 = 12.0;

fn seeded_memo_pdf() -> Vec<u8> {
    // One card per declared kind, each blank: the indorsement date is unset,
    // which is the case the widget exists for.
    let (engine, quill, parsed) = common::seeded_memo();
    let result = engine
        .render(
            quill,
            &parsed,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("render should succeed");
    result.artifacts[0].bytes.clone()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The indirect object carrying `/T (name)`. The overlay pass appends
/// uncompressed widget dicts, so a byte-level scan suffices; bounding the slice
/// to one object keeps a neighbour's keys from being read as this widget's.
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

fn rect(obj: &[u8]) -> [f32; 4] {
    let at = find(obj, b"/Rect").expect("/Rect");
    let open = at + find(&obj[at..], b"[").expect("[") + 1;
    let close = open + find(&obj[open..], b"]").expect("]");
    let nums: Vec<f32> = std::str::from_utf8(&obj[open..close])
        .unwrap()
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    [nums[0], nums[1], nums[2], nums[3]]
}

/// Page width in points, read from any `/MediaBox`; every page is us-letter.
fn page_width(pdf: &[u8]) -> f32 {
    let at = find(pdf, b"/MediaBox").expect("/MediaBox");
    let open = at + find(&pdf[at..], b"[").expect("[") + 1;
    let close = open + find(&pdf[open..], b"]").expect("]");
    let nums: Vec<f32> = std::str::from_utf8(&pdf[open..close])
        .unwrap()
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    nums[2]
}

#[test]
fn indorsement_date_widget_is_set_like_the_date_it_replaces() {
    let pdf = seeded_memo_pdf();
    let obj = widget_object(&pdf, "Ind_0_Date");

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
}

/// AFH 33-337 places the date one inch from the right edge, and `/Q 2` measures
/// from the widget's right edge, so that edge is what has to land on the margin.
#[test]
fn indorsement_date_widget_ends_on_the_right_margin() {
    let pdf = seeded_memo_pdf();
    let [_, _, x1, _] = rect(widget_object(&pdf, "Ind_0_Date"));
    let gap = page_width(&pdf) - x1;
    assert!(
        (gap - PT_PER_IN).abs() < 0.5,
        "widget right edge should sit 1in from the page edge, sits {gap}pt"
    );
}

/// The regression guard for the fixed-size trade-off: auto-size shrinks an
/// overlong value to fit, a fixed size clips it. Only the box width keeps the
/// longest real date on the page.
///
/// Asserted as a multiple of the body size, not as points, because `font_size`
/// is a document field with no declared ceiling: a width that merely cleared
/// 96pt would still clip once a memo raised the size under it.
#[test]
fn indorsement_date_widget_fits_the_longest_date_at_any_body_size() {
    let pdf = seeded_memo_pdf();
    let [x0, _, x1, _] = rect(widget_object(&pdf, "Ind_0_Date"));
    let ems = (x1 - x0) / DEFAULT_FONT_SIZE_PT;
    let needed = LONGEST_DATE_PT / DEFAULT_FONT_SIZE_PT;
    assert!(
        ems >= needed,
        "widget is {ems:.2}em wide, under the {needed:.2}em \
         \"September 28, 2026\" sets in Times: a value that long would clip"
    );
}
