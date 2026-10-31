//! The Typst backend resolves its own plate at `open`: `plate.typ` by name, or
//! the file `typst.plate_file` names.

use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;

const YAML: &str = "quill:\n  name: t\n  version: \"1.0.0\"\n  backend: typst\n  \
                    description: d\n\ntypst:\n  plate_file: plate.typ\n";

const BARE: &str = "quill:\n  name: t\n  version: \"1.0.0\"\n  backend: typst\n  description: d\n";

/// With no `typst:` section the plate is `plate.typ` at the quill root. A
/// `plate_file` names another, and `plate.typ` beside it is then a module no
/// render compiles.
#[test]
fn the_plate_is_plate_typ_by_name_unless_plate_file_names_another() {
    let by_name = tree_quill(&[("Quill.yaml", BARE), ("plate.typ", "hello\n")]);
    assert_eq!(quillmark_typst::plate_file(&by_name).expect("plate.typ loads"), "plate.typ");
    TypstBackend::default()
        .open(&by_name, &serde_json::json!({}), common::test_date())
        .expect("plate.typ renders");

    let declared = tree_quill(&[
        ("Quill.yaml", &format!("{BARE}typst:\n  plate_file: tpl/main.typ\n")),
        ("plate.typ", "#nowhere\n"),
        ("tpl/main.typ", "hello\n"),
    ]);
    assert_eq!(quillmark_typst::plate_file(&declared).expect("the plate loads"), "tpl/main.typ");
    TypstBackend::default()
        .open(&declared, &serde_json::json!({}), common::test_date())
        .expect("the declared plate renders, and plate.typ is not compiled");
}

/// A quill holding no `plate.typ` and declaring no `plate_file` has no plate:
/// `open` refuses it, hinting both.
#[test]
fn a_quill_with_no_plate_fails_at_open() {
    let diags = open_err(&[("Quill.yaml", BARE), ("main.typ", "hello\n")]);
    let codes: Vec<_> = diags.iter().map(|d| d.code.as_deref()).collect();
    assert_eq!(codes, [Some("typst::plate_missing")], "{diags:?}");
    let hint = diags[0].hint.as_deref().expect("a hint");
    assert!(hint.contains("`plate.typ`") && hint.contains("`typst.plate_file`"), "{hint}");
}

/// A plate the quill holds at a path Typst refuses has no name the world can
/// load it at: taking `main.typ` would shadow the quill's own `main.typ`. A
/// `plate_file` that is not a string, or holds no step once its `.` and empty
/// steps drop, names no path at all.
#[test]
fn a_plate_file_typst_cannot_address_fails_at_open() {
    for (declared, plate) in [
        ("lay\\out.typ", "lay\\out.typ"),
        ("5", "plate.typ"),
        ("~", "plate.typ"),
        ("\"\"", "plate.typ"),
        (".", "plate.typ"),
        ("./", "plate.typ"),
    ] {
        let diags = open_err(&[
            ("Quill.yaml", &YAML.replace("plate.typ", declared)),
            (plate, "#import \"main.typ\": word\n#word\n"),
            ("main.typ", "#let word = \"real\"\n"),
        ]);
        let codes: Vec<_> = diags.iter().map(|d| d.code.as_deref()).collect();
        assert_eq!(codes, [Some("typst::plate_path_invalid")], "{declared}: {diags:?}");
    }
}

/// A `.` step or an empty one in `plate_file` drops: the plate renders, and
/// its diagnostics name it by its path from the quill root.
#[test]
fn a_plate_file_drops_its_dot_and_empty_steps() {
    use quillmark_core::types::{OutputFormat, RenderOptions};

    for (declared, file) in [
        ("./plate.typ", "plate.typ"),
        ("tpl/./plate.typ", "tpl/plate.typ"),
        ("tpl//plate.typ", "tpl/plate.typ"),
    ] {
        let q = tree_quill(&[
            ("Quill.yaml", &YAML.replace("plate.typ", declared)),
            (file, "#set text(font: \"nosuchfont\")\nhello\n"),
        ]);
        assert_eq!(quillmark_typst::plate_file(&q).expect("the plate loads"), file);
        let session = TypstBackend::default()
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
        assert_eq!(located, [file], "{declared}: {:?}", session.warnings());
    }
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
            "quill:\n  name: t\n  version: \"1.0.0\"\n  backend: typst\n  description: d\n\n\
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
    const YAML: &str = "quill:\n  name: t\n  version: \"1.0.0\"\n  backend: typst\n  \
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

/// The helper's names reach a plate only through the module name it chose, so
/// every other import form of the helper, in the plate or a module it imports,
/// is refused at the import, with the form that compiles in the message.
#[test]
fn every_helper_import_but_the_module_form_fails_at_open() {
    for (file, import) in [
        ("plate.typ", "#import \"@local/quillmark-helper:0.2.0\": *"),
        ("plate.typ", "#import \"@local/quillmark-helper:0.2.0\": data, ink"),
        ("plate.typ", "#import \"@local/quillmark-helper:0.2.0\""),
        ("plate.typ", "#import \"@local/quillmark-helper:0.2.0\" as qm: data"),
        ("plate.typ", "#import \"@local/quillmark-helper:0.1.0\" as qm"),
        ("lib.typ", "#import \"@local/quillmark-helper:0.2.0\": data"),
    ] {
        let (plate, module) = if file == "plate.typ" {
            (format!("hello\n{import}\n"), "#let x = 1\n".to_string())
        } else {
            ("#import \"lib.typ\"\nhello\n".to_string(), format!("{import}\n"))
        };
        let diags = open_err(&[("Quill.yaml", YAML), ("plate.typ", &plate), ("lib.typ", &module)]);
        let [diag] = &diags[..] else {
            panic!("{import}: one refusal: {diags:?}");
        };
        assert_eq!(diag.code.as_deref(), Some("typst::helper_import"), "{import}");
        assert!(
            diag.message.contains("#import \"@local/quillmark-helper:0.2.0\" as qm"),
            "{import}: {}",
            diag.message
        );
        let at = diag.location.as_ref().expect("the import is located");
        let line = if file == "plate.typ" { 2 } else { 1 };
        assert_eq!((at.file.as_str(), at.line), (file, line), "{import}");
    }

    let session = TypstBackend::default().open(
        &tree_quill(&[
            ("Quill.yaml", YAML),
            ("plate.typ", "#import \"@local/quillmark-helper:0.2.0\" as helper\n#helper.data\n"),
        ]),
        &serde_json::json!({}),
        common::test_date(),
    );
    assert!(session.is_ok(), "the module form compiles under any name");
}

fn open_err(files: &[(&str, &str)]) -> Vec<quillmark_core::error::Diagnostic> {
    match TypstBackend::default().open(&tree_quill(files), &serde_json::json!({}), common::test_date()) {
        Ok(_) => panic!("the compile must fail"),
        Err(e) => e.into_diagnostics(),
    }
}

/// `files` are inserted under their `/`-joined tree paths, `Quill.yaml` among them.
fn tree_quill(files: &[(&str, &str)]) -> quillmark_core::quill::Quill {
    let (yaml, rest): (Vec<_>, Vec<_>) = files.iter().partition(|(path, _)| *path == "Quill.yaml");
    let rest: Vec<(&str, &[u8])> = rest.iter().map(|(p, c)| (*p, c.as_bytes())).collect();
    common::quill(yaml[0].1, &rest)
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
