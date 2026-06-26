//! Acceptance test for issue #745: Typst errors from `eval`'d runtime strings.
//!
//! When a plate runs `eval(..., mode: "markup")` on a runtime string and the
//! Typst compiler errors inside it, the diagnostic span points at an ephemeral
//! source that was never registered in the world, so it can't resolve to a
//! file/line. Such a diagnostic must still carry an actionable `hint` pointing
//! the caller at dynamically-evaluated field content, rather than surfacing the
//! bare Typst message with no anchor.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderOptions};
use quillmark_typst::TypstBackend;

fn host_source() -> Quill {
    fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let p: PathBuf = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("resources")
        .join("quills")
        .join("usaf_memo")
        .join("0.2.0");
    Quill::from_tree(walk(&quill_path).expect("walk fixture")).expect("load source")
}

/// A plate that `eval`s a runtime string referencing an unknown variable. In
/// the current Typst version this error's span resolves back to the `eval`
/// call site in `main.typ`, so it is the "resolvable span (common case)" that
/// must be left unchanged by the fix for issue #745.
const EVAL_ERROR_PLATE: &str =
    "#set page(width: 400pt, height: 300pt)\n#eval(\"#general\", mode: \"markup\")\n";

/// Run the plate and return the resulting diagnostics. Compilation happens
/// during `open`, so the error may surface from either `open` or `render`.
fn diagnostics_for(plate: &str) -> Vec<quillmark_core::Diagnostic> {
    match TypstBackend.open(plate, &host_source(), &serde_json::json!({})) {
        Ok(session) => session
            .render(&RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            })
            .expect_err("eval of `#general` should fail to compile")
            .into_diagnostics(),
        Err(err) => err.into_diagnostics(),
    }
}

/// Regression guard for issue #745's second acceptance criterion: a Typst
/// error whose span *does* resolve must be left untouched — it keeps its
/// location and does not get the generic dynamically-evaluated-content hint.
#[test]
fn resolvable_eval_error_is_unchanged() {
    let diags = diagnostics_for(EVAL_ERROR_PLATE);
    assert!(
        !diags.is_empty(),
        "compilation error must carry diagnostics"
    );

    let diag = diags
        .iter()
        .find(|d| d.message.contains("unknown variable: general"))
        .expect("expected the `unknown variable: general` diagnostic");

    assert!(
        diag.location.is_some(),
        "this eval error resolves to the call site; expected a location, got None"
    );
    assert!(
        diag.hint
            .as_deref()
            .map_or(true, |h| !h.contains("dynamically evaluated content")),
        "a resolvable diagnostic must not receive the generic eval hint, got: {:?}",
        diag.hint
    );
}
