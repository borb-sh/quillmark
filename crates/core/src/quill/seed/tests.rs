
use serde_json::json;

use crate::quill::quill_from_yaml;
use crate::{Document, SeedOverlay, Severity};

fn overlay(value: serde_json::Value) -> SeedOverlay {
    SeedOverlay::from_json(&value).expect("overlay json must be an object")
}

const QUILL: &str = r#"
quill:
  name: seed_test
  version: "1.0"
  backend: typst
  description: Seed test
main:
  body:
    example: "Main body text."
  fields:
    title:
      type: string
      example: FIRSTNAME LASTNAME
    status:
      type: string
      default: draft
    notes:
      type: string
card_kinds:
  note:
    fields:
      author:
        type: string
        example: A. Author
      tag:
        type: string
"#;

#[test]
fn seed_main_commits_only_example_fields() {
    let quill = quill_from_yaml(QUILL);
    let card = quill.seed_main();
    let payload = card.payload();

    assert_eq!(
        payload.get("title").and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );
    assert!(
        payload.get("status").is_none(),
        "default-only field must be absent (interpolated at render)"
    );
    assert!(payload.get("notes").is_none());

    let reference = card.quill().expect("main card must carry $quill");
    assert_eq!(reference.name, "seed_test");
    assert_eq!(
        card.kind(),
        Some("main"),
        "main card must carry $kind: main"
    );

    assert_eq!(card.body_markdown(), "Main body text.");
}

#[test]
fn seeded_document_round_trips_through_markdown() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    let markdown = doc.to_markdown();
    let reparsed = crate::Document::parse(&markdown)
        .expect("seeded document must re-parse from its own markdown")
        .document;

    assert_eq!(
        reparsed.main().quill().map(|r| r.name.as_str()),
        Some("seed_test")
    );
    assert_eq!(reparsed.main().kind(), Some("main"));
    assert_eq!(
        reparsed
            .main()
            .payload()
            .get("title")
            .and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );
    assert_eq!(reparsed.main().body_markdown(), "Main body text.");

    assert_eq!(reparsed.cards().len(), 1);
    assert_eq!(reparsed.cards()[0].kind(), Some("note"));
    assert_eq!(
        reparsed.cards()[0]
            .payload()
            .get("author")
            .and_then(|v| v.as_str()),
        Some("A. Author"),
    );
}

#[test]
fn seed_document_emits_one_seeded_card_per_kind() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    assert_eq!(
        doc.main().payload().get("title").and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );

    assert_eq!(doc.cards().len(), 1);
    let note = &doc.cards()[0];
    assert_eq!(note.kind(), Some("note"));
    assert!(
        note.quill().is_none(),
        "composable card must not carry $quill"
    );
    assert_eq!(
        note.payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );
    assert!(note.payload().get("tag").is_none());
}

#[test]
fn seeded_document_compiles_with_default_then_blank_for_absent_fields() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    let data = quill
        .compile_data(&doc)
        .expect("seeded document must compile");

    assert_eq!(
        data.get("title").and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("draft"));
    assert_eq!(data.get("notes").and_then(|v| v.as_str()), Some(""));
}

#[test]
fn seed_card_for_known_and_unknown_kind() {
    let quill = quill_from_yaml(QUILL);

    let note = quill.seed_card("note", None).expect("known kind");
    assert_eq!(note.kind(), Some("note"));
    assert_eq!(
        note.payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );

    assert!(
        quill.seed_card("missing", None).is_none(),
        "unknown kind must return None"
    );
}

#[test]
fn overlay_adds_a_field_the_base_omits() {
    let quill = quill_from_yaml(QUILL);
    assert!(quill
        .seed_card("note", None)
        .unwrap()
        .payload()
        .get("tag")
        .is_none());
    let ov = overlay(json!({ "tag": "pinned" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    assert_eq!(
        card.payload().get("tag").and_then(|v| v.as_str()),
        Some("pinned")
    );
    assert_eq!(
        card.payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );
}

#[test]
fn overlay_added_field_lands_in_declaration_position() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: order_seed
  version: "1.0"
  backend: typst
  description: Seed order test
card_kinds:
  note:
    fields:
      alpha:
        type: string
      beta:
        type: string
        example: B
"#,
    );
    let ov = overlay(json!({ "alpha": "A" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    let keys: Vec<&str> = card.payload().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["alpha", "beta"]);
}

#[test]
fn overlay_body_overrides_and_non_schema_keys_are_ignored() {
    let quill = quill_from_yaml(QUILL);
    assert_eq!(quill.seed_card("note", None).unwrap().body_markdown(), "");
    let ov = overlay(json!({ "author": "X", "$body": "Overlay body.", "bogus": "drop me" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    assert_eq!(card.body_markdown(), "Overlay body.");
    assert!(
        card.payload().get("bogus").is_none(),
        "a key naming no schema field must not land on the card",
    );
}

#[test]
fn seed_omits_body_when_body_disabled() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: bodyless
  version: "1.0"
  backend: typst
  description: Bodyless card test
main:
  fields:
    title:
      type: string
      example: T
card_kinds:
  data:
    body:
      enabled: false
    fields:
      value:
        type: string
        example: V
"#,
    );

    let card = quill.seed_card("data", None).expect("known kind");
    assert_eq!(
        card.body_markdown(),
        "",
        "body must be empty when body.enabled is false"
    );
    assert_eq!(
        card.payload().get("value").and_then(|v| v.as_str()),
        Some("V"),
    );
}

fn doc_with_seed(seed_block: &str) -> Document {
    let md = format!("~~~card-yaml\n$quill: seed_test@1.0\n$kind: main\n{seed_block}~~~\n");
    Document::parse(&md).expect("doc should parse").document
}

#[test]
fn seed_overlay_type_mismatch_is_advisory_and_does_not_gate_render() {
    let quill = quill_from_yaml(QUILL);
    let doc = doc_with_seed("$seed:\n  note:\n    author: { given: A }\n");

    let diags = quill.validate(&doc);
    let seed_diag = diags
        .iter()
        .find(|d| d.path.as_deref() == Some("$seed.note.author"))
        .expect("a diagnostic rooted at the seed field");
    assert_eq!(
        seed_diag.code.as_deref(),
        Some("validation::type_mismatch"),
        "a shape no card could carry is still flagged: {seed_diag:?}",
    );
    assert_eq!(
        seed_diag.severity,
        Severity::Warning,
        "seed diagnostics are advisory, not errors",
    );

    assert!(
        quill.compile_data(&doc).is_ok(),
        "compile_data must ignore $seed"
    );
    assert!(quill.dry_run(&doc).is_ok(), "dry_run must ignore $seed");
}

#[test]
fn seed_overlay_unknown_kind_is_flagged_but_renders() {
    let quill = quill_from_yaml(QUILL);
    let doc = doc_with_seed("$seed:\n  bogus_kind:\n    x: 1\n");
    let diags = quill.validate(&doc);
    let d = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::seed_unknown_kind"))
        .expect("unknown-kind advisory");
    assert_eq!(d.path.as_deref(), Some("$seed.bogus_kind"));
    assert_eq!(d.severity, Severity::Warning);
    assert!(quill.compile_data(&doc).is_ok());
}

#[test]
fn well_formed_seed_overlay_yields_no_seed_diagnostics() {
    let quill = quill_from_yaml(QUILL);
    let doc = doc_with_seed("$seed:\n  note:\n    author: Custom\n");
    let diags = quill.validate(&doc);
    assert!(
        !diags
            .iter()
            .any(|d| d.path.as_deref().is_some_and(|p| p.starts_with("$seed"))),
        "a well-formed overlay should produce no seed diagnostics: {diags:?}",
    );
}

/// The container is the spelling `seed_variant` reads its discriminant off, so
/// the overlay the seeder accepts is the overlay the validator passes.
#[test]
fn a_variant_container_overlay_validates_clean_and_seeds() {
    const VARIANT_QUILL: &str = r#"
quill:
  name: seed_test
  version: "1.0"
  backend: typst
  description: Seed variant test
main:
  fields:
    title:
      type: string
card_kinds:
  entry:
    fields:
      classification:
        type: enum
        values: [UNCLASSIFIED, CUI]
        default: ""
        variants:
          CUI:
            note: { type: richtext }
"#;
    let quill = quill_from_yaml(VARIANT_QUILL);
    let doc = doc_with_seed(
        "$seed:\n  entry:\n    classification:\n      value: CUI\n      note: hello\n",
    );

    let diags = quill.validate(&doc);
    assert!(
        !diags
            .iter()
            .any(|d| d.path.as_deref().is_some_and(|p| p.starts_with("$seed"))),
        "a container overlay is a document value, not a schema literal: {diags:?}",
    );

    let overlay = overlay(json!({ "classification": { "value": "CUI", "note": "hello" } }));
    let card = quill
        .seed_card("entry", Some(&overlay))
        .expect("kind exists");
    assert_eq!(
        card.payload()
            .get("classification")
            .expect("seeded classification")
            .as_json()["value"],
        json!("CUI"),
    );
}

/// `value` stays absent — a `default:` is never persisted — and the container
/// that leaves it out is a valid card.
#[test]
fn a_variant_overlay_without_a_discriminant_commits_its_cells_under_the_default_world() {
    let quill = quill_from_yaml(
        r#"
quill: { name: seed_test, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string, default: "" }
card_kinds:
  entry:
    fields:
      classification:
        type: enum
        values: [UNCLASSIFIED, CUI]
        default: CUI
        variants:
          CUI:
            note: { type: richtext }
      other: { type: string }
"#,
    );
    let overlay = overlay(json!({ "classification": { "note": "hello" }, "other": "kept" }));
    let card = quill
        .seed_card("entry", Some(&overlay))
        .expect("kind exists");

    let classification = card
        .payload()
        .get("classification")
        .expect("an overlay cell commits without a discriminant to name its world")
        .as_json()
        .clone();
    assert!(
        classification.get("note").is_some(),
        "the cell the overlay supplied must reach the card: {classification}"
    );
    assert!(
        classification.get("value").is_none(),
        "a `default:` discriminant stays deferred to the render floor: {classification}"
    );
    assert_eq!(
        card.payload().get("other").and_then(|v| v.as_str()),
        Some("kept"),
        "the sibling field commits as it always did"
    );

    let doc = Document::from_main_and_cards(quill.seed_main(), vec![card]);
    let diags = quill.validate(&doc);
    assert!(diags.is_empty(), "a seeded card is a valid document: {diags:?}");
}

/// Null ≡ absent in an overlay cell as in any authored one: the field is simply
/// unanswered, and the seeded card blank-fills it at render.
#[test]
fn a_null_overlay_cell_draws_no_diagnostic() {
    let quill = quill_from_yaml(QUILL);
    let doc = doc_with_seed("$seed:\n  note:\n    author: null\n");
    let diags = quill.validate(&doc);
    assert!(
        !diags
            .iter()
            .any(|d| d.path.as_deref().is_some_and(|p| p.starts_with("$seed"))),
        "a present-null overlay cell is an absent one: {diags:?}",
    );
}

/// A typed dictionary's seed composes from the examples its properties declare,
/// and stays absent when none of them declare any.
#[test]
fn a_dictionarys_seed_is_composed_from_its_properties_examples() {
    let quill = quill_from_yaml(
        r#"
quill: { name: sd, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    contact:
      type: object
      properties:
        name: { type: string, example: Ada }
        email: { type: string, default: "hi@example.com" }
        note: { type: string }
    empty:
      type: object
      properties:
        tag: { type: string, default: t }
"#,
    );
    let seeded = quill.seed_document();
    let payload = seeded.main().payload();

    assert_eq!(
        payload.get("contact").map(|v| v.as_json().clone()),
        Some(serde_json::json!({ "name": "Ada" })),
        "the commit is sparse: only the cells with an example, the rest deferred"
    );
    assert!(
        payload.get("empty").is_none(),
        "a dictionary whose cells commit nothing stays absent, as any field does"
    );
    // The marker rides a committed example on a must-fill cell, at its own path.
    assert!(
        payload
            .get("contact")
            .expect("contact seeded")
            .nonroot_fill_paths()
            .any(|p| p == vec![crate::value::PathSegment::Key("name".to_string())]),
        "a must-fill cell's marker rides at the cell, not the container"
    );
}
