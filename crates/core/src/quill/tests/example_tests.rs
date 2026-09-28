use super::*;

const MANIFEST: &str = "quill:\n  name: memo\n  version: \"1.2.0\"\n  backend: typst\n  \
                        description: memo\nmain:\n  fields:\n    subject:\n      type: string\n";

fn load(markdown: &str) -> Result<Quill, Vec<Diagnostic>> {
    Quill::from_tree(tree(&[
        ("Quill.yaml", MANIFEST.as_bytes()),
        ("example.md", markdown.as_bytes()),
    ]))
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().filter_map(|d| d.code.as_deref()).collect()
}

#[test]
fn the_example_is_handed_out_pinned_to_its_quill_version() {
    let quill = load("~~~\n$quill: memo\nsubject: Budget review\n~~~\n\nFilled-in prose.\n")
        .expect("loads");

    let parsed = quill.example_document().expect("present").expect("parses");

    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.document.quill_reference().to_string(), "memo@1.2.0");
    assert_eq!(
        parsed.document.main().payload().get("subject").and_then(|v| v.as_str()),
        Some("Budget review")
    );
}

#[test]
fn only_the_root_example_md_is_the_example() {
    let example = "~~~\n$quill: memo\n~~~\n".as_bytes();
    for elsewhere in ["examples/example.md", "Example.md"] {
        let quill = Quill::from_tree(tree(&[
            ("Quill.yaml", MANIFEST.as_bytes()),
            (elsewhere, example),
        ]))
        .expect("loads");
        assert!(quill.example_document().is_none(), "{elsewhere}");
    }
}

#[test]
fn the_load_never_reads_the_example() {
    let broken =
        load("~~~\n$quill: memo\nsubject: [unclosed\n~~~\n").expect("content never refuses a load");
    let errors = broken.example_document().expect("present").expect_err("does not parse");
    assert_eq!(
        errors[0].location.as_ref().map(|l| l.file.as_str()),
        Some("example.md"),
        "{errors:?}"
    );
}

#[test]
fn the_example_names_its_quill_with_no_selector() {
    for reference in ["memo@1", "memo@1.2.0", "letter"] {
        let quill = load(&format!("~~~\n$quill: {reference}\n~~~\n")).expect("loads");
        let errors = quill
            .example_document()
            .expect("present")
            .expect_err(reference);
        assert_eq!(codes(&errors), ["quill::example_reference"], "{reference}");
    }
}
