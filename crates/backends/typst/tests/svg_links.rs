//! A link's URL reaches SVG output as a well-formed `href` that decodes back
//! to the URL.

use quillmark_core::{
    backend::Backend,
    types::{OutputFormat, RenderOptions},
};
use quillmark_typst::TypstBackend;

mod common;

const URL: &str = r#"https://example.com/?a=1&b=2<c"d&amp;e&quot;f"#;
const TILED: &str = "https://example.com/?tiled=1&b=2";

#[test]
fn link_hrefs_parse_as_xml_and_decode_to_the_url() {
    let url = URL.replace('"', "\\\"");
    let plate = format!(
        "#set page(width: 200pt, height: 120pt)\n#link(\"{url}\")[q]\n\
         #rect(fill: tiling(size: (10pt, 10pt), link(\"{TILED}\", box(width: 5pt, height: 5pt, fill: red))))\n"
    );
    let quill = common::quill_with_plate(&common::yaml("main:\n  fields: {}\n"), &plate);
    let session = TypstBackend::default()
        .open(&quill, &serde_json::json!({}), common::test_date())
        .expect("compiles");
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Svg))
        .expect("render");
    let svg = String::from_utf8(result.artifacts[0].bytes.clone()).expect("svg is text");

    let doc = roxmltree::Document::parse(&svg).unwrap_or_else(|e| panic!("{e}: {svg}"));
    let xlink = "http://www.w3.org/1999/xlink";
    let links: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("a")).collect();
    let hrefs: Vec<_> =
        links.iter().map(|a| (a.attribute("href"), a.attribute((xlink, "href")))).collect();
    assert_eq!(hrefs, [(Some(URL), Some(URL)), (Some(TILED), Some(TILED))]);
}
