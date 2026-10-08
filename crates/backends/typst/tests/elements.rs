//! A carrier element the quill declares renders through the helper's
//! dispatcher: the plate's renderer under `elements` with the attributes the
//! quill's declaration coerces, else the built-in `keep`.

use quillmark_core::{
    backend::Backend,
    quill::Quill,
    types::{OutputFormat, RenderOptions},
};
use quillmark_typst::TypstBackend;

mod common;
use common::content;

const PAGE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, elements
#set page(width: 200pt, height: 120pt, margin: 10pt)
"#;

/// A renderer that compiles only when its attributes arrive at their declared
/// types and the declared default fills the one the document leaves out.
const STAMP: &str = r#"
#elements.update(e => e + (stamp: (attrs, body) => {
  assert(type(attrs.size) == int, message: repr(attrs))
  assert(type(attrs.day) == datetime, message: repr(attrs))
  assert(attrs.tone == "red", message: repr(attrs))
  text(fill: red, body)
}))
"#;

const DECLARING: &str = "honors:\n  elements:\n    keep: { scope: block }\n    stamp:\n      scope: inline\n      \
                         attrs:\n        size: { type: integer }\n        day: { type: date }\n        \
                         tone: { type: enum, values: [red, blue], default: red }\n";

fn quill(honors: &str, plate: &str) -> Quill {
    common::quill_with_plate(
        &common::yaml(&format!("{honors}main:\n  fields: {{}}\n")),
        &format!("{PAGE}{plate}#data.at(\"$body\", default: [])\n"),
    )
}

/// Each page's SVG.
fn pages(quill: &Quill, markdown: &str) -> Vec<String> {
    let data = serde_json::json!({ "$body": content(markdown) });
    let session = TypstBackend
        .open(quill, &data, common::test_date())
        .unwrap_or_else(|e| panic!("{markdown:?} compiles: {e}"));
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        .expect("render");
    result
        .artifacts
        .iter()
        .map(|a| String::from_utf8(a.bytes.clone()).expect("svg is text"))
        .collect()
}

const STAMPED: &str = "a <quill-stamp size=\"4\" day=\"2024-01-15\">b</quill-stamp> c";

#[test]
fn the_plates_renderer_receives_the_declared_attributes() {
    let declared = pages(&quill(DECLARING, STAMP), STAMPED);
    let bare = pages(&quill("", STAMP), STAMPED);
    assert_ne!(declared, bare, "the renderer colors the stamped text");
    assert_eq!(bare, pages(&quill("", STAMP), "a b c"));
}

/// The built-in `keep` moves a run that would break across pages to the next
/// one whole.
#[test]
fn the_built_in_keep_keeps_its_run_on_one_page() {
    let filler = "line\n\n".repeat(3);
    let kept = format!("{filler}<quill-keep>\n\none\n\ntwo\n\nthree\n\n</quill-keep>");
    let glyphs = |svg: &String| svg.matches("<use ").count();
    let declared = pages(&quill(DECLARING, ""), &kept);
    let bare = pages(&quill("", ""), &kept);
    assert!(
        glyphs(&declared[0]) < glyphs(&bare[0]),
        "the kept run leaves the first page: {} vs {}",
        glyphs(&declared[0]),
        glyphs(&bare[0])
    );
}
