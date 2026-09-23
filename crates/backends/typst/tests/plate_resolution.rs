//! The Typst backend resolves its own plate from `typst.plate_file`. Core reads
//! no template at load time, so a missing plate fails at `open`, not at load.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;
use common::quill;

const YAML: &str = "quill:\n  name: t\n  version: \"1.0\"\n  backend: typst\n  \
                    description: d\n\ntypst:\n  plate_file: plate.typ\n";

#[test]
fn plate_file_is_resolved_from_the_typst_section() {
    let q = quill(
        YAML,
        &[(
            "plate.typ",
            b"#set page(width: 100pt, height: 100pt)\n= Hi\n",
        )],
    );
    let session = TypstBackend
        .open(&q, &serde_json::json!({}))
        .expect("open should resolve typst.plate_file and compile");
    assert!(session.page_count() >= 1);
}

#[test]
fn missing_plate_file_errors_at_open_not_load() {
    let q = quill(YAML, &[]);
    let err = match TypstBackend.open(&q, &serde_json::json!({})) {
        Ok(_) => panic!("a missing plate file must fail at open"),
        Err(e) => e,
    };
    let diags = err.into_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::plate_missing")),
        "expected a typst::plate_missing diagnostic, got {:?}",
        diags.iter().map(|d| d.code.as_deref()).collect::<Vec<_>>()
    );
}

#[test]
fn a_nested_plate_resolves_assets_from_the_root_and_diagnoses_under_its_own_path() {
    use quillmark_core::quill::{FileTreeNode, Quill};
    use std::collections::HashMap;

    let mut root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    let files: [(&str, &[u8]); 3] = [
        (
            "Quill.yaml",
            b"quill:\n  name: t\n  version: \"1.0\"\n  backend: typst\n  description: d\n\n\
              typst:\n  plate_file: tpl/layout.typ\n",
        ),
        (
            "tpl/layout.typ",
            b"#set page(width: 100pt, height: 100pt)\n#image(\"assets/dot.svg\")\n\
              #let d = (a: 1)\n#d.presentr\n",
        ),
        (
            "assets/dot.svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>",
        ),
    ];
    for (path, contents) in files {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.to_vec(),
            },
        )
        .expect("insert");
    }
    let q = Quill::from_tree(root).expect("load quill");

    let diags = match TypstBackend.open(&q, &serde_json::json!({})) {
        Ok(_) => panic!("the missing key must fail the compile"),
        Err(e) => e.into_diagnostics(),
    };
    assert!(
        !diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::file_not_found")),
        "assets/ resolves from the quill root: {diags:?}"
    );
    let location = diags
        .iter()
        .find(|d| d.message.contains("presentr"))
        .and_then(|d| d.location.as_ref())
        .expect("the missing-key error carries a location");
    assert_eq!(
        (location.file.as_str(), location.line, location.column),
        ("tpl/layout.typ", 4, 4)
    );
}
