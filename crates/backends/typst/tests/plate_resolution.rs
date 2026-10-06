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

/// A `plate_file` led by `./` names the file at the quill root: the plate
/// renders, and its diagnostics name it as they would for `plate.typ`.
#[test]
fn a_plate_file_led_by_dot_slash_names_the_quill_root_file() {
    use quillmark_core::types::{OutputFormat, RenderOptions};

    let q = quill(
        &YAML.replace("plate.typ", "./plate.typ"),
        &[("plate.typ", b"#set text(font: \"nosuchfont\")\nhello\n")],
    );
    let session = TypstBackend
        .open(&q, &serde_json::json!({}), common::test_date())
        .expect("the plate loads");
    let pdf = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("the plate renders");
    assert!(!pdf.artifacts[0].bytes.is_empty());
    let located: Vec<_> = session
        .warnings()
        .iter()
        .filter_map(|d| d.location.as_ref().map(|l| l.file.as_str()))
        .collect();
    assert_eq!(located, ["plate.typ"], "{:?}", session.warnings());
}

/// The tree holds no file at a path with a leading `/` or a `..` step, so such
/// a `plate_file` misses its plate. The hint offers the spelling from the quill
/// root where the quill holds a file there; a plain path that misses has no
/// spelling to fix.
#[test]
fn a_rooted_or_dot_dot_plate_file_misses_and_hints_the_spelling_from_the_root() {
    let hint_of = |declared: &str| {
        let diags = open_err(&[
            ("Quill.yaml", &YAML.replace("plate.typ", declared)),
            ("plate.typ", "hello\n"),
        ]);
        let codes: Vec<_> = diags.iter().map(|d| d.code.as_deref()).collect();
        assert_eq!(codes, [Some("typst::plate_missing")], "{declared}: {diags:?}");
        diags[0].hint.clone()
    };

    for declared in ["/plate.typ", "tpl/../plate.typ"] {
        let hint = hint_of(declared).expect("a hint");
        assert!(hint.contains("`plate.typ`"), "{declared}: {hint}");
    }
    for (declared, spelling) in [("../plate.typ", "`plate.typ`"), ("/absent.typ", "`absent.typ`")] {
        let hint = hint_of(declared).expect("a hint");
        assert!(!hint.contains(spelling), "{declared}: {hint}");
    }
    assert_eq!(hint_of("absent.typ"), None);
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

/// A path missed below the quill root hints the rooted path of the file the
/// root holds, from any module depth and for any file kind, but only where it
/// holds one. A bare spelling and a rooted one through the module's directory
/// search the same path, and draw the same hint.
#[test]
fn a_path_missed_below_the_quill_root_hints_the_rooted_file_the_root_holds() {
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

    for written in [image, "#image(\"/tpl/assets/logo.svg\")\n"] {
        let hint = hint_of(&[
            ("Quill.yaml", YAML),
            ("tpl/layout.typ", written),
            ("assets/logo.svg", SVG),
        ])
        .expect("the root holds assets/logo.svg");
        assert!(hint.contains("`/assets/logo.svg`"), "{written}: {hint}");
    }

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

/// An import of a package the load skipped fails as the missing file it is,
/// and the failure carries the load's reason after that error.
#[test]
fn a_failed_open_carries_the_load_warnings_that_explain_it() {
    use quillmark_core::error::Severity;

    let diags = open_err(&[
        ("Quill.yaml", YAML),
        (
            "packages/broken/typst.toml",
            "[package]\nname = \"broken\"\nentrypoint = \"lib.typ\"\n",
        ),
        ("packages/broken/lib.typ", "#let x = 1\n"),
        ("plate.typ", "#import \"@local/broken:0.1.0\": x\n#x\n"),
    ]);
    let codes: Vec<_> = diags
        .iter()
        .map(|d| (d.severity, d.code.as_deref()))
        .collect();
    assert_eq!(
        codes,
        [
            (Severity::Error, Some("typst::file_not_found")),
            (Severity::Warning, Some("typst::package_manifest")),
        ],
        "{diags:?}"
    );
}
