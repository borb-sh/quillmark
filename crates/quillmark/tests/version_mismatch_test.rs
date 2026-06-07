//! # `$quill` Name/Version Compatibility Tests
//!
//! A document's `$quill: name@selector` reference is checked against the loaded
//! quill at render time. A *name* mismatch is advisory (the engine renders with
//! the quill it was handed and emits a `quill::name_mismatch` warning); a
//! *version* mismatch is a hard error (`quill::version_mismatch`), since
//! rendering against an incompatible format is a footgun.

use quillmark::{Document, Quillmark};
use quillmark_core::{OutputFormat, RenderError, RenderOptions};
use std::fs;
use tempfile::TempDir;

/// Write a minimal typst quill named `test_quill` at the given version.
fn make_quill(temp_dir: &TempDir, version: &str) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        format!(
            "quill:\n  name: \"test_quill\"\n  version: \"{}\"\n  backend: \"typst\"\n  plate_file: \"plate.typ\"\n  description: \"Test\"\n",
            version
        ),
    )
    .unwrap();
    fs::write(quill_path.join("plate.typ"), "Content").unwrap();
    quill_path
}

fn render_ref(
    quill_path: &std::path::Path,
    quill_ref: &str,
) -> Result<quillmark_core::RenderResult, RenderError> {
    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(quill_path)
        .expect("quill_from_path failed");
    let markdown = format!(
        "~~~card-yaml\n$quill: {}\n$kind: main\n~~~\n\n# Content\n",
        quill_ref
    );
    let doc = Document::from_markdown(&markdown).expect("parse failed");
    quill.render(
        &doc,
        &RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        },
    )
}

#[test]
fn version_out_of_selector_is_a_hard_error() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");

    // Document pins `@2`; loaded quill is 3.0.0 → incompatible → render fails.
    let err = render_ref(&quill_path, "test_quill@2").expect_err("render should fail");
    let codes: Vec<_> = err.diagnostics().iter().filter_map(|d| d.code.as_deref()).collect();
    assert!(
        codes.contains(&"quill::version_mismatch"),
        "expected version_mismatch error, got: {codes:?}"
    );
}

#[test]
fn version_out_of_selector_fails_dry_run() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");
    let engine = Quillmark::new();
    let quill = engine.quill_from_path(&quill_path).unwrap();
    let doc = Document::from_markdown(
        "~~~card-yaml\n$quill: test_quill@2\n$kind: main\n~~~\n\n# Content\n",
    )
    .unwrap();

    let err = quill.dry_run(&doc).expect_err("dry_run should fail");
    let codes: Vec<_> = err.diagnostics().iter().filter_map(|d| d.code.as_deref()).collect();
    assert!(codes.contains(&"quill::version_mismatch"), "got: {codes:?}");
}

#[test]
fn exact_selector_match_renders() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "2.1.0");

    let result = render_ref(&quill_path, "test_quill@2.1.0").expect("render should succeed");
    assert!(result.warnings.is_empty(), "got: {:?}", result.warnings);
    assert!(!result.artifacts.is_empty());
}

#[test]
fn minor_selector_matches_any_patch() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "2.1.5");

    // `@2.1` matches any patch in the 2.1 series.
    let result = render_ref(&quill_path, "test_quill@2.1").expect("render should succeed");
    assert!(result.warnings.is_empty(), "got: {:?}", result.warnings);
}

#[test]
fn latest_selector_matches_any_version() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "9.9.9");

    // Bare name defaults to `Latest`, which matches any version.
    let result = render_ref(&quill_path, "test_quill").expect("render should succeed");
    assert!(result.warnings.is_empty(), "got: {:?}", result.warnings);
}

#[test]
fn name_mismatch_warns_and_skips_the_version_check() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");

    // Name differs — the name warning fires and the (would-be incompatible)
    // version selector is moot, so render still succeeds.
    let result = render_ref(&quill_path, "other_quill@2").expect("render should succeed");
    assert_eq!(result.warnings.len(), 1, "expected exactly one warning");
    assert_eq!(result.warnings[0].code.as_deref(), Some("quill::name_mismatch"));
    assert!(!result.artifacts.is_empty());
}
