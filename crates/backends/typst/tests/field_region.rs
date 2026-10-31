//! `qm.region` through the public `Backend`/`LiveSession` path: what a
//! preview consumer actually reads back — a click routed, a warning surfaced,
//! and the ink a claim takes on a laid-out page. Which addresses a claim
//! admits is stated in `quillmark/tests/address_grammar.rs`.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;

fn compile(
    plate: &str,
) -> Result<quillmark_core::session::LiveSession, quillmark_core::error::RenderError> {
    let source = common::quill_with_plate(
        &common::yaml(
            "main:\n  fields:\n    classification: { type: text }\n    subject: { type: text }\n",
        ),
        plate,
    );
    TypstBackend::default().open(
        &source,
        &serde_json::json!({ "classification": "SECRET", "subject": "Widgets" }),
        common::test_date(),
    )
}

fn open(plate: &str) -> quillmark_core::session::LiveSession {
    compile(plate).expect("open")
}

#[test]
fn a_claim_answers_field_at_and_an_unclosed_one_warns_through_the_session() {
    let session = open(
        r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 200pt, margin: 40pt)
#let banner(level) = box(stroke: 1pt, inset: 6pt)[#upper(level)]
#qm.region("classification")[#banner(qm.data.classification)]
"#,
    );
    let region = session
        .regions()
        .into_iter()
        .find(|r| r.field == "classification")
        .expect("the claim surfaces in the sidecar");
    let (cx, cy) = (
        (region.rect[0] + region.rect[2]) / 2.0,
        (region.rect[1] + region.rect[3]) / 2.0,
    );
    assert_eq!(
        session.field_at(region.page, cx, cy, 0.0).as_deref(),
        Some("classification"),
        "a click inside the claim routes to its field"
    );

    // The symptom — chrome routing clicks to a field — does not point at its
    // cause, and only the plate author can fix it.
    let stranded = open(
        r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt, header: [PAGE CHROME])
#let r = qm.region("classification")[#box(stroke: 1pt)[X]]
#r.children.at(0)
#lorem(300)
"#,
    );
    let warning = stranded
        .warnings()
        .iter()
        .find(|d| d.code.as_deref() == Some("typst::unclosed_region"))
        .expect("the unclosed claim is reported")
        .message
        .clone();
    assert!(
        warning.contains("classification"),
        "the warning names the field the author must fix: {warning}"
    );
}

#[test]
fn a_claim_lays_its_body_out_where_the_body_alone_would_land() {
    // 0.01pt sits between the two scales in play: one stray space in the
    // inline flow costs 2.715pt at this body size, while shaping `AAABBBCCC`
    // as three runs instead of one costs float noise near 1e-14pt.
    let plate = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 200pt, margin: 40pt)
#let same(what, bare, got) = assert(calc.abs(got - bare) < 0.01pt,
  message: what + " moved: " + repr(bare) + " -> " + repr(got))
#context {
  let bare = measure[AAABBBCCC]
  let claimed = measure[AAA#qm.region("subject")[BBB]CCC]
  same("width", bare.width, claimed.width)
  same("height", bare.height, claimed.height)
}
"#;
    if let Err(err) = compile(plate) {
        panic!("{err}");
    }
}

/// The same marker shape as `qm.region`'s, neutral only because a `box`
/// follows it rather than text.
#[test]
fn a_widget_lays_out_where_its_box_alone_would_land() {
    let plate = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 200pt, margin: 40pt)
#context {
  let bare = measure[AAA#box(width: 20pt, height: 8pt)CCC].width
  let widget = measure[AAA#qm.form-field("F", width: 20pt, height: 8pt)CCC].width
  assert(calc.abs(widget - bare) < 0.01pt,
    message: "the widget moved the line: " + repr(bare) + " -> " + repr(widget))
}
"#;
    if let Err(err) = compile(plate) {
        panic!("{err}");
    }
}

fn center(r: &quillmark_core::region::RenderedRegion) -> (f32, f32) {
    ((r.rect[0] + r.rect[2]) / 2.0, (r.rect[1] + r.rect[3]) / 2.0)
}

/// A `str` cast to content inside a function argument carries a detached span,
/// so whether it is a claim's ink must not depend on what the plate wrapped it
/// in.
#[test]
fn a_claim_takes_detached_glyphs_its_body_prints() {
    let session = open(
        r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 200pt, margin: 40pt)
#qm.region("subject", text(size: 9pt, "LIT"))
"#,
    );
    let region = session
        .regions()
        .into_iter()
        .find(|r| r.field == "subject")
        .expect("a detached-span body surfaces its claim");
    let (cx, cy) = center(&region);
    assert_eq!(
        session.field_at(region.page, cx, cy, 0.0).as_deref(),
        Some("subject")
    );

    let session = open(
        r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 200pt, margin: 40pt)
#qm.region("subject", list[AAA][BBB])
"#,
    );
    let region = session
        .regions()
        .into_iter()
        .find(|r| r.field == "subject")
        .expect("the list surfaces its claim");
    assert!(
        region.rect[0] < 41.0,
        "the claim's box starts at the bullet on the margin: {:?}",
        region.rect
    );
    assert_eq!(
        session
            .field_at(region.page, region.rect[0] + 0.5, region.rect[3] - 3.0, 0.0)
            .as_deref(),
        Some("subject"),
        "a click on the first bullet routes to the claim"
    );
}

/// Wrapping a content field in a claim moves none of its regions: a bullet or
/// an enum number the claim takes sits between the field's segments, and an
/// underline or strike stroke stays unclaimed, since Typst paints it after its
/// run's glyphs and inside their box, where a claimed stroke would answer a
/// click on the word beneath it.
#[test]
fn a_claim_around_a_content_field_moves_none_of_its_regions() {
    use quillmark_content::model::{Content, Line, LineKind, Mark, MarkKind};
    use quillmark_core::session::LiveSession;
    let open_body = |print: &str, body: &serde_json::Value| {
        let source = common::quill_with_plate(
            &common::yaml(
                "main:\n  fields:\n    subject: { type: text }\n    prose: { type: richtext, multiline: true }\n",
            ),
            &format!(
                "#import \"@local/quillmark-helper:0.2.0\" as qm\n\
                 #set page(width: 400pt, height: 300pt, margin: 40pt)\n\
                 #set underline(stroke: 12pt)\n\
                 #set strike(stroke: 12pt)\n{print}\n"
            ),
        );
        TypstBackend::default()
            .open(
                &source,
                &serde_json::json!({ "subject": "Widgets", "prose": body }),
                common::test_date(),
            )
            .expect("open")
    };
    let of = |session: &LiveSession| -> Vec<_> {
        session
            .regions()
            .into_iter()
            .filter(|r| r.field == "prose")
            .collect()
    };
    let decorated = quillmark_content::serial::to_canonical_value(
        &Content::new(
            "Start struck and underlined, then a long trailing plain run of text.".to_string(),
            vec![Line::new(LineKind::Para)],
        )
        .with_marks(vec![
            Mark::new(6, 12, MarkKind::Strike),
            Mark::new(17, 27, MarkKind::Underline),
        ])
        .into_normalized(),
    );
    let listed = common::content("- one\n- two\n  - nested\n\n1. first\n2. second");
    for body in [&listed, &decorated] {
        let bare = of(&open_body("#qm.data.prose", body));
        assert!(!bare.is_empty(), "sanity: the field is drawn");
        assert_eq!(
            of(&open_body("#qm.region(\"subject\")[#qm.data.prose]", body)),
            bare
        );
    }

    let claimed = open_body("#qm.region(\"subject\")[#qm.data.prose]", &decorated);
    for pos in [9, 20] {
        let glyph = claimed
            .locate("prose", pos)
            .expect("the decorated word is drawn");
        let (cx, cy) = center(&glyph);
        assert_eq!(
            claimed.field_at(glyph.page, cx, cy, 0.0).as_deref(),
            Some("prose"),
            "a click on the stroke band at {pos} answers the content field"
        );
    }
}

/// Typst walks a page's background and header before its body, so a claim
/// open across a page break meets the next page's marginals while open. A
/// claim opened inside a marginal is the marginal's own, and takes its ink.
#[test]
fn a_marginal_answers_only_to_a_claim_opened_inside_it() {
    let session = open(
        r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 40pt, numbering: "1",
  header: qm.region("classification")[PAGE HEADER])
#qm.region("subject")[#lorem(200)]
"#,
    );
    assert!(session.page_count() > 1, "the claim needs a page break");
    let regions = session.regions();
    let claimed = |field: &str| -> Vec<_> { regions.iter().filter(|r| r.field == field).collect() };
    let subject = claimed("subject");
    assert!(
        subject.len() > 1,
        "the claim surfaces on each page: {subject:?}"
    );
    for r in &subject {
        assert!(
            r.rect[1] >= 39.5 && r.rect[3] <= 160.5,
            "the claim stays inside the body area: {r:?}"
        );
    }
    let banners = claimed("classification");
    assert_eq!(
        banners.len(),
        session.page_count(),
        "one header claim per page: {banners:?}"
    );
    for r in &banners {
        assert!(
            r.rect[1] >= 160.0,
            "the header claim stays in the header: {r:?}"
        );
        let (cx, cy) = center(r);
        assert_eq!(
            session.field_at(r.page, cx, cy, 0.0).as_deref(),
            Some("classification"),
            "page {}'s header answers its own claim",
            r.page
        );
    }
    for page in 0..session.page_count() {
        assert_eq!(
            session.field_at(page, 150.0, 20.0, 10.0),
            None,
            "page {page}'s number resolves to nothing"
        );
    }
}

/// The claim's column is 100pt wide and every footnote and float spans the
/// page, so a claimed note or float widens the claim's box past the column.
/// The column is a table cell broken across pages, which Typst parents to the
/// cell as it parents a footnote to its anchor.
#[test]
fn a_footnote_or_float_answers_only_to_a_claim_opened_inside_it() {
    let session = open(
        r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt)
Lead#footnote[#qm.region("subject")[INSIDE] #lorem(30)]
#qm.region("classification")[#table(columns: (100pt,), inset: 0pt, stroke: none, lorem(150))]
#place(top, float: true, lorem(30))
"#,
    );
    let regions = session.regions();
    let column: Vec<_> = regions
        .iter()
        .filter(|r| r.field == "classification")
        .collect();
    assert!(
        column.len() > 2,
        "the claim surfaces on each page: {column:?}"
    );
    for r in &column {
        assert!(r.rect[2] <= 120.5, "the claim stays in its column: {r:?}");
    }
    assert!(
        regions.iter().any(|r| r.field == "subject"),
        "a claim inside a footnote takes the note"
    );
}
