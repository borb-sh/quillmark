//! The boundaries that take caller-supplied JSON rather than markdown or a
//! typed Rust value: a restored blob, a server response, an editor's own change
//! stream. One property — arbitrary JSON produces `Err`, never a panic, since a
//! panic traps the WASM module and costs the document rather than the operation.
//! Round-trip preservation belongs to the crate owning each lane.

use proptest::prelude::*;
use quillmark_core::{Card, CardWire, Document};
use serde_json::{json, Value};

/// The keys the decoders dispatch on, so generated objects reach past the first
/// branch; the noise arm keeps the rest of the space.
const DISCRIMINATORS: &[&str] = &[
    "type", "kind", "key", "value", "field", "schema", "main", "body", "payload", "items",
    "islands", "lines", "marks", "text",
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

    #[test]
    fn card_wire_decode_never_panics(v in arb_json()) {
        if let Ok(wire) = serde_json::from_value::<CardWire>(v) {
            let _ = Card::try_from(wire);
        }
    }

    /// The `install` lane.
    #[test]
    fn canonical_content_decode_never_panics(v in arb_json()) {
        let _ = quillmark_content::serial::from_canonical_value(&v);
    }

    /// The `applyChange` lane.
    #[test]
    fn op_wire_decode_never_panics(v in arb_json()) {
        let _ = quillmark_content::ops::change_bundle_from_value(&v);
        let _ = quillmark_content::ops::line_op_from_value(&v);
        let _ = quillmark_content::ops::mark_op_from_value(&v);
    }
}

/// The sweeps pass whether or not a value reaches a decoder, so this pins the
/// reach itself. The floor sits well under the observed rate: generator drift
/// does not fail the build, a collapse in reach does.
#[test]
fn generator_reaches_the_decoders() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strat = arb_json();
    const RUNS: usize = 400;
    let reached = (0..RUNS)
        .filter(|_| {
            let v = strat.new_tree(&mut runner).unwrap().current();
            serde_json::from_value::<CardWire>(v).is_ok()
        })
        .count();
    assert!(
        reached > 10,
        "generator rarely reaches a CardWire: {reached}/{RUNS}"
    );
}
