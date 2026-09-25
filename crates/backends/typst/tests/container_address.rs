//! The container property step (`classification.poc`, `address.city`) through
//! the public `Backend`/`LiveSession` path, and the address a region carries for
//! a plate that reads one property.
//!
//! The addresses the grammar admits are stated once, against both backends and
//! all three helpers, in `quillmark/tests/address_grammar.rs`; which read a
//! window anchors on is stated once at `overlay::span_scan`. What is here is
//! what only a whole session answers: the region a read surfaces and the click
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
      type: string
      description: a scalar, which offers no step at all
    balance:
      type: integer
      description: a negative number, which a bare `#` embed will not lex
    address:
      type: object
      description: a typed dictionary
      properties:
        city:
          type: string
        street:
          type: string
    tags:
      type: array
      description: a primitive list, whose element offers no further step
      items:
        type: string
    refs:
      type: array
      description: a typed table
      items:
        type: object
        properties:
          org:
            type: string
          num:
            type: string
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      default: ""
      description: a variant container
      variants:
        CUI:
          poc:
            type: string
          controlled_by:
            type: string
          note:
            type: richtext
          reply_by:
            type: date
"#;

fn data() -> serde_json::Value {
    serde_json::json!({
        "subject": "Widgets",
        "balance": -12,
        "address": { "city": "Dayton", "street": "1864 Fourth St" },
        "classification": {
            "value": "CUI",
            "poc": "Capt J. Smith",
            "controlled_by": "SAF/AA",
            "note": common::content("Handle per **DoDM 5200.48**"),
            "reply_by": "2026-03-04",
        },
        "tags": ["urgent"],
        "refs": [{ "org": "AFRL/RQ", "num": "2026-01" }],
    })
}

fn open(plate: &str) -> quillmark_core::session::LiveSession {
    TypstBackend
        .open(&common::quill_with_plate(YAML, plate), &data(), common::test_date())
        .expect("open")
}

/// A read, a bound alias, a widget and a claim each region on the address the
/// plate stepped to, and a click there routes to it rather than to the
/// container holding it. The container read whole still regions on the
/// container: the step is what narrows it.
#[test]
fn every_read_regions_and_routes_on_the_address_it_names() {
    let cases: [(&str, &[&str], &[&str]); 6] = [
        (
            "#data.classification.poc\n#data.address.city",
            &["classification.poc", "address.city"],
            &["classification", "address"],
        ),
        (
            "#let c = data.classification\n#let a = data.at(\"address\", default: (:))\n\
             #c.poc #c.at(\"controlled_by\") #a.city",
            &["classification.poc", "classification.controlled_by", "address.city"],
            &["classification", "address"],
        ),
        ("#data.refs.at(0).org", &["refs.0.org"], &["refs"]),
        (
            "#data.classification.value\n#data.address.len()",
            &["classification.value", "address"],
            &[],
        ),
        (
            "#form-field(\"Poc\", type: \"text\", value: data.classification.poc, field: \"classification.poc\")\n\
             #field-region(\"address.city\")[#box(stroke: 1pt, inset: 4pt)[#upper(data.address.city)]]\n\
             #field-region(\"refs.0.org\")[AFRL/RQ]\n\
             #form-field(\"Ref0Num\", field: \"refs.0.num\", value: data.refs.at(0).num)",
            &["classification.poc", "address.city", "refs.0.org", "refs.0.num"],
            &[],
        ),
        // A variant cell of a rich type lowers exactly as a card-level one: the
        // content as a markup block, the date as a native `datetime`.
        (
            "#data.classification.note\n\
             #assert(data.classification.reply_by.year() == 2026)\n\
             #display(\"classification.reply_by\", \"[year]\")",
            &["classification.note", "classification.reply_by"],
            &[],
        ),
    ];
    for (reads, surfaced, unclaimed) in cases {
        let session = open(&format!(
            "#import \"@local/quillmark-helper:0.1.0\": data, display, field-region, form-field\n\
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

/// A field printed through its dictionary's `ink` keeps its address past each
/// shape a direct read loses it to: a function parameter, a loop variable over
/// filtered rows, a destructuring, and a date printed or formatted inside a
/// helper. A row's `$path` addresses the ink it composes.
#[test]
fn ink_keeps_the_address_through_functions_loops_and_patterns() {
    let session = open(
        "#import \"@local/quillmark-helper:0.1.0\": data, display, field-region, ink\n\
         #set page(width: 400pt, height: 200pt, margin: 40pt)\n\
         #let shout(c) = upper(c)\n\
         #shout(ink(data).subject)\n\
         #ink(data).balance\n\
         #for r in data.refs.filter(r => r.org != \"\") [#ink(r).org / \
           #field-region(r.at(\"$path\") + \"num\")[No. #r.num.len()]]\n\
         #let (poc, ..rest) = ink(data.classification)\n\
         #poc\n\
         #let due(c) = display(c, \"reply_by\", \"[year]\")\n\
         #due(data.classification)\n\
         #ink(data).tags.join(\", \")\n",
    );
    let regions = session.regions();
    for field in [
        "subject",
        "balance",
        "refs.0.org",
        "refs.0.num",
        "classification.poc",
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
        "#import \"@local/quillmark-helper:0.1.0\": data, ink\n\
         #let stamp(c) = ink(c).reply_by\n\
         #stamp(data.classification)\n",
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
            type: string
"#;
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 200pt, margin: 40pt)
#for card in data.at("$cards") {
  field-region(card.at("$path") + "origin.office")[#card.origin.office]
}
"#;
    let session = TypstBackend
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
#import "@local/quillmark-helper:0.1.0": display
#set page(width: 400pt, height: 200pt, margin: 40pt)
#assert(display("classification.reply_by", "[year]") == none)
"#;
    let mut blank = data();
    blank["classification"]["reply_by"] = serde_json::Value::String(String::new());
    TypstBackend
        .open(&common::quill_with_plate(YAML, plate), &blank, common::test_date())
        .expect("a blank date compiles");
}
