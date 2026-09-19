//! Enum variants: fields that exist only for one enum value.
//!
//! The axis has four surfaces that must agree on one answer — load, the render
//! floor, validation, and the authoring projections — and the shape of bug it
//! invites is a disagreement between them. Each test below pins one surface to
//! the same reading of the same schema.

use crate::document::Document;
use crate::quill::{blank, build_transform_schema, quill_from_yaml, FieldSchema, Quill, QuillConfig};
use crate::value::QuillValue;
use serde_json::json;

/// A quill whose `classification` enum brings a `CUI` field set into play:
/// `controlled_by` obliged in that world (no `default:`), `category` optional.
fn quill_yaml() -> &'static str {
    r#"
quill:
  name: variant_probe
  version: "0.1.0"
  backend: typst
  description: Enum variant probe

typst:
  plate_file: plate.typ

main:
  fields:
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI, SECRET]
      default: ""
      variants:
        CUI:
          controlled_by: { type: string }
          category: { type: string, default: "" }
        SECRET:
          declassify_on: { type: string }
    title:
      type: string
      default: ""
"#
}

fn config() -> QuillConfig {
    QuillConfig::from_yaml(quill_yaml()).expect("variant probe loads")
}

fn quill() -> Quill {
    quill_from_yaml(quill_yaml())
}

fn doc(fields: &str) -> Document {
    let markdown =
        format!("~~~\n$quill: variant_probe@0.1.0\n$kind: main\n{fields}~~~\n");
    Document::parse(&markdown).expect("document parses").document
}

/// The plate's card-level keys, the body and metadata aside.
fn plate(document: &Document) -> serde_json::Value {
    let mut data = config()
        .compile_data(document)
        .expect("compile_data succeeds");
    let object = data.as_object_mut().unwrap();
    object.retain(|k, _| !k.starts_with('$') && k != "title");
    data
}

fn codes(document: &Document) -> Vec<(String, String)> {
    quill()
        .validate(document)
        .into_iter()
        .map(|d| {
            (
                d.code.unwrap_or_default(),
                d.path.unwrap_or_default(),
            )
        })
        .collect()
}

fn field(yaml: &str) -> FieldSchema {
    let value = QuillValue::from_yaml_str(yaml).unwrap();
    FieldSchema::from_quill_value("classification".to_string(), &value).unwrap()
}

fn load_error(fields: &str) -> String {
    let yaml = format!(
        r#"
quill:
  name: bad
  version: "0.1.0"
  backend: typst
  description: bad

typst:
  plate_file: plate.typ

main:
  fields:
{fields}
"#
    );
    let err = QuillConfig::from_yaml(&yaml).expect_err("expected a load error");
    format!("{err:?}")
}

#[test]
fn variants_on_a_non_enum_field_is_a_load_error() {
    assert!(load_error(
        "    name:\n      type: string\n      variants:\n        A:\n          x: { type: string }\n"
    )
    .contains("quill::variants_on_non_enum"));
}

#[test]
fn a_variant_keyed_by_a_non_member_is_a_load_error() {
    let err = load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        B:\n          x: { type: string }\n",
    );
    assert!(err.contains("quill::variant_unknown_value"));
}

/// The same rule `quill::enum_blank_member` states from the `values:` side.
#[test]
fn a_variant_keyed_by_the_blank_is_a_load_error() {
    let err = load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        \"\":\n          x: { type: string }\n",
    );
    assert!(err.contains("quill::variant_unknown_value"));
}

#[test]
fn a_non_string_variant_default_is_a_load_error() {
    let variant = load_error(
        "    c:\n      type: enum\n      values: [\"1\"]\n      default: 1\n      \
         variants:\n        \"1\":\n          x: { type: string }\n",
    );
    let plain = load_error("    c:\n      type: enum\n      values: [\"1\"]\n      default: 1\n");
    assert!(
        variant.contains("quill::default_type_mismatch"),
        "variant-bearing enum accepted a numeric default: {variant}"
    );
    assert!(plain.contains("quill::default_type_mismatch"), "{plain}");
}

/// A hoisted field earns the flat path's key gate: without it a variant could
/// declare `$kind` and forge document metadata.
#[test]
fn a_variant_field_key_obeys_the_field_name_gate() {
    let err = load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          $kind: { type: string }\n",
    );
    assert!(err.contains("quill::invalid_field_name"));
}

#[test]
fn an_empty_variant_and_an_empty_variants_map_are_load_errors() {
    assert!(load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        A: {}\n"
    )
    .contains("quill::variant_empty"));
    assert!(
        load_error("    c:\n      type: enum\n      values: [A]\n      variants: {}\n")
            .contains("quill::variant_empty")
    );
}

/// A variant carries any leaf type a card field may; every surface below pins
/// one, and this one is load.
#[test]
fn a_variant_carries_any_leaf_type() {
    for ty in ["richtext", "plaintext", "date", "datetime"] {
        let yaml = format!(
            r#"
quill:
  name: ok
  version: "0.1.0"
  backend: typst
  description: ok

typst:
  plate_file: plate.typ

main:
  fields:
    c:
      type: enum
      values: [A]
      variants:
        A:
          x: {{ type: {ty} }}
"#
        );
        let config = QuillConfig::from_yaml(&yaml)
            .unwrap_or_else(|e| panic!("type: {ty} must load inside a variant: {e:?}"));
        let cell = config
            .main
            .cell("x")
            .unwrap_or_else(|| panic!("type: {ty} cell resolves at card level"));
        assert_eq!(cell.r#type.as_str(), ty);
        assert!(config.main.fields.get("x").is_none(), "a cell is not a field");
    }
}

/// The four value surfaces on a content cell whose world resolves at value
/// time: coercion imports the markdown, validation type-checks it, the render
/// floor carries the live cell and blank-fills the dormant one, and the content
/// companion cache sees the cell itself as content-bearing.
#[test]
fn a_variant_content_cell_crosses_every_value_surface() {
    const YAML: &str = r#"
quill:
  name: variant_content
  version: "0.1.0"
  backend: typst
  description: variant content probe

typst:
  plate_file: plate.typ

main:
  fields:
    classification:
      type: enum
      values: [CUI]
      default: ""
      variants:
        CUI:
          note: { type: richtext }
          reply_by: { type: date }
"#;
    let config = QuillConfig::from_yaml(YAML).expect("loads");
    // The gate every companion, resting-form and seed path consults is the
    // cell's own; the enum bears none.
    assert!(crate::quill::config::field_contains_content(
        config.main.cell("note").unwrap()
    ));
    assert!(!crate::quill::config::field_contains_content(
        &config.main.fields["classification"]
    ));

    let markdown = "~~~
$quill: variant_content@0.1.0
$kind: main
classification: CUI
note: A **bold** note
reply_by: 2026-03-04
~~~
";
    let document = Document::parse(markdown).expect("parses").document;
    assert!(
        quill_from_yaml(YAML)
            .validate(&document)
            .iter()
            .all(|d| d.severity != crate::error::Severity::Error),
        "a variant content cell validates: {:?}",
        quill_from_yaml(YAML).validate(&document)
    );

    let plate = config.compile_data(&document).expect("compiles");
    // Coercion imported the markdown to canonical content, so the cell reaches
    // the plate as the content object a card-level richtext would.
    assert_eq!(plate["classification"], json!("CUI"));
    assert!(
        plate["note"].get("text").is_some(),
        "the cell is canonical content, not a raw string: {plate}"
    );
    assert_eq!(plate["reply_by"], json!("2026-03-04"));

    // The blank world owns no cell, so each rests at its blank on the wire.
    let blank_doc = Document::parse(concat!(
        "~~~\n",
        "$quill: variant_content@0.1.0\n",
        "$kind: main\n",
        "classification: \"\"\n",
        "~~~\n",
    ))
    .expect("parses")
    .document;
    let blank_plate = config.compile_data(&blank_doc).expect("compiles");
    assert_eq!(blank_plate["classification"], json!(""));
    assert_eq!(blank_plate["note"]["text"], json!(""));
    assert_eq!(blank_plate["reply_by"], json!(""));
}

/// The two card-level keys are what a cell may not carry.
#[test]
fn a_variant_field_may_not_carry_variants_or_a_group() {
    // A cell inherits the discriminant's group, so declaring one is a dead knob.
    assert!(load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          x: { type: string, ui: { group: g } }\n"
    )
    .contains("quill::nested_group_not_supported"));
    assert!(load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          x: { type: enum, values: [P], variants: { P: { y: { type: string } } } }\n"
    )
    .contains("quill::variant_placement"));
}

/// The union projection is what stays one level deep; the shapes below it do
/// not.
#[test]
fn a_variant_field_holds_a_container() {
    for cell in [
        "x: { type: object, properties: { y: { type: string } } }",
        "x: { type: array, items: { type: string } }",
        "x: { type: array, items: { type: object, properties: { y: { type: string } } } }",
    ] {
        let yaml = format!(
            "quill:\n  name: ok\n  version: \"0.1.0\"\n  backend: typst\n  description: ok\n\ntypst:\n  plate_file: plate.typ\n\nmain:\n  fields:\n    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          {cell}\n"
        );
        QuillConfig::from_yaml(&yaml).unwrap_or_else(|e| panic!("{cell}: {e:?}"));
    }
}

/// Two spellings of one name would coerce a live value under the other world's
/// type.
#[test]
fn a_name_two_worlds_declare_differently_is_a_load_error() {
    let err = load_error(
        "    c:\n      type: enum\n      values: [A, B]\n      variants:\n        A:\n          note: { type: string }\n        B:\n          note: { type: integer }\n",
    );
    assert!(err.contains("quill::variant_field_collision"), "{err}");
    assert!(err.contains("'A'") && err.contains("'B'"), "{err}");
}

/// A cell rests beside the card's fields, so a name is one cell of the card:
/// a field and a cell under one name would answer for two declarations, and
/// two enums' cells under one name have no discriminant to say which brings
/// it into play. Both refuse whether or not the declarations agree.
#[test]
fn a_cell_shadowing_a_field_or_another_enums_cell_is_a_load_error() {
    let shadows_field = load_error(
        "    note: { type: string }\n    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          note: { type: string }\n",
    );
    assert!(shadows_field.contains("quill::variant_field_collision"), "{shadows_field}");
    assert!(shadows_field.contains("'c'") && shadows_field.contains("'note'"), "{shadows_field}");

    let two_enums = load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          note: { type: string }\n    d:\n      type: enum\n      values: [B]\n      variants:\n        B:\n          note: { type: string }\n",
    );
    assert!(two_enums.contains("quill::variant_field_collision"), "{two_enums}");
    assert!(two_enums.contains("'c'") && two_enums.contains("'d'"), "{two_enums}");
}

/// Repetition is how a shared field set is spelled, so the gate is disagreement
/// rather than repetition.
#[test]
fn a_name_two_worlds_declare_identically_loads() {
    let config = QuillConfig::from_yaml(&quill_yaml().replace(
        "        SECRET:\n          declassify_on: { type: string }",
        "        SECRET:\n          declassify_on: { type: string }\n          controlled_by: { type: string }",
    ))
    .expect("an identically-repeated variant field loads");
    let doc = Document::parse(
        "~~~\n$quill: variant_probe@0.1.0\n$kind: main\nclassification: SECRET\ncontrolled_by: SAF/AA\ndeclassify_on: 20301231\n~~~\n",
    )
    .expect("parses")
    .document;
    let data = config.compile_data(&doc).expect("compile_data succeeds");
    assert_eq!(data["controlled_by"], json!("SAF/AA"));
}

#[test]
fn variants_below_card_level_is_a_load_error() {
    let err = load_error(
        "    o:\n      type: object\n      properties:\n        c:\n          type: enum\n          values: [A]\n          variants:\n            A:\n              x: { type: string }\n",
    );
    assert!(err.contains("quill::variant_placement"));
}

/// `variants:` changes no resting shape: the enum blanks to the bare string a
/// variantless one does.
#[test]
fn a_variant_bearing_enum_blanks_to_the_bare_string() {
    let schema = field(
        "type: enum\nvalues: [A]\nvariants:\n  A:\n    x: { type: string }\n",
    );
    assert_eq!(blank(&schema).into_json(), json!(""));
}

/// Every declared name is present whichever world is live, so a plate reads a
/// cell without a guard and a dormant world's cell reads as its blank.
#[test]
fn an_empty_document_renders_every_cell_at_its_blank() {
    assert_eq!(
        plate(&doc("")),
        json!({ "classification": "", "controlled_by": "", "category": "", "declassify_on": "" })
    );
}

/// What makes an unguarded read total inside the branch a plate writes.
#[test]
fn the_live_world_arrives_complete_and_blank_filled() {
    assert_eq!(
        plate(&doc("classification: CUI\ncontrolled_by: SAF/AA\n")),
        json!({ "classification": "CUI", "controlled_by": "SAF/AA", "category": "", "declassify_on": "" })
    );
}

/// A dormant world's answer never reaches the plate under a tag that disowns
/// it: the cell rests at its blank, not at the stranded value and not at its
/// `default:`.
#[test]
fn a_dormant_worlds_answers_never_reach_the_plate() {
    let yaml = quill_yaml().replace(
        "          category: { type: string, default: \"\" }",
        "          category: { type: string, default: PRVCY }",
    );
    let config = QuillConfig::from_yaml(&yaml).unwrap();
    let data = config
        .compile_data(&doc("classification: UNCLASSIFIED\ncontrolled_by: SAF/AA\n"))
        .unwrap();
    assert_eq!(data["classification"], json!("UNCLASSIFIED"));
    assert_eq!(data["controlled_by"], json!(""));
    assert_eq!(data["category"], json!(""), "a dormant cell's default does not fire");

    let live = config.compile_data(&doc("classification: CUI\n")).unwrap();
    assert_eq!(live["category"], json!("PRVCY"), "a live cell's default does");
}

#[test]
fn the_discriminant_falls_to_the_default_and_carries_its_world() {
    let yaml = quill_yaml().replace("      default: \"\"\n      variants:", "      default: CUI\n      variants:");
    let config = QuillConfig::from_yaml(&yaml).unwrap();
    let data = config.compile_data(&doc("controlled_by: SAF/AA\n")).unwrap();
    assert_eq!(data["classification"], json!("CUI"));
    assert_eq!(data["controlled_by"], json!("SAF/AA"));
}

/// Null ≡ absent holds on the discriminant as on any enum.
#[test]
fn a_null_discriminant_blank_fills() {
    assert_eq!(plate(&doc("classification:\n"))["classification"], json!(""));
}

/// The plate's key order: a cell follows its discriminant among the
/// declared-but-absent tail, and an authored cell keeps its authored position.
#[test]
fn a_cell_follows_its_discriminant_on_the_plate() {
    let keys = |document: &Document| -> Vec<String> {
        plate(document)
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    assert_eq!(
        keys(&doc("")),
        ["classification", "controlled_by", "category", "declassify_on"]
    );
    assert_eq!(
        keys(&doc("controlled_by: SAF/AA\nclassification: CUI\n")),
        ["controlled_by", "classification", "category", "declassify_on"]
    );
}

/// `resolve()`'s contract is byte-parity with the plate, a cell being one row
/// carrying its own rung.
#[test]
fn resolve_reports_each_cell_as_its_own_row_matching_the_plate() {
    let quill = quill();
    let document = doc("classification: CUI\ncontrolled_by: SAF/AA\n");
    let resolved = quill.resolve(&document);
    let data = quill.compile_data(&document).unwrap();
    let row = |name: &str| {
        resolved
            .main
            .fields
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("{name} row"))
            .clone()
    };
    for name in ["classification", "controlled_by", "category", "declassify_on"] {
        assert_eq!(row(name).value.as_json(), &data[name], "{name}");
    }
    use crate::quill::resolved::FieldSource;
    assert_eq!(row("classification").source, FieldSource::Authored);
    assert_eq!(row("controlled_by").source, FieldSource::Authored);
    assert_eq!(row("category").source, FieldSource::Default);
    assert_eq!(row("declassify_on").source, FieldSource::Blank);

    // Rows sit in declaration order, a cell after its discriminant.
    let names: Vec<&str> = resolved.main.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["classification", "controlled_by", "category", "declassify_on", "title"]
    );
}

/// The conditional-obligation payoff: a field with no `default:` is obliged in
/// its own world and silent everywhere else — the thing `must_fill` alone cannot
/// say.
#[test]
fn obligation_follows_the_selected_world() {
    let obliged = codes(&doc("classification: CUI\n"));
    assert!(obliged.contains(&(
        "validation::must_fill".to_string(),
        "main.controlled_by".to_string()
    )));
    // `category` declares a `default:`, so it stays skippable in the same world.
    assert!(!obliged.iter().any(|(_, path)| path == "main.category"));

    // The identical schema obliges nothing once another world is selected.
    let quiet = codes(&doc("classification: UNCLASSIFIED\n"));
    assert!(!quiet.iter().any(|(_, path)| path == "main.controlled_by"));
    assert!(!quiet.iter().any(|(_, path)| path == "main.declassify_on"));
}

/// Flipping a discriminant in an editor must not cost the author their answers,
/// and must not hand them a document that refuses to render.
#[test]
fn a_stranded_value_is_kept_and_warned_never_gated() {
    let document = doc("classification: UNCLASSIFIED\ncontrolled_by: SAF/AA\n");
    let diags = quill().validate(&document);
    let stranded = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::out_of_variant"))
        .expect("out_of_variant warning");
    assert_eq!(stranded.severity, crate::error::Severity::Warning);
    assert_eq!(stranded.path.as_deref(), Some("main.controlled_by"));
    assert_eq!(stranded.args["variant"], json!("CUI"));
    assert_eq!(stranded.args["selected"], json!("UNCLASSIFIED"));
    // Kept in the document…
    assert_eq!(
        document.main().payload().get("controlled_by").unwrap().as_json(),
        &json!("SAF/AA")
    );
    // …and blank on the wire.
    assert_eq!(plate(&document)["controlled_by"], json!(""));
}

/// A key no variant declares is an undeclared field, which every other surface
/// carries without comment; only a key some *other* world owns is the stranded
/// case worth naming. A present-null cell is absent and strands nothing.
#[test]
fn an_undeclared_or_null_key_draws_no_variant_warning() {
    for fields in ["classification: CUI\nnote: hello\n", "classification: CUI\ndeclassify_on:\n"] {
        let document = doc(fields);
        assert!(
            !quill()
                .validate(&document)
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::out_of_variant")),
            "{fields}"
        );
    }
}

#[test]
fn the_domain_check_lands_on_the_discriminant_path() {
    let found = codes(&doc("classification: NATO\n"));
    assert!(found.contains(&(
        "validation::enum_violation".to_string(),
        "main.classification".to_string()
    )));
}

/// The live world's fields still type-check; a dormant world's do not, since
/// nothing downstream reads them.
#[test]
fn only_the_live_worlds_fields_are_type_checked() {
    let bad_live = codes(&doc("classification: CUI\ncontrolled_by: [1, 2]\n"));
    assert!(bad_live
        .iter()
        .any(|(code, path)| code == "validation::type_mismatch" && path == "main.controlled_by"));

    let bad_dormant = codes(&doc("classification: UNCLASSIFIED\ncontrolled_by: [1, 2]\n"));
    assert!(!bad_dormant
        .iter()
        .any(|(code, _)| code == "validation::type_mismatch"));
}

/// A cell is a field of the card on the typed read and write doors, so a
/// consumer reaches it by name and never through the enum.
#[test]
fn a_cell_reads_and_writes_by_its_own_name() {
    let quill = quill();
    let mut document = doc("classification: CUI\n");
    quill
        .writer(&mut document)
        .set("controlled_by", "SAF/AA")
        .expect("a cell is a declared name");
    assert_eq!(
        quill.reader(&document).get("controlled_by").unwrap().unwrap().as_json(),
        &json!("SAF/AA")
    );
    assert!(
        quill.writer(&mut document).set("nonesuch", "x").is_err(),
        "an undeclared name still refuses"
    );
}

/// A blueprint is a document, so it shows one world and *names* the rest; the
/// discriminant is the scalar cell every other enum is, and a live cell sits
/// beside it at card level.
#[test]
fn the_blueprint_shows_one_world_and_names_the_others() {
    let bp = config().blueprint();
    assert!(bp.contains("# when CUI: controlled_by, category"), "{bp}");
    assert!(bp.contains("# when SECRET: declassify_on"), "{bp}");
    assert!(bp.contains("classification: \"\" # enum<UNCLASSIFIED | CUI | SECRET>"), "{bp}");
    // The blank world owns no field set, so no variant cell is emitted.
    assert!(!bp.contains("controlled_by:"), "{bp}");
    assert!(!bp.contains("value:"), "{bp}");
}

#[test]
fn the_blueprint_emits_the_default_worlds_cells_and_round_trips() {
    let yaml = quill_yaml().replace("      default: \"\"\n      variants:", "      default: CUI\n      variants:");
    let config = QuillConfig::from_yaml(&yaml).unwrap();
    let bp = config.blueprint();
    assert!(
        bp.contains(concat!(
            "classification: CUI # enum<UNCLASSIFIED | CUI | SECRET>\n",
            "controlled_by: !must_fill # string\n",
            "category: \"\" # string\n",
        )),
        "{bp}"
    );
    assert!(!bp.contains("declassify_on:"), "{bp}");
    // The blueprint round-trips through the parser by construction: the shape
    // an author edits is the shape the parser reads back.
    let reparsed = Document::parse(&bp).expect("blueprint round-trips").document;
    assert_eq!(
        reparsed.main().payload().get("classification").unwrap().as_json(),
        &json!("CUI")
    );
    assert!(reparsed.main().payload().get("controlled_by").is_some());
}

/// A defaultless discriminant carries the marker itself, as any scalar does.
#[test]
fn a_defaultless_discriminant_carries_the_marker() {
    let yaml = quill_yaml().replace("      default: \"\"\n      variants:", "      variants:");
    let bp = QuillConfig::from_yaml(&yaml).unwrap().blueprint();
    assert!(bp.contains("classification: !must_fill # enum<"), "{bp}");
}

/// A cell holds a container like any field, so the blueprint expands one per
/// property. A flattened cell would hand the author a scalar slot where the
/// schema wants a mapping, drop every property's own description and `default:`,
/// and stamp the marker on a path the obligation predicate never addresses.
#[test]
fn the_blueprint_expands_a_container_cell_per_property() {
    const YAML: &str = r#"
quill:
  name: variant_container
  version: "0.1.0"
  backend: typst
  description: variant container probe

typst:
  plate_file: plate.typ

main:
  fields:
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      default: CUI
      variants:
        CUI:
          controlled_by:
            type: object
            description: Controlling office.
            properties:
              office: { type: string, description: Office symbol. }
              phone: { type: string, default: "" }
          citations:
            type: array
            items:
              type: object
              properties:
                src: { type: string }
"#;
    let bp = QuillConfig::from_yaml(YAML).expect("loads").blueprint();
    assert!(
        bp.contains(concat!(
            "# Controlling office.\n",
            "controlled_by: # object\n",
            "  # Office symbol.\n",
            "  office: !must_fill # string\n",
            "  phone: \"\" # string\n",
            "citations: # array<object>\n",
            "  - src: !must_fill # string\n",
        )),
        "{bp}"
    );

    // The marked cells are the cells the schema-side predicate warns at: a
    // container is a namespace on both surfaces, never a cell on either.
    let document = Document::parse(&bp).expect("the blueprint parses").document;
    let warned: Vec<String> = quill_from_yaml(YAML)
        .validate(&document)
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("validation::must_fill"))
        .filter_map(|d| d.path)
        .collect();
    assert!(
        warned.contains(&"main.controlled_by.office".to_string())
            && warned.contains(&"main.citations[0].src".to_string()),
        "{warned:?}"
    );
    assert!(
        !warned.iter().any(|p| p == "main.controlled_by"),
        "{warned:?}"
    );

    let reparsed = Document::parse(&document.to_markdown())
        .expect("re-emit parses")
        .document;
    assert_eq!(document, reparsed, "the expansion round-trips");
}

/// Which world is live decides which fields are even candidates, so the
/// discriminant must resolve before the field set is walked.
#[test]
fn seeding_resolves_the_discriminant_before_walking_the_field_set() {
    let yaml = quill_yaml()
        .replace("      default: \"\"\n", "      example: CUI\n")
        .replace(
            "          controlled_by: { type: string }",
            "          controlled_by: { type: string, example: SAF/AA }",
        )
        .replace(
            "          declassify_on: { type: string }",
            "          declassify_on: { type: string, example: \"20301231\" }",
        );
    let quill = quill_from_yaml(&yaml);
    let seeded = quill.seed_document();
    let payload = seeded.main().payload();
    assert_eq!(payload.get("classification").unwrap().as_json(), &json!("CUI"));
    assert_eq!(payload.get("controlled_by").unwrap().as_json(), &json!("SAF/AA"));
    // `declassify_on` belongs to a world the seed did not select.
    assert!(payload.get("declassify_on").is_none());
    // The seed and the blueprint stamp the same cells.
    assert!(payload.is_fill("classification"));
    assert!(payload.is_fill("controlled_by"));
}

/// The transform schema projects a cell beside its enum: at schema time there
/// is no live world, so the union is the only projection available.
#[test]
fn the_transform_schema_projects_every_cell_beside_its_enum() {
    let schema = build_transform_schema(&config());
    let json = schema.as_json();
    let props = json["properties"].as_object().unwrap();
    assert_eq!(
        props["classification"]["enum"],
        json!(["", "UNCLASSIFIED", "CUI", "SECRET"])
    );
    assert_eq!(props["controlled_by"]["type"], json!("string"));
    assert_eq!(props["declassify_on"]["type"], json!("string"));
    let keys: Vec<&str> = props.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["classification", "controlled_by", "category", "declassify_on", "title", "$body"]
    );
    for name in ["controlled_by", "category", "declassify_on"] {
        assert_eq!(
            props[name]
                .as_object()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with("quillmark:variant"))
                .count(),
            0,
            "`variants:` on the declaration view states the owner instead"
        );
    }
}

/// `schema()` is the declaration view and emits what the author wrote, so a
/// round-trip through it re-loads.
#[test]
fn the_declaration_schema_round_trips_through_a_reload() {
    let emitted = config().schema();
    let variants = &emitted["main"]["fields"]["classification"]["variants"];
    assert_eq!(variants["CUI"]["controlled_by"]["type"], json!("string"));
    assert_eq!(variants["SECRET"]["declassify_on"]["type"], json!("string"));
    assert!(emitted["main"]["fields"].get("controlled_by").is_none());
}

/// A container-shaped schema literal is refused at load, in either slot, and the
/// diagnostic names the member spelling that works.
#[test]
fn a_container_shaped_schema_literal_is_a_load_error() {
    for slot in ["default", "example"] {
        let yaml = format!(
            concat!(
                "quill:\n",
                "  name: variant_literal\n",
                "  version: \"0.1.0\"\n",
                "  backend: typst\n",
                "  description: probe\n",
                "main:\n",
                "  fields:\n",
                "    classification:\n",
                "      type: enum\n",
                "      values: [UNCLASSIFIED, CUI]\n",
                "      {slot}: {{ value: CUI, note: \"A **bold** note\" }}\n",
                "      variants:\n",
                "        CUI:\n",
                "          note: {{ type: richtext }}\n",
            ),
            slot = slot,
        );
        let err = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
        let diag = err
            .iter()
            .find(|d| d.code.as_deref() == Some(&format!("quill::{slot}_type_mismatch")))
            .unwrap_or_else(|| panic!("a container-shaped `{slot}:` is a load error, got: {err:?}"));
        assert!(
            diag.hint
                .as_deref()
                .is_some_and(|h| h.contains(&format!("{slot}: UNCLASSIFIED"))),
            "the hint names the member spelling, got: {:?}",
            diag.hint
        );
    }
}

/// The retired container spelling is a type mismatch on the enum, refused at
/// validate and at render, never silently adopted.
#[test]
fn the_container_spelling_is_refused() {
    let document = doc("classification:\n  value: CUI\n  controlled_by: SAF/AA\n");
    let found = codes(&document);
    assert!(
        found.contains(&(
            "validation::type_mismatch".to_string(),
            "main.classification".to_string()
        )),
        "{found:?}"
    );
    assert!(config().compile_data(&document).is_err());
}
