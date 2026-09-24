//! A raster the backend cannot allocate is refused before it is asked for:
//! `typst_render` takes the pixel dimensions unchecked and unwraps the buffer.
//!
//! What counts as unrasterizable is `quillmark_core::backend`'s to say and is
//! pinned there; what this backend owes is the wiring — both raster knobs reach
//! that check, and every page it counts paints.

use quillmark_core::{
    backend::Backend,
    error::RenderError,
    session::LiveSession,
    types::{OutputFormat, RenderOptions},
};
use quillmark_typst::TypstBackend;

mod common;
use common::quill_with_plate as quill;

const YAML: &str = r#"
quill:
  name: raster_scale
  version: 0.1.0
  backend: typst
  description: one small page to rasterize
typst:
  plate_file: plate.typ
main:
  fields: {}
"#;

const PLATE: &str = "#set page(width: 200pt, height: 120pt, margin: 12pt)\nink\n";

fn open() -> LiveSession {
    TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}), None)
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
