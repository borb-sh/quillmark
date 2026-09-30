//! `type: matrix`: a vocabulary someone ticks, a member held by being present.
//!
//! A matrix is sugar over a typed dictionary, so the risk is not that the
//! inherited walks break but that what the type *adds* disagrees with them:
//! presence as the tick, the roster written onto the wire, the wire closed over
//! an unheld member, and an open matrix's added items. Each test below pins one
//! surface to the same reading of the same schema.

use crate::document::Document;
use crate::quill::{
    blank, build_transform_schema, quill_from_yaml, test_date, Quill, QuillConfig,
};
use crate::value::QuillValue;
use serde_json::json;

/// A four-member roster with one column.
fn quill_yaml() -> &'static str {
    r#"
quill:
  name: matrix_probe
  version: "0.1.0"
  backend: typst
  description: Matrix probe

typst:
  plate_file: plate.typ

main:
  fields:
    qualifications:
      type: matrix
      members:
        sq_cc_candidate: Sq/CC Candidate
        flight_cc: Flight CC
        dodin_ops: DODIN Ops
        cyber_200: Cyber 200
      properties:
        detail: { type: plaintext, inline: true, default: "" }
    title:
      type: string
      default: ""
"#
}

/// The probe with its roster open to added items.
fn open_yaml() -> String {
    quill_yaml().replace("      type: matrix\n", "      type: matrix\n      open: true\n")
}

fn config() -> QuillConfig {
    QuillConfig::from_yaml(quill_yaml()).expect("matrix probe loads")
}

fn quill() -> Quill {
    quill_from_yaml(quill_yaml())
}

fn doc(fields: &str) -> Document {
    let markdown = format!("~~~\n$quill: matrix_probe@0.1.0\n$kind: main\n{fields}~~~\n");
    Document::parse(&markdown).expect("document parses").document
}

/// The matrix as the plate receives it.
fn plate_of(yaml: &str, document: &Document) -> serde_json::Value {
    QuillConfig::from_yaml(yaml)
        .expect("probe loads")
        .compile_data(document, test_date())
        .expect("compile_data succeeds")["qualifications"]
        .clone()
}

fn plate(document: &Document) -> serde_json::Value {
    plate_of(quill_yaml(), document)
}

fn codes_of(quill: &Quill, document: &Document) -> Vec<(String, String)> {
    quill
        .validate(document)
        .into_iter()
        .map(|d| (d.code.unwrap_or_default(), d.path.unwrap_or_default()))
        .collect()
}

fn codes(document: &Document) -> Vec<(String, String)> {
    codes_of(&quill(), document)
}

fn load_error(backend: &str, fields: &str) -> String {
    let yaml = format!(
        "quill:\n  name: bad\n  version: \"0.1.0\"\n  backend: {backend}\n  description: bad\n\
         main:\n  fields:\n{fields}\n"
    );
    format!("{:?}", QuillConfig::from_yaml(&yaml).expect_err("expected a load error"))
}

/// The stored-spelling table: a member present in the document is held,
/// whatever it carries, and one absent, `false` or null is not. An unheld
/// member's columns reach the wire at their blanks, not their defaults.
#[test]
fn a_member_is_held_by_being_present() {
    let yaml = quill_yaml().replace("default: \"\" }", "default: TBD }");
    let wire = plate_of(
        &yaml,
        &doc(concat!(
            "qualifications:\n",
            "  flight_cc: true\n",
            "  dodin_ops: { detail: X }\n",
            "  cyber_200: {}\n",
            "  sq_cc_candidate: false\n",
        )),
    );
    for (id, held, detail) in [
        ("flight_cc", true, "TBD"),
        ("dodin_ops", true, "X"),
        ("cyber_200", true, "TBD"),
        ("sq_cc_candidate", false, ""),
    ] {
        assert_eq!(wire[id]["held"], json!(held), "{id}");
        assert_eq!(wire[id]["detail"]["text"], json!(detail), "{id}");
    }

    let absent = plate_of(&yaml, &doc("qualifications:\n  flight_cc: null\n"));
    assert_eq!(absent["flight_cc"]["held"], json!(false));
    assert_eq!(absent["flight_cc"]["detail"]["text"], json!(""));
}

/// Presence is the tick, so a mapping storing `held` contradicts or repeats
/// it. Either value is refused, and the refusal gates the render; a null is
/// absent, as at every type.
#[test]
fn a_stored_tick_is_refused() {
    for spelling in ["{ held: true, detail: X }", "{ held: false }"] {
        let document = doc(&format!("qualifications:\n  flight_cc: {spelling}\n"));
        assert_eq!(
            codes(&document),
            [(
                "validation::held_stored".to_string(),
                "main.qualifications.flight_cc.held".to_string()
            )],
            "{spelling}"
        );
        assert!(config().compile_data(&document, test_date()).is_err(), "{spelling}");
    }
    assert!(codes(&doc("qualifications:\n  flight_cc: { held: null }\n")).is_empty());
}

/// The refusal holds wherever a matrix nests, and a quill's own literals are
/// refused at load under the literal's code family.
#[test]
fn a_stored_tick_is_refused_at_every_depth() {
    let yaml = r#"
quill: { name: deep, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    rows:
      type: array
      items:
        type: object
        properties:
          quals: { type: matrix, members: { flight_cc: Flight CC } }
card_kinds:
  entry:
    fields:
      quals: { type: matrix, members: { flight_cc: Flight CC } }
"#;
    let quill = quill_from_yaml(yaml);
    let markdown = concat!(
        "~~~\n$quill: deep@1.0.0\n$kind: main\n",
        "rows:\n  - quals: { flight_cc: { held: true } }\n~~~\n\n",
        "~~~\n$kind: entry\nquals: { flight_cc: { held: false } }\n~~~\n",
    );
    let document = Document::parse(markdown).expect("parses").document;
    let found = codes_of(&quill, &document);
    for path in ["main.rows[0].quals.flight_cc.held", "cards.entry[0].quals.flight_cc.held"] {
        assert!(
            found.contains(&("validation::held_stored".to_string(), path.to_string())),
            "{path} in {found:?}"
        );
    }
    assert!(quill.compile_data(&document, test_date()).is_err());

    for (literal, code) in [
        (
            "      default: [ { quals: { flight_cc: { held: false } } } ]\n",
            "quill::default_held_stored",
        ),
        ("", "quill::seed_held_stored"),
    ] {
        let bad = yaml
            .replace("      type: array\n", &format!("      type: array\n{literal}"))
            .replace(
                "  entry:\n",
                if literal.is_empty() {
                    "  entry:\n    seed:\n      quals: { flight_cc: { held: true } }\n"
                } else {
                    "  entry:\n"
                },
            );
        let error = format!("{:?}", QuillConfig::from_yaml(&bad).expect_err("refused at load"));
        assert!(error.contains(code), "{code}: {error}");
    }
}

/// The typed write lands one spelling for a held member with nothing else
/// answered, and keeps every other as it came.
#[test]
fn a_held_member_with_no_answers_rests_as_the_bare_tick() {
    let markdown = concat!(
        "~~~\n$quill: matrix_probe@0.1.0\n$kind: main\n",
        "qualifications:\n  flight_cc: {}\n  dodin_ops: { detail: X }\n  cyber_200: false\n",
        "~~~\n",
    );
    let quill = quill();
    let mut parsed = quill.parse(markdown).expect("parses and conforms");
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(
        parsed.document.main().payload().get("qualifications").unwrap().as_json(),
        &json!({ "flight_cc": true, "dodin_ops": { "detail": "X" }, "cyber_200": false })
    );

    quill
        .writer(&mut parsed.document)
        .set(
            "qualifications",
            QuillValue::from_json(json!({ "sq_cc_candidate": {}, "flight_cc": "true" })),
        )
        .expect("the typed write lands");
    assert_eq!(
        parsed.document.main().payload().get("qualifications").unwrap().as_json(),
        &json!({ "sq_cc_candidate": true, "flight_cc": true })
    );
}

/// Total at the plate, in declaration order, carrying the labels the roster
/// holds: a plate prints the whole vocabulary without a second copy of it, and
/// an authored `title` on a roster member is overwritten rather than carried,
/// and warned at.
#[test]
fn the_projection_is_total_and_carries_the_roster() {
    let document = doc("qualifications:\n  cyber_200: true\n  flight_cc: { title: Forged }\n");
    assert_eq!(
        codes(&document),
        [(
            "validation::unknown_field".to_string(),
            "main.qualifications.flight_cc.title".to_string()
        )]
    );
    let wire = plate(&document);
    let members = wire.as_object().expect("a matrix projects as a mapping");

    assert_eq!(
        members.keys().collect::<Vec<_>>(),
        ["sq_cc_candidate", "flight_cc", "dodin_ops", "cyber_200"],
        "every member present, in declaration order"
    );
    assert_eq!(wire["flight_cc"]["title"], json!("Flight CC"));
}

/// The roster is what a document may name, as an enum's `values:` is.
#[test]
fn a_member_outside_the_roster_is_refused() {
    let found = codes(&doc("qualifications:\n  ghost: { title: Ghost }\n"));
    assert!(
        found.contains(&(
            "validation::enum_violation".to_string(),
            "main.qualifications.ghost".to_string()
        )),
        "expected a closed-domain violation, got {found:?}"
    );
}

/// An open matrix prints its roster in roster order, then each item a document
/// adds in id order, carrying its own title and its columns. Document equality
/// ignores a mapping's key order, so two documents it calls equal compose one
/// plate. An added item titled as a roster member is still the author's answer:
/// it prints and draws nothing.
#[test]
fn an_open_matrix_prints_added_items_after_the_roster_in_id_order() {
    let yaml = open_yaml();
    let document = doc(concat!(
        "qualifications:\n",
        "  wing_ig: { title: Wing IG, detail: 81 TRW }\n",
        "  flight_cc: true\n",
        "  aide: { title: Flight CC }\n",
    ));
    assert!(codes_of(&quill_from_yaml(&yaml), &document).is_empty());

    let wire = plate_of(&yaml, &document);
    let members = wire.as_object().expect("a matrix projects as a mapping");
    assert_eq!(
        members.keys().collect::<Vec<_>>(),
        ["sq_cc_candidate", "flight_cc", "dodin_ops", "cyber_200", "aide", "wing_ig"],
    );
    let reordered = doc(concat!(
        "qualifications:\n",
        "  aide: { title: Flight CC }\n",
        "  flight_cc: true\n",
        "  wing_ig: { title: Wing IG, detail: 81 TRW }\n",
    ));
    assert_eq!(reordered, document);
    let rewire = plate_of(&yaml, &reordered);
    assert_eq!(
        rewire.as_object().unwrap().keys().collect::<Vec<_>>(),
        members.keys().collect::<Vec<_>>()
    );
    assert_eq!(wire["wing_ig"]["held"], json!(true));
    assert_eq!(wire["wing_ig"]["title"], json!("Wing IG"));
    assert_eq!(wire["wing_ig"]["detail"]["text"], json!("81 TRW"));
    assert_eq!(wire["aide"]["title"], json!("Flight CC"));
    assert_eq!(wire["aide"]["detail"]["text"], json!(""));
}

/// An open matrix's domain is its roster and every key spelled as a member id
/// whose mapping carries a `title`. Anything else is the closed matrix's fault,
/// hinted toward the added spelling.
#[test]
fn a_key_outside_an_open_roster_needs_an_id_and_a_title() {
    let quill = quill_from_yaml(&open_yaml());
    for (spelling, key) in [
        ("wing_ig: true", "wing_ig"),
        ("wing_ig: { detail: X }", "wing_ig"),
        ("wing_ig: { title: null }", "wing_ig"),
        ("Wing IG: { title: Wing IG }", "Wing IG"),
    ] {
        let diags = quill.validate(&doc(&format!("qualifications:\n  {spelling}\n")));
        let [diag] = diags.as_slice() else {
            panic!("{spelling}: expected one diagnostic, got {diags:?}");
        };
        assert_eq!(diag.code.as_deref(), Some("validation::enum_violation"), "{spelling}");
        assert_eq!(diag.path.as_deref(), Some(format!("main.qualifications.{key}").as_str()));
        assert_eq!(diag.args.get("open"), Some(&json!(true)), "{spelling}");
        assert!(diag.hint.as_deref().is_some_and(|h| h.contains("`title`")), "{diag:?}");
    }

    let titled = codes_of(&quill, &doc("qualifications:\n  wing_ig: { title: { a: 1 } }\n"));
    assert_eq!(
        titled,
        [(
            "validation::type_mismatch".to_string(),
            "main.qualifications.wing_ig.title".to_string()
        )],
        "an added item's title is a string cell"
    );
}

/// An absent matrix blank-fills to every member unheld, so the plate needs no
/// guard for a document that never mentioned it.
#[test]
fn an_absent_matrix_blank_fills_to_every_member_unheld() {
    let wire = plate(&doc("title: T\n"));
    for id in ["sq_cc_candidate", "flight_cc", "dodin_ops", "cyber_200"] {
        assert_eq!(wire[id]["held"], json!(false), "{id} blanks unheld");
    }

    let field = &config().main.fields["qualifications"];
    assert_eq!(
        blank(field).as_json()["flight_cc"]["held"],
        json!(false),
        "the floor agrees with the projection"
    );
}

/// The schema fixes the keys, so a matrix holds no literal; a member id carries
/// a field key's discipline; `held` and `title` are the keys the type writes
/// itself; the roster and the type imply each other, as `open` and the type do;
/// and a form has no widget for an item a document adds.
#[test]
fn a_malformed_matrix_declaration_is_refused_by_code() {
    for (backend, fields, code) in [
        ("typst", "    m:\n      type: matrix\n      default: {}\n      members: { a: A }\n", "quill::default_on_namespace"),
        (
            "typst",
            "    m:\n      type: matrix\n      members: { \"DO / Det CC\": Label }\n",
            "quill::invalid_matrix_member",
        ),
        (
            "typst",
            "    m:\n      type: matrix\n      members: { a: A }\n      properties:\n        held: { type: string }\n",
            "quill::matrix_reserved_column",
        ),
        (
            "typst",
            "    m:\n      type: matrix\n      members: { a: A }\n      properties:\n        title: { type: string }\n",
            "quill::matrix_reserved_column",
        ),
        ("typst", "    m:\n      type: matrix\n", "quill::field_parse_error"),
        ("typst", "    m:\n      type: string\n      members: { a: A }\n", "quill::field_parse_error"),
        ("typst", "    m:\n      type: string\n      open: true\n", "quill::field_parse_error"),
        (
            "acroform",
            "    record:\n      type: object\n      properties:\n        m: { type: matrix, open: true, members: { a: A } }\n",
            "quill::open_matrix_unsupported",
        ),
    ] {
        assert!(load_error(backend, fields).contains(code), "{code}: {fields}");
    }
}

/// A namespace holds no literal, so a fresh document ticks no member.
#[test]
fn a_matrix_seeds_empty() {
    let seeded = quill_from_yaml(quill_yaml()).seed_document();
    assert!(
        seeded.main().payload().get("qualifications").is_none(),
        "a seeded document ticks nothing"
    );
}

/// A member's cells are ordinary addresses: a region on the Typst backend, a
/// widget on acroform. The transform schema is where both resolve one, and
/// where an open matrix's added item is the node every key past the roster
/// lowers against, its `title` a cell.
#[test]
fn a_members_cells_are_addressable() {
    let schema = build_transform_schema(&config());
    let matrix = &schema.as_json()["properties"]["qualifications"];
    let member = &matrix["properties"]["flight_cc"];

    assert_eq!(member["properties"]["held"]["type"], json!("boolean"));
    assert!(
        member["properties"]["detail"]["contentMediaType"].is_string(),
        "a column crosses as the content it is: {member}"
    );
    assert!(
        member["properties"].get("title").is_none(),
        "`title` is written by the projection, not held as a cell, so it carries no address"
    );
    assert!(matrix.get("additionalProperties").is_none(), "a closed matrix admits nothing");

    let open = QuillConfig::from_yaml(&open_yaml()).expect("open probe loads");
    let schema = build_transform_schema(&open);
    let item = &schema.as_json()["properties"]["qualifications"]["additionalProperties"];
    assert_eq!(item["properties"]["title"]["type"], json!("string"));
    assert!(item["properties"]["detail"]["contentMediaType"].is_string(), "{item}");
    assert_eq!(open.schema()["main"]["fields"]["qualifications"]["open"], json!(true));
}

/// The roster rides the format slot as an enum's domain does, and the body
/// carries the sparse spelling: no third annotation form, and nothing for a
/// model to delete. A closed checklist's bare tick is its whole spelling, so it
/// takes no hint line; an open one shows the added spelling beside it.
#[test]
fn the_blueprint_shows_the_vocabulary_in_the_annotation_and_ticks_nothing() {
    let checklist = quill_yaml().replace(
        "      properties:\n        detail: { type: plaintext, inline: true, default: \"\" }\n",
        "",
    );
    let bp = QuillConfig::from_yaml(&checklist).expect("loads").blueprint();
    assert!(
        bp.contains(concat!(
            "# Matrix probe\n",
            "qualifications: {} # matrix<sq_cc_candidate | flight_cc | dodin_ops | cyber_200>\n",
        )),
        "{bp}"
    );

    let open = checklist.replace("      type: matrix\n", "      type: matrix\n      open: true\n");
    let bp = QuillConfig::from_yaml(&open).expect("loads").blueprint();
    assert!(
        bp.contains(concat!(
            "# e.g. {sq_cc_candidate: true, new_item: {title: string}}\n",
            "qualifications: {} # matrix<sq_cc_candidate | flight_cc | dodin_ops | cyber_200>\n",
        )),
        "{bp}"
    );

    // The placeholder never names a member, which the pasted hint would tick.
    let taken = open.replace("        dodin_ops: DODIN Ops\n", "        new_item: New Item\n");
    let bp = QuillConfig::from_yaml(&taken).expect("loads").blueprint();
    assert!(
        bp.contains("# e.g. {sq_cc_candidate: true, new_item_2: {title: string}}\n"),
        "{bp}"
    );
}

/// A column's cap reaches every member the page prints, roster or added, and
/// no member it does not.
#[test]
fn an_overfull_column_warns_under_every_held_member() {
    let yaml = open_yaml().replace(
        "detail: { type: plaintext, inline: true, default: \"\" }",
        "units: { type: array, max: 1, items: { type: string } }",
    );
    let quill = quill_from_yaml(&yaml);
    let found = codes_of(
        &quill,
        &doc(concat!(
            "qualifications:\n",
            "  flight_cc: { units: [a, b] }\n",
            "  wing_ig: { title: Wing IG, units: [a, b] }\n",
            "  dodin_ops: false\n",
        )),
    );
    assert_eq!(
        found,
        [
            ("validation::cardinality".to_string(), "main.qualifications.flight_cc.units".to_string()),
            ("validation::cardinality".to_string(), "main.qualifications.wing_ig.units".to_string()),
        ]
    );
}

/// A matrix declaring columns names them in its `# e.g.` line, spelled as a
/// present member, and a column with nothing to show carrying its type; an open
/// one adds an item keyed by a placeholder id. Pasted into the cell, the hint is
/// a value the schema accepts once each typed column is answered.
#[test]
fn the_blueprint_hint_spells_a_held_member_with_every_column() {
    let columns = "detail: { type: plaintext, inline: true, default: \"333 TRS/DO, 2024\" }\n        \
                   unit: { type: string, default: HQ }\n        \
                   earned: { type: date }\n        \
                   logged: { type: datetime }";
    let cells = "detail: \"333 TRS/DO, 2024\", unit: HQ, earned: date<YYYY-MM-DD | today>, logged: \"datetime<YYYY-MM-DDThh:mm[:ss]>\"";
    let annotation = "qualifications: {} # matrix<sq_cc_candidate | flight_cc | dodin_ops | cyber_200>\n";
    for (yaml, hint) in [
        (quill_yaml().to_string(), format!("{{sq_cc_candidate: {{{cells}}}}}")),
        (
            open_yaml(),
            format!("{{sq_cc_candidate: {{{cells}}}, new_item: {{title: string, {cells}}}}}"),
        ),
    ] {
        let yaml = yaml.replace("detail: { type: plaintext, inline: true, default: \"\" }", columns);
        let bp = QuillConfig::from_yaml(&yaml).expect("loads").blueprint();
        assert!(bp.contains(&format!("# e.g. {hint}\n{annotation}")), "{bp}");

        let quill = quill_from_yaml(&yaml);
        let paste = |hint: &str| {
            let markdown = format!(
                "~~~\n$quill: matrix_probe@0.1.0\n$kind: main\nqualifications: {hint}\n~~~\n"
            );
            Document::parse(&markdown).expect("the hint parses").document
        };
        assert!(!quill.validate(&paste(&hint)).is_empty());

        let answered = paste(
            &hint
                .replace("date<YYYY-MM-DD | today>", "2024-05-01")
                .replace("\"datetime<YYYY-MM-DDThh:mm[:ss]>\"", "2024-05-01T09:30"),
        );
        assert!(quill.validate(&answered).is_empty(), "{:?}", quill.validate(&answered));
        let wire = quill.compile_data(&answered, test_date()).expect("compiles")["qualifications"]
            ["sq_cc_candidate"]
            .clone();
        assert_eq!(wire["held"], json!(true));
        assert_eq!(wire["detail"]["text"], json!("333 TRS/DO, 2024"));
    }
}

/// A matrix below a typed dictionary or a table row carries the same hint at
/// its own slot, and the blueprint still round-trips.
#[test]
fn a_nested_matrix_carries_its_hint_at_its_own_slot() {
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    record:
      type: object
      properties:
        quals:
          type: matrix
          members: { flight_cc: Flight CC }
          properties:
            detail: { type: string, default: Ops }
    rows:
      type: array
      items:
        type: object
        properties:
          quals:
            type: matrix
            open: true
            members: { flight_cc: Flight CC }
            properties:
              detail: { type: string, default: Cyber }
"#;
    let bp = QuillConfig::from_yaml(yaml).expect("loads").blueprint();
    assert!(
        bp.contains(concat!(
            "record: # object\n",
            "  # e.g. {flight_cc: {detail: Ops}}\n",
            "  quals: {} # matrix<flight_cc>\n",
        )),
        "{bp}"
    );
    assert!(
        bp.contains(concat!(
            "rows: # array<object>\n",
            "  -\n",
            "    # e.g. {flight_cc: {detail: Cyber}, new_item: {title: string, detail: Cyber}}\n",
            "    quals: {} # matrix<flight_cc>\n",
        )),
        "{bp}"
    );

    let doc1 = Document::parse(&bp).expect("blueprint parses").document;
    let doc2 = Document::parse(&doc1.to_markdown())
        .expect("re-emit parses")
        .document;
    assert_eq!(doc1, doc2);
}

/// A bare tick is the render floor's boolean coercion, not raw truthiness:
/// `"false"` is the case that tells them apart. A mapping is held whatever it
/// holds.
#[test]
fn the_tick_is_judged_by_the_render_floor_not_by_raw_truthiness() {
    let yaml = quill_yaml().replace(
        "detail: { type: plaintext, inline: true, default: \"\" }",
        "detail: { type: plaintext, inline: true }",
    );
    let quill = quill_from_yaml(&yaml);
    let held_at = |fields: &str| -> bool {
        let markdown = format!("~~~\n$quill: matrix_probe@0.1.0\n$kind: main\n{fields}~~~\n");
        let document = Document::parse(&markdown).expect("parses").document;
        quill.compile_data(&document, test_date()).expect("compiles")["qualifications"]["flight_cc"]
            ["held"]
            .as_bool()
            .expect("the tick is a boolean on the wire")
    };

    for (spelling, held) in [
        ("title: T\n", false),
        ("qualifications:\n  flight_cc: {}\n", true),
        ("qualifications:\n  flight_cc: true\n", true),
        ("qualifications:\n  flight_cc: false\n", false),
        ("qualifications:\n  flight_cc: \"false\"\n", false),
        ("qualifications:\n  flight_cc: \"true\"\n", true),
        ("qualifications:\n  flight_cc: 0\n", false),
        ("qualifications:\n  flight_cc: 1\n", true),
    ] {
        assert_eq!(held_at(spelling), held, "wire disagrees on {spelling:?}");
    }
}

/// A member the floor refuses is the member's own failure. The container is not
/// mis-shaped, and a sibling spelled as the bare tick is not: both would
/// otherwise report `validation::type_mismatch` at a path the author cannot act
/// on.
#[test]
fn one_members_refusal_does_not_convict_the_matrix_or_its_siblings() {
    let found = codes(&doc(
        "qualifications:\n  flight_cc: true\n  dodin_ops: [1, 2]\n",
    ));
    let mismatches: Vec<&String> = found
        .iter()
        .filter(|(code, _)| code == "validation::type_mismatch")
        .map(|(_, path)| path)
        .collect();

    assert_eq!(
        mismatches,
        [&"main.qualifications.dodin_ops.held".to_string()],
        "only the refused member reports: {found:?}"
    );
}
