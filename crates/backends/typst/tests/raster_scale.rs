//! A raster the backend cannot allocate is refused before it is asked for:
//! `typst_render` takes the pixel dimensions unchecked and unwraps the buffer.

use quillmark_core::{Backend, LiveSession, OutputFormat, RenderError, RenderOptions};
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

/// Every value a caller can hand either knob and get no raster from: not
/// finite, not positive, or past the pixel ceiling.
const UNRASTERIZABLE_PPI: [f32; 5] = [f32::INFINITY, f32::NAN, 0.0, -144.0, 1e9];

fn open() -> LiveSession {
    TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}))
        .expect("open")
}

fn code(err: RenderError) -> String {
    err.diagnostics()[0]
        .code
        .clone()
        .expect("a refusal carries its code")
}

#[test]
fn a_ppi_that_cannot_be_rasterized_is_refused_rather_than_rendered() {
    let session = open();
    for ppi in UNRASTERIZABLE_PPI {
        let opts = RenderOptions::default()
            .with_output_format(OutputFormat::Png)
            .with_ppi(ppi);
        assert_eq!(
            code(session
                .render(&opts)
                .err()
                .unwrap_or_else(|| panic!("{ppi} ppi is not rasterizable"))),
            "backend::invalid_raster_scale"
        );
    }

    let opts = RenderOptions::default()
        .with_output_format(OutputFormat::Png)
        .with_ppi(144.0);
    assert!(!session.render(&opts).expect("144 ppi renders").artifacts[0]
        .bytes
        .is_empty());
}

#[test]
fn a_canvas_scale_that_cannot_be_rasterized_is_refused_rather_than_painted() {
    let session = open();
    for scale in UNRASTERIZABLE_PPI.map(|ppi| ppi / 72.0) {
        assert_eq!(
            code(session
                .render_rgba(0, scale)
                .err()
                .unwrap_or_else(|| panic!("{scale}x is not rasterizable"))),
            "backend::invalid_raster_scale"
        );
    }

    assert!(session
        .render_rgba(0, 2.0)
        .expect("2x rasterizes")
        .is_some());
    assert!(
        session
            .render_rgba(99, 2.0)
            .expect("a page out of range is not a refused scale")
            .is_none(),
        "an out-of-range page still answers None"
    );
}
