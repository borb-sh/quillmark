//! `RenderOptions::pages` is backend-neutral: this backend answers it under the
//! same `backend::*` codes the PDF-form backend does.

use quillmark_core::{Backend, LiveSession, OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;
use serde_json::json;

mod common;

const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#data.at("msg")
#pagebreak()
second
"#;

fn session() -> LiveSession {
    let yaml = "quill:\n  name: pages\n  version: 0.1.0\n  backend: typst\n  description: page-selection acceptance quill\n\ntypst:\n  plate_file: plate.typ\n\nmain:\n  fields:\n    msg:\n      description: message\n      type: string\n";
    TypstBackend
        .open(
            &common::quill_with_plate(yaml, PLATE),
            &json!({ "msg": "first" }),
        )
        .expect("open")
}

fn refusal_code(format: OutputFormat, pages: Vec<usize>) -> String {
    let mut opts = RenderOptions::default().with_output_format(format);
    opts.pages = Some(pages);
    let err = session()
        .render(&opts)
        .expect_err("the selection is refused");
    err.diagnostics()[0]
        .code
        .clone()
        .expect("a refusal carries its code")
}

#[test]
fn page_selection_narrows_svg_to_the_named_page() {
    let session = session();
    assert_eq!(session.page_count(), 2, "the plate breaks onto a second page");

    let mut opts = RenderOptions::default().with_output_format(OutputFormat::Svg);
    opts.pages = Some(vec![1]);
    let selected = session.render(&opts).expect("render page 1");
    assert_eq!(selected.artifacts.len(), 1);

    opts.pages = None;
    assert_eq!(session.render(&opts).expect("render whole").artifacts.len(), 2);
}

#[test]
fn a_page_past_the_document_is_refused() {
    assert_eq!(
        refusal_code(OutputFormat::Svg, vec![2]),
        "backend::page_index_out_of_bounds"
    );
}

#[test]
fn pdf_refuses_a_page_selection() {
    assert_eq!(
        refusal_code(OutputFormat::Pdf, vec![0]),
        "backend::page_selection_not_supported"
    );
}
