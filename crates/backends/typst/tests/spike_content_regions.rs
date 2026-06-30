//! SPIKE (#773): validate that auto-tagged content (markdown) fields produce
//! frame-derived regions keyed on the schema path — including multi-rect output
//! when a field's content breaks across pages. Run with `--nocapture` to see
//! the geometry.

use std::collections::HashMap;

use quillmark_core::{Backend, FileTreeNode, Quill};
use quillmark_typst::TypstBackend;

const QUILL_YAML: &str = r#"
quill:
  name: spike
  version: 0.1.0
  backend: typst
  description: region auto-tag spike

typst:
  plate_file: plate.typ

main:
  fields:
    intro:
      type: markdown
      description: a short intro paragraph
    body:
      type: markdown
      description: a long body that wraps and breaks across pages
"#;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.intro

#data.body
"#;

fn quill() -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: QUILL_YAML.as_bytes().to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: PLATE.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
}

#[test]
fn content_fields_emit_frame_regions() {
    // `body` is long enough to overflow page 0 and continue on page 1, so the
    // frame walk should emit one region per page it touches.
    let long = "This is a markdown paragraph that wraps across several lines. "
        .repeat(200);
    let data = serde_json::json!({
        "intro": "A **short** intro paragraph on the first page.",
        "body": long,
    });

    let session = TypstBackend.open(&quill(), &data).expect("open");
    let regions = session.regions();

    println!("\n=== SPIKE regions ({}) ===", regions.len());
    for r in &regions {
        println!(
            "  field={:<10} page={} rect=[{:7.1}, {:7.1}, {:7.1}, {:7.1}]  ({}w x {}h)",
            r.field,
            r.page,
            r.rect[0],
            r.rect[1],
            r.rect[2],
            r.rect[3],
            (r.rect[2] - r.rect[0]) as i32,
            (r.rect[3] - r.rect[1]) as i32,
        );
    }
    println!("=== end ===\n");

    // 1. intro tags, keyed on the schema path (not a widget name).
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(intro.len(), 1, "intro should be one region on one page");
    let [x0, y0, x1, y1] = intro[0].rect;
    assert!(x1 > x0 && y1 > y0, "intro must have positive area: {:?}", intro[0].rect);

    // 2. body spans pages → multiple regions sharing the field, distinct pages.
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body").collect();
    assert!(
        body.len() >= 2,
        "long body should break across pages into >=2 regions, got {}",
        body.len()
    );
    let pages: std::collections::HashSet<usize> = body.iter().map(|r| r.page).collect();
    assert!(pages.len() >= 2, "body regions should span >=2 distinct pages, got {pages:?}");
    for r in &body {
        assert!(r.rect[2] - r.rect[0] > 200.0, "each body fragment should span most of the text column");
    }
}
