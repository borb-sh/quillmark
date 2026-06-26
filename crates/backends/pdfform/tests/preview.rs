//! Feature-gated (`--features preview`): pdfform preview via hayro — SVG output
//! and the raster-preview capability (render_rgba / page_size_pt), both rendering
//! the stripped background.
#![cfg(feature = "preview")]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderOptions};
use quillmark_pdfform::PdfformBackend;

fn fixture_quill() -> Quill {
    fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let p: PathBuf = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_form/0.1.0");
    Quill::from_tree(walk(&dir).expect("walk fixture")).expect("load simple_form quill")
}

#[test]
fn svg_is_supported_and_vector() {
    assert!(PdfformBackend
        .supported_formats()
        .contains(&OutputFormat::Svg));

    let quill = fixture_quill();
    let session = PdfformBackend
        .open("", &quill, &serde_json::json!({ "full_name": "Ada" }))
        .expect("open");
    let result = session
        .render(&RenderOptions {
            output_format: Some(OutputFormat::Svg),
            ..Default::default()
        })
        .expect("render svg");

    assert_eq!(result.output_format, OutputFormat::Svg);
    assert_eq!(result.artifacts.len(), 1);
    let svg = String::from_utf8(result.artifacts[0].bytes.clone()).unwrap();
    assert!(svg.starts_with("<svg"), "expected svg, got: {:.40}", svg);
    assert!(svg.contains("<path"), "expected vector paths");
    assert!(!svg.contains("<image"), "should be vector, not rasterized");
    // The regions sidecar rides along on the SVG render too.
    assert_eq!(result.regions.len(), 4);
}

#[test]
fn raster_capability_renders_background() {
    let quill = fixture_quill();
    let session = PdfformBackend
        .open("", &quill, &serde_json::json!({}))
        .expect("open");
    let handle = session.handle();

    assert_eq!(handle.page_size_pt(0), Some((612.0, 792.0)));

    let (w, h, rgba) = handle.render_rgba(0, 2.0).expect("render_rgba");
    assert_eq!((w, h), (1224, 1584)); // 612x792 at 2x
    assert_eq!(rgba.len(), (w * h * 4) as usize);

    // Out-of-range page yields None, not a panic.
    assert!(handle.render_rgba(9, 1.0).is_none());
    assert!(handle.page_size_pt(9).is_none());
}
