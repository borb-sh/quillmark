//! `qm.roster(dict, key)` through a real compile: the rows a matrix prints, from
//! data, a card and a table row, and the regions a plate claims with them.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;

const SCHEMA: &str = r#"
main:
  fields:
    quals:
      type: matrix
      open: true
      members: { dco: DCO, cyber: Cyber }
      properties:
        detail: { type: text }
    later:
      type: matrix?
      members: { dco: DCO }
    rows:
      type: array
      items:
        type: object
        properties:
          quals: { type: matrix, members: { dco: DCO, cyber: Cyber } }
card_kinds:
  entry:
    fields:
      quals: { type: matrix, members: { dco: DCO, cyber: Cyber } }
"#;

/// Plate data as `compile_data` composes it: a matrix holds its held members
/// alone, in roster order, then its added items sorted.
fn data() -> serde_json::Value {
    serde_json::json!({
        "quals": {
            "dco": { "detail": common::content("Ops") },
            "aide": { "title": common::content("Aide"), "detail": common::content("") },
            "wing_ig": { "title": common::content("Wing IG"), "detail": common::content("81 TRW") },
        },
        "later": null,
        "rows": [{ "quals": { "cyber": {} } }],
        "$cards": [{ "$kind": "entry", "quals": { "dco": {} } }],
    })
}

fn open(body: &str) -> Result<quillmark_core::session::LiveSession, String> {
    let plate = format!(
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n\
         #set page(width: 400pt, height: 400pt, margin: 20pt)\n{body}\n"
    );
    TypstBackend::default()
        .open(
            &common::quill_with_plate(&common::yaml(SCHEMA), &plate),
            &data(),
            common::test_date(),
        )
        .map_err(|e| format!("{e:?}"))
}

/// Every roster member in roster order, held or not, then each added item in
/// id order: a held row carries its columns and an unheld one `none`. A matrix
/// is found the same way from data, a card and a table row, and an unanswered
/// optional one holds nothing.
#[test]
fn roster_rows_are_the_vocabulary_in_order_from_every_container() {
    open(
        r#"#let rows = qm.roster(qm.data, "quals")
#assert.eq(rows.map(r => r.id), ("dco", "cyber", "aide", "wing_ig"))
#assert.eq(rows.map(r => r.held), (true, false, true, true))
#assert.eq(rows.map(r => qm.str(r.title)), ("DCO", "Cyber", "Aide", "Wing IG"))
#assert.eq(rows.map(r => r.path), ("quals.dco", "quals.cyber", "quals.aide", "quals.wing_ig"))
#assert.eq(rows.at(1).value, none)
#assert.eq(qm.str(rows.at(3).value.title), "Wing IG")
#assert.eq(qm.roster(qm.data.rows.at(0), "quals").map(r => r.held), (false, true))
#assert.eq(qm.roster(qm.data.rows.at(0), "quals").at(1).path, "rows.0.quals.cyber")
#let card = qm.data.at("$cards").at(0)
#assert.eq(qm.roster(card, "quals").map(r => r.path), ("$cards.entry.0.quals.dco", "$cards.entry.0.quals.cyber"))
#assert.eq(qm.roster(qm.data, "later").map(r => r.held), (false,))"#,
    )
    .expect("every assertion holds");

    let refused = open(r#"#qm.roster(qm.data, "rows")"#).err().expect("a table is no matrix");
    assert!(refused.contains("is not a matrix field address"), "{refused}");
}

/// A row's `path` is a claimable address whether or not the row is held, and
/// an added item's title keeps the address of its own cell inside the claim.
#[test]
fn a_rows_path_claims_its_region() {
    let session = open(
        r#"#for row in qm.roster(qm.data, "quals") [
  #qm.region(row.path)[#(if row.held [x] else [o]) #(if row.held and "title" in row.value {
    qm.ink(row.value, "title")
  } else { row.title })]
]"#,
    )
    .expect("opens");
    let fields: Vec<String> = session.regions().into_iter().map(|r| r.field).collect();
    for field in ["quals.dco", "quals.cyber", "quals.aide", "quals.wing_ig", "quals.wing_ig.title"] {
        assert!(fields.iter().any(|f| f == field), "{field} in {fields:?}");
    }
}

/// `qm.ink` reads a member's cells totally: a blank one is `none`, a roster
/// member's `title` is no address, and a display pattern belongs to a date.
#[test]
fn ink_answers_a_blank_cell_and_refuses_what_no_address_carries() {
    open(r#"#assert.eq(qm.ink(qm.data.quals.aide, "detail"), none)"#).expect("a blank cell is none");
    for (body, refusal) in [
        (r#"#qm.ink(qm.data.quals.dco, "title")"#, "not a schema field address"),
        (r#"#qm.ink(qm.data.quals.dco, "detail", "[year]")"#, "display pattern"),
    ] {
        let err = open(body).err().unwrap_or_else(|| panic!("{body} must not compile"));
        assert!(err.contains(refusal), "{body}: {err}");
    }
}
