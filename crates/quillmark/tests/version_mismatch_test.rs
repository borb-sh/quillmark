//! # Version-Selector Mismatch Warning Tests
//!
//! A document's `$quill: name@selector` reference is informational — the engine
//! always renders with the quill it was handed. When the loaded quill's version
//! does not satisfy the selector, render emits a non-fatal
//! `quill::version_mismatch` warning and still produces an artifact.

use quillmark::{Document, Quillmark};
use quillmark_core::{OutputFormat, RenderOptions};
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

fn render_with_ref(quill_path: &std::path::Path, quill_ref: &str) -> quillmark_core::RenderResult {
    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(quill_path)
        .expect("quill_from_path failed");
    let markdown = format!(
        "~~~card-yaml\n$quill: {}\n$kind: main\n~~~\n\n# Content\n",
        quill_ref
    );
    let doc = Document::from_markdown(&markdown).expect("parse failed");
    quill
        .render(
            &doc,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            },
        )
        .expect("render should succeed despite mismatch")
}

#[test]
fn major_selector_mismatch_warns_but_renders() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");

    // Document pins `@2`; loaded quill is 3.0.0 → mismatch.
    let result = render_with_ref(&quill_path, "test_quill@2");

    assert_eq!(result.warnings.len(), 1, "expected exactly one warning");
    assert_eq!(
        result.warnings[0].code.as_deref(),
        Some("quill::version_mismatch")
    );
    assert!(!result.artifacts.is_empty(), "artifact must be produced");
}

#[test]
fn exact_selector_match_is_silent() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "2.1.0");

    let result = render_with_ref(&quill_path, "test_quill@2.1.0");

    assert!(
        result.warnings.is_empty(),
        "matching selector should not warn: {:?}",
        result.warnings
    );
}

#[test]
fn minor_selector_matches_any_patch() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "2.1.5");

    // `@2.1` matches any patch in the 2.1 series.
    let result = render_with_ref(&quill_path, "test_quill@2.1");

    assert!(
        result.warnings.is_empty(),
        "minor selector should match any patch: {:?}",
        result.warnings
    );
}

#[test]
fn latest_selector_never_warns() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "9.9.9");

    // Bare name defaults to `Latest`, which matches any version.
    let result = render_with_ref(&quill_path, "test_quill");

    assert!(
        result.warnings.is_empty(),
        "latest selector should never warn: {:?}",
        result.warnings
    );
}

#[test]
fn name_mismatch_takes_precedence_over_version() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill(&temp_dir, "3.0.0");

    // Name differs — the name warning fires; the version selector is moot.
    let result = render_with_ref(&quill_path, "other_quill@2");

    assert_eq!(result.warnings.len(), 1, "expected exactly one warning");
    assert_eq!(
        result.warnings[0].code.as_deref(),
        Some("quill::ref_mismatch")
    );
}
