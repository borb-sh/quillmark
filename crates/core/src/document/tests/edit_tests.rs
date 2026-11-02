use crate::document::edit::{EditError, is_valid_field_name};
use crate::document::{Card, Codec, Document};
use crate::value::QuillValue;
use crate::version::QuillReference;
use std::str::FromStr;

fn make_doc() -> Document {
    Document::parse(
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Hello\n~~~\n\nBody text.\n",
    )
    .unwrap()
    .document
}

fn make_doc_with_cards() -> Document {
    Document::parse(
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Hello\n~~~\n\nBody.\n\n~~~card-yaml\n$kind: note\nfoo: bar\n~~~\n\nCard body.\n\n~~~card-yaml\n$kind: summary\n~~~\n",
    )
    .unwrap()
    .document
}

fn qv(s: &str) -> QuillValue {
    QuillValue::from_json(serde_json::json!(s))
}

fn commit_richtext(
    card: &mut Card,
    name: &str,
    value: &serde_json::Value,
    inline: bool,
) -> Result<(), EditError> {
    use crate::quill::{FieldSchema, FieldType};
    let schema = FieldSchema::new(
        name.to_string(),
        FieldType::RichText { multiline: !inline },
        None,
    );
    card.commit_field(name, QuillValue::from_json(value.clone()), &schema)
}

#[test]
fn test_field_name_grammar() {
    for name in ["title", "_private", "a1b2c3", "_", "MixedCase_1"] {
        assert!(is_valid_field_name(name), "{name:?}");
    }
    for name in ["", "123abc", "my-field", "my field", "$body"] {
        assert!(!is_valid_field_name(name), "{name:?}");
    }
}

#[test]
fn test_document_store_field_rejects_invalid_names() {
    for name in ["$body", "$quill", "\u{212A}elvin", "e\u{0301}tat"] {
        let mut doc = make_doc();
        assert_eq!(
            doc.unchecked().store_field(None, name, qv("value")),
            Err(EditError::InvalidFieldName(name.to_string())),
            "expected InvalidFieldName for {:?}",
            name
        );
        doc.unchecked().store_field(None, "after", qv("w")).unwrap();
        assert_eq!(Document::parse(&doc.to_markdown()).unwrap().document, doc);
    }
}

#[test]
fn test_document_insert_and_remove_card() {
    let mut doc = make_doc_with_cards();
    doc.unchecked().insert_card(Some(0), Card::new("intro").unwrap(), None).unwrap();
    doc.unchecked().insert_card(Some(3), Card::new("footer").unwrap(), None).unwrap();
    let kinds: Vec<_> = doc.cards().iter().map(|c| c.kind()).collect();
    assert_eq!(
        kinds,
        [Some("intro"), Some("note"), Some("summary"), Some("footer")]
    );
    assert_eq!(
        doc.unchecked().insert_card(Some(5), Card::new("note").unwrap(), None),
        Err(EditError::IndexOutOfRange { index: 5, len: 4 })
    );
    assert_eq!(doc.remove_card(0).unwrap().kind(), Some("intro"));
    assert_eq!(doc.cards().len(), 3);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

/// An edit grows a document no further than the card count markdown parse
/// admits, so its markdown always parses back.
#[test]
fn test_placing_a_card_past_the_card_ceiling_is_refused() {
    use crate::error::MAX_CARD_COUNT;

    let mut doc = make_doc();
    for _ in 0..MAX_CARD_COUNT {
        doc.unchecked().insert_card(None, Card::new("note").unwrap(), None).unwrap();
    }
    assert!(Document::parse(&doc.to_markdown()).is_ok());
    let full = EditError::TooManyCards {
        max: MAX_CARD_COUNT,
    };
    assert_eq!(doc.unchecked().insert_card(None, Card::new("note").unwrap(), None), Err(full.clone()));
    assert_eq!(
        doc.unchecked().insert_card(Some(0), Card::new("note").unwrap(), None),
        Err(full.clone())
    );
    assert_eq!(full.code(), "edit::too_many_cards");
    assert_eq!(doc.cards().len(), MAX_CARD_COUNT);
}

#[test]
fn test_move_card_reorders() {
    for (from, to, want) in [
        (0, 0, ["note", "summary"]), // no-op, same index
        (1, 0, ["summary", "note"]), // last to first
        (0, 1, ["summary", "note"]), // first to last
    ] {
        let mut doc = make_doc_with_cards();
        doc.move_card(from, to).unwrap();
        assert_eq!(doc.cards()[0].kind(), Some(want[0]));
        assert_eq!(doc.cards()[1].kind(), Some(want[1]));
    }
}

#[test]
fn test_set_card_kind_renames_in_place() {
    let mut doc = make_doc_with_cards(); // note(0) with field foo=bar, summary(1)
    doc.set_card_kind(0, "annotation").unwrap();
    assert_eq!(doc.cards()[0].kind(), Some("annotation"));
    assert_eq!(
        doc.cards()[0].payload().get("foo").unwrap().as_str(),
        Some("bar")
    );
    assert_eq!(doc.cards()[1].kind(), Some("summary"));
    let reparsed = Document::parse(&doc.to_markdown()).unwrap().document;
    assert_eq!(reparsed.cards()[0].kind(), Some("annotation"));
}

#[test]
fn test_set_card_kind_rejects_invalid_kind() {
    let mut doc = make_doc_with_cards();
    for bad in ["", "Bad", "with-dash", "1leading_digit"] {
        match doc.set_card_kind(0, bad) {
            Err(EditError::InvalidKindName(t)) => assert_eq!(t, bad),
            other => panic!("expected InvalidKindName for {bad:?}, got {other:?}"),
        }
    }
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

#[test]
fn test_document_new_blank_canvas() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    assert_eq!(doc.quill_reference().to_string(), "test_quill");
    assert!(doc.cards().is_empty());
    assert_eq!(doc.main().body_markdown(), "");

    doc.main_card_mut().store_fields([("title", "Hello")]).unwrap();
    let mut card = Card::new("note").unwrap();
    card.store_field("qty", 3).unwrap();
    doc.unchecked().insert_card(None, card, None).unwrap();

    let reparsed = Document::parse(&doc.to_markdown()).unwrap().document;
    assert_eq!(doc, reparsed);
}

#[test]
fn test_card_store_fields_inserts_in_iterator_order() {
    let mut card = Card::new("note").unwrap();
    card.store_fields([("b".to_string(), qv("two")), ("a".to_string(), qv("one"))])
        .unwrap();
    let keys: Vec<&String> = card.payload().iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["b", "a"]);
    assert_eq!(card.payload().get("a").unwrap().as_str(), Some("one"));
}

#[test]
fn test_card_store_fields_collects_every_violation_and_applies_none() {
    let mut card = Card::new("note").unwrap();
    card.store_field("existing", qv("old")).unwrap();
    let errors = card
        .store_fields([
            ("existing".to_string(), qv("new")),
            ("bad-name".to_string(), qv("v")),
            ("also bad".to_string(), qv("v")),
        ])
        .unwrap_err();
    let names: Vec<_> = errors.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["bad-name", "also bad"]);
    assert!(
        errors
            .iter()
            .all(|(n, e)| *e == EditError::InvalidFieldName(n.clone()))
    );
    assert_eq!(
        card.payload().get("existing").unwrap().as_str(),
        Some("old")
    );
}

#[test]
fn test_card_store_fields_repeated_name_last_wins() {
    let mut card = Card::new("note").unwrap();
    card.store_field("title", qv("draft")).unwrap();
    card.store_fields([
        ("title".to_string(), qv("first")),
        ("title".to_string(), qv("final")),
    ])
    .unwrap();
    assert_eq!(card.payload().get("title").unwrap().as_str(), Some("final"));
}

#[test]
fn test_store_field_scalar_conversions() {
    let mut card = Card::new("note").unwrap();
    card.store_field("name", "Alice").unwrap();
    card.store_field("qty", 3).unwrap();
    card.store_field("price", 2.5).unwrap();
    card.store_field("active", true).unwrap();
    card.store_field("tags", serde_json::json!(["a", "b"]))
        .unwrap();
    card.store_fields([("count", 1), ("total", 2)]).unwrap();
    assert_eq!(card.payload().get("name").unwrap().as_str(), Some("Alice"));
    assert_eq!(card.payload().get("qty").unwrap().as_i64(), Some(3));
    assert_eq!(card.payload().get("price").unwrap().as_f64(), Some(2.5));
    assert_eq!(card.payload().get("active").unwrap().as_bool(), Some(true));
    assert_eq!(card.payload().get("total").unwrap().as_i64(), Some(2));
}

#[test]
fn test_card_remove_field_existing() {
    let mut doc = make_doc_with_cards();
    let mut card = doc.card_mut(0).unwrap();
    let removed = card.remove_field("foo").unwrap();
    assert_eq!(removed.unwrap().as_str(), Some("bar"));
    assert!(card.payload().get("foo").is_none());
}

#[test]
fn test_card_remove_field_invalid_name_throws() {
    let mut card = Card::new("note").unwrap();
    match card.remove_field("Bad-Name") {
        Err(EditError::InvalidFieldName(name)) => assert_eq!(name, "Bad-Name"),
        other => panic!("expected InvalidFieldName, got {other:?}"),
    }
}

#[test]
fn test_replace_body_reports_import_error() {
    let mut card = Card::new("note").unwrap();
    let deep = ">".repeat(crate::error::MAX_NESTING_DEPTH + 5);
    match card.revise_body(&deep) {
        Err(EditError::Import(_)) => {}
        other => panic!("expected Import, got {other:?}"),
    }
}

#[test]
fn test_overwrite_body_sets_directly() {
    use quillmark_content::lines::model::{Mark, MarkKind};

    let mut content = quillmark_content::lines::import::from_markdown("underlined body")
        .unwrap()
        .content
        .into_content();
    content.marks.push(Mark::new(0, 10, MarkKind::Underline));
    let content = content.into_lines();

    let mut card = Card::new("note").unwrap();
    card.overwrite_body(content.clone());
    assert_eq!(card.body_lines(), &content);
}

#[test]
fn test_overwrite_field_sets_directly() {
    use quillmark_content::lines::model::{Mark, MarkKind};

    let mut content = quillmark_content::lines::import::from_markdown("underlined intro")
        .unwrap()
        .content
        .into_content();
    content.marks.push(Mark::new(0, 10, MarkKind::Underline));
    let content = content.into_lines();

    let mut card = Card::new("note").unwrap();
    card.overwrite_field("intro", content.clone()).unwrap();
    let read = card
        .field_content("intro", Codec::Richtext)
        .unwrap()
        .unwrap();
    assert_eq!(read, content);

    assert_eq!(
        card.overwrite_field("$bad", quillmark_content::lines::model::Lines::empty())
            .unwrap_err()
            .code(),
        "edit::invalid_field_name"
    );
}

#[test]
fn test_revise_field_diff_imports_and_returns_its_change() {
    use quillmark_content::lines::model::{Mark, MarkKind};

    let mut card = Card::new("note").unwrap();
    let revised = card.revise_field("intro", "hello target world").unwrap();
    assert!(!revised.applied.steps.is_empty());

    let mut base = card
        .field_content("intro", Codec::Richtext)
        .unwrap()
        .unwrap()
        .into_content();
    // 6..12 is "target".
    base.marks
        .push(Mark::new(6, 12, MarkKind::Anchor { id: "c1".into() }));
    let base = base.into_lines();
    card.overwrite_field("intro", base).unwrap();
    let _ = card
        .revise_field("intro", "why keep the target here")
        .unwrap();
    let read = card
        .field_content("intro", Codec::Richtext)
        .unwrap()
        .unwrap();
    assert!(
        read.marks
            .iter()
            .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
    );

    card.store_field(
        "count",
        crate::value::QuillValue::from_json(serde_json::json!(3)),
    )
    .unwrap();
    assert_eq!(
        card.revise_field("count", "x").unwrap_err().code(),
        "edit::field_decode"
    );
}

#[test]
fn test_revise_field_checked_preserves_anchors_and_enforces_single_line() {
    use crate::quill::{FieldSchema, FieldType};
    use quillmark_content::lines::model::{Mark, MarkKind};

    let inline = FieldSchema::new(
        "subject".to_string(),
        FieldType::RichText { multiline: false },
        None,
    );

    let mut card = Card::new("note").unwrap();
    let revised = card
        .revise_field_checked("subject", "hello target world", &inline)
        .unwrap();
    assert!(!revised.applied.steps.is_empty());

    let mut base = card
        .field_content("subject", Codec::Richtext)
        .unwrap()
        .unwrap()
        .into_content();
    // 6..12 is "target".
    base.marks
        .push(Mark::new(6, 12, MarkKind::Anchor { id: "c1".into() }));
    let base = base.into_lines();
    card.overwrite_field("subject", base).unwrap();
    let _ = card
        .revise_field_checked("subject", "why keep the target here", &inline)
        .unwrap();
    let read = card
        .field_content("subject", Codec::Richtext)
        .unwrap()
        .unwrap();
    assert!(
        read.marks
            .iter()
            .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")),
        "anchor should rebase onto surviving text, unlike the cold commit_field"
    );

    let before = card
        .field_text("subject", Codec::Richtext)
        .unwrap()
        .unwrap();
    let err = card
        .revise_field_checked("subject", "line one\n\nline two", &inline)
        .unwrap_err();
    assert_eq!(err.code(), "edit::field_not_single_line");
    assert_eq!(
        card.field_text("subject", Codec::Richtext)
            .unwrap()
            .unwrap(),
        before
    );

    let block = FieldSchema::new(
        "body".to_string(),
        FieldType::RichText { multiline: true },
        None,
    );
    let d = card
        .revise_field_checked("body", "para one\n\npara two", &block)
        .unwrap();
    assert!(!d.applied.steps.is_empty());
    assert!(
        card.field_text("body", Codec::Richtext)
            .unwrap()
            .unwrap()
            .contains("para two")
    );
}

#[test]
fn test_commit_field_richtext_content_object_reads_back() {
    use quillmark_content::lines::model::{Mark, MarkKind};

    let mut content = quillmark_content::lines::import::from_markdown("underlined intro")
        .unwrap()
        .content
        .into_content();
    content.marks.push(Mark::new(0, 10, MarkKind::Underline));
    let content = content.into_lines();
    let json = quillmark_content::lines::serial::to_canonical_value(&content);

    let mut card = Card::new("note").unwrap();
    commit_richtext(&mut card, "intro", &json, false).unwrap();

    assert!(card.payload().get("intro").unwrap().as_json().is_object());
    let read = card
        .field_content("intro", Codec::Richtext)
        .unwrap()
        .unwrap();
    assert_eq!(read, content);
}

#[test]
fn test_commit_field_richtext_markdown_null_and_rejects_bad() {
    let mut card = Card::new("note").unwrap();

    commit_richtext(
        &mut card,
        "intro",
        &serde_json::json!("**bold** intro"),
        false,
    )
    .unwrap();
    assert_eq!(
        card.field_text("intro", Codec::Richtext).unwrap().unwrap(),
        "**bold** intro"
    );

    commit_richtext(&mut card, "intro", &serde_json::Value::Null, false).unwrap();
    assert!(card.payload().get("intro").unwrap().as_json().is_null());
    assert!(
        card.field_content("intro", Codec::Richtext)
            .unwrap()
            .unwrap()
            .is_blank()
    );

    assert_eq!(
        commit_richtext(
            &mut card,
            "intro",
            &serde_json::json!({ "not": "a content" }),
            false
        )
        .unwrap_err()
        .code(),
        "edit::field_decode"
    );
    assert_eq!(
        commit_richtext(&mut card, "intro", &serde_json::json!(42), false)
            .unwrap_err()
            .code(),
        "edit::field_decode"
    );
}

#[test]
fn test_commit_field_richtext_single_line_enforced_at_write() {
    let mut card = Card::new("note").unwrap();

    commit_richtext(
        &mut card,
        "title",
        &serde_json::json!("A single line"),
        true,
    )
    .unwrap();
    assert_eq!(
        card.field_text("title", Codec::Richtext).unwrap().unwrap(),
        "A single line"
    );

    let err = commit_richtext(
        &mut card,
        "title",
        &serde_json::json!("line one\n\nline two"),
        true,
    )
    .unwrap_err();
    assert_eq!(err.code(), "edit::field_not_single_line");
    assert_eq!(
        card.field_text("title", Codec::Richtext).unwrap().unwrap(),
        "A single line"
    );
}

#[test]
fn test_commit_field_array_of_single_line_richtext_reports_not_single_line() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let mut schema = FieldSchema::new("refs".to_string(), FieldType::Array, None);
    schema.items = Some(Box::new(FieldSchema::new(
        "refs".to_string(),
        FieldType::RichText { multiline: false },
        None,
    )));

    card.commit_field(
        "refs",
        QuillValue::from_json(serde_json::json!(["one line"])),
        &schema,
    )
    .unwrap();

    let err = card
        .commit_field(
            "refs",
            QuillValue::from_json(serde_json::json!(["line one\n\nline two"])),
            &schema,
        )
        .unwrap_err();
    assert_eq!(err.code(), "edit::field_not_single_line");
}

#[test]
fn test_commit_field_scalar_strict() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let int_schema = FieldSchema::new("qty".to_string(), FieldType::Integer, None);

    card.commit_field(
        "qty",
        QuillValue::from_json(serde_json::json!("3")),
        &int_schema,
    )
    .unwrap();
    assert_eq!(
        card.payload().get("qty").unwrap().as_json(),
        &serde_json::json!(3)
    );

    let err = card
        .commit_field(
            "qty",
            QuillValue::from_json(serde_json::json!(true)),
            &int_schema,
        )
        .unwrap_err();
    assert_eq!(err.code(), "edit::field_coercion_failed");

    assert_eq!(
        card.commit_field(
            "qty",
            QuillValue::from_json(serde_json::json!("x")),
            &int_schema
        )
        .unwrap_err()
        .code(),
        "edit::field_coercion_failed"
    );
    assert_eq!(
        card.payload().get("qty").unwrap().as_json(),
        &serde_json::json!(3)
    );

    let object = FieldSchema::new("meta".to_string(), FieldType::Object, None);
    assert_eq!(
        card.commit_field(
            "meta",
            QuillValue::from_json(serde_json::json!(42)),
            &object
        )
        .unwrap_err()
        .code(),
        "edit::field_coercion_failed"
    );

    let schema = FieldSchema::new("$bad".to_string(), FieldType::Integer, None);
    assert_eq!(
        card.commit_field("$bad", QuillValue::from_json(serde_json::json!(1)), &schema)
            .unwrap_err()
            .code(),
        "edit::invalid_field_name"
    );
}

#[test]
fn test_field_content_absent_and_non_content() {
    let mut card = Card::new("note").unwrap();
    assert!(card.field_content("missing", Codec::Richtext).is_none());
    assert!(card.field_text("missing", Codec::Richtext).is_none());

    card.store_field("count", 3).unwrap();
    assert!(
        card.field_content("count", Codec::Richtext)
            .unwrap()
            .is_err()
    );
    assert!(card.field_text("count", Codec::Richtext).unwrap().is_err());
}

#[test]
fn test_revise_body_returns_its_change_and_updates_body() {
    use quillmark_content::change::Assoc;
    use quillmark_content::mapping::Mapping;
    use quillmark_content::model::Pos;

    let mut card = Card::new("note").unwrap();
    let _ = card.revise_body("hello world").unwrap();
    let applied = card.revise_body("hello brave world").unwrap().applied;
    assert_eq!(card.body_lines().text, "hello brave world");
    let mapping = Mapping::from_change(&applied).unwrap();
    assert_eq!(mapping.map(&Pos::at(6), Assoc::Before).pos, Pos::at(6));
    assert_eq!(mapping.map(&Pos::at(11), Assoc::After).pos, Pos::at(17));
}

#[test]
fn test_revise_body_rebases_anchor() {
    use quillmark_content::lines::model::{Mark, MarkKind};

    let mut base = quillmark_content::lines::import::from_markdown("keep the target word")
        .unwrap()
        .content
        .into_content();
    // Anchor over "target" (chars 9..15).
    base.marks
        .push(Mark::new(9, 15, MarkKind::Anchor { id: "c1".into() }));
    let base = base.into_lines();
    let mut card = Card::new("note").unwrap();
    card.overwrite_body(base);

    let _ = card.revise_body("why keep the target word").unwrap();
    let anchor = card
        .body_lines()
        .marks
        .iter()
        .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
        .expect("identity anchor survives the whole-document replace");
    let text = &card.body_lines().text;
    let s = quillmark_content::usv::char_to_byte(text, anchor.start);
    let e = quillmark_content::usv::char_to_byte(text, anchor.end);
    assert_eq!(&text[s..e], "target");
}

#[test]
fn test_remove_ext_returns_previous_and_clears() {
    let mut doc = make_doc();
    let mut ext = serde_json::Map::new();
    ext.insert("agent".to_string(), serde_json::json!(1));
    doc.main_mut().store_ext(ext).expect("set_ext");

    let removed = doc.main_mut().remove_ext().unwrap();
    assert_eq!(removed["agent"].as_i64(), Some(1));
    assert!(doc.main().ext().is_none());
    assert!(doc.main_mut().remove_ext().is_none());
}

/// Nests `{"a":…}` `depth` levels iteratively, so the test itself stays stack-safe.
fn deep_value(depth: usize) -> serde_json::Value {
    let mut v = serde_json::json!(1);
    for _ in 0..depth {
        let mut m = serde_json::Map::new();
        m.insert("a".to_string(), v);
        v = serde_json::Value::Object(m);
    }
    v
}

#[test]
fn store_field_rejects_value_past_depth_limit() {
    let mut doc = crate::document::Document::parse("~~~\n$quill: q@1.0\n$kind: main\n~~~\n")
        .unwrap()
        .document;
    let ok = crate::value::QuillValue::from_json(deep_value(50));
    assert!(doc.unchecked().store_field(None, "x", ok).is_ok());

    let too_deep = crate::value::QuillValue::from_json(deep_value(150));
    let err = doc.unchecked().store_field(None, "y", too_deep).unwrap_err();
    assert!(
        matches!(err, crate::document::EditError::ValueTooDeep { max: 128 }),
        "expected ValueTooDeep, got {err:?}"
    );
    let serde_json::Value::Object(map) = deep_value(150) else {
        unreachable!()
    };
    assert!(doc.main_mut().store_ext(map).is_err());
}

/// The `$ext` map is itself a level, so its values carry `MAX_JSON_DEPTH - 1`.
#[test]
fn store_ext_charges_the_map_its_own_level() {
    let mut doc = crate::document::Document::parse("~~~\n$quill: q@1.0\n$kind: main\n~~~\n")
        .unwrap()
        .document;
    let map = |depth: usize| {
        let mut m = serde_json::Map::new();
        m.insert("a".to_string(), deep_value(depth));
        m
    };

    doc.main_mut()
        .store_ext(map(127))
        .expect("127 levels under a map is exactly the limit");
    let err = doc.main_mut().store_ext(map(128)).unwrap_err();
    assert!(
        matches!(err, crate::document::EditError::ValueTooDeep { max: 128 }),
        "expected ValueTooDeep, got {err:?}"
    );
    assert_eq!(err.code(), "edit::value_too_deep");
    assert_eq!(
        doc.main().ext().and_then(|m| m.get("a")),
        Some(&deep_value(127)),
        "the refused map leaves the stored one untouched"
    );
}

/// The namespace merge behind `$seed` charges the map the same level the
/// wholesale `$ext` store does, so one overlay is bounded at
/// `MAX_JSON_DEPTH - 1` however many kinds sit beside it.
#[test]
fn store_seed_overlay_charges_the_map_its_own_level() {
    let mut doc = crate::document::Document::parse("~~~\n$quill: q@1.0\n$kind: main\n~~~\n")
        .unwrap()
        .document;

    doc.main_mut()
        .store_seed_overlay("note", deep_value(127))
        .expect("127 levels under the map is exactly the limit");
    let err = doc
        .main_mut()
        .store_seed_overlay("memo", deep_value(128))
        .unwrap_err();
    assert!(
        matches!(err, crate::document::EditError::ValueTooDeep { max: 128 }),
        "expected ValueTooDeep, got {err:?}"
    );
    assert_eq!(err.code(), "edit::value_too_deep");

    let seed = doc
        .main()
        .seed()
        .expect("the accepted overlay is still there");
    assert_eq!(seed.get("note"), Some(&deep_value(127)));
    assert!(
        !seed.contains_key("memo"),
        "the refused overlay is not written"
    );
}

#[test]
fn wire_card_rejects_value_past_depth_limit_and_bad_names() {
    let wire: crate::document::CardWire = serde_json::from_value(serde_json::json!({
        "kind": "note",
        "payloadItems": [
            {"type": "field", "key": "x", "value": deep_value(150)}
        ],
        "body": ""
    }))
    .unwrap();
    let err = crate::document::Card::try_from(wire).unwrap_err();
    assert_eq!(err.code(), "edit::value_too_deep");

    let wire: crate::document::CardWire = serde_json::from_value(serde_json::json!({
        "kind": "note",
        "payloadItems": [
            {"type": "field", "key": "Bad Name", "value": 1}
        ],
        "body": ""
    }))
    .unwrap();
    let err = crate::document::Card::try_from(wire).unwrap_err();
    assert_eq!(err.code(), "edit::invalid_field_name");
}

/// `$quill` and `$seed` bind the document root. Placing a card that carries
/// either is refused, so the emitted markdown stays parseable — the invariant
/// `from_main_and_cards` asserts, enforced where a card is placed.
#[test]
fn test_insert_card_refuses_root_only_entries() {
    use crate::document::CardWire;

    let placed = |mutate: &dyn Fn(&mut CardWire)| {
        let mut wire = CardWire::new("note".to_string(), serde_json::Value::Null);
        mutate(&mut wire);
        let card = Card::try_from(wire).expect("the wire stays permissive on placement metadata");
        let mut doc = make_doc();
        doc.unchecked().insert_card(None, card.clone(), None).map(|_| doc).map_err(|e| {
            let mut doc = make_doc();
            assert_eq!(
                doc.unchecked().insert_card(Some(0), card, None),
                Err(e.clone()),
                "push and insert agree"
            );
            e
        })
    };

    assert_eq!(
        placed(&|w| w.quill = Some("memo@1.0.0".to_string())).unwrap_err(),
        EditError::RootOnlyEntry {
            key: "$quill".to_string()
        }
    );
    assert_eq!(
        placed(&|w| w.seed = Some(serde_json::json!({"note": {}}).as_object().unwrap().clone()))
            .unwrap_err(),
        EditError::RootOnlyEntry {
            key: "$seed".to_string()
        }
    );

    // `$ext` is not root-only: it rides any card.
    let doc = placed(&|w| w.ext = Some(serde_json::json!({"ns": {}}).as_object().unwrap().clone()))
        .expect("$ext is allowed on a composable card");
    let _ = Document::parse(&doc.to_markdown()).expect("emit reparses");

    // The same wire is how the main card is read back, so refusing `$quill` at
    // placement must not cost that round trip.
    let main = CardWire::from(make_doc().main());
    assert!(main.quill.is_some());
    Card::try_from(main).expect("the main card still crosses the wire");
}

#[test]
fn test_wire_refuses_payload_level_violations() {
    use crate::document::edit::PayloadViolation;
    use crate::document::wire::{CardWire, PayloadItemWire, WireError};

    let violation = |err: &WireError| match err {
        WireError::Edit(EditError::InvalidPayload(v)) => {
            assert_eq!(err.code(), "edit::invalid_payload");
            v.clone()
        }
        other => panic!("{other:?}"),
    };
    let built = |items: Vec<PayloadItemWire>| {
        let mut wire = CardWire::new("note".to_string(), serde_json::Value::Null);
        wire.payload_items = items;
        Card::try_from(wire)
    };
    let field = |key: &str| PayloadItemWire::Field {
        key: key.to_string(),
        value: serde_json::json!(1),
    };

    let dup = built(vec![field("title"), field("title")]).unwrap_err();
    assert!(
        matches!(violation(&dup), PayloadViolation::DuplicateField { key } if key == "title"),
        "{dup:?}"
    );

    let too_many = built(
        (0..=crate::error::MAX_FIELD_COUNT)
            .map(|i| field(&format!("f{i}")))
            .collect(),
    )
    .unwrap_err();
    assert!(
        matches!(violation(&too_many), PayloadViolation::TooManyFields { .. }),
        "{too_many:?}"
    );

    // A comment spanning lines emits bare YAML after the first line: the one
    // violation whose markdown re-reads *cleanly*, as fields no one wrote.
    let injected = built(vec![PayloadItemWire::Comment {
        text: "hi\ninjected: pwned".to_string(),
        inline: false,
    }])
    .unwrap_err();
    assert!(
        matches!(violation(&injected), PayloadViolation::MultiLineComment),
        "{injected:?}"
    );

    built(vec![field("title"), field("subject")]).expect("distinct keys cross");
}

/// The storage DTO is a hand-craftable ingress, so it owns the invariants the
/// parser enforces on source. Each refused payload differs from a loading one
/// by one item.
#[test]
fn storage_dto_refuses_what_the_parser_refuses() {
    let load = |items: serde_json::Value| {
        serde_json::from_value::<crate::document::Document>(serde_json::json!({
            "schema": "quillmark/document@0.129.0",
            "main": {"payload": {"items": items}, "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}},
            "cards": []
        }))
    };
    let quill = serde_json::json!({"type": "quill", "value": "q@1.0"});
    let kind = |v: &str| serde_json::json!({"type": "kind", "value": v});
    let field = |key: &str, value: serde_json::Value| serde_json::json!({"type": "field", "key": key, "value": value});
    let serde_json::Value::Object(deep_map) = deep_value(150) else {
        unreachable!()
    };

    let doc = load(serde_json::json!([quill, field("x", deep_value(50))]))
        .expect("an absent root kind loads");
    assert_eq!(doc.main().kind(), Some("main"));
    assert_eq!(
        Document::parse(&doc.to_markdown())
            .expect("emit reparses")
            .document,
        doc
    );

    for (label, items) in [
        (
            "deep field",
            serde_json::json!([quill, kind("main"), field("x", deep_value(150))]),
        ),
        (
            "deep $ext",
            serde_json::json!([quill, kind("main"), {"type": "ext", "value": deep_map}]),
        ),
        (
            "bad field name",
            serde_json::json!([quill, kind("main"), field("bad name", "v".into())]),
        ),
        (
            "multi-line comment",
            serde_json::json!([quill, kind("main"), {"type": "comment", "text": "hi\ninjected: pwned", "inline": false}]),
        ),
        (
            "non-main root kind",
            serde_json::json!([quill, kind("note")]),
        ),
        (
            "repeated $quill",
            serde_json::json!([quill, kind("main"), quill]),
        ),
        (
            "repeated $kind",
            serde_json::json!([quill, kind("main"), kind("main")]),
        ),
    ] {
        assert!(load(items).is_err(), "{label} loaded");
    }
}

/// The §8 field count is the payload's own invariant, so the write doors hold
/// it where a caller holding one `(name, value)` pair cannot: a card filled to
/// the cap refuses a new field at the call, and what it already carries still
/// passes the exits that would have reported it.
#[test]
fn a_field_write_past_the_count_is_refused_at_the_write() {
    use crate::document::edit::{PayloadViolation, validate_payload};
    use quillmark_content::lines::model::Lines;

    let max = crate::error::MAX_FIELD_COUNT;
    let names: Vec<String> = (0..max).map(|i| format!("f{i}")).collect();
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    doc.main_card_mut()
        .store_fields(names.iter().map(|n| (n.as_str(), qv("v"))))
        .expect("a batch that fills the card exactly to the cap lands");
    assert_eq!(doc.main().payload().len(), max);

    doc.unchecked().store_field(None, "f0", qv("replaced"))
        .expect("a replace is not a growth");

    let mut refused = Vec::new();
    refused.push(doc.unchecked().store_field(None, "late", qv("v")).unwrap_err());
    refused.push(doc.main_mut().revise_field("late", "text").unwrap_err());
    refused.push(
        doc.main_mut()
            .overwrite_field("late", Lines::empty())
            .unwrap_err(),
    );
    refused.push(
        commit_richtext(doc.main_card_mut(), "late", &serde_json::json!("v"), false).unwrap_err(),
    );
    for err in refused {
        assert_eq!(err.code(), "edit::invalid_payload");
        assert!(
            matches!(
                err,
                EditError::InvalidPayload(PayloadViolation::TooManyFields { count, max: m })
                    if count == max + 1 && m == max
            ),
            "{err:?}"
        );
    }

    assert_eq!(doc.main().payload().len(), max);
    validate_payload(doc.main().payload()).expect("the storage exits take what the writes built");
    let _ = Document::parse(&doc.to_markdown()).expect("the markdown it emits reparses");
}

/// A batch charges the count over the batch rather than per call, so it names
/// every field past the cap and applies none of itself.
#[test]
fn a_batch_past_the_count_names_its_overflowing_tail() {
    let max = crate::error::MAX_FIELD_COUNT;
    let names: Vec<String> = (0..max - 1).map(|i| format!("f{i}")).collect();
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    doc.main_card_mut()
        .store_fields(names.iter().map(|n| (n.as_str(), qv("v"))))
        .expect("one slot short of the cap");

    let errs = doc
        .main_card_mut()
        .store_fields([
            ("f0", qv("replaced")),
            ("a", qv("v")),
            ("b", qv("v")),
            ("c", qv("v")),
        ])
        .unwrap_err();
    assert_eq!(
        errs.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"],
        "a present name is a replace, and `a` takes the last slot"
    );
    assert!(
        errs.iter()
            .all(|(_, e)| e.code() == "edit::invalid_payload")
    );

    assert_eq!(doc.main().payload().len(), max - 1);
    assert_eq!(doc.main().payload().get("f0").unwrap().as_str(), Some("v"));
}

/// [`CardMut`](crate::document::CardMut) is the whole mutable surface of a
/// placed card, and no verb on it carries a card between the root and
/// composable roles: the root keeps `$quill`, and no composable card gains
/// `$quill` or `$seed`. A `&mut Card` would let one whole-card assignment do
/// all three at once, past every gate that polices placement.
#[test]
fn no_verb_on_a_placed_card_moves_it_between_roles() {
    use crate::document::CardMut;
    use crate::quill::{FieldSchema, FieldType};
    use quillmark_content::lines::model::Lines;

    fn richtext() -> FieldSchema {
        FieldSchema::new(
            "f".to_string(),
            FieldType::RichText { multiline: true },
            None,
        )
    }
    fn ignore<T, E>(_: Result<T, E>) {}

    let verbs: [(&str, fn(&mut CardMut<'_>)); 11] = [
        ("remove_field", |c| ignore(c.remove_field("f"))),
        ("store_ext", |c| ignore(c.store_ext(serde_json::Map::new()))),
        ("remove_ext", |c| {
            c.remove_ext();
        }),
        ("store_seed_overlay", |c| {
            ignore(c.store_seed_overlay("note", serde_json::json!({ "f": "v" })))
        }),
        ("remove_seed_overlay", |c| {
            c.remove_seed_overlay("note");
        }),
        ("overwrite_body", |c| c.overwrite_body(Lines::empty())),
        ("overwrite_field", |c| {
            ignore(c.overwrite_field("f", Lines::empty()))
        }),
        ("revise_body", |c| ignore(c.revise_body("body"))),
        ("revise_field", |c| ignore(c.revise_field("f", "text"))),
        ("commit_field", |c| {
            ignore(c.commit_field("f", qv("v"), &richtext()))
        }),
        ("revise_field_checked", |c| {
            ignore(c.revise_field_checked("f", "text", &richtext()))
        }),
    ];

    for (verb_name, verb) in verbs {
        let mut doc = make_doc_with_cards();
        verb(&mut doc.main_mut());
        verb(&mut doc.card_mut(0).unwrap());
        assert!(
            doc.main().quill().is_some(),
            "{verb_name} took `$quill` off the root"
        );
        assert!(
            doc.cards()
                .iter()
                .all(|c| c.quill().is_none() && c.seed().is_none()),
            "{verb_name} put a root-only entry on a composable card"
        );
        let _ = doc.quill_reference();
    }
}

fn stored(doc: &Document) -> String {
    serde_json::to_string(&crate::document::dto::StoredDocument::from(doc.clone())).unwrap()
}

/// A content value keyed in reverse of its canonical order: equal JSON, other
/// bytes.
fn reversed_keys(v: &serde_json::Value) -> serde_json::Value {
    let map = v.as_object().unwrap();
    serde_json::Value::Object(map.iter().rev().map(|(k, v)| (k.clone(), v.clone())).collect())
}

/// A field's content holds as a body's does: stored in its canonical form,
/// so equal documents store equal bytes and the markdown export projects it,
/// its anchors dropped; and one that does not decode is refused at the
/// write, the wire and the load.
#[test]
fn a_field_write_holds_its_content_as_a_body_write_does() {
    let mut content = quillmark_content::markdown::from_markdown("Hello *world*").unwrap().content;
    content.anchors.push(quillmark_content::model::Anchor::new("a1", 0, 5));
    let canonical = content.to_value();
    let shuffled = reversed_keys(&canonical);
    assert_ne!(serde_json::to_string(&shuffled).unwrap(), serde_json::to_string(&canonical).unwrap());

    let mut a = make_doc();
    a.unchecked().store_field(None, "f", QuillValue::from_json(shuffled.clone())).unwrap();
    let mut b = make_doc();
    b.unchecked().store_field(None, "f", QuillValue::from_json(canonical.clone())).unwrap();
    assert_eq!(a, b);
    assert_eq!(stored(&a), stored(&b));
    assert!(a.to_markdown().contains("f: Hello *world*"), "{}", a.to_markdown());
    assert!(!a.to_markdown().contains("a1"));

    let mut nested = make_doc();
    nested.unchecked().store_field(None, "rows", QuillValue::from_json(serde_json::json!([{ "f": shuffled }]))).unwrap();
    assert_eq!(nested.main().payload().get("rows").unwrap().as_json(), &serde_json::json!([{ "f": canonical }]));

    let mut bad = canonical.clone();
    bad["anchors"][0]["end"] = 99.into();
    let refused = make_doc().unchecked().store_field(None, "f", QuillValue::from_json(bad.clone())).unwrap_err();
    assert_eq!(refused.code(), "edit::field_decode");
    let refused = make_doc()
        .unchecked()
        .store_field(None, "rows", QuillValue::from_json(serde_json::json!([{ "f": bad }])))
        .unwrap_err();
    assert!(matches!(&refused, EditError::FieldDecode { at, .. } if at.len() == 2), "{refused:?}");

    let mut wire = crate::document::CardWire::new("note".to_string(), serde_json::Value::Null);
    wire.payload_items.push(crate::document::PayloadItemWire::Field { key: "f".into(), value: bad.clone() });
    assert_eq!(Card::try_from(wire).unwrap_err().code(), "edit::field_decode");

    let row = stored(&b).replace(&serde_json::to_string(&canonical).unwrap(), &serde_json::to_string(&bad).unwrap());
    assert!(Document::from_stored(&row).is_err(), "a load refuses a field it would refuse as a body");
}

/// A card placed with no `$id` mints one positionally, or under the writer's
/// actor where one is given, so two writers placing a card into one document
/// mint two ids.
#[test]
fn a_placed_card_mints_under_its_writers_actor() {
    let place = |actor: Option<&str>| {
        let mut doc = make_doc();
        let id = doc.unchecked().insert_card(None, Card::new("note").unwrap(), actor).unwrap();
        assert_eq!(doc.cards()[0].id(), Some(id.as_str()));
        id
    };
    let (none, seven, eight) = (place(None), place(Some("7")), place(Some("8")));
    assert_eq!(none, place(None));
    assert_eq!(seven, format!("7-{none}"));
    assert_ne!(seven, eight);

    let mut doc = make_doc();
    assert_eq!(
        doc.unchecked().insert_card(None, Card::new("note").unwrap(), Some("a:b")),
        Err(EditError::InvalidCardActor("a:b".into()))
    );
    let mut spelled = Card::new("note").unwrap();
    spelled.payload_mut().set_id("kept");
    assert_eq!(doc.unchecked().insert_card(None, spelled, Some("7")).unwrap(), "kept");
}
