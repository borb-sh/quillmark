
use serde_json::json;

use crate::quill::{quill_from_yaml, test_date};
use crate::{document::{Document, SeedOverlay}, error::Severity};

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
fn empty_document_equals_the_hand_written_two_line_document() {
    let quill = quill_from_yaml(QUILL);
    let config = quill.config();
    let markdown = format!(
        "~~~\n$quill: {}@{}\n$kind: main\n~~~\n",
        config.name, config.version
    );

    let parsed = Document::parse(&markdown)
        .expect("the two-line empty document must parse")
        .document;

    assert_eq!(
        quill.empty_document(),
        parsed,
        "the constructor must spell the document the authoring contract names"
    );
}

#[test]
fn seed_main_commits_neither_an_example_nor_a_field() {
    let quill = quill_from_yaml(QUILL);
    let card = quill.seed_main();

    assert!(
        card.payload().is_empty(),
        "an `example:` never answers a field, and a `default:` is interpolated at render"
    );

    let reference = card.quill().expect("main card must carry $quill");
    assert_eq!(reference.name, "seed_test");
    assert_eq!(
        card.kind(),
        Some("main"),
        "main card must carry $kind: main"
    );

    assert_eq!(
        card.body_markdown(),
        "",
        "`body.example` is guide text, never a seeded body"
    );
}

#[test]
fn seed_document_emits_one_seeded_card_per_kind() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    assert_eq!(doc.main(), &quill.seed_main());
    assert_eq!(doc.cards().len(), 1);
    let note = &doc.cards()[0];
    assert_eq!(note.kind(), Some("note"));
    assert!(
        note.quill().is_none(),
        "composable card must not carry $quill"
    );
    assert!(note.payload().is_empty());
    assert!(quill.seed_card("missing", None).is_none());

    let reparsed = Document::parse(&doc.to_markdown())
        .expect("seeded document must re-parse from its own markdown")
        .document;
    assert_eq!(reparsed, doc);
}

#[test]
fn seeded_document_compiles_with_default_then_blank_for_absent_fields() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    let data = quill
        .compile_data(&doc, test_date())
        .expect("seeded document must compile");

    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some(""));
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("draft"));
    assert_eq!(data.get("notes").and_then(|v| v.as_str()), Some(""));
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
    let ov = overlay(json!({ "beta": "B", "alpha": "A" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    let keys: Vec<&str> = card.payload().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["alpha", "beta"]);
}

#[test]
fn overlay_fills_fields_and_body_and_ignores_non_schema_keys() {
    let quill = quill_from_yaml(QUILL);
    let ov = overlay(json!({ "tag": "pinned", "$body": "Overlay body.", "bogus": "drop me" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    assert_eq!(card.payload().get("tag").and_then(|v| v.as_str()), Some("pinned"));
    assert!(card.payload().get("author").is_none());
    assert_eq!(card.body_markdown(), "Overlay body.");
    assert!(card.payload().get("bogus").is_none());
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

    let ov = overlay(json!({ "$body": "Overlay body." }));
    let card = quill.seed_card("data", Some(&ov)).expect("known kind");
    assert_eq!(
        card.body_markdown(),
        "",
        "body must be empty when body.enabled is false"
    );
}

fn doc_with_seed(seed_block: &str) -> Document {
    let md = format!("~~~card-yaml\n$quill: seed_test@1.0\n$kind: main\n{seed_block}~~~\n");
    Document::parse(&md).expect("doc should parse").document
}

/// A seed overlay is advisory: a defect is a warning at its `$seed` path and
/// never gates render, and a well-formed or present-null cell draws nothing.
#[test]
fn seed_overlay_diagnostics_are_advisory_and_do_not_gate_render() {
    let quill = quill_from_yaml(QUILL);
    for (seed, path, code) in [
        ("$seed:\n  note:\n    author: { given: A }\n", "$seed.note.author", "validation::type_mismatch"),
        ("$seed:\n  bogus_kind:\n    x: 1\n", "$seed.bogus_kind", "validation::seed_unknown_kind"),
    ] {
        let doc = doc_with_seed(seed);
        let diags = quill.validate(&doc);
        let d = diags
            .iter()
            .find(|d| d.path.as_deref() == Some(path))
            .unwrap_or_else(|| panic!("no diagnostic at {path}: {diags:?}"));
        assert_eq!(d.code.as_deref(), Some(code));
        assert_eq!(d.severity, Severity::Warning);
        assert!(quill.compile_data(&doc, test_date()).is_ok(), "{path}");
        assert!(quill.dry_run(&doc).is_ok(), "{path}");
    }

    for seed in ["$seed:\n  note:\n    author: Custom\n", "$seed:\n  note:\n    author: null\n"] {
        let diags = quill.validate(&doc_with_seed(seed));
        assert!(
            !diags.iter().any(|d| d.path.as_deref().is_some_and(|p| p.starts_with("$seed"))),
            "{seed}: {diags:?}"
        );
    }
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
