//! `example.md` at the quill root: `example_document` parses, pairs, pins, and
//! conforms it, and the load never reads it.
use super::*;

const MANIFEST: &str = "quill:\n  name: memo\n  version: \"1.2.0\"\n  backend: typst\n  \
                        description: memo\nmain:\n  fields:\n    subject:\n      type: string\n";

fn load(markdown: &str) -> Quill {
    quill_from(&[
        ("Quill.yaml", MANIFEST.as_bytes()),
        (EXAMPLE_FILE, markdown.as_bytes()),
    ])
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().filter_map(|d| d.code.as_deref()).collect()
}

#[test]
fn the_example_is_handed_out_pinned_to_its_quill_version() {
    let quill = load("~~~\n$quill: memo\nsubject: Budget review\n~~~\n\nFilled-in prose.\n");

    let parsed = quill.example_document().expect("present").expect("parses");

    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.document.quill_reference().to_string(), "memo@1.2.0");
    assert_eq!(
        parsed.document.main().payload().get("subject").and_then(|v| v.as_str()),
        Some("Budget review")
    );
}

/// Only the root `example.md` is the example, and the load reads none of it.
#[test]
fn the_example_is_the_root_file_by_name_and_never_refuses_a_load() {
    let body = b"~~~\n$quill: memo\n~~~\n";
    for entries in [
        &[("Quill.yaml", MANIFEST.as_bytes())][..],
        &[("Quill.yaml", MANIFEST.as_bytes()), ("examples/example.md", body)],
        &[("Quill.yaml", MANIFEST.as_bytes()), ("Example.md", body)],
    ] {
        assert!(quill_from(entries).example_document().is_none());
    }

    let refused = QuillConfig::from_yaml_with_warnings(
        &MANIFEST.replace("description: memo\n", "description: memo\n  example: example.md\n"),
    )
    .expect_err("`quill.example` is no key");
    assert_eq!(codes(&refused), ["quill::unknown_key"]);

    let broken = load("~~~\n$quill: memo\nsubject: [unclosed\n~~~\n");
    let errors = broken.example_document().expect("present").expect_err("does not parse");
    assert_eq!(
        errors[0].location.as_ref().map(|l| l.file.as_str()),
        Some(EXAMPLE_FILE),
        "{errors:?}"
    );
}

#[test]
fn the_example_names_its_quill_with_no_selector() {
    for reference in ["memo@1", "memo@1.2.0", "letter"] {
        let errors = load(&format!("~~~\n$quill: {reference}\n~~~\n"))
            .example_document()
            .expect("present")
            .expect_err(reference);
        assert_eq!(codes(&errors), ["quill::example_reference"], "{reference}");
    }
}
