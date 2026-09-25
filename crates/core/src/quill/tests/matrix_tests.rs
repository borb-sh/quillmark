//! `type: matrix`: a closed vocabulary someone ticks.
//!
//! A matrix is sugar over a typed dictionary, so the risk is not that the
//! inherited walks break but that the three things the type *adds* disagree
//! with them: the roster written onto the wire, and the wire closed over an
//! unheld member. The bare-scalar tick is the
//! variant precedent, inherited rather than added. Each test below pins one
//! surface to the same reading of the same schema.

use crate::document::Document;
use crate::quill::{
    blank, build_transform_schema, quill_from_yaml, test_date, Quill, QuillConfig,
};
use serde_json::json;

/// A four-member roster with one column.
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
        sq_cc_candidate: Sq/CC Candidate
        flight_cc: Flight CC
        dodin_ops: DODIN Ops
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
        .compile_data(document, test_date())
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
        "quill:\n  name: bad\n  version: \"0.1.0\"\n  backend: typst\n  description: bad\n\
         main:\n  fields:\n{fields}\n"
    );
    format!("{:?}", QuillConfig::from_yaml(&yaml).expect_err("expected a load error"))
}

/// The stored-spelling table: a bare scalar is the tick, and every spelling
/// conforms to the member object.
#[test]
fn every_stored_spelling_conforms_to_the_member_object() {
    let held = plate(&doc(
        "qualifications:\n  flight_cc: true\n  dodin_ops: { held: true, detail: X }\n",
    ));

    // Absent: not held.
    assert_eq!(held["sq_cc_candidate"]["held"], json!(false));
    // Bare `true`: held, columns at their blanks.
    assert_eq!(held["flight_cc"]["held"], json!(true));
    // A mapping spelling the tick: held, with its columns.
    assert_eq!(held["dodin_ops"]["held"], json!(true));
    assert_eq!(held["dodin_ops"]["detail"]["text"], json!("X"));
}

/// A mapping naming no `held` takes the ordinary ladder to unheld; the empty
/// mapping is what an editor opening a row writes first. An unheld member's
/// columns are retained in the document, so tick-type-untick-retick loses
/// nothing, and blank on the wire, which carries the live world only.
#[test]
fn an_unheld_member_keeps_its_detail_in_the_document_and_blanks_it_on_the_wire() {
    let document = doc(concat!(
        "qualifications:\n",
        "  flight_cc: { detail: X }\n",
        "  dodin_ops: {}\n",
        "  cyber_200: { held: false, detail: kept }\n",
    ));
    let wire = plate(&document);
    let stored = document.main().payload().get("qualifications").unwrap().as_json().clone();
    for id in ["flight_cc", "dodin_ops", "cyber_200"] {
        assert_eq!(wire[id]["held"], json!(false), "{id}");
    }
    for (id, detail) in [("flight_cc", "X"), ("cyber_200", "kept")] {
        assert_eq!(wire[id]["detail"]["text"], json!(""), "{id}");
        assert_eq!(stored[id]["detail"], json!(detail), "{id}");
    }
}

/// Total at the plate, in declaration order, carrying the labels the roster
/// holds: a plate prints the whole vocabulary without a second copy of it, and
/// an authored `title` is overwritten rather than carried.
#[test]
fn the_projection_is_total_and_carries_the_roster() {
    let wire = plate(&doc(
        "qualifications:\n  cyber_200: true\n  flight_cc: { held: true, title: Forged }\n",
    ));
    let members = wire.as_object().expect("a matrix projects as a mapping");

    assert_eq!(
        members.keys().collect::<Vec<_>>(),
        ["sq_cc_candidate", "flight_cc", "dodin_ops", "cyber_200"],
        "every member present, in declaration order"
    );
    assert_eq!(wire["flight_cc"]["title"], json!("Flight CC"));
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

/// The schema fixes the keys, so a matrix holds no literal; a member id carries
/// a field key's discipline; `held` and `title` are the keys the type writes
/// itself; and the roster and the type imply each other.
#[test]
fn a_malformed_matrix_declaration_is_refused_by_code() {
    for (fields, code) in [
        ("    m:\n      type: matrix\n      default: {}\n      members: { a: A }\n", "quill::default_on_namespace"),
        ("    m:\n      type: matrix\n      example: {}\n      members: { a: A }\n", "quill::example_on_namespace"),
        (
            "    m:\n      type: matrix\n      members: { \"DO / Det CC\": Label }\n",
            "quill::invalid_matrix_member",
        ),
        (
            "    m:\n      type: matrix\n      members: { a: A }\n      properties:\n        held: { type: string }\n",
            "quill::matrix_reserved_column",
        ),
        (
            "    m:\n      type: matrix\n      members: { a: A }\n      properties:\n        title: { type: string }\n",
            "quill::matrix_reserved_column",
        ),
        ("    m:\n      type: matrix\n", "quill::field_parse_error"),
        ("    m:\n      type: string\n      members: { a: A }\n", "quill::field_parse_error"),
    ] {
        assert!(load_error(fields).contains(code), "{code}: {fields}");
    }
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
/// model to delete. A checklist's bare tick is its whole spelling, so it takes
/// no hint line.
#[test]
fn the_blueprint_shows_the_vocabulary_in_the_annotation_and_ticks_nothing() {
    let checklist = quill_yaml().replace(
        "      properties:\n        detail: { type: plaintext, inline: true, default: \"\" }\n",
        "",
    );
    let bp = QuillConfig::from_yaml(&checklist).expect("loads").blueprint();
    assert!(
        bp.contains(concat!(
            "# Matrix probe\n",
            "qualifications: {} # matrix<sq_cc_candidate | flight_cc | dodin_ops | cyber_200>\n",
        )),
        "{bp}"
    );
}

/// A matrix declaring columns names them in its `# e.g.` line, spelled as a
/// held member so the tick a mapping needs is on the page, and a column with
/// nothing to show carrying its type. Pasted into the cell, the hint is a
/// member the schema accepts once that column is answered.
#[test]
fn the_blueprint_hint_spells_a_held_member_with_every_column() {
    let yaml = quill_yaml().replace(
        "detail: { type: plaintext, inline: true, default: \"\" }",
        "detail: { type: plaintext, inline: true, default: \"\", example: \"333 TRS/DO, 2024\" }\n        \
         unit: { type: string, default: HQ }\n        \
         earned: { type: date }",
    );
    let bp = QuillConfig::from_yaml(&yaml).expect("loads").blueprint();
    let hint = "{sq_cc_candidate: {held: true, detail: \"333 TRS/DO, 2024\", unit: HQ, earned: date<YYYY-MM-DD | today>}}";
    assert!(
        bp.contains(&format!(
            "# e.g. {hint}\nqualifications: {{}} # matrix<sq_cc_candidate | flight_cc | dodin_ops | cyber_200>\n"
        )),
        "{bp}"
    );

    let quill = quill_from_yaml(&yaml);
    let paste = |hint: &str| {
        let markdown =
            format!("~~~\n$quill: matrix_probe@0.1.0\n$kind: main\nqualifications: {hint}\n~~~\n");
        Document::parse(&markdown).expect("the hint parses").document
    };
    assert!(!quill.validate(&paste(hint)).is_empty());

    let answered = paste(&hint.replace("date<YYYY-MM-DD | today>", "2024-05-01"));
    let wire = quill.compile_data(&answered, test_date()).expect("compiles")["qualifications"]
        ["sq_cc_candidate"]
        .clone();
    assert_eq!(wire["held"], json!(true));
    assert_eq!(wire["detail"]["text"], json!("333 TRS/DO, 2024"));
    assert!(quill.validate(&answered).is_empty());
}

/// A matrix below a typed dictionary or a table row carries the same hint at
/// its own slot, and the blueprint still round-trips.
#[test]
fn a_nested_matrix_carries_its_hint_at_its_own_slot() {
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    record:
      type: object
      properties:
        quals:
          type: matrix
          members: { flight_cc: Flight CC }
          properties:
            detail: { type: string, example: Ops }
    rows:
      type: array
      items:
        type: object
        properties:
          quals:
            type: matrix
            members: { flight_cc: Flight CC }
            properties:
              detail: { type: string, example: Cyber }
"#;
    let bp = QuillConfig::from_yaml(yaml).expect("loads").blueprint();
    assert!(
        bp.contains(concat!(
            "record: # object\n",
            "  # e.g. {flight_cc: {held: true, detail: Ops}}\n",
            "  quals: {} # matrix<flight_cc>\n",
        )),
        "{bp}"
    );
    assert!(
        bp.contains(concat!(
            "rows: # array<object>\n",
            "  -\n",
            "    # e.g. {flight_cc: {held: true, detail: Cyber}}\n",
            "    quals: {} # matrix<flight_cc>\n",
        )),
        "{bp}"
    );

    let doc1 = Document::parse(&bp).expect("blueprint parses").document;
    let doc2 = Document::parse(&doc1.to_markdown())
        .expect("re-emit parses")
        .document;
    assert_eq!(doc1, doc2);
}

/// The tick is the render floor's boolean coercion, not raw truthiness:
/// `"false"` is the case that tells them apart.
#[test]
fn the_tick_is_judged_by_the_render_floor_not_by_raw_truthiness() {
    let yaml = quill_yaml().replace(
        "detail: { type: plaintext, inline: true, default: \"\" }",
        "detail: { type: plaintext, inline: true }",
    );
    let quill = quill_from_yaml(&yaml);
    let held_at = |fields: &str| -> bool {
        let markdown = format!("~~~\n$quill: matrix_probe@0.1.0\n$kind: main\n{fields}~~~\n");
        let document = Document::parse(&markdown).expect("parses").document;
        quill.compile_data(&document, test_date()).expect("compiles")["qualifications"]["flight_cc"]
            ["held"]
            .as_bool()
            .expect("the tick is a boolean on the wire")
    };

    for (spelling, held) in [
        ("title: T\n", false),
        ("qualifications:\n  flight_cc: { held: false }\n", false),
        ("qualifications:\n  flight_cc: {}\n", false),
        ("qualifications:\n  flight_cc: true\n", true),
        ("qualifications:\n  flight_cc: \"false\"\n", false),
        ("qualifications:\n  flight_cc: \"true\"\n", true),
        ("qualifications:\n  flight_cc: 0\n", false),
        ("qualifications:\n  flight_cc: 1\n", true),
    ] {
        assert_eq!(held_at(spelling), held, "wire disagrees on {spelling:?}");
    }
}

/// A member the floor refuses is the member's own failure. The container is not
/// mis-shaped, and a sibling spelled as the bare tick is not: both would
/// otherwise report `validation::type_mismatch` at a path the author cannot act
/// on.
#[test]
fn one_members_refusal_does_not_convict_the_matrix_or_its_siblings() {
    let found = codes(&doc(
        "qualifications:\n  flight_cc: true\n  dodin_ops: [1, 2]\n",
    ));
    let mismatches: Vec<&String> = found
        .iter()
        .filter(|(code, _)| code == "validation::type_mismatch")
        .map(|(_, path)| path)
        .collect();

    assert_eq!(
        mismatches,
        [&"main.qualifications.dodin_ops.held".to_string()],
        "only the refused member reports: {found:?}"
    );
}
