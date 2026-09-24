//! End-to-end acceptance for the `sample_form` fixture: render through the full
//! engine, reparse with lopdf, assert the filled AcroForm: a value lands in
//! `/V`, which a synthesizing viewer renders from.

use lopdf::Document as PdfDoc;
use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};

// `headline` is inline richtext, `bio` block richtext.
const FILLED: &str = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Ada Lovelace\n\
comments:\n\
  - First comment line.\n\
  - Second comment line.\n\
agree: true\n\
favorite_color: green\n\
headline: The **headline**\n\
bio: A **bold** claim and _emphasis_.\n\
~~~\n";

fn quill() -> quillmark::Quill {
    quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill")
}

fn open_session(markdown: &str) -> quillmark::LiveSession {
    let doc = Document::parse(markdown).expect("parse markdown").document;
    Quillmark::new().open(&quill(), &doc, None).expect("open ok")
}

/// The rendered PDF and its AcroForm dict.
fn acroform(result: &quillmark::RenderResult) -> (PdfDoc, lopdf::Dictionary) {
    let doc = PdfDoc::load_mem(&result.artifacts[0].bytes).expect("lopdf reparse: structurally valid");
    let af_ref = doc.catalog().unwrap().get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap().clone();
    (doc, af)
}

fn render(markdown: &str) -> (PdfDoc, lopdf::Dictionary) {
    let doc = Document::parse(markdown).expect("parse markdown").document;
    let result = Quillmark::new()
        .render(
            &quill(),
            &doc,
            None,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("render ok");
    acroform(&result)
}

mod common;
use common::{decode_pdf_text, widget};

#[test]
fn fixture_renders_structurally_valid_filled_pdf() {
    let (doc, af) = render(FILLED);
    let af = &af;
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    assert_eq!(af.get(b"Fields").unwrap().as_array().unwrap().len(), 10);

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

    // A richtext field crosses the seam as canonical content JSON and lowers to
    // `Content.text` for the widget `/V`; the Adobe-only `/RV` is never written.
    for (name, text) in [
        ("Headline", "The headline"),
        ("Bio", "A bold claim and emphasis."),
    ] {
        let w = widget(&doc, af, name);
        assert_eq!(decode_pdf_text(w.get(b"V").unwrap().as_str().unwrap()), text);
        assert!(w.get(b"RV").is_err(), "{name} carries no /RV");
    }

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
    let (doc, af) = render(FILLED);
    let af = &af;

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

/// A non-ASCII value round-trips through `/V`; an unchecked box and an absent
/// array render blank.
#[test]
fn a_non_ascii_value_round_trips_and_unset_fields_render_blank() {
    let md = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: \"Café — Señor 'Ünïcøde'\"\n\
agree: false\n\
favorite_color: red\n\
~~~\n";
    let (doc, af) = render(md);
    let af = &af;

    let full = widget(&doc, af, "FullName");
    assert_eq!(
        decode_pdf_text(full.get(b"V").unwrap().as_str().unwrap()),
        "Café — Señor 'Ünïcøde'"
    );

    let agree = widget(&doc, af, "Agree");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Off");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Off");

    let comments = widget(&doc, af, "Comments");
    assert!(comments.get(b"V").is_err(), "absent array → no /V");
}

#[test]
fn apply_rebinds_values_and_reports_dirty_pages() {
    let doc = Document::parse(FILLED).expect("parse markdown").document;
    let mut session = open_session(FILLED);

    let cs = session.update(&doc).expect("update");
    assert_eq!(cs.page_count, session.page_count());
    assert!(cs.dirty_pages.is_empty(), "dirty: {:?}", cs.dirty_pages);

    let doc2 = Document::parse(&FILLED.replace("Ada Lovelace", "Grace Hopper"))
        .expect("parse markdown")
        .document;
    let cs = session.update(&doc2).expect("update");
    assert_eq!(cs.dirty_pages, vec![0]);

    let (pdf, af) = acroform(
        &session
            .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
            .expect("render ok"),
    );
    let name = widget(&pdf, &af, "FullName");
    assert_eq!(
        decode_pdf_text(name.get(b"V").unwrap().as_str().unwrap()),
        "Grace Hopper"
    );
}

/// One region per schema-bound field, in form.json order, which is stamping
/// order: the fixture's four unbound widgets produce none.
#[test]
fn regions_follow_form_json_order_which_is_stamping_order() {
    let regions = open_session(FILLED).regions();
    assert_eq!(
        regions.iter().map(|r| r.field.as_str()).collect::<Vec<_>>(),
        [
            "full_name",
            "comments",
            "agree",
            "favorite_color",
            "headline",
            "bio"
        ]
    );
}
