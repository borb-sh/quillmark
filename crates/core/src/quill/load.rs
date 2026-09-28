//! Quill loading and construction routines.
use crate::error::{Diagnostic, Severity};

use super::{FileTreeNode, Quill, QuillConfig};

fn diag(message: impl Into<String>, code: &str) -> Diagnostic {
    Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string())
}

pub(super) fn example_missing(path: &str) -> Diagnostic {
    diag(
        format!("quill.example names '{path}', which is not a file in the quill"),
        "quill::example_missing",
    )
    .with_hint(
        "Name a Markdown file by its path relative to the quill root, e.g. \
         'example: example.md'."
            .to_string(),
    )
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
    /// not read here: a backend resolves its own inputs at render time. A
    /// declared `quill.example` must name a file in the tree; its content is
    /// not read.
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

        // Existence alone: what the example says is `quillmark validate`'s to
        // judge, so its content never refuses a load.
        if let Some(example) = &config.example {
            if root.get_file(example).is_none() {
                return Err(vec![example_missing(example)]);
            }
        }

        Ok(Quill {
            config,
            files: root,
            warnings,
        })
    }
}
