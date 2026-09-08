use std::fs;
use tempfile::TempDir;

use quillmark::Quillmark;

fn make_quill_dir(temp_dir: &TempDir, name: &str, backend: &str) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join(name);
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        format!(
            "quill:\n  name: \"{}\"\n  version: \"1.0\"\n  backend: \"{}\"\n  description: \"Test\"\n\n{}:\n  plate_file: plate.typ\n",
            name, backend, backend
        ),
    )
    .unwrap();
    fs::write(quill_path.join("plate.typ"), "#rect(width: 1cm)").unwrap();
    quill_path
}

#[test]
fn the_default_engine_registers_one_backend_per_enabled_feature() {
    let engine = Quillmark::new();
    let mut backends = engine.registered_backends();
    backends.sort_unstable();
    let mut expected = Vec::new();
    if cfg!(feature = "pdfform") {
        expected.push("pdfform");
    }
    if cfg!(feature = "typst") {
        expected.push("typst");
    }
    assert_eq!(backends, expected);
}

#[test]
fn test_unsupported_backend_errors_at_render_time() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill_dir(&temp_dir, "bad_backend_quill", "non_existent");

    // Loading tags the quill with its declared backend id without resolving it.
    let quill =
        quillmark::quill_from_path(quill_path).expect("load succeeds; backend resolved later");
    assert_eq!(quill.backend_id(), "non_existent");

    let engine = Quillmark::new();
    let err = engine
        .supported_formats(&quill)
        .expect_err("unregistered backend must not resolve");
    assert_eq!(
        err.diagnostics()[0].code.as_deref(),
        Some("engine::backend_not_found")
    );
}

/// A path that names no directory is the caller's mistake, not a bundle missing
/// its `Quill.yaml`.
#[test]
fn a_missing_quill_path_names_the_path_not_the_missing_config() {
    let temp_dir = TempDir::new().unwrap();
    let err = quillmark::quill_from_path(temp_dir.path().join("no_such_quill"))
        .expect_err("a nonexistent path does not load");
    let text = err.to_string();
    assert!(
        text.contains("Quill directory not found") && text.contains("no_such_quill"),
        "the error should name the missing path, got: {text}"
    );
}
