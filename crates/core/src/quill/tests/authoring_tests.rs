//! The authoring document: its JSON Schema, and the export and import that
//! carry a document across it.
use super::*;
use serde_json::json;

mod properties;

const SCHEMA: &str = r#"
main:
  title: Memo
  description: A one-page memo.
  body:
    description: What the memo says.
    example: "Filled-in *prose*."
  fields:
    subject:
      type: richtext
      multiline: false
      title: Subject
      description: The memo's subject line.
      example: Budget *review*
    from: { type: text, default: HQ }
    count: { type: integer? }
    when: { type: date }
    refs: { type: array, items: { type: text } }
    contact:
      type: object
      properties:
        city: { type: text }
    marking:
      type: enum
      values: [PUBLIC, CUI, SECRET]
      default: PUBLIC
      variants:
        CUI:
          poc: { type: richtext, multiline: false }
card_kinds:
  indorsement:
    title: Indorsement
    fields:
      from: { type: text }
      kind: { type: text }
  stamp:
    body:
      enabled: false
    fields:
      label: { type: text }
"#;

fn quill() -> Quill {
    quill_from_yaml(&with_header(SCHEMA))
}

#[test]
fn the_schema_is_the_document_envelope_with_a_kind_per_card() {
    let schema = quill().config().authoring_schema();
    assert_eq!(schema["title"], json!("Memo"));
    assert_eq!(schema["additionalProperties"], json!(false));
    let props = &schema["properties"];
    assert_eq!(props["fields"]["type"], json!(["object", "null"]));
    assert_eq!(props["body"]["contentMediaType"], json!(MARKDOWN_MEDIA_TYPE));
    assert_eq!(props["body"]["description"], json!("What the memo says."));
    assert_eq!(props["body"]["examples"], json!(["Filled-in *prose*."]));
    assert_eq!(
        props["cards"]["items"]["oneOf"],
        json!([{ "$ref": "#/$defs/indorsement" }, { "$ref": "#/$defs/stamp" }])
    );

    let indorsement = &schema["$defs"]["indorsement"];
    assert_eq!(indorsement["title"], json!("Indorsement"));
    assert_eq!(indorsement["required"], json!(["kind"]));
    assert_eq!(indorsement["properties"]["kind"], json!({ "const": "indorsement" }));
    // A quill field named `kind` sits under `fields`, clear of the discriminator.
    assert_eq!(
        indorsement["properties"]["fields"]["properties"]["kind"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        schema["$defs"]["stamp"]["properties"]["body"],
        json!({ "type": ["string", "null"], "maxLength": 0 })
    );
}

#[test]
fn a_content_leaf_is_its_codecs_text_and_every_cell_admits_null() {
    let schema = quill().config().authoring_schema();
    let fields = &schema["properties"]["fields"]["properties"];
    assert_eq!(
        fields["subject"],
        json!({
            "title": "Subject",
            "description": "The memo's subject line.",
            "type": ["string", "null"],
            "contentMediaType": MARKDOWN_MEDIA_TYPE,
            "examples": ["Budget *review*"],
        })
    );
    assert_eq!(fields["from"], json!({ "type": ["string", "null"], "default": "HQ" }));
    assert_eq!(fields["count"]["type"], json!(["integer", "null"]));
    assert_eq!(fields["when"], json!({ "type": ["string", "null"], "format": "date" }));
    assert_eq!(fields["refs"]["items"]["type"], json!(["string", "null"]));
    assert_eq!(fields["contact"]["properties"]["city"]["type"], json!(["string", "null"]));
    assert!(!schema.to_string().contains(CONTENT_MEDIA_TYPE), "{schema}");
}

#[test]
fn a_variant_bearing_enum_is_one_branch_per_world() {
    let schema = quill().config().authoring_schema();
    let marking = &schema["properties"]["fields"]["properties"]["marking"];
    assert_eq!(marking["default"], json!({ "value": "PUBLIC" }));
    let branches = marking["oneOf"].as_array().unwrap();
    assert_eq!(branches[0]["properties"]["value"], json!({ "const": "CUI" }));
    assert_eq!(branches[0]["properties"]["poc"]["contentMediaType"], json!(MARKDOWN_MEDIA_TYPE));
    assert_eq!(branches[0]["required"], json!(["value"]));
    assert_eq!(branches[1]["properties"]["value"]["enum"], json!(["", "PUBLIC", "SECRET"]));
    assert_eq!(branches[2], json!({ "type": "string", "enum": ["", "PUBLIC", "CUI", "SECRET"] }));
    assert_eq!(branches[3], json!({ "type": "null" }));
}

const MARKDOWN: &str = r#"~~~
$quill: q@1.0
$kind: main
$ext: { cursor: 3 }
subject: Budget *review*
count: 4
refs: [a, b]
marking:
  value: CUI
  poc: "**J.** Doe"
stranger: kept
~~~

The memo body.

~~~
$kind: indorsement
from: 42 SFS/CC
kind: concur
~~~

Concur.
"#;

#[test]
fn a_document_exports_in_the_values_form_and_imports_back_equal() {
    let quill = quill();
    let parsed = quill.parse(MARKDOWN).expect("parses");
    let mut document = parsed.document;
    document.main_mut().remove_ext();
    let authored = quill.to_authoring(&document);
    assert_eq!(
        authored,
        json!({
            "fields": {
                "subject": "Budget *review*",
                "count": 4,
                "refs": ["a", "b"],
                "marking": { "value": "CUI", "poc": "**J.** Doe" },
                "stranger": "kept",
            },
            "body": "The memo body.",
            "cards": [{
                "kind": "indorsement",
                "fields": { "from": "42 SFS/CC", "kind": "concur" },
                "body": "Concur.",
            }],
        })
    );

    let imported = quill.from_authoring(&authored).expect("imports");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.document, document);
}

#[test]
fn null_is_absent_and_the_quill_reference_is_the_quills() {
    let quill = quill();
    let imported = quill
        .from_authoring(&json!({
            "fields": { "subject": null, "from": "Ops" },
            "body": null,
            "cards": [{ "kind": "stamp", "fields": null }],
        }))
        .expect("imports")
        .document;
    assert_eq!(imported.quill_reference().to_string(), "q@1.0");
    assert!(imported.main().payload().get("subject").is_none());
    assert_eq!(imported.main().payload().get("from").unwrap().as_json(), &json!("Ops"));
    assert_eq!(imported.cards()[0].kind(), Some("stamp"));
    assert_eq!(
        quill.to_authoring(&imported),
        json!({ "fields": { "from": "Ops" }, "body": "", "cards": [{ "kind": "stamp", "fields": {} }] })
    );
}

#[test]
fn a_value_the_schema_refuses_lands_and_validates_at_its_document_path() {
    let quill = quill();
    let imported = quill
        .from_authoring(&json!({
            "fields": { "count": "many", "subject": "Two\n\nparagraphs" },
            "cards": [{ "kind": "indorsement", "fields": { "from": 7 } }],
        }))
        .expect("imports");
    assert!(
        imported.warnings.iter().any(|d| d.path.as_deref() == Some("main.subject")),
        "{:?}",
        imported.warnings
    );
    let paths: Vec<_> = quill
        .validate(&imported.document)
        .into_iter()
        .filter_map(|d| d.path)
        .collect();
    assert!(paths.contains(&"main.count".to_string()), "{paths:?}");
}

#[test]
fn a_shape_the_document_does_not_take_is_refused_with_every_error() {
    let quill = quill();
    let refusals = |value: serde_json::Value| -> Vec<(String, Option<String>)> {
        quill
            .from_authoring(&value)
            .map(|_| ())
            .expect_err("refused")
            .into_iter()
            .map(|d| match d.args.get("pointer") {
                Some(pointer) => {
                    assert_eq!(d.path, None, "a shape error names no document-model path");
                    (d.code.unwrap_or_default(), pointer.as_str().map(str::to_string))
                }
                None => (d.code.unwrap_or_default(), d.path),
            })
            .collect()
    };
    let shape = |at: &str| (AUTHORING_INVALID_SHAPE.to_string(), Some(at.to_string()));
    assert_eq!(refusals(json!([])), [shape("")]);
    assert_eq!(
        refusals(json!({
            "$quill": "q@1.0",
            "": 1,
            "fields": [],
            "body": 3,
            "cards": [{ "fields": {} }, { "kind": "indorsement", "a/b": 1 }, { "kind": "BAD KIND" }],
        })),
        [
            shape("/$quill"),
            shape("/"),
            shape("/fields"),
            shape("/body"),
            shape("/cards/0/kind"),
            shape("/cards/1/a~1b"),
            ("edit::invalid_kind_name".to_string(), Some("cards[2]".to_string())),
        ]
    );
    assert_eq!(refusals(json!({ "fields": { "": 1 } })), [shape("/fields/")]);
    assert_eq!(
        refusals(json!({ "fields": { "$ext": {} } })),
        [("edit::invalid_field_name".to_string(), Some("main.$ext".to_string()))]
    );
}

#[test]
fn a_leaf_that_does_not_decode_exports_as_stored() {
    let quill = quill();
    let value = json!({ "fields": { "subject": 5, "refs": ["- a\n- b"] } });
    let imported = quill.from_authoring(&value).expect("imports").document;
    assert_eq!(quill.to_authoring(&imported)["fields"], value["fields"]);

    let one_line = json!({ "fields": { "subject": "- a\n- b" } });
    let imported = quill.from_authoring(&one_line).expect("imports").document;
    let exported = quill.to_authoring(&imported);
    assert_eq!(exported["fields"], one_line["fields"]);
    assert_eq!(quill.from_authoring(&exported).expect("imports").document, imported);
}
