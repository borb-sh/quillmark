//! The plain codec: a `codec: "plain"` value is a content like any other on
//! the wire and through every write, holds only plain paragraphs, and reads
//! and revises as its literal text.

use quillmark_content::change::apply;
use quillmark_content::diff::{diff, replace, revise};
use quillmark_content::markdown::{from_markdown, plain, to_plaintext, to_text, to_text_annotated};
use quillmark_content::model::{Anchor, Codec, Content, Invariant};
use quillmark_content::repair::{Raw, repair};
use quillmark_content::wire::ParseError;
use serde_json::json;

fn anchored(s: &str, id: &str, quote: &str) -> Content {
    let mut c = plain(s);
    let start = s.find(quote).unwrap();
    c.anchors.push(Anchor::new(id, start, start + quote.chars().count()));
    c
}

#[test]
fn the_codec_rides_the_wire_and_the_hash_bytes() {
    let c = anchored("a *literal* line", "c1", "literal");
    let v = c.to_value();
    assert_eq!(v["codec"], json!("plain"));
    assert_eq!(Content::from_value(&v), Ok(c.clone()));
    assert_eq!(c.to_json(), serde_json::to_string(&v).unwrap());
    assert!(c.to_json().starts_with(r#"{"anchors":[{"end":10,"id":"c1","start":3}],"blocks":[{"id":"b0","kind":"para"}],"codec":"plain","#));

    let rich = Content { codec: Codec::Rich, ..c.clone() };
    assert!(rich.to_value().get("codec").is_none(), "the rich codec is unspelled");
    assert_ne!(rich.to_json(), c.to_json());

    assert_eq!(
        Content::from_value(&json!({"blocks": [{"id": "b0", "kind": "para"}], "codec": "loud", "text": ""})),
        Err(ParseError::UnknownName { axis: "codec", name: "loud".into() })
    );
}

#[test]
fn a_plain_value_holds_only_plain_paragraphs() {
    let mut v = plain("a b").to_value();
    v["marks"] = json!([{ "start": 0, "end": 1, "type": "strong" }]);
    assert_eq!(Content::from_value(&v), Err(ParseError::Invalid(Invariant::NotPlain)));

    let bold = from_markdown("**a** b").unwrap().content;
    let change = diff(&plain("a b"), &bold).unwrap();
    assert!(apply(&plain("a b"), &change).is_err(), "no step formats a plain value");
}

#[test]
fn every_write_keeps_the_codec() {
    let before = anchored("one two three", "c1", "two");
    let after = plain("one two three four");
    let change = replace(&before, &after).unwrap();
    let applied = apply(&before, &change).unwrap().content;
    assert_eq!(applied.codec, Codec::Plain);
    assert_eq!(applied.anchors, [Anchor::new("c1", 4, 7)]);

    let raw = Raw::from(&before);
    assert_eq!(repair(raw).0, before);

    let mut v = before.to_value();
    v["marks"] = json!([{ "start": 0, "end": 3, "type": "emph" }]);
    let (repaired, _, _) = repair(Raw::from_value(&v).unwrap());
    assert_eq!(repaired, before, "repair strips what a plain value cannot hold");
}

#[test]
fn a_plain_value_reads_and_revises_as_its_literal_text() {
    let c = anchored("a *literal* line", "c1", "literal");
    assert_eq!(to_text(&c), "a *literal* line");
    let read = to_text_annotated(&c);
    assert_eq!(read.markdown, "a *literal* line", "no tag in a plain read");
    assert_eq!(read.anchors.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["c1"]);

    let revised = revise(&c, "so a *literal* line").unwrap();
    assert_eq!(revised.content.codec, Codec::Plain);
    assert_eq!(to_plaintext(&revised.content), "so a *literal* line");
    assert_eq!(revised.content.anchors, [Anchor::new("c1", 6, 13)]);
}

/// A blank line is a paragraph break and any other line break a hard break,
/// both ways, so a value of two paragraphs reads and revises as two.
#[test]
fn a_blank_line_is_a_paragraph_break_both_ways() {
    let two = plain("a\n\nb");
    assert_eq!((two.text.as_str(), two.blocks.len()), ("a\nb", 2));
    assert_eq!(to_plaintext(&two), "a\n\nb");
    let broken = plain("a\nb");
    assert_eq!((broken.text.as_str(), broken.blocks.len()), ("a\u{2028}b", 1));
    let revised = revise(&two, "a\n\nB").unwrap();
    assert_eq!((revised.content.text.as_str(), revised.content.blocks.len()), ("a\nB", 2));
}

mod properties {
    use super::*;
    use proptest::prelude::*;
    use quillmark_content::model::HARD_BREAK;

    fn paragraph() -> impl Strategy<Value = String> {
        prop::collection::vec(("[a-z*_ ]{1,6}", prop::bool::ANY), 0..4).prop_map(|runs| {
            let mut out = String::new();
            for (k, (run, broken)) in runs.iter().enumerate() {
                if k > 0 {
                    out.push(if *broken { HARD_BREAK } else { ' ' });
                }
                out.push_str(run);
            }
            out
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn the_text_read_is_a_fixed_point(s in "[a-z* \n]{0,24}") {
            prop_assert_eq!(to_plaintext(&plain(&s)), s);
        }

        /// A plain value whose every hard break stands between two characters
        /// of its paragraph reads back as itself.
        #[test]
        fn a_value_reads_back_as_itself(paragraphs in prop::collection::vec(paragraph(), 1..5)) {
            let text = paragraphs.join("\n");
            let blocks: Vec<_> = (0..paragraphs.len()).map(|n| json!({"id": format!("b{n}"), "kind": "para"})).collect();
            let v = Content::from_value(&json!({"text": text, "blocks": blocks, "codec": "plain"})).unwrap();
            prop_assert_eq!(plain(&to_plaintext(&v)), v);
        }
    }
}
