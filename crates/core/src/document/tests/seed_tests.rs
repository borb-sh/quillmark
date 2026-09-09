use serde_json::json;

use crate::document::tests::parse;
use crate::document::{Document, MetaKey, PayloadItem};

#[test]
fn seed_with_mapping_value_is_accepted() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
    signature_block:
      - \"JANE A. DOE, Col, USAF\"
      - Commander
title: Hi
~~~
",
    );
    let seed = doc.main().payload().seed().expect("$seed present");
    let ind = seed.get("indorsement").and_then(|v| v.as_object()).unwrap();
    assert_eq!(ind.get("from").and_then(|v| v.as_str()), Some("49 FW/CC"));
}

#[test]
fn seed_with_scalar_value_is_rejected() {
    let err = Document::parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed: just-a-string
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("Invalid `$seed`") && err.contains("mapping"),
        "expected $seed-must-be-mapping rejection, got: {err}",
    );
}

#[test]
fn seed_on_composable_card_is_rejected() {
    let err = Document::parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~

~~~card-yaml
$kind: indorsement
$seed:
  note:
    from: X
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("must not carry `$seed`"),
        "expected composable-$seed rejection, got: {err}",
    );
}

#[test]
fn seed_round_trips_through_markdown() {
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
title: Body
~~~

Body content.
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    let reparsed = parse(&emitted);
    assert_eq!(doc, reparsed);
    assert!(
        emitted.contains("$seed:\n  indorsement:\n    from: 49 FW/CC\n"),
        "unexpected emit:\n{emitted}",
    );
}

#[test]
fn empty_seed_emits_as_inline_braces() {
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed: {}
~~~
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("$seed: {}\n"),
        "expected `$seed: {{}}` literal in emit, got:\n{emitted}",
    );
    let reparsed = parse(&emitted);
    assert_eq!(doc, reparsed);
}

#[test]
fn comments_inside_seed_round_trip() {
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    # pin the squadron office symbol
    from: 49 FW/CC
~~~
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("# pin the squadron office symbol"),
        "nested $seed comment must survive emit:\n{emitted}",
    );
    assert_eq!(doc, parse(&emitted));

    let json = serde_json::to_string(&doc).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
}

#[test]
fn set_seed_inserts_after_ext_and_before_user_fields() {
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  a: 1
title: Hi
~~~
",
    );
    let mut seed = serde_json::Map::new();
    seed.insert("indorsement".into(), json!({ "from": "X" }));
    doc.main_mut().payload_mut().set_seed(seed);

    let items = doc.main().payload().items();
    // Canonical order: $quill, $kind, $ext, $seed, then user fields.
    assert!(matches!(items[0], PayloadItem::Quill { .. }));
    assert!(matches!(items[1], PayloadItem::Kind { .. }));
    assert!(matches!(
        items[2],
        PayloadItem::Meta {
            key: MetaKey::Ext,
            ..
        }
    ));
    assert!(matches!(
        items[3],
        PayloadItem::Meta {
            key: MetaKey::Seed,
            ..
        }
    ));
    assert!(matches!(items[4], PayloadItem::Field { .. }));
}

#[test]
fn seed_overlay_parses_with_body() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
    $body: \"Standard endorsement text.\"
~~~
",
    );
    let seed = doc.main().seed();
    let overlay = seed
        .and_then(|m| m.get("indorsement"))
        .and_then(crate::SeedOverlay::from_json)
        .expect("overlay present");
    assert_eq!(
        overlay.fields.get("from").and_then(|v| v.as_str()),
        Some("49 FW/CC"),
    );
    assert_eq!(overlay.body.as_deref(), Some("Standard endorsement text."));
    assert!(!overlay.fields.contains_key("$body"));
    assert!(seed.and_then(|m| m.get("missing")).is_none());
}

#[test]
fn seed_namespace_mutators_preserve_siblings() {
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~
",
    );
    let card = doc.main_mut();
    card.store_seed_overlay("indorsement", json!({ "from": "A" }))
        .unwrap();
    card.store_seed_overlay("attachment", json!({ "label": "B" }))
        .unwrap();
    assert_eq!(card.seed().map(|m| m.len()), Some(2));

    let removed = card.remove_seed_overlay("indorsement").unwrap();
    assert_eq!(removed.get("from").and_then(|v| v.as_str()), Some("A"));
    assert_eq!(card.seed().map(|m| m.len()), Some(1));
    assert!(card.seed().unwrap().contains_key("attachment"));

    card.remove_seed_overlay("attachment");
    assert!(card.seed().is_none());
}

#[test]
fn empty_seed_overlay_round_trips() {
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~
",
    );
    doc.main_mut()
        .store_seed_overlay("indorsement", json!({}))
        .unwrap();

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("$seed:\n  indorsement: {}\n"),
        "empty overlay must keep its key, got:\n{emitted}",
    );
    assert_eq!(doc, parse(&emitted));
}

#[test]
fn store_seed_overlay_rejects_invalid_and_reserved_kinds() {
    // `$seed` is keyed by composable card-kind, so the writer must reject
    // names that could never name a composable card (unlike free-form `$ext`).
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~
",
    );
    let card = doc.main_mut();

    assert!(matches!(
        card.store_seed_overlay("main", json!({ "from": "A" })),
        Err(crate::document::EditError::ReservedKind)
    ));
    assert!(matches!(
        card.store_seed_overlay("Bad-Kind", json!({ "from": "A" })),
        Err(crate::document::EditError::InvalidKindName(_))
    ));

    assert!(card.seed().is_none());
}

/// `$seed` binds the document root, so the setter refuses every card that is
/// not it — after placement as well as at it, and through either mutable door.
#[test]
fn store_seed_overlay_refuses_a_card_that_is_not_the_root() {
    use crate::document::{Card, EditError};

    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~
",
    );
    doc.push_card(Card::new("indorsement").unwrap()).unwrap();

    let refused = EditError::RootOnlyEntry {
        key: MetaKey::Seed.as_str().to_string(),
    };
    assert_eq!(
        doc.card_mut(0)
            .unwrap()
            .store_seed_overlay("indorsement", json!({ "from": "A" })),
        Err(refused.clone())
    );
    assert_eq!(
        doc.cards_mut()[0].store_seed_overlay("indorsement", json!({ "from": "A" })),
        Err(refused)
    );
    assert!(doc.card(0).unwrap().seed().is_none());

    // The root still carries the overlay, and the markdown it emits reparses.
    doc.main_mut()
        .store_seed_overlay("indorsement", json!({ "from": "A" }))
        .unwrap();
    assert_eq!(doc, parse(&doc.to_markdown()));
}

#[test]
fn seed_overlay_drops_reserved_keys_other_than_body() {
    // An overlay only ever carries user fields plus the reserved `$body`;
    // any other `$`-key must be dropped, never smuggled in as a user field.
    let overlay = crate::SeedOverlay::from_json(&json!({
        "from": "49 FW/CC",
        "$body": "Body override.",
        "$kind": "smuggled",
        "$quill": "x@1.0",
    }))
    .expect("overlay is an object");

    assert_eq!(overlay.body.as_deref(), Some("Body override."));
    assert!(overlay.fields.contains_key("from"));
    assert!(!overlay.fields.contains_key("$kind"));
    assert!(!overlay.fields.contains_key("$quill"));
    assert_eq!(
        overlay.fields.len(),
        1,
        "only the user field should survive"
    );
}

#[test]
fn seed_is_stripped_from_plate_json() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: \"Should not reach the backend\"
title: Hi
~~~
",
    );
    let plate = doc.to_plate_json_gated(true, None);
    let obj = plate.as_object().expect("plate is an object");
    assert!(
        !obj.contains_key("$seed"),
        "plate must not contain `$seed`: {plate}"
    );
    assert!(
        !obj.contains_key("seed"),
        "plate must not contain `seed`: {plate}"
    );
    assert_eq!(obj.get("title").and_then(|v| v.as_str()), Some("Hi"));
    assert!(obj.contains_key("$quill"));
    assert!(obj.contains_key("$cards"));
}

#[test]
fn seed_round_trips_through_serde_json() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
    $body: \"Body override.\"
title: Hi
~~~

Body.
",
    );
    let json = serde_json::to_string(&doc).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
    assert_eq!(doc.to_markdown(), restored.to_markdown());

    assert!(
        json.contains("\"type\":\"seed\""),
        "expected seed variant in DTO: {json}"
    );
    assert!(
        json.contains("quillmark/document@0.112.0"),
        "expected 0.112.0 schema tag: {json}",
    );
}
