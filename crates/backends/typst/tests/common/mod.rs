//! Quill builders shared by the backend's acceptance tests: a self-contained
//! quill from literals, and the `usaf_memo` fixture as a host when a test needs
//! real fonts and packages but nothing quill-specific.

// Each integration test binary compiles this module and uses part of it.
#![allow(dead_code)]

use quillmark_core::backend::Backend;
use quillmark_core::error::{Diagnostic, RenderError};
use quillmark_core::quill::{CalendarDate, FileTreeNode, Quill};
use quillmark_core::session::LiveSession;
use quillmark_core::types::{OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;
use std::collections::HashMap;

/// No fonts dir is needed (the backend's embedded Roboto faces render text)
/// and the backend injects the helper package. `files` are inserted under their
/// `/`-joined tree paths.
pub fn quill(yaml: &str, files: &[(&str, &[u8])]) -> Quill {
    let mut root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    for (path, bytes) in
        std::iter::once(("Quill.yaml", yaml.as_bytes())).chain(files.iter().copied())
    {
        root.insert(
            path,
            FileTreeNode::File {
                contents: bytes.to_vec(),
            },
        )
        .expect("insert quill file");
    }
    Quill::from_tree(root).expect("load quill")
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
    let rt = quillmark_content::import::from_markdown(markdown)
        .expect("import")
        .content;
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

/// The PDF `plate` compiles to against the `usaf_memo` host, given `data`.
pub fn compile(plate: &str, data: &serde_json::Value) -> Result<Vec<u8>, RenderError> {
    let session = TypstBackend::default().open(&host_with_plate(plate), data, test_date())?;
    let result = session.render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))?;
    Ok(result.artifacts[0].bytes.clone())
}

/// The parsed PDF, its AcroForm dict, and a `/T` to widget map.
pub fn acroform_of(
    pdf: &[u8],
) -> (
    lopdf::Document,
    lopdf::Dictionary,
    HashMap<String, lopdf::Dictionary>,
) {
    let doc = lopdf::Document::load_mem(pdf).expect("reparse");
    let af_ref = doc
        .catalog()
        .expect("catalog")
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap().clone();
    let mut by_name = HashMap::new();
    for f in af.get(b"Fields").unwrap().as_array().unwrap() {
        let widget = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name =
            String::from_utf8_lossy(widget.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, widget.clone());
    }
    (doc, af, by_name)
}

/// A session over one richtext `prose` on a letter page.
pub fn body_session(body: serde_json::Value) -> LiveSession {
    TypstBackend::default()
        .open(
            &quill_with_plate(
                &yaml("main:\n  fields:\n    prose: { type: richtext, multiline: true }\n"),
                r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#qm.data.prose
"#,
            ),
            &serde_json::json!({ "prose": body }),
            test_date(),
        )
        .expect("open")
}
