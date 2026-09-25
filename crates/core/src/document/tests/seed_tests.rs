use serde_json::json;

use crate::document::tests::parse;
use crate::document::{Document, MetaKey, PayloadItem};

#[test]
fn seed_round_trips_through_markdown_and_storage() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    # pin the squadron office symbol
    from: 49 FW/CC
    signature_block:
      - \"JANE A. DOE, Col, USAF\"
      - Commander
    $body: \"Body override.\"
title: Hi
~~~

Body.
",
    );
    let seed = doc.main().payload().seed().expect("$seed present");
    assert_eq!(seed["indorsement"]["from"], json!("49 FW/CC"));

    let emitted = doc.to_markdown();
    assert!(emitted.contains("# pin the squadron office symbol"), "{emitted}");
    assert_eq!(doc, parse(&emitted));

    let json = serde_json::to_string(&doc).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
    assert!(json.contains("\"type\":\"seed\""), "{json}");
}

#[test]
fn set_ext_and_set_seed_insert_in_canonical_order() {
    let mut doc = parse("~~~card-yaml\n$quill: q@1.0\n$kind: main\ntitle: Hi\n~~~\n");
    let mut seed = serde_json::Map::new();
    seed.insert("indorsement".into(), json!({ "from": "X" }));
    doc.main_mut().payload_mut().set_seed(seed);
    let mut ext = serde_json::Map::new();
    ext.insert("rename".into(), json!("Greeting"));
    doc.main_mut().payload_mut().set_ext(ext);

    let items = doc.main().payload().items();
    assert!(matches!(items[0], PayloadItem::Quill { .. }));
    assert!(matches!(items[1], PayloadItem::Kind { .. }));
    assert!(matches!(items[2], PayloadItem::Meta { key: MetaKey::Ext, .. }));
    assert!(matches!(items[3], PayloadItem::Meta { key: MetaKey::Seed, .. }));
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
        .and_then(crate::document::SeedOverlay::from_json)
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
    let mut card = doc.main_mut();
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

/// An overlay edit rewrites `$seed` where it stands, its line's trailer kept,
/// and removing a kind the map lacks changes nothing.
#[test]
fn seed_overlay_edits_keep_the_seed_line_trailer() {
    let mut doc = parse(
        "~~~\n$quill: q@1.0\n$kind: main # A letter.\n$seed: {note: {x: 1}} # seed note\ntitle: t\n~~~\n",
    );
    let untouched = doc.clone();
    assert_eq!(doc.main_mut().remove_seed_overlay("absent"), None);
    assert_eq!(doc, untouched);

    doc.main_mut()
        .store_seed_overlay("attachment", json!({ "y": 2 }))
        .unwrap();
    doc.main_mut().remove_seed_overlay("note").unwrap();
    let md = doc.to_markdown();
    assert!(
        md.contains("$kind: main # A letter.\n$seed: # seed note\n  attachment:\n"),
        "{md}"
    );
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
    let mut card = doc.main_mut();

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
    let overlay = crate::document::SeedOverlay::from_json(&json!({
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
fn ext_and_seed_are_stripped_from_plate_json() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  presentation:
    title: \"Should not reach the backend\"
$seed:
  indorsement:
    from: \"Should not reach the backend\"
title: Hi
~~~
",
    );
    let plate = doc.to_plate_json_gated(true, None);
    let obj = plate.as_object().expect("plate is an object");
    for key in ["$ext", "ext", "$seed", "seed"] {
        assert!(!obj.contains_key(key), "plate carries `{key}`: {plate}");
    }
    assert_eq!(obj.get("title").and_then(|v| v.as_str()), Some("Hi"));
}
