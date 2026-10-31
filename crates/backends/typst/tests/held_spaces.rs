//! Spaces Typst's markup would collapse reach the page as typed: a line's
//! leading spaces indent it, and a run of spaces inside a line is as wide as
//! the spaces in it.

use quillmark_core::backend::Backend;
use quillmark_core::session::LiveSession;
use quillmark_typst::TypstBackend;

mod common;
use common::{content, quill_with_plate as quill, yaml};

fn body_session(markdown: &str) -> LiveSession {
    TypstBackend::default()
        .open(
            &quill(
                &yaml("main:\n  fields:\n    prose: { type: richtext, multiline: true }\n"),
                r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#qm.data.prose
"#,
            ),
            &serde_json::json!({ "prose": content(markdown) }),
            common::test_date(),
        )
        .expect("open")
}

/// The left edge of the caret before the first `needle` in `markdown`'s body.
fn x_of(markdown: &str, needle: char) -> f32 {
    let session = body_session(markdown);
    let rt = quillmark_content::import::from_markdown(markdown).expect("import").content;
    let pos = rt.text.chars().position(|c| c == needle).expect("needle in text");
    session.locate("prose", pos).expect("caret").rect[0]
}

#[test]
fn collapsible_spaces_keep_their_width() {
    let flush = x_of("Indented", 'I');
    let indented = x_of("&#32;&#32;&#32;&#32;Indented", 'I');
    assert!(indented > flush, "four leading spaces indent: {indented} vs {flush}");

    let one = x_of("a. Z", 'Z');
    let two = x_of("a.  Z", 'Z');
    let four = x_of("a.    Z", 'Z');
    assert!(one < two && two < four, "spaces hold their width: {one}, {two}, {four}");
    assert_eq!(x_of("<u>a. </u> Z", 'Z'), two, "either side of a mark close");
    assert_eq!(x_of("[a. ](https://x.y) Z", 'Z'), two, "either side of a link close");
}

#[test]
fn a_text_value_inks_no_space_it_does_not_hold() {
    TypstBackend::default()
        .open(
            &quill(
                &yaml("main:\n  fields:\n    name: { type: text }\n"),
                r#"
#import "@local/quillmark-helper:0.2.0" as qm
#context assert.eq(measure[Dear #qm.ink(qm.data, "name"),], measure[Dear Mr. Babbage,])
"#,
            ),
            &serde_json::json!({ "name": content("Mr. Babbage") }),
            common::test_date(),
        )
        .expect("the ink sits flush against the text around it");
}
