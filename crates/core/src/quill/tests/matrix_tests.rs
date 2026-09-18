//! `type: matrix`: a closed vocabulary someone ticks.
//!
//! A matrix is sugar over a typed dictionary, so the risk is not that the
//! inherited walks break but that the three things the type *adds* disagree
//! with them: the presence-implies-held spelling, the roster written onto the
//! wire, and the wire closed over an unheld member. Each test below pins one
//! surface to the same reading of the same schema.

use crate::document::Document;
use crate::quill::{blank, build_transform_schema, quill_from_yaml, Quill, QuillConfig};
use serde_json::json;

/// A five-member roster across three blocks, the last ungrouped, with one
/// column. `detail` carries a `default:`, so nothing inside a held member is
/// obliged unless a test says so.
fn quill_yaml() -> &'static str {
    r#"
quill:
  name: matrix_probe
  version: "0.1.0"
  backend: typst
  description: Matrix probe

typst:
  plate_file: plate.typ

main:
  fields:
    qualifications:
      type: matrix
      members:
        - group: Leadership
          values:
            sq_cc_candidate: Sq/CC Candidate
            flight_cc: Flight CC
        - group: Operations
          values:
            dodin_ops: DODIN Ops
        - values:
            cyber_200: Cyber 200
      properties:
        detail: { type: plaintext, inline: true, default: "" }
    title:
      type: string
      default: ""
"#
}

fn config() -> QuillConfig {
    QuillConfig::from_yaml(quill_yaml()).expect("matrix probe loads")
}

fn quill() -> Quill {
    quill_from_yaml(quill_yaml())
}

fn doc(fields: &str) -> Document {
    let markdown = format!("~~~\n$quill: matrix_probe@0.1.0\n$kind: main\n{fields}~~~\n");
    Document::parse(&markdown).expect("document parses").document
}

/// The matrix as the plate receives it.
fn plate(document: &Document) -> serde_json::Value {
    config()
        .compile_data(document)
        .expect("compile_data succeeds")["qualifications"]
        .clone()
}

fn codes(document: &Document) -> Vec<(String, String)> {
    quill()
        .validate(document)
        .into_iter()
        .map(|d| (d.code.unwrap_or_default(), d.path.unwrap_or_default()))
        .collect()
}

fn load_error(fields: &str) -> String {
    let yaml = format!(
        r#"
quill:
  name: bad
  version: "0.1.0"
  backend: typst
  description: bad

typst:
  plate_file: plate.typ

main:
  fields:
{fields}
"#
    );
    let err = QuillConfig::from_yaml(&yaml).expect_err("expected a load error");
    format!("{err:?}")
}

/// The stored-spelling table: key presence is the tick unless the mapping
/// spells otherwise, and every spelling conforms to the member object.
#[test]
fn every_stored_spelling_conforms_to_the_member_object() {
    let held = plate(&doc(
        "qualifications:\n  flight_cc: true\n  dodin_ops: { detail: X }\n",
    ));

    // Absent: not held.
    assert_eq!(held["sq_cc_candidate"]["held"], json!(false));
    // Bare `true`: held, columns at their blanks.
    assert_eq!(held["flight_cc"]["held"], json!(true));
    // A mapping naming no `held`: held, with its columns.
    assert_eq!(held["dodin_ops"]["held"], json!(true));
    assert_eq!(held["dodin_ops"]["detail"]["text"], json!("X"));
}

/// An unticked member's retained answer is a fact about the stored form alone:
/// the document keeps it, so tick-type-untick-retick loses nothing, and the
/// wire carries the live world only.
#[test]
fn an_unheld_member_keeps_its_detail_in_the_document_and_blanks_it_on_the_wire() {
    let document = doc("qualifications:\n  flight_cc: { held: false, detail: kept }\n");

    let stored = document.main().payload().get("qualifications").unwrap();
    assert_eq!(
        stored.as_json()["flight_cc"]["detail"],
        json!("kept"),
        "the document retains what the author typed"
    );

    let wire = plate(&document);
    assert_eq!(wire["flight_cc"]["held"], json!(false));
    assert_eq!(
        wire["flight_cc"]["detail"]["text"],
        json!(""),
        "an unheld member's columns render at their blanks"
    );
}

/// Total at the plate, in declaration order, carrying the labels the roster
/// holds: a plate prints the whole vocabulary without a second copy of it.
#[test]
fn the_projection_is_total_and_carries_the_roster() {
    let wire = plate(&doc("qualifications:\n  cyber_200: true\n"));
    let members = wire.as_object().expect("a matrix projects as a mapping");

    assert_eq!(
        members.keys().collect::<Vec<_>>(),
        ["sq_cc_candidate", "flight_cc", "dodin_ops", "cyber_200"],
        "every member present, in declaration order"
    );
    assert_eq!(wire["flight_cc"]["title"], json!("Flight CC"));
    assert_eq!(wire["flight_cc"]["group"], json!("Leadership"));
    assert_eq!(
        wire["cyber_200"]["group"],
        json!(""),
        "an ungrouped member carries the blank heading, so a plate reads it unguarded"
    );
}

/// The roster is what a document may name, as an enum's `values:` is.
#[test]
fn a_member_outside_the_roster_is_refused() {
    let found = codes(&doc("qualifications:\n  ghost: true\n"));
    assert!(
        found.contains(&(
            "validation::enum_violation".to_string(),
            "main.qualifications.ghost".to_string()
        )),
        "expected a closed-domain violation, got {found:?}"
    );
}

/// The title is the projection's, not the document's: writing one is overwritten
/// rather than carried, so a plate's label cannot be forged from a document.
#[test]
fn an_authored_title_does_not_reach_the_wire() {
    let wire = plate(&doc(
        "qualifications:\n  flight_cc: { held: true, title: Forged }\n",
    ));
    assert_eq!(wire["flight_cc"]["title"], json!("Flight CC"));
}

/// An absent matrix blank-fills to every member unheld, so the plate needs no
/// guard for a document that never mentioned it.
#[test]
fn an_absent_matrix_blank_fills_to_every_member_unheld() {
    let wire = plate(&doc("title: T\n"));
    for id in ["sq_cc_candidate", "flight_cc", "dodin_ops", "cyber_200"] {
        assert_eq!(wire[id]["held"], json!(false), "{id} blanks unheld");
    }

    let field = &config().main.fields["qualifications"];
    assert_eq!(
        blank(field).as_json()["flight_cc"]["held"],
        json!(false),
        "the floor agrees with the projection"
    );
}

/// The schema fixes the keys, so the matrix holds no literal of its own — the
/// existing namespace rule, no exception.
#[test]
fn a_literal_on_a_matrix_is_refused_as_a_namespace_literal() {
    for slot in ["default", "example"] {
        let err = load_error(&format!(
            "    m:\n      type: matrix\n      {slot}: {{}}\n      \
             members:\n        - values: {{ a: A }}\n"
        ));
        assert!(
            err.contains(&format!("quill::{slot}_on_namespace")),
            "expected quill::{slot}_on_namespace, got {err}"
        );
    }
}

/// A member id is what the wire, the address and the document speak, so it
/// carries the same discipline as a field key.
#[test]
fn a_member_id_that_is_not_an_identifier_is_a_load_error() {
    let err = load_error(
        "    m:\n      type: matrix\n      members:\n        - values: { \"DO / Det CC\": Label }\n",
    );
    assert!(err.contains("quill::invalid_matrix_member"), "{err}");
}

/// A mapping has one slot per key, so a duplicate is unspellable in a document;
/// the roster is a sequence of blocks and can spell one, which the loader refuses.
#[test]
fn a_member_declared_twice_is_a_load_error() {
    let err = load_error(
        "    m:\n      type: matrix\n      members:\n        - values: { a: A }\n        \
         - values: { a: Again }\n",
    );
    assert!(err.contains("quill::duplicate_matrix_member"), "{err}");
}

#[test]
fn the_synthesized_tick_cannot_be_declared_as_a_column() {
    let err = load_error(
        "    m:\n      type: matrix\n      members:\n        - values: { a: A }\n      \
         properties:\n        held: { type: string }\n",
    );
    assert!(err.contains("quill::matrix_reserved_column"), "{err}");
}

#[test]
fn a_matrix_without_a_roster_is_a_load_error() {
    assert!(load_error("    m:\n      type: matrix\n").contains("quill::field_parse_error"));
}

#[test]
fn a_roster_off_a_matrix_is_a_load_error() {
    let err = load_error(
        "    m:\n      type: string\n      members:\n        - values: { a: A }\n",
    );
    assert!(err.contains("quill::field_parse_error"), "{err}");
}

/// Obligation is per column inside a *held* member, the variant rule one level
/// down: an unticked member asks for nothing, and the matrix itself obliges
/// nothing.
#[test]
fn a_defaultless_column_is_obliged_only_inside_a_held_member() {
    let yaml = quill_yaml().replace(
        "detail: { type: plaintext, inline: true, default: \"\" }",
        "detail: { type: plaintext, inline: true }",
    );
    let quill = quill_from_yaml(&yaml);
    let obliged = |fields: &str| -> Vec<String> {
        let markdown = format!("~~~\n$quill: matrix_probe@0.1.0\n$kind: main\n{fields}~~~\n");
        let document = Document::parse(&markdown).expect("parses").document;
        quill
            .validate(&document)
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("validation::must_fill"))
            .map(|d| d.path.unwrap_or_default())
            .collect()
    };

    assert!(
        obliged("title: T\n").is_empty(),
        "an absent matrix obliges nothing"
    );
    assert!(
        obliged("qualifications:\n  flight_cc: { held: false }\n").is_empty(),
        "an unticked member obliges nothing"
    );
    assert_eq!(
        obliged("qualifications:\n  flight_cc: true\n"),
        ["main.qualifications.flight_cc.detail"],
        "a held member obliges its defaultless columns"
    );
}

/// A namespace holds no literal, and a column's `example:` documents one cell's
/// shape rather than which members a fresh document ticks.
#[test]
fn a_matrix_seeds_empty() {
    let yaml = quill_yaml().replace(
        "detail: { type: plaintext, inline: true, default: \"\" }",
        "detail: { type: plaintext, inline: true, default: \"\", example: Earned 2024 }",
    );
    let seeded = quill_from_yaml(&yaml).seed_document();
    assert!(
        seeded.main().payload().get("qualifications").is_none(),
        "a seeded document ticks nothing"
    );
}

/// A member's cells are ordinary addresses: a region on the Typst backend, a
/// widget on acroform. The transform schema is where both resolve one.
#[test]
fn a_members_cells_are_addressable() {
    let schema = build_transform_schema(&config());
    let member = &schema.as_json()["properties"]["qualifications"]["properties"]["flight_cc"];

    assert_eq!(member["properties"]["held"]["type"], json!("boolean"));
    assert!(
        member["properties"]["detail"]["contentMediaType"].is_string(),
        "a column crosses as the content it is: {member}"
    );
    assert!(
        member["properties"].get("title").is_none(),
        "`title` is written by the projection, not held as a cell, so it carries no address"
    );
}

/// The roster rides the format slot as an enum's domain does, and the body
/// carries the sparse spelling: no third annotation form, and nothing for a
/// model to delete.
#[test]
fn the_blueprint_shows_the_vocabulary_in_the_annotation_and_ticks_nothing() {
    let bp = config().blueprint();
    assert!(
        bp.contains(
            "qualifications: {} # matrix<sq_cc_candidate | flight_cc | dodin_ops | cyber_200>\n"
        ),
        "{bp}"
    );
}
