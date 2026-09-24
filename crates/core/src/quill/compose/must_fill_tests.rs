//! The obligation axis: `default:`'s absence and the unauthored-cell predicate.

use crate::quill::quill_from_yaml;
use crate::quill::resolved::FieldSource;
use crate::{document::Document, quill::Quill};

fn obligations(quill: &Quill, md: &str) -> Vec<(String, String)> {
    let doc = Document::parse(md).expect("parse").document;
    let mut out: Vec<(String, String)> = quill
        .validate(&doc)
        .iter()
        .filter(|d| d.code.as_deref() == Some("validation::must_fill"))
        .map(|d| {
            (
                d.path.clone().expect("every must_fill anchors at a path"),
                d.args["trigger"].as_str().expect("trigger arg").to_string(),
            )
        })
        .collect();
    out.sort();
    out
}

fn paths(quill: &Quill, md: &str) -> Vec<String> {
    obligations(quill, md).into_iter().map(|(p, _)| p).collect()
}

const SCALARS: &str = r#"
quill: { name: ob, version: 1.0.0, backend: typst, description: obligations }
main:
  fields:
    subject:    { type: string }
    severity:   { type: enum, values: [low, high] }
    status:     { type: string, default: draft }
    confirmed:  { type: string, example: draft }
    optional:   { type: string, default: "" }
"#;

fn md(fields: &str) -> String {
    format!("~~~card-yaml\n$quill: ob@1.0.0\n$kind: main\n{fields}~~~\n")
}

#[test]
fn a_defaultless_field_is_obliged_and_a_defaulted_one_is_not() {
    let quill = quill_from_yaml(SCALARS);

    assert_eq!(
        paths(&quill, &md("")),
        ["main.confirmed", "main.severity", "main.subject"]
    );
}

#[test]
fn authoring_a_value_or_the_blank_discharges_the_obligation() {
    let quill = quill_from_yaml(SCALARS);

    assert_eq!(
        paths(
            &quill,
            &md("subject: Q3 results\nseverity: \"\"\nconfirmed: draft\n")
        ),
        Vec::<String>::new()
    );
}

#[test]
fn null_does_not_discharge_but_the_blank_does() {
    let quill = quill_from_yaml(SCALARS);

    let rest = "severity: low\nconfirmed: draft\n";
    assert_eq!(
        paths(&quill, &md(&format!("subject: null\n{rest}"))),
        ["main.subject"]
    );
    assert!(paths(&quill, &md(&format!("subject: \"\"\n{rest}"))).is_empty());
}

const CONTAINERS: &str = r#"
quill: { name: cn, version: 1.0.0, backend: typst, description: containers }
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string }
        city:   { type: string }
        zip:    { type: string, default: "" }
    recipients:
      type: array
      items: { type: string }
"#;

fn cn(fields: &str) -> String {
    format!("~~~card-yaml\n$quill: cn@1.0.0\n$kind: main\n{fields}~~~\n")
}

#[test]
fn a_typed_dict_is_obliged_at_its_leaves_present_or_absent() {
    let quill = quill_from_yaml(CONTAINERS);

    let absent = paths(&quill, &cn("recipients: []\n"));
    let touched = paths(&quill, &cn("recipients: []\naddress:\n  city: Pittsburgh\n"));
    assert_eq!(absent, ["main.address.city", "main.address.street"]);
    assert_eq!(touched, ["main.address.street"]);
}

#[test]
fn a_touched_container_does_not_silence_its_unauthored_leaves() {
    let quill = quill_from_yaml(CONTAINERS);

    // The source rung is asserted alongside to show what a `FieldSource`-keyed
    // check would see instead: one authored dict, and no leaf.
    let doc = Document::parse(&cn("address:\n  city: Pittsburgh\n"))
        .expect("parse")
        .document;
    let (_, source) = super::resolve_value_sourced(
        doc.main().payload().get("address"),
        &crate::quill::QuillConfig::from_yaml(CONTAINERS).unwrap().main.fields["address"],
        None,
    );
    assert_eq!(source, FieldSource::Authored, "the view sees one authored dict");
    assert!(paths(&quill, &cn("address:\n  city: Pittsburgh\n"))
        .contains(&"main.address.street".to_string()));
}

#[test]
fn an_array_is_one_cell_and_the_empty_array_answers_it() {
    let quill = quill_from_yaml(CONTAINERS);

    assert!(paths(&quill, &cn("")).contains(&"main.recipients".to_string()));
    assert!(!paths(&quill, &cn("recipients: []\n")).contains(&"main.recipients".to_string()));

    assert!(paths(&quill, &cn("recipients: [Ada, null]\n"))
        .contains(&"main.recipients[1]".to_string()));
}

const CARDS: &str = r#"
quill: { name: cd, version: 1.0.0, backend: typst, description: cards }
main:
  fields:
    title: { type: string }
card_kinds:
  indorsement:
    fields:
      from: { type: string }
"#;

#[test]
fn a_card_kind_is_obliged_once_per_instance() {
    let quill = quill_from_yaml(CARDS);
    let root = "~~~card-yaml\n$quill: cd@1.0.0\n$kind: main\ntitle: T\n~~~\n";
    let card = "\n~~~card-yaml\n$kind: indorsement\n~~~\n";

    assert!(paths(&quill, root).is_empty());
    assert_eq!(
        paths(&quill, &format!("{root}{card}")),
        ["cards.indorsement[0].from"]
    );
    assert_eq!(
        paths(&quill, &format!("{root}{card}{card}")),
        [
            "cards.indorsement[0].from",
            "cards.indorsement[1].from"
        ]
    );
}

#[test]
fn the_marker_wins_where_both_triggers_fire() {
    let quill = quill_from_yaml(SCALARS);

    assert_eq!(
        obligations(&quill, &md("subject: !must_fill\nseverity: \"\"\nconfirmed: x\n")),
        [("main.subject".to_string(), "marker".to_string())]
    );

    // Present, in-domain, indistinguishable from authored content: the schema
    // predicate is structurally blind to this one, so only the tag catches it.
    assert_eq!(
        obligations(&quill, &md("subject: !must_fill Draft\nseverity: low\nconfirmed: x\n")),
        [("main.subject".to_string(), "marker".to_string())]
    );
}

#[test]
fn a_hand_written_tag_fires_on_an_unobliged_field() {
    let quill = quill_from_yaml(SCALARS);

    assert_eq!(
        obligations(
            &quill,
            &md("subject: S\nseverity: low\nconfirmed: x\noptional: !must_fill later\n")
        ),
        [("main.optional".to_string(), "marker".to_string())]
    );
}

#[test]
fn an_unauthored_obligation_never_gates_render() {
    let quill = quill_from_yaml(SCALARS);
    let doc = Document::parse(&md("")).expect("parse").document;

    assert_eq!(paths(&quill, &md("")).len(), 3, "the document is incomplete");
    let plate = quill.compile_data(&doc, None).expect("and renders anyway");
    assert_eq!(plate["subject"], "", "the unauthored cell blank-fills");
}

const SEEDED: &str = r#"
quill: { name: sd, version: 1.0.0, backend: typst, description: seed }
main:
  fields:
    subject:  { type: string, example: Q3 results }
    memo_for: { type: array, items: { type: string }, example: [Ada] }
    status:   { type: string, default: draft, example: final }
"#;

#[test]
fn a_fresh_seed_and_a_blank_document_report_the_same_cells() {
    let quill = quill_from_yaml(SEEDED);

    let seeded = quill.seed_document();
    let mut from_seed: Vec<(String, String)> = quill
        .validate(&seeded)
        .iter()
        .filter(|d| d.code.as_deref() == Some("validation::must_fill"))
        .map(|d| {
            (
                d.path.clone().unwrap(),
                d.args["trigger"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    from_seed.sort();

    let blank = obligations(
        &quill_from_yaml(SEEDED),
        "~~~card-yaml\n$quill: sd@1.0.0\n$kind: main\n~~~\n",
    );

    assert_eq!(
        from_seed.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        blank.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        "seed and blank document agree on the cells"
    );
    assert_eq!(
        from_seed,
        [
            ("main.memo_for".to_string(), "marker".to_string()),
            ("main.subject".to_string(), "marker".to_string())
        ],
        "the seed's signal rides the committed marker"
    );
    assert!(
        blank.iter().all(|(_, t)| t == "unauthored"),
        "the blank document's rides the schema: {blank:?}"
    );
}

#[test]
fn a_seed_overlay_value_commits_unmarked() {
    let quill = quill_from_yaml(
        r#"
quill: { name: ov, version: 1.0.0, backend: typst, description: overlay }
main:
  fields:
    title: { type: string }
card_kinds:
  note:
    fields:
      label: { type: string, example: A label }
"#,
    );

    let bare = quill.seed_card("note", None).expect("declared kind");
    assert!(bare.payload().is_fill("label"), "an example stays a placeholder");

    let overlay =
        crate::document::SeedOverlay::from_json(&serde_json::json!({ "label": "Enclosure" }))
        .expect("valid overlay");
    let chosen = quill.seed_card("note", Some(&overlay)).expect("declared kind");
    assert!(
        !chosen.payload().is_fill("label"),
        "an overlay value is a decision, not a placeholder"
    );
}
