//! The workspace Typst's own tooling compiles a plate from.

use quillmark_core::quill::{FileTreeNode, Quill};
use quillmark_typst::workspace::workspace;

/// `files` are inserted under their `/`-joined tree paths.
fn quill(yaml: &str, files: &[(&str, &[u8])]) -> Quill {
    let mut root = FileTreeNode::Directory {
        files: Default::default(),
    };
    for (path, contents) in [("Quill.yaml", yaml.as_bytes())].iter().chain(files) {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.to_vec(),
            },
        )
        .expect("insert");
    }
    Quill::from_tree(root).expect("load quill")
}

/// A vendored package lands at the spec its manifest names, not its directory
/// name, and a quill shipping no fonts exports the faces it renders in.
#[test]
fn a_workspace_lays_out_packages_by_spec_and_carries_the_fallback_fonts() {
    let q = quill(
        "quill:\n  name: t\n  version: 0.1.0\n  backend: typst\n  description: t\n\
         typst:\n  plate_file: plate.typ\nmain:\n  fields:\n    title: { type: string }\n",
        &[
            ("plate.typ", b"#import \"@preview/p:1.2.0\": x\n"),
            (
                "packages/vendor-x/typst.toml",
                b"[package]\nnamespace = \"preview\"\nname = \"p\"\nversion = \"1.2.0\"\n\
                  entrypoint = \"src/lib.typ\"\n",
            ),
            ("packages/vendor-x/src/lib.typ", b"#let x = 1\n"),
        ],
    );
    let ws = workspace(&q, &serde_json::json!({ "title": "Quarterly Findings" })).expect("workspace");
    assert_eq!(ws.plate_file, "plate.typ");

    let file = |path: &str| {
        ws.files
            .iter()
            .find(|(p, _)| p.as_path() == std::path::Path::new(path))
            .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_else(|| {
                panic!("{path} missing from {:?}", ws.files.iter().map(|(p, _)| p).collect::<Vec<_>>())
            })
    };
    assert!(file("packages/local/quillmark-helper/0.1.0/lib.typ").contains("Quarterly Findings"));
    file("packages/local/quillmark-helper/0.1.0/typst.toml");
    file("packages/preview/p/1.2.0/typst.toml");
    file("packages/preview/p/1.2.0/src/lib.typ");
    file("fonts/Figtree-Regular.ttf");
}

#[test]
fn a_quill_of_another_backend_has_no_workspace() {
    let q = quill(
        "quill:\n  name: f\n  version: 0.1.0\n  backend: acroform\n  description: f\n",
        &[],
    );
    let Err(err) = workspace(&q, &serde_json::json!({})) else {
        panic!("an acroform quill exported a Typst workspace");
    };
    assert_eq!(
        err.diagnostics()[0].code.as_deref(),
        Some("typst::wrong_backend")
    );
}
