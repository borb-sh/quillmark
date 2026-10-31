//! Heading survival through `usaf_memo`'s paragraph rebuild, which buffers a
//! heading and prepends it to the next element as `strong[…]` (the AFH run-in
//! style). Every heading must reach the page exactly once, in its own place.
//!
//! The oracle is `regions()`: a region's `span` is the content range its glyphs
//! came from, so text that never reached the page has no span covering it.

#![cfg(feature = "typst")]

use quillmark::Document;

mod common;

/// `$body` regions as (content span, rect) pairs in content order.
fn body_rects(body: &str) -> Vec<([usize; 2], [f32; 4])> {
    let markdown = format!("~~~card-yaml\n$quill: usaf_memo\n$kind: main\n~~~\n\n{body}\n");
    let (engine, quill) = common::memo();
    let parsed = Document::parse(&markdown).expect("parses").document;
    let session = engine.open(quill, &parsed, common::test_date()).expect("opens");
    let mut regions: Vec<_> = session
        .regions()
        .iter()
        .filter(|r| r.field == "$body")
        .filter_map(|r| r.span.map(|s| (s, r.rect)))
        .collect();
    regions.sort_by_key(|(span, _)| *span);
    regions
}

/// `$body` regions as (content span, top edge) pairs in content order.
fn body_regions(body: &str) -> Vec<([usize; 2], f32)> {
    body_rects(body).into_iter().map(|(s, r)| (s, r[1])).collect()
}

fn spans(body: &str) -> Vec<[usize; 2]> {
    body_regions(body).into_iter().map(|(s, _)| s).collect()
}

/// Nothing follows the heading, so the buffer must be drained after the loop
/// rather than dying with it.
#[test]
fn a_trailing_heading_reaches_the_page() {
    assert_eq!(spans("one\n\n# Trailing"), [[0, 3], [4, 12]]);
    assert_eq!(spans("- one\n- # Absorbed"), [[0, 3], [4, 12]]);
    assert_eq!(spans("# Only"), [[0, 4]]);
}

/// The buffer holds one heading, so the second must flush it, not overwrite it.
#[test]
fn consecutive_headings_both_reach_the_page() {
    assert_eq!(spans("# First\n\n# Second\n\ntext"), [[0, 5], [6, 12], [13, 17]]);
}

/// Whether every `$body` region sits on one line — a run-in *is* the heading
/// and the text it joined sharing a line, and exact spans cannot see it (a
/// run-in surfaces as one merged region or two adjacent ones). The tolerance
/// separates a bold/regular baseline difference (hundredths of a point) from a
/// paragraph gap (tens).
fn on_one_line(body: &str) -> bool {
    let tops: Vec<f32> = body_regions(body).into_iter().map(|(_, y)| y).collect();
    let (lo, hi) = tops.iter().fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(*y), h.max(*y)));
    hi - lo < 2.0
}

/// The text inks whether or not it lands in the right item, so place is the
/// tell: prepended to the next item's content, a heading shares that item's line.
#[test]
fn a_heading_does_not_cross_into_the_next_item() {
    let regions = body_regions("- # Absorbed\n- second");
    assert_eq!(
        regions.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        [[0, 8], [9, 15]],
        "both texts ink"
    );
    assert!(
        !on_one_line("- # Absorbed\n- second"),
        "the heading holds its own line rather than joining the next item: {regions:?}"
    );
}

/// A heading joins the next block *of its own item*, top level or list item.
#[test]
fn the_run_in_style_still_runs_in() {
    assert!(on_one_line("- # Absorbed\n  item text"));
    assert!(on_one_line("# Absorbed\n\nitem text"));
}

/// Numbering formats run out at six levels; a list nested past them still
/// numbers and inks every item.
#[test]
fn a_list_nested_past_the_numbering_formats_renders() {
    let body = "one\n\n- two\n  - three\n    - four\n      - five\n        - six\n          - seven\n            - eight";
    assert_eq!(spans(body).len(), 8, "every level inks");
}

/// A level past the numbering formats restarts under each new parent: its
/// first item's number is as wide the second time, so its text starts at the
/// same left edge.
#[test]
fn a_level_past_the_numbering_formats_restarts_under_a_new_parent() {
    let body = "one\n\n- two\n  - three\n    - four\n      - five\n        - six\n          - seven\n        - six\n          - seven";
    let text: Vec<&str> = body.lines().map(|l| l.trim_start().trim_start_matches("- ")).filter(|l| !l.is_empty()).collect();
    let text = text.join("\n");
    let lefts: Vec<f32> = body_rects(body)
        .into_iter()
        .filter(|([start, end], _)| &text[*start..*end] == "seven")
        .map(|(_, rect)| rect[0])
        .collect();
    assert_eq!(lefts.len(), 2, "both deepest items ink");
    assert!((lefts[0] - lefts[1]).abs() < 0.01, "{lefts:?}");
}
