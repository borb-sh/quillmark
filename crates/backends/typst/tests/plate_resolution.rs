//! The Typst backend resolves its own plate from `typst.plate_file`. Core reads
//! no template at load time, so a missing plate fails at `open`, not at load.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;
use common::quill;

const YAML: &str = "quill:\n  name: t\n  version: \"1.0\"\n  backend: typst\n  \
                    description: d\n\ntypst:\n  plate_file: plate.typ\n";

#[test]
fn missing_plate_file_errors_at_open_not_load() {
    let q = quill(YAML, &[]);
    let err = match TypstBackend.open(&q, &serde_json::json!({}), None) {
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

/// `tpl/layout.typ` reaches a sibling by a bare path, a module up a level by
/// `..`, and a module and an asset by a `/`-rooted path.
#[test]
fn project_sources_import_as_typst_resolves_paths() {
    let diags = open_err(&[
        (
            "Quill.yaml",
            "quill:\n  name: t\n  version: \"1.0\"\n  backend: typst\n  description: d\n\n\
             typst:\n  plate_file: tpl/layout.typ\n",
        ),
        (
            "tpl/layout.typ",
            "#import \"parts.typ\": greet\n#import \"/shared/lib.typ\": word\n\
             #set page(width: 100pt, height: 100pt)\n#image(\"/assets/dot.svg\")\n\
             #greet #word\n#let d = (a: 1)\n#d.presentr\n",
        ),
        (
            "tpl/parts.typ",
            "#import \"../shared/lib.typ\": word\n#let greet = [hi #word]\n",
        ),
        ("shared/lib.typ", "#let word = \"there\"\n"),
        (
            "assets/dot.svg",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>",
        ),
    ]);
    assert!(
        !diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::file_not_found")),
        "every path resolves: {diags:?}"
    );
    let location = diags
        .iter()
        .find(|d| d.message.contains("presentr"))
        .and_then(|d| d.location.as_ref())
        .expect("the missing-key error carries a location");
    assert_eq!(
        (location.file.as_str(), location.line, location.column),
        ("tpl/layout.typ", 7, 4)
    );
}

/// A vendored package loads under its spec alone: its files are not project
/// sources a path import reaches.
#[test]
fn a_package_file_is_not_importable_by_path() {
    let diags = open_err(&[
        ("Quill.yaml", YAML),
        (
            "packages/p/typst.toml",
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\n",
        ),
        ("packages/p/lib.typ", "#let x = 1\n"),
        ("plate.typ", "#import \"packages/p/lib.typ\": x\n#x\n"),
    ]);
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::file_not_found")),
        "{diags:?}"
    );
}

/// `files` are inserted under their `/`-joined tree paths.
fn open_err(files: &[(&str, &str)]) -> Vec<quillmark_core::error::Diagnostic> {
    use quillmark_core::quill::{FileTreeNode, Quill};
    use std::collections::HashMap;

    let mut root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    for (path, contents) in files {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.as_bytes().to_vec(),
            },
        )
        .expect("insert");
    }
    let q = Quill::from_tree(root).expect("load quill");
    match TypstBackend.open(&q, &serde_json::json!({}), None) {
        Ok(_) => panic!("the compile must fail"),
        Err(e) => e.into_diagnostics(),
    }
}
