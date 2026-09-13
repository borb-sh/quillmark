//! Headless proof that the acroform canvas raster is complete and that region
//! geometry lands on it: the canvas contract (`prose/canon/PREVIEW.md`) requires
//! a session returning `Some` from `render_rgba` to bake every piece of page
//! content, field values included, into the pixels with no caller-side
//! compositing, and `page_size_pt` / `regions` to measure from the raster's own
//! origin — the lower-left corner of the page's canvas box.
//!
//! Region geometry is in PDF points (bottom-left origin) and the raster is
//! top-left device pixels, so locating a field box needs the canonical
//! `y_canvas = (pageHeightPt - y_pdf) × scale` flip.

use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
use quillmark::{Document, FileTreeNode, Quill, Quillmark};

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
fn acroform_canvas_raster_is_complete() {
    let session = open();

    let scale: f32 = 2.0;
    let (width_pt, height_pt) = session.page_size_pt(0).expect("page 0 size");
    let (px_w, px_h, rgba) = session
        .render_rgba(0, scale)
        .expect("page 0 rasterizes at 2x")
        .expect("acroform session must rasterize page 0");

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

    let (l, t, r, b) = region_box_px(region.rect, height_pt, scale);
    let left = l.clamp(0, px_w as i64) as u32;
    let right = r.clamp(0, px_w as i64) as u32;
    let top = t.clamp(0, px_h as i64) as u32;
    let bottom = b.clamp(0, px_h as i64) as u32;

    assert!(
        left < right && top < bottom,
        "field region rect must map to a non-empty pixel box: \
         x[{left},{right}) y[{top},{bottom}) in {px_w}x{px_h}"
    );

    let (ink, opaque) = ink_bounds(&rgba, px_w, (left, top, right, bottom));

    assert!(
        opaque > 0,
        "field region box must contain opaque page-background pixels"
    );
    assert!(
        ink.is_some(),
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

#[test]
fn every_counted_page_sizes_and_rasterizes() {
    let session = open();
    assert!(session.page_count() > 0, "sample_form has pages");
    for page in 0..session.page_count() {
        assert!(
            session.page_size_pt(page).is_some(),
            "page {page} is counted, so it has an extent"
        );
        assert!(
            session
                .render_rgba(page, 1.0)
                .expect("a counted page rasterizes at 1x")
                .is_some(),
            "page {page} is counted, so it paints"
        );
    }
}

#[test]
fn a_translated_media_box_keeps_page_geometry_on_the_ink() {
    geometry_lands_on_the_ink(blank_page(NARROW_LETTER, None));
}

#[test]
fn a_crop_box_keeps_page_geometry_on_the_ink() {
    geometry_lands_on_the_ink(blank_page([0.0, 0.0, 612.0, 792.0], Some(NARROW_LETTER)));
}

/// A `pdfcrop`-style page box, its lower-left corner away from user-space
/// `(0, 0)`.
const NARROW_LETTER: [f32; 4] = [96.0, 133.0, 500.0, 700.0];

/// One text widget on an otherwise blank page, so every non-white pixel in the
/// raster is the field's flattened value: the reported page size must be the
/// raster's, and the field's region must be the box that value drew in.
fn geometry_lands_on_the_ink(form_pdf: Vec<u8>) {
    const FORM_JSON: &str = r#"{
      "schema": "quillmark/form@0.2.0",
      "fields": [
        {
          "name": "FullName",
          "schema_field": "full_name",
          "page": 0,
          "rect": { "x": 180, "y": 90, "w": 200, "h": 20 }
        }
      ]
    }"#;

    let mut tree = quillmark::tree_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form tree");
    tree.insert(
        "form.pdf",
        FileTreeNode::File { contents: form_pdf },
    )
    .expect("replace form.pdf");
    tree.insert(
        "form.json",
        FileTreeNode::File {
            contents: FORM_JSON.as_bytes().to_vec(),
        },
    )
    .expect("replace form.json");

    let quill = Quill::from_tree(tree).expect("load patched quill");
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    let session = Quillmark::new().open(&quill, &doc).expect("open session");

    let scale: f32 = 2.0;
    let (width_pt, height_pt) = session.page_size_pt(0).expect("page 0 size");
    let (px_w, px_h, rgba) = session
        .render_rgba(0, scale)
        .expect("page 0 rasterizes at 2x")
        .expect("rasterize page 0");

    assert!(
        (px_w as i64 - (width_pt * scale).round() as i64).abs() <= 1
            && (px_h as i64 - (height_pt * scale).round() as i64).abs() <= 1,
        "raster {px_w}x{px_h} should match page_size_pt {width_pt}x{height_pt} × {scale}"
    );

    let region = session
        .regions()
        .into_iter()
        .find(|r| r.page == 0 && r.field == "full_name")
        .expect("a region for the bound text field");
    let box_px = region_box_px(region.rect, height_pt, scale);

    let (ink, _) = ink_bounds(&rgba, px_w, (0, 0, px_w, px_h));
    let ink = ink.expect("the field value draws ink");
    assert!(
        ink.0 >= box_px.0 && ink.1 >= box_px.1 && ink.2 <= box_px.2 && ink.3 <= box_px.3,
        "ink at {ink:?} must lie inside the field's region box {box_px:?} in {px_w}x{px_h}"
    );
}

/// The `(left, top, right, bottom)` pixel box a region's rect covers, unclamped
/// so it can be compared against a bbox that a caller measured itself.
fn region_box_px(rect: [f32; 4], height_pt: f32, scale: f32) -> (i64, i64, i64, i64) {
    let [x0, y0, x1, y1] = rect;
    (
        (x0 * scale).floor() as i64,
        ((height_pt - y1) * scale).floor() as i64,
        (x1 * scale).ceil() as i64,
        ((height_pt - y0) * scale).ceil() as i64,
    )
}

/// Inside the pixel box `(left, top, right, bottom)`: the bounds of every
/// non-white opaque pixel — `None` when the box holds none — and how many
/// opaque pixels the box holds at all.
fn ink_bounds(
    rgba: &[u8],
    px_w: u32,
    (left, top, right, bottom): (u32, u32, u32, u32),
) -> (Option<(i64, i64, i64, i64)>, u64) {
    let mut bounds: Option<(i64, i64, i64, i64)> = None;
    let mut opaque = 0u64;
    for y in top as i64..bottom as i64 {
        for x in left as i64..right as i64 {
            let i = ((y as usize) * (px_w as usize) + (x as usize)) * 4;
            let (r, g, b, a) = (rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]);
            if a != 255 {
                continue;
            }
            opaque += 1;
            if r < 250 || g < 250 || b < 250 {
                bounds = Some(match bounds {
                    None => (x, y, x + 1, y + 1),
                    Some((l, t, rt, bt)) => (l.min(x), t.min(y), rt.max(x + 1), bt.max(y + 1)),
                });
            }
        }
    }
    (bounds, opaque)
}

/// A one-page background drawing nothing, with `media` as its `/MediaBox` and
/// `crop` as its `/CropBox` when given.
fn blank_page(media: [f32; 4], crop: Option<[f32; 4]>) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    let media = Rect::new(media[0], media[1], media[2], media[3]);

    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id)
        .kids([page_id])
        .count(1)
        .media_box(media)
        .finish();
    {
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id)
            .media_box(media)
            .contents(content_id);
        if let Some(c) = crop {
            page.pair(
                Name(b"CropBox"),
                Rect::new(c[0], c[1], c[2], c[3]),
            );
        }
    }
    pdf.stream(content_id, &Content::new().finish());
    pdf.finish()
}
