//! Region coverage on the flagship `usaf_memo` quill, whose package rebuilds
//! body paragraphs through a state buffer (AFH 33-337 auto-numbering): span
//! tracking rides the rebuilt glyphs' own origins, so bodies stay addressable
//! with no recovery step in the plate.

#![cfg(feature = "typst")]

use std::collections::HashSet;

use quillmark::{OutputFormat, RenderOptions};

mod common;

#[test]
fn usaf_memo_regions_cover_body_signature_and_cards() {
    // One card per declared kind, so the indorsement addresses are present.
    let (engine, quill, parsed) = common::seeded_memo();

    let mut session = engine
        .open(quill, &parsed)
        .expect("usaf_memo should open a session");

    let regions = session.regions();
    let fields: HashSet<&str> = regions.iter().map(|r| r.field.as_str()).collect();

    for expected in [
        "$body",
        "signature_block",
        "$cards.indorsement.0.signature_block",
    ] {
        assert!(
            fields.contains(expected),
            "expected a region keyed {expected:?}; got {fields:?}"
        );
    }
    assert!(
        !fields.contains("$cards.indorsement.0.$body"),
        "an empty card body draws nothing and surfaces no region: {fields:?}"
    );

    // Plate space keys an array element `<field>.<i>`; a binding hands out the
    // bracketed `DocPath` index instead.
    assert!(
        fields.contains("references.0"),
        "each `references` element regions on its own address: {fields:?}"
    );
    let translated: HashSet<String> =
        quillmark_core::regions_to_doc_path(regions.clone(), &parsed.card_kinds())
            .into_iter()
            .map(|r| r.field)
            .collect();
    assert!(
        translated.contains("main.references[0]"),
        "the element address crosses as a bracketed DocPath index: {translated:?}"
    );

    let body = regions
        .iter()
        .find(|r| r.field == "$body")
        .expect("$body region present");
    let cx = (body.rect[0] + body.rect[2]) / 2.0;
    let cy = (body.rect[1] + body.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(body.page, cx, cy, 0.0).as_deref(),
        Some("$body"),
        "a click inside the rebuilt body routes to $body"
    );

    let mut edited = parsed.clone();
    quillmark::TypedWriter::new(quill.config(), &mut edited)
        .card(0)
        .expect("the indorsement card")
        .revise_body("The indorsement **body**, rebuilt by render-body.")
        .expect("set the card body");
    session.update(&edited).expect("update with an edited card body");
    let fields: HashSet<String> = session.regions().into_iter().map(|r| r.field).collect();
    assert!(
        fields.contains("$cards.indorsement.0.$body"),
        "a non-empty card body regions through the rebuild: {fields:?}"
    );

    let with_regions = engine
        .render(
            quill,
            &parsed,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf).with_regions(true),
        )
        .expect("usaf_memo should render to PDF");
    assert_eq!(
        with_regions.regions, regions,
        "one-shot sidecar matches the session query"
    );

    let without_regions = engine
        .render(
            quill,
            &parsed,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("usaf_memo should render to PDF");
    assert!(
        without_regions.regions.is_empty(),
        "the sidecar is opt-in; exports carry no regions by default"
    );
}

/// The laundered shape: the vendored package, not the plate, inks the date
/// deep inside `utils.typ`'s `display-date`. The plate passes the field's
/// *content projection* rather than its value, and that ink is born in the
/// generated helper, so its glyphs carry a helper span resolving to the
/// recorded window wherever the package finally places them.
#[test]
fn usaf_memo_date_region_rides_the_vendored_display() {
    let (engine, quill, parsed) = common::seeded_memo();
    let mut session = engine.open(quill, &parsed).expect("open a session");

    // The seed leaves the date blank: a native `today()` fallback inks no field
    // region, so commit a real date first.
    let mut edited = parsed.clone();
    quillmark::TypedWriter::new(quill.config(), &mut edited)
        .set("date", "2026-01-02")
        .expect("set a real date");
    session.update(&edited).expect("update with a real memo date");

    let regions = session.regions();
    let date = regions
        .iter()
        .find(|r| r.field == "date")
        .unwrap_or_else(|| panic!("a real memo date must surface a `date` region: {regions:?}"));
    assert!(
        date.rect[2] > date.rect[0] && date.rect[3] > date.rect[1],
        "the date region has positive area: {:?}",
        date.rect
    );
    let cx = (date.rect[0] + date.rect[2]) / 2.0;
    let cy = (date.rect[1] + date.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(date.page, cx, cy, 0.0).as_deref(),
        Some("date"),
        "a click on the vendored-placed memo date routes to its schema path"
    );
}

/// An indorsement whose date is blank draws nothing at all, so the widget seated
/// in its reserved space is the only thing carrying that address. Without it the
/// endorser's date is the one memo field a preview cannot route a click to.
#[test]
fn a_blank_indorsement_date_regions_through_its_fill_in_widget() {
    // The seed leaves the indorsement date blank, which is the fill-in case.
    let (engine, quill, parsed) = common::seeded_memo();
    let session = engine.open(quill, &parsed).expect("open a session");

    let regions = session.regions();
    let date = regions
        .iter()
        .find(|r| r.field == "$cards.indorsement.0.date")
        .unwrap_or_else(|| panic!("the blank date must surface a region: {regions:?}"));
    assert!(
        date.span.is_none(),
        "a widget region carries no content span: {date:?}"
    );
    let cx = (date.rect[0] + date.rect[2]) / 2.0;
    let cy = (date.rect[1] + date.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(date.page, cx, cy, 0.0).as_deref(),
        Some("$cards.indorsement.0.date"),
        "a click on the fill-in widget routes to the card's date"
    );

    let pdf = engine
        .render(
            quill,
            &parsed,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("render to PDF");
    let bytes = &pdf.artifacts[0].bytes;
    assert!(
        bytes
            .windows(b"Ind_0_Date".len())
            .any(|w| w == b"Ind_0_Date"),
        "the same span is a typeable AcroForm text field in the PDF"
    );
}
