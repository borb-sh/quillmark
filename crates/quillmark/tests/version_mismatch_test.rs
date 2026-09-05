//! A document's `$quill: name@selector` is checked against the loaded quill at
//! render time and in `dry_run`. Rendering a valid document against the wrong
//! quill is a hard error, never a warning.

use quillmark::{Document, RenderError};
use std::fs;
use tempfile::TempDir;

// `engine.render` resolves the declared `typst` backend before the reference
// check runs, so without the feature the reject paths would fail on
// `engine::backend_not_found` instead. `dry_run` needs no backend.
#[cfg(feature = "typst")]
use quillmark::Quillmark;
#[cfg(feature = "typst")]
use quillmark::{OutputFormat, RenderOptions, RenderResult};

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

#[cfg(feature = "typst")]
fn render_ref(
    quill_path: &std::path::Path,
    quill_ref: &str,
) -> Result<RenderResult, RenderError> {
    let engine = Quillmark::new();
    let quill = quillmark::quill_from_path(quill_path).expect("from_path failed");
    let markdown = format!(
        "~~~card-yaml\n$quill: {}\n$kind: main\n~~~\n\n# Content\n",
        quill_ref
    );
    let doc = Document::parse(&markdown).expect("parse failed").document;
    engine.render(
        &quill,
        &doc,
        &RenderOptions::default().with_output_format(OutputFormat::Pdf),
    )
}

/// Selector acceptance without driving a Typst compile.
fn dry_run_ref(quill_path: &std::path::Path, quill_ref: &str) -> Result<(), RenderError> {
    let quill = quillmark::quill_from_path(quill_path).expect("from_path failed");
    let markdown = format!(
        "~~~card-yaml\n$quill: {}\n$kind: main\n~~~\n\n# Content\n",
        quill_ref
    );
    let doc = Document::parse(&markdown).expect("parse failed").document;
    quill.dry_run(&doc)
}

/// The check emits exactly one diagnostic.
#[cfg(feature = "typst")]
fn mismatch_code(err: &RenderError) -> Option<&str> {
    err.diagnostics().first().and_then(|d| d.code.as_deref())
}

#[test]
#[cfg(feature = "typst")]
fn version_out_of_selector_is_a_hard_error() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");

    let err = render_ref(&quill_path, "test_quill@2").expect_err("render should fail");
    assert_eq!(mismatch_code(&err), Some("quill::version_mismatch"));
}

#[test]
#[cfg(feature = "typst")]
fn name_mismatch_is_a_hard_error() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");

    // The version is left unevaluated: a selector against another quill is moot.
    let err = render_ref(&quill_path, "other_quill@2").expect_err("render should fail");
    assert_eq!(mismatch_code(&err), Some("quill::name_mismatch"));
}

/// The check rides `update`, not just the open door: a session compiles every
/// edit through its own config, so the pairing stays checkable.
#[test]
#[cfg(feature = "typst")]
fn update_rechecks_the_reference_against_the_sessions_quill() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");
    let quill = quillmark::quill_from_path(&quill_path).expect("from_path failed");

    let doc = |quill_ref: &str| {
        Document::parse(&format!(
            "~~~card-yaml\n$quill: {}\n$kind: main\n~~~\n\n# Content\n",
            quill_ref
        ))
        .expect("parse failed")
        .document
    };

    let engine = Quillmark::new();
    let mut session = engine
        .open(&quill, &doc("test_quill@3"))
        .expect("open against the matching quill");
    let pages = session.page_count();

    let err = session
        .update(&doc("other_quill@3"))
        .expect_err("update must refuse another quill's document");
    assert_eq!(mismatch_code(&err), Some("quill::name_mismatch"));

    let err = session
        .update(&doc("test_quill@2"))
        .expect_err("update must refuse an out-of-selector version");
    assert_eq!(mismatch_code(&err), Some("quill::version_mismatch"));

    // A refusal is raised before the backend is touched, so reads still serve
    // the compile `open` produced.
    assert_eq!(session.page_count(), pages);
    session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("reads still serve the last-good compile");
    session
        .update(&doc("test_quill@3"))
        .expect("the matching document still applies");
}

#[test]
fn exact_selector_match_accepts() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "2.1.0");

    dry_run_ref(&quill_path, "test_quill@2.1.0").expect("selector should be accepted");
}
