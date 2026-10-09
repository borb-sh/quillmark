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
use crate::value::QuillValue;

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
        (arb_key(), arb_json()).prop_map(|(k, v)| json!({ "type": "field", "key": k, "value": v })),
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
            "quillmark/document@0.125.0",
            "quillmark/document@0.124.0",
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

/// A boolean word of YAML 1.1 or 1.2, each letter's case drawn.
fn arb_boolean_word() -> impl Strategy<Value = String> {
    (
        prop::sample::select(&["y", "n", "yes", "no", "on", "off", "true", "false"][..]),
        any::<u8>(),
    )
        .prop_map(|(word, upper)| {
            word.chars()
                .enumerate()
                .map(|(i, c)| if (upper >> i) & 1 == 1 { c.to_ascii_uppercase() } else { c })
                .collect()
        })
}

proptest! {
    /// A plain scalar is a boolean exactly when it spells `true` or `false`,
    /// in any letter case, and any other word is the string written, as a
    /// field, a sequence element and a nested value alike, across an emission.
    #[test]
    fn only_true_and_false_read_as_booleans(word in arb_boolean_word()) {
        let src = format!("~~~\n$quill: q\nv: {word}\nlist: [{word}]\nmap:\n  k: {word}\n~~~\n");
        let doc = Document::parse(&src).expect("a well-formed block").document;
        let want = match word.to_ascii_lowercase().as_str() {
            "true" => json!(true),
            "false" => json!(false),
            _ => json!(word),
        };
        let payload = doc.main().payload();
        let read = |key: &str| payload.get(key).map(|v| v.as_json().clone());
        prop_assert_eq!(read("v"), Some(want.clone()));
        prop_assert_eq!(read("list"), Some(json!([want.clone()])));
        prop_assert_eq!(read("map"), Some(json!({ "k": want })));

        let emitted = doc.to_markdown();
        let reparsed = Document::parse(&emitted).expect("an emission re-parses").document;
        prop_assert_eq!(reparsed, doc, "emitted:\n{}", emitted);
    }
}

/// Multi-line text built from what a literal block must refuse or hold verbatim:
/// edge and line-end whitespace, YAML indicators, a fence, a comment-shaped
/// line, and the line breaks YAML reads that `\n` does not.
fn arb_lines() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just("a"),
            Just("# h"),
            Just("- "),
            Just("k: v"),
            Just("~~~"),
            Just("|"),
            Just("\""),
            Just("\\"),
            Just(" "),
            Just("\t"),
            Just("\n"),
            Just("\n\n"),
            Just("\r"),
            Just("\u{85}"),
            Just("\u{2028}"),
            Just("\u{FEFF}"),
        ],
        0..14,
    )
    .prop_map(|parts| parts.concat())
}

/// A nested key built from what decides whether a plain key opens and where it
/// ends: an inner `:`, a leading `-`, `?` or `$`, a space, a `#`.
fn arb_nested_key() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(&["a", "og", ":", "-", "?", "$", " ", "#", "/"][..]),
        1..6,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Every position a scalar is written at: a top-level field, a nested key,
    /// a sequence item, a sequence item's dash-line key and its next key, the
    /// containers under those two, and `$ext`. `k`, a generated key, fills each
    /// nested key position.
    #[test]
    fn a_multi_line_string_round_trips_at_every_depth(
        text in arb_lines(),
        k in arb_nested_key()
    ) {
        let mut doc = parse_or_skip("~~~\n$quill: q\n$kind: main\n~~~\n").expect("a root");
        let fields = [
            ("top", json!(text)),
            ("map", json!({ k.clone(): text })),
            ("seq", json!([text])),
            (
                "rows",
                json!([{ k.clone(): text, "next": text }, { "first": text, k.clone(): text }]),
            ),
            ("nest", json!([{ "first": { k.clone(): text }, k.clone(): [text] }])),
        ];
        for (key, value) in fields {
            doc.main_mut().store_field(key, QuillValue::from_json(value)).expect("stored");
        }
        let ext = json!({ "meta": { k.clone(): text } });
        doc.main_mut().payload_mut().set_ext(ext.as_object().unwrap().clone());

        let emitted = doc.to_markdown();
        let back = Document::parse(&emitted)
            .unwrap_or_else(|e| panic!("re-parse failed: {e}\nEmitted:\n{emitted}"))
            .document;
        prop_assert_eq!(&doc, &back, "Emitted:\n{}", emitted);
        prop_assert_eq!(&emitted, &back.to_markdown(), "emit not idempotent");
    }
}

#[derive(Debug, Clone, Copy)]
enum Fate {
    Keep,
    Edit,
    Delete,
}

fn arb_fate() -> impl Strategy<Value = Fate> {
    prop_oneof![Just(Fate::Keep), Just(Fate::Edit), Just(Fate::Delete)]
}

fn arb_card() -> impl Strategy<Value = (&'static str, String)> {
    (
        prop::sample::select(&["note", "memo"][..]),
        "[a-z]{1,8}( [a-z]{1,8}){0,4}",
    )
}

fn card_block(kind: &str, body: &str) -> String {
    format!("\n~~~\n$kind: {kind}\n~~~\n\n{body}\n")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// A whole-document revise over cards each anchored at its leading word:
    /// with other cards inserted, deleted, edited and every card reordered, an
    /// unchanged card keeps its anchor and aligns to its stored self, an anchor
    /// rests only on the card its stored card aligned to, and the receipt names
    /// exactly the anchors the revised document no longer holds. The
    /// annotated read revises the stored document to itself.
    #[test]
    fn an_unchanged_card_keeps_its_anchor_through_a_whole_document_revise(
        stored in prop::collection::vec((arb_card(), arb_fate()), 1..7),
        inserted in prop::collection::vec(arb_card(), 0..4),
        order in prop::collection::vec(any::<u32>(), 12),
    ) {
        let mut src = "~~~\n$quill: q\n~~~\n\nMain.\n".to_string();
        for (i, ((kind, words), _)) in stored.iter().enumerate() {
            src.push_str(&card_block(kind, &format!("card{i} {words}")));
        }
        let mut doc = parse_or_skip(&src).expect("a generated document parses");
        for i in 0..stored.len() {
            let mut card = doc.card_mut(i).unwrap();
            let body = super::revise_tests::anchored(card.body(), &format!("card{i}"), &format!("c{i}"));
            card.overwrite_body(body);
        }

        let mut reread = doc.clone();
        let read = doc.to_markdown_annotated().markdown;
        let receipt = reread.revise(&read).expect("the annotated read parses");
        prop_assert!(receipt.dropped_anchors.is_empty(), "{:?}", receipt.dropped_anchors);
        prop_assert_eq!(&reread, &doc, "the annotated read revises to itself:\n{}", read);

        // (stored index if kept unchanged, kind, body)
        let mut incoming: Vec<(Option<usize>, &str, String)> = Vec::new();
        for (i, ((kind, words), fate)) in stored.iter().enumerate() {
            match fate {
                Fate::Keep => incoming.push((Some(i), kind, format!("card{i} {words}"))),
                Fate::Edit => incoming.push((None, kind, format!("card{i} {words} and more"))),
                Fate::Delete => {}
            }
        }
        for (j, (kind, words)) in inserted.iter().enumerate() {
            incoming.push((None, kind, format!("fresh{j} {words}")));
        }
        let mut keyed: Vec<_> = incoming.into_iter().enumerate().collect();
        keyed.sort_by_key(|(n, _)| order[*n]);
        let incoming: Vec<_> = keyed.into_iter().map(|(_, c)| c).collect();

        let mut md = "~~~\n$quill: q\n~~~\n\nMain.\n".to_string();
        for (_, kind, body) in &incoming {
            md.push_str(&card_block(kind, body));
        }
        let receipt = doc.revise(&md).expect("the incoming document parses");

        let held: Vec<Vec<String>> = doc
            .cards()
            .iter()
            .map(|c| super::revise_tests::anchor_ids(c.body()))
            .collect();
        for (p, (kept, _, _)) in incoming.iter().enumerate() {
            if let Some(i) = kept {
                prop_assert_eq!(receipt.alignment[p], Some(*i));
                prop_assert_eq!(&held[p], &vec![format!("c{i}")]);
            }
            for id in &held[p] {
                prop_assert_eq!(Some(id.clone()), receipt.alignment[p].map(|i| format!("c{i}")));
            }
        }
        let mut dropped: Vec<String> = receipt.dropped_anchors.iter().map(|d| d.id.clone()).collect();
        dropped.sort();
        let mut lost: Vec<String> = (0..stored.len())
            .map(|i| format!("c{i}"))
            .filter(|id| !held.iter().flatten().any(|h| h == id))
            .collect();
        lost.sort();
        prop_assert_eq!(dropped, lost);
        for d in &receipt.dropped_anchors {
            let i: usize = d.id[1..].parse().unwrap();
            let kind = stored[i].0 .0;
            prop_assert_eq!(d.path.to_string(), format!("cards.{kind}[{i}].body"));
        }
    }
}
