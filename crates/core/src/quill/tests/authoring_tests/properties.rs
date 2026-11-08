//! `Quill::from_authoring` over generated JSON: refusal rather than a panic for
//! any value, and for a value of the authoring document's shape, export after
//! import is a fixed point, from the second import on where the export names
//! a loss.

use proptest::prelude::*;

use super::*;

fn arb_json(max_depth: u32) -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i32>().prop_map(serde_json::Value::from),
        "(\\PC|\n){0,12}".prop_map(serde_json::Value::String),
    ];
    leaf.prop_recursive(max_depth, 24, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..=3).prop_map(serde_json::Value::Array),
            prop::collection::btree_map("[a-z$]{1,6}", inner, 0..=3)
                .prop_map(|m| serde_json::Value::Object(m.into_iter().collect())),
        ]
    })
}

/// A card envelope over the quill's own field names, each holding any value.
fn arb_card(kinds: &'static [&'static str]) -> impl Strategy<Value = serde_json::Value> {
    let names = prop::sample::select(vec![
        "subject", "from", "count", "when", "refs", "contact", "marking", "kind", "label",
    ]);
    (
        prop::sample::select(kinds.to_vec()),
        prop::collection::btree_map(names, arb_json(2), 0..=4),
        prop::option::of("[a-z *_\\n]{0,24}"),
    )
        .prop_map(|(kind, fields, body)| {
            let mut card = serde_json::json!({ "kind": kind, "fields": fields });
            if let Some(body) = body {
                card["body"] = body.into();
            }
            card
        })
}

proptest! {
    #[test]
    fn any_value_imports_or_is_refused(value in arb_json(4)) {
        let _ = quill().from_authoring(&value);
    }

    #[test]
    fn export_after_import_is_a_fixed_point(
        main in arb_card(&["main"]),
        cards in prop::collection::vec(arb_card(&["indorsement", "stamp", "unknown"]), 0..=3),
    ) {
        let quill = quill();
        let mut value = main;
        value.as_object_mut().unwrap().remove("kind");
        value["cards"] = cards.into();
        let Ok(first) = quill.from_authoring(&value) else {
            return Ok(());
        };
        let exported = quill.to_authoring(&first.document);
        let second = quill.from_authoring(&exported).expect("an export imports");
        // A body mark markdown cannot write back is a named loss: the export
        // drops it, and the fixed point holds from the second import on.
        let doc = &first.document;
        let losses: Vec<_> = std::iter::once(doc.main())
            .chain(doc.cards())
            .flat_map(|card| quillmark_content::markdown::to_markdown_checked(card.body()).losses)
            .collect();
        prop_assert!(
            losses.iter().all(|l| matches!(l, quillmark_content::markdown::Loss::Mark { .. })),
            "the export loses more than marks: {:?}", losses
        );
        if losses.is_empty() {
            prop_assert_eq!(&second.document, &first.document);
        } else {
            let third = quill.from_authoring(&quill.to_authoring(&second.document)).expect("an export imports");
            prop_assert_eq!(&third.document, &second.document);
        }
        prop_assert_eq!(quill.to_authoring(&second.document), exported);
    }
}
