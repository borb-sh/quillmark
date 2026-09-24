//! An authored blank on every `usaf_memo` enum still renders.
//!
//! `frontmatter` asserts `memo_style in ("usaf", "daf")` and `indorsement`
//! asserts its `format`, so a plate that passes a blank through to either fails
//! the Typst compile rather than mis-rendering quietly. Deleting a plate guard
//! must fail here rather than in a user's document.

#![cfg(feature = "typst")]

use quillmark::{OutputFormat, RenderOptions};

mod common;

/// Every enum `usaf_memo` declares, authored blank at once: the main card's
/// three and the indorsement kind's two.
const BLANK_EVERYWHERE: &str = "\
~~~card-yaml
$quill: usaf_memo@0.2.0
$kind: main
letterhead_title: DEPARTMENT OF THE AIR FORCE
letterhead_caption: [123D EXAMPLE WING]
memo_for: [SOME/CC]
subject: A memo whose every enum is blank
letterhead_seal: \"\"
classification: \"\"
memo_style: \"\"
signature_block: [A. AUTHOR, Captain, USAF, Flight Commander]
~~~

Body prose.

~~~card-yaml
$kind: indorsement
from: SOME/CC
for: OTHER/CC
format: \"\"
action: \"\"
signature_block: [B. ENDORSER, Major, USAF, Commander]
~~~
Indorsement prose.
";

#[test]
fn every_enum_authored_blank_still_renders() {
    let (engine, quill) = common::memo();

    let parsed = quillmark::Document::parse(BLANK_EVERYWHERE)
        .expect("document should parse")
        .document;

    let blocking: Vec<_> = quill
        .validate(&parsed)
        .into_iter()
        .filter(|d| d.code.as_deref() != Some("validation::must_fill"))
        .collect();
    assert!(
        blocking.is_empty(),
        "an authored blank is in-domain for every enum; got: {blocking:?}"
    );

    let rendered = engine
        .render(
            quill,
            &parsed,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("a memo with every enum blank should render");
    assert!(
        !rendered.artifacts[0].bytes.is_empty(),
        "the render should produce a non-empty PDF"
    );
}

/// The seal is the one blank with a visible render consequence: it omits the
/// seal rather than falling back to an asset nobody chose.
#[test]
fn a_blank_seal_omits_the_seal_rather_than_choosing_one() {
    let (engine, quill) = common::memo();

    let images = |seal: &str| {
        let md = BLANK_EVERYWHERE.replace("letterhead_seal: \"\"", seal);
        let doc = quillmark::Document::parse(&md).expect("parse").document;
        let svg = engine
            .render(
                quill,
                &doc,
                &RenderOptions::default().with_output_format(OutputFormat::Svg),
            )
            .expect("render")
            .artifacts[0]
            .bytes
            .clone();
        String::from_utf8(svg).expect("svg").matches("<image").count()
    };

    assert!(
        images("letterhead_seal: \"\"") < images("letterhead_seal: dow"),
        "a blank seal must draw no seal: that is the silent fabrication the \
         blank exists to close"
    );
}
