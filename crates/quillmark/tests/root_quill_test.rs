//! A derived quill in a quiver's source layout: it loads over the root beside
//! it, packs with the root's `Quill.yaml` inside, and reads a document written
//! for the root. Each plate asserts, so a render that succeeds is the plate
//! having seen what the test states.

#![cfg(feature = "typst")]

use std::path::Path;
use std::str::FromStr;

use quillmark::{
    DeltaClass, Document, Engine, OutputFormat, Quill, QuillReference, ROOT_COPY_PATH,
    RenderOptions, Severity,
};

mod common;

const ROOT_YAML: &str = r#"quill:
  name: resume
  version: "1.2.0"
  backend: typst
  description: A resume

typst:
  plate_file: plate.typ

schema:
  main:
    fields:
      name: { type: text }
      summary: { type: richtext, shape: lines, default: A person }
  job:
    fields:
      employer: { type: text }
"#;

const ROOT_PLATE: &str = r#"#import "@local/quillmark-helper:0.2.0" as qm
#assert.eq(qm.str(qm.data.name), "Ada")
#assert.eq(qm.data.at("$cards").map(card => qm.str(card.employer)), ("Analytical Engines",))
ok
"#;

const DERIVED_YAML: &str = r#"quill:
  name: resume_modern
  version: "0.1.0"
  backend: typst
  description: A resume with an accent
  root: resume@1.2.0

typst:
  plate_file: plate.typ

schema:
  main:
    fields:
      summary: { default: A modern person }
      accent: { type: text, default: teal }
  job:
    fields:
      logo: { type: text }
"#;

const DERIVED_PLATE: &str = r#"#import "@local/quillmark-helper:0.2.0" as qm
#assert.eq(qm.str(qm.data.name), "Ada")
#assert.eq(qm.str(qm.data.accent), "teal")
#assert.eq(qm.data.at("$cards").map(card => (qm.str(card.employer), qm.str(card.logo))), (("Analytical Engines", ""),))
ok
"#;

const DOCUMENT: &str = "~~~card-yaml\n$quill: resume@1.2\n$kind: main\nname: Ada\n~~~\n\n\
                        ~~~card-yaml\n$kind: job\nemployer: Analytical Engines\n~~~\n";

fn write(dir: &Path, yaml: &str, plate: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("Quill.yaml"), yaml).unwrap();
    std::fs::write(dir.join("plate.typ"), plate).unwrap();
}

fn render(quill: &Quill, document: &Document) {
    Engine::new()
        .open(quill, document, common::test_date())
        .and_then(|session| {
            session.render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        })
        .unwrap_or_else(|e| panic!("{} renders: {e:?}", quill.name()));
}

fn errors(quill: &Quill, document: &Document) -> Vec<String> {
    quill
        .validate(document)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

#[test]
fn a_document_moves_from_a_root_to_a_quill_derived_from_it() {
    let quiver = tempfile::tempdir().unwrap();
    let quills = quiver.path().join("quills");
    write(&quills.join("resume/1.2.0"), ROOT_YAML, ROOT_PLATE);
    write(
        &quills.join("resume_modern/0.1.0"),
        DERIVED_YAML,
        DERIVED_PLATE,
    );

    let root = quillmark::quill_from_path(quills.join("resume/1.2.0")).unwrap();
    let source = quillmark::quill_from_path(quills.join("resume_modern/0.1.0")).unwrap();
    assert_eq!(
        source.files().get_file(ROOT_COPY_PATH),
        Some(ROOT_YAML.as_bytes())
    );

    let derived = Quill::from_bundle(&source.to_bundle().unwrap()).unwrap();
    assert_eq!(derived.config(), source.config());
    let named = derived
        .config()
        .root
        .as_ref()
        .expect("the quill names its root");
    assert_eq!(named.fingerprint, root.config().contract_fingerprint());

    let mut document = Document::parse(DOCUMENT).unwrap().document;
    render(&root, &document);

    let delta = derived.config().delta(root.config());
    assert!(delta.is_lossless());
    assert_eq!(
        delta
            .changes()
            .map(|e| (e.kind.as_str(), e.path.as_str(), e.class))
            .collect::<Vec<_>>(),
        [
            ("main", "accent", DeltaClass::New),
            ("job", "logo", DeltaClass::New)
        ]
    );

    let written = document.to_markdown();
    document.set_quill_ref(QuillReference::from_str("resume_modern@0.1").unwrap());
    derived.conform(&mut document).unwrap();
    assert_eq!(errors(&derived, &document), Vec::<String>::new());
    render(&derived, &document);

    document.set_quill_ref(QuillReference::from_str("resume@1.2").unwrap());
    root.conform(&mut document).unwrap();
    assert_eq!(document.to_markdown(), written);
}
