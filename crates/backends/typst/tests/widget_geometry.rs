//! Where a widget's region lands against where its box actually prints. Each
//! case renders the widget page and a twin page identical but for a filled box
//! of the widget's size in the widget's place, so the pixels that differ *are*
//! the widget's box, measured rather than restated from the same metadata the
//! region comes from.

use quillmark_core::{Backend, LiveSession, RenderedRegion};
use quillmark_typst::TypstBackend;

mod common;
use common::quill_with_plate as quill;

const YAML: &str = r#"
quill:
  name: widget_geometry
  version: 0.1.0
  backend: typst
  description: widget rect against rendered ink
typst:
  plate_file: plate.typ
main:
  fields:
    inline_field:
      type: string
      description: a widget seated in a line of text
    centered_field:
      type: string
      description: a widget under #align(center, ..)
"#;

/// Each widget page is followed by its twin: same text, same box, filled.
const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 200pt, height: 120pt, margin: 12pt)
#set text(size: 20pt)

Wide #form-field("inline", type: "text", field: "inline_field", width: 40pt, height: 12pt) text.

#pagebreak()
Wide #box(width: 40pt, height: 12pt, fill: black) text.

#pagebreak()
#align(center, form-field("centered", type: "text", field: "centered_field", width: 40pt, height: 12pt))

#pagebreak()
#align(center, box(width: 40pt, height: 12pt, fill: black))
"#;

const SCALE: f32 = 4.0;

fn open() -> LiveSession {
    TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}))
        .expect("open")
}

/// The bounding box, in Typst top-left-origin points, of the pixels that differ
/// between `page` and `twin`.
fn diff_bbox(session: &LiveSession, page: usize, twin: usize) -> [f32; 4] {
    let (w, h, a) = session
        .render_rgba(page, SCALE)
        .expect("SCALE rasterizes")
        .expect("render page");
    let (tw, th, b) = session
        .render_rgba(twin, SCALE)
        .expect("SCALE rasterizes")
        .expect("render twin");
    assert_eq!((w, h), (tw, th), "the twin pages render at the same size");

    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if a[i..i + 4] != b[i..i + 4] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    assert!(x0 < x1 && y0 < y1, "the twin pages differ somewhere");
    [
        x0 as f32 / SCALE,
        y0 as f32 / SCALE,
        x1 as f32 / SCALE,
        y1 as f32 / SCALE,
    ]
}

fn region(session: &LiveSession, field: &str) -> RenderedRegion {
    session
        .regions()
        .into_iter()
        .find(|r| r.field == field)
        .unwrap_or_else(|| panic!("{field} surfaces a region"))
}

/// A region is PDF bottom-left points; the rendered bbox is top-left. One
/// rendered pixel of slack covers the partly-covered pixel at each edge.
fn assert_region_covers_ink(session: &LiveSession, r: &RenderedRegion, ink: [f32; 4], what: &str) {
    let (_, page_h) = session.page_size_pt(r.page).expect("page size");
    let expected = [r.rect[0], page_h - r.rect[3], r.rect[2], page_h - r.rect[1]];
    let tol = 1.0 / SCALE;
    for (i, edge) in ["x0", "y0", "x1", "y1"].iter().enumerate() {
        assert!(
            (ink[i] - expected[i]).abs() <= tol,
            "{what}: reported {edge} {} is {:.2}pt off the printed box's {:.2}pt \
             (reported {expected:?}, printed {ink:?})",
            expected[i],
            ink[i] - expected[i],
            ink[i],
        );
    }
}

/// An inline box hangs a full box-height above the line's own baseline, so no
/// point the line supplies is the box's top-left.
#[test]
fn an_inline_widget_reports_the_rect_its_box_prints_in() {
    let session = open();
    let r = region(&session, "inline_field");
    assert_eq!(r.page, 0, "the inline widget is on the first page");
    assert_region_covers_ink(&session, &r, diff_bbox(&session, 0, 1), "inline widget");
}

/// A block-level tag is hoisted to the flow cursor, whose x is the left margin
/// whatever the alignment does to the box beside it.
#[test]
fn a_centered_widget_reports_the_rect_its_box_prints_in() {
    let session = open();
    let r = region(&session, "centered_field");
    assert_eq!(r.page, 2, "the centered widget is on the third page");
    assert_region_covers_ink(&session, &r, diff_bbox(&session, 2, 3), "centered widget");
}
