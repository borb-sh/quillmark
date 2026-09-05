//! SVG and PNG are views of the *flattened* form — values baked into the page
//! content — so they render without viewer appearance synthesis.

use pdf_writer::{Pdf, Rect, Ref};
use quillmark::{Document, FileTreeNode, OutputFormat, Quill, Quillmark, RenderOptions};
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

fn render(format: OutputFormat, ppi: Option<f32>) -> Vec<quillmark_core::Artifact> {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    let mut opts = RenderOptions::default().with_output_format(format);
    opts.ppi = ppi;
    engine
        .render(&quill, &doc, &opts)
        .unwrap_or_else(|e| panic!("render {format:?}: {e:?}"))
        .artifacts
}

#[test]
fn renders_svg_per_page() {
    let artifacts = render(OutputFormat::Svg, None);
    assert!(!artifacts.is_empty(), "at least one SVG page");
    for art in &artifacts {
        assert_eq!(art.output_format, OutputFormat::Svg);
        let text = std::str::from_utf8(&art.bytes).expect("SVG is UTF-8");
        assert!(text.contains("<svg"), "artifact must be an SVG document");
    }
}

#[test]
fn renders_png_per_page() {
    let artifacts = render(OutputFormat::Png, Some(96.0));
    assert!(!artifacts.is_empty(), "at least one PNG page");
    for art in &artifacts {
        assert_eq!(art.output_format, OutputFormat::Png);
        assert!(
            art.bytes
                .starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            "artifact must carry the PNG signature"
        );
    }
}

/// Two widgets, one per page, so a bound field exercises each page box.
const TWO_PAGE_FORM_JSON: &str = r#"{
  "schema": "quillmark/form@0.2.0",
  "fields": [
    {
      "name": "FullName",
      "schema_field": "full_name",
      "page": 0,
      "rect": { "x": 180, "y": 100, "w": 340, "h": 20 }
    },
    {
      "name": "Comments",
      "schema_field": "comments",
      "page": 1,
      "rect": { "x": 180, "y": 140, "w": 340, "h": 80 }
    }
  ]
}"#;

/// Two US-Letter pages drawing a rule at different heights, so one page's ink
/// never matches the other's.
fn two_page_background() -> Vec<u8> {
    let letter = Rect::new(0.0, 0.0, 612.0, 792.0);
    let mut pdf = Pdf::new();
    pdf.catalog(Ref::new(1)).pages(Ref::new(2));
    pdf.pages(Ref::new(2))
        .kids([Ref::new(3), Ref::new(5)])
        .count(2)
        .media_box(letter);
    pdf.page(Ref::new(3))
        .parent(Ref::new(2))
        .media_box(letter)
        .contents(Ref::new(4));
    pdf.stream(Ref::new(4), b"0.75 w 180 672 340 20 re S");
    pdf.page(Ref::new(5))
        .parent(Ref::new(2))
        .media_box(letter)
        .contents(Ref::new(6));
    pdf.stream(Ref::new(6), b"0.75 w 180 472 340 20 re S");
    pdf.finish()
}

/// The fixture quill on a two-page background: page selection needs a document
/// with more than one page to select from.
fn two_page_quill() -> Quill {
    let mut tree = quillmark::tree_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form tree");
    tree.insert(
        "form.pdf",
        FileTreeNode::File {
            contents: two_page_background(),
        },
    )
    .expect("replace form.pdf");
    tree.insert(
        "form.json",
        FileTreeNode::File {
            contents: TWO_PAGE_FORM_JSON.as_bytes().to_vec(),
        },
    )
    .expect("replace form.json");
    Quill::from_tree(tree).expect("load two-page quill")
}

fn render_two_page(
    format: OutputFormat,
    pages: Option<Vec<usize>>,
) -> Result<quillmark::RenderResult, RenderError> {
    let quill = two_page_quill();
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    let mut opts = RenderOptions::default().with_output_format(format);
    opts.pages = pages;
    Quillmark::new().render(&quill, &doc, &opts)
}

fn refusal_code(format: OutputFormat, pages: Vec<usize>) -> String {
    let err = render_two_page(format, Some(pages)).expect_err("the selection is refused");
    err.diagnostics()[0]
        .code
        .clone()
        .expect("a refusal carries its code")
}

#[test]
fn page_selection_narrows_raster_output_to_the_named_page() {
    for format in [OutputFormat::Svg, OutputFormat::Png] {
        let whole = render_two_page(format, None).expect("render whole document");
        assert_eq!(whole.artifacts.len(), 2, "{format:?}: one artifact per page");

        let first = render_two_page(format, Some(vec![0])).expect("render page 0");
        let second = render_two_page(format, Some(vec![1])).expect("render page 1");
        assert_eq!(first.artifacts.len(), 1, "{format:?}: one page selected");
        assert_eq!(second.artifacts.len(), 1, "{format:?}: one page selected");
        assert_ne!(
            first.artifacts[0].bytes, second.artifacts[0].bytes,
            "{format:?}: the selection renders the named page, not always the first"
        );
    }
}

#[test]
fn a_page_past_the_form_is_refused() {
    for format in [OutputFormat::Svg, OutputFormat::Png] {
        assert_eq!(
            refusal_code(format, vec![2]),
            "backend::page_index_out_of_bounds",
            "{format:?}"
        );
    }
}

#[test]
fn pdf_refuses_a_page_selection() {
    assert_eq!(
        refusal_code(OutputFormat::Pdf, vec![0]),
        "backend::page_selection_not_supported"
    );
}

