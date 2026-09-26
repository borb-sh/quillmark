//! Enum variants: fields that exist only for one enum value.
//!
//! The axis has four surfaces that must agree on one answer — load, the render
//! floor, validation, and the authoring projections — and the shape of bug it
//! invites is a disagreement between them. Each test below pins one surface to
//! the same reading of the same schema.

use crate::document::Document;
use crate::quill::{
    blank, build_transform_schema, quill_from_yaml, test_date, FieldSchema, Quill, QuillConfig,
};
use crate::value::QuillValue;
use serde_json::json;

/// A quill whose `classification` enum brings a `CUI` field set into play:
/// `controlled_by` with no `default:`, `category` defaulted.
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

fn plate(document: &Document) -> serde_json::Value {
    config()
        .compile_data(document, test_date())
        .expect("compile_data succeeds")["classification"]
        .clone()
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

fn load(fields: &str) -> Result<QuillConfig, String> {
    QuillConfig::from_yaml(&format!(
        "quill:\n  name: probe\n  version: \"0.1.0\"\n  backend: typst\n  description: probe\n\
         main:\n  fields:\n{fields}\n"
    ))
}

fn load_error(fields: &str) -> String {
    format!("{:?}", load(fields).expect_err("expected a load error"))
}

/// A variant key is a declared, non-blank member; a hoisted cell earns the flat
/// path's key gate (else a variant could declare `$kind`), may not shadow the
/// `value` discriminant, carries neither a group nor worlds of its own, and two
/// worlds may repeat a name only identically.
#[test]
fn a_malformed_variant_declaration_is_refused_by_code() {
    for (fields, code) in [
        (
            "    name:\n      type: string\n      variants:\n        A:\n          x: { type: string }\n",
            "quill::variants_on_non_enum",
        ),
        (
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        B:\n          x: { type: string }\n",
            "quill::variant_unknown_value",
        ),
        (
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        \"\":\n          x: { type: string }\n",
            "quill::variant_unknown_value",
        ),
        (
            "    c:\n      type: enum\n      values: [\"1\"]\n      default: 1\n      \
             variants:\n        \"1\":\n          x: { type: string }\n",
            "quill::default_type_mismatch",
        ),
        (
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          value: { type: string }\n",
            "quill::variant_reserved_field_name",
        ),
        (
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          $kind: { type: string }\n",
            "quill::invalid_field_name",
        ),
        (
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        A: {}\n",
            "quill::variant_empty",
        ),
        ("    c:\n      type: enum\n      values: [A]\n      variants: {}\n", "quill::variant_empty"),
        (
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          x: { type: string, ui: { group: g } }\n",
            "quill::nested_group_not_supported",
        ),
        (
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          x: { type: enum, values: [P], variants: { P: { y: { type: string } } } }\n",
            "quill::variant_placement",
        ),
        (
            "    c:\n      type: enum\n      values: [A, B]\n      variants:\n        A:\n          note: { type: string }\n        B:\n          note: { type: integer }\n",
            "quill::variant_field_collision",
        ),
    ] {
        assert!(load_error(fields).contains(code), "{code}: {fields}");
    }
}

/// A variant cell carries any type a card field may, containers included.
#[test]
fn a_variant_cell_carries_any_field_type() {
    for ty in ["richtext", "plaintext", "date", "datetime"] {
        let config = load(&format!(
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          x: {{ type: {ty} }}\n"
        ))
        .unwrap_or_else(|e| panic!("type: {ty} must load inside a variant: {e:?}"));
        let cell = config.main.fields["c"].variant_field("x").expect("cell resolves");
        assert_eq!(cell.r#type.as_str(), ty);
    }
    for cell in [
        "x: { type: object, properties: { y: { type: string } } }",
        "x: { type: array, items: { type: string } }",
        "x: { type: array, items: { type: object, properties: { y: { type: string } } } }",
    ] {
        load(&format!(
            "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          {cell}\n"
        ))
        .unwrap_or_else(|e| panic!("{cell}: {e:?}"));
    }
}

/// The four value surfaces on a content cell whose world resolves at value
/// time: coercion imports the markdown, validation type-checks it, the render
/// floor carries the live world's cell and blank-fills the absent one, and the
/// content companion cache sees the container as content-bearing at all.
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
    // The gate every companion, resting-form and seed path consults: a
    // container whose world carries a content leaf must read as content-bearing.
    assert!(crate::quill::config::field_contains_content(
        &config.main.fields["classification"]
    ));

    let markdown = "~~~
$quill: variant_content@0.1.0
$kind: main
classification:
  value: CUI
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

    let plate =
        config.compile_data(&document, test_date()).expect("compiles")["classification"].clone();
    // Coercion imported the markdown to canonical content, so the cell reaches
    // the plate as the content object a card-level richtext would.
    assert_eq!(plate["value"], json!("CUI"));
    assert!(
        plate["note"].get("text").is_some(),
        "the cell is canonical content, not a raw string: {plate}"
    );
    assert_eq!(plate["reply_by"], json!("2026-03-04"));

    // The blank world carries no cell at all, per the closed-container rule.
    let blank_doc = Document::parse(concat!(
        "~~~\n",
        "$quill: variant_content@0.1.0\n",
        "$kind: main\n",
        "classification: \"\"\n",
        "~~~\n",
    ))
    .expect("parses")
    .document;
    let blank_plate =
        config.compile_data(&blank_doc, test_date()).expect("compiles")["classification"].clone();
    assert_eq!(blank_plate, json!({ "value": "" }));
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
        "~~~\n$quill: variant_probe@0.1.0\n$kind: main\nclassification:\n  value: SECRET\n  controlled_by: SAF/AA\n  declassify_on: 20301231\n~~~\n",
    )
    .expect("parses")
    .document;
    let data = config.compile_data(&doc, test_date()).expect("compile_data succeeds");
    assert_eq!(data["classification"]["controlled_by"], json!("SAF/AA"));
}

/// A world may open where the position's own live shape is the schema's. A
/// typed dictionary qualifies at any depth; an element, a column and a cell do
/// not, and an object inherits their ban rather than laundering it.
#[test]
fn a_world_opens_in_a_dictionary_and_nowhere_else_below_a_card() {
    load(
        r#"    o:
      type: object
      properties:
        c:
          type: enum
          values: [A]
          variants:
            A:
              x: { type: string }
"#,
    )
    .expect("a dictionary's property opens a world");

    // Each ban reaches through an object, so the position a message names is the
    // one that carries it rather than the dictionary that laundered it.
    for (position, fields) in [
        (
            "an array element",
            r#"    a:
      type: array
      items:
        type: object
        properties:
          c:
            type: enum
            values: [A]
            variants:
              A:
                x: { type: string }
"#,
        ),
        (
            "a matrix column",
            r#"    m:
      type: matrix
      members:
        one: One
      properties:
        c:
          type: enum
          values: [A]
          variants:
            A:
              x: { type: string }
"#,
        ),
        (
            "another variant's field",
            r#"    c:
      type: enum
      values: [P]
      variants:
        P:
          o:
            type: object
            properties:
              d:
                type: enum
                values: [A]
                variants:
                  A:
                    x: { type: string }
"#,
        ),
    ] {
        let err = load_error(fields);
        assert!(
            err.contains("quill::variant_placement") && err.contains(position),
            "{position}: {err}"
        );
    }
}

#[test]
fn the_blank_of_a_variant_bearing_enum_is_the_container_holding_the_blank() {
    let schema = field("type: enum\nvalues: [A]\nvariants:\n  A:\n    x: { type: string }\n");
    assert_eq!(blank(&schema).into_json(), json!({ "value": "" }));
    assert_eq!(blank(&field("type: enum\nvalues: [A]\n")).into_json(), json!(""));
}

/// The container is a closed shape: the live world arrives complete and
/// blank-filled, so an unguarded read inside the branch a plate writes is
/// total, and a dormant world's payload never reaches the plate.
#[test]
fn the_plate_carries_exactly_the_live_world() {
    for (fields, want) in [
        ("", json!({ "value": "" })),
        ("classification:\n  value:\n", json!({ "value": "" })),
        (
            "classification:\n  value: CUI\n",
            json!({ "value": "CUI", "controlled_by": "", "category": "" }),
        ),
        (
            "classification:\n  value: UNCLASSIFIED\n  controlled_by: SAF/AA\n",
            json!({ "value": "UNCLASSIFIED" }),
        ),
        ("classification: SECRET\n", json!({ "value": "SECRET", "declassify_on": "" })),
    ] {
        assert_eq!(plate(&doc(fields)), want, "{fields}");
    }
}

#[test]
fn the_discriminant_falls_to_the_default_and_carries_its_world() {
    let yaml = quill_yaml().replace("      default: \"\"\n      variants:", "      default: CUI\n      variants:");
    let data = QuillConfig::from_yaml(&yaml).unwrap().compile_data(&doc(""), test_date()).unwrap();
    assert_eq!(
        data["classification"],
        json!({ "value": "CUI", "controlled_by": "", "category": "" })
    );
}

fn classification_row(quill: &Quill, document: &Document) -> crate::quill::resolved::ResolvedField {
    quill
        .resolve(document, test_date())
        .main
        .fields
        .into_iter()
        .find(|f| f.name == "classification")
        .expect("classification row")
}

/// `resolve()` reports the container as one cell at parity with the plate. Its
/// rung is the strongest that contributed: a container the document wrote reads
/// `authored` whichever rung filled its discriminant, an absent or present-null
/// one keeps the discriminant's rung, and a value no container is built from
/// stays raw rather than reading as a blank world the document answered.
#[test]
fn resolve_reports_the_container_as_one_cell_at_its_strongest_rung() {
    use crate::quill::resolved::FieldSource::{Authored, Default};
    let quill = quill();
    for (fields, source) in [
        ("classification:\n  value: CUI\n  controlled_by: SAF/AA\n", Authored),
        ("classification: {}\n", Authored),
        ("classification:\n  value:\n", Authored),
        ("", Default),
    ] {
        let document = doc(fields);
        let row = classification_row(&quill, &document);
        assert_eq!(row.value.as_json(), &plate(&document), "{fields}");
        assert_eq!(row.source, source, "{fields}");
    }

    let row = classification_row(&quill, &doc("classification: [CUI, SECRET]\n"));
    assert_eq!(row.source, Authored);
    assert_eq!(row.value.as_json(), &json!(["CUI", "SECRET"]));

    let defaulted = quill_from_yaml(&quill_yaml().replace(
        "      default: \"\"\n      variants:",
        "      default: CUI\n      variants:",
    ));
    for (fields, source, controlled_by) in [
        ("classification: {}\n", Authored, ""),
        ("classification:\n  controlled_by: SAF/AA\n", Authored, "SAF/AA"),
        ("classification:\n", Default, ""),
    ] {
        let row = classification_row(&defaulted, &doc(fields));
        assert_eq!(
            row.value.as_json(),
            &json!({ "value": "CUI", "controlled_by": controlled_by, "category": "" }),
            "{fields}"
        );
        assert_eq!(row.source, source, "{fields}");
    }
}

/// Flipping a discriminant in an editor must not cost the author their answers,
/// and must not hand them a document that refuses to render.
#[test]
fn a_stranded_value_is_kept_and_warned_never_gated() {
    let document = doc("classification:\n  value: UNCLASSIFIED\n  controlled_by: SAF/AA\n");
    let diags = quill().validate(&document);
    let stranded = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::out_of_variant"))
        .expect("out_of_variant warning");
    assert_eq!(stranded.severity, crate::error::Severity::Warning);
    assert_eq!(
        stranded.path.as_deref(),
        Some("main.classification.controlled_by")
    );
    assert_eq!(stranded.args["variant"], json!("CUI"));
    // Kept in the document…
    assert_eq!(
        document.main().payload().get("classification").unwrap().as_json()["controlled_by"],
        json!("SAF/AA")
    );
    // …and dropped from the wire.
    assert_eq!(plate(&document), json!({ "value": "UNCLASSIFIED" }));
}

/// A key no variant declares is an undeclared field, which every other surface
/// carries without comment; only a key some *other* world owns is the stranded
/// case worth naming.
#[test]
fn an_undeclared_key_draws_no_variant_warning() {
    let document = doc("classification:\n  value: CUI\n  note: hello\n");
    assert!(!quill()
        .validate(&document)
        .iter()
        .any(|d| d.code.as_deref() == Some("validation::out_of_variant")));
}

#[test]
fn the_domain_check_lands_on_the_discriminant_path() {
    let found = codes(&doc("classification:\n  value: NATO\n"));
    assert!(found.contains(&(
        "validation::enum_violation".to_string(),
        "main.classification.value".to_string()
    )));
}

/// The live world's fields still type-check; a dormant world's do not, since
/// nothing downstream reads them.
#[test]
fn only_the_live_worlds_fields_are_type_checked() {
    let bad_live = codes(&doc("classification:\n  value: CUI\n  controlled_by: [1, 2]\n"));
    assert!(bad_live
        .iter()
        .any(|(code, path)| code == "validation::type_mismatch"
            && path == "main.classification.controlled_by"));

    let bad_dormant = codes(&doc(
        "classification:\n  value: UNCLASSIFIED\n  controlled_by: [1, 2]\n",
    ));
    assert!(!bad_dormant
        .iter()
        .any(|(code, _)| code == "validation::type_mismatch"));
}

/// A blueprint is a document, so only one world can be live: the rest are the
/// same cells, commented out.
#[test]
fn the_blueprint_comments_out_every_world_it_does_not_show() {
    let bp = config().blueprint();
    assert!(bp.contains("classification: # enum<UNCLASSIFIED | CUI | SECRET>"));
    assert!(
        bp.contains(concat!(
            "  value: \"\"\n",
            "  # when CUI:\n",
            "  # controlled_by: # string\n",
            "  # category: \"\" # string\n",
            "  # when SECRET:\n",
            "  # declassify_on: # string\n",
        )),
        "{bp}"
    );

    // The blank world is selected and owns no field set, so nothing is live: a
    // commented cell reaches the payload as a comment or not at all.
    let parsed = Document::parse(&bp).expect("blueprint round-trips").document;
    let classification = parsed
        .main()
        .payload()
        .get("classification")
        .expect("the container survives")
        .as_json()
        .clone();
    assert_eq!(classification, json!({ "value": "" }), "{bp}");

    assert_eq!(parsed.to_markdown(), bp);
}

#[test]
fn the_blueprint_emits_the_default_worlds_cells_and_round_trips() {
    let yaml = quill_yaml().replace("      default: \"\"\n      variants:", "      default: CUI\n      variants:");
    let config = QuillConfig::from_yaml(&yaml).unwrap();
    let bp = config.blueprint();
    assert!(bp.contains("value: CUI"));
    assert!(bp.contains("controlled_by:"));
    // The blueprint round-trips through the parser by construction, and the
    // container survives it as the container: the shape an author edits is the
    // shape the parser reads back.
    let reparsed = Document::parse(&bp).expect("blueprint round-trips").document;
    let value = reparsed
        .main()
        .payload()
        .get("classification")
        .expect("classification survives the round trip")
        .as_json()
        .clone();
    assert_eq!(value["value"], json!("CUI"));
    assert!(value.get("controlled_by").is_some());
}

/// A cell holds a container like any field, so the blueprint expands one per
/// property. A flattened cell would hand the author a scalar slot where the
/// schema wants a mapping, and drop every property's own description and
/// `default:`.
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
            "  # Controlling office.\n",
            "  controlled_by: # object\n",
            "    # Office symbol.\n",
            "    office: # string\n",
            "    phone: \"\" # string\n",
            "  citations: # array<object>\n",
            "    - src: # string\n",
        )),
        "{bp}"
    );

    let document = Document::parse(&bp).expect("the blueprint parses").document;
    let reparsed = Document::parse(&document.to_markdown())
        .expect("re-emit parses")
        .document;
    assert_eq!(document, reparsed, "the expansion round-trips");
}

#[test]
fn the_transform_schema_flattens_every_world_into_one_container() {
    let schema = build_transform_schema(&config());
    let json = schema.as_json();
    let cls = &json["properties"]["classification"];
    assert_eq!(cls["type"], json!("object"));
    assert_eq!(
        cls["properties"]["value"]["enum"],
        json!(["", "UNCLASSIFIED", "CUI", "SECRET"])
    );
    assert_eq!(cls["properties"]["controlled_by"]["type"], json!("string"));
    assert_eq!(cls["properties"]["declassify_on"]["type"], json!("string"));
    // `variants:` on the declaration view states which member owns a cell.
    for name in ["controlled_by", "category", "declassify_on"] {
        let cell = cls["properties"][name].as_object().expect("cell projects as an object");
        assert!(!cell.keys().any(|k| k.starts_with("quillmark:variant")), "{name}");
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
}

/// A container-shaped schema literal is refused at load, in either slot, and the
/// diagnostic names the discriminant spelling that works.
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
            "the hint names the discriminant spelling, got: {:?}",
            diag.hint
        );
    }
}

/// A world inside a typed dictionary. The dictionary's own shape is the
/// schema's, so the gap between declared and live shape stays exactly one level
/// deep — it sits one step further down the address, which is the whole of what
/// this position changes. Each test below pins one surface to that reading.
const NESTED_YAML: &str = r#"
quill:
  name: nested_probe
  version: "0.1.0"
  backend: typst
  description: A world inside a typed dictionary

typst:
  plate_file: plate.typ

main:
  fields:
    header:
      type: object
      description: Banner block.
      properties:
        office: { type: string, default: "" }
        classification:
          type: enum
          values: [UNCLASSIFIED, CUI]
          default: ""
          description: Marking shown in the banner.
          variants:
            CUI:
              controlled_by: { type: string }
              category: { type: string, default: "" }
"#;

fn nested_doc(fields: &str) -> Document {
    let markdown = format!("~~~\n$quill: nested_probe@0.1.0\n$kind: main\n{fields}~~~\n");
    Document::parse(&markdown).expect("document parses").document
}

fn nested_header(document: &Document) -> serde_json::Value {
    QuillConfig::from_yaml(NESTED_YAML)
        .expect("loads")
        .compile_data(document, test_date())
        .expect("compile_data succeeds")["header"]
        .clone()
}

/// The render floor's three answers, one address down: the blank container in an
/// empty document, the live world complete, and a dormant cell off the wire.
#[test]
fn a_nested_world_reaches_the_plate_as_the_closed_shape() {
    assert_eq!(
        nested_header(&nested_doc("")),
        json!({ "office": "", "classification": { "value": "" } })
    );
    assert_eq!(
        nested_header(&nested_doc("header:\n  classification:\n    value: CUI\n")),
        json!({
            "office": "",
            "classification": { "value": "CUI", "controlled_by": "", "category": "" }
        })
    );
    assert_eq!(
        nested_header(&nested_doc(
            "header:\n  classification:\n    value: UNCLASSIFIED\n    controlled_by: SAF/AA\n"
        )),
        json!({ "office": "", "classification": { "value": "UNCLASSIFIED" } })
    );
}

/// The strand anchors through the dictionary: the walk finds the container by
/// descending an object's declared properties rather than by scanning card
/// fields.
#[test]
fn a_nested_worlds_strand_names_the_path_through_the_dictionary() {
    let document = nested_doc(
        "header:\n  classification:\n    value: UNCLASSIFIED\n    controlled_by: SAF/AA\n",
    );
    let stranded = quill_from_yaml(NESTED_YAML)
        .validate(&document)
        .into_iter()
        .find(|d| d.code.as_deref() == Some("validation::out_of_variant"))
        .expect("out_of_variant warning");
    assert_eq!(stranded.severity, crate::error::Severity::Warning);
    assert_eq!(
        stranded.path.as_deref(),
        Some("main.header.classification.controlled_by")
    );
    assert_eq!(stranded.args["variant"], json!("CUI"));

    // Kept in the document, as it is at card level: only the wire is strict.
    assert_eq!(
        document.main().payload().get("header").unwrap().as_json()["classification"]
            ["controlled_by"],
        json!("SAF/AA")
    );
}

/// The worlds seat themselves in whichever container holds the discriminant, so
/// a dormant block lands at the dictionary's indent and what a reader uncomments
/// is the line the live world would show.
#[test]
fn the_blueprint_seats_a_nested_worlds_cells_in_the_dictionary() {
    let bp = QuillConfig::from_yaml(NESTED_YAML).expect("loads").blueprint();
    assert!(
        bp.contains(concat!(
            "  classification: # enum<UNCLASSIFIED | CUI>\n",
            "    value: \"\"\n",
            "    # when CUI:\n",
            "    # controlled_by: # string\n",
            "    # category: \"\" # string\n",
        )),
        "{bp}"
    );

    let parsed = Document::parse(&bp).expect("blueprint round-trips").document;
    assert_eq!(
        parsed.main().payload().get("header").unwrap().as_json()["classification"],
        json!({ "value": "" }),
        "{bp}"
    );
    assert_eq!(parsed.to_markdown(), bp);

    let yaml = NESTED_YAML.replace("          default: \"\"\n", "          default: CUI\n");
    let live = QuillConfig::from_yaml(&yaml).expect("loads").blueprint();
    assert!(live.contains("    controlled_by: # string\n"), "{live}");
}

/// A cell stays unconditionally addressable while only conditionally live: the
/// union crosses as the dictionary's own property, so `header.classification.poc`
/// resolves against the schema whichever world a document later selects.
#[test]
fn a_nested_cell_is_addressable_through_the_dictionary() {
    let config = QuillConfig::from_yaml(NESTED_YAML).expect("loads");
    let schema = build_transform_schema(&config);
    let cls = &schema.as_json()["properties"]["header"]["properties"]["classification"];
    assert_eq!(cls["type"], json!("object"));
    assert_eq!(cls["properties"]["value"]["enum"], json!(["", "UNCLASSIFIED", "CUI"]));
    assert_eq!(cls["properties"]["controlled_by"]["type"], json!("string"));
    assert_eq!(cls["properties"]["category"]["type"], json!("string"));
}

