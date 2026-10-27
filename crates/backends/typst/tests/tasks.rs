//! A task list item renders through the helper's `_qm-task`: the plate's
//! renderer under `tasks`, else a drawn box, ticked where done.

use quillmark_core::quill::Quill;

mod common;
use common::glyphs;

const PAGE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 200pt, height: 200pt, margin: 10pt)
"#;

fn quill(plate: &str) -> Quill {
    common::quill_with_plate(
        &common::yaml("main:\n  fields: {}\n"),
        &format!("{PAGE}{plate}#qm.data.at(\"$body\", default: [])\n"),
    )
}

fn svg(quill: &Quill, markdown: &str) -> String {
    common::body_page(quill, markdown).0
}

const TASKS: &str = "- [ ] open\n- [x] done\n  - [ ] nested\n\n  more\n- [x]\n  # heading\n- plain";

/// The built-in box draws ahead of each task's text and ticks the done one;
/// every glyph of the text still renders.
#[test]
fn a_task_draws_its_box_and_tick() {
    let plain = svg(&quill(""), "- open\n- done");
    let open = svg(&quill(""), "- [ ] open\n- [ ] done");
    let done = svg(&quill(""), "- [ ] open\n- [x] done");
    assert_ne!(plain, open, "the box draws");
    assert_ne!(open, done, "the tick draws");
    assert_eq!(glyphs(&plain), glyphs(&done), "the text renders as itself");
    svg(&quill(""), TASKS);
}

/// A plate's renderer replaces the box, receiving whether the item is done
/// and its body.
#[test]
fn the_plates_task_renderer_receives_done_and_the_body() {
    let plate = r#"
#qm.tasks.update(_ => (done, body) => {
  assert(type(done) == bool, message: repr(done))
  assert(type(body) == content, message: repr(body))
  if done { [Y ] } else { [N ] }
  body
})
"#;
    let marked = svg(&quill(plate), "- [ ] a\n- [x] b");
    assert_eq!(glyphs(&marked), glyphs(&svg(&quill(""), "- N a\n- Y b")));
    svg(&quill(plate), TASKS);
}
