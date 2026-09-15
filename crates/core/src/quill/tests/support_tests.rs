
use crate::quill::{quill_from_yaml, BlockConstruct, QuillConfig, UNSUPPORTED_CONSTRUCT};
use crate::{document::Document, error::Severity, quill::Quill};

const YAML: &str = r#"
quill:
  name: decliner
  version: 0.1.0
  backend: typst
  description: A quill that declines constructs
main:
  body:
    unsupported: [rule, table]
  fields: {}
card_kinds:
  note:
    body: {}
    fields: {}
"#;

fn quill() -> Quill {
    quill_from_yaml(YAML)
}

fn warn(body: &str) -> Vec<(String, String, String, u64)> {
    let markdown = format!("~~~card-yaml\n$quill: decliner\n~~~\n\n{body}\n");
    let parsed = quill().parse(&markdown).expect("parses and conforms");
    parsed
        .warnings
        .iter()
        .filter(|d| d.code.as_deref() == Some(UNSUPPORTED_CONSTRUCT))
        .map(|d| {
            assert_eq!(d.severity, Severity::Warning, "never fatal: {d:?}");
            (
                d.code.clone().unwrap(),
                d.path.clone().unwrap_or_default(),
                d.args["construct"].as_str().unwrap().to_string(),
                d.args["count"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn constructs(body: &str) -> Vec<(String, u64)> {
    warn(body)
        .into_iter()
        .map(|(_, _, construct, count)| (construct, count))
        .collect()
}

#[test]
fn a_declined_construct_warns_against_its_body() {
    assert_eq!(
        warn("one\n\n***\n\ntwo"),
        [(
            UNSUPPORTED_CONSTRUCT.to_string(),
            "main.body".to_string(),
            "rule".to_string(),
            1
        )]
    );
}

#[test]
fn occurrences_collapse_into_one_count() {
    assert_eq!(constructs("a\n\n***\n\nb\n\n***\n\nc\n\n***"), [("rule".to_string(), 3)]);
    assert_eq!(
        constructs("- ***\n\n| a |\n| --- |\n| 1 |\n\n***"),
        [("rule".to_string(), 2), ("table".to_string(), 1)]
    );
}

#[test]
fn an_undeclared_construct_is_silent() {
    assert_eq!(constructs("# Title\n\n> quoted\n\n- a\n- b\n\n```\nx\n```"), []);
    assert_eq!(constructs("just a paragraph"), []);
    assert_eq!(constructs(""), []);
}

#[test]
fn the_declaration_is_per_body() {
    let markdown = "~~~card-yaml\n$quill: decliner\n~~~\n\n***\n\n~~~\n$kind: note\n~~~\n\n***\n";
    let parsed = quill().parse(markdown).expect("parses");
    let paths: Vec<_> = parsed
        .warnings
        .iter()
        .filter(|d| d.code.as_deref() == Some(UNSUPPORTED_CONSTRUCT))
        .map(|d| d.path.clone().unwrap_or_default())
        .collect();
    assert_eq!(paths, ["main.body"], "the note's body declines nothing");
}

#[test]
fn the_walk_is_idempotent() {
    let markdown = "~~~card-yaml\n$quill: decliner\n~~~\n\n***\n";
    let quill = quill();
    let mut document = Document::parse(markdown).expect("parses").document;
    quill.conform(&mut document).expect("conforms");
    let first = quill.unsupported_constructs(&document);
    assert_eq!(first, quill.unsupported_constructs(&document));
    assert_eq!(first.len(), 1);
}

#[test]
fn an_unknown_construct_name_fails_the_load() {
    let yaml = YAML.replace("[rule, table]", "[rules]");
    assert!(
        QuillConfig::from_yaml_with_warnings(&yaml).is_err(),
        "`rules` is not a construct name"
    );
}

#[test]
fn a_container_counts_once_per_run() {
    let quill = quill_from_yaml(&YAML.replace("[rule, table]", "[list, quote]"));
    let census = |body: &str| {
        let markdown = format!("~~~card-yaml\n$quill: decliner\n~~~\n\n{body}\n");
        let parsed = quill.parse(&markdown).expect("parses");
        parsed
            .warnings
            .iter()
            .filter(|d| d.code.as_deref() == Some(UNSUPPORTED_CONSTRUCT))
            .map(|d| {
                (
                    d.args["construct"].as_str().unwrap().to_string(),
                    d.args["count"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(census("- a\n- b\n- c"), [("list".to_string(), 1)]);
    assert_eq!(census("- a\n\n  still a"), [("list".to_string(), 1)]);
    assert_eq!(census("- a\n  - b\n  - c"), [("list".to_string(), 2)]);
    assert_eq!(census("- a\n\nbetween\n\n- b"), [("list".to_string(), 2)]);
    assert_eq!(census("> one\n>\n> two"), [("quote".to_string(), 1)]);
}

#[test]
fn every_construct_name_is_reachable() {
    for (construct, body) in [
        (BlockConstruct::Heading, "# h"),
        (BlockConstruct::Rule, "***"),
        (BlockConstruct::Code, "```\nx\n```"),
        (BlockConstruct::List, "- a"),
        (BlockConstruct::Quote, "> q"),
        (BlockConstruct::Table, "| a |\n| --- |\n| 1 |"),
        (BlockConstruct::Image, "![alt](cat.png)"),
    ] {
        let quill = quill_from_yaml(&YAML.replace("[rule, table]", &format!("[{construct}]")));
        let markdown = format!("~~~card-yaml\n$quill: decliner\n~~~\n\n{body}\n");
        let parsed = quill.parse(&markdown).expect("parses");
        let hits: Vec<_> = parsed
            .warnings
            .iter()
            .filter(|d| d.code.as_deref() == Some(UNSUPPORTED_CONSTRUCT))
            .collect();
        assert_eq!(hits.len(), 1, "{construct} did not fire on {body:?}");
        assert_eq!(hits[0].args["construct"].as_str().unwrap(), construct.as_str());
    }
}
