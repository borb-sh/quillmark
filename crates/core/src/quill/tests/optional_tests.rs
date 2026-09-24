//! `type: <t>?`: an unanswered cell renders `none` rather than its type's blank.
//!
//! The `?` changes the render floor only. Obligation still keys on `default:`,
//! which a `?` refuses, so every surface below reads the same cell two ways:
//! the plate sees `none`, and `validate` still asks for an answer.

use crate::document::Document;
use crate::quill::{build_transform_schema, quill_from_yaml, FieldSource, QuillConfig};
use serde_json::json;

const QUILL_YAML: &str = r#"
quill:
  name: optional_probe
  version: "0.1.0"
  backend: typst
  description: Optional probe

main:
  fields:
    quorum: { type: integer? }
    confidential: { type: boolean? }
    location: { type: string? }
    attendees: { type: array?, items: { type: string } }
    result: { type: enum?, values: [carried, failed] }
    adjourned: { type: date? }
    minutes: { type: richtext? }
    tally:
      type: object
      properties:
        votes_for: { type: integer? }
        votes_against: { type: integer }
"#;

fn document(fields: &str) -> Document {
    let markdown = format!("~~~\n$quill: optional_probe@0.1.0\n$kind: main\n{fields}~~~\n");
    Document::parse(&markdown).expect("parses").document
}

/// `0` authored stays `0`, which is the distinction the `?` exists to draw; an
/// authored `""` is an answer on a `string?` but the blank's own spelling on an
/// `enum?`.
#[test]
fn an_unanswered_optional_cell_renders_none_at_every_depth() {
    let quill = quill_from_yaml(QUILL_YAML);
    let doc = document("quorum: 0\nlocation: \"\"\nresult: \"\"\ntally: {}\n");
    let plate = quill.config().compile_data(&doc, None).expect("compiles");

    assert_eq!(plate["quorum"], json!(0));
    assert_eq!(plate["location"], json!(""));
    for name in ["confidential", "attendees", "result", "adjourned", "minutes"] {
        assert_eq!(plate[name], json!(null), "{name}: {plate}");
    }
    assert_eq!(plate["tally"], json!({ "votes_for": null, "votes_against": 0 }));

    let resolved = quill.resolve(&doc, None);
    let row = |name: &str| {
        resolved
            .main
            .fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| (f.value.as_json().clone(), f.source))
            .unwrap()
    };
    assert_eq!(row("quorum"), (json!(0), FieldSource::Authored));
    assert_eq!(row("confidential"), (json!(null), FieldSource::Blank));

    let mut obliged: Vec<String> = quill
        .validate(&doc)
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("validation::must_fill"))
        .map(|d| d.path.unwrap_or_default())
        .collect();
    obliged.sort();
    assert_eq!(
        obliged,
        [
            "main.adjourned",
            "main.attendees",
            "main.confidential",
            "main.minutes",
            "main.tally.votes_against",
            "main.tally.votes_for",
        ],
        "a `?` leaves obligation to `default:`, and an authored blank discharges it"
    );
}

/// The declaration view spells the `?` as the quill did and reloads to the same
/// schema; the wire admits `null`; the blueprint names the cell optional.
#[test]
fn every_projection_carries_the_question_mark() {
    let config = QuillConfig::from_yaml(QUILL_YAML).expect("loads");

    let declared = config.schema();
    assert_eq!(declared["main"]["fields"]["quorum"]["type"], json!("integer?"));
    assert_eq!(
        declared["main"]["fields"]["tally"]["properties"]["votes_for"]["type"],
        json!("integer?")
    );
    let reloaded = QuillConfig::from_yaml(&QUILL_YAML.replace(
        "quorum: { type: integer? }",
        &format!("quorum: {}", declared["main"]["fields"]["quorum"]),
    ))
    .expect("the declaration view reloads");
    assert_eq!(reloaded.main.fields["quorum"], config.main.fields["quorum"]);

    let wire = build_transform_schema(&config);
    let props = &wire.as_json()["properties"];
    assert_eq!(props["quorum"]["type"], json!(["integer", "null"]));
    assert_eq!(props["attendees"]["type"], json!(["array", "null"]));
    assert_eq!(props["result"]["enum"], json!([null, "", "carried", "failed"]));
    assert_eq!(props["tally"]["properties"]["votes_against"]["type"], json!("integer"));

    let blueprint = config.blueprint();
    assert!(blueprint.contains("quorum: !must_fill # integer?"), "{blueprint}");
    assert!(blueprint.contains("# enum<carried | failed>?"), "{blueprint}");
}

#[test]
fn a_question_mark_refuses_a_default_and_a_namespace() {
    for (field, code) in [
        ("{ type: integer?, default: 0 }", "quill::optional_default"),
        (
            "{ type: object?, properties: { a: { type: string } } }",
            "quill::optional_namespace",
        ),
        ("{ type: matrix?, members: { a: A } }", "quill::optional_namespace"),
        (
            "{ type: enum?, values: [a], variants: { a: { note: { type: string } } } }",
            "quill::optional_namespace",
        ),
    ] {
        let yaml = format!(
            "quill:\n  name: q\n  version: \"1.0\"\n  backend: typst\n  description: q\n\
             main:\n  fields:\n    f: {field}\n"
        );
        let err = QuillConfig::from_yaml(&yaml).expect_err(field);
        assert!(err.to_string().contains(code), "{field}: {err}");
    }
}
