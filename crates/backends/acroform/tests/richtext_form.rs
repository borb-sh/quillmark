//! End-to-end acceptance for the `richtext_form` fixture. A richtext field
//! crosses the seam as canonical content JSON and lowers to `Content.text` for
//! the widget `/V`; the Adobe-only `/RV` entry is never written.

use lopdf::Document as PdfDoc;
use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};

// `headline` is inline richtext, `bio` block richtext.
const FILLED: &str = "~~~\n\
$quill: richtext_form\n\
$kind: main\n\
headline: The **headline**\n\
bio: A **bold** claim and _emphasis_.\n\
~~~\n";

mod common;
use common::{decode_pdf_text, widget};

#[test]
fn richtext_fields_lower_to_plaintext_field_values() {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("richtext_form"))
        .expect("load richtext_form quill");
    let engine = Quillmark::new();
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    let result = engine
        .render(
            &quill,
            &doc,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("render ok");
    assert_eq!(result.output_format, OutputFormat::Pdf);

    let pdf = &result.artifacts[0].bytes;
    let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse: structurally valid");
    let cat = doc.catalog().expect("catalog");
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();

    let headline = widget(&doc, af, "FullName");
    assert_eq!(
        decode_pdf_text(headline.get(b"V").unwrap().as_str().unwrap()),
        "The headline"
    );

    let bio = widget(&doc, af, "Comments");
    assert_eq!(
        decode_pdf_text(bio.get(b"V").unwrap().as_str().unwrap()),
        "A bold claim and emphasis."
    );
}

