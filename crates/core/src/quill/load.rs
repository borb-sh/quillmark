//! Quill loading and construction routines.
use crate::error::{Diagnostic, Severity};

use super::{FileTreeNode, Quill, QuillConfig};

fn diag(message: impl Into<String>, code: &str) -> Diagnostic {
    Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string())
}

impl Quill {
    /// Build a Quill from an in-memory file tree. Filesystem walking belongs
    /// upstream (see `quillmark::quill_from_path`).
    ///
    /// # Errors
    ///
    /// Returns a non-empty `Vec<Diagnostic>` describing every problem found.
    /// When `Quill.yaml` itself contains multiple errors they are all
    /// reported together. Backend-specific assets (e.g. a Typst plate) are
    /// not read here: a backend resolves its own inputs at render time.
    ///
    /// Advisory diagnostics ride the quill, readable at any time from
    /// [`warnings`](Self::warnings).
    pub fn from_tree(root: FileTreeNode) -> Result<Self, Vec<Diagnostic>> {
        let quill_yaml_bytes = root.get_file("Quill.yaml").ok_or_else(|| {
            vec![diag(
                "Quill.yaml not found in file tree",
                "quill::missing_file",
            )]
        })?;

        let quill_yaml_content = String::from_utf8(quill_yaml_bytes.to_vec()).map_err(|e| {
            vec![diag(
                format!("Quill.yaml is not valid UTF-8: {}", e),
                "quill::invalid_utf8",
            )]
        })?;

        let (config, warnings) = QuillConfig::from_yaml_with_warnings(&quill_yaml_content)?;

        // The one bundle file core itself resolves at load: a declared example
        // names a markdown document, not a backend asset, so core can hold it
        // to its word. Reading it here is what makes `Quill::example` total.
        if let Some(path) = &config.example {
            match root.get_file(path) {
                None => {
                    return Err(vec![diag(
                        format!("quill.example names no file in the bundle: '{}'", path),
                        "quill::example_missing",
                    )
                    .with_hint(
                        "Use a bundle-relative path (no leading '/' and no '..'), \
                         e.g. 'example.md'."
                            .to_string(),
                    )])
                }
                Some(bytes) => {
                    if let Err(e) = std::str::from_utf8(bytes) {
                        return Err(vec![diag(
                            format!("quill.example file '{}' is not valid UTF-8: {}", path, e),
                            "quill::example_invalid_utf8",
                        )]);
                    }
                }
            }
        }

        Ok(Quill {
            config,
            files: root,
            warnings,
        })
    }
}
