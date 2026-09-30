
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
  fields:
    title:
      type: string
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
fn seed_main_commits_no_field_and_an_empty_body() {
    let quill = quill_from_yaml(QUILL);
    let card = quill.seed_main();

    assert!(
        card.payload().is_empty(),
        "a `default:` is interpolated at render, never seeded"
    );

    let reference = card.quill().expect("main card must carry $quill");
    assert_eq!(reference.name, "seed_test");
    assert_eq!(
        card.kind(),
        Some("main"),
        "main card must carry $kind: main"
    );

    assert_eq!(card.body_markdown(), "");
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
card_kinds:
  data:
    body:
      enabled: false
    fields:
      value:
        type: string
"#,
    );

    let ov = overlay(json!({ "$body": "Overlay body." }));
    let card = quill.seed_card("data", Some(&ov)).expect("known kind");
    assert_eq!(
        card.body_markdown(),
        "",
        "body must be empty when body.enabled is false"
    );

    let doc = Document::parse(
        "~~~\n$quill: bodyless@1.0\n$kind: main\n$seed:\n  data:\n    $body: Overlay body.\n~~~\n",
    )
    .expect("doc should parse")
    .document;
    let diags = quill.validate(&doc);
    assert!(
        diags.iter().any(|d| d.path.as_deref() == Some("$seed.data.$body")
            && d.code.as_deref() == Some("validation::seed_unknown_field")),
        "{diags:?}"
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
        ("$seed:\n  note:\n    $body: 3\n", "$seed.note.$body", "validation::seed_overlay_shape"),
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

const KIND_SEED_QUILL: &str = r#"
quill:
  name: kind_seed
  version: "1.0"
  backend: typst
  description: Kind seed test
card_kinds:
  note:
    seed:
      author: Kind Author
      tag: kind
      $body: Kind body.
    fields:
      author:
        type: string
      tag:
        type: string
      level:
        type: integer
"#;

/// A document's overlay replaces the kind's seed whole, `$body` included, a
/// null in it reading as absent; the kind's seed applies where the document
/// carries no overlay, and an empty overlay seeds nothing.
#[test]
fn a_document_overlay_replaces_the_kind_seed_whole() {
    let quill = quill_from_yaml(KIND_SEED_QUILL);
    let field = |card: &crate::document::Card, name: &str| card.payload().get(name).cloned();

    let bare = quill.seed_card("note", None).expect("known kind");
    assert_eq!(field(&bare, "author").as_ref().and_then(|v| v.as_str()), Some("Kind Author"));
    assert_eq!(field(&bare, "tag").as_ref().and_then(|v| v.as_str()), Some("kind"));
    assert!(field(&bare, "level").is_none());
    assert_eq!(bare.body_markdown(), "Kind body.");

    let ov = overlay(json!({ "author": "Doc Author", "tag": null, "level": 2 }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    assert_eq!(field(&card, "author").as_ref().and_then(|v| v.as_str()), Some("Doc Author"));
    assert!(field(&card, "tag").is_none());
    assert_eq!(field(&card, "level").as_ref().and_then(|v| v.as_json().as_i64()), Some(2));
    assert_eq!(card.body_markdown(), "");

    let doc = Document::parse(
        "~~~\n$quill: kind_seed@1.0\n$kind: main\n$seed:\n  note: {}\n~~~\n",
    )
    .expect("doc should parse")
    .document;
    let empty = doc
        .main()
        .seed()
        .and_then(|seed| seed.get("note"))
        .and_then(SeedOverlay::from_json)
        .expect("an empty overlay is an overlay");
    let card = quill.seed_card("note", Some(&empty)).expect("known kind");
    for name in ["author", "tag", "level"] {
        assert!(field(&card, name).is_none(), "{name}");
    }
    assert_eq!(card.body_markdown(), "");

    let doc = quill.seed_document();
    assert_eq!(doc.cards()[0], bare, "the seeded document carries each kind's seed");

    assert_eq!(
        quill.config().schema()["card_kinds"]["note"]["seed"],
        json!({ "author": "Kind Author", "tag": "kind", "$body": "Kind body." })
    );
}

/// A kind's `seed:` is checked by the walker a document's `$seed` is, each
/// warning there a load error here, plus the refusal of a seed the unanswered
/// field already renders.
#[test]
fn a_defective_kind_seed_fails_the_load() {
    let quill_yaml = |main: &str, seed: &str| {
        format!(
            r#"
quill: {{ name: bad_seed, version: "1.0", backend: typst, description: x }}
main:
  fields:
    title: {{ type: string }}
{main}
card_kinds:
  note:
    body: {{ enabled: false }}
    seed: {seed}
    fields:
      author: {{ type: string, default: Anon }}
      level: {{ type: number, default: 1 }}
      names: {{ type: array, max: 1, items: {{ type: string }} }}
      office: {{ type: object, properties: {{ symbol: {{ type: string }} }} }}
      mark:
        type: enum
        values: [U, CUI]
        default: U
        variants:
          CUI: {{ note: {{ type: string }} }}
      broken: {{ type: strin }}
"#
        )
    };
    for (main, seed, code) in [
        ("", "{ bogus: 1 }", "quill::seed_unknown_field"),
        ("", "{ level: high }", "quill::seed_type_mismatch"),
        ("", "{ $body: Text }", "quill::seed_unknown_field"),
        ("", "{ names: [a, b] }", "quill::seed_cardinality"),
        ("", "{ office: { symbol: X, bogus: 1 } }", "quill::seed_unknown_field"),
        ("", "{ mark: { value: U, note: stranded } }", "quill::seed_out_of_variant"),
        ("", "[a]", "quill::seed_overlay_shape"),
        ("", "{ author: Anon }", "quill::seed_redundant"),
        ("", "{ level: 1.0 }", "quill::seed_redundant"),
        ("", "{ mark: { value: U } }", "quill::seed_redundant"),
        ("", "{ names: [] }", "quill::seed_redundant"),
        ("", "{ names: null }", "quill::seed_redundant"),
        ("", "{}", "quill::seed_redundant"),
        ("  seed: { title: T }", "{ names: [a] }", "quill::invalid_card_schema"),
    ] {
        let errors = crate::quill::QuillConfig::from_yaml_with_warnings(&quill_yaml(main, seed))
            .expect_err(seed);
        assert!(
            errors.iter().any(|d| d.code.as_deref() == Some(code)
                && d.severity == Severity::Error),
            "{seed}: expected {code}, got {errors:?}"
        );
        assert!(
            !errors.iter().any(|d| d.code.as_deref() == Some("quill::seed_unknown_field")
                && d.message.contains("broken")),
            "{seed}: a field that failed to parse draws no seed error: {errors:?}"
        );
    }
}

/// A kind seed reaching into a typed dictionary and a variant world makes a
/// card that validates clean and is already at rest; a blank overriding a
/// non-blank `default:` still means something, and loads.
#[test]
fn a_seeded_document_validates_clean_and_conforms_as_a_no_op() {
    let quill = quill_from_yaml(
        r#"
quill: { name: deep_seed, version: "1.0", backend: typst, description: x }
card_kinds:
  entry:
    seed:
      office: { symbol: 49 FW/CC }
      mark: { value: CUI, note: "*Handle* with care." }
      urgent: false
      $body: Write the entry here.
    fields:
      office: { type: object, properties: { symbol: { type: string } } }
      mark:
        type: enum
        values: [U, CUI]
        default: U
        variants:
          CUI: { note: { type: richtext } }
      urgent: { type: boolean, default: true }
"#,
    );
    let doc = quill.seed_document();
    assert!(quill.validate(&doc).is_empty(), "{:?}", quill.validate(&doc));
    let mut conformed = doc.clone();
    quill.conform(&mut conformed).expect("conforms");
    assert_eq!(conformed, doc);
}


/// `quill::seed_redundant` asks the render floor: a seed is refused only where
/// it resolves as the absent field does, on every render date.
#[test]
fn a_kind_seed_is_redundant_only_where_the_floor_renders_it() {
    let quill_yaml = |seed: &str| {
        format!(
            r#"
quill: {{ name: floor_seed, version: "1.0", backend: typst, description: x }}
card_kinds:
  entry:
    seed: {seed}
    fields:
      office:
        type: object
        properties: {{ symbol: {{ type: string, default: 49 FW }}, room: {{ type: string }} }}
      note: {{ type: richtext, default: "*Handle* with care." }}
      due: {{ type: date, default: today }}
"#
        )
    };
    let load = |seed: &str| crate::quill::QuillConfig::from_yaml_with_warnings(&quill_yaml(seed));
    for seed in [
        "{ office: {} }",
        "{ office: { symbol: 49 FW } }",
        "{ office: { room: \"\" } }",
        "{ note: \"*Handle* with care.\" }",
        "{ due: today }",
        "{ $body: \"  \" }",
    ] {
        let errors = load(seed).expect_err(seed);
        assert!(
            errors.iter().any(|d| d.code.as_deref() == Some("quill::seed_redundant")),
            "{seed}: {errors:?}"
        );
    }
    for seed in [
        "{ office: { symbol: \"\", room: \"\" } }",
        "{ note: Handle with care. }",
        "{ due: 2000-01-01 }",
    ] {
        assert!(load(seed).is_ok(), "{seed}: {:?}", load(seed).err());
    }
}
