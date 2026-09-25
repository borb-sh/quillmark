//! Quill builders shared by the backend's acceptance tests: a self-contained
//! quill from literals, and the `usaf_memo` fixture as a host when a test needs
//! real fonts and packages but nothing quill-specific.

// Each integration test binary compiles this module and uses part of it.
#![allow(dead_code)]

use quillmark_core::quill::{CalendarDate, FileTreeNode, Quill};
use std::collections::HashMap;

/// No fonts dir is needed (the backend's embedded Figtree faces render text)
/// and the backend injects the helper package.
pub fn quill(yaml: &str, files: &[(&str, &[u8])]) -> Quill {
    let mut map = HashMap::new();
    map.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    for (name, bytes) in files {
        map.insert(
            (*name).to_string(),
            FileTreeNode::File {
                contents: bytes.to_vec(),
            },
        );
    }
    Quill::from_tree(FileTreeNode::Directory { files: map }).expect("load quill")
}

/// A `Quill.yaml` whose plate is `plate.typ`, declaring `schema` (its `main:`
/// and `card_kinds:` sections).
pub fn yaml(schema: &str) -> String {
    format!(
        "quill:\n  name: t\n  version: 0.1.0\n  backend: typst\n  description: t\n\
         typst:\n  plate_file: plate.typ\n{schema}"
    )
}

/// [`quill`] for the common `Quill.yaml` + `plate.typ` pair.
pub fn quill_with_plate(yaml: &str, plate: &str) -> Quill {
    quill(yaml, &[("plate.typ", plate.as_bytes())])
}

pub fn host_tree() -> FileTreeNode {
    quillmark::tree_from_path(quillmark_fixtures::quills_path("usaf_memo")).expect("walk fixture")
}

/// The fixture's `typst.plate_file: plate.typ` makes the backend read this.
pub fn host_with_plate(plate: &str) -> Quill {
    let mut tree = host_tree();
    if let FileTreeNode::Directory { files } = &mut tree {
        files.insert(
            "plate.typ".to_string(),
            FileTreeNode::File {
                contents: plate.as_bytes().to_vec(),
            },
        );
    }
    Quill::from_tree(tree).expect("load source")
}

/// The shape the seam carries for a richtext field: canonical Content-JSON, not
/// a markdown string.
pub fn content(markdown: &str) -> serde_json::Value {
    let rt = quillmark_content::import::from_markdown(markdown).expect("import");
    quillmark_content::serial::to_canonical_value(&rt)
}

/// The render date the tests open sessions with.
pub fn test_date() -> CalendarDate {
    CalendarDate::new(2026, 3, 14).expect("a calendar day")
}
