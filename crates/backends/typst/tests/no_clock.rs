//! A render reads no clock: `datetime.today()` is a fixed placeholder.

use quillmark_core::{backend::Backend, types::{OutputFormat, RenderOptions}};
use quillmark_typst::TypstBackend;

mod common;

#[test]
fn today_is_the_placeholder_at_any_offset() {
    let plate = "#set page(width: 200pt, height: 100pt)\n\
        #let placeholder = datetime(year: 1970, month: 1, day: 1)\n\
        #assert.eq(datetime.today(), placeholder)\n\
        #assert.eq(datetime.today(offset: -12), placeholder)\n\
        #assert.eq(datetime.today(offset: 14), placeholder)\n\
        #datetime.today().display()\n";
    let session = TypstBackend
        .open(&common::host_with_plate(plate), &serde_json::json!({}))
        .expect("open session");
    session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        .expect("a plate reading today renders against the placeholder");
}
