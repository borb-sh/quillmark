//! A document's `$quill: name@selector` is checked against the loaded quill at
//! render time and in `dry_run`. Rendering a valid document against the wrong
//! quill is a hard error, never a warning.

use quillmark::{Document, RenderError};
use std::fs;
use tempfile::TempDir;

mod common;

#[cfg(feature = "typst")]
use quillmark::{Engine, OutputFormat, RenderOptions};

fn make_quill(temp_dir: &TempDir, version: &str) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        format!(
            "quill:\n  name: \"test_quill\"\n  version: \"{}\"\n  backend: \"typst\"\n  description: \"Test\"\n\ntypst:\n  plate_file: plate.typ\n",
            version
        ),
    )
    .unwrap();
    fs::write(quill_path.join("plate.typ"), "Content").unwrap();
    quill_path
}

fn doc(quill_ref: &str) -> Document {
    Document::parse(&format!(
        "~~~card-yaml\n$quill: {}\n$kind: main\n~~~\n\n# Content\n",
        quill_ref
    ))
    .expect("parse failed")
    .document
}

fn code(err: &RenderError) -> Option<&str> {
    err.diagnostics().first().and_then(|d| d.code.as_deref())
}

#[test]
fn the_reference_is_checked_against_the_loaded_quill() {
    let temp_dir = TempDir::new().unwrap();
    let quill = quillmark::quill_from_path(make_quill(&temp_dir, "2.1.0")).expect("from_path failed");
    for (quill_ref, expected) in [
        ("test_quill@2.1.0", None),
        ("test_quill@2.1", None),
        ("test_quill@2.1.1", Some("quill::version_mismatch")),
        ("test_quill@2.2", Some("quill::version_mismatch")),
        ("borb.test_quill@2.1", None),
        ("borb.other_quill@2.1", Some("quill::name_mismatch")),
        // The version is left unevaluated: a selector against another quill is moot.
        ("other_quill@3.0", Some("quill::name_mismatch")),
    ] {
        let got = quill.dry_run(&doc(quill_ref));
        assert_eq!(got.as_ref().err().and_then(code), expected, "{quill_ref}: {got:?}");
    }
}

/// A document the quill creates pins its `MAJOR.MINOR`: the next patch reads
/// it, and the next minor refuses it.
#[test]
fn a_created_document_pins_its_quills_series() {
    let quill_at = |version: &str| {
        let temp_dir = TempDir::new().unwrap();
        quillmark::quill_from_path(make_quill(&temp_dir, version)).expect("from_path failed")
    };
    let created = quill_at("1.2.0");
    let authored = created
        .from_authoring(&serde_json::json!({}))
        .expect("imports")
        .document;
    for doc in [created.seed_document(), authored] {
        let patched = quill_at("1.2.1").dry_run(&doc);
        assert!(patched.is_ok(), "{patched:?}");
        let next_minor = quill_at("1.3.0").dry_run(&doc);
        assert_eq!(next_minor.as_ref().err().and_then(code), Some("quill::version_mismatch"));
    }
}

/// The check rides `update`, not just the open door: a session compiles every
/// edit through its own config, so the pairing stays checkable.
#[test]
#[cfg(feature = "typst")]
fn update_rechecks_the_reference_against_the_sessions_quill() {
    let temp_dir = TempDir::new().unwrap();
    let quill = quillmark::quill_from_path(make_quill(&temp_dir, "3.0.0")).expect("from_path failed");

    let engine = Engine::new();
    let err = engine
        .render(&quill, &doc("other_quill@3.0"), common::test_date(), &RenderOptions::default())
        .expect_err("render must refuse another quill's document");
    assert_eq!(code(&err), Some("quill::name_mismatch"));

    let mut session = engine
        .open(&quill, &doc("test_quill@3.0"), common::test_date())
        .expect("open against the matching quill");
    let pages = session.page_count();

    let err = session
        .update(&doc("other_quill@3.0"))
        .expect_err("update must refuse another quill's document");
    assert_eq!(code(&err), Some("quill::name_mismatch"));

    let err = session
        .update(&doc("test_quill@3.1"))
        .expect_err("update must refuse an out-of-selector version");
    assert_eq!(code(&err), Some("quill::version_mismatch"));

    // A refusal is raised before the backend is touched, so reads still serve
    // the compile `open` produced.
    assert_eq!(session.page_count(), pages);
    session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("reads still serve the last-good compile");
    session
        .update(&doc("test_quill@3.0"))
        .expect("the matching document still applies");
}
