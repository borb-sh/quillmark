mod matrix_tests;
mod optional_tests;
mod properties;
mod support_tests;
mod today_tests;
mod variant_tests;

use super::*;
use crate::{document::Document, error::{Diagnostic, Severity}, value::QuillValue};
use std::collections::HashMap;
use std::path::PathBuf;

/// The minimal `Quill.yaml` a load needs, with `name` spelled in.
fn manifest(name: &str) -> Vec<u8> {
    format!("quill:\n  name: {name}\n  version: \"1.0\"\n  backend: typst\n  description: {name}\n")
        .into_bytes()
}

/// A quill tree from `(path, contents)` pairs. A path nests on `/`, and one
/// ending in `/` is an empty directory. Core is filesystem-agnostic, so every
/// load here is a tree; path loading lives in `quillmark::quill_from_path` and
/// is tested beside it.
fn tree(entries: &[(&str, &[u8])]) -> FileTreeNode {
    let mut root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    for &(path, contents) in entries {
        let node = match path.ends_with('/') {
            true => FileTreeNode::Directory {
                files: HashMap::new(),
            },
            false => FileTreeNode::File {
                contents: contents.to_vec(),
            },
        };
        root.insert(path.trim_end_matches('/'), node)
            .expect("insert");
    }
    root
}

fn quill_from(entries: &[(&str, &[u8])]) -> Quill {
    Quill::from_tree(tree(entries)).expect("load quill")
}

/// `sections` under the four required `quill:` keys. Only a test *about* the
/// header spells its own.
fn with_header(sections: &str) -> String {
    format!("quill:\n  name: q\n  version: \"1.0\"\n  backend: typst\n  description: q\n{sections}")
}

/// A config from `sections`, with the load's advisory warnings dropped.
fn config_with_sections(sections: &str) -> Result<QuillConfig, Vec<Diagnostic>> {
    QuillConfig::from_yaml_with_warnings(&with_header(sections)).map(|(c, _)| c)
}

/// A config declaring one `main` field, the shape most parse tests want.
fn quill_with_field(field_yaml: &str) -> Result<QuillConfig, Vec<Diagnostic>> {
    config_with_sections(&format!("main:\n  fields:\n{field_yaml}"))
}

#[test]
fn the_ignore_set_anchors_directories_and_not_names() {
    let ignore = QuillIgnore;

    assert!(ignore.is_ignored("target"));
    assert!(ignore.is_ignored("target/debug/deps"));
    assert!(ignore.is_ignored(".git/HEAD"));
    assert!(!ignore.is_ignored("src/target.rs"));
    assert!(!ignore.is_ignored("packages/vendor/node_modules"));
    assert!(!ignore.is_ignored("my_node_modules"));

    assert!(ignore.is_ignored(".gitignore"));
    assert!(ignore.is_ignored("packages/vendor/.gitignore"));
    assert!(!ignore.is_ignored("plate.typ"));
}

#[test]
fn test_find_files_pattern() {
    let quill = quill_from(&[
        ("Quill.yaml", &manifest("find_files")),
        ("plate.typ", b"template"),
        ("assets/image.png", b"png data"),
        ("assets/data.json", b"json data"),
        ("assets/fonts/font.ttf", b"font data"),
    ]);

    assert!(quill.files().find_files("assets/*").len() >= 3);

    let typ_files = quill.files().find_files("*.typ");
    assert_eq!(typ_files, vec![PathBuf::from("plate.typ")]);
}

/// The advisory channel reaches whoever holds the quill. `from_tree` is the
/// door every binding takes, and a warning that only the loader's return value
/// carried was a warning no binding host could ever read.
#[test]
fn a_config_warning_rides_the_loaded_quill() {
    let quill = quill_from(&[(
        "Quill.yaml",
        br#"quill: { name: warn, version: "1.0", backend: typst, description: w }
main:
  fields:
    title: { type: string }
card_kinds:
  skills:
    body:
      enabled: false
      example: This example is unused
    fields:
      items: { type: array, items: { type: string } }
"#,
    )]);

    assert_eq!(
        quill
            .warnings()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect::<Vec<_>>(),
        ["quill::body_example_unused", "quill::bodiless_card_kind"]
    );
}

/// An empty directory is a tree the flat form cannot carry, so it is the one
/// shape the round trip has to state.
#[test]
fn test_to_tree_round_trips_from_tree() {
    let yaml = manifest("roundtrip");
    let asset = b"\x00\x01\x02 binary asset";
    let quill = quill_from(&[
        ("Quill.yaml", &yaml),
        ("plate.typ", b"= Plate"),
        ("assets/logo.bin", asset),
        ("empty/", b""),
    ]);

    let flat = quill.to_tree();
    assert_eq!(
        flat,
        vec![
            ("Quill.yaml".to_string(), yaml),
            ("assets/logo.bin".to_string(), asset.to_vec()),
            ("plate.typ".to_string(), b"= Plate".to_vec()),
        ]
    );

    let mut rebuilt_root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    for (path, contents) in flat {
        rebuilt_root
            .insert(&path, FileTreeNode::File { contents })
            .unwrap();
    }
    let rebuilt = Quill::from_tree(rebuilt_root).unwrap();
    assert_eq!(rebuilt.name(), quill.name());
    assert_eq!(rebuilt.to_tree(), quill.to_tree());
}

#[test]
fn test_list_directories() {
    let quill = quill_from(&[
        ("Quill.yaml", &manifest("list_dirs")),
        ("plate.typ", b"plate content"),
        ("assets/logo.png", &[137, 80, 78, 71]),
        ("assets/fonts/font.ttf", b"font data"),
        ("empty/", b""),
    ]);

    let mut root_dirs = quill.files().list_directories("");
    root_dirs.sort();
    assert_eq!(
        root_dirs,
        vec![PathBuf::from("assets"), PathBuf::from("empty")]
    );

    assert_eq!(
        quill.files().list_directories("assets"),
        vec![PathBuf::from("assets/fonts")]
    );

    assert!(quill.files().list_directories("empty").is_empty());
    assert!(quill.files().list_directories("plate.typ").is_empty()); // file, not directory
    assert!(quill.files().list_directories("nonexistent").is_empty());
}

#[test]
fn test_quill_config_from_yaml() {
    let yaml_content = r#"
quill:
  name: test_config
  version: "1.0"
  backend: typst
  description: Test configuration parsing
  author: Test Author

typst:
  plate_file: plate.typ
  packages:
    - "@preview/bubble:0.2.2"

main:
  fields:
    title:
      description: Document title
      type: string
    author:
      type: string
      description: Document author
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    assert_eq!(config.name, "test_config");
    assert_eq!(config.main.name, "main");
    assert_eq!(config.backend, "typst");
    assert_eq!(config.description, "Test configuration parsing");
    assert_eq!(config.main.description, None);

    assert_eq!(config.version, "1.0");
    assert_eq!(config.author, "Test Author");

    assert_eq!(
        config
            .backend_config
            .get("plate_file")
            .and_then(|v| v.as_str()),
        Some("plate.typ")
    );
    assert!(config.backend_config.contains_key("packages"));

    assert_eq!(config.main.fields.len(), 2);
    assert!(config.main.fields.contains_key("title"));
    assert!(config.main.fields.contains_key("author"));

    let title_field = &config.main.fields["title"];
    assert_eq!(title_field.description, Some("Document title".to_string()));
    assert_eq!(title_field.r#type, FieldType::String);
}

#[test]
fn an_unquoted_numeric_version_reads_as_written() {
    let load = |version: &str| {
        QuillConfig::from_yaml_with_warnings(&format!(
            "quill:\n  name: numeric_version\n  version: {version}\n  backend: typst\n  description: Numeric version\n"
        ))
    };
    for version in ["1.0", "1.10"] {
        assert_eq!(load(version).expect(version).0.version, version);
    }
    let errors = load("1.1e1").expect_err("an exponent is not a version");
    assert!(errors.iter().any(|d| d.code.as_deref() == Some("quill::invalid_version")));
}

/// The header is the one section the loader reads before anything else, so its
/// own defects are named rather than reported as a missing config.
#[test]
fn a_defective_quill_header_names_what_it_is_missing() {
    for (yaml, code) in [
        ("fields:\n  title:\n    description: Title\n", "quill::missing_section"),
        ("quill:\n  backend: typst\n  description: d\n", "quill::missing_name"),
        ("quill:\n  name: test\n  description: d\n", "quill::missing_backend"),
        (
            "quill:\n  name: test\n  version: \"1.0\"\n  backend: typst\n",
            "quill::missing_description",
        ),
        (
            "quill:\n  name: test\n  version: \"1.0\"\n  backend: typst\n  description: \"   \"\n",
            "quill::empty_description",
        ),
    ] {
        let errs = QuillConfig::from_yaml_with_warnings(yaml).expect_err(code);
        assert!(errs.iter().any(|d| d.code.as_deref() == Some(code)), "{code}: {errs:?}");
    }
}

/// snake_case is the one identifier grammar, whatever declares the name.
#[test]
fn test_quill_config_rejects_non_snake_case_identifiers() {
    for (yaml, code) in [
        (
            "quill:\n  name: BadQuill\n  version: \"1.0\"\n  backend: typst\n  description: q\n".to_string(),
            "quill::invalid_name",
        ),
        (
            with_header("card_kinds:\n  BadCard:\n    fields:\n      title: { type: string }\n"),
            "quill::invalid_card_name",
        ),
        (
            with_header("main:\n  fields:\n    BadField: { type: string }\n"),
            "quill::invalid_field_name",
        ),
        (
            with_header("card_kinds:\n  profile:\n    fields:\n      DisplayName: { type: string }\n"),
            "quill::invalid_field_name",
        ),
    ] {
        let errs = QuillConfig::from_yaml_with_warnings(&yaml).expect_err(code);
        assert!(errs.iter().any(|d| d.code.as_deref() == Some(code)), "{code}: {errs:?}");
    }
    assert!(QuillConfig::from_yaml(&with_header(
        "card_kinds:\n  _private_card:\n    fields:\n      title: { type: string }\n"
    ))
    .is_ok());
}

#[test]
fn test_invalid_quill_name_hint_suggests_a_valid_name() {
    for (name, suggestion) in [
        ("My-Quill", Some("my_quill")),
        ("_private", Some("private")),
        ("2nd.form", Some("nd_form")),
        ("__", None),
    ] {
        let yaml = format!(
            "quill:\n  name: \"{name}\"\n  version: \"1.0\"\n  backend: typst\n  description: q\n"
        );
        let errs = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
        let diag = errs
            .iter()
            .find(|d| d.code.as_deref() == Some("quill::invalid_name"))
            .unwrap_or_else(|| panic!("{name} is not a valid quill name"));
        let expected = suggestion.map(|s| format!("Rename '{name}' to '{s}'"));
        assert_eq!(diag.hint, expected, "hint for {name}");
        if let Some(s) = suggestion {
            let renamed = yaml.replace(&format!("\"{name}\""), s);
            assert!(QuillConfig::from_yaml(&renamed).is_ok(), "{s} loads");
        }
    }
}

#[test]
fn test_config_defaults_method() {
    let yaml_content = &with_header(r#"main:
  fields:
    author:
      type: string
      default: Anonymous
    status:
      type: string
      default: draft
    title:
      type: string
"#);

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let defaults = config.main.defaults();

    assert_eq!(defaults.len(), 2);
    assert_eq!(defaults.get("author").unwrap().as_str(), Some("Anonymous"));
    assert_eq!(defaults.get("status").unwrap().as_str(), Some("draft"));
    assert!(!defaults.contains_key("title"));
}

#[test]
fn test_parse_card_with_fields_in_yaml() {
    let yaml_content = &with_header(r#"card_kinds:
  endorsements:
    description: Chain of endorsements
    fields:
      name:
        type: string
      org:
        type: string
        default: Unknown
  myscope:
    description: My scope
"#);

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let card = config.card_kind("endorsements").unwrap();
    assert_eq!(card.name, "endorsements");
    assert_eq!(card.description, Some("Chain of endorsements".to_string()));
    let names: Vec<&str> = card.fields.keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["name", "org"]);
    assert_eq!(card.defaults().get("org").and_then(|v| v.as_str()), Some("Unknown"));

    assert!(config.card_kind("myscope").unwrap().fields.is_empty());
}

/// A card schema is bounded by what one card-yaml block carries, so the
/// blueprint it drives spells every field and re-parses.
#[test]
fn a_card_declaring_more_fields_than_a_block_carries_is_refused_at_load() {
    let max = crate::error::MAX_FIELD_COUNT;
    let yaml = |count: usize| {
        let fields: String = (0..count)
            .map(|i| format!("      f{i}: {{ type: string, example: v }}\n"))
            .collect();
        format!(
            "quill: {{ name: wide, version: \"1.0\", backend: typst, description: x }}\n\
             card_kinds:\n  line_item:\n    fields:\n{fields}"
        )
    };

    let errors = QuillConfig::from_yaml_with_warnings(&yaml(max + 1)).unwrap_err();
    let diag = errors
        .iter()
        .find(|d| d.code.as_deref() == Some("quill::too_many_fields"))
        .unwrap_or_else(|| panic!("no field-count diagnostic in {errors:?}"));
    assert!(
        diag.message.contains("card_kinds.line_item"),
        "the diagnostic names the card the author wrote: {}",
        diag.message
    );

    let blueprint = QuillConfig::from_yaml(&yaml(max)).expect("loads").blueprint();
    let reparsed = Document::parse(&blueprint)
        .expect("a blueprint at the cap re-parses")
        .document;
    assert_eq!(reparsed.cards()[0].payload().len(), max);
}

#[test]
fn test_quill_config_allows_card_collision() {
    let yaml_content = &with_header(r#"main:
  fields:
    conflict:
      description: Field
      type: string

card_kinds:
  conflict:
    description: Card
"#);

    let config = QuillConfig::from_yaml(yaml_content).expect("a card may share a field's name");
    assert!(config.main.fields.contains_key("conflict"));
    assert!(config.card_kind("conflict").is_some());
}

/// A type means the same thing wherever it is declared: every container nests
/// in every other, and properties keep declaration order.
#[test]
fn containers_nest_at_every_position() {
    let config = config_with_sections(
        r#"main:
  fields:
    rows:
      type: array
      items:
        type: object
        properties:
          zulu: { type: number }
          alpha:
            type: object
            properties:
              inner: { type: string }
    grid:
      type: array
      items:
        type: array
        items: { type: integer }
    address:
      type: object
      properties:
        lines:
          type: array
          items: { type: string }
"#,
    )
    .expect("loads");

    let row = config.main.fields["rows"].items.as_ref().expect("items");
    assert_eq!(row.r#type, FieldType::Object);
    let props = row.properties.as_ref().expect("properties");
    assert_eq!(props.keys().map(String::as_str).collect::<Vec<_>>(), ["zulu", "alpha"]);
    assert_eq!(props["zulu"].r#type, FieldType::Number);
    assert!(props["alpha"].properties.as_ref().expect("properties").contains_key("inner"));

    let grid_row = config.main.fields["grid"].items.as_ref().expect("items");
    assert_eq!(grid_row.items.as_ref().expect("items").r#type, FieldType::Integer);

    let lines = &config.main.fields["address"].properties.as_ref().expect("properties")["lines"];
    assert_eq!(lines.items.as_ref().expect("items").r#type, FieldType::String);
}

#[test]
fn a_malformed_container_declaration_is_refused_by_code() {
    for (field, code) in [
        ("    m:\n      type: object\n", "quill::object_missing_properties"),
        ("    m:\n      type: object\n      properties: {}\n", "quill::object_empty_properties"),
        (
            "    m:\n      type: array\n      items:\n        type: object\n        properties: {}\n",
            "quill::object_empty_properties",
        ),
        ("    m:\n      type: array\n", "quill::array_missing_items"),
        (
            "    m:\n      type: array\n      properties:\n        org: { type: string }\n",
            "quill::array_properties_not_supported",
        ),
    ] {
        let err = quill_with_field(field).expect_err(code);
        assert!(err.iter().any(|d| d.code.as_deref() == Some(code)), "{code}: {err:?}");
    }
}

#[test]
fn group_registry_list_form_orders_blueprint_by_declaration() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups: [beta, alpha]
  fields:
    a1: { type: string, ui: { group: alpha } }
    b1: { type: string, ui: { group: beta } }
"#;
    let bp = QuillConfig::from_yaml(yaml).unwrap().blueprint();
    let a = bp.find("a1:").unwrap();
    let b = bp.find("b1:").unwrap();
    assert!(b < a, "registry order (beta<alpha) must drive clustering:\n{bp}");
}

#[test]
fn group_registry_map_form_carries_title_override() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups:
      addressing: {}
      letterhead: { title: "Letterhead & Seal" }
  fields:
    a: { type: string, ui: { group: addressing } }
    l: { type: string, ui: { group: letterhead } }
"#;
    let config = QuillConfig::from_yaml(yaml).unwrap();
    let reg = &config.main.ui.as_ref().unwrap().groups.as_ref().unwrap().0;
    assert_eq!(reg.len(), 2);
    assert_eq!(reg[0].id, "addressing");
    assert_eq!(reg[0].title, None);
    assert_eq!(reg[1].id, "letterhead");
    assert_eq!(reg[1].title.as_deref(), Some("Letterhead & Seal"));
}

#[test]
fn a_bad_group_declaration_is_refused_by_code() {
    for (main, code) in [
        (
            "  ui:\n    groups: [addressing]\n  fields:\n    s: { type: string, ui: { group: letterhead } }\n",
            "quill::unknown_group",
        ),
        (
            "  fields:\n    s: { type: string, ui: { group: addressing } }\n",
            "quill::implicit_group",
        ),
        (
            "  ui:\n    groups: [addressing, addressing]\n  fields:\n    s: { type: string, ui: { group: addressing } }\n",
            "quill::duplicate_group",
        ),
        (
            "  ui:\n    groups: [Addressing]\n  fields:\n    s: { type: string, ui: { group: Addressing } }\n",
            "quill::invalid_group_id",
        ),
        (
            "  ui:\n    groups: [location]\n  fields:\n    a:\n      type: object\n      properties:\n        s: { type: string, ui: { group: location } }\n",
            "quill::nested_group_not_supported",
        ),
    ] {
        let err = config_with_sections(&format!("main:\n{main}")).expect_err(code);
        assert!(err.iter().any(|d| d.code.as_deref() == Some(code)), "{code}: {err:?}");
    }
}

#[test]
fn group_registry_round_trips_through_serde_and_schema() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups: [addressing, letterhead]
  fields:
    a: { type: string, ui: { group: addressing } }
    l: { type: string, ui: { group: letterhead } }
"#;
    let config = QuillConfig::from_yaml(yaml).unwrap();
    let ui = config.main.ui.clone().unwrap();
    let json = serde_json::to_value(&ui).unwrap();
    let back: UiCardSchema = serde_json::from_value(json).unwrap();
    assert_eq!(ui, back, "registry must survive emit → parse");

    let schema = config.schema();
    let groups = schema["main"]["ui"]["groups"].as_object().unwrap();
    let keys: Vec<&str> = groups.keys().map(String::as_str).collect();
    assert_eq!(keys, ["addressing", "letterhead"]);
    assert!(groups["addressing"].as_object().unwrap().is_empty());
}

/// One field's coercion: `field_yaml` declares it as `f`, `value` is what a
/// document carries there. `Ok` is the coerced JSON; `Err` is the `(path,
/// target)` pair a refusal names, which is what a caller routes on.
fn coerce(
    field_yaml: &str,
    value: serde_json::Value,
) -> Result<serde_json::Value, (String, String)> {
    let config = quill_with_field(field_yaml).expect("the declaration loads");
    let mut payload = indexmap::IndexMap::new();
    payload.insert("f".to_string(), QuillValue::from_json(value));
    match config.coerce_payload(&payload) {
        Ok(coerced) => Ok(coerced.get("f").expect("the field survives").as_json().clone()),
        Err(super::CoercionError::Uncoercible { path, target, .. }) => Err((path, target)),
    }
}

/// Coercion is by declared type: a scalar takes the type's canonical form, a
/// container coerces element- and property-wise, and a value no form admits
/// names the path and the target it failed at.
#[test]
fn a_document_value_coerces_by_declared_type_or_names_where_it_could_not() {
    use serde_json::json;

    const INTS: &str = "    f:\n      type: array\n      items: { type: integer }\n";
    const ROWS: &str = "    f:\n      type: array\n      items:\n        type: object\n        properties:\n          name: { type: string }\n          value: { type: number }\n          active: { type: boolean }\n";

    for (what, field, value, want) in [
        ("number", "    f: { type: number }\n", json!("42"), json!(42)),
        (
            "boolean",
            "    f: { type: boolean }\n",
            json!("true"),
            json!(true),
        ),
        (
            "date",
            "    f: { type: date }\n",
            json!("2026-04-13"),
            json!("2026-04-13"),
        ),
        (
            "datetime",
            "    f: { type: datetime }\n",
            json!("2026-04-13T20:00:00"),
            json!("2026-04-13T20:00:00"),
        ),
        // A bare scalar into a string takes the type's canonical token.
        (
            "string from a bool",
            "    f: { type: string }\n",
            json!(true),
            json!("true"),
        ),
        (
            "string from an integer",
            "    f: { type: string }\n",
            json!(47),
            json!("47"),
        ),
        (
            "string from a float",
            "    f: { type: string }\n",
            json!(1.5),
            json!("1.5"),
        ),
        ("a scalar array", INTS, json!(["1", "2"]), json!([1, 2])),
        (
            "a row's properties",
            ROWS,
            json!([{ "name": "Math", "value": "95", "active": "true" }]),
            json!([{ "name": "Math", "value": 95, "active": true }]),
        ),
    ] {
        assert_eq!(coerce(field, value), Ok(want), "{what}");
    }

    for (what, field, value, path, target) in [
        (
            "a decimal is not an integer",
            "    f: { type: integer }\n",
            json!("42.5"),
            "f",
            "integer",
        ),
        (
            "an unparseable datetime",
            "    f: { type: datetime }\n",
            json!("13-04-2026"),
            "f",
            "datetime",
        ),
        // The date grammar itself is `formats::parse_date`'s; what is here is
        // that coercion reaches it.
        (
            "a time component in a date",
            "    f: { type: date }\n",
            json!("2026-04-13T12:00"),
            "f",
            "date",
        ),
        // The element's own index, not the array's name.
        ("a bad element", INTS, json!([1, "nope"]), "f[1]", "integer"),
    ] {
        assert_eq!(
            coerce(field, value),
            Err((path.to_string(), target.to_string())),
            "{what}"
        );
    }
}

/// A card's fields take the same walk through the card-kind's own schema.
#[test]
fn test_config_coerce_cards_item_wise() {
    let yaml_content = &with_header(r#"card_kinds:
  indorsement:
    fields:
      score: { type: number }
      active: { type: boolean }
"#);
    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let card_fields = indexmap::IndexMap::from([
        ("score".to_string(), QuillValue::from_json(serde_json::json!("100"))),
        ("active".to_string(), QuillValue::from_json(serde_json::json!("false"))),
    ]);

    let coerced = config.coerce_card("indorsement", &card_fields).unwrap();
    assert_eq!(coerced.get("score").unwrap().as_i64(), Some(100));
    assert_eq!(coerced.get("active").unwrap().as_bool(), Some(false));
}

#[test]
fn test_card_ui_title_parses_literal_and_template_forms() {
    let yaml_content = &with_header(r#"main:
  ui:
    title: Memorandum
  fields:
    subject:
      type: string
      ui:
        title: Status Label

card_kinds:
  indorsement:
    ui:
      title: "{from} → {for}"
    fields:
      from:
        type: string
      for:
        type: string
"#);

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    assert_eq!(
        config.main.ui.as_ref().unwrap().title.as_deref(),
        Some("Memorandum"),
        "literal main.ui.title"
    );
    let indorsement = config.card_kind("indorsement").unwrap();
    assert_eq!(
        indorsement.ui.as_ref().unwrap().title.as_deref(),
        Some("{from} → {for}"),
        "template card ui.title carried verbatim"
    );

    let schema = config.schema();
    assert_eq!(schema["main"]["ui"]["title"].as_str(), Some("Memorandum"));
    assert_eq!(
        schema["card_kinds"]["indorsement"]["ui"]["title"].as_str(),
        Some("{from} → {for}")
    );
    assert_eq!(
        schema["main"]["fields"]["subject"]["ui"]["title"].as_str(),
        Some("Status Label")
    );
}

#[test]
fn test_unknown_key_in_quill_section_errors() {
    let yaml_content = &with_header(r#"  auther: Jane Doe
"#);

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code.as_deref(), Some("quill::unknown_key"));
    assert!(err[0].message.contains("auther"));
    assert!(err[0].hint.as_deref().unwrap_or("").contains("author"));
}

#[test]
fn test_root_level_fields_gets_targeted_hint() {
    let yaml_content = &with_header(r#"fields:
  author:
    type: string
"#);

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    let fields_errors: Vec<&Diagnostic> = err
        .iter()
        .filter(|d| d.message.contains("fields"))
        .collect();
    assert_eq!(
        fields_errors.len(),
        1,
        "expected exactly one error for root-level `fields`, got {} ({:?})",
        fields_errors.len(),
        fields_errors
    );
    assert_eq!(
        fields_errors[0].code.as_deref(),
        Some("quill::unknown_section")
    );
    assert!(fields_errors[0]
        .hint
        .as_deref()
        .unwrap_or("")
        .contains("main.fields"));
}

#[test]
fn test_multiple_errors_collected_in_one_pass() {
    let yaml_content = r#"
quill:
  name: BadName
  version: "1.0"
  backend: typst
  description: Multi-error test
  platefile: foo.typ

main:
  fields:
    BadFieldName:
      type: string
    legit:
      title: Bad legacy key
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();
    let codes: Vec<&str> = err.iter().filter_map(|d| d.code.as_deref()).collect();
    for code in [
        "quill::invalid_name",
        "quill::unknown_key",
        "quill::invalid_field_name",
        "quill::field_parse_error",
    ] {
        assert!(codes.contains(&code), "missing {code}: {codes:?}");
    }
}

#[test]
fn main_refuses_every_shape_a_card_kind_refuses() {
    let cases = [
        (
            "misspelled fields",
            "main:\n  feilds:\n    title:\n      type: string",
        ),
        ("unknown key", "main:\n  title: Memo"),
        ("list-shaped fields", "main:\n  fields: [title, author]"),
        ("non-mapping main", "main: [x]"),
        ("scalar main", "main: 5"),
        ("empty main", "main:"),
    ];

    for (label, sections) in cases {
        let err = config_with_sections(sections)
            .err()
            .unwrap_or_else(|| panic!("{label} loaded instead of erroring"));
        assert!(
            err.iter()
                .any(|d| d.code.as_deref() == Some("quill::invalid_card_schema")),
            "{label}: {:?}",
            err.iter().map(|d| d.code.as_deref()).collect::<Vec<_>>()
        );
    }
}

/// The hint names every key the block admits, so a misspelling finds its fix.
#[test]
fn a_malformed_ui_or_body_block_reports_its_own_code_on_main_and_card_kinds() {
    for (prefix, indent) in [("main:\n", "  "), ("card_kinds:\n  note:\n", "    ")] {
        let ui_err = config_with_sections(&format!("{prefix}{indent}ui:\n{indent}  bogus_key: nope"))
            .expect_err("a malformed ui block");
        assert!(
            ui_err.iter().any(|d| d.code.as_deref() == Some("quill::invalid_ui")),
            "{prefix}: {ui_err:?}"
        );

        let body_err =
            config_with_sections(&format!("{prefix}{indent}body:\n{indent}  unsuported: [Table]"))
                .expect_err("a malformed body block");
        let hint = body_err
            .iter()
            .find(|d| d.code.as_deref() == Some("quill::invalid_body"))
            .and_then(|d| d.hint.clone())
            .expect("a malformed body carries a hint");
        for key in ["enabled", "example", "unsupported"] {
            assert!(hint.contains(key), "{prefix}: hint omits {key}: {hint}");
        }
    }
}

#[test]
fn body_example_fence_line_is_an_error_on_main_and_card_kinds() {
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Opening paragraph.\n\n~~~card-yaml\n$kind: note\n~~~\n\nClosing paragraph."
  fields:
    title: { type: string }
card_kinds:
  note:
    body:
      example: "See below:\n~~~card-yaml\n$kind: other\n~~~\nEnd."
    fields:
      author: { type: string }
"#;
    let errors = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    let fence_labels: Vec<&str> = errors
        .iter()
        .filter(|d| d.code.as_deref() == Some("quill::body_example_contains_fence"))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        fence_labels.len(),
        2,
        "both body-example sites are guarded, got: {errors:?}"
    );
    assert!(
        fence_labels.iter().any(|m| m.contains("`main.body.example`"))
            && fence_labels
                .iter()
                .any(|m| m.contains("`card_kinds.note.body.example`")),
        "each error names its own site, got: {fence_labels:?}"
    );
}

#[test]
fn body_example_without_a_card_opener_is_accepted() {
    let yaml = "
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: \"See code:\\n\\n```rust\\nlet x = 1;\\n```\\n\\nEnd.\"
  fields:
    title: { type: string }
";
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    assert!(
        result.is_ok(),
        "a backtick code fence must not trigger a card-fence error: {result:?}"
    );
}

/// An authored literal is judged against its declaration: the type at its own
/// depth, and an enum's members.
#[test]
fn a_literal_outside_its_declaration_is_refused_by_code() {
    for (field, code) in [
        ("    f:\n      type: integer\n      example: 20.04\n", "quill::example_type_mismatch"),
        ("    f:\n      type: boolean\n      example: \"true\"\n", "quill::example_type_mismatch"),
        ("    f:\n      type: datetime\n      example: 42\n", "quill::example_type_mismatch"),
        (
            "    f:\n      type: array\n      items: { type: string }\n      example: foo\n",
            "quill::example_type_mismatch",
        ),
        (
            "    f:\n      type: array\n      items: { type: string }\n      example: [1]\n",
            "quill::example_type_mismatch",
        ),
        ("    f:\n      type: string\n      example: [one, two]\n", "quill::example_type_mismatch"),
        ("    f:\n      type: string\n      default: 20.04\n", "quill::default_type_mismatch"),
        (
            "    f:\n      type: enum\n      values: [a, b]\n      example: c\n",
            "quill::example_not_in_enum",
        ),
    ] {
        let err = quill_with_field(field).expect_err(code);
        assert!(err.iter().any(|d| d.code.as_deref() == Some(code)), "{field}: {err:?}");
    }

    let err = quill_with_field("    f:\n      type: string\n      example: 20.04\n").unwrap_err();
    assert!(
        err.iter().any(|d| d.hint.as_deref().is_some_and(|h| h.contains("\"20.04\""))),
        "an unquoted decimal under a string is hinted to its quoted form: {err:?}"
    );

    for field in [
        "    f:\n      type: string\n      example: \"20.04\"\n",
        "    f:\n      type: enum\n      values: [a, b]\n      example: a\n",
    ] {
        quill_with_field(field).expect(field);
    }
}

/// `values:` enumerates choices; `default:` is a value, and the blank is a legal
/// value that is never a choice. Both halves are pinned on one field because
/// `usaf_memo` ships this shape: rejecting a blank `default:` breaks the
/// reference quill, and admitting `""` to `values:` restores the fabricated
/// floor.
#[test]
fn enum_rejects_a_blank_value_but_accepts_a_blank_default() {
    let err = quill_with_field(
        "    classification:\n      type: enum\n      values: [\"\", UNCLASSIFIED]\n",
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.code.as_deref() == Some("quill::enum_blank_member")),
        "`\"\"` in values: should be a load error, got: {err:?}"
    );

    let config = quill_with_field(
        "    classification:\n      type: enum\n      values: [UNCLASSIFIED, CUI]\n      default: \"\"\n",
    )
    .expect("a blank default is the field's blank, not an out-of-domain member");
    let field = config.main.fields.get("classification").unwrap();
    assert_eq!(field.default.as_ref().unwrap().as_json(), &serde_json::json!(""));
    assert_eq!(
        field.r#type,
        FieldType::Enum {
            values: vec!["UNCLASSIFIED".to_string(), "CUI".to_string()]
        },
        "the declared choices carry no blank"
    );
}

/// `blank` may not lean on the loader's rejection of a declared `""`: the parse
/// half of the gate folds the `values:` list whole, and only the loader reads
/// the members.
#[test]
fn enum_blank_ignores_a_declared_blank_member() {
    let field = FieldSchema::from_quill_value(
        "f".to_string(),
        &QuillValue::from_yaml_str("type: enum\nvalues: [\"\", red]\n").unwrap(),
    )
    .unwrap();
    assert_eq!(super::blank(&field).into_json(), serde_json::json!(""));

    let reordered = FieldSchema::from_quill_value(
        "f".to_string(),
        &QuillValue::from_yaml_str("type: enum\nvalues: [red, green]\n").unwrap(),
    )
    .unwrap();
    assert_eq!(
        super::blank(&reordered).into_json(),
        serde_json::json!(""),
        "a `values:` reorder is a render no-op: the blank is not values[0]"
    );
}

/// A declaration the field grammar does not admit, including the retired keys
/// `must_fill`, `enum` and `ui.order`, which `deny_unknown_fields` refuses
/// rather than a named arm.
#[test]
fn a_declaration_outside_the_field_grammar_is_a_field_parse_error() {
    for field in [
        "    f:\n      description: no type\n",
        "    f:\n      type: markdown\n",
        "    f:\n      type: richtext(inline)\n",
        "    f:\n      type: string\n      inline: true\n",
        "    f:\n      type: enum\n",
        "    f:\n      type: string\n      values: [a, b]\n",
        "    f:\n      type: string\n      must_fill: true\n",
        "    f:\n      type: string\n      enum: [a, b]\n",
        "    f:\n      type: string\n      ui: { order: 3 }\n",
    ] {
        let err = quill_with_field(field).expect_err(field);
        assert!(
            err.iter().any(|d| d.code.as_deref() == Some("quill::field_parse_error")),
            "{field}: {err:?}"
        );
    }
}
#[test]
fn inline_richtext_example_over_one_para_is_a_load_error() {
    let err = quill_with_field(
        "    tag:\n      type: richtext\n      inline: true\n      example: \"one\\n\\ntwo\"\n",
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.code.as_deref() == Some("validation::not_inline")),
        "a two-paragraph inline example should fail load with validation::not_inline, got: {err:?}"
    );
}

/// A richtext literal is authored as markdown or as a canonical content object;
/// a bare scalar is neither, and loading refuses rather than importing the
/// scalar's text.
#[test]
fn a_bare_scalar_richtext_example_is_a_load_error() {
    let err = quill_with_field("    tag:\n      type: richtext\n      example: 47\n").unwrap_err();
    assert!(
        err.iter().any(|d| d.code.as_deref() == Some("quill::richtext_example_import")),
        "{err:?}"
    );
}

/// The four positions ride one test because the failure shape is a companion walk
/// that stops at one of them. Authoring only the containers is what drops each
/// cell to the render floor, where a missing companion blank-fills.
#[test]
fn a_nested_content_defaults_literal_reaches_the_plate_at_every_position() {
    let yaml = with_header(r#"main:
  fields:
    top:
      type: richtext
      default: "A **top** note"
    dict:
      type: object
      properties:
        note:
          type: richtext
          default: "A **dict** note"
    rows:
      type: array
      items:
        type: object
        properties:
          note:
            type: richtext
            default: "A **row** note"
    literal:
      type: array
      items:
        type: plaintext
        default: "A *literal* line"
    c:
      type: enum
      values: [CUI]
      default: ""
      variants:
        CUI:
          note:
            type: richtext
            default: "A **variant** note"
"#);
    let config = QuillConfig::from_yaml(&yaml).expect("nested defaults load");
    let document = Document::parse(concat!(
        "~~~\n",
        "$quill: nested_default@1.0\n",
        "$kind: main\n",
        "dict: {}\n",
        "rows:\n  - {}\n",
        "literal: [null]\n",
        "c:\n  value: CUI\n",
        "~~~\n",
    ))
    .expect("parses")
    .document;
    let plate = config.compile_data(&document, None).expect("compiles");

    for (path, cell, text) in [
        ("top", &plate["top"], "A top note"),
        ("dict.note", &plate["dict"]["note"], "A dict note"),
        ("rows.0.note", &plate["rows"][0]["note"], "A row note"),
        ("c.note", &plate["c"]["note"], "A variant note"),
    ] {
        assert_eq!(
            cell["text"].as_str(),
            Some(text),
            "{path} carries its own default: {plate}"
        );
        assert_eq!(
            cell["marks"][0]["type"], "strong",
            "{path} keeps the default's marks: {plate}"
        );
    }
    // A `plaintext` leaf is the other content codec, and its literal imports
    // verbatim: the asterisks are text, not emphasis.
    let literal = &plate["literal"][0];
    assert_eq!(literal["text"].as_str(), Some("A *literal* line"));
    assert_eq!(literal["marks"], serde_json::json!([]));
}

/// The sibling position: a `default:` reached *through* an absent container. A
/// cell's literal commits the same imported content whatever sits above it, and
/// an `array` — the one container that is itself a cell — commits its own.
#[test]
fn a_content_default_inside_an_absent_container_reaches_the_plate_as_content() {
    let yaml = with_header(r#"main:
  fields:
    dict:
      type: object
      properties:
        note:
          type: richtext
          default: "A **dict** note"
    rows:
      type: array
      default: ["A **row** note"]
      items:
        type: richtext
    plain:
      type: object
      properties:
        tag:
          type: string
          default: bare
"#);
    let config = QuillConfig::from_yaml(&yaml).expect("container defaults load");
    let document = Document::parse(concat!(
        "~~~\n",
        "$quill: container_default@1.0\n",
        "$kind: main\n",
        "~~~\n",
    ))
    .expect("parses")
    .document;
    let plate = config.compile_data(&document, None).expect("compiles");

    for (path, cell, text) in [
        ("dict.note", &plate["dict"]["note"], "A dict note"),
        ("rows.0", &plate["rows"][0], "A row note"),
    ] {
        assert_eq!(
            cell["text"].as_str(),
            Some(text),
            "{path} is canonical content, not the raw markdown: {plate}"
        );
        assert_eq!(
            cell["marks"][0]["type"], "strong",
            "{path} keeps the default's marks: {plate}"
        );
    }
    // A cell bearing no content has no companion to read, and its `default:`
    // still crosses verbatim through the absent container above it.
    assert_eq!(plate["plain"]["tag"], serde_json::json!("bare"));
}

/// Importing a literal is what checks it, so the nested gate is the companion walk
/// reaching the leaf rather than a validation pass of its own. The diagnostic must
/// name the leaf's declaration path: the card field holding it is not the mistake.
#[test]
fn a_nested_inline_richtext_default_over_one_para_is_a_load_error() {
    let err = quill_with_field(concat!(
        "    dict:\n",
        "      type: object\n",
        "      properties:\n",
        "        tag:\n",
        "          type: richtext\n",
        "          inline: true\n",
        "          default: \"one\\n\\ntwo\"\n",
    ))
    .unwrap_err();
    assert!(
        err.iter().any(|d| {
            d.code.as_deref() == Some("validation::not_inline")
                && d.message.contains("`dict.tag`")
        }),
        "the nested literal fails load, naming the leaf: {err:?}"
    );
}

#[test]
fn array_of_inline_richtext_caches_each_element() {
    let config = quill_with_field(
        "    refs:\n      type: array\n      items:\n        type: richtext\n        inline: true\n      default:\n        - \"first *ref*\"\n        - \"second ref\"\n",
    )
    .expect("array<inline richtext> loads");
    let field = config.main.fields.get("refs").unwrap();
    let content = field
        .default_content
        .as_ref()
        .expect("default_content cached");
    let arr = content.as_json().as_array().expect("array of content");
    assert_eq!(arr.len(), 2);
    assert!(
        arr.iter().all(|e| e.is_object()),
        "each element is a content object"
    );
}

/// `inline` at the coercion layer, for both content codecs: one block imports
/// to content, more is refused naming the declared type.
#[test]
fn inline_coercion_takes_one_block_and_refuses_more_for_both_content_types() {
    for ty in ["richtext", "plaintext"] {
        let field = format!("    f:\n      type: {ty}\n      inline: true\n");
        assert!(coerce(&field, serde_json::json!("just one line")).is_ok_and(|v| v.is_object()));
        assert_eq!(
            coerce(&field, serde_json::json!("one\n\ntwo")),
            Err(("f".to_string(), format!("{ty}(inline)")))
        );
    }
}

#[test]
fn richtext_blank_is_empty_content() {
    let field = FieldSchema::new("x".to_string(), FieldType::RichText { inline: false }, None);
    let floor = blank(&field);
    assert!(
        floor.as_json().is_object(),
        "richtext blank is the empty content, not a string: {:?}",
        floor.as_json()
    );
}

#[test]
fn plaintext_field_caches_literal_content() {
    let config = quill_with_field(
        "    subject:\n      type: plaintext\n      default: \"a *literal* subject\"\n",
    )
    .expect("plaintext default loads");
    let field = config.main.fields.get("subject").unwrap();
    assert_eq!(field.r#type, FieldType::PlainText { inline: false });
    let content = field.default_content.as_ref().expect("default_content cached");
    let rt = quillmark_content::serial::from_canonical_value(content.as_json()).unwrap();
    assert!(rt.is_plain(), "cached plaintext content is plain");
    assert_eq!(
        quillmark_content::export::to_plaintext(&rt),
        "a *literal* subject",
        "the asterisks are literal, not emphasis"
    );
}

/// A plaintext string imports verbatim, and a mark-bearing wire content is
/// refused rather than stripped.
#[test]
fn plaintext_coercion_imports_verbatim_and_refuses_marks() {
    const FIELD: &str = "    f:\n      type: plaintext\n";
    let value = coerce(FIELD, serde_json::json!("*not bold* text")).expect("plaintext coerces");
    let rt = quillmark_content::serial::from_canonical_value(&value).unwrap();
    assert!(rt.marks.is_empty(), "no marks: delimiters stayed literal");
    assert_eq!(quillmark_content::export::to_plaintext(&rt), "*not bold* text");

    let marked = quillmark_content::import::from_markdown("a **bold** word")
        .unwrap()
        .into_content()
        .into_normalized();
    assert_eq!(
        coerce(FIELD, quillmark_content::serial::to_canonical_value(&marked)),
        Err(("f".to_string(), "plaintext".to_string()))
    );
}

#[test]
fn inline_survives_the_declaration_wire_for_both_content_types() {
    for ty in ["richtext", "plaintext"] {
        let config = quill_with_field(&format!(
            "    subject:\n      type: {ty}\n      inline: true\n"
        ))
        .expect("loads");
        let field = config.main.fields.get("subject").unwrap();
        let wire = serde_json::to_value(field).expect("serializes");
        assert_eq!(
            wire["inline"],
            serde_json::json!(true),
            "{ty} lost `inline: true` on the wire: {wire}"
        );
    }
}

#[test]
fn plaintext_transform_schema_carries_media_type_and_plain_annotation() {
    let config = quill_with_field("    subject:\n      type: plaintext\n      inline: true\n")
        .expect("loads");
    let schema = super::schema::build_transform_schema(&config);
    let json = schema.as_json();
    let subject = &json["properties"]["subject"];
    assert_eq!(subject["type"], "object");
    assert_eq!(
        subject["contentMediaType"],
        super::schema::CONTENT_MEDIA_TYPE
    );
    assert_eq!(subject[super::schema::QUILLMARK_PLAIN_KEY], true);
    assert_eq!(subject[super::schema::QUILLMARK_INLINE_KEY], true);
}

#[test]
fn enum_type_projects_to_json_schema_string_enum() {
    let config = quill_with_field(
        "    color:\n      type: enum\n      values: [red, green, blue]\n",
    )
    .expect("type: enum loads");
    let field = config.main.fields.get("color").unwrap();
    // The model layer carries the declared choices only: the blank is not one.
    assert_eq!(
        field.r#type,
        FieldType::Enum {
            values: vec!["red".to_string(), "green".to_string(), "blue".to_string()]
        }
    );
    // The payload is what `values:` re-emits from.
    assert_eq!(
        serde_json::to_value(field).unwrap()["values"],
        serde_json::json!(["red", "green", "blue"])
    );
    let schema = super::schema::build_transform_schema(&config);
    let color = &schema.as_json()["properties"]["color"];
    assert_eq!(color["type"], "string");
    // The projection carries the wire-valid domain, which leads with the blank.
    assert_eq!(color["enum"], serde_json::json!(["", "red", "green", "blue"]));
}

#[test]
fn enum_membership_is_validated_on_a_document_value() {
    let config = quill_with_field("    color:\n      type: enum\n      values: [red, blue]\n")
        .expect("loads");
    let field = config.main.fields.get("color").unwrap();
    let errs = super::validation::validate_field(
        field,
        &QuillValue::from_json(serde_json::json!("green")),
        &crate::path::DocPath::main().field("color"),
    );
    assert!(
        errs.iter().any(|e| e.code() == "validation::enum_violation"),
        "an out-of-domain enum value should raise enum_violation, got: {errs:?}"
    );
    let ok = super::validation::validate_field(
        field,
        &QuillValue::from_json(serde_json::json!("red")),
        &crate::path::DocPath::main().field("color"),
    );
    assert!(ok.is_empty(), "an in-domain value validates, got: {ok:?}");
}

/// A card is a part someone writes. The loader sees the proxy (no body), never
/// the fact (whether the kind interleaves), so it advises; and `main` keeps the
/// key unwarned, a form having no root prose.
#[test]
fn a_bodiless_card_kind_warns_and_a_bodiless_main_does_not() {
    let (_, warnings) = QuillConfig::from_yaml_with_warnings(&with_header(
        r#"main:
  body:
    enabled: false
  fields:
    title: { type: string }
card_kinds:
  itinerary:
    body:
      enabled: false
    fields:
      leg: { type: string }
  note:
    fields:
      text: { type: string }
"#,
    ))
    .expect("a bodiless card kind still loads");

    let bodiless: Vec<&Diagnostic> = warnings
        .iter()
        .filter(|d| d.code.as_deref() == Some("quill::bodiless_card_kind"))
        .collect();
    assert_eq!(bodiless.len(), 1, "one kind, one warning: {warnings:?}");
    assert_eq!(bodiless[0].severity, Severity::Warning);
    assert!(bodiless[0].message.contains("itinerary"));
    assert!(
        bodiless[0].hint.as_deref().is_some_and(|h| h.contains("array")),
        "the hint points at the row shape: {:?}",
        bodiless[0].hint
    );
}

/// A table draws one row per element and one column per property, so it loads
/// on the shape it reads and nowhere else. `schema()` echoes it verbatim: the
/// key is the editor's to honor or decline, and nothing else consumes it.
#[test]
fn ui_layout_table_loads_on_a_typed_table_and_is_refused_elsewhere() {
    let config = quill_with_field(
        "    tours:\n      type: array\n      ui:\n        layout: table\n      \
         items:\n        type: object\n        properties:\n          unit: { type: string }\n",
    )
    .expect("a typed table may ask for the table control");
    assert_eq!(
        config.main.fields["tours"].ui.as_ref().unwrap().layout,
        Some(FieldLayout::Table)
    );
    assert_eq!(
        config.schema()["main"]["fields"]["tours"]["ui"]["layout"],
        serde_json::json!("table")
    );

    for (label, field) in [
        (
            "a scalar array",
            "    tags:\n      type: array\n      ui:\n        layout: table\n      \
             items: { type: string }\n",
        ),
        (
            "a typed dictionary",
            "    addr:\n      type: object\n      ui:\n        layout: table\n      \
             properties:\n        street: { type: string }\n",
        ),
        (
            "a scalar",
            "    title:\n      type: string\n      ui:\n        layout: table\n",
        ),
    ] {
        let err = quill_with_field(field).expect_err(label);
        assert!(
            err.iter()
                .any(|d| d.code.as_deref() == Some("quill::invalid_ui")),
            "{label}: expected quill::invalid_ui, got {err:?}"
        );
    }
}

/// Declaring the key contracts that every column is a leaf, so the only decline
/// left to a consumer is the capability one it alone can answer. The boundary is
/// containment, not height: prose is a column whatever its `inline`.
#[test]
fn a_table_column_is_a_leaf_and_a_container_column_is_refused() {
    quill_with_field(
        "    notes:\n      type: array\n      ui:\n        layout: table\n      \
         items:\n        type: object\n        properties:\n          \
         body: { type: richtext }\n",
    )
    .expect("a block richtext is a leaf: how tall it renders is the consumer's call");

    for (label, column) in [
        ("an array column", "{ type: array, items: { type: string } }"),
        (
            "an object column",
            "{ type: object, properties: { city: { type: string } } }",
        ),
    ] {
        let err = quill_with_field(&format!(
            "    appendices:\n      type: array\n      ui:\n        layout: table\n      \
             items:\n        type: object\n        properties:\n          \
             entries: {column}\n"
        ))
        .expect_err(label);
        let diag = err
            .iter()
            .find(|d| d.code.as_deref() == Some("quill::table_column_not_flat"))
            .unwrap_or_else(|| panic!("{label}: expected the column refusal, got {err:?}"));
        assert!(
            diag.message.contains("appendices[].entries"),
            "{label}: the refusal names the column, not the field: {}",
            diag.message
        );
    }
}

/// `max:` caps an array's element count, which is arity: a fact no other type
/// has, and no `items:` declaration can carry.
#[test]
fn max_loads_on_an_array_and_is_refused_elsewhere() {
    let config =
        quill_with_field("    rows:\n      type: array\n      max: 37\n      items: { type: string }\n")
            .expect("an array may declare its cap");
    assert_eq!(config.main.fields["rows"].max, Some(37));
    assert_eq!(
        config.schema()["main"]["fields"]["rows"]["max"],
        serde_json::json!(37)
    );

    for field in [
        "    n:\n      type: integer\n      max: 3\n",
        "    s:\n      type: string\n      max: 3\n",
    ] {
        let err = quill_with_field(field).expect_err("max off an array");
        assert!(
            err.iter()
                .any(|d| d.code.as_deref() == Some("quill::field_parse_error")),
            "{err:?}"
        );
    }

    let negative = quill_with_field(
        "    rows:\n      type: array\n      max: -1\n      items: { type: string }\n",
    )
    .expect_err("a cap is a count");
    assert!(
        negative
            .iter()
            .any(|d| d.code.as_deref() == Some("quill::field_parse_error")),
        "{negative:?}"
    );
}

/// A quill seeding past the cap it declares would warn on a document nobody
/// authored, so the literal is held to the same count the document is.
#[test]
fn a_literal_longer_than_max_is_a_load_error() {
    for slot in ["default", "example"] {
        let err = quill_with_field(&format!(
            "    rows:\n      type: array\n      max: 2\n      {slot}: [a, b, c]\n      \
             items: {{ type: string }}\n"
        ))
        .expect_err("a literal over the cap");
        assert!(
            err.iter()
                .any(|d| d.code.as_deref() == Some(&format!("quill::{slot}_over_max")[..])),
            "expected quill::{slot}_over_max, got {err:?}"
        );
    }

    quill_with_field(
        "    rows:\n      type: array\n      max: 2\n      default: [a, b]\n      \
         items: { type: string }\n",
    )
    .expect("a literal at the cap fits it");
}

/// The obligation family, never a gate: an over-filled document renders, with
/// the plate's own rule for the surplus. The cap is per declaration, so a
/// nested array is capped by its own.
#[test]
fn an_over_filled_array_warns_at_its_own_path() {
    let quill = quill_from_yaml(&with_header(
        r#"main:
  fields:
    rows:
      type: array
      max: 2
      items:
        type: object
        properties:
          tags:
            type: array
            max: 1
            items: { type: string }
"#,
    ));
    let doc = Document::parse(
        "~~~\n$quill: q@1.0\n$kind: main\nrows:\n  - tags: [a, b]\n  - tags: [a]\n  - tags: [a]\n~~~\n",
    )
    .expect("parses")
    .document;

    let found: Vec<(String, String)> = quill
        .validate(&doc)
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("validation::cardinality"))
        .map(|d| {
            assert_eq!(d.severity, Severity::Warning, "a cap never gates render");
            (d.path.unwrap_or_default(), format!("{:?}", d.args))
        })
        .collect();

    assert_eq!(
        found.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
        ["main.rows", "main.rows[0].tags"],
        "the outer cap and the inner one each report at their own path"
    );
    assert!(found[0].1.contains("\"max\"") && found[0].1.contains("\"actual\""));
    assert!(
        quill.compile_data(&doc, None).is_ok(),
        "an over-filled document still renders"
    );
}

/// The blueprint is a document the quill must accept: a cap it declares cannot
/// be a cap its own placeholder row breaks.
#[test]
fn a_capped_table_blueprints_within_its_own_cap() {
    for max in [0, 1, 3] {
        let quill = quill_from_yaml(&with_header(&format!(
            r#"main:
  fields:
    rows:
      type: array
      max: {max}
      items:
        type: object
        properties:
          unit: {{ type: string, default: "" }}
"#
        )));
        let doc = Document::parse(&quill.config().blueprint())
            .expect("the blueprint parses")
            .document;
        let over: Vec<Diagnostic> = quill
            .validate(&doc)
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("validation::cardinality"))
            .collect();
        assert!(
            over.is_empty(),
            "max: {max} blueprints over its own cap: {over:?}"
        );
    }
}

/// The blueprint is the surface the MCP author reads, so a cap it does not show
/// is a cap learned from prose. The line holds the own-line position
/// `# composable (0..N)` takes — under the description, above the `# e.g.` hint
/// — at both depths a cap loads at.
#[test]
fn a_cap_rides_its_own_leading_line_under_the_description() {
    let bp = config_with_sections(
        r#"main:
  fields:
    rows:
      type: array
      max: 2
      description: The units that fit the page.
      default: [a]
      example: [a, b]
      items: { type: string }
    box:
      type: object
      properties:
        tags:
          type: array
          max: 1
          description: Tags for the box.
          default: [t]
          example: [u]
          items: { type: string }
"#,
    )
    .expect("a cap loads at either depth")
    .blueprint();

    assert!(
        bp.contains("# The units that fit the page.\n# up to 2\n# e.g. [a, b]\nrows:"),
        "{bp}"
    );
    assert!(
        bp.contains("  # Tags for the box.\n  # up to 1\n  # e.g. [u]\n  tags:"),
        "{bp}"
    );

    let uncapped = config_with_sections(
        "main:\n  fields:\n    rows:\n      type: array\n      items: { type: string }\n",
    )
    .expect("an uncapped array loads")
    .blueprint();
    assert!(!uncapped.contains("up to"), "{uncapped}");
}
