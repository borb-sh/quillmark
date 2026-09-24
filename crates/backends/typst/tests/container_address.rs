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
        .open(&common::quill_with_plate(YAML, plate), &data(), None)
        .expect("open")
}

/// A property read anchors on the property, so the region names the cell the
/// plate read rather than the container holding it.
#[test]
fn a_property_read_regions_on_the_property() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 200pt, margin: 40pt)
#data.classification.poc
#data.address.city
"#;
    let session = open(plate);
    let regions = session.regions();
    for field in ["classification.poc", "address.city"] {
        assert!(
            regions.iter().any(|r| r.field == field),
            "{field:?} regions on its own address: {regions:?}"
        );
    }
    assert!(
        !regions.iter().any(|r| r.field == "classification"),
        "the container does not also claim the property's ink: {regions:?}"
    );

    let poc = regions
        .iter()
        .find(|r| r.field == "classification.poc")
        .expect("the property region surfaces");
    let (cx, cy) = (
        (poc.rect[0] + poc.rect[2]) / 2.0,
        (poc.rect[1] + poc.rect[3]) / 2.0,
    );
    assert_eq!(
        session.field_at(poc.page, cx, cy, 0.0).as_deref(),
        Some("classification.poc"),
        "a click on the cell routes to the cell, not the container"
    );
}

/// A bound read surfaces the region the direct chain surfaces, so naming a
/// container before stepping into it costs no address.
#[test]
fn a_property_read_through_a_let_alias_keeps_the_property_address() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 200pt, margin: 40pt)
#let c = data.classification
#let a = data.at("address", default: (:))
#c.poc #c.at("controlled_by") #a.city
"#;
    let session = open(plate);
    let regions = session.regions();
    for field in ["classification.poc", "classification.controlled_by", "address.city"] {
        assert!(
            regions.iter().any(|r| r.field == field),
            "{field:?} regions through the alias: {regions:?}"
        );
    }

    let poc = regions
        .iter()
        .find(|r| r.field == "classification.poc")
        .expect("the property region surfaces");
    let (cx, cy) = (
        (poc.rect[0] + poc.rect[2]) / 2.0,
        (poc.rect[1] + poc.rect[3]) / 2.0,
    );
    assert_eq!(
        session.field_at(poc.page, cx, cy, 0.0).as_deref(),
        Some("classification.poc"),
        "a click on a bound read routes to the cell it read"
    );
}

/// The container read whole is still one region: the step is what narrows it.
#[test]
fn the_container_read_whole_still_regions_on_the_container() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 200pt, margin: 40pt)
#data.classification.value
#repr(data.address)
"#;
    let regions = open(plate).regions();
    for field in ["classification.value", "address"] {
        assert!(
            regions.iter().any(|r| r.field == field),
            "{field:?} regions: {regions:?}"
        );
    }
}

/// The whole point of the tables: a widget and a claim can now name a cell.
#[test]
fn a_widget_and_a_claim_bind_a_container_property() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region, form-field
#set page(width: 400pt, height: 200pt, margin: 40pt)
#form-field("Poc", type: "text", value: data.classification.poc, field: "classification.poc")
#field-region("address.city")[#box(stroke: 1pt, inset: 4pt)[#upper(data.address.city)]]
"#;
    let regions = open(plate).regions();
    for field in ["classification.poc", "address.city"] {
        assert!(
            regions.iter().any(|r| r.field == field),
            "{field:?} surfaces: {regions:?}"
        );
    }
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
            None,
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

/// A typed table's row property claims ink of its own, through an explicit
/// claim and through a widget alike.
#[test]
fn a_typed_table_row_property_is_addressable() {
    let session = open(
        r#"
#import "@local/quillmark-helper:0.1.0": data, field-region, form-field
#field-region("refs.0.org")[AFRL/RQ]
#form-field("Ref0Num", field: "refs.0.num", value: data.refs.at(0).num)
"#,
    );
    let fields: Vec<String> = session.regions().into_iter().map(|r| r.field).collect();
    assert!(
        fields.iter().any(|f| f == "refs.0.org"),
        "an explicit claim regions on the row property: {fields:?}"
    );
    assert!(
        fields.iter().any(|f| f == "refs.0.num"),
        "a widget binds the same grammar: {fields:?}"
    );
}

/// The address the scan reports must be the one the lowering and `_qm-known-path`
/// already agree on: anchoring on the array routes a click on the org cell to the
/// whole table.
#[test]
fn a_direct_row_cell_read_anchors_on_the_row_property() {
    let session = open(
        r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 200pt, margin: 40pt)
#data.refs.at(0).org
"#,
    );
    let regions = session.regions();
    let fields: Vec<&str> = regions.iter().map(|r| r.field.as_str()).collect();
    assert!(fields.contains(&"refs.0.org"), "{fields:?}");
    assert!(
        !fields.contains(&"refs"),
        "the table does not also claim the cell's ink: {fields:?}"
    );

    let cell = regions
        .iter()
        .find(|r| r.field == "refs.0.org")
        .expect("the row property region surfaces");
    let (cx, cy) = (
        (cell.rect[0] + cell.rect[2]) / 2.0,
        (cell.rect[1] + cell.rect[3]) / 2.0,
    );
    assert_eq!(
        session.field_at(cell.page, cx, cy, 0.0).as_deref(),
        Some("refs.0.org"),
        "a click on the cell routes to the cell, not the table"
    );
}

/// A variant cell of a rich type lowers exactly as a card-level one does: the
/// container projects as `type: object` carrying `properties`, so the walk
/// recurses into it without knowing what a variant is.
#[test]
fn a_variant_cell_lowers_its_declared_type() {
    let session = open(
        r#"
#import "@local/quillmark-helper:0.1.0": data, display
#set page(width: 612pt, height: 792pt, margin: 72pt)
// A markup block, not the canonical-content wire JSON a raw dict carries.
#data.classification.note
// Native `datetime`: the component read would not compile against a string.
#assert(data.classification.reply_by.year() == 2026)
#display("classification.reply_by", "[year]")
"#,
    );
    let fields: Vec<String> = session.regions().into_iter().map(|r| r.field).collect();
    assert!(
        fields.iter().any(|f| f == "classification.note"),
        "the cell's content regions on its own address: {fields:?}"
    );
    assert!(
        fields.iter().any(|f| f == "classification.reply_by"),
        "the cell's date projection regions too: {fields:?}"
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
        .open(&common::quill_with_plate(YAML, plate), &blank, None)
        .expect("a blank date compiles");
}
