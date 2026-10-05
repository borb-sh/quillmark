//! A raster the backend cannot allocate is refused before it is asked for:
//! `typst_render` takes the pixel dimensions unchecked and unwraps the buffer.
//!
//! What counts as unrasterizable is `quillmark_core::backend`'s to say and is
//! pinned there; what this backend owes is the wiring — both raster knobs reach
//! that check, and every page it counts paints at the size the check counts.

use quillmark_core::{
    backend::{raster_size, Backend},
    error::RenderError,
    session::LiveSession,
    types::{OutputFormat, RenderOptions},
};
use quillmark_typst::TypstBackend;

mod common;
use common::{quill_with_plate as quill, yaml};

const PLATE: &str = "#set page(width: 200pt, height: 120pt, margin: 12pt)\nink\n";

fn open() -> LiveSession {
    TypstBackend
        .open(
            &quill(&yaml("main:\n  fields: {}\n"), PLATE),
            &serde_json::json!({}),
            common::test_date(),
        )
        .expect("open")
}

fn code(err: RenderError) -> String {
    err.diagnostics()[0]
        .code
        .clone()
        .expect("a refusal carries its code")
}

#[test]
fn both_raster_knobs_reach_the_check_and_every_counted_page_paints() {
    let session = open();

    let png = |ppi: f32| {
        RenderOptions::default()
            .with_output_format(OutputFormat::Png)
            .with_ppi(ppi)
    };
    assert_eq!(
        code(session
            .render(&png(f32::INFINITY))
            .expect_err("an infinite ppi is not rasterizable")),
        "backend::invalid_raster_scale"
    );
    assert!(
        !session.render(&png(144.0)).expect("144 ppi renders").artifacts[0]
            .bytes
            .is_empty()
    );

    assert_eq!(
        code(session
            .render_rgba(0, f32::INFINITY)
            .expect_err("an infinite canvas scale is not rasterizable")),
        "backend::invalid_raster_scale"
    );
    assert!(
        session
            .render_rgba(99, 2.0)
            .expect("a page out of range is not a refused scale")
            .is_none(),
        "an out-of-range page still answers None"
    );

    assert!(session.page_count() > 0, "the plate draws a page");
    for page in 0..session.page_count() {
        let (width_pt, height_pt) = session
            .page_size_pt(page)
            .unwrap_or_else(|| panic!("page {page} is counted, so it has an extent"));
        for scale in [0.001, 1.0, 2.5] {
            let (w, h, _) = session
                .render_rgba(page, scale)
                .unwrap_or_else(|e| panic!("page {page} rasterizes at {scale}x: {e}"))
                .unwrap_or_else(|| panic!("page {page} is counted, so it paints"));
            assert_eq!(
                (w, h),
                raster_size(scale, width_pt, height_pt),
                "page {page} at {scale}x is the size the check counts"
            );
        }
    }
}

#[test]
fn the_ppi_a_refusal_names_renders_every_selected_page() {
    let plate = "#page(width: 100pt, height: 9000pt)[A]\n#page(width: 100pt, height: 12000pt)[B]\n";
    let session = TypstBackend
        .open(
            &quill(&yaml("main:\n  fields: {}\n"), plate),
            &serde_json::json!({}),
            common::test_date(),
        )
        .expect("open");
    let png = |ppi: f32| {
        RenderOptions::default()
            .with_output_format(OutputFormat::Png)
            .with_ppi(ppi)
    };

    let refused = session
        .render(&png(RenderOptions::DEFAULT_PPI))
        .expect_err("both pages pass the ceiling at the default ppi");
    let hint = refused.diagnostics()[0]
        .hint
        .clone()
        .expect("a raster refusal carries a hint");
    let ppi = hint
        .split_whitespace()
        .find_map(|word| word.parse::<f32>().ok())
        .unwrap_or_else(|| panic!("the hint names a ppi: {hint}"));
    let rendered = session
        .render(&png(ppi))
        .unwrap_or_else(|e| panic!("{ppi} ppi, as hinted, renders both pages: {e}"));
    assert_eq!(rendered.artifacts.len(), 2);

    session
        .render(&png(131.0).with_pages(vec![0]))
        .expect("a longer page outside the selection does not hold it to a lower ppi");
}
