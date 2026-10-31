use quillmark::Engine;

mod common;

fn make_quill(name: &str, backend: &str) -> quillmark::Quill {
    let yaml = format!(
        "quill:\n  name: \"{name}\"\n  version: \"1.0.0\"\n  backend: \"{backend}\"\n  description: \"Test\"\n\n{backend}:\n  plate_file: plate.typ\n"
    );
    common::quill_from_files(&[("Quill.yaml", &yaml), ("plate.typ", "#rect(width: 1cm)")])
}

#[test]
fn the_default_engine_registers_one_backend_per_enabled_feature() {
    let engine = Engine::new();
    let mut backends = engine.registered_backends();
    backends.sort_unstable();
    let mut expected = Vec::new();
    if cfg!(feature = "acroform") {
        expected.push("acroform");
    }
    if cfg!(feature = "typst") {
        expected.push("typst");
    }
    assert_eq!(backends, expected);
}

#[test]
fn test_unsupported_backend_errors_at_render_time() {
    // Loading tags the quill with its declared backend id without resolving it.
    let quill = make_quill("bad_backend_quill", "non_existent");
    assert_eq!(quill.backend_id(), "non_existent");

    let engine = Engine::new();
    let err = engine
        .supported_formats(&quill)
        .expect_err("unregistered backend must not resolve");
    assert_eq!(
        err.diagnostics()[0].code.as_deref(),
        Some("engine::backend_not_found")
    );
}
