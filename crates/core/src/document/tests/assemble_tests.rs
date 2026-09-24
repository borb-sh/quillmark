use crate::document::Document;

/// `Document::parse` with the warnings dropped — what these tests read is the
/// document or the refusal.
fn decompose(markdown: &str) -> Result<Document, crate::error::ParseError> {
    Document::parse(markdown).map(|p| p.document)
}

#[test]
fn test_size_and_emptiness_refusals_carry_their_codes() {
    let oversized_yaml = format!(
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ndata: \"{}\"\n~~~\n\nBody",
        "x".repeat(crate::error::MAX_YAML_SIZE + 1)
    );
    for (input, code) in [
        ("".to_string(), "parse::empty_input"),
        ("   ".to_string(), "parse::empty_input"),
        ("\n\n\t\n".to_string(), "parse::empty_input"),
        ("a".repeat(crate::error::MAX_INPUT_SIZE + 1), "parse::input_too_large"),
        (oversized_yaml, "parse::input_too_large"),
    ] {
        assert_eq!(decompose(&input).unwrap_err().code(), code, "{:.40?}", input);
    }
}

/// Each block parses once the one offending line is dropped.
#[test]
fn test_structural_violations_are_refused() {
    let root = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n";
    for (label, markdown) in [
        ("non-main root kind", "~~~card-yaml\n$quill: test_quill\n$kind: other\n~~~".to_string()),
        ("empty $quill", "~~~card-yaml\n$quill:\n~~~".to_string()),
        ("$quill on a card", format!("{root}~~~card-yaml\n$quill: second\n$kind: note\n~~~")),
        ("$seed on a card", format!("{root}~~~card-yaml\n$kind: note\n$seed:\n  a:\n    from: X\n~~~")),
        ("unknown $ key on a card", format!("{root}~~~card-yaml\n$foo: bar\n$kind: note\n~~~")),
        ("$id on a card", format!("{root}~~~card-yaml\n$kind: note\n$id: a\n~~~")),
        ("$id on the root", "~~~\n$quill: q@0.1\n$id: x\n~~~\n".to_string()),
        ("scalar $ext", "~~~card-yaml\n$quill: q\n$kind: main\n$ext: just-a-string\n~~~".to_string()),
        ("scalar $seed", "~~~card-yaml\n$quill: q\n$kind: main\n$seed: just-a-string\n~~~".to_string()),
        ("non-ASCII field name", "~~~card-yaml\n$quill: q\n$kind: main\nタイトル: x\n~~~".to_string()),
        ("field name with a space", "~~~card-yaml\n$quill: q\n$kind: main\nbad name: v\n~~~".to_string()),
    ] {
        let err = decompose(&markdown).expect_err(label);
        assert_eq!(err.code(), "parse::invalid_structure", "{label}: {err}");
    }
    for kind in ["ITEMS", "123items", "my-items", ""] {
        let markdown = format!("{root}~~~card-yaml\n$kind: {kind}\n~~~\n\nBody.");
        let err = decompose(&markdown).unwrap_err();
        assert_eq!(err.code(), "parse::invalid_structure", "kind {kind:?}: {err}");
    }
}

#[test]
fn test_missing_quill_diagnostic_code() {
    let cases = [
        "# Hello World\n\nNo payload here.",
        "Just prose, no card-yaml block.",
    ];
    for input in cases {
        let err = decompose(input).unwrap_err();
        let diag = err.to_diagnostic();
        assert_eq!(
            diag.code.as_deref(),
            Some("parse::missing_quill"),
            "expected parse::missing_quill for {input:?}, got: {:?}",
            diag.code
        );
    }
}

#[test]
fn test_malformed_quill_reference_carries_code_and_grammar_hint() {
    let err =
        decompose("~~~card-yaml\n$quill: Resume@2.1.0\n$kind: main\n~~~\n\nBody\n").unwrap_err();
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("parse::invalid_quill_reference"));
    assert_eq!(
        diag.hint.as_deref(),
        Some(crate::version::quill_ref_hint()),
        "the malformed-reference diagnostic must carry the canonical grammar hint"
    );
}

#[test]
fn test_body_prose_inside_the_block_is_told_to_close_the_block() {
    let md = "~~~card-yaml\n$quill: usaf_memo\n$kind: main\ntitle: Near-Miss Report\n\
              88th Communications Squadron, Wright-Patterson AFB\n\
              This memorandum documents a near-miss on the flight line.\n~~~\n";
    let err = decompose(md).unwrap_err();
    let hint = err.to_diagnostic().hint.expect("hint should be set");
    assert!(hint.contains("reads as prose"), "got: {hint}");
    assert!(hint.contains("88th Communications Squadron"), "got: {hint}");
    assert!(!hint.contains("block scalar"), "got: {hint}");
}

/// `---` front matter declaring `$quill` is one fence away from a root block,
/// so the message names that edit.
#[test]
fn test_dash_frontmatter_with_quill_names_the_fence_edit() {
    let err = decompose("---\n$quill: usaf_memo\n$kind: main\ntitle: Memo\n---\n\nBody\n")
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Replace the opening `---`"), "got: {msg}");
    assert!(msg.contains("~~~"), "got: {msg}");
}

/// Without `$quill` the document needs both edits, so the generic shape — which
/// names the fence *and* the key — beats the fence-only hint.
#[test]
fn test_dash_frontmatter_without_quill_reports_the_generic_shape() {
    let err = decompose("---\nquill: usaf_memo\ntitle: Memo\n---\n\nBody\n").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("$quill: <name>"), "got: {msg}");
    assert!(
        !msg.contains("Replace the opening `---`"),
        "fence-only hint under-reports the missing key: {msg}"
    );
}

#[test]
fn test_missing_block_with_bare_yaml_calls_out_missing_fence() {
    let err = decompose("$quill: usaf_memo\n$kind: main\ntitle: Memo\n").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("missing the `~~~` fence"), "got: {msg}");
}

#[test]
fn test_unclosed_root_fence_names_the_missing_closer_not_the_missing_block() {
    let markdown =
        "~~~\n$quill: usaf_memo@0.3.0\n$kind: main\nsubject: Memo\nfont_size: 12\n\nThe body.\n";
    let err = decompose(markdown).unwrap_err();
    let msg = err.to_string();
    assert_eq!(
        err.to_diagnostic().code.as_deref(),
        Some("parse::missing_quill")
    );
    assert!(
        msg.contains("Root card-yaml block opened at line 1 is never closed"),
        "got: {msg}"
    );
    assert!(
        msg.contains("after the last field (`font_size`)"),
        "got: {msg}"
    );
    assert!(
        !msg.contains("The document must open with"),
        "the generic advice restates what the author already wrote: {msg}"
    );
}

#[test]
fn test_two_tilde_closer_is_named_as_the_failed_closer() {
    let markdown = "~~~\n$quill: usaf_memo@0.3.0\n$kind: main\ntitle: Memo\n~~\n\nThe body.\n";
    let msg = decompose(markdown).unwrap_err().to_string();
    assert!(
        msg.contains("Root card-yaml block opened at line 1 is never closed"),
        "got: {msg}"
    );
    assert!(
        msg.contains("The line `~~` at line 5 does not close it"),
        "got: {msg}"
    );
}

#[test]
fn test_indented_closer_is_named_as_the_failed_closer() {
    let markdown = "~~~\n$quill: usaf_memo@0.3.0\n$kind: main\ntitle: Memo\n  ~~~\n\nBody.\n";
    let msg = decompose(markdown).unwrap_err().to_string();
    assert!(
        msg.contains("The line `~~~` at line 5 does not close it"),
        "got: {msg}"
    );
}

#[test]
fn test_unclosed_root_fence_without_quill_keeps_the_generic_message() {
    for markdown in [
        "~~~\ntitle: Memo\n\nThe body.\n",
        "---\n$quill: test_quill\n$kind: main\ntitle: T\n~~~\n\nBody.",
    ] {
        let msg = decompose(markdown).unwrap_err().to_string();
        assert!(msg.contains("Missing required root"), "got: {msg}");
    }
}

/// Every `---` below the root block is CommonMark's, whatever it encloses: a
/// would-be card stays in the body, and paired breaks around a `Word:`
/// paragraph are prose the parser neither claims nor refuses.
#[test]
fn test_dash_blocks_below_the_root_are_body_prose() {
    let as_card = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\nBody.\n\n\
                   ---\n$kind: note\nlabel: a\n---\n\nNote body.";
    let doc = decompose(as_card).expect("a `---` below the root block is CommonMark's");
    assert!(doc.cards().is_empty(), "no card opens on `---`");
    assert!(
        doc.main().body_markdown().contains("$kind: note"),
        "the would-be card stays in the body: {:?}",
        doc.main().body_markdown()
    );

    let as_prose = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\nBody.\n\n\
                    ---\n\nNote: the second break closes nothing.\n\n---\n\nMore body.";
    let doc = decompose(as_prose).expect("thematic breaks are CommonMark's");
    assert!(doc.cards().is_empty());
    assert!(doc.main().body_markdown().contains("Note: the second break"));
}

#[test]
fn test_tilde_opener_with_dash_closer_falls_through() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: T\n---\n\nBody.";
    let err = decompose(markdown).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("opened at line 1 is never closed"),
        "got: {msg}"
    );
}

/// The card sequence: every block below the root becomes a card in source
/// order, keeping its kind and its body, whatever kinds interleave. What a
/// card's payload can hold is the named tests around this one.
#[test]
fn cards_parse_in_source_order_keeping_kind_and_body() {
    let markdown = "\
~~~card-yaml
$quill: test_quill
$kind: main
title: Global
~~~

Global body.

~~~card-yaml
$kind: section
heading: Introduction
~~~

Intro content.

~~~card-yaml
$kind: item
name: Item 1
~~~

First item body.

~~~card-yaml
$kind: item
name: Item 2
~~~

Second item body.

~~~card-yaml
$kind: section
heading: Conclusion
~~~

Conclusion content.
";
    let doc = decompose(markdown).expect("parses");
    assert_eq!(doc.quill_reference().name, "test_quill");
    assert_eq!(doc.main().body_markdown(), "Global body.");

    let want = [
        ("section", "Introduction", "Intro content."),
        ("item", "Item 1", "First item body."),
        ("item", "Item 2", "Second item body."),
        ("section", "Conclusion", "Conclusion content."),
    ];
    assert_eq!(
        doc.cards().len(),
        want.len(),
        "one card per block below the root"
    );
    for (card, (kind, label, body)) in doc.cards().iter().zip(want) {
        let got_label = card
            .payload()
            .get("heading")
            .or_else(|| card.payload().get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{kind} card carries its label field"));
        assert_eq!(
            (card.kind(), got_label, card.body_markdown().as_str()),
            (Some(kind), label, body)
        );
    }
}

#[test]
fn test_payload_and_body_split() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n\ndescription: after a blank line\n~~~\n\n# Hello World\n\nThis is the body.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body_markdown(), "# Hello World\n\nThis is the body.");
    let keys: Vec<&str> = doc.main().payload().keys().map(|k| k.as_str()).collect();
    assert_eq!(keys, ["title", "description"]);
    assert_eq!(
        doc.main().payload().get("description").unwrap().as_str(),
        Some("after a blank line")
    );
}

/// Blank lines between a fence and its neighbours are separators, not body.
#[test]
fn test_bodies_shed_their_blank_separators() {
    let root = "~~~card-yaml\n$quill: q\n$kind: main\n~~~";
    for (label, markdown, want) in [
        ("payload at EOF", format!("{root}"), vec![""]),
        ("leading newlines", format!("{root}\n\n\n\nBody."), vec!["Body."]),
        ("trailing newlines", format!("{root}\n\nBody.\n\n\n"), vec!["Body."]),
        (
            "body before a card",
            format!("{root}\n\nbody\n\n~~~card-yaml\n$kind: x\n~~~\n"),
            vec!["body", ""],
        ),
        (
            "card bodies",
            format!("{root}\n\n~~~card-yaml\n$kind: a\n~~~\n\nfirst\n\n~~~card-yaml\n$kind: b\n~~~\n\nsecond\n"),
            vec!["", "first", "second"],
        ),
    ] {
        let doc = decompose(&markdown).unwrap();
        let got: Vec<String> = std::iter::once(doc.main())
            .chain(doc.cards())
            .map(|c| c.body_markdown())
            .collect();
        assert_eq!(got, want, "{label}");
    }
}

#[test]
fn test_uppercase_payload_keys_accepted_at_parse() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: section
BODY: Test
~~~";

    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("BODY")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test"
    );
    assert!(
        doc.to_markdown().contains("BODY: Test"),
        "uppercase field name must round-trip bare and verbatim"
    );
}

#[test]
fn test_root_without_kind_is_accepted_and_synthesised() {
    let markdown = "~~~card-yaml
$quill: test_quill
title: Test
~~~

Body content.";

    let doc = decompose(markdown).expect("root without $kind should parse");
    assert_eq!(doc.main().kind(), Some("main"));
    assert_eq!(doc.main().quill().unwrap().name.as_str(), "test_quill");

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("$kind: main"),
        "canonical emission should synthesise $kind: main; got: {emitted}"
    );

    let quill_pos = emitted.find("$quill:").expect("emitted lacks $quill");
    let kind_pos = emitted.find("$kind:").expect("emitted lacks $kind");
    let title_pos = emitted.find("title:").expect("emitted lacks title");
    assert!(
        quill_pos < kind_pos && kind_pos < title_pos,
        "canonical order is $quill < $kind < user fields; got: {emitted}"
    );

    let reparsed = decompose(&emitted).expect("emitted form re-parses");
    assert_eq!(doc, reparsed);
}

#[test]
fn test_over_nested_body_surfaces_body_import_error() {
    let deep = ">".repeat(crate::error::MAX_NESTING_DEPTH + 5);
    let markdown =
        format!("~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n{deep} too deep\n");
    let err = decompose(&markdown).unwrap_err();
    assert_eq!(
        err.to_diagnostic().code.as_deref(),
        Some("parse::body_import")
    );
}

#[test]
fn test_canonical_root_with_kind_round_trips_byte_equal() {
    let canonical = "~~~\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\nBody.\n\n~~~\n$kind: note\nname: Widget\n~~~\n";
    let doc = decompose(canonical).unwrap();
    assert_eq!(doc.to_markdown(), canonical);
}

#[test]
fn dollar_keys_at_any_position_in_payload_work() {
    let markdown = "~~~card-yaml
title: First
$quill: test_quill
author: Bob
$kind: main
~~~

Body.";

    let doc = decompose(markdown).expect("payload with $-keys mid-mapping should parse");
    assert_eq!(doc.main().quill().unwrap().to_string(), "test_quill");
    assert_eq!(doc.main().kind(), Some("main"));
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str(),
        Some("First")
    );
    assert_eq!(
        doc.main().payload().get("author").unwrap().as_str(),
        Some("Bob")
    );
    assert!(doc.main().payload().get("$quill").is_none());
    assert!(doc.main().payload().get("$kind").is_none());

    let emitted = doc.to_markdown();
    let reparsed = decompose(&emitted).expect("round-trip should re-parse");
    assert_eq!(doc, reparsed);
}

/// A YAML scalar is opaque to the markdown layer, so a value that reads as
/// markup anywhere else survives a payload whole, at every nesting. What the
/// *body* does with `<<word>>` is CommonMark's, and `quillmark_content` owns
/// it.
#[test]
fn chevrons_survive_every_payload_position() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test <<with chevrons>>
items:
  - \"<<first>>\"
metadata:
  description: \"<<nested value>>\"
~~~

Body.

~~~card-yaml
$kind: items
description: \"<<card yaml>>\"
~~~

Card body.";

    let doc = decompose(markdown).unwrap();
    let main = doc.main().payload();
    assert_eq!(
        main.get("title").unwrap().as_str().unwrap(),
        "Test <<with chevrons>>"
    );
    assert_eq!(
        main.get("items").unwrap().as_array().unwrap()[0]
            .as_str()
            .unwrap(),
        "<<first>>"
    );
    assert_eq!(
        main.get("metadata")
            .unwrap()
            .as_object()
            .unwrap()
            .get("description")
            .unwrap()
            .as_str()
            .unwrap(),
        "<<nested value>>"
    );
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("description")
            .unwrap()
            .as_str()
            .unwrap(),
        "<<card yaml>>"
    );
}

#[test]
fn test_multiline_chevrons_projection() {
    // A plain-text `<<text ... >>` spanning a line follows CommonMark HTML rules
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n<<text\nacross lines>>";
    let doc = decompose(markdown).unwrap();
    let body = doc.main().body_markdown();
    assert_eq!(body, "\\<>");
}

#[test]
fn test_unmatched_chevrons_preserved() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n<<unmatched";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body_markdown(), "\\<\\<unmatched");
}

#[test]
fn test_line_ending_normalization() {
    for markdown in [
        "~~~card-yaml\r\n$quill: test_quill\r\n$kind: main\r\ntitle: Test\r\n~~~\r\n\r\nBody content.",
        "~~~card-yaml\n$quill: test_quill\r\n$kind: main\r\ntitle: Test\r\n~~~\n\nBody.",
    ] {
        let doc = decompose(markdown).unwrap();
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str().unwrap(),
            "Test"
        );
    }
}

#[test]
fn crlf_input_leaves_no_carriage_return_in_comment_text() {
    let markdown = "~~~card-yaml\r\n$quill: test_quill\r\n$kind: main\r\n# standalone\r\ntitle: Test # trailing\r\n~~~\r\n\r\nBody.";
    let doc = decompose(markdown).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        !emitted.contains('\r'),
        "emit is LF-only, got: {emitted:?}"
    );
    assert!(
        emitted.contains("# trailing\n") && emitted.contains("# standalone\n"),
        "comment text kept its content, got: {emitted:?}"
    );
}

/// The YAML scalar forms a card-yaml block admits, and the value each parses
/// to. What survives *emission* is `emit`'s own scalar round-trips.
#[test]
fn single_field_yaml_scalar_types() {
    use serde_json::json;

    for (label, field, key, want) in [
        (
            "literal block scalar (`|`)",
            "description: |\n  one\n  two\n",
            "description",
            json!("one\ntwo\n"),
        ),
        (
            "folded block scalar (`>`)",
            "description: >\n  one\n  two\n",
            "description",
            json!("one two\n"),
        ),
        ("empty string", "empty: \"\"\n", "empty", json!("")),
        (
            "structural characters, quoted",
            "special: \"colon: here, and [brackets]\"\n",
            "special",
            json!("colon: here, and [brackets]"),
        ),
        ("integer", "count: 42\n", "count", json!(42)),
        ("float", "price: 19.99\n", "price", json!(19.99)),
        ("boolean", "active: true\n", "active", json!(true)),
        (
            "a sequence of mixed scalars",
            "items:\n  - first\n  - 100\n  - true\n",
            "items",
            json!(["first", 100, true]),
        ),
    ] {
        let markdown = format!("~~~card-yaml\n$quill: test_quill\n$kind: main\n{field}~~~\n\nBody.");
        let doc = decompose(&markdown).unwrap_or_else(|e| panic!("{label}: parse failed: {e}"));
        let got = doc
            .main()
            .payload()
            .get(key)
            .unwrap_or_else(|| panic!("{label}: missing field {key:?}"));
        assert_eq!(got.as_json(), &want, "{label}");
    }
}

#[test]
fn test_f2_strip_does_not_overstrip_content_newlines() {
    let markdown =
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n```\ncode\n```\n\n\n~~~card-yaml\n$kind: x\n~~~\n";
    let doc = decompose(markdown).unwrap();
    let emitted = doc.to_markdown();
    let reparsed = Document::parse(&emitted).unwrap().document;
    assert_eq!(doc.main().body_markdown(), reparsed.main().body_markdown());
    assert!(
        doc.main().body_markdown().ends_with("```"),
        "expected code block, got {:?}",
        doc.main().body_markdown()
    );
}

#[test]
fn test_to_plate_json_with_cards() {
    let markdown = "~~~card-yaml
$quill: usaf_memo
$kind: main
title: Test
~~~

Global body.

~~~card-yaml
$kind: indorsement
for: ORG
~~~

Card body here.
";
    let doc = Document::parse(markdown).unwrap().document;
    let json = doc.to_plate_json_gated(true, None);

    assert_eq!(json["$quill"], "usaf_memo");
    assert_eq!(json["title"], "Test");
    assert_eq!(json["$body"]["text"], "Global body.");

    let cards = json["$cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["$kind"], "indorsement");
    assert_eq!(cards[0]["for"], "ORG");
    assert_eq!(cards[0]["$body"]["text"], "Card body here.");
}

#[test]
fn test_to_plate_json_kindless_card_omits_kind() {
    use crate::document::{Card, Payload};

    let mut doc = Document::parse("~~~card-yaml\n$quill: my_quill\n$kind: main\n~~~\n\nBody.\n")
        .unwrap()
        .document;
    doc.cards_vec_mut().push(Card::from_parts(
        Payload::new(),
        quillmark_content::model::Normalized::empty(),
    ));

    let json = doc.to_plate_json_gated(true, None);
    let card = &json["$cards"][0];
    assert!(
        card.get("$kind").is_none(),
        "a kindless card must carry no $kind: {card}"
    );
    assert!(
        card.get("$body").is_some(),
        "the schema-free serializer still emits $body: {card}"
    );
}

#[test]
fn test_to_plate_json_quill_first() {
    let doc = Document::parse(
        "~~~card-yaml\n$quill: my_quill\n$kind: main\nfoo: bar\nbaz: qux\n~~~\n",
    )
    .unwrap()
    .document;
    let json = doc.to_plate_json_gated(true, None);
    let obj = json.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    assert_eq!(keys[0], "$quill");
}

/// `serde_json::Map::remove` under `preserve_order` is `swap_remove`, not the
/// order-preserving `shift_remove`.
#[test]
fn payload_field_order_preserved_after_quill_removal() {
    let md = "~~~card-yaml\n$quill: q\n$kind: main\nsender: Alice\nrecipient: Bob\ndate: March 15\nsubject: hi\n~~~\n";
    let doc = Document::parse(md).unwrap().document;
    let keys: Vec<&str> = doc.main().payload().keys().map(|s| s.as_str()).collect();
    assert_eq!(
        keys,
        vec!["sender", "recipient", "date", "subject"],
        "Payload fields must preserve insertion order"
    );
}

#[test]
fn a_user_field_named_id_is_untouched() {
    let md = "~~~\n$quill: q@0.1\n~~~\n\n~~~\n$kind: note\nid: a\n~~~\n";
    let doc = Document::parse(md).unwrap().document;
    assert_eq!(
        doc.cards()[0].payload().get("id").map(|v| v.as_json().clone()),
        Some(serde_json::json!("a"))
    );
}

/// The 1-indexed document line carrying `needle`.
fn line_of(markdown: &str, needle: &str) -> u32 {
    markdown
        .lines()
        .position(|l| l.contains(needle))
        .map(|i| i as u32 + 1)
        .unwrap_or_else(|| panic!("`{needle}` is not in the fixture"))
}

#[test]
fn test_yaml_error_location_is_document_absolute() {
    let markdown = "# Heading\n\nIntro prose.\n\n~~~\n$quill: usaf_memo\n$kind: main\ntitle: Briefing\n\n\nunit: 88th Communications Squadron: Wright-Patterson AFB\n~~~\n\nBody\n";
    let diag = decompose(markdown).unwrap_err().to_diagnostic();

    assert_eq!(
        diag.code.as_deref(),
        Some("parse::yaml_error_with_location")
    );
    let loc = diag.location.expect("the diagnostic carries a location");
    assert_eq!(loc.file, "input.md");
    assert_eq!(loc.line, line_of(markdown, "88th Communications"));
    assert_eq!(loc.column, 35);
}

#[test]
fn test_yaml_error_message_carries_one_line_number_system() {
    let markdown = "~~~\n$quill: usaf_memo\n$kind: main\nunit: a: b\n~~~\n\nBody\n";
    let diag = decompose(markdown).unwrap_err().to_diagnostic();

    assert!(
        diag.message.starts_with("YAML error in the root card-yaml block: "),
        "the message names the block rather than a second line number: {}",
        diag.message
    );
    assert_eq!(
        diag.args.keys().collect::<Vec<_>>(),
        vec!["blockIndex"],
        "the coordinates ride on `location`, not `args`"
    );
}

#[test]
fn test_yaml_error_location_survives_trimmed_leading_blanks() {
    let markdown = "~~~\n\n# leading comment\n\n$quill: usaf_memo\n$kind: main\ntitle: Briefing\n\nunit: 88th Communications Squadron: Wright-Patterson AFB\n~~~\n\nBody\n";
    let diag = decompose(markdown).unwrap_err().to_diagnostic();

    let loc = diag.location.expect("the diagnostic carries a location");
    assert_eq!(loc.line, line_of(markdown, "88th Communications"));
    assert_eq!(loc.column, 35);
}

#[test]
fn test_yaml_error_in_composable_card_names_the_block() {
    let markdown = "~~~\n$quill: usaf_memo\n$kind: main\n~~~\n\nBody\n\n~~~\n$kind: note\n# a comment\nunit: a: b\n~~~\n";
    let diag = decompose(markdown).unwrap_err().to_diagnostic();

    assert!(
        diag.message
            .starts_with("YAML error in card-yaml block 1: "),
        "got: {}",
        diag.message
    );
    assert_eq!(diag.args["blockIndex"], serde_json::json!(1));
    let loc = diag.location.expect("the diagnostic carries a location");
    assert_eq!(loc.line, line_of(markdown, "unit: a: b"));
}
