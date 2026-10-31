//! Quill builders shared by the backend's acceptance tests: a self-contained
//! quill from literals, and the `usaf_memo` fixture as a host when a test needs
//! real fonts and packages but nothing quill-specific.

// Each integration test binary compiles this module and uses part of it.
#![allow(dead_code)]

use quillmark_core::backend::Backend;
use quillmark_core::error::Diagnostic;
use quillmark_core::quill::{CalendarDate, FileTreeNode, Quill};
use quillmark_core::types::{OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;
use std::collections::HashMap;

/// No fonts dir is needed (the backend's embedded Roboto faces render text)
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
    let rt = quillmark_content::import::from_markdown(markdown).expect("import").content;
    quillmark_content::serial::to_canonical_value(&rt)
}

/// The render date the tests open sessions with.
pub fn test_date() -> CalendarDate {
    CalendarDate::new(2026, 3, 14).expect("a calendar day")
}

/// Each page's SVG for a body of `markdown`, and the render's warnings.
pub fn body_svgs(quill: &Quill, markdown: &str) -> (Vec<String>, Vec<Diagnostic>) {
    let data = serde_json::json!({ "$body": content(markdown) });
    let session = TypstBackend::default()
        .open(quill, &data, test_date())
        .unwrap_or_else(|e| panic!("{markdown:?} compiles: {e}"));
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        .expect("render");
    let pages = result
        .artifacts
        .iter()
        .map(|a| String::from_utf8(a.bytes.clone()).expect("svg is text"))
        .collect();
    (pages, result.warnings)
}

/// [`body_svgs`] for a body that renders to one page.
pub fn body_page(quill: &Quill, markdown: &str) -> (String, Vec<Diagnostic>) {
    let (mut pages, warnings) = body_svgs(quill, markdown);
    assert_eq!(pages.len(), 1, "one page for {markdown:?}");
    (pages.remove(0), warnings)
}

/// The glyphs an SVG draws.
pub fn glyphs(svg: &str) -> usize {
    svg.matches("<use ").count()
}
