//! `quill.example`: the load checks the file exists, and `example_document`
//! parses, pairs, pins, and conforms it.
use super::*;

const PATH: &str = "examples/filled.md";

fn manifest_with(example: &str) -> String {
    format!(
        "quill:\n  name: memo\n  version: \"1.2.0\"\n  backend: typst\n  description: memo\n  \
         example: {example}\nmain:\n  fields:\n    subject:\n      type: string\n"
    )
}

fn load(markdown: &str) -> Result<Quill, Vec<Diagnostic>> {
    Quill::from_tree(tree(&[
        ("Quill.yaml", manifest_with(PATH).as_bytes()),
        (PATH, markdown.as_bytes()),
    ]))
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().filter_map(|d| d.code.as_deref()).collect()
}

#[test]
fn the_example_is_handed_out_pinned_to_its_quill_version() {
    let quill = load("~~~\n$quill: memo\nsubject: Budget review\n~~~\n\nFilled-in prose.\n")
        .expect("loads");

    let parsed = quill.example_document().expect("declared").expect("parses");

    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.document.quill_reference().to_string(), "memo@1.2.0");
    assert_eq!(
        parsed.document.main().payload().get("subject").and_then(|v| v.as_str()),
        Some("Budget review")
    );
    assert!(quill_from(&[("Quill.yaml", &manifest("memo"))])
        .example_document()
        .is_none());
}

#[test]
fn the_load_checks_the_example_exists_and_never_reads_it() {
    let missing = Quill::from_tree(tree(&[("Quill.yaml", manifest_with(PATH).as_bytes())]))
        .expect_err("a named file that is absent refuses the load");
    assert_eq!(codes(&missing), ["quill::example_missing"]);

    let escaping = Quill::from_tree(tree(&[(
        "Quill.yaml",
        manifest_with("../example.md").as_bytes(),
    )]))
    .expect_err("a path out of the quill names no file in it");
    assert_eq!(codes(&escaping), ["quill::example_missing"]);

    let not_a_path = QuillConfig::from_yaml_with_warnings(&manifest_with("[a, b]"))
        .expect_err("a list is not a path");
    assert_eq!(codes(&not_a_path), ["quill::invalid_example"]);

    let broken =
        load("~~~\n$quill: memo\nsubject: [unclosed\n~~~\n").expect("content never refuses a load");
    let errors = broken.example_document().expect("declared").expect_err("does not parse");
    assert_eq!(
        errors[0].location.as_ref().map(|l| l.file.as_str()),
        Some(PATH),
        "{errors:?}"
    );
}

#[test]
fn the_example_names_its_quill_with_no_selector() {
    for reference in ["memo@1", "memo@1.2.0", "letter"] {
        let quill = load(&format!("~~~\n$quill: {reference}\n~~~\n")).expect("loads");
        let errors = quill
            .example_document()
            .expect("declared")
            .expect_err(reference);
        assert_eq!(codes(&errors), ["quill::example_reference"], "{reference}");
    }
}
