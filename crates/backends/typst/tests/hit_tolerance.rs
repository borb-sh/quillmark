//! The pointer-slack argument on `field_at` / `position_at`: a click within
//! `tol` points of a field's ink resolves to the nearest such ink: the leading
//! between two lines is inside a paragraph and on no glyph. These hold what the
//! slack may and may not change.

use quillmark_core::{backend::Backend, session::LiveSession};
use quillmark_typst::TypstBackend;

mod common;
use common::{content, quill_with_plate as quill};

const YAML: &str = r#"
quill:
  name: hit_tolerance
  version: 0.1.0
  backend: typst
  description: tolerant hit-testing
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: richtext
      description: the paragraph above
    body:
      type: richtext
      description: the paragraph below
"#;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 300pt, margin: 30pt)
#set text(size: 11pt)

#data.intro

#data.body
"#;

fn open() -> LiveSession {
    let data = serde_json::json!({
        "intro": content(&"Intro text that wraps across more than one line of the measure. ".repeat(3)),
        "body": content(&"Body text that also wraps across more than one line of the measure. ".repeat(3)),
    });
    TypstBackend.open(&quill(YAML, PLATE), &data).expect("open")
}

/// The `[low, high]` y bands on page 0 that answer at column `x` with no
/// tolerance: one per line of type, top of the page last.
fn live_bands(session: &LiveSession, x: f32) -> Vec<[f32; 2]> {
    let (_, h) = session.page_size_pt(0).expect("page size");
    let mut bands: Vec<[f32; 2]> = Vec::new();
    let mut y = 0.0;
    while y <= h {
        if session.position_at(0, x, y, 0.0).is_some() {
            match bands.last_mut() {
                Some(band) if band[1] >= y - 0.5 => band[1] = y,
                _ => bands.push([y, y]),
            }
        }
        y += 0.25;
    }
    bands
}

#[test]
fn a_click_in_the_leading_between_two_lines_takes_the_nearer_line() {
    let session = open();
    let x = 120.0;
    let bands = live_bands(&session, x);
    assert!(bands.len() >= 3, "the two fields wrap to several lines: {bands:?}");

    // Adjacent lines of one paragraph, `lower` printed after `upper` on the page.
    let (lower, upper) = (bands[bands.len() - 2], bands[bands.len() - 1]);
    let gap = upper[0] - lower[1];
    assert!(gap > 1.0, "the leading is a real gap: {gap}pt between {lower:?} and {upper:?}");

    let tol = gap;
    let near_upper = session
        .position_at(0, x, lower[1] + gap * 0.75, tol)
        .expect("a point in the leading resolves under tolerance");
    let near_lower = session
        .position_at(0, x, lower[1] + gap * 0.25, tol)
        .expect("a point in the leading resolves under tolerance");
    assert_ne!(
        near_upper.pos, near_lower.pos,
        "the two halves of the leading take different lines"
    );

    for (hit, band, side) in [
        (&near_upper, upper, "upper"),
        (&near_lower, lower, "lower"),
    ] {
        let caret = session
            .locate(&hit.field, hit.pos)
            .expect("the resolved position locates a caret");
        assert!(
            caret.rect[3] >= band[0] && caret.rect[1] <= band[1],
            "the {side} half of the leading lands on the {side} line: caret {:?} against band {band:?}",
            caret.rect
        );
    }
}

/// Distance ranking, not a grown box: outset rects overlap, and a first match
/// over them in paint order answers the later-painted item however far away it
/// is. Here that is the paragraph below, and the click is nearer the one above.
#[test]
fn the_nearer_field_wins_over_the_later_painted_one() {
    let session = open();
    let x = 120.0;
    let bands = live_bands(&session, x);

    let last_of = |field: &str| {
        let mut lowest: Option<[f32; 2]> = None;
        for band in &bands {
            let mid = (band[0] + band[1]) / 2.0;
            if session.position_at(0, x, mid, 0.0).map(|h| h.field).as_deref() == Some(field)
                && lowest.is_none_or(|low| band[0] < low[0])
            {
                lowest = Some(*band);
            }
        }
        lowest.expect("the field prints at this column")
    };
    let intro_last = last_of("intro");
    let body_first = bands
        .iter()
        .copied()
        .filter(|b| b[1] < intro_last[0])
        .max_by(|a, b| a[0].total_cmp(&b[0]))
        .expect("the body prints below the intro");

    let gap = intro_last[0] - body_first[1];
    let y = body_first[1] + gap * 0.9;
    let hit = session
        .position_at(0, x, y, gap)
        .expect("the paragraph break resolves under a tolerance spanning it");
    assert_eq!(
        hit.field, "intro",
        "a point {:.2}pt below the intro and {:.2}pt above the body takes the intro",
        intro_last[0] - y,
        y - body_first[1]
    );
}

/// The property that makes the slack safe to raise: it only ever fills a miss.
#[test]
fn a_tolerance_never_changes_an_answer_an_exact_hit_already_had() {
    let session = open();
    let (w, h) = session.page_size_pt(0).expect("page size");
    let mut exact = 0;
    let mut filled = 0;
    let (mut x, step) = (0.0, 7.0);
    while x <= w {
        let mut y = 0.0;
        while y <= h {
            match session.position_at(0, x, y, 0.0) {
                Some(hit) => {
                    exact += 1;
                    assert_eq!(
                        session.position_at(0, x, y, 8.0),
                        Some(hit),
                        "a tolerance changed the answer at ({x}, {y})"
                    );
                }
                None => filled += usize::from(session.position_at(0, x, y, 8.0).is_some()),
            }
            if let Some(field) = session.field_at(0, x, y, 0.0) {
                assert_eq!(
                    session.field_at(0, x, y, 8.0),
                    Some(field),
                    "a tolerance changed the field at ({x}, {y})"
                );
            }
            y += step;
        }
        x += step;
    }
    assert!(exact > 0, "the sweep crosses ink");
    assert!(filled > 0, "the sweep crosses ink it only reaches under tolerance");
}

/// A short line with the rest of its measure empty beside it: `intro` is one
/// line, `body` fills the measure below.
fn open_with_a_short_line() -> LiveSession {
    let data = serde_json::json!({
        "intro": content("Short."),
        "body": content(&"Body text that fills the measure and wraps past one line. ".repeat(3)),
    });
    TypstBackend
        .open(&quill(YAML, PLATE), &data)
        .expect("open")
}

/// The slack is a radius in both axes, so it does not reach along a row: the
/// empty measure beside a short line is as far from that line as a point the
/// same distance above it. A tolerance that crossed a measure would make a
/// click's column stop mattering, and a click in one page margin answer with a
/// field set against the other.
#[test]
fn the_empty_measure_beside_a_line_is_not_that_line() {
    let session = open_with_a_short_line();
    let intro = session
        .regions()
        .into_iter()
        .find(|r| r.field == "intro")
        .expect("intro surfaces a region");
    let y = (intro.rect[1] + intro.rect[3]) / 2.0;
    let x = intro.rect[2] + 120.0;
    for tol in [0.0f32, 3.1, 8.0, 20.0, 120.0] {
        assert_ne!(
            session.field_at(0, x, y, tol).as_deref(),
            Some("intro"),
            "tol={tol} reached 120pt along the row into the intro's own whitespace"
        );
    }
    assert_eq!(
        session.field_at(0, x, y, 3.1),
        None,
        "and at a pointer's slack that whitespace is on no field at all"
    );
}

#[test]
fn a_point_past_the_tolerance_is_still_a_miss() {
    let session = open();
    // The page corner: margin alone puts it 30pt from any ink.
    assert_eq!(session.position_at(0, 2.0, 2.0, 8.0), None);
    assert_eq!(session.field_at(0, 2.0, 2.0, 8.0), None);
}

/// A point that is not finite is outside every box, so both queries answer
/// nothing. The documented click transform produces one whenever `renderScale`
/// is zero.
#[test]
fn a_non_finite_point_resolves_to_nothing() {
    let session = open();
    for (x, y) in [
        (f32::NAN, f32::NAN),
        (f32::NAN, 100.0),
        (120.0, f32::NAN),
        (f32::INFINITY, f32::INFINITY),
        (f32::NEG_INFINITY, 100.0),
    ] {
        assert_eq!(session.position_at(0, x, y, 8.0), None, "positionAt({x}, {y})");
        assert_eq!(session.field_at(0, x, y, 8.0), None, "fieldAt({x}, {y})");
    }
}

const WIDGET_YAML: &str = r#"
quill:
  name: hit_tolerance_widget
  version: 0.1.0
  backend: typst
  description: a widget above a paragraph
typst:
  plate_file: plate.typ
main:
  fields:
    blank:
      type: string
      description: the fill-in widget
    body:
      type: richtext
      description: the paragraph below it
"#;

const WIDGET_PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, form-field
#set page(width: 400pt, height: 300pt, margin: 30pt)
#set text(size: 11pt)

#form-field("blank", type: "text", field: "blank", width: 60pt, height: 20pt)

#v(40pt)

#data.body
"#;

fn open_with_widget() -> LiveSession {
    let data = serde_json::json!({
        "blank": "",
        "body": content("Body text well below the widget."),
    });
    TypstBackend
        .open(&quill(WIDGET_YAML, WIDGET_PLATE), &data)
        .expect("open")
}

/// The nearer of the widget and content lanes answers at every tolerance, so
/// raising `tol` never moves a click to a farther field.
#[test]
fn raising_the_tolerance_never_moves_a_click_to_a_farther_field() {
    let session = open_with_widget();
    let widget = session
        .regions()
        .into_iter()
        .find(|r| r.field == "blank")
        .expect("the widget surfaces a region");
    let x = (widget.rect[0] + widget.rect[2]) / 2.0;

    // Just below the widget's lower edge: nearest is the widget, by a wide
    // margin over any body ink further down the page.
    let y = widget.rect[1] - 2.0;
    let near = session
        .field_at(widget.page, x, y, 4.0)
        .expect("a point 2pt off the widget resolves under a 4pt tolerance");
    assert_eq!(near, "blank", "the nearest field is the widget");

    for tol in [8.0f32, 20.0, 60.0, 200.0] {
        assert_eq!(
            session.field_at(widget.page, x, y, tol).as_deref(),
            Some("blank"),
            "tol={tol} moved the click off the widget it is 2pt from"
        );
    }
}

/// No tolerance admits a non-finite point, an infinite one included: the gap is
/// absent rather than large. `renderScale == 0` yields both at once, an infinite
/// `tolPt` and a non-finite point, from the same division.
#[test]
fn an_infinite_tolerance_does_not_admit_a_non_finite_point() {
    let session = open();
    for (x, y) in [
        (f32::NAN, f32::NAN),
        (120.0, f32::NAN),
        (f32::NEG_INFINITY, 100.0),
    ] {
        assert_eq!(session.field_at(0, x, y, f32::INFINITY), None, "fieldAt({x}, {y})");
        assert_eq!(session.position_at(0, x, y, f32::INFINITY), None, "positionAt({x}, {y})");
    }
}

const OVERLAID_PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, form-field
#set page(width: 400pt, height: 300pt, margin: 30pt)
#set text(size: 11pt)

#data.body

#place(top + left, dx: 40pt, dy: 40pt,
  form-field("blank", type: "text", field: "blank", width: 60pt, height: 20pt))
"#;

fn open_with_overlaid_widget() -> LiveSession {
    let data = serde_json::json!({
        "blank": "",
        "body": content(&"Body text under and around the placed widget. ".repeat(6)),
    });
    TypstBackend
        .open(&quill(WIDGET_YAML, OVERLAID_PLATE), &data)
        .expect("open")
}

/// A widget takes a tie, so ink beneath one never swallows a click that lands
/// on it. The widget is laid over the paragraph, so the point is inside both.
#[test]
fn a_widget_takes_a_tie_with_the_content_ink_under_it() {
    let session = open_with_overlaid_widget();
    let widget = session
        .regions()
        .into_iter()
        .find(|r| r.field == "blank")
        .expect("the widget surfaces a region");
    let (cx, cy) = (
        (widget.rect[0] + widget.rect[2]) / 2.0,
        (widget.rect[1] + widget.rect[3]) / 2.0,
    );
    assert!(
        session.position_at(widget.page, cx, cy, 0.0).is_some(),
        "the fixture puts content ink under the widget, so the gaps tie"
    );
    for tol in [0.0f32, 8.0, 60.0] {
        assert_eq!(
            session.field_at(widget.page, cx, cy, tol).as_deref(),
            Some("blank"),
            "tol={tol}"
        );
    }
}
