//! The container property step (`classification.poc`, `address.city`) through
//! the public `Backend`/`LiveSession` path, and the address a region carries for
//! a plate that prints one property.
//!
//! The addresses the grammar admits are stated once, against both backends and
//! all three helpers, in `quillmark/tests/address_grammar.rs`. What is here is
//! what only a whole session answers: the region a print surfaces and the click
//! it takes.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;

const YAML: &str = r#"
quill:
  name: container_address
  version: 0.1.0
  backend: typst
  description: container property addressing
typst:
  plate_file: plate.typ
main:
  fields:
    subject:
      type: text
      description: a scalar, which offers no step at all
    balance:
      type: integer
      description: a negative number, which a bare `#` embed will not lex
    address:
      type: object
      description: a typed dictionary
      properties:
        city:
          type: text
        street:
          type: text
    tags:
      type: array
      description: a primitive list, whose element offers no further step
      items:
        type: text
    refs:
      type: array
      description: a typed table
      items:
        type: object
        properties:
          org:
            type: text
          num:
            type: text
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      default: ""
      description: a variant container
      variants:
        CUI:
          poc:
            type: text
          controlled_by:
            type: text
          note:
            type: richtext
            multiline: true
          reply_by:
            type: date
"#;

fn data() -> serde_json::Value {
    let text = common::content;
    serde_json::json!({
        "subject": text("Widgets"),
        "balance": -12,
        "address": { "city": text("Dayton"), "street": text("1864 Fourth St") },
        "classification": {
            "value": "CUI",
            "poc": text("Capt J. Smith"),
            "controlled_by": text("SAF/AA"),
            "note": common::content("Handle per **DoDM 5200.48**"),
            "reply_by": "2026-03-04",
        },
        "tags": [text("urgent")],
        "refs": [{ "org": text("AFRL/RQ"), "num": text("2026-01") }],
    })
}

fn open(plate: &str) -> quillmark_core::session::LiveSession {
    TypstBackend::default()
        .open(&common::quill_with_plate(YAML, plate), &data(), common::test_date())
        .expect("open")
}

/// An ink print, one through a bound dictionary, a widget and a claim each
/// region on the address the plate named, and a click there routes to it
/// rather than to the container holding it. A container carries no ink of its
/// own, so naming it whole prints nothing and regions nothing.
#[test]
fn every_print_regions_and_routes_on_the_address_it_names() {
    let cases: [(&str, &[&str], &[&str]); 6] = [
        (
            "#qm.ink(qm.data, \"classification.poc\")\n#qm.ink(qm.data, \"address.city\")",
            &["classification.poc", "address.city"],
            &["classification", "address"],
        ),
        (
            "#let c = qm.data.classification\n#let a = qm.data.at(\"address\", default: (:))\n\
             #qm.ink(c, \"poc\") #qm.ink(c, \"controlled_by\") #qm.ink(a, \"city\")",
            &["classification.poc", "classification.controlled_by", "address.city"],
            &["classification", "address"],
        ),
        ("#qm.ink(qm.data.refs.at(0), \"org\")", &["refs.0.org"], &["refs"]),
        (
            "#qm.ink(qm.data, \"classification.value\")\n\
             #assert.eq(qm.ink(qm.data, \"address\"), none)",
            &["classification.value"],
            &["address"],
        ),
        (
            "#qm.form-field(\"Poc\", type: \"text\", value: qm.str(qm.data.classification.poc), field: \"classification.poc\")\n\
             #qm.region(\"address.city\")[#box(stroke: 1pt, inset: 4pt)[#upper(qm.data.address.city)]]\n\
             #qm.region(\"refs.0.org\")[AFRL/RQ]\n\
             #qm.form-field(\"Ref0Num\", field: \"refs.0.num\", value: qm.str(qm.data.refs.at(0).num))",
            &["classification.poc", "address.city", "refs.0.org", "refs.0.num"],
            &[],
        ),
        // A variant cell of a rich type lowers exactly as a card-level one: the
        // content as a markup block, the date as a native `datetime`.
        (
            "#qm.data.classification.note\n\
             #assert(qm.data.classification.reply_by.year() == 2026)\n\
             #qm.ink(qm.data, \"classification.reply_by\", \"[year]\")",
            &["classification.note", "classification.reply_by"],
            &[],
        ),
    ];
    for (reads, surfaced, unclaimed) in cases {
        let session = open(&format!(
            "#import \"@local/quillmark-helper:0.2.0\" as qm\n\
             #set page(width: 400pt, height: 200pt, margin: 40pt)\n{reads}\n"
        ));
        let regions = session.regions();
        for field in surfaced {
            let r = regions
                .iter()
                .find(|r| r.field == *field)
                .unwrap_or_else(|| panic!("{field:?} regions on its own address: {regions:?}"));
            let (cx, cy) = ((r.rect[0] + r.rect[2]) / 2.0, (r.rect[1] + r.rect[3]) / 2.0);
            assert_eq!(
                session.field_at(r.page, cx, cy, 0.0).as_deref(),
                Some(*field),
                "a click on {field:?} routes to it"
            );
        }
        for field in unclaimed {
            assert!(
                !regions.iter().any(|r| r.field == *field),
                "the container {field:?} does not also claim the cell's ink: {regions:?}"
            );
        }
    }
}

/// A field printed through `qm.ink` keeps its address past a function
/// parameter, a loop variable over filtered rows, and a date printed or
/// formatted inside a helper. A row's `$path` addresses the ink it composes.
#[test]
fn ink_keeps_the_address_through_functions_loops_and_patterns() {
    let session = open(
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n\
         #set page(width: 400pt, height: 200pt, margin: 40pt)\n\
         #let shout(c) = upper(c)\n\
         #shout(qm.ink(qm.data, \"subject\"))\n\
         #qm.ink(qm.data, \"balance\")\n\
         #for r in qm.data.refs.filter(r => r.org != \"\") [#qm.ink(r, \"org\") / \
           #qm.region(r.at(\"$path\") + \"num\")[No. #qm.str(r.num).len()]]\n\
         #let due(c) = qm.ink(c, \"reply_by\", \"[year]\")\n\
         #due(qm.data.classification)\n\
         #qm.ink(qm.data, \"tags\").join(\", \")\n",
    );
    let regions = session.regions();
    for field in [
        "subject",
        "balance",
        "refs.0.org",
        "refs.0.num",
        "classification.reply_by",
        "tags.0",
    ] {
        let r = regions
            .iter()
            .find(|r| r.field == field)
            .unwrap_or_else(|| panic!("{field:?} regions through its ink: {regions:?}"));
        let (cx, cy) = ((r.rect[0] + r.rect[2]) / 2.0, (r.rect[1] + r.rect[3]) / 2.0);
        assert_eq!(session.field_at(r.page, cx, cy, 0.0).as_deref(), Some(field));
    }

    // A date's ink prints its default display, laundered like any other.
    let session = open(
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n\
         #let stamp(c) = qm.ink(c, \"reply_by\")\n\
         #stamp(qm.data.classification)\n",
    );
    let regions = session.regions();
    let r = regions
        .iter()
        .find(|r| r.field == "classification.reply_by")
        .unwrap_or_else(|| panic!("a date's ink regions: {regions:?}"));
    let (cx, cy) = ((r.rect[0] + r.rect[2]) / 2.0, (r.rect[1] + r.rect[3]) / 2.0);
    assert_eq!(
        session.field_at(r.page, cx, cy, 0.0).as_deref(),
        Some("classification.reply_by")
    );
}

/// A card's container fields ride the same table, keyed through the card's
/// `$path` prefix.
#[test]
fn a_card_container_property_is_addressable() {
    const CARD_YAML: &str = r#"
quill:
  name: card_container_address
  version: 0.1.0
  backend: typst
  description: card container addressing
typst:
  plate_file: plate.typ
main:
  body:
    enabled: false
card_kinds:
  endorsement:
    fields:
      origin:
        type: object
        properties:
          office:
            type: text
"#;
    let plate = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 200pt, margin: 40pt)
#for card in qm.data.at("$cards") {
  qm.region(card.at("$path") + "origin.office")[#card.origin.office]
}
"#;
    let session = TypstBackend::default()
        .open(
            &common::quill_with_plate(CARD_YAML, plate),
            &serde_json::json!({
                "$cards": [ { "$kind": "endorsement", "origin": { "office": "SAF/AA" } } ]
            }),
            common::test_date(),
        )
        .expect("open");
    let regions = session.regions();
    assert!(
        regions
            .iter()
            .any(|r| r.field == "$cards.endorsement.0.origin.office"),
        "the card's container property claims its ink: {regions:?}"
    );
}

/// The assert answers about the address, the `none` about the value: a declared
/// date left blank keeps the documented fallback rather than becoming an error.
#[test]
fn a_blank_date_still_projects_none() {
    let plate = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 200pt, margin: 40pt)
#assert(qm.ink(qm.data, "classification.reply_by", "[year]") == none)
"#;
    let mut blank = data();
    blank["classification"]["reply_by"] = serde_json::Value::String(String::new());
    TypstBackend::default()
        .open(&common::quill_with_plate(YAML, plate), &blank, common::test_date())
        .expect("a blank date compiles");
}

/// Spreading a typed dictionary into a function passes its `$path` as a named
/// argument; the failure hints the sink, and the sink compiles.
#[test]
fn spreading_a_typed_dictionary_hints_its_path_key() {
    let plate = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#let line(city: none, street: none) = [#street, #city]
#line(..qm.data.address)
"#;
    let quill = common::quill_with_plate(YAML, plate);
    let diags = match TypstBackend::default().open(&quill, &data(), common::test_date()) {
        Ok(_) => panic!("the spread must fail"),
        Err(e) => e.into_diagnostics(),
    };
    let diag = diags
        .iter()
        .find(|d| d.message.contains("$path"))
        .unwrap_or_else(|| panic!("an unexpected `$path`: {diags:?}"));
    let hint = diag.hint.as_deref().expect("a hint");
    assert!(hint.contains("..rest"), "{hint}");

    let sunk = plate.replace("street: none)", "street: none, ..rest)");
    TypstBackend::default()
        .open(&common::quill_with_plate(YAML, &sunk), &data(), common::test_date())
        .expect("the hinted sink takes `$path`");
}
