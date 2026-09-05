//! Headless proof that the pdfform canvas raster is complete: the canvas
//! contract (`prose/canon/PREVIEW.md`) requires a session returning `Some` from
//! `render_rgba` to bake every piece of page content, field values included,
//! into the pixels with no caller-side compositing.
//!
//! Region geometry is in PDF points (bottom-left origin) and the raster is
//! top-left device pixels, so locating a field box needs the canonical
//! `y_canvas = (pageHeightPt - y_pdf) × scale` flip.

use quillmark::{Document, Quillmark};

const FILLED: &str = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Ada Lovelace\n\
comments:\n\
  - First comment line.\n\
  - Second comment line.\n\
agree: true\n\
favorite_color: green\n\
~~~\n";

fn open() -> quillmark_core::LiveSession {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    engine.open(&quill, &doc).expect("open session")
}

#[test]
fn pdfform_canvas_raster_is_complete() {
    let session = open();

    assert!(
        session.page_size_pt(0).is_some(),
        "pdfform session must expose page geometry"
    );

    let scale: f32 = 2.0;
    let (width_pt, height_pt) = session.page_size_pt(0).expect("page 0 size");
    let (px_w, px_h, rgba) = session
        .render_rgba(0, scale)
        .expect("page 0 rasterizes at 2x")
        .expect("pdfform session must rasterize page 0");

    let expect_w = (width_pt * scale).round() as i64;
    let expect_h = (height_pt * scale).round() as i64;
    // hayro rounds independently per axis; allow ±1 px of rounding slack.
    assert!(
        (px_w as i64 - expect_w).abs() <= 1,
        "raster width {px_w} should match page_size_pt × scale ≈ {expect_w}"
    );
    assert!(
        (px_h as i64 - expect_h).abs() <= 1,
        "raster height {px_h} should match page_size_pt × scale ≈ {expect_h}"
    );
    assert_eq!(
        rgba.len(),
        (px_w as usize) * (px_h as usize) * 4,
        "RGBA buffer must be w*h*4 bytes"
    );

    let regions = session.regions();
    let region = regions
        .iter()
        .find(|r| r.page == 0 && r.field == "full_name")
        .expect("a region for the bound text field on page 0");

    let [x0, y0, x1, y1] = region.rect;
    let left = (x0 * scale).floor().max(0.0) as u32;
    let right = ((x1 * scale).ceil() as u32).min(px_w);
    let top = ((height_pt - y1) * scale).floor().max(0.0) as u32;
    let bottom = (((height_pt - y0) * scale).ceil() as u32).min(px_h);

    assert!(
        left < right && top < bottom,
        "field region rect must map to a non-empty pixel box: \
         x[{left},{right}) y[{top},{bottom}) in {px_w}x{px_h}"
    );

    let mut ink = 0u64; // non-white, opaque
    let mut opaque = 0u64;
    for y in top..bottom {
        for x in left..right {
            let i = ((y as usize) * (px_w as usize) + (x as usize)) * 4;
            let (r, g, b, a) = (rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]);
            if a == 255 {
                opaque += 1;
                if r < 250 || g < 250 || b < 250 {
                    ink += 1;
                }
            }
        }
    }

    assert!(
        opaque > 0,
        "field region box must contain opaque page-background pixels"
    );
    assert!(
        ink > 0,
        "field region box must contain non-white opaque pixels"
    );
}

#[test]
fn a_canvas_scale_that_cannot_be_rasterized_is_refused_rather_than_painted() {
    let session = open();
    for scale in [f32::INFINITY, f32::NAN, 0.0, -2.0, 1e6] {
        let err = session
            .render_rgba(0, scale)
            .err()
            .unwrap_or_else(|| panic!("{scale}x is not rasterizable"));
        assert_eq!(
            err.diagnostics()[0].code.as_deref(),
            Some("backend::invalid_raster_scale")
        );
    }

    assert!(
        session
            .render_rgba(99, 2.0)
            .expect("a page out of range is not a refused scale")
            .is_none(),
        "an out-of-range page still answers None"
    );
}
