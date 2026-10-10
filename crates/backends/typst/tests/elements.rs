//! A carrier element renders through the helper's dispatcher: the plate's
//! renderer under `elements`, its attributes as strings, else the built-in
//! `keep`, else what the element wraps.

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

/// A renderer that compiles only when its attributes arrive as written.
const STAMP: &str = r#"
#elements.update(e => e + (stamp: (attrs, body) => {
  assert(attrs == (day: "2024-01-15", size: "4"), message: repr(attrs))
  text(fill: red, body)
}))
"#;

fn quill(plate: &str) -> Quill {
    common::quill_with_plate(
        &common::yaml("main:\n  fields: {}\n"),
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

const STAMPED: &str = "a\n\n<qm-stamp size=\"4\" day=\"2024-01-15\">\n\nb\n\n</qm-stamp>\n\nc";

#[test]
fn the_plates_renderer_receives_the_attributes() {
    let rendered = pages(&quill(STAMP), STAMPED);
    let unregistered = pages(&quill(""), STAMPED);
    assert_ne!(rendered, unregistered, "the renderer colors the stamped text");
    let glyphs = |pages: &[String]| pages.iter().map(|p| p.matches("<use ").count()).sum::<usize>();
    assert_eq!(glyphs(&unregistered), glyphs(&pages(&quill(""), "a\n\nb\n\nc")), "unregistered draws its text");
}

/// The built-in `keep` moves a run that would break across pages to the next
/// one whole, whatever the plate's registry holds.
#[test]
fn the_built_in_keep_keeps_its_run_on_one_page() {
    let filler = "line\n\n".repeat(3);
    let run = "one\n\ntwo\n\nthree";
    let kept = format!("{filler}<qm-keep>\n\n{run}\n\n</qm-keep>");
    let glyphs = |svg: &String| svg.matches("<use ").count();
    let bare = pages(&quill(""), &format!("{filler}{run}"));
    for plate in ["", "#elements.update((stamp: (attrs, body) => body))\n"] {
        let kept = pages(&quill(plate), &kept);
        assert!(
            glyphs(&kept[0]) < glyphs(&bare[0]),
            "under {plate:?} the kept run leaves the first page: {} vs {}",
            glyphs(&kept[0]),
            glyphs(&bare[0])
        );
    }
}

const UNREGISTERED: &str = "typst::unregistered_element";

/// The compile's warnings under `typst::unregistered_element`.
fn unregistered(quill: &Quill, markdown: &str) -> Vec<quillmark_core::error::Diagnostic> {
    let data = serde_json::json!({ "$body": content(markdown) });
    let session = TypstBackend
        .open(quill, &data, common::test_date())
        .unwrap_or_else(|e| panic!("{markdown:?} compiles: {e}"));
    session
        .warnings()
        .iter()
        .filter(|d| d.code.as_deref() == Some(UNREGISTERED))
        .cloned()
        .collect()
}

/// A misspelled `keep` warns once at its field however often the plate places
/// it, and names what the quill renders: `keep`, and a renderer the plate
/// registers after the content, neither of which warns.
#[test]
fn an_element_no_renderer_takes_warns_at_its_field() {
    let quill = common::quill_with_plate(
        &common::yaml("main:\n  fields: {}\n"),
        &format!(
            "{PAGE}#data.at(\"$body\", default: [])\n#data.at(\"$body\", default: [])\n\
             #elements.update(e => e + (stamp: (attrs, body) => text(fill: red, body)))\n"
        ),
    );
    let markdown = "<qm-kep>\n\na\n\n</qm-kep>\n\n<qm-keep>\n\nb\n\n</qm-keep>\n\n\
                    <qm-stamp>\n\nc\n\n</qm-stamp>\n\n- <qm-kep>\n\n  d\n\n  </qm-kep>";
    let warned = unregistered(&quill, markdown);
    assert_eq!(warned.len(), 1, "{warned:#?}");
    assert_eq!(warned[0].path.as_deref(), Some("main.body"));
    assert!(warned[0].message.contains("`qm-kep`"), "{}", warned[0].message);
    let hint = warned[0].hint.as_deref().unwrap_or_default();
    assert!(hint.contains("`qm-keep`") && hint.contains("`qm-stamp`"), "{hint}");
}
