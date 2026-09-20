//! A content an editor can build but markdown import never produces
//! (`strong[0,4)` partially overlapping `code[2,6)`) must lower to Typst that
//! *compiles*, not merely markup that looks balanced: the emitter's output is
//! embedded in a `#let _qm_cN = [ .. ]` block, so one unclosed `[` breaks the
//! whole generated helper file's parse.

use quillmark_content::model::{Content, Line, LineKind, MarkKind};
use quillmark_core::backend::Backend;
use quillmark_typst::TypstBackend;

mod common;
use common::quill_with_plate as quill;

fn overlap_content() -> serde_json::Value {
    use quillmark_content::model::Mark;
    let rt = Content::new("abcdef".to_string(), vec![Line::new(LineKind::Para)]).with_marks(vec![
        Mark::new(0, 4, MarkKind::Strong),
        Mark::new(2, 6, MarkKind::Code),
    ]);
    // No cross-kind overlap invariant exists, so normalize/validate keep it.
    quillmark_content::serial::to_canonical_value(&rt.into_normalized())
}

const YAML: &str = r#"
quill:
  name: overlap_compile
  version: 0.1.0
  backend: typst
  description: overlapping wrap+code compile guard
typst:
  plate_file: plate.typ
main:
  fields:
    prose:
      type: richtext
      description: a paragraph with overlapping wrap and code marks
"#;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)

#data.prose
"#;

#[test]
fn overlapping_wrap_and_code_compiles() {
    let data = serde_json::json!({ "prose": overlap_content() });
    let session = TypstBackend
        .open(&quill(YAML, PLATE), &data)
        .expect("overlapping wrap+code content must compile");
    assert!(session.page_count() >= 1, "produced at least one page");
}
