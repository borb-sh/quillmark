//! Emission: the fixed points the codecs claim, over the fixture corpus and
//! over the shapes no fixture carries.

use crate::document::Document;
use crate::document::tests::{assert_round_trip, collect_md_files, fixtures_root, parse};

/// Every fixture document, through the three fixed points at once. One walk,
/// because a document that breaks one usually breaks all three, and three walks
/// reported it three times with three copies of the bookkeeping.
#[test]
fn every_fixture_document_holds_the_codecs_fixed_points() {
    let mut paths = Vec::new();
    collect_md_files(&fixtures_root(), &mut paths);
    assert!(!paths.is_empty(), "no fixture documents found");

    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for path in &paths {
        let label = path.display().to_string();
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        // A bundled README carries no root card-yaml block, so it is not a
        // document and has nothing to hold.
        let Ok(a) = Document::parse(&src).map(|p| p.document) else {
            continue;
        };

        let emitted = a.to_markdown();
        let b = match Document::parse(&emitted) {
            Ok(p) => p.document,
            Err(e) => {
                failures.push(format!("{label}: the emission does not parse: {e}\n{emitted}"));
                continue;
            }
        };
        if a != b {
            failures.push(format!("{label}: emit∘parse is not the identity\n{emitted}"));
        }
        if b.to_markdown() != emitted {
            failures.push(format!("{label}: a second emission differs\n{emitted}"));
        }

        let json = serde_json::to_string(&a).expect("a document serializes");
        let restored: Document = serde_json::from_str(&json).expect("and deserializes");
        if restored.to_markdown() != emitted {
            failures.push(format!(
                "{label}: the storage DTO round trip emits different markdown\n{emitted}"
            ));
        }
        checked += 1;
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert!(checked > 0, "every fixture was skipped");
}

/// The value shapes, which `emit`'s own scalar round-trips do not reach: a
/// container, a card, and the empty spellings of each. Scalar fidelity is
/// `emit`'s unit tests plus
/// `lossiness_tests::quoting_normalises_to_canonical_form_with_type_fidelity`.
#[test]
fn round_trip_value_shapes() {
    for (label, src) in [
        (
            "nested map",
            "~~~card-yaml\n$quill: q\n$kind: main\nsender:\n  name: Alice\n  city: Springfield\n~~~\n",
        ),
        (
            "sequence",
            "~~~card-yaml\n$quill: q\n$kind: main\ntags:\n  - demo\n  - test\n~~~\n",
        ),
        (
            "empty sequence",
            "~~~card-yaml\n$quill: q\n$kind: main\nempty: []\n~~~\n",
        ),
        (
            "cards",
            "~~~card-yaml\n$quill: q\n$kind: main\ntitle: Test\n~~~\n\nBody text.\n\n\
~~~card-yaml\n$kind: section\nheading: Chapter 1\n~~~\n\nCard body here.\n",
        ),
        (
            "card with empty body",
            "~~~card-yaml\n$quill: q\n$kind: main\ntitle: Test\n~~~\n\n\
~~~card-yaml\n$kind: empty_body_card\ntitle: No body\n~~~\n",
        ),
    ] {
        assert_round_trip(label, src);
    }
}

#[test]
fn round_trip_quill_version_selectors() {
    for qref in &["q", "q@1", "q@1.2", "q@1.2.3", "q@latest"] {
        let src = format!(
            "~~~card-yaml\n$quill: {}\n$kind: main\ntitle: t\n~~~\n",
            qref
        );
        assert_round_trip(&format!("quill ref {}", qref), &src);
    }
}

/// A document whose root carries `fields` in order, stored as given.
fn doc_with(fields: &[(&str, serde_json::Value)]) -> Document {
    let mut doc = Document::new("test".parse().unwrap());
    for (key, value) in fields {
        doc.main_mut()
            .store_field(key, crate::value::QuillValue::from_json(value.clone()))
            .unwrap();
    }
    doc
}

#[test]
fn nested_map_keys_with_structural_chars_emit_valid_yaml() {
    let doc = doc_with(&[(
        "config",
        serde_json::json!({ "a: b": 1, "*star": 2, "n": 3, "needs # comment": 4 }),
    )]);

    let md = doc.to_markdown();
    let reparsed = Document::parse(&md)
        .unwrap_or_else(|e| panic!("emitted YAML must re-parse, got error {e}\n{md}"))
        .document;
    let cfg = reparsed.main().payload().get("config").unwrap().as_json();
    assert_eq!(cfg["a: b"], serde_json::json!(1));
    assert_eq!(cfg["*star"], serde_json::json!(2));
    assert_eq!(cfg["n"], serde_json::json!(3));
    assert_eq!(cfg["needs # comment"], serde_json::json!(4));
}

/// A comment's position is its index among its mapping's children, and a key of
/// any spelling (quoted, spaced, with an inner `:` or a leading `-`, a tab after
/// its `:`) is one of those children.
#[test]
fn a_comment_after_a_quoted_nested_key_holds_its_position() {
    let src = "\
~~~card-yaml
$quill: test@1.0
$kind: main
config:
  \"a b\": 1
  # a note
  city: Anytown
  '- dash': 2
  # a second note
  spaced key : 3
  # a third note
  zip: 12345
  og:meta:
    og:title: Home
    # a fourth note
    og:type: site
  -x:\t4
  # a fifth note
  end: 5
~~~

Body.
";
    let doc = Document::parse(src).expect("parses").document;
    let md = doc.to_markdown();
    // The emitter re-spells each key canonically; the comments keep their slots.
    assert!(
        md.contains("a b: 1\n  # a note\n  city: Anytown\n"),
        "the first comment moved: {md}"
    );
    assert!(
        md.contains("\"- dash\": 2\n  # a second note\n  spaced key: 3\n"),
        "the second comment moved: {md}"
    );
    assert!(
        md.contains("spaced key: 3\n  # a third note\n  zip: 12345\n"),
        "the third comment moved: {md}"
    );
    assert!(
        md.contains("  og:meta:\n    og:title: Home\n    # a fourth note\n    og:type: site\n"),
        "the fourth comment moved: {md}"
    );
    assert!(
        md.contains("  -x: 4\n  # a fifth note\n  end: 5\n"),
        "the fifth comment moved: {md}"
    );
    let reparsed = Document::parse(&md).expect("the emitted document re-parses").document;
    assert_eq!(doc, reparsed, "emit is not a fixed point: {md}");

    // And a retired `!must_fill` under a quoted key nulls the node it tags.
    let filled = "\
~~~card-yaml
$quill: test@1.0
$kind: main
config:
  \"a b\": !must_fill Example
  city: Anytown
~~~

Body.
";
    let doc = Document::parse(filled).expect("parses").document;
    assert_eq!(
        doc.main().payload().get("config").unwrap().as_json(),
        &serde_json::json!({"a b": null, "city": "Anytown"})
    );
}

/// Emit projects a canonical content object through the markdown exporter, so an
/// indented plaintext sample survives the re-parse only if the projection escapes
/// what markdown strips at a line's edges.
#[test]
fn an_indented_plaintext_field_survives_emit_and_reparse() {
    let text = "    indented\nplain\ntrailing   ";
    let content = quillmark_content::import::from_plaintext(text);
    let md = doc_with(&[(
        "sample",
        quillmark_content::serial::to_canonical_value(&content),
    )])
    .to_markdown();
    let back = Document::parse(&md).expect("re-parses").document;
    let projected = back
        .main()
        .payload()
        .get("sample")
        .and_then(|v| v.as_str())
        .expect("the field projected to a markdown string");
    assert_eq!(
        quillmark_content::import::from_markdown(projected)
            .expect("the projection re-imports")
            .text,
        text,
        "indented plaintext lost in emit:\n{md}"
    );
}

/// The projection guard is byte identity against the canonical form, so a
/// content object spelling a zero `instance` stays structural: it decodes to the
/// same value and re-encodes to different bytes.
#[test]
fn only_the_canonical_spelling_of_a_content_field_projects_to_markdown() {
    let content = quillmark_content::import::from_markdown("> quoted").unwrap();
    let canonical = quillmark_content::serial::to_canonical_value(&content);
    let mut spelled = canonical.clone();
    spelled["lines"][0]["containers"][0]["instance"] = serde_json::json!(0);

    let md = doc_with(&[("stored", canonical), ("spelled", spelled)]).to_markdown();
    assert!(md.contains(r#"stored: "> quoted""#), "got:\n{md}");
    let back = Document::parse(&md).expect("re-parses").document;
    assert_eq!(back.main().payload().get("stored").unwrap().as_str(), Some("> quoted"));

    let spelled = back.main().payload().get("spelled").expect("field survives");
    assert_eq!(
        spelled.as_json()["lines"][0]["containers"][0]["instance"],
        serde_json::json!(0),
        "got:\n{md}"
    );
}

/// A nested content cell emits its projected scalar. `$ext` is no field value:
/// nothing converts a projection there back, so its content stays structural.
#[test]
fn a_nested_content_cell_projects_and_ext_content_does_not() {
    use crate::value::QuillValue;

    let content = quillmark_content::serial::to_canonical_value(
        &quillmark_content::import::from_markdown("and **this**").unwrap(),
    );
    let meta = QuillValue::from_json(serde_json::json!({ "blurb": content.clone() }));

    let mut doc = Document::new("q@1.0.0".parse().expect("reference"));
    doc.main_mut().store_field("meta", meta).expect("stores");
    let mut ext = serde_json::Map::new();
    ext.insert("host".to_string(), content.clone());
    doc.main_mut().store_ext(ext).expect("ext stores");

    let md = doc.to_markdown();
    assert!(md.contains("  blurb: and **this**\n"), "{md}");
    let back = Document::parse(&md).expect("re-parses").document;
    assert_eq!(back.main().ext().unwrap()["host"], content, "{md}");
}

#[test]
fn synthesised_kind_leaves_the_quill_trailer_on_quill() {
    let src = "~~~card-yaml\n$quill: q@1.0 # note on quill\ntitle: x\n~~~\n";
    let doc = parse(src);

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("$quill: q@1.0 # note on quill\n$kind: main\n"),
        "trailer belongs to $quill, not to the synthesised $kind\nGot:\n{}",
        emitted
    );
    assert_eq!(
        parse(&emitted),
        doc,
        "emit must re-parse to the same document"
    );
}

#[test]
fn store_ext_leaves_the_kind_trailer_on_kind() {
    let src = "~~~card-yaml\n$quill: q@1.0\n$kind: main # note on kind\ntitle: x\n~~~\n";
    let mut doc = parse(src);

    let mut ext = serde_json::Map::new();
    ext.insert("editor".into(), serde_json::json!({ "pinned": true }));
    doc.main_mut().store_ext(ext).expect("shallow map stores");

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("$kind: main # note on kind\n$ext:\n"),
        "trailer belongs to $kind, not to the new $ext\nGot:\n{}",
        emitted
    );
    assert_eq!(
        parse(&emitted),
        doc,
        "emit must re-parse to the same document"
    );
}

#[test]
fn a_comment_before_a_sequence_item_first_key_stays_inside_the_item() {
    let src = "\
~~~
$quill: test@1.0
$kind: main
items:
  -
    # c
    name: a
~~~

Body.
";
    let doc = Document::parse(src).expect("parses").document;
    let md = doc.to_markdown();
    assert_eq!(md, src, "the first emit is not the fixed point");

    let reparsed = Document::parse(&md)
        .expect("the emitted document re-parses")
        .document;
    assert_eq!(doc, reparsed, "emit is not a fixed point: {md}");
}
