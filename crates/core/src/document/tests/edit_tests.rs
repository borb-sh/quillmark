
use crate::document::edit::{is_valid_field_name, EditError};
use crate::document::meta::is_valid_kind_name;
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
    let schema = FieldSchema::new(name.to_string(), FieldType::RichText { inline }, None);
    card.commit_field(name, QuillValue::from_json(value.clone()), &schema)
}

fn qv_int(n: i64) -> QuillValue {
    QuillValue::from_json(serde_json::json!(n))
}

#[test]
fn test_valid_field_names() {
    assert!(is_valid_field_name("title"));
    assert!(is_valid_field_name("my_field"));
    assert!(is_valid_field_name("_private"));
    assert!(is_valid_field_name("abc123"));
    assert!(is_valid_field_name("a1b2c3"));
    assert!(is_valid_field_name("x"));
    assert!(is_valid_field_name("_"));
    assert!(is_valid_field_name("Title"));
    assert!(is_valid_field_name("BODY"));
    assert!(is_valid_field_name("MixedCase_1"));
}

#[test]
fn test_invalid_field_names() {
    assert!(!is_valid_field_name(""));
    assert!(!is_valid_field_name("123abc")); // starts with digit
    assert!(!is_valid_field_name("my-field")); // hyphen not allowed
    assert!(!is_valid_field_name("my field")); // space not allowed
    assert!(!is_valid_field_name("$body")); // $-prefix reserved for metadata
}

#[test]
fn test_document_store_field_rejects_dollar_prefixed_names() {
    for name in ["$body", "$cards", "$quill", "$kind"] {
        let mut doc = make_doc();
        let result = doc.main_mut().store_field(name, qv("value"));
        assert_eq!(
            result,
            Err(EditError::InvalidFieldName(name.to_string())),
            "expected InvalidFieldName for '{}'",
            name
        );
    }
}

#[test]
fn test_document_store_field_rejects_non_ascii_names() {
    for name in ["\u{212A}elvin", "e\u{0301}tat"] {
        let mut doc = make_doc();
        assert_eq!(
            doc.main_mut().store_field(name, qv("value")),
            Err(EditError::InvalidFieldName(name.to_string())),
            "expected InvalidFieldName for {:?}",
            name
        );
        doc.main_mut().store_field("after", qv("w")).unwrap();
        assert_eq!(Document::parse(&doc.to_markdown()).unwrap().document, doc);
    }
}

#[test]
fn test_document_store_field_updates_existing() {
    let mut doc = make_doc();
    doc.main_mut().store_field("title", qv("New Title")).unwrap();
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str(),
        Some("New Title")
    );
}

#[test]
fn test_document_insert_card_at_zero() {
    let mut doc = make_doc_with_cards(); // 2 cards: note, summary
    let card = Card::new("intro").unwrap();
    doc.insert_card(0, card).unwrap();
    assert_eq!(doc.cards().len(), 3);
    assert_eq!(doc.cards()[0].kind(), Some("intro"));
    assert_eq!(doc.cards()[1].kind(), Some("note"));
}

#[test]
fn test_document_insert_card_at_end() {
    let mut doc = make_doc_with_cards(); // 2 cards
    let len = doc.cards().len();
    let card = Card::new("footer").unwrap();
    doc.insert_card(len, card).unwrap();
    assert_eq!(doc.cards()[len].kind(), Some("footer"));
}

#[test]
fn test_document_insert_card_out_of_range() {
    let mut doc = make_doc(); // 0 cards
    let card = Card::new("note").unwrap();
    let result = doc.insert_card(1, card);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: 1, len: 0 }));
}

#[test]
fn test_document_remove_card() {
    let mut doc = make_doc_with_cards(); // 2 cards: note, summary
    let removed = doc.remove_card(0);
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().kind(), Some("note"));
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("summary"));
}

#[test]
fn test_document_card_mut() {
    let mut doc = make_doc_with_cards();
    {
        let card = doc.card_mut(0).unwrap();
        card.revise_body("Updated card body.").unwrap();
    }
    assert_eq!(doc.cards()[0].body_markdown(), "Updated card body.");
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
fn test_set_card_kind_round_trips_via_markdown() {
    let mut doc = make_doc_with_cards();
    doc.set_card_kind(0, "annotation").unwrap();
    let md = doc.to_markdown();
    let reparsed = crate::Document::parse(&md).unwrap().document;
    assert_eq!(reparsed.cards()[0].kind(), Some("annotation"));
}

#[test]
fn test_document_new_blank_canvas() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    assert_eq!(doc.quill_reference().to_string(), "test_quill");
    assert!(doc.cards().is_empty());
    assert_eq!(doc.main().body_markdown(), "");

    doc.main_mut().store_fields([("title", "Hello")]).unwrap();
    let mut card = Card::new("note").unwrap();
    card.store_field("qty", 3).unwrap();
    doc.push_card(card).unwrap();

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
fn test_card_store_fields_collects_every_violation() {
    let mut card = Card::new("note").unwrap();
    let errors = card
        .store_fields([
            ("ok".to_string(), qv("fine")),
            ("bad-name".to_string(), qv("v")),
            ("also bad".to_string(), qv("v")),
        ])
        .unwrap_err();
    assert_eq!(errors.len(), 2);
    assert_eq!(
        errors[0],
        (
            "bad-name".to_string(),
            EditError::InvalidFieldName("bad-name".to_string())
        )
    );
    assert_eq!(
        errors[1],
        (
            "also bad".to_string(),
            EditError::InvalidFieldName("also bad".to_string())
        )
    );
}

#[test]
fn test_card_store_fields_atomic_on_error() {
    let mut card = Card::new("note").unwrap();
    card.store_field("existing", qv("old")).unwrap();
    let result = card.store_fields([
        ("existing".to_string(), qv("new")),
        ("bad-name".to_string(), qv("v")),
    ]);
    assert!(result.is_err());
    assert_eq!(
        card.payload().get("existing").unwrap().as_str(),
        Some("old")
    );
    assert!(card.payload().get("bad-name").is_none());
}

/// The emitted `x: a: 1` does not re-parse, so the mutator refuses what parse
/// refuses in source. A canonical content object stays storable.
#[test]
fn test_store_fill_refuses_a_mapping() {
    let mut card = Card::new("note").unwrap();
    let err = card
        .store_fill("x", QuillValue::from_json(serde_json::json!({"a": 1})))
        .expect_err("a fill-marked mapping is refused");
    assert_eq!(err.code(), "edit::fill_on_mapping", "{err}");
    assert!(card.payload().get("x").is_none(), "and nothing was stored");

    // A canonical content object is the one mapping a marker may target: emit
    // projects it to a markdown scalar first.
    let content = quillmark_content::import::from_markdown("Q3 results").unwrap();
    card.store_fill(
        "subject",
        QuillValue::from_json(quillmark_content::serial::to_canonical_value(&content)),
    )
    .expect("a fill-marked content object is stored");

    let md = crate::document::Document::from_main_and_cards(
        {
            let mut main = card.clone();
            main.payload_mut()
                .set_quill("q@1.0.0".parse().expect("reference"));
            main.payload_mut().set_kind("main");
            main
        },
        Vec::new(),
    )
    .to_markdown();
    let _ = crate::document::Document::parse(&md).expect("the emitted document re-parses");
}

/// The batch carries the same fill-target rule as the single write: a marker on
/// a nested mapping emits as an unmarked key and reloads as a DTO violation.
#[test]
fn test_card_store_fields_refuses_a_nested_fill_on_a_mapping() {
    let marked = || {
        let mut value = QuillValue::from_json(serde_json::json!({"a": {"b": 1}}));
        assert!(value.set_fill_at(&[crate::value::PathSegment::Key("a".to_string())]));
        value
    };
    let mut card = Card::new("note").unwrap();
    card.store_field("keep", qv("old")).unwrap();

    let single = card
        .store_field("x", marked())
        .expect_err("a marker on a nested mapping is refused");
    let batch = card
        .store_fields([("keep".to_string(), qv("new")), ("x".to_string(), marked())])
        .expect_err("and the batch refuses it identically");

    assert_eq!(batch, vec![("x".to_string(), single)]);
    assert!(card.payload().get("x").is_none());
    assert_eq!(card.payload().get("keep").unwrap().as_str(), Some("old"));
}

#[test]
fn test_card_store_fields_clears_fill_and_repeated_name_last_wins() {
    let mut card = Card::new("note").unwrap();
    card.store_fill("title", qv("draft")).unwrap();
    card.store_fields([
        ("title".to_string(), qv("first")),
        ("title".to_string(), qv("final")),
    ])
    .unwrap();
    let value = card.payload().get("title").unwrap();
    assert!(!value.fill());
    assert_eq!(value.as_str(), Some("final"));
}

/// The payload item's flag is the sole carrier of a root `!must_fill`, so a
/// value arriving with its own root bit set is stored under the marker the
/// mutator names: `Payload::is_fill` and [`QuillValue::fill`] agree afterwards.
#[test]
fn test_store_clears_a_root_fill_bit_on_the_incoming_value() {
    let mut marked = qv("draft");
    assert!(marked.set_fill_at(&[]));

    let mut card = Card::new("note").unwrap();
    card.store_fields([("x".to_string(), marked.clone())])
        .unwrap();
    card.store_field("y", marked.clone()).unwrap();
    card.store_fill("z", marked).unwrap();

    for key in ["x", "y"] {
        assert!(!card.payload().is_fill(key), "{key} carries no marker");
        assert!(!card.payload().get(key).unwrap().fill(), "{key} value");
    }
    assert!(card.payload().is_fill("z"));
    assert!(!card.payload().get("z").unwrap().fill());
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
    let card = doc.card_mut(0).unwrap();
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
    use quillmark_content::model::{Mark, MarkKind};

    let mut content = quillmark_content::import::from_markdown("underlined body").unwrap().into_content();
    content.marks.push(Mark::new(0, 10, MarkKind::Underline));
    let content = content.into_normalized();

    let mut card = Card::new("note").unwrap();
    card.overwrite_body(content.clone());
    assert_eq!(card.body(), &content);
    assert!(card
        .body()
        .marks
        .iter()
        .any(|m| matches!(m.kind, MarkKind::Underline)));
}

#[test]
fn test_overwrite_field_sets_directly() {
    use quillmark_content::model::{Mark, MarkKind};

    let mut content = quillmark_content::import::from_markdown("underlined intro").unwrap().into_content();
    content.marks.push(Mark::new(0, 10, MarkKind::Underline));
    let content = content.into_normalized();

    let mut card = Card::new("note").unwrap();
    card.overwrite_field("intro", content.clone()).unwrap();
    let read = card.field_content("intro", Codec::Richtext).unwrap().unwrap();
    assert_eq!(read, content);
    assert!(read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)));

    assert_eq!(
        card.overwrite_field("$bad", quillmark_content::Normalized::empty())
            .unwrap_err()
            .code(),
        "edit::invalid_field_name"
    );
}

#[test]
fn test_revise_field_diff_imports_and_returns_delta() {
    use quillmark_content::model::{Mark, MarkKind};

    let mut card = Card::new("note").unwrap();
    let delta = card.revise_field("intro", "hello target world").unwrap();
    assert!(!delta.ops.is_empty());

    let mut base = card
        .field_content("intro", Codec::Richtext)
        .unwrap()
        .unwrap()
        .into_content();
    // 6..12 is "target".
    base.marks
        .push(Mark::new(6, 12, MarkKind::Anchor { id: "c1".into() }));
    let base = base.into_normalized();
    card.overwrite_field("intro", base).unwrap();
    card.revise_field("intro", "why keep the target here").unwrap();
    let read = card.field_content("intro", Codec::Richtext).unwrap().unwrap();
    assert!(read
        .marks
        .iter()
        .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")));

    card.store_field("count", crate::QuillValue::from_json(serde_json::json!(3)))
        .unwrap();
    assert_eq!(
        card.revise_field("count", "x").unwrap_err().code(),
        "edit::field_decode"
    );
}

#[test]
fn test_revise_field_checked_preserves_anchors_and_enforces_inline() {
    use crate::quill::{FieldSchema, FieldType};
    use quillmark_content::model::{Mark, MarkKind};

    let inline = FieldSchema::new(
        "subject".to_string(),
        FieldType::RichText { inline: true },
        None,
    );

    let mut card = Card::new("note").unwrap();
    let delta = card
        .revise_field_checked("subject", "hello target world", &inline)
        .unwrap();
    assert!(!delta.ops.is_empty());

    let mut base = card
        .field_content("subject", Codec::Richtext)
        .unwrap()
        .unwrap()
        .into_content();
    // 6..12 is "target".
    base.marks
        .push(Mark::new(6, 12, MarkKind::Anchor { id: "c1".into() }));
    let base = base.into_normalized();
    card.overwrite_field("subject", base).unwrap();
    card.revise_field_checked("subject", "why keep the target here", &inline)
        .unwrap();
    let read = card.field_content("subject", Codec::Richtext).unwrap().unwrap();
    assert!(
        read.marks
            .iter()
            .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")),
        "anchor should rebase onto surviving text, unlike the cold commit_field"
    );

    let before = card.field_text("subject", Codec::Richtext).unwrap().unwrap();
    let err = card
        .revise_field_checked("subject", "line one\n\nline two", &inline)
        .unwrap_err();
    assert_eq!(err.code(), "edit::field_not_inline");
    assert_eq!(card.field_text("subject", Codec::Richtext).unwrap().unwrap(), before);

    let block = FieldSchema::new("body".to_string(), FieldType::RichText { inline: false }, None);
    let d = card
        .revise_field_checked("body", "para one\n\npara two", &block)
        .unwrap();
    assert!(!d.ops.is_empty());
    assert!(card.field_text("body", Codec::Richtext).unwrap().unwrap().contains("para two"));
}

#[test]
fn test_commit_field_richtext_content_object_reads_back() {
    use quillmark_content::model::{Mark, MarkKind};

    let mut content = quillmark_content::import::from_markdown("underlined intro").unwrap().into_content();
    content.marks.push(Mark::new(0, 10, MarkKind::Underline));
    let content = content.into_normalized();
    let json = quillmark_content::serial::to_canonical_value(&content);

    let mut card = Card::new("note").unwrap();
    commit_richtext(&mut card, "intro", &json, false).unwrap();

    assert!(card.payload().get("intro").unwrap().as_json().is_object());
    let read = card.field_content("intro", Codec::Richtext).unwrap().unwrap();
    assert_eq!(read, content);
    assert!(read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)));
}

#[test]
fn test_commit_field_richtext_markdown_null_and_rejects_bad() {
    let mut card = Card::new("note").unwrap();

    commit_richtext(&mut card, "intro", &serde_json::json!("**bold** intro"), false).unwrap();
    assert_eq!(card.field_text("intro", Codec::Richtext).unwrap().unwrap(), "**bold** intro");

    commit_richtext(&mut card, "intro", &serde_json::Value::Null, false).unwrap();
    assert!(card.payload().get("intro").unwrap().as_json().is_null());
    assert!(card.field_content("intro", Codec::Richtext).unwrap().unwrap().is_blank());

    assert_eq!(
        commit_richtext(&mut card, "intro", &serde_json::json!({ "not": "a content" }), false)
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
fn test_commit_field_richtext_inline_enforced_at_write() {
    let mut card = Card::new("note").unwrap();

    commit_richtext(&mut card, "title", &serde_json::json!("A single line"), true).unwrap();
    assert_eq!(card.field_text("title", Codec::Richtext).unwrap().unwrap(), "A single line");

    let err = commit_richtext(
        &mut card,
        "title",
        &serde_json::json!("line one\n\nline two"),
        true,
    )
    .unwrap_err();
    assert_eq!(err.code(), "edit::field_not_inline");
    assert_eq!(card.field_text("title", Codec::Richtext).unwrap().unwrap(), "A single line");
}

#[test]
fn test_commit_field_array_of_inline_richtext_reports_not_inline() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let mut schema = FieldSchema::new("refs".to_string(), FieldType::Array, None);
    schema.items = Some(Box::new(FieldSchema::new(
        "refs".to_string(),
        FieldType::RichText { inline: true },
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
    assert_eq!(err.code(), "edit::field_not_inline");
}

#[test]
fn test_commit_field_scalar_strict() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let int_schema = FieldSchema::new("qty".to_string(), FieldType::Integer, None);

    card.commit_field("qty", QuillValue::from_json(serde_json::json!("3")), &int_schema)
        .unwrap();
    assert_eq!(
        card.payload().get("qty").unwrap().as_json(),
        &serde_json::json!(3)
    );

    let err = card
        .commit_field("qty", QuillValue::from_json(serde_json::json!(true)), &int_schema)
        .unwrap_err();
    assert_eq!(err.code(), "edit::field_coercion_failed");

    assert_eq!(
        card.commit_field("qty", QuillValue::from_json(serde_json::json!("x")), &int_schema)
            .unwrap_err()
            .code(),
        "edit::field_coercion_failed"
    );
    assert_eq!(
        card.payload().get("qty").unwrap().as_json(),
        &serde_json::json!(3)
    );
}

#[test]
fn test_commit_field_object_rejects_non_object() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let schema = FieldSchema::new("meta".to_string(), FieldType::Object, None);
    assert_eq!(
        card.commit_field("meta", QuillValue::from_json(serde_json::json!(42)), &schema)
            .unwrap_err()
            .code(),
        "edit::field_coercion_failed"
    );
}

#[test]
fn test_commit_field_rejects_bad_name() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
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
    assert!(card.field_content("count", Codec::Richtext).unwrap().is_err());
    assert!(card.field_text("count", Codec::Richtext).unwrap().is_err());
}

#[test]
fn test_content_field_emits_as_markdown_projection() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    commit_richtext(
        doc.main_mut(),
        "intro",
        &serde_json::json!("**bold** intro"),
        false,
    )
    .unwrap();

    let md = doc.to_markdown();
    assert!(
        md.contains("intro: \"**bold** intro\""),
        "expected markdown projection, got:\n{md}"
    );
    assert!(!md.contains("lines:"), "content object leaked into card-yaml:\n{md}");

    let reparsed = Document::parse(&md).unwrap().document;
    assert_eq!(
        reparsed.main().payload().get("intro").unwrap().as_str(),
        Some("**bold** intro")
    );
}

#[test]
fn test_non_content_object_field_emits_structurally() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    doc.main_mut()
        .store_field(
            "addr",
            QuillValue::from_json(serde_json::json!({ "city": "Paris" })),
        )
        .unwrap();
    let md = doc.to_markdown();
    assert!(md.contains("addr:"), "{md}");
    assert!(md.contains("city: Paris"), "{md}");
}

/// The projection guard is byte-exact (canonical-string equality), not an
/// order-independent `Value` compare.
#[test]
fn test_noncanonical_order_content_field_stays_structural() {
    let rt = quillmark_content::import::from_markdown("**bold**").unwrap();
    let canonical = quillmark_content::serial::to_canonical_value(&rt);
    let obj = canonical.as_object().unwrap();
    let mut scrambled = serde_json::Map::new();
    for k in obj.keys().rev() {
        scrambled.insert(k.clone(), obj[k].clone());
    }

    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    doc.main_mut()
        .store_field(
            "intro",
            QuillValue::from_json(serde_json::Value::Object(scrambled)),
        )
        .unwrap();
    let md = doc.to_markdown();
    assert!(
        md.contains("marks:") && md.contains("lines:"),
        "non-canonical-order content should stay structural, got:\n{md}"
    );
}

#[test]
fn test_revise_body_returns_delta_and_updates_body() {
    use crate::{Assoc, Delta};

    let mut card = Card::new("note").unwrap();
    card.revise_body("hello world").unwrap();
    let delta: Delta = card.revise_body("hello brave world").unwrap();
    assert_eq!(card.body().text, "hello brave world");
    // The delta maps a stale position at the end of "hello " forward across
    // the inserted "brave ".
    assert_eq!(delta.map_pos(6, Assoc::Before), 6);
    assert_eq!(delta.map_pos(11, Assoc::After), 17);
}

#[test]
fn test_revise_body_rebases_anchor() {
    use quillmark_content::model::{Mark, MarkKind};

    let mut base = quillmark_content::import::from_markdown("keep the target word").unwrap().into_content();
    // Anchor over "target" (chars 9..15).
    base.marks.push(Mark::new(9, 15, MarkKind::Anchor { id: "c1".into() }));
    let base = base.into_normalized();
    let mut card = Card::new("note").unwrap();
    card.overwrite_body(base);

    card.revise_body("why keep the target word").unwrap();
    let anchor = card
        .body()
        .marks
        .iter()
        .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
        .expect("identity anchor survives the whole-document replace");
    let text = &card.body().text;
    let s = quillmark_content::usv::char_to_byte(text, anchor.start);
    let e = quillmark_content::usv::char_to_byte(text, anchor.end);
    assert_eq!(&text[s..e], "target");
}

#[test]
fn test_apply_body_change_applies_bundle() {
    use crate::{ChangeBundle, MarkOp};
    use quillmark_content::delta::diff;
    use quillmark_content::model::MarkKind;

    let mut card = Card::new("note").unwrap();
    card.revise_body("abc").unwrap();
    card.apply_body_change(&ChangeBundle {
        delta: diff("abc", "abXc"),
        mark_ops: vec![MarkOp::Add {
            start: 3,
            end: 4,
            kind: MarkKind::Strong,
        }],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(card.body().text, "abXc");
    let strong = card
        .body()
        .marks
        .iter()
        .find(|m| matches!(m.kind, MarkKind::Strong))
        .expect("strong mark applied post-delta");
    assert_eq!((strong.start, strong.end), (3, 4));
}

#[test]
fn test_apply_body_change_reports_out_of_range() {
    use crate::{ChangeBundle, MarkOp};
    use quillmark_content::delta::diff;
    use quillmark_content::model::MarkKind;

    let mut card = Card::new("note").unwrap();
    card.revise_body("abc").unwrap();
    let result = card.apply_body_change(&ChangeBundle {
        delta: diff("abc", "abc"),
        mark_ops: vec![MarkOp::Add {
            start: 0,
            end: 99,
            kind: MarkKind::Strong,
        }],
        ..Default::default()
    });
    match result {
        Err(EditError::ContentApply(_)) => {}
        other => panic!("expected ContentApply, got {other:?}"),
    }
}

#[test]
fn test_apply_field_change_splices_and_persists() {
    use crate::{ChangeBundle, MarkOp};
    use quillmark_content::delta::diff;
    use quillmark_content::model::MarkKind;

    let mut card = Card::new("note").unwrap();
    commit_richtext(&mut card, "intro", &serde_json::json!("abc"), false).unwrap();
    card.apply_field_change(
        "intro",
        &ChangeBundle {
            delta: diff("abc", "abXc"),
            mark_ops: vec![MarkOp::Add {
                start: 3,
                end: 4,
                kind: MarkKind::Strong,
            }],
            ..Default::default()
        },
    )
    .unwrap();

    assert!(card.payload().get("intro").unwrap().as_json().is_object());
    let rt = card.field_content("intro", Codec::Richtext).unwrap().unwrap();
    assert_eq!(rt.text, "abXc");
    assert!(rt.marks.iter().any(|m| matches!(m.kind, MarkKind::Strong)));
}

#[test]
fn test_apply_field_change_rejects_non_content() {
    let mut card = Card::new("note").unwrap();
    card.store_field("count", 3).unwrap();
    assert_eq!(
        card.apply_field_change("count", &crate::ChangeBundle::default())
            .unwrap_err()
            .code(),
        "edit::field_decode"
    );
}

#[test]
fn test_apply_field_change_treats_an_absent_field_as_empty() {
    use crate::ChangeBundle;

    let mut card = Card::new("note").unwrap();
    card.apply_field_change("intro", &ChangeBundle::default())
        .expect("a zero-base bundle lands on the empty content");
    assert_eq!(card.field_text("intro", Codec::Richtext).unwrap().unwrap(), "");

    let stale = ChangeBundle {
        delta: quillmark_content::Delta {
            ops: vec![quillmark_content::delta::Op::Retain(4)],
        },
        ..ChangeBundle::default()
    };
    assert_eq!(
        card.apply_field_change("missing", &stale)
            .unwrap_err()
            .code(),
        "edit::content_apply"
    );
    assert!(card.payload().get("missing").is_none());
}

#[test]
fn test_invariants_after_mutation_sequence() {
    let mut doc = make_doc();

    doc.main_mut().store_field("author", qv("Alice")).unwrap();
    doc.main_mut().store_field("version", qv_int(3)).unwrap();

    let c1 = Card::new("note").unwrap();
    let c2 = Card::new("summary").unwrap();
    let c3 = Card::new("appendix").unwrap();
    doc.push_card(c1).unwrap();
    doc.push_card(c2).unwrap();
    doc.insert_card(1, c3).unwrap(); // now: note, appendix, summary

    doc.card_mut(0)
        .unwrap()
        .store_field("text", qv("Hello"))
        .unwrap();

    doc.move_card(2, 0).unwrap(); // summary, note, appendix

    doc.remove_card(1); // summary, appendix

    doc.main_mut().revise_body("Updated body.").unwrap();

    doc.main_mut().remove_field("version").unwrap();

    for key in doc.main().payload().keys() {
        assert!(
            is_valid_field_name(key),
            "invalid key '{}' found in payload",
            key
        );
    }

    for card in doc.cards() {
        if let Some(kind) = card.kind() {
            assert!(is_valid_kind_name(kind), "invalid kind '{}' found", kind);
        }
    }

    let json = doc.to_plate_json();
    assert!(json.is_object());
    assert_eq!(json["$quill"].as_str(), Some("test_quill"));
    assert!(json["$cards"].is_array());
    assert_eq!(json["$body"]["text"].as_str(), Some("Updated body."));

    assert_eq!(
        doc.main().payload().get("author").unwrap().as_str(),
        Some("Alice")
    );
    assert!(doc.main().payload().get("version").is_none());
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

#[test]
fn test_store_ext_namespace_preserves_siblings() {
    let mut doc = make_doc();
    doc.main_mut()
        .store_ext_namespace("presentation", serde_json::json!({ "title": "A" }))
        .expect("set_ext_namespace");
    doc.main_mut()
        .store_ext_namespace("agent", serde_json::json!({ "pinned": true }))
        .expect("set_ext_namespace");

    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("A"));
    assert_eq!(ext["agent"]["pinned"].as_bool(), Some(true));

    doc.main_mut()
        .store_ext_namespace("presentation", serde_json::json!({ "title": "B" }))
        .expect("set_ext_namespace");
    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("B"));
    assert_eq!(ext["agent"]["pinned"].as_bool(), Some(true));
}

#[test]
fn test_remove_ext_namespace_preserves_siblings() {
    let mut doc = make_doc();
    doc.main_mut()
        .store_ext_namespace("presentation", serde_json::json!({ "title": "A" }))
        .expect("set_ext_namespace");
    doc.main_mut()
        .store_ext_namespace("tutorial", serde_json::json!(["step-1", "step-2"]))
        .expect("set_ext_namespace");

    let removed = doc.main_mut().remove_ext_namespace("tutorial").unwrap();
    assert_eq!(removed, serde_json::json!(["step-1", "step-2"]));
    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("A"));
    assert!(!ext.contains_key("tutorial"));
}

#[test]
fn test_remove_ext_namespace_drops_ext_when_empty() {
    let mut doc = make_doc();
    doc.main_mut()
        .store_ext_namespace("tutorial", serde_json::json!(["step-1"]))
        .expect("set_ext_namespace");

    let removed = doc.main_mut().remove_ext_namespace("tutorial").unwrap();
    assert_eq!(removed, serde_json::json!(["step-1"]));
    assert!(doc.main().ext().is_none());
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
    let mut doc =
        crate::document::Document::parse("~~~\n$quill: q@1.0\n$kind: main\n~~~\n").unwrap().document;
    let ok = crate::value::QuillValue::from_json(deep_value(50));
    assert!(doc.main_mut().store_field("x", ok).is_ok());

    let too_deep = crate::value::QuillValue::from_json(deep_value(150));
    let err = doc.main_mut().store_field("y", too_deep).unwrap_err();
    assert!(
        matches!(err, crate::document::EditError::ValueTooDeep { max: 128 }),
        "expected ValueTooDeep, got {err:?}"
    );
    let too_deep = crate::value::QuillValue::from_json(deep_value(150));
    assert!(doc.main_mut().store_fill("y", too_deep).is_err());
    let serde_json::Value::Object(map) = deep_value(150) else {
        unreachable!()
    };
    assert!(doc.main_mut().store_ext(map).is_err());
    assert!(doc
        .main_mut()
        .store_ext_namespace("ns", deep_value(150))
        .is_err());
}

/// The `$ext` map is itself a level, so its values carry `MAX_JSON_DEPTH - 1`:
/// the wholesale store and the namespace merge bound the merged map identically.
#[test]
fn store_ext_charges_the_map_its_own_level() {
    let mut doc =
        crate::document::Document::parse("~~~\n$quill: q@1.0\n$kind: main\n~~~\n").unwrap().document;
    let map = |depth: usize| {
        let mut m = serde_json::Map::new();
        m.insert("a".to_string(), deep_value(depth));
        m
    };

    doc.main_mut().store_ext(map(127)).expect("127 levels under a map is exactly the limit");
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

    doc.main_mut().store_ext_namespace("ns", deep_value(127)).expect("the merge bound matches");
    assert!(doc.main_mut().store_ext_namespace("ns", deep_value(128)).is_err());
}

#[test]
fn storage_dto_rejects_value_past_depth_limit() {
    let stored = serde_json::json!({
        "schema": "quillmark/document@0.92.0",
        "main": {
            "payload": {"items": [
                {"type": "quill", "value": "q@1.0"},
                {"type": "kind", "value": "main"},
                {"type": "field", "key": "x", "value": deep_value(150)}
            ]},
            "body": ""
        },
        "cards": []
    });
    let err = serde_json::from_value::<crate::document::Document>(stored).unwrap_err();
    assert!(
        err.to_string().contains("deeper than the maximum"),
        "expected depth error, got {err}"
    );

    let serde_json::Value::Object(deep_map) = deep_value(150) else {
        unreachable!()
    };
    let stored = serde_json::json!({
        "schema": "quillmark/document@0.92.0",
        "main": {
            "payload": {"items": [
                {"type": "quill", "value": "q@1.0"},
                {"type": "kind", "value": "main"},
                {"type": "ext", "value": deep_map}
            ]},
            "body": ""
        },
        "cards": []
    });
    let err = serde_json::from_value::<crate::document::Document>(stored).unwrap_err();
    assert!(
        err.to_string().contains("deeper than the maximum"),
        "expected $ext depth error, got {err}"
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
    assert!(
        err.to_string().contains("deeper than the maximum"),
        "expected depth error, got {err}"
    );

    let wire: crate::document::CardWire = serde_json::from_value(serde_json::json!({
        "kind": "note",
        "payloadItems": [
            {"type": "field", "key": "Bad Name", "value": 1}
        ],
        "body": ""
    }))
    .unwrap();
    let err = crate::document::Card::try_from(wire).unwrap_err();
    assert!(
        err.to_string().contains("[A-Za-z_]"),
        "expected name error, got {err}"
    );
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
        doc.push_card(card.clone()).map(|()| doc).map_err(|e| {
            let mut doc = make_doc();
            assert_eq!(doc.insert_card(0, card), Err(e.clone()), "push and insert agree");
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
        placed(&|w| w.seed = Some(
            serde_json::json!({"note": {}}).as_object().unwrap().clone()
        ))
        .unwrap_err(),
        EditError::RootOnlyEntry {
            key: "$seed".to_string()
        }
    );

    // `$ext` is not root-only: it rides any card.
    let doc = placed(&|w| {
        w.ext = Some(serde_json::json!({"ns": {}}).as_object().unwrap().clone())
    })
    .expect("$ext is allowed on a composable card");
    let _ = Document::parse(&doc.to_markdown()).expect("emit reparses");

    // The same wire is how the main card is read back, so refusing `$quill` at
    // placement must not cost that round trip.
    let main = CardWire::from(make_doc().main());
    assert!(main.quill.is_some());
    Card::try_from(main).expect("the main card still crosses the wire");
}

/// A card the wire refuses never reaches placement: the payload invariants that
/// are card-local are checked where the card is built.
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
        fill: false,
        nested_fills: Vec::new(),
    };

    let dup = built(vec![field("title"), field("title")]).unwrap_err();
    assert!(
        matches!(violation(&dup), PayloadViolation::DuplicateField { key } if key == "title"),
        "{dup:?}"
    );

    let too_many =
        built((0..=crate::error::MAX_FIELD_COUNT).map(|i| field(&format!("f{i}"))).collect())
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

/// The storage DTO is a hand-craftable ingress, so it owns the positional
/// invariants the parser enforces on source.
#[test]
fn storage_dto_polices_root_kind_and_repeated_entries() {
    let load = |items: serde_json::Value| {
        serde_json::from_value::<crate::document::Document>(serde_json::json!({
            "schema": "quillmark/document@0.92.0",
            "main": {"payload": {"items": items}, "body": ""},
            "cards": []
        }))
    };
    let quill = serde_json::json!({"type": "quill", "value": "q@1.0"});
    let kind = |v: &str| serde_json::json!({"type": "kind", "value": v});

    let err = load(serde_json::json!([quill, kind("note")])).unwrap_err();
    assert!(
        err.to_string().contains("reserved for the document root"),
        "a non-`main` root kind emits a block the parser rejects, got: {err}"
    );

    // Absent is not malformed: the parser synthesises it, so the DTO does too.
    let doc = load(serde_json::json!([quill])).expect("an absent root kind loads");
    assert_eq!(doc.main().kind(), Some("main"));
    let reparsed = Document::parse(&doc.to_markdown()).expect("emit reparses");
    assert_eq!(reparsed.document.main().kind(), Some("main"));

    for repeated in [
        serde_json::json!([quill, kind("main"), quill]),
        serde_json::json!([quill, kind("main"), kind("main")]),
    ] {
        let err = load(repeated).unwrap_err();
        assert!(
            err.to_string().contains("duplicate"),
            "a repeated `$` entry emits the line twice, got: {err}"
        );
    }
}

/// A comment carrying a newline emits bare YAML after its first line: the
/// storage twin of the wire's injection guard.
#[test]
fn storage_dto_refuses_a_multi_line_comment() {
    let err = serde_json::from_value::<crate::document::Document>(serde_json::json!({
        "schema": "quillmark/document@0.92.0",
        "main": {
            "payload": {"items": [
                {"type": "quill", "value": "q@1.0"},
                {"type": "kind", "value": "main"},
                {"type": "comment", "text": "hi\ninjected: pwned", "inline": false}
            ]},
            "body": ""
        },
        "cards": []
    }))
    .unwrap_err();
    assert!(err.to_string().contains("one line"), "got: {err}");
}

/// One violation, the three ingestion boundaries that spell it. Parse and
/// storage render the text `FieldViolation` owns, naming the key; the wire is a
/// mutator door, so it renders the `EditError` `store_field` raises for the
/// same name.
#[test]
fn every_ingestion_boundary_renders_its_violation_text() {
    use crate::document::edit::FieldViolation;

    let expected = FieldViolation::InvalidName.message("bad name");

    let parse_err =
        Document::parse("~~~card-yaml\n$quill: test_quill\n$kind: main\nbad name: v\n~~~\n")
            .unwrap_err();
    assert!(
        parse_err.to_string().contains(&expected),
        "parse: {parse_err}"
    );

    let mut wire = crate::document::CardWire::new("main".to_string(), serde_json::json!(""));
    wire.payload_items = vec![crate::document::PayloadItemWire::Field {
        key: "bad name".to_string(),
        value: serde_json::json!("v"),
        fill: false,
        nested_fills: Vec::new(),
    }];
    let wire_err = Card::try_from(wire).unwrap_err();
    assert_eq!(
        wire_err.to_string(),
        EditError::InvalidFieldName("bad name".to_string()).to_string(),
        "wire: {wire_err}"
    );

    let storage_err = serde_json::from_value::<Document>(serde_json::json!({
        "schema": "quillmark/document@0.92.0",
        "main": {
            "payload": {"items": [
                {"type": "quill", "value": "q@1.0"},
                {"type": "kind", "value": "main"},
                {"type": "field", "key": "bad name", "value": "v"}
            ]},
            "body": ""
        },
        "cards": []
    }))
    .unwrap_err();
    assert!(
        storage_err.to_string().contains(&expected),
        "storage: {storage_err}"
    );
}
