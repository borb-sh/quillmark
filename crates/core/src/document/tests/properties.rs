//! `Document`'s two doors over input it did not write: the JSON blob a restore
//! hands back, and the markdown a previous emission produced.
//!
//! The JSON side asserts refusal, never a panic: a panic traps the WASM module
//! and costs the document rather than the operation. The markdown side asserts
//! that an emission re-parses and that a second emission moves nothing —
//! richtext's markdown is a lossy projection, so the loop is a fixed point
//! after one pass rather than the identity. The shaped generators, whose bodies
//! survive the projection whole, take the identity.

use proptest::prelude::*;
use serde_json::{json, Value};

use crate::document::{Card, CardWire, Document};

/// The keys the decoders dispatch on, so generated objects reach past the first
/// branch; the noise arm keeps the rest of the space.
const DISCRIMINATORS: &[&str] = &[
    "type", "kind", "key", "value", "fill", "schema", "main", "cards", "quill", "body", "payload",
    "payloadItems", "items", "text", "inline",
];

fn arb_key() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(DISCRIMINATORS).prop_map(str::to_string),
        "\\PC{0,12}",
    ]
}

/// Arbitrary JSON, container-biased: the decoders branch on object keys and
/// array shapes, so a scalar-weighted generator would spend its budget failing
/// at the first `as_object()`.
fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(Value::from),
        arb_key().prop_map(Value::from),
    ];
    leaf.prop_recursive(6, 96, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::hash_map(arb_key(), inner, 0..6)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// One `payloadItems` entry, tagged so the envelope deserializes and the
/// generated value reaches the payload checks behind it.
fn arb_payload_item() -> impl Strategy<Value = Value> {
    prop_oneof![
        (arb_key(), arb_json(), any::<bool>())
            .prop_map(|(k, v, f)| json!({ "type": "field", "key": k, "value": v, "fill": f })),
        (arb_key(), any::<bool>())
            .prop_map(|(t, i)| json!({ "type": "comment", "text": t, "inline": i })),
        arb_json(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn storage_decode_never_panics(v in arb_json()) {
        let _ = serde_json::from_str::<Document>(&v.to_string());
    }

    /// A well-formed envelope with an arbitrary payload reaches past the
    /// `schema` discriminator, where the interesting decoding is.
    #[test]
    fn storage_decode_never_panics_past_the_tag(main in arb_json(), cards in arb_json()) {
        for schema in [
            "quillmark/document@0.116.0",
            "quillmark/document@0.115.0",
            "quillmark/document@0.112.0",
            "quillmark/document@0.93.0",
            "quillmark/document@0.92.0",
            "quillmark/document@0.82.0",
            "quillmark/document@0.81.0",
        ] {
            let blob = json!({ "schema": schema, "main": main, "cards": cards });
            let _ = serde_json::from_str::<Document>(&blob.to_string());
        }
    }

    /// `CardWire` denies unknown fields, so the envelope is spelled and only
    /// the values inside it are drawn: every case reaches `Card::try_from`,
    /// which is where the kind, duplicate, count and body checks live.
    #[test]
    fn card_wire_decode_never_panics_past_the_tag(
        kind in arb_key(),
        quill in prop::option::of(arb_key()),
        items in prop::collection::vec(arb_payload_item(), 0..6),
        body in arb_json(),
    ) {
        let blob = json!({
            "kind": kind,
            "quill": quill,
            "payloadItems": items,
            "body": body,
        });
        if let Ok(wire) = serde_json::from_value::<CardWire>(blob) {
            let _ = Card::try_from(wire);
        }
    }
}

fn parse_or_skip(src: &str) -> Option<Document> {
    Document::parse(src).ok().map(|p| p.document)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// A body of arbitrary text under a well-formed root block: `Document::parse`
    /// refuses anything without a `$quill` root, so the block is spelled and the
    /// body is what is drawn. U+FFFC is excluded, the island slot being dropped
    /// by both import codecs.
    #[test]
    fn the_markdown_loop_settles_on_an_arbitrary_body(body in "[\\PC--\u{FFFC}]{0,1000}") {
        let Some(doc) = parse_or_skip(&format!("~~~card-yaml\n$quill: q\n~~~\n\n{body}")) else {
            return Ok(());
        };

        let once = doc.to_markdown();
        let reparsed = Document::parse(&once).unwrap_or_else(|e| panic!(
            "re-parse of an emitted document failed.\nError: {}\nBody: {:.200}\nEmitted:\n{}",
            e, body, once
        )).document;

        prop_assert_eq!(&once, &reparsed.to_markdown(), "the loop did not settle");
    }

    #[test]
    fn a_payload_shaped_document_round_trips(
        quill in "[a-z][a-z0-9_]{0,20}",
        key in "[a-z][a-z0-9_]{0,15}",
        value in "\\PC{0,100}"
    ) {
        let src = format!("~~~card-yaml\n$quill: {}\n$kind: main\n{}: \"{}\"\n~~~\n\nBody.\n",
            quill, key, value.replace('\\', "\\\\").replace('"', "\\\""));

        let Some(doc_a) = parse_or_skip(&src) else { return Ok(()) };
        let emit1 = doc_a.to_markdown();

        let doc_b = Document::parse(&emit1).unwrap_or_else(|e| {
            panic!("payload-shaped: re-parse failed.\nError: {}\nSrc:\n{}\nEmitted:\n{}", e, src, emit1)
        }).document;

        prop_assert_eq!(&doc_a, &doc_b, "payload-shaped: doc_a != doc_b.\nEmitted:\n{}", emit1);
        prop_assert_eq!(&emit1, &doc_b.to_markdown(), "payload-shaped: emit not idempotent.");
    }

    #[test]
    fn a_document_with_cards_round_trips(
        quill in "[a-z][a-z0-9_]{0,20}",
        card_kind in "[a-z][a-z0-9_]{0,15}",
        card_key in "[a-z][a-z0-9_]{0,15}",
        card_value in "[a-zA-Z0-9 ]{0,50}"
    ) {
        let src = format!(
            "~~~card-yaml\n$quill: {}\n$kind: main\ntitle: \"test\"\n~~~\n\nBody here.\n\n~~~card-yaml\n$kind: {}\n{}: \"{}\"\n~~~\n\nCard body.\n",
            quill, card_kind, card_key, card_value
        );

        let Some(doc_a) = parse_or_skip(&src) else { return Ok(()) };
        let emit1 = doc_a.to_markdown();

        let doc_b = Document::parse(&emit1).unwrap_or_else(|e| {
            panic!("with-cards: re-parse failed.\nError: {}\nEmitted:\n{}", e, emit1)
        }).document;

        prop_assert_eq!(&doc_a, &doc_b);
        prop_assert_eq!(&emit1, &doc_b.to_markdown());
    }
}
