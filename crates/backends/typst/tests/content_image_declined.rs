//! An image island in a content field draws nothing and warns. What a content
//! image's url names — a quill asset, a document-relative path, a remote url —
//! is undecided, so the backend declines the construct outright rather than
//! resolving one reading of it, and says so under `backend::declined_construct`
//! against the field's own `DocPath`.

use quillmark_core::{
    Backend, Diagnostic, FileTreeNode, OutputFormat, Quill, RenderOptions, Severity,
};
use quillmark_typst::TypstBackend;
use std::collections::HashMap;

mod common;
use common::content;

const YAML: &str = r#"
quill:
  name: content_image_declined
  version: 0.1.0
  backend: typst
  description: content images the backend declines
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: richtext
      description: a paragraph carrying an image island
card_kinds:
  note:
    description: a note
    fields: {}
"#;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 300pt, margin: 20pt)

#image("assets/logo.svg", width: 16pt)

#data.at("intro", default: [])
#data.at("$body", default: [])
#for card in data.at("$cards", default: ()) [#card.at("$body", default: [])]
"#;

const LOGO: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"
viewBox="0 0 16 16"><rect width="16" height="16" fill="blue"/></svg>"#;

fn quill() -> Quill {
    let mut root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    for (path, contents) in [
        ("Quill.yaml", YAML.as_bytes()),
        ("plate.typ", PLATE.as_bytes()),
        ("assets/logo.svg", LOGO),
    ] {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.to_vec(),
            },
        )
        .expect("insert quill file");
    }
    Quill::from_tree(root).expect("load quill")
}

fn declines(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings
        .iter()
        .filter(|d| d.code.as_deref() == Some("backend::declined_construct"))
        .collect()
}

/// The plate's own `#image("assets/logo.svg")` keeps working: a plate sits at
/// the project root, which is where assets are registered.
#[test]
fn a_content_image_renders_nothing_and_the_plate_keeps_its_own() {
    let data = serde_json::json!({
        "$body": content("![logo](assets/logo.svg)\n\n![rooted](/assets/logo.svg)"),
    });

    let session = TypstBackend
        .open(&quill(), &data)
        .expect("an image island compiles rather than failing the render");
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
        .expect("render ok");
    assert!(!result.artifacts[0].bytes.is_empty(), "produced a PDF");

    let declined = declines(session.warnings());
    assert_eq!(declined.len(), 1, "one per field, not one per image");
    let w = declined[0];
    assert_eq!(w.severity, Severity::Warning);
    assert_eq!(w.path.as_deref(), Some("main.body"));
    assert_eq!(w.args.get("backend").and_then(|v| v.as_str()), Some("typst"));
    assert_eq!(
        w.args.get("construct").and_then(|v| v.as_str()),
        Some("image")
    );
    assert_eq!(w.args.get("count").and_then(|v| v.as_u64()), Some(2));
}

/// A url naming nothing on the file system draws the same refusal in the same
/// vocabulary: the decline is the backend's, so it cannot turn on what the url
/// would have resolved to.
#[test]
fn a_remote_or_missing_url_warns_rather_than_failing_the_compile() {
    for url in ["https://example.com/x.png", "missing.png", "assets/marc.png"] {
        let data = serde_json::json!({ "$body": content(&format!("![alt]({url})")) });
        let session = TypstBackend
            .open(&quill(), &data)
            .unwrap_or_else(|e| panic!("{url} should compile: {e}"));
        assert_eq!(declines(session.warnings()).len(), 1, "for {url}");
    }
}

/// Every content field, not only a body: the address is the one the codegen
/// walked, translated to document-model space. Order is codegen's, which sorts
/// keys at every level, so `$cards` precedes a main field.
#[test]
fn a_named_field_and_a_card_body_carry_their_own_paths() {
    let data = serde_json::json!({
        "intro": content("see ![this](x.png)"),
        "$cards": [
            { "$kind": "note", "$body": content("![a](x.png)") },
        ],
    });

    let session = TypstBackend.open(&quill(), &data).expect("open");
    let paths: Vec<_> = declines(session.warnings())
        .iter()
        .filter_map(|d| d.path.clone())
        .collect();
    assert_eq!(paths, vec!["cards.note[0].body", "main.intro"]);
}

#[test]
fn a_content_without_images_warns_about_nothing() {
    let data = serde_json::json!({ "$body": content("plain **prose** and a [link](https://x)") });
    let session = TypstBackend.open(&quill(), &data).expect("open");
    assert!(declines(session.warnings()).is_empty());
}
