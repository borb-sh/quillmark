//! End-to-end acceptance test for the `richtext_form` fixture: a pdfform quill
//! that binds richtext fields. It exercises the corpus → plaintext lowering —
//! a richtext field crosses the seam as canonical corpus JSON and pdfform lowers
//! it to `RichText.text` (markup dropped, island slots stripped) for the widget
//! `/V`. (No pdfform field carries rich formatting; Adobe-only `/RV` is
//! deferred.)

use lopdf::Document as PdfDoc;
use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};

// `headline` (inline richtext) and `bio` (block richtext) authored as markdown;
// coercion imports each to a corpus, and pdfform lowers each to plaintext.
const FILLED: &str = "~~~\n\
$quill: richtext_form\n\
$kind: main\n\
headline: The **headline**\n\
bio: A **bold** claim and _emphasis_.\n\
~~~\n";

fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

fn widget<'a>(doc: &'a PdfDoc, af: &lopdf::Dictionary, name: &str) -> &'a lopdf::Dictionary {
    for f in af.get(b"Fields").unwrap().as_array().unwrap() {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        if w.get(b"T").unwrap().as_str().unwrap() == name.as_bytes() {
            return w;
        }
    }
    panic!("no field named {name}");
}

#[test]
fn richtext_fields_lower_to_plaintext_field_values() {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("richtext_form"))
        .expect("load richtext_form quill");
    let engine = Quillmark::new();
    let doc = Document::from_markdown(FILLED).expect("parse markdown");
    let result = engine
        .render(
            &quill,
            &doc,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            },
        )
        .expect("render ok");
    assert_eq!(result.output_format, OutputFormat::Pdf);

    let pdf = &result.artifacts[0].bytes;
    let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse — structurally valid");
    let cat = doc.catalog().expect("catalog");
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();

    // Inline richtext → plaintext: the `**` markup is dropped (marks live off the
    // text), leaving "The headline".
    let headline = widget(&doc, af, "FullName");
    assert_eq!(
        decode_pdf_text(headline.get(b"V").unwrap().as_str().unwrap()),
        "The headline"
    );

    // Block richtext → plaintext into the multiline widget.
    let bio = widget(&doc, af, "Comments");
    assert_eq!(
        decode_pdf_text(bio.get(b"V").unwrap().as_str().unwrap()),
        "A bold claim and emphasis."
    );
}
