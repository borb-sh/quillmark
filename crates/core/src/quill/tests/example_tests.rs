//! `example:` and `body.example`: checked at load as `default:` is, and
//! committed by `example_document`, which a root `example.md` never feeds.
use super::*;
use serde_json::json;

const SCHEMA: &str = r#"
main:
  body:
    description: What the memo says.
    example: "Filled-in *prose*."
  fields:
    subject: { type: text, example: Budget review }
    count: { type: integer, default: 3, example: 7 }
    note: { type: text }
    contact:
      type: object
      properties:
        city: { type: text, example: Springfield }
        zip: { type: text }
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: text, example: ACME }
          role: { type: text }
    rows:
      type: array
      example: [{ org: Initech }]
      items:
        type: object
        properties:
          org: { type: text, example: ACME }
    tags: { type: array, items: { type: text, example: urgent } }
    quals:
      type: matrix
      members: { cc: CC, ops: Ops }
      properties:
        detail: { type: text, example: Earned 2024 }
    marking:
      type: enum
      values: [PUBLIC, CUI]
      example: CUI
      variants:
        CUI:
          poc: { type: text, example: J. Doe }
card_kinds:
  item:
    seed:
      title: TITLE
      owner: OWNER
    fields:
      title: { type: text, example: First milestone }
      owner: { type: text }
"#;

fn load(sections: &str, extra: &[(&str, &[u8])]) -> Quill {
    let yaml = with_header(sections);
    let mut entries = vec![("Quill.yaml", yaml.as_bytes())];
    entries.extend_from_slice(extra);
    quill_from(&entries)
}

fn field(card: &crate::document::Card, name: &str) -> serde_json::Value {
    card.payload()
        .get(name)
        .map(|v| v.as_json().clone())
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn the_example_document_commits_every_declared_example() {
    let quill = load(SCHEMA, &[]);
    let doc = quill.example_document().expect("examples are declared");

    assert_eq!(doc.quill_reference().to_string(), "q@1.0");
    let main = doc.main();
    assert_eq!(field(main, "subject"), json!("Budget review"));
    assert_eq!(field(main, "count"), json!(7));
    assert_eq!(field(main, "note"), json!(null));
    assert_eq!(field(main, "contact"), json!({ "city": "Springfield" }));
    assert_eq!(field(main, "refs"), json!([{ "org": "ACME" }]));
    assert_eq!(field(main, "rows"), json!([{ "org": "Initech" }]));
    assert_eq!(field(main, "tags"), json!(["urgent"]));
    assert_eq!(
        field(main, "quals"),
        json!({ "cc": { "detail": "Earned 2024" } })
    );
    assert_eq!(
        field(main, "marking"),
        json!({ "value": "CUI", "poc": "J. Doe" })
    );
    assert_eq!(
        quillmark_content::lines::export::to_markdown(main.body()),
        "Filled-in *prose*."
    );

    let [card] = doc.cards() else {
        panic!("one card per kind")
    };
    assert_eq!(field(card, "title"), json!("First milestone"));
    assert_eq!(
        field(card, "owner"),
        json!("OWNER"),
        "a seed no example names stays"
    );

    assert!(
        quill.validate(&doc).is_empty(),
        "{:?}",
        quill.validate(&doc)
    );
    let mut conformed = doc.clone();
    quill.conform(&mut conformed).expect("conforms");
    assert_eq!(conformed, doc, "the example is at rest");
}

/// A world opens on the example's member, else the default's; the blank opens
/// none.
#[test]
fn a_variant_world_commits_under_the_member_that_opens_it() {
    let world = |discriminant: &str| {
        load(
            &format!(
                "main:\n  fields:\n    marking:\n      type: enum\n      values: [PUBLIC, CUI]\n\
                 {discriminant}      variants:\n        CUI:\n          poc: {{ type: text, example: J. Doe }}\n"
            ),
            &[],
        )
        .example_document()
        .map(|doc| field(doc.main(), "marking"))
    };
    assert_eq!(
        world("      default: CUI\n"),
        Some(json!({ "poc": "J. Doe" }))
    );
    assert_eq!(world(""), None);
}

/// A root `example.md` is an ordinary file: a quill declaring no example has
/// none.
#[test]
fn a_quill_declaring_no_example_has_no_example_document() {
    let quill = load(
        "main:\n  fields:\n    subject: { type: text, default: Hello }\n",
        &[(
            "example.md",
            b"~~~\n$quill: q\nsubject: Budget review\n~~~\n",
        )],
    );
    assert!(quill.example_document().is_none());
}

#[test]
fn an_example_is_checked_at_load_as_a_default_is() {
    for (field_yaml, code) in [
        (
            "    n: { type: integer, example: many }\n",
            "quill::example_type_mismatch",
        ),
        (
            "    e: { type: enum, values: [a, b], example: c }\n",
            "quill::example_not_in_enum",
        ),
        (
            "    d: { type: date, example: 2026-13-40 }\n",
            "quill::example_format_violation",
        ),
        (
            "    a: { type: array, max: 1, items: { type: text }, example: [x, y] }\n",
            "quill::example_over_max",
        ),
        (
            "    t: { type: text, max: 3, example: long }\n",
            "quill::example_over_length",
        ),
        (
            "    o:\n      type: object\n      example: { x: y }\n      properties:\n        x: { type: text }\n",
            "quill::example_on_namespace",
        ),
        (
            "    r: { type: richtext, multiline: false, example: \"# Heading\" }\n",
            "validation::not_single_line",
        ),
    ] {
        let errors = quill_with_field(field_yaml).expect_err(field_yaml);
        assert_eq!(codes(&errors), [code], "{field_yaml}");
    }
}

#[test]
fn a_body_example_and_description_are_checked_at_load() {
    for (body, code) in [
        (
            "    enabled: false\n    example: Prose.\n",
            "quill::invalid_body",
        ),
        (
            "    enabled: false\n    description: Prose.\n",
            "quill::invalid_body",
        ),
        (
            "    example: \"<div>x</div>\"\n",
            "quill::richtext_example_import",
        ),
    ] {
        let errors = config_with_sections(&format!("main:\n  body:\n{body}")).expect_err(body);
        assert_eq!(codes(&errors), [code], "{body}");
    }
}
