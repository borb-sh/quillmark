
use crate::document::Document;

fn assert_round_trip(label: &str, src: &str) {
    let a = Document::parse(src)
        .unwrap_or_else(|e| panic!("{}: parse failed on original: {}", label, e))
        .document;
    let emitted = a.to_markdown();
    let b = Document::parse(&emitted)
        .unwrap_or_else(|e| {
            panic!(
                "{}: parse failed on emitted document.\nError: {}\nEmitted:\n{}",
                label, e, emitted
            )
        })
        .document;
    assert_eq!(
        a, b,
        "{}: round-trip produced different Documents.\nEmitted:\n{}",
        label, emitted
    );
}

#[test]
fn fixtures_round_trip() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    let resources_dir = std::path::Path::new(manifest_dir)
        .join("..") // crates/core → crates
        .join("fixtures")
        .join("resources");

    let mut fixture_paths: Vec<std::path::PathBuf> = Vec::new();

    for entry in std::fs::read_dir(&resources_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            fixture_paths.push(path);
        }
    }

    assert!(
        !fixture_paths.is_empty(),
        "no fixture files found: check paths"
    );

    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for path in &fixture_paths {
        let label = path.to_string_lossy();
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP {}: cannot read: {}", label, e);
                skipped += 1;
                continue;
            }
        };

        match Document::parse(&src).map(|p| p.document) {
            Err(_) => {
                skipped += 1;
                continue;
            }
            Ok(a) => {
                let emitted = a.to_markdown();
                match Document::parse(&emitted).map(|p| p.document) {
                    Err(e) => {
                        failed += 1;
                        failures.push(format!(
                            "FAIL {}: re-parse failed: {}\nEmitted:\n{}",
                            label, e, emitted
                        ));
                    }
                    Ok(b) => {
                        if a == b {
                            passed += 1;
                        } else {
                            failed += 1;
                            failures.push(format!(
                                "FAIL {}: documents differ after round-trip.\nEmitted:\n{}",
                                label, emitted
                            ));
                        }
                    }
                }
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "Fixture round-trip failures ({} failed, {} passed, {} skipped):\n{}",
            failed,
            passed,
            skipped,
            failures.join("\n\n")
        );
    }

    assert!(
        passed > 0,
        "No fixtures passed round-trip: did all files get skipped?"
    );

    eprintln!(
        "fixtures_round_trip: {} passed, {} skipped",
        passed, skipped
    );
}

#[test]
fn round_trip_value_types() {
    let cases: &[(&str, &str)] = &[
        (
            "booleans",
            "~~~card-yaml\n$quill: q\n$kind: main\nflag_true: true\nflag_false: false\n~~~\n",
        ),
        (
            "null",
            "~~~card-yaml\n$quill: q\n$kind: main\nnull_field: null\n~~~\n",
        ),
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
            "\
~~~card-yaml
$quill: q
$kind: main
title: Test
~~~

Body text.

~~~card-yaml
$kind: section
heading: Chapter 1
~~~

Card body here.
",
        ),
        (
            "card with empty body",
            "\
~~~card-yaml
$quill: q
$kind: main
title: Test
~~~

~~~card-yaml
$kind: empty_body_card
title: No body
~~~
",
        ),
        (
            "string with backslash",
            "~~~card-yaml\n$quill: q\n$kind: main\npath: \"C:\\\\Users\\\\test\"\n~~~\n",
        ),
        (
            "multiline string",
            "~~~card-yaml\n$quill: q\n$kind: main\nbio: \"Line one\\nLine two\"\n~~~\n",
        ),
    ];

    for (label, src) in cases {
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

#[test]
fn empty_map_emits_inline_braces() {
    use crate::value::QuillValue;
    use indexmap::IndexMap;

    let mut payload: IndexMap<String, QuillValue> = IndexMap::new();
    payload.insert(
        "empty_obj".to_string(),
        QuillValue::from_json(serde_json::json!({})),
    );
    payload.insert(
        "real_field".to_string(),
        QuillValue::from_json(serde_json::json!("hello")),
    );

    use crate::document::{Card, Payload};
    let mut p = Payload::from_index_map(payload);
    p.set_quill("test".parse().unwrap());
    p.set_kind("main");
    let main = Card::from_parts(p, quillmark_content::Normalized::empty());
    let doc = crate::document::Document::from_main_and_cards(main, vec![]);

    let md = doc.to_markdown();
    assert!(
        md.contains("empty_obj: {}\n"),
        "empty object should emit as inline braces, got:\n{}",
        md
    );
    assert!(
        md.contains("real_field: hello"),
        "real field should appear in emit, got:\n{}",
        md
    );
    assert_eq!(
        doc,
        crate::document::Document::parse(&md).unwrap().document,
        "empty object must survive the round-trip, got:\n{}",
        md
    );
}

#[test]
fn nested_map_keys_with_structural_chars_emit_valid_yaml() {
    use crate::document::{Card, Payload};
    use crate::value::QuillValue;
    use indexmap::IndexMap;

    let mut payload: IndexMap<String, QuillValue> = IndexMap::new();
    payload.insert(
        "config".to_string(),
        QuillValue::from_json(serde_json::json!({
            "a: b": 1,
            "*star": 2,
            "n": 3,
            "needs # comment": 4
        })),
    );
    let mut p = Payload::from_index_map(payload);
    p.set_quill("test".parse().unwrap());
    p.set_kind("main");
    let main = Card::from_parts(p, quillmark_content::Normalized::empty());
    let doc = Document::from_main_and_cards(main, vec![]);

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

/// A comment's position is its index among its mapping's children, and a key the
/// emitter quotes or spaces is one of those children.
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
    let reparsed = Document::parse(&md).expect("the emitted document re-parses").document;
    assert_eq!(doc, reparsed, "emit is not a fixed point: {md}");

    // And the nested fill under a quoted key resolves against the parsed value.
    let filled = "\
~~~card-yaml
$quill: test@1.0
$kind: main
config:
  \"a b\": !must_fill
  city: Anytown
~~~

Body.
";
    let doc = Document::parse(filled).expect("parses").document;
    let md = doc.to_markdown();
    assert!(md.contains("a b: !must_fill\n"), "the marker moved: {md}");
    assert_eq!(
        doc,
        Document::parse(&md).expect("re-parses").document,
        "emit is not a fixed point: {md}"
    );
}

/// Emit projects a canonical content object through the markdown exporter, so an
/// indented plaintext sample survives the re-parse only if the projection escapes
/// what markdown strips at a line's edges.
#[test]
fn an_indented_plaintext_field_survives_emit_and_reparse() {
    use crate::document::{Card, Payload};
    use crate::value::QuillValue;
    use indexmap::IndexMap;

    let text = "    indented\nplain\ntrailing   ";
    let content = quillmark_content::from_plaintext(text);

    let mut payload: IndexMap<String, QuillValue> = IndexMap::new();
    payload.insert(
        "sample".to_string(),
        QuillValue::from_json(quillmark_content::serial::to_canonical_value(&content)),
    );
    let mut p = Payload::from_index_map(payload);
    p.set_quill("test".parse().unwrap());
    p.set_kind("main");
    let main = Card::from_parts(p, quillmark_content::Normalized::empty());

    let md = Document::from_main_and_cards(main, vec![]).to_markdown();
    let back = Document::parse(&md).expect("re-parses").document;
    let projected = back
        .main()
        .payload()
        .get("sample")
        .and_then(|v| v.as_str())
        .expect("the field projected to a markdown string");
    assert_eq!(
        quillmark_content::from_markdown(projected)
            .expect("the projection re-imports")
            .text,
        text,
        "indented plaintext lost in emit:\n{md}"
    );
}

/// `store_field` keeps what it is handed, so both canonical forms rest here and
/// the projection guard is byte identity against either.
#[test]
fn a_seam_form_field_projects_to_markdown_like_a_stored_one() {
    use crate::document::{Card, Payload};
    use crate::value::QuillValue;
    use indexmap::IndexMap;

    let content = quillmark_content::from_markdown("> quoted").unwrap();
    let storage = quillmark_content::serial::to_canonical_value(&content);
    let seam = quillmark_content::serial::to_seam_value(&content);
    assert_ne!(storage, seam, "the forms must differ for this to test anything");

    let mut payload: IndexMap<String, QuillValue> = IndexMap::new();
    payload.insert("stored".to_string(), QuillValue::from_json(storage));
    payload.insert("read_back".to_string(), QuillValue::from_json(seam));
    let mut p = Payload::from_index_map(payload);
    p.set_quill("test".parse().unwrap());
    p.set_kind("main");
    let main = Card::from_parts(p, quillmark_content::Normalized::empty());

    let md = Document::from_main_and_cards(main, vec![]).to_markdown();
    assert!(md.contains(r#"stored: "> quoted""#), "got:\n{md}");
    assert!(md.contains(r#"read_back: "> quoted""#), "got:\n{md}");
}
