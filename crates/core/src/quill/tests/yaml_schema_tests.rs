//! `docs/schemas/quill.schema.json`, the JSON Schema editors check a
//! `Quill.yaml` against, held to the loader: every key set it names is the one
//! the loader reads, it accepts every quill the loader loads, and it refuses
//! the shape errors the loader refuses.

use std::collections::BTreeSet;

use super::*;
use crate::quill::config::QUILL_SECTION_KEYS;
use crate::quill::types::{BODY_CARD_SCHEMA_KEYS, FieldType, UI_CARD_SCHEMA_KEYS};

const SCHEMA: &str = include_str!("../../../../../docs/schemas/quill.schema.json");
const FIXTURE_QUILLS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/resources/quills");
const DOCS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs");
const MODELINE: &str =
    "# yaml-language-server: $schema=https://quillmark.dev/schemas/quill.schema.json\n";

const HEADER: &str =
    "quill:\n  name: memo\n  version: \"1.0.0\"\n  backend: typst\n  description: A memo\n";

fn schema() -> serde_json::Value {
    serde_json::from_str(SCHEMA).expect("the schema is JSON")
}

fn validator() -> jsonschema::Validator {
    jsonschema::draft7::new(&schema()).expect("the schema compiles")
}

fn instance(yaml: &str) -> serde_json::Value {
    crate::value::parse_yaml(yaml).expect("the probe is YAML")
}

fn keys(schema: &serde_json::Value, definition: &str) -> BTreeSet<String> {
    schema["definitions"][definition]["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("definitions.{definition} declares properties"))
        .keys()
        .cloned()
        .collect()
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

/// The names serde lists after "expected" in the one error loading `yaml`
/// draws: a struct's fields for an unknown key, an enum's variants for an
/// unknown one.
fn serde_expected(yaml: &str) -> BTreeSet<String> {
    let errors = QuillConfig::from_yaml_with_warnings(yaml).expect_err("the probe is refused");
    let message = &errors[0].message;
    let (_, expected) = message
        .split_once("expected")
        .unwrap_or_else(|| panic!("serde lists what it expected: {message}"));
    expected
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// A field of `token`'s type carrying the keys its type requires.
fn minimal_field(token: &str) -> String {
    let companion = match token.trim_end_matches('?') {
        "richtext" => ", multiline: false",
        "enum" => ", values: [a]",
        "array" => ", items: { type: text }",
        "object" => ", properties: { a: { type: text } }",
        "matrix" => ", members: { a: A }",
        "number" | "integer" | "boolean" | "date" | "datetime" | "text" => "",
        other => panic!("give type `{other}` its companion keys here"),
    };
    format!("{HEADER}main:\n  fields:\n    f: {{ type: \"{token}\"{companion} }}\n")
}

#[test]
fn the_schema_names_every_key_the_loader_reads() {
    let schema = schema();
    assert_eq!(keys(&schema, "quill"), set(QUILL_SECTION_KEYS));
    assert_eq!(keys(&schema, "body"), set(BODY_CARD_SCHEMA_KEYS));
    assert_eq!(keys(&schema, "card_ui"), set(UI_CARD_SCHEMA_KEYS));

    let card = serde_expected(&format!("{HEADER}main:\n  zz: 1\n"));
    assert_eq!(keys(&schema, "card_kind"), card);
    let main: BTreeSet<String> = card.into_iter().filter(|k| k != "seed").collect();
    assert_eq!(keys(&schema, "main"), main, "`main` declares no `seed`");

    assert_eq!(
        keys(&schema, "field"),
        serde_expected(&format!(
            "{HEADER}main:\n  fields:\n    f: {{ type: text, zz: 1 }}\n"
        ))
    );
    assert_eq!(
        keys(&schema, "field_ui"),
        serde_expected(&format!(
            "{HEADER}main:\n  fields:\n    f: {{ type: text, ui: {{ zz: 1 }} }}\n"
        ))
    );
    let layouts: BTreeSet<String> =
        schema["definitions"]["field_ui"]["properties"]["layout"]["enum"]
            .as_array()
            .expect("layout is an enum")
            .iter()
            .map(|v| v.as_str().expect("a layout is a string").to_string())
            .collect();
    assert_eq!(
        layouts,
        serde_expected(&format!(
            "{HEADER}main:\n  fields:\n    f: {{ type: text, ui: {{ layout: zz }} }}\n"
        ))
    );

    for backend in schema["definitions"]["quill"]["properties"]["backend"]["enum"]
        .as_array()
        .expect("backend is an enum")
    {
        let backend = backend.as_str().expect("a backend id is a string");
        let section = &schema["properties"][backend];
        let declared: BTreeSet<String> = section["properties"]
            .as_object()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();
        let read = crate::backend::config_keys(backend)
            .unwrap_or_else(|| panic!("`{backend}` is a backend this build renders"));
        assert_eq!(declared, set(read), "the `{backend}:` section");
    }
}

#[test]
fn the_schema_types_are_the_ones_the_loader_loads() {
    let validator = validator();
    let declared: BTreeSet<String> = schema()["definitions"]["field"]["properties"]["type"]["enum"]
        .as_array()
        .expect("type is an enum")
        .iter()
        .map(|v| v.as_str().expect("a type token is a string").to_string())
        .collect();
    let mut loaded = BTreeSet::new();
    for name in FieldType::NAMES {
        for token in [name.to_string(), format!("{name}?")] {
            let yaml = minimal_field(&token);
            let loads = QuillConfig::from_yaml_with_warnings(&yaml).is_ok();
            assert_eq!(
                validator.is_valid(&instance(&yaml)),
                loads,
                "`type: {token}`: loader and schema disagree"
            );
            if loads {
                loaded.insert(token);
            }
        }
    }
    assert_eq!(declared, loaded);
}

#[test]
fn the_schema_scopes_each_key_to_the_types_the_loader_takes_it_on() {
    let validator = validator();
    let scoped = [
        ("values", "[a]"),
        ("variants", "{ a: { g: { type: text } } }"),
        ("items", "{ type: text }"),
        ("properties", "{ a: { type: text } }"),
        ("multiline", "true"),
        ("max", "3"),
        ("members", "{ a: A }"),
        ("open", "true"),
    ];
    for name in FieldType::NAMES {
        let base = minimal_field(name);
        for (key, value) in scoped {
            if base.contains(&format!(" {key}: ")) {
                continue;
            }
            let yaml = base.replace(" }\n", &format!(", {key}: {value} }}\n"));
            assert_eq!(
                validator.is_valid(&instance(&yaml)),
                QuillConfig::from_yaml_with_warnings(&yaml).is_ok(),
                "`{key}` on `type: {name}`: loader and schema disagree\n{yaml}"
            );
        }
    }
}

#[test]
fn the_schema_accepts_every_fixture_quill() {
    let validator = validator();
    let mut checked = 0;
    for quill in std::fs::read_dir(FIXTURE_QUILLS).expect("the fixture quills") {
        for version in std::fs::read_dir(quill.expect("a quill").path()).expect("its versions") {
            let path = version.expect("a version").path().join("Quill.yaml");
            let yaml = std::fs::read_to_string(&path).expect("a Quill.yaml");
            QuillConfig::from_yaml_with_warnings(&yaml)
                .unwrap_or_else(|e| panic!("{} loads: {e:?}", path.display()));
            let refusals: Vec<String> = validator
                .iter_errors(&instance(&yaml))
                .map(|e| format!("{}: {e}", e.instance_path()))
                .collect();
            assert!(
                refusals.is_empty(),
                "{} loads but the schema refuses it: {refusals:#?}",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no fixture quill found under {FIXTURE_QUILLS}");
}

fn markdown_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a docs directory") {
        let path = entry.expect("a docs entry").path();
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

#[test]
fn every_guide_example_naming_the_schema_loads_and_validates() {
    let validator = validator();
    let mut pages = Vec::new();
    markdown_files(std::path::Path::new(DOCS), &mut pages);
    let mut checked = 0;
    for page in pages {
        let text = std::fs::read_to_string(&page).expect("a docs page");
        for block in text.split("```yaml\n").skip(1) {
            let Some(yaml) = block
                .split("```")
                .next()
                .filter(|b| b.starts_with(MODELINE) && *b != MODELINE)
            else {
                continue;
            };
            let at = page.display();
            QuillConfig::from_yaml_with_warnings(yaml)
                .unwrap_or_else(|e| panic!("an example in {at} loads: {e:?}"));
            let refusals: Vec<String> = validator
                .iter_errors(&instance(yaml))
                .map(|e| format!("{}: {e}", e.instance_path()))
                .collect();
            assert!(
                refusals.is_empty(),
                "an example in {at} loads but the schema refuses it: {refusals:#?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no guide example names the schema");
}

#[test]
fn the_schema_refuses_the_shape_errors_the_loader_refuses() {
    let validator = validator();
    let field = |spec: &str| format!("{HEADER}main:\n  fields:\n    f: {{ {spec} }}\n");
    let cases = [
        field("type: Date"),
        field("type: string"),
        field("type: \"object?\", properties: { a: { type: text } }"),
        field("type: text, required: true"),
        field("type: text, ui: { title: To }"),
        field("type: text, ui: { multiline: true }"),
        field("type: text, ui: { layout: grid }"),
        field("type: enum"),
        field("type: enum, values: [\"\"]"),
        field("type: array"),
        field("type: object"),
        field("type: matrix"),
        field("type: richtext"),
        field("type: text, items: { type: text }"),
        field("type: text, values: [a]"),
        field("type: text, variants: { a: {} }"),
        field("type: number, multiline: true"),
        field("type: number, max: 3"),
        field("type: text, members: { a: A }"),
        field("type: enum, values: [a], open: true"),
        format!("{HEADER}main:\n  fields:\n    Subject: {{ type: text }}\n"),
        format!("{HEADER}main:\n  fields:\n    body: {{ type: text }}\n"),
        format!("{HEADER}main:\n  seed: {{}}\n"),
        format!("{HEADER}main:\n  body: {{ hidden: true }}\n"),
        format!("{HEADER}main:\n  ui: {{ title: Memo }}\n"),
        format!("{HEADER}card_kinds:\n  main: {{}}\n"),
        format!("{HEADER}card_kinds:\n  Note: {{}}\n"),
        format!("{HEADER}fields: {{}}\n"),
        format!("{HEADER}typst:\n  packages: []\n"),
        format!("{HEADER}  colour: blue\n"),
        format!("{HEADER}  requires: \">=1\"\n"),
        HEADER.replace("backend: typst", "backend: acroform") + "typst: {}\n",
        HEADER.replace("name: memo", "name: MyMemo"),
        HEADER.replace("\"1.0.0\"", "1.0"),
        HEADER.replace("\"1.0.0\"", "\"1.0\""),
        HEADER.replace("  description: A memo\n", ""),
        "main: {}\n".to_string(),
    ];
    for yaml in cases {
        assert!(
            QuillConfig::from_yaml_with_warnings(&yaml).is_err(),
            "the loader refuses:\n{yaml}"
        );
        assert!(
            !validator.is_valid(&instance(&yaml)),
            "the schema refuses:\n{yaml}"
        );
    }
}
