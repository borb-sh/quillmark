//! `field-region` through the public `Backend`/`LiveSession` path: what a
//! preview consumer actually reads back.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;

const YAML: &str = r#"
quill:
  name: field_region
  version: 0.1.0
  backend: typst
  description: field-region acceptance
typst:
  plate_file: plate.typ
main:
  fields:
    classification:
      type: string
      description: a scalar the plate presents as a banner
    subject:
      type: string
      description: an unrelated scalar
"#;

fn compile(
    plate: &str,
) -> Result<quillmark_core::session::LiveSession, quillmark_core::error::RenderError> {
    let source = common::quill_with_plate(YAML, plate);
    TypstBackend.open(
        &source,
        &serde_json::json!({ "classification": "SECRET", "subject": "Widgets" }),
    )
}

fn open(plate: &str) -> quillmark_core::session::LiveSession {
    compile(plate).expect("open")
}

#[test]
fn a_claim_surfaces_as_a_region_and_answers_field_at() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 200pt, margin: 40pt)
#let banner(level) = box(stroke: 1pt, inset: 6pt)[#upper(level)]
#field-region("classification")[#banner(data.classification)]
"#;
    let session = open(plate);
    let region = session
        .regions()
        .into_iter()
        .find(|r| r.field == "classification")
        .expect("the claim surfaces in the sidecar");
    assert!(region.span.is_none(), "a claim carries no content span");

    let (cx, cy) = (
        (region.rect[0] + region.rect[2]) / 2.0,
        (region.rect[1] + region.rect[3]) / 2.0,
    );
    assert_eq!(
        session.field_at(region.page, cx, cy, 0.0).as_deref(),
        Some("classification"),
        "a click inside the claim routes to its field"
    );
}

#[test]
fn a_claim_does_not_displace_a_nested_scalar_site() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 200pt, margin: 40pt)
#field-region("subject")[Level: #data.classification]
"#;
    let regions = open(plate).regions();
    for field in ["subject", "classification"] {
        assert!(
            regions.iter().any(|r| r.field == field),
            "{field:?} keeps a region of its own: {regions:?}"
        );
    }
}

#[test]
fn a_claim_lays_its_body_out_where_the_body_alone_would_land() {
    // 0.01pt sits between the two scales in play: one stray space in the
    // inline flow costs 2.715pt at this body size, while shaping `AAABBBCCC`
    // as three runs instead of one costs float noise near 1e-14pt.
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": field-region
#set page(width: 400pt, height: 200pt, margin: 40pt)
#let same(what, bare, got) = assert(calc.abs(got - bare) < 0.01pt,
  message: what + " moved: " + repr(bare) + " -> " + repr(got))
#context {
  let bare = measure[AAABBBCCC]
  let claimed = measure[AAA#field-region("subject")[BBB]CCC]
  same("width", bare.width, claimed.width)
  same("height", bare.height, claimed.height)
}
"#;
    if let Err(err) = compile(plate) {
        panic!("{err}");
    }
}

/// The same marker shape as `field-region`'s, neutral only because a `box`
/// follows it rather than text.
#[test]
fn a_widget_lays_out_where_its_box_alone_would_land() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 400pt, height: 200pt, margin: 40pt)
#context {
  let bare = measure[AAA#box(width: 20pt, height: 8pt)CCC].width
  let widget = measure[AAA#form-field("F", width: 20pt, height: 8pt)CCC].width
  assert(calc.abs(widget - bare) < 0.01pt,
    message: "the widget moved the line: " + repr(bare) + " -> " + repr(widget))
}
"#;
    if let Err(err) = compile(plate) {
        panic!("{err}");
    }
}

/// The symptom — chrome routing clicks to a field — does not point at its
/// cause, and only the plate author can fix it.
#[test]
fn an_unclosed_claim_warns_and_claims_nothing() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 300pt, height: 200pt, margin: 20pt, header: [PAGE CHROME])
#let r = field-region("classification")[#box(stroke: 1pt)[X]]
#r.children.at(0)
#lorem(300)
"#;
    let session = open(plate);
    assert!(
        session.page_count() > 1,
        "the runaway needs a page after the stranded open"
    );

    let warning = session
        .warnings()
        .iter()
        .find(|d| d.code.as_deref() == Some("typst::unclosed_field_region"))
        .expect("the unclosed claim is reported");
    assert!(
        warning.message.contains("classification"),
        "the warning names the field the author must fix: {}",
        warning.message
    );

    assert!(
        !session.regions().iter().any(|r| r.field == "classification"),
        "and the claim surfaces nothing rather than every page's chrome"
    );
}

#[test]
fn an_unknown_field_address_fails_the_compile() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": field-region
#field-region("not_a_field")[x]
"#;
    let source = common::quill_with_plate(YAML, plate);
    let Err(err) = TypstBackend.open(&source, &serde_json::json!({})) else {
        panic!("an unknown address must not compile");
    };
    assert!(
        format!("{err}").contains("not a schema field address"),
        "the assert names the problem: {err}"
    );
}
