//! End-to-end acceptance for the `sample_form` fixture: render through the full
//! engine, reparse with lopdf, assert the filled AcroForm. Technique A means
//! values land in `/V`; appearance synthesis is the viewer's job.

use lopdf::Document as PdfDoc;
use quillmark::{Document, FileTreeNode, OutputFormat, Quill, Quillmark, RenderOptions};

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

fn render(markdown: &str) -> quillmark::RenderResult {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::parse(markdown).expect("parse markdown").document;
    engine
        .render(
            &quill,
            &doc,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("render ok")
}

fn open_session(markdown: &str) -> quillmark::LiveSession {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::parse(markdown).expect("parse markdown").document;
    engine.open(&quill, &doc).expect("open ok")
}

mod common;
use common::{decode_pdf_text, widget};

#[test]
fn fixture_renders_structurally_valid_filled_pdf() {
    let result = render(FILLED);
    assert_eq!(result.output_format, OutputFormat::Pdf);
    let pdf = &result.artifacts[0].bytes;

    let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse: structurally valid");
    let cat = doc.catalog().expect("catalog");
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    assert_eq!(af.get(b"Fields").unwrap().as_array().unwrap().len(), 8);

    // The form.json field carries no `tooltip`, so `/TU` is inherited from the
    // schema field's `description`.
    let full = widget(&doc, af, "FullName");
    assert_eq!(full.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert_eq!(
        full.get(b"TU").unwrap().as_str().unwrap(),
        b"Full legal name of the applicant. Binds the FullName text field."
    );

    let comments = widget(&doc, af, "Comments");
    assert_eq!(
        decode_pdf_text(comments.get(b"V").unwrap().as_str().unwrap()),
        "First comment line.\nSecond comment line."
    );

    let color = widget(&doc, af, "FavoriteColor");
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"green");

    // One region per schema-bound field: the fixture's four unbound widgets
    // carry no `schema_field`, so eight fields yield four regions.
    let regions = open_session(FILLED).regions();
    assert_eq!(regions.len(), 4);
    assert!(
        regions
            .iter()
            .all(|r| !r.field.starts_with("Signer") && r.field != "Signature"),
        "no unbound widget produces a region"
    );
    let r_full = regions.iter().find(|r| r.field == "full_name").unwrap();
    assert!(r_full.page < doc.get_pages().len().max(1));
    assert!(
        r_full.rect[2] > r_full.rect[0] && r_full.rect[3] > r_full.rect[1],
        "region rect is a proper box: {:?}",
        r_full.rect
    );

    let info = doc
        .get_object(doc.trailer.get(b"Info").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let producer = info.get(b"Producer").unwrap().as_str().unwrap();
    assert!(
        producer.starts_with(b"Quillmark "),
        "producer = {:?}",
        String::from_utf8_lossy(producer)
    );
}

#[test]
fn unbound_widgets_stamp_their_declared_kind_and_take_no_value() {
    let result = render(FILLED);
    let doc = PdfDoc::load_mem(&result.artifacts[0].bytes).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();

    for (name, ft) in [
        ("SignerInitials", &b"Tx"[..]),
        ("SignerConfirms", &b"Btn"[..]),
        ("SignerRole", &b"Ch"[..]),
        ("Signature", &b"Sig"[..]),
    ] {
        assert_eq!(
            widget(&doc, af, name).get(b"FT").unwrap().as_name().unwrap(),
            ft,
            "{name}"
        );
    }

    let opts: Vec<String> = widget(&doc, af, "SignerRole")
        .get(b"Opt")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| decode_pdf_text(o.as_str().unwrap()))
        .collect();
    assert_eq!(opts, ["witness", "notary", "guardian"]);

    // No `schema_field`, so the document's own `agree: true` cannot reach these.
    assert!(widget(&doc, af, "SignerInitials").get(b"V").is_err());
    assert!(widget(&doc, af, "SignerRole").get(b"V").is_err());
    assert_eq!(
        widget(&doc, af, "SignerConfirms")
            .get(b"V")
            .unwrap()
            .as_name()
            .unwrap(),
        b"Off"
    );
}

#[test]
fn non_ascii_value_round_trips_through_acroform_v() {
    let md = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: \"Café — Señor 'Ünïcøde'\"\n\
agree: true\n\
favorite_color: green\n\
~~~\n";
    let result = render(md);
    let pdf = &result.artifacts[0].bytes;
    let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse");
    let cat = doc.catalog().unwrap();
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();

    let full = widget(&doc, af, "FullName");
    assert_eq!(
        decode_pdf_text(full.get(b"V").unwrap().as_str().unwrap()),
        "Café — Señor 'Ünïcøde'"
    );
}

#[test]
fn unchecked_and_unmatched_choice_render_blank() {
    let md = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Bob\n\
agree: false\n\
favorite_color: red\n\
~~~\n";
    let result = render(md);
    let pdf = &result.artifacts[0].bytes;
    let doc = PdfDoc::load_mem(pdf).unwrap();
    let cat = doc.catalog().unwrap();
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();

    let agree = widget(&doc, af, "Agree");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Off");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Off");

    let comments = widget(&doc, af, "Comments");
    assert!(comments.get(b"V").is_err(), "absent array → no /V");
}

#[test]
fn apply_rebinds_values_and_reports_dirty_pages() {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    let mut session = engine.open(&quill, &doc).expect("open ok");

    let cs = session.update(&doc).expect("update");
    assert_eq!(cs.page_count, session.page_count());
    assert!(cs.dirty_pages.is_empty(), "dirty: {:?}", cs.dirty_pages);

    let doc2 = Document::parse(&FILLED.replace("Ada Lovelace", "Grace Hopper"))
        .expect("parse markdown")
        .document;
    let cs = session.update(&doc2).expect("update");
    assert_eq!(cs.dirty_pages, vec![0]);

    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("render ok");
    let pdf = PdfDoc::load_mem(&result.artifacts[0].bytes).unwrap();
    let cat = pdf.catalog().unwrap();
    let af = pdf
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let name = widget(&pdf, af, "FullName");
    assert_eq!(
        decode_pdf_text(name.get(b"V").unwrap().as_str().unwrap()),
        "Grace Hopper"
    );
}

/// The fixture's schema with two bound widgets stacked on one rect, so every
/// point inside it sits the same distance from both.
const STACKED_FORM_JSON: &str = r#"{
  "schema": "quillmark/form@0.2.0",
  "fields": [
    {
      "name": "Under",
      "schema_field": "full_name",
      "page": 0,
      "rect": { "x": 180, "y": 100, "w": 340, "h": 20 }
    },
    {
      "name": "Over",
      "schema_field": "comments",
      "page": 0,
      "rect": { "x": 180, "y": 100, "w": 340, "h": 20 }
    }
  ]
}"#;

#[test]
fn field_at_tie_takes_the_later_stamped_widget() {
    let mut tree = quillmark::tree_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form tree");
    tree.insert(
        "form.json",
        FileTreeNode::File {
            contents: STACKED_FORM_JSON.as_bytes().to_vec(),
        },
    )
    .expect("replace form.json");
    let quill = Quill::from_tree(tree).expect("load patched quill");
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    let session = Quillmark::new().open(&quill, &doc).expect("open ok");

    let regions = session.regions();
    assert_eq!(
        regions.iter().map(|r| r.field.as_str()).collect::<Vec<_>>(),
        ["full_name", "comments"],
        "regions follow `form.json` order, which is stamping order"
    );
    assert_eq!(regions[0].rect, regions[1].rect);

    let [x0, y0, x1, y1] = regions[0].rect;
    assert_eq!(
        session
            .field_at(regions[0].page, (x0 + x1) / 2.0, (y0 + y1) / 2.0, 0.0)
            .as_deref(),
        Some("comments")
    );
}
