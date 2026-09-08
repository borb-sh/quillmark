//! PDF is the whole `render()` surface: the stamped, interactive AcroForm,
//! emitted whole. Canvas paint (`render_rgba`) is a separate seam, covered by
//! `canvas_conformance.rs`.

use quillmark::{Document, OutputFormat, Quill, Quillmark, RenderOptions};
use quillmark_core::RenderError;

const FILLED: &str = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Ada Lovelace\n\
comments:\n\
  - First comment line.\n\
agree: true\n\
favorite_color: green\n\
~~~\n";

fn sample_form() -> Quill {
    quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill")
}

fn render(opts: &RenderOptions) -> Result<quillmark::RenderResult, RenderError> {
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    Quillmark::new().render(&sample_form(), &doc, opts)
}

fn refusal_code(opts: &RenderOptions) -> String {
    let err = render(opts).expect_err("the render is refused");
    err.diagnostics()[0]
        .code
        .clone()
        .expect("a refusal carries its code")
}

#[test]
fn pdf_is_the_only_output_format() {
    let engine = Quillmark::new();
    assert_eq!(
        engine
            .supported_formats(&sample_form())
            .expect("the pdfform backend resolves"),
        [OutputFormat::Pdf]
    );

    let artifacts = render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("render PDF")
        .artifacts;
    assert_eq!(artifacts.len(), 1, "the AcroForm is emitted whole");
    assert_eq!(artifacts[0].output_format, OutputFormat::Pdf);
    assert!(artifacts[0].bytes.starts_with(b"%PDF-"));
}

#[test]
fn a_visual_page_format_is_refused() {
    for format in [OutputFormat::Svg, OutputFormat::Png] {
        assert_eq!(
            refusal_code(&RenderOptions::default().with_output_format(format)),
            "backend::format_not_supported",
            "{format:?}"
        );
    }
}

#[test]
fn pdf_refuses_a_page_selection() {
    let mut opts = RenderOptions::default().with_output_format(OutputFormat::Pdf);
    opts.pages = Some(vec![0]);
    assert_eq!(refusal_code(&opts), "backend::page_selection_not_supported");
}
