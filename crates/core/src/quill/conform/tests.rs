
mod properties;

use serde_json::json;

use crate::document::StoredDocument;
use crate::quill::quill_from_yaml;
use crate::{document::{Document, SeedOverlay}, quill::Quill, value::QuillValue};

const QUILL: &str = r#"
quill:
  name: conform_test
  version: "1.0"
  backend: typst
  description: Conform test
main:
  fields:
    subject:
      type: richtext
      inline: true
      example: "Q3 **results**"
    note:
      type: plaintext
      example: "a *literal* line"
    qty:
      type: integer
    tags:
      type: array
      items:
        type: richtext
    meta:
      type: object
      properties:
        label:
          type: plaintext
        blurb:
          type: richtext
card_kinds:
  entry:
    fields:
      body:
        type: richtext
      caption:
        type: plaintext
        example: "raw *text*"
"#;

fn quill() -> Quill {
    quill_from_yaml(QUILL)
}

fn bytes(doc: &Document) -> String {
    serde_json::to_string(&StoredDocument::from(doc.clone())).expect("storage DTO serializes")
}

fn parse_bound(quill: &Quill, md: &str) -> (Document, Vec<crate::error::Diagnostic>) {
    let parsed = quill.parse(md).expect("the bound door parses");
    (parsed.document, parsed.warnings)
}

const MD: &str = "\
~~~card-yaml
$quill: conform_test@1.0.0
subject: Q3 **results**
note: a *literal* line
qty: 3
tags:
  - one **bold**
  - two
meta:
  label: keep *this*
  blurb: and **this**
~~~

Main body.

~~~card-yaml
$kind: entry
body: card **body**
caption: raw *text*
~~~

Entry body.
";

#[test]
fn parse_then_conform_equals_typed_write() {
    let quill = quill();
    let (conformed, warnings) = parse_bound(&quill, MD);
    assert!(warnings.is_empty(), "clean document: {warnings:?}");

    let mut written = Document::parse(MD).expect("transport parse").document;
    {
        let mut w = quill.writer(&mut written);
        w.set("subject", "Q3 **results**").unwrap();
        w.set("note", "a *literal* line").unwrap();
        w.set("tags", json!(["one **bold**", "two"])).unwrap();
        w.set(
            "meta",
            json!({ "label": "keep *this*", "blurb": "and **this**" }),
        )
        .unwrap();
        let mut card = w.card(0).unwrap();
        card.set("body", "card **body**").unwrap();
        card.set("caption", "raw *text*").unwrap();
    }

    assert_eq!(conformed, written, "the two lanes must rest equal");
    assert_eq!(bytes(&conformed), bytes(&written), "and byte-equal");
}

#[test]
fn rest_is_per_codec_at_every_depth() {
    let quill = quill();
    let (doc, _) = parse_bound(&quill, MD);
    let payload = doc.main().payload();

    assert!(payload.get("subject").unwrap().as_json().is_object());
    assert_eq!(
        payload.get("note").unwrap().as_json(),
        &json!("a *literal* line"),
        "plaintext rests as the literal string, escapes and all"
    );
    assert_eq!(payload.get("qty").unwrap().as_json(), &json!(3));

    let tags = payload.get("tags").unwrap().as_json();
    assert!(tags.as_array().unwrap().iter().all(|e| e.is_object()));
    let meta = payload.get("meta").unwrap().as_json();
    assert_eq!(meta.get("label").unwrap(), &json!("keep *this*"));
    assert!(meta.get("blurb").unwrap().is_object());

    let card = &doc.cards()[0];
    assert!(card.payload().get("body").unwrap().as_json().is_object());
    assert_eq!(
        card.payload().get("caption").unwrap().as_json(),
        &json!("raw *text*")
    );
}

#[test]
fn conform_preserves_comments_and_untouched_bytes() {
    let quill = quill();
    let md = "\
~~~card-yaml
$quill: conform_test@1.0.0
# a leading comment
note: plain text
meta:
  # a nested comment
  label: inner
~~~

Body.
";
    let (mut doc, _) = parse_bound(&quill, md);
    let before = bytes(&doc);
    let markdown_before = doc.to_markdown();
    assert!(
        markdown_before.contains("# a nested comment"),
        "comments survive the first conform: {markdown_before}"
    );
    quill.conform(&mut doc).expect("conform");
    assert_eq!(bytes(&doc), before);
    assert_eq!(doc.to_markdown(), markdown_before);
}

#[test]
fn fill_marked_fields_are_skipped() {
    let quill = quill();
    let md = "\
~~~card-yaml
$quill: conform_test@1.0.0
subject: !must_fill Q3 **results**
meta:
  label: keep *this*
  blurb: !must_fill and **this**
~~~

Body.
";
    let (mut doc, _) = parse_bound(&quill, md);
    let payload = doc.main().payload();
    assert!(
        payload.get("subject").unwrap().as_json().is_string(),
        "a root marker skips the field"
    );
    assert!(
        payload.get("meta").unwrap().as_json()["blurb"].is_string()
            && payload.get("meta").unwrap().as_json()["label"] == json!("keep *this*"),
        "a marker on one property skips the whole field, its clean siblings included"
    );
    assert!(doc.main().payload().is_fill("subject"));

    let before = bytes(&doc);
    quill.conform(&mut doc).expect("conform");
    assert_eq!(bytes(&doc), before, "and a repeat conform moves nothing");
}

#[test]
fn wrong_quill_errors_before_any_mutation() {
    let quill = quill();
    let md = "~~~card-yaml\n$quill: other_quill\nsubject: hi\n~~~\n\nBody.";
    let mut doc = Document::parse(md).expect("transport parse").document;
    let before = bytes(&doc);
    let err = quill.conform(&mut doc).expect_err("name mismatch errors");
    assert_eq!(
        err.diagnostics()[0].code.as_deref(),
        Some("quill::name_mismatch")
    );
    assert_eq!(bytes(&doc), before, "the document is untouched");
    assert!(quill.parse(md).is_err(), "and the bound parse fails too");
}

#[test]
fn non_conforming_value_rests_authored_with_a_diagnostic() {
    let quill = quill();
    let md = "~~~card-yaml\n$quill: conform_test@1.0.0\nsubject: 42\n~~~\n\nBody.";
    let (doc, warnings) = parse_bound(&quill, md);
    let diag = warnings
        .iter()
        .find(|d| d.code.as_deref().is_some_and(|c| c.starts_with("conform::")))
        .expect("a conform diagnostic");
    assert_eq!(diag.code.as_deref(), Some("conform::field_decode"));
    assert_eq!(diag.path.as_deref(), Some("main.subject"));
    assert_eq!(
        doc.main().payload().get("subject").unwrap().as_json(),
        &json!(42),
        "the value stays authored: no silent retype"
    );
    quill.compile_data(&doc).expect("still renders");
    assert!(
        !quill
            .validate(&doc)
            .iter()
            .any(|d| d.severity == crate::error::Severity::Error),
        "and validates clean: the render floor accepts the scalar"
    );

    let mut legacy = doc;
    legacy
        .main_mut()
        .store_field("subject", QuillValue::from_json(json!({ "prose": "older" })))
        .unwrap();
    let before = bytes(&legacy);
    let diags = quill.conform(&mut legacy).expect("the quill matches");
    assert_eq!(
        diags[0].code.as_deref(),
        Some("conform::field_decode")
    );
    assert_eq!(bytes(&legacy), before, "the value is left exactly as stored");
}

#[test]
fn conform_is_a_no_op_on_seeds() {
    let quill = quill();
    let mut doc = quill.seed_document();
    assert_eq!(
        doc.main().payload().get("note").unwrap().as_json(),
        &json!("a *literal* line"),
        "a seeded plaintext field rests as its literal string"
    );
    let before = bytes(&doc);
    let diags = quill.conform(&mut doc).expect("conform");
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(bytes(&doc), before, "seed_document is already at rest");

    let overlay = SeedOverlay::from_json(&json!({ "caption": "overlaid *text*" })).unwrap();
    let card = quill.seed_card("entry", Some(&overlay)).expect("kind exists");
    assert_eq!(
        card.payload().get("caption").unwrap().as_json(),
        &json!("overlaid *text*")
    );
    let mut doc2 = quill.seed_document();
    doc2.push_card(card).unwrap();
    let before2 = bytes(&doc2);
    quill.conform(&mut doc2).expect("conform");
    assert_eq!(bytes(&doc2), before2, "seed_card is already at rest");
}

/// A variant cell commits through the same strict write every other seeded
/// field takes, so a content cell rests as a content object rather than as the
/// overlay's raw markdown. Otherwise conform moves bytes and hash on a document
/// nobody edited.
#[test]
fn conform_is_a_no_op_on_a_seeded_variant_cell() {
    const YAML: &str = r#"
quill:
  name: variant_seed
  version: "1.0"
  backend: typst
  description: variant seed rest
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
    let quill = quill_from_yaml(YAML);
    let overlay = SeedOverlay::from_json(&json!({
        "classification": "CUI", "note": "**bold note**"
    }))
    .unwrap();
    let card = quill.seed_card("entry", Some(&overlay)).expect("kind exists");
    let value = card
        .payload()
        .get("note")
        .expect("seeded note")
        .as_json()
        .clone();
    assert!(
        value.is_object(),
        "a seeded content cell rests as a content object, got {value}"
    );

    let mut doc = quill.seed_document();
    doc.push_card(card).unwrap();
    let before = bytes(&doc);
    let diags = quill.conform(&mut doc).expect("conform");
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(bytes(&doc), before, "the seeded cell is already at rest");
}

#[test]
fn a_fill_tag_on_a_seeded_cell_survives_store_load_conform() {
    let quill = quill();
    let doc = quill.seed_document();
    // Seeding stamps them: an `example` on a must-fill field is shape
    // documentation, not the answer.
    let tagged = bytes(&doc);

    let mut loaded = Document::try_from(
        serde_json::from_str::<StoredDocument>(&tagged).expect("the tagged seed loads"),
    )
    .expect("and converts to a document");
    let diags = quill.conform(&mut loaded).expect("the quill matches");

    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(bytes(&loaded), tagged, "the cycle moves no bytes");
    assert!(
        loaded.main().payload().is_fill("subject") && loaded.main().payload().is_fill("note"),
        "both tags survive the round trip"
    );
    assert_eq!(
        loaded.main().payload().get("note").unwrap().as_json(),
        &json!("a *literal* line"),
        "the tagged plaintext cell keeps its resting form"
    );
    assert!(
        loaded.main().payload().get("subject").unwrap().as_json().is_object(),
        "and the tagged richtext cell keeps its canonical content object"
    );

    // The flag rides the payload item; nothing recomputes it from the schema at
    // load. Dropped from a document whose schema still says must-fill, it stays
    // dropped — the document is sovereign over its own markers.
    let mut cleared = quill.seed_document();
    for key in ["subject", "note"] {
        let seeded = cleared.main().payload().get(key).expect("seeded").clone();
        cleared.main_mut().store_field(key, seeded).unwrap();
    }
    let untagged = bytes(&cleared);
    assert_ne!(tagged, untagged, "the tag is stored, not inferred");

    let mut reloaded = Document::try_from(
        serde_json::from_str::<StoredDocument>(&untagged).expect("the untagged seed loads"),
    )
    .expect("and converts to a document");
    quill.conform(&mut reloaded).expect("the quill matches");
    assert_eq!(bytes(&reloaded), untagged, "and no cycle re-stamps it");
}

#[test]
fn the_plate_shape_for_plaintext_is_unchanged() {
    let quill = quill();
    let plate_note = |value: serde_json::Value| {
        let mut doc = Document::parse(
            "~~~card-yaml\n$quill: conform_test@1.0.0\n~~~\n\nBody.",
        )
        .expect("parse")
        .document;
        {
            let mut w = quill.writer(&mut doc);
            w.set("note", value).unwrap();
        }
        let plate = quill.compile_data(&doc).expect("compiles");
        plate["note"].clone()
    };

    let from_string = plate_note(json!("a *literal* line"));
    let from_object = plate_note(json!(quillmark_content::serial::to_canonical_value(
        &quillmark_content::import::from_plaintext("a *literal* line")
    )));
    assert!(from_string.is_object(), "plate keeps the content object");
    assert_eq!(from_string, from_object, "both commit inputs, one plate");
    assert_eq!(from_string["text"], json!("a *literal* line"));
}

#[test]
fn the_plaintext_revise_lane_is_literal() {
    let quill = quill();
    let md = r#"~~~card-yaml
$quill: conform_test@1.0.0
note: 'a \*b\* line'
~~~

Body.
"#;
    let (mut doc, _) = parse_bound(&quill, md);
    let before = bytes(&doc);
    let text = quill
        .reader(&doc)
        .get("note")
        .unwrap()
        .expect("note is present");
    let text = text
        .as_str()
        .expect("a plaintext field reads as its literal text")
        .to_string();
    assert_eq!(text, r"a \*b\* line");

    let delta = quill
        .writer(&mut doc)
        .revise_field("note", &text)
        .expect("revise");
    assert!(
        delta
            .ops
            .iter()
            .all(|op| matches!(op, quillmark_content::delta::Op::Retain(_))),
        "a no-change revise is all-retain: {delta:?}"
    );
    assert_eq!(bytes(&doc), before, "a no-change revise moves no bytes");

    quill
        .writer(&mut doc)
        .revise_field("note", r"a \*b\* line, revised")
        .expect("revise");
    assert_eq!(
        doc.main().payload().get("note").unwrap().as_json(),
        &json!(r"a \*b\* line, revised"),
        "and an edit rests as the literal string, escapes intact"
    );
}

#[test]
fn a_0_92_0_row_migrates_then_converges() {
    let quill = quill();
    let legacy = json!({
        "schema": "quillmark/document@0.92.0",
        "main": {
            "payload": { "items": [
                { "type": "quill", "value": "conform_test@1.0.0" },
                { "type": "kind", "value": "main" },
                { "type": "field", "key": "subject", "value": "Q3 **results**" },
                { "type": "field", "key": "note", "value":
                    quillmark_content::serial::to_canonical_value(
                        &quillmark_content::import::from_plaintext("a *literal* line")) },
                { "type": "field", "key": "qty", "value": 3 },
            ]},
            "body": "Main **body**."
        },
        "cards": [{
            "payload": { "items": [
                { "type": "kind", "value": "entry" },
                { "type": "field", "key": "body", "value": "card **body**" },
            ]},
            "body": "Entry body."
        }]
    })
    .to_string();

    let mut doc = Document::try_from(
        serde_json::from_str::<StoredDocument>(&legacy).expect("a 0.92.0 blob still loads"),
    )
    .expect("and migrates forward");
    assert_eq!(doc.main().body_markdown(), "Main **body**.");
    assert!(doc.main().payload().get("subject").unwrap().as_json().is_string());

    let diags = quill.conform(&mut doc).expect("the quill matches");
    assert!(diags.is_empty(), "{diags:?}");
    assert!(
        doc.main().payload().get("subject").unwrap().as_json().is_object(),
        "the authored richtext string converges to the canonical content object"
    );
    assert_eq!(
        doc.main().payload().get("note").unwrap().as_json(),
        &json!("a *literal* line"),
        "and the object-rest plaintext converges to its literal string"
    );
    assert!(doc.cards()[0].payload().get("body").unwrap().as_json().is_object());

    let restored = bytes(&doc);
    assert!(restored.contains("quillmark/document@0.115.0"));
    let (authored, _) = parse_bound(
        &quill,
        "\
~~~card-yaml
$quill: conform_test@1.0.0
$kind: main
subject: Q3 **results**
note: a *literal* line
qty: 3
~~~

Main **body**.

~~~card-yaml
$kind: entry
body: card **body**
~~~

Entry body.
",
    );
    assert_eq!(restored, bytes(&authored), "one document, two ingress routes");
}
