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

fn plate(document: &Document) -> serde_json::Value {
    config()
        .compile_data(document)
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

#[test]
fn a_variant_field_named_value_collides_with_the_discriminant() {
    let err = load_error(
        "    c:\n      type: enum\n      values: [A]\n      variants:\n        A:\n          value: { type: string }\n",
    );
    assert!(err.contains("quill::variant_reserved_field_name"));
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
        let cell = config.main.fields["c"]
            .variant_field("x")
            .unwrap_or_else(|| panic!("type: {ty} cell resolves"));
        assert_eq!(cell.r#type.as_str(), ty);
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
            .all(|d| d.severity != crate::Severity::Error),
        "a variant content cell validates: {:?}",
        quill_from_yaml(YAML).validate(&document)
    );

    let plate = config.compile_data(&document).expect("compiles")["classification"].clone();
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
    let blank_plate = config.compile_data(&blank_doc).expect("compiles")["classification"].clone();
    assert_eq!(blank_plate, json!({ "value": "" }));
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
    let data = config.compile_data(&doc).expect("compile_data succeeds");
    assert_eq!(data["classification"]["controlled_by"], json!("SAF/AA"));
}

#[test]
fn variants_below_card_level_is_a_load_error() {
    let err = load_error(
        "    o:\n      type: object\n      properties:\n        c:\n          type: enum\n          values: [A]\n          variants:\n            A:\n              x: { type: string }\n",
    );
    assert!(err.contains("quill::variant_placement"));
}

#[test]
fn the_blank_of_a_variant_bearing_enum_is_the_container_holding_the_blank() {
    let schema = field(
        "type: enum\nvalues: [A]\nvariants:\n  A:\n    x: { type: string }\n",
    );
    assert_eq!(blank(&schema).into_json(), json!({ "value": "" }));
}

#[test]
fn a_variantless_enum_still_blanks_to_the_bare_string() {
    let schema = field("type: enum\nvalues: [A]\n");
    assert_eq!(blank(&schema).into_json(), json!(""));
}

#[test]
fn an_empty_document_renders_the_container_with_no_variant_fields() {
    assert_eq!(plate(&doc("")), json!({ "value": "" }));
}

/// What makes an unguarded read total inside the branch a plate writes.
#[test]
fn the_live_world_arrives_complete_and_blank_filled() {
    assert_eq!(
        plate(&doc("classification:\n  value: CUI\n")),
        json!({ "value": "CUI", "controlled_by": "", "category": "" })
    );
}

/// The container is a closed shape, so a payload cannot arrive under a tag that
/// disowns it.
#[test]
fn a_dormant_worlds_fields_never_reach_the_plate() {
    assert_eq!(
        plate(&doc(
            "classification:\n  value: UNCLASSIFIED\n  controlled_by: SAF/AA\n"
        )),
        json!({ "value": "UNCLASSIFIED" })
    );
}

/// The hand-authored spelling of a world with nothing filled in.
#[test]
fn a_bare_scalar_is_adopted_as_the_discriminant() {
    assert_eq!(
        plate(&doc("classification: SECRET\n")),
        json!({ "value": "SECRET", "declassify_on": "" })
    );
}

#[test]
fn the_discriminant_falls_to_the_default_and_carries_its_world() {
    let yaml = quill_yaml().replace("      default: \"\"\n      variants:", "      default: CUI\n      variants:");
    let config = QuillConfig::from_yaml(&yaml).unwrap();
    let data = config.compile_data(&doc("")).unwrap();
    assert_eq!(
        data["classification"],
        json!({ "value": "CUI", "controlled_by": "", "category": "" })
    );
}

/// Null ≡ absent holds through the container.
#[test]
fn a_null_discriminant_blank_fills() {
    assert_eq!(
        plate(&doc("classification:\n  value:\n")),
        json!({ "value": "" })
    );
}

/// `resolve()`'s contract is byte-parity with the plate, the container being one
/// cell carrying one rung as a typed dictionary is.
#[test]
fn resolve_reports_the_container_as_one_cell_matching_the_plate() {
    let quill = quill();
    let document = doc("classification:\n  value: CUI\n  controlled_by: SAF/AA\n");
    let resolved = quill.resolve(&document);
    let row = resolved
        .main
        .fields
        .iter()
        .find(|f| f.name == "classification")
        .expect("classification row");
    assert_eq!(row.value.as_json(), &plate(&document));
    assert_eq!(row.source, crate::quill::resolved::FieldSource::Authored);

    // An unanswered container reports the rung that supplied its discriminant:
    // here the schema's `default: ""`, exactly as a plain enum would.
    let blank_doc = doc("");
    let resolved = quill.resolve(&blank_doc);
    let row = resolved
        .main
        .fields
        .iter()
        .find(|f| f.name == "classification")
        .unwrap();
    assert_eq!(row.value.as_json(), &plate(&blank_doc));
    assert_eq!(row.source, crate::quill::resolved::FieldSource::Default);
}

fn classification_row(quill: &Quill, document: &Document) -> crate::quill::resolved::ResolvedField {
    quill
        .resolve(document)
        .main
        .fields
        .into_iter()
        .find(|f| f.name == "classification")
        .expect("classification row")
}

/// A container the document wrote reads `authored` whichever rung filled its
/// discriminant (`prose/canon/SCHEMAS.md` § "The resolved-value view"), so the
/// reported rung does not turn on whether the schema happens to carry a
/// `default:`.
#[test]
fn a_present_container_reads_authored_whichever_rung_filled_the_tag() {
    let quill = quill();
    for fields in ["classification: {}\n", "classification:\n  value:\n"] {
        let document = doc(fields);
        let row = classification_row(&quill, &document);
        assert_eq!(row.value.as_json(), &plate(&document));
        assert_eq!(
            row.source,
            crate::quill::resolved::FieldSource::Authored,
            "{fields}"
        );
    }

    // The value the row carries is still the `default:` member's blank-filled
    // world: only the rung the container reports changes.
    let yaml = quill_yaml().replace(
        "      default: \"\"\n      variants:",
        "      default: CUI\n      variants:",
    );
    let defaulted = quill_from_yaml(&yaml);
    let row = classification_row(&defaulted, &doc("classification: {}\n"));
    assert_eq!(
        row.value.as_json(),
        &json!({ "value": "CUI", "controlled_by": "", "category": "" })
    );
    assert_eq!(row.source, crate::quill::resolved::FieldSource::Authored);

    // A present-null container is absent, so it keeps the discriminant's rung.
    let row = classification_row(&defaulted, &doc("classification:\n"));
    assert_eq!(row.source, crate::quill::resolved::FieldSource::Default);
}

/// A value the container cannot be built from stays raw, as a mis-shaped
/// `array` or typed dictionary does: `resolve()` labels the row Authored, and a
/// blank world under that label would read as an answer the document gave.
#[test]
fn a_mis_shaped_container_value_stays_raw() {
    let quill = quill();
    let document = doc("classification: [CUI, SECRET]\n");
    let row = quill
        .resolve(&document)
        .main
        .fields
        .into_iter()
        .find(|f| f.name == "classification")
        .expect("classification row");

    assert_eq!(row.source, crate::quill::resolved::FieldSource::Authored);
    assert_eq!(row.value.as_json(), &json!(["CUI", "SECRET"]));
}

/// The container is a namespace like any other: its rung is the strongest that
/// contributed, so a cell the document wrote lifts a container whose
/// discriminant came from the schema.
#[test]
fn an_authored_cell_lifts_a_defaulted_discriminant() {
    let yaml = quill_yaml().replace(
        "      default: \"\"\n      variants:",
        "      default: CUI\n      variants:",
    );
    let quill = quill_from_yaml(&yaml);
    let document = doc("classification:\n  controlled_by: SAF/AA\n");
    let row = quill
        .resolve(&document)
        .main
        .fields
        .into_iter()
        .find(|f| f.name == "classification")
        .expect("classification row");
    assert_eq!(
        row.value.as_json(),
        &json!({ "value": "CUI", "controlled_by": "SAF/AA", "category": "" })
    );
    assert_eq!(row.source, crate::quill::resolved::FieldSource::Authored);
}

/// The conditional-obligation payoff: a field with no `default:` is obliged in
/// its own world and silent everywhere else — the thing `must_fill` alone cannot
/// say.
#[test]
fn obligation_follows_the_selected_world() {
    let obliged = codes(&doc("classification:\n  value: CUI\n"));
    assert!(obliged.contains(&(
        "validation::must_fill".to_string(),
        "main.classification.controlled_by".to_string()
    )));
    // `category` declares a `default:`, so it stays skippable in the same world.
    assert!(!obliged
        .iter()
        .any(|(_, path)| path == "main.classification.category"));

    // The identical schema obliges nothing once another world is selected.
    let quiet = codes(&doc("classification:\n  value: UNCLASSIFIED\n"));
    assert!(!quiet
        .iter()
        .any(|(_, path)| path.starts_with("main.classification.")));
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
    assert_eq!(stranded.severity, crate::Severity::Warning);
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

/// A blueprint is a document, so it shows one world and *names* the rest.
#[test]
fn the_blueprint_shows_one_world_and_names_the_others() {
    let bp = config().blueprint();
    assert!(bp.contains("# when CUI: controlled_by, category"));
    assert!(bp.contains("# when SECRET: declassify_on"));
    assert!(bp.contains("classification: # enum<UNCLASSIFIED | CUI | SECRET>"));
    assert!(bp.contains("value: \"\""));
    // The blank world owns no field set, so no variant cell is emitted.
    assert!(!bp.contains("controlled_by:"));
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
            "  # Controlling office.\n",
            "  controlled_by: # object\n",
            "    # Office symbol.\n",
            "    office: !must_fill # string\n",
            "    phone: \"\" # string\n",
            "  citations: # array<object>\n",
            "    - src: !must_fill # string\n",
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
        warned.contains(&"main.classification.controlled_by.office".to_string())
            && warned.contains(&"main.classification.citations[0].src".to_string()),
        "{warned:?}"
    );
    assert!(
        !warned
            .iter()
            .any(|p| p == "main.classification.controlled_by"),
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
        );
    let quill = quill_from_yaml(&yaml);
    let seeded = quill.seed_document();
    let value = seeded
        .main()
        .payload()
        .get("classification")
        .expect("seeded classification")
        .as_json()
        .clone();
    assert_eq!(value["value"], json!("CUI"));
    assert_eq!(value["controlled_by"], json!("SAF/AA"));
    // `declassify_on` belongs to a world the seed did not select.
    assert!(value.get("declassify_on").is_none());
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
}

/// `variants:` on the declaration view states it instead, keyed by member.
#[test]
fn the_transform_schema_does_not_restate_which_member_owns_a_cell() {
    let schema = build_transform_schema(&config());
    let cls = &schema.as_json()["properties"]["classification"];
    for name in ["controlled_by", "category", "declassify_on"] {
        assert_eq!(
            cls["properties"][name]
                .as_object()
                .expect("variant cell projects as an object")
                .keys()
                .filter(|k| k.starts_with("quillmark:variant"))
                .count(),
            0
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

/// `usaf_memo` ships a blank `default:` on a variant-bearing enum, so the scalar
/// spelling is load-bearing rather than merely tolerated.
#[test]
fn a_scalar_schema_literal_stays_legal_on_a_variant_bearing_enum() {
    let plate = config()
        .compile_data(&doc(""))
        .expect("a blank scalar `default:` loads and compiles");
    assert_eq!(plate["classification"]["value"], json!(""));
}
