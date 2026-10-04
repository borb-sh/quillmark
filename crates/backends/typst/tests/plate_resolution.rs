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
    let err = match TypstBackend.open(&q, &serde_json::json!({}), common::test_date()) {
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

/// A plate the quill holds at a path Typst refuses has no name the world can
/// load it at: taking `main.typ` would shadow the quill's own `main.typ`.
#[test]
fn a_plate_file_typst_cannot_address_fails_at_open() {
    let diags = open_err(&[
        (
            "Quill.yaml",
            "quill:\n  name: t\n  version: \"1.0\"\n  backend: typst\n  description: d\n\n\
             typst:\n  plate_file: lay\\out.typ\n",
        ),
        ("lay\\out.typ", "#import \"main.typ\": word\n#word\n"),
        ("main.typ", "#let word = \"real\"\n"),
    ]);
    let codes: Vec<_> = diags.iter().map(|d| d.code.as_deref()).collect();
    assert_eq!(codes, [Some("typst::plate_path_invalid")], "{diags:?}");
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

/// A bare path missed below the quill root hints its rooted spelling, from any
/// module depth and for any file kind, but only where the quill holds a file
/// at that spelling.
#[test]
fn a_bare_path_missing_a_quill_root_file_hints_the_rooted_spelling() {
    const YAML: &str = "quill:\n  name: t\n  version: \"1.0\"\n  backend: typst\n  \
                        description: d\n\ntypst:\n  plate_file: tpl/layout.typ\n";
    const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>";
    let hint_of = |files: &[(&str, &str)]| {
        let diags = open_err(files);
        diags
            .iter()
            .find(|d| d.code.as_deref() == Some("typst::file_not_found"))
            .unwrap_or_else(|| panic!("a missing file: {diags:?}"))
            .hint
            .clone()
    };
    let image = "#image(\"assets/logo.svg\")\n";

    let hint = hint_of(&[
        ("Quill.yaml", YAML),
        ("tpl/layout.typ", image),
        ("assets/logo.svg", SVG),
    ])
    .expect("the root holds assets/logo.svg");
    assert!(hint.contains("`/assets/logo.svg`"), "{hint}");

    let hint = hint_of(&[
        ("Quill.yaml", YAML),
        ("tpl/layout.typ", "#import \"parts/header.typ\": accent\n#accent\n"),
        (
            "tpl/parts/header.typ",
            "#import \"shared/theme.typ\": accent\n",
        ),
        ("shared/theme.typ", "#let accent = [x]\n"),
    ])
    .expect("the root holds shared/theme.typ");
    assert!(hint.contains("`/shared/theme.typ`"), "{hint}");

    assert_eq!(
        hint_of(&[("Quill.yaml", YAML), ("tpl/layout.typ", image)]),
        None,
        "no file at the root, no hint"
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

/// An error inside a vendored package names the file under the package's spec,
/// apart from a quill module of the same path.
#[test]
fn a_package_file_is_located_under_its_spec() {
    let diags = open_err(&[
        ("Quill.yaml", YAML),
        (
            "packages/p/typst.toml",
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\n",
        ),
        ("packages/p/lib.typ", "#let b = 1 + \"x\"\n"),
        ("lib.typ", "#let a = 1\n"),
        (
            "plate.typ",
            "#import \"lib.typ\": a\n#import \"@local/p:0.1.0\": b\n#a #b\n",
        ),
    ]);
    let files: Vec<_> = diags
        .iter()
        .filter_map(|d| d.location.as_ref().map(|l| l.file.as_str()))
        .collect();
    assert_eq!(files, ["@local/p:0.1.0/lib.typ"], "{diags:?}");
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
    match TypstBackend.open(&q, &serde_json::json!({}), common::test_date()) {
        Ok(_) => panic!("the compile must fail"),
        Err(e) => e.into_diagnostics(),
    }
}
