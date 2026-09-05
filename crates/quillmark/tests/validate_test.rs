//! [`quillmark::Quill::validate`], the editor-facing validation surface.

use std::collections::HashMap;

use quillmark::{Document, FileTreeNode, Quill};

fn quill_from_yaml(yaml: &str) -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    let root = FileTreeNode::Directory { files };
    Quill::from_tree(root).expect("quill_from_yaml: from_tree failed")
}

const SIMPLE: &str = r#"
quill:
  name: validate_test
  version: "1.0"
  backend: typst
  description: Validate surface test

main:
  fields:
    title:
      type: string
    status:
      type: string
      default: draft
    count:
      type: integer

card_kinds:
  note:
    fields:
      label:
        type: string
"#;

#[test]
fn validate_clean_document_has_no_diagnostics() {
    let quill = quill_from_yaml(SIMPLE);
    // `status` is absent and falls back to its default.
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n\
              title: \"T\"\ncount: 1\n~~~\n";
    let doc = Document::parse(md).unwrap().document;

    assert!(
        quill.validate(&doc).is_empty(),
        "a complete, well-formed document should produce no diagnostics"
    );
}

#[test]
fn validate_forwards_type_mismatch_with_path_and_hint() {
    let quill = quill_from_yaml(SIMPLE);
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n\
              title: \"T\"\ncount: \"not-a-number\"\n~~~\n";
    let doc = Document::parse(md).unwrap().document;

    let diags = quill.validate(&doc);
    let diag = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::type_mismatch"))
        .expect("expected a type_mismatch diagnostic");
    assert_eq!(diag.path.as_deref(), Some("main.count"));
    assert!(diag.hint.is_some(), "type_mismatch should carry a hint");
}

#[test]
fn validate_reports_unknown_card_kind() {
    let quill = quill_from_yaml(SIMPLE);
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\ntitle: \"T\"\ncount: 1\n~~~\n\n\
              ~~~card-yaml\n$kind: ghost\nbody: \"B\"\n~~~\n";
    let doc = Document::parse(md).unwrap().document;

    let diags = quill.validate(&doc);
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("validation::unknown_card")),
        "expected validation::unknown_card; got: {:?}",
        diags.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn validate_warns_on_must_fill_marker() {
    let quill = quill_from_yaml(SIMPLE);
    // With and without a suggested value, on the main card and a composable one.
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n\
              title: !must_fill Draft\ncount: !must_fill\n~~~\n\n\
              ~~~card-yaml\n$kind: note\nlabel: !must_fill\n~~~\n";
    let doc = Document::parse(md).unwrap().document;

    let diags = quill.validate(&doc);
    let marked: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("validation::must_fill"))
        .inspect(|d| assert_eq!(d.severity, quillmark::Severity::Warning))
        .filter_map(|d| d.path.clone())
        .collect();
    assert!(
        marked.contains(&"main.title".to_string())
            && marked.contains(&"main.count".to_string())
            && marked.contains(&"cards.note[0].label".to_string()),
        "main-card and composable-card !must_fill markers should all warn; \
         got paths: {marked:?}"
    );
}

/// The render floor's leniencies, one per row of the type table. A value the
/// floor adopts is valid; `validate` and the render door give one verdict.
const LENIENT: &str = r#"
quill:
  name: lenient
  version: "1.0"
  backend: typst
  description: Render-floor leniencies

main:
  fields:
    caption:
      type: array
      items:
        type: string
    verified:
      type: boolean
    count:
      type: integer
    ratio:
      type: number
    heading:
      type: string
    grade:
      type: enum
      values: [alpha, beta]
    signed_on:
      type: date
    prose:
      type: richtext
    literal:
      type: plaintext
"#;

fn lenient_doc(fields: &str) -> Document {
    let md = format!("~~~card-yaml\n$quill: lenient\n$kind: main\n{fields}~~~\n");
    Document::parse(&md).unwrap().document
}

#[test]
fn validate_accepts_every_value_the_render_floor_adopts() {
    let quill = quill_from_yaml(LENIENT);
    let doc = lenient_doc(
        "caption: DEPARTMENT OF THE AIR FORCE\n\
         verified: 1\n\
         count: \"3\"\n\
         ratio: \"1.5\"\n\
         heading: [ONE LINE]\n\
         grade: alpha\n\
         signed_on: [\"2026-08-25\"]\n\
         prose: 7\n\
         literal: true\n",
    );

    let diags = quill.validate(&doc);
    assert!(
        diags.is_empty(),
        "every value here is one the render floor adopts; got: {:?}",
        diags
            .iter()
            .map(|d| (&d.code, &d.path))
            .collect::<Vec<_>>()
    );
    assert!(quill.dry_run(&doc).is_ok(), "and the render door agrees");
}

#[test]
fn validate_refuses_what_the_render_floor_refuses() {
    let quill = quill_from_yaml(LENIENT);
    // No floor adopts these.
    for (field, value) in [
        ("count", "not-a-number"),
        ("ratio", "not-a-number"),
        ("verified", "yes"),
    ] {
        let doc = lenient_doc(&format!("{field}: \"{value}\"\n"));
        let diags = quill.validate(&doc);
        assert!(
            diags
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::type_mismatch")
                    && d.path.as_deref() == Some(&format!("main.{field}")[..])),
            "`{field}: {value}` should be a type_mismatch; got: {:?}",
            diags.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
        assert!(
            quill.dry_run(&doc).is_err(),
            "`{field}: {value}` does not render either"
        );
    }

    let doc = lenient_doc("heading:\n  a: 1\n");
    assert!(
        quill
            .validate(&doc)
            .iter()
            .any(|d| d.code.as_deref() == Some("validation::type_mismatch")
                && d.path.as_deref() == Some("main.heading")),
        "an object is not a string the floor can build"
    );
    assert!(quill.dry_run(&doc).is_err());
}

#[test]
fn validate_refuses_a_well_shaped_value_the_floor_cannot_conform() {
    let quill = quill_from_yaml(LENIENT);
    // A content field rests in an object and an `integer` in an integer
    // literal, so only the floor's conformance separates these from valid
    // values.
    for (field, authored) in [
        ("prose", "prose:\n  prose: older\n"),
        ("literal", "literal:\n  prose: older\n"),
        ("count", "count: 18446744073709551615\n"),
    ] {
        let doc = lenient_doc(authored);
        let diags = quill.validate(&doc);
        assert!(
            diags
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::type_mismatch")
                    && d.path.as_deref() == Some(&format!("main.{field}")[..])),
            "`{authored}` should be a type_mismatch at `main.{field}`; got: {:?}",
            diags
                .iter()
                .map(|d| (&d.code, &d.path))
                .collect::<Vec<_>>()
        );
        assert!(
            quill.dry_run(&doc).is_err(),
            "`{authored}` does not render either"
        );
    }
}

#[test]
fn validate_reports_a_refused_element_without_mistyping_its_siblings() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: elems
  version: "1.0"
  backend: typst
  description: element-wise conformance
main:
  fields:
    counts:
      type: array
      items:
        type: integer
"#,
    );
    // `true` is a value the floor adopts for an integer; `"abc"` is not.
    let md = "~~~card-yaml\n$quill: elems\n$kind: main\ncounts: [true, \"abc\"]\n~~~\n";
    let doc = Document::parse(md).unwrap().document;

    let mismatched: Vec<_> = quill
        .validate(&doc)
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("validation::type_mismatch"))
        .filter_map(|d| d.path)
        .collect();
    assert_eq!(
        mismatched,
        vec!["main.counts[1]".to_string()],
        "only the element the floor refused is a mismatch"
    );
}

#[test]
fn validate_checks_the_enum_domain_of_a_scalar_the_floor_stringifies() {
    let quill = quill_from_yaml(LENIENT);
    // `5` reaches the render floor as the string `"5"`, which is out of domain.
    let doc = lenient_doc("grade: 5\n");

    let diags = quill.validate(&doc);
    let diag = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::enum_violation"))
        .unwrap_or_else(|| {
            panic!(
                "expected enum_violation; got: {:?}",
                diags.iter().map(|d| &d.code).collect::<Vec<_>>()
            )
        });
    assert_eq!(diag.path.as_deref(), Some("main.grade"));
    assert!(quill.dry_run(&doc).is_err());
}

#[test]
fn validate_reports_a_bare_variant_scalar_at_the_path_its_author_wrote() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: variants
  version: "1.0"
  backend: typst
  description: variant-bearing enum
main:
  fields:
    classification:
      type: enum
      values: [cui, secret]
      default: ""
      variants:
        cui:
          caveat:
            type: string
"#,
    );
    // The floor normalizes the bare scalar into `{value: …}`; the diagnostic
    // names the field the author wrote, not the key the floor minted.
    let md = "~~~card-yaml\n$quill: variants\n$kind: main\nclassification: bogus\n~~~\n";
    let doc = Document::parse(md).unwrap().document;

    let diag = quill
        .validate(&doc)
        .into_iter()
        .find(|d| d.code.as_deref() == Some("validation::enum_violation"))
        .expect("expected an enum_violation");
    assert_eq!(diag.path.as_deref(), Some("main.classification"));
}
