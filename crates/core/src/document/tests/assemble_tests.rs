use crate::document::assemble::decompose;
use crate::document::Document;

#[test]
fn test_empty_input_dedicated_error() {
    for input in ["", "   ", "\n\n\t\n"] {
        let err = decompose(input).unwrap_err().to_string();
        assert!(
            err.contains("Empty markdown input"),
            "expected dedicated empty-input message for {input:?}, got: {err}"
        );
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

#[test]
fn test_root_dash_frontmatter_without_quill_reports_missing_quill() {
    let err = decompose("---\nquill: usaf_memo\ntitle: Memo\n---\n\nBody\n").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("must declare `$quill: <name>`"), "got: {msg}");
    assert!(!msg.contains("`---` YAML frontmatter"), "stale hint: {msg}");
    assert!(
        !msg.contains("Replace the opening `---`"),
        "stale hint: {msg}"
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
    let markdown = "~~~\ntitle: Memo\n\nThe body.\n";
    let msg = decompose(markdown).unwrap_err().to_string();
    assert!(msg.contains("Missing required root"), "got: {msg}");
}

#[test]
fn test_root_opener_with_foreign_info_string_parses_as_the_bare_form() {
    let foreign = "~~~metadata\n$quill: usaf_memo@0.3.0\n$kind: main\n~~~\n\nBody.\n";
    let bare = "~~~\n$quill: usaf_memo@0.3.0\n$kind: main\n~~~\n\nBody.\n";
    let foreign_doc = decompose(foreign).expect("any info string opens the root block");
    let bare_doc = decompose(bare).expect("bare root block parses");
    assert_eq!(foreign_doc, bare_doc);
}

#[test]
fn test_dash_root_block_parses_equivalent_to_card_yaml() {
    let dash_md = "---\n$quill: test_quill\n$kind: main\ntitle: Test\n---\n\nBody.";
    let canonical_md = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\nBody.";
    let dash_doc = decompose(dash_md).expect("--- root block should parse");
    let canonical_doc = decompose(canonical_md).expect("canonical root block parses");
    assert_eq!(dash_doc, canonical_doc);
    assert_eq!(dash_doc.quill_reference().name, "test_quill");
    assert_eq!(
        dash_doc
            .main()
            .payload()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test"
    );
    assert_eq!(dash_doc.main().body_markdown(), "Body.");
}

#[test]
fn test_dash_root_block_emits_canonical_card_yaml() {
    let dash_md = "---\n$quill: test_quill\n$kind: main\ntitle: Test\n---\n\nBody.";
    let doc = decompose(dash_md).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.starts_with("~~~\n"),
        "expected canonical opener, got: {emitted:?}"
    );
    assert!(
        !emitted.contains("---\n"),
        "stray dash fence in emit: {emitted:?}"
    );
}

#[test]
fn test_dash_root_with_composable_card_yaml_parses() {
    let markdown = "---\n$quill: test_quill\n$kind: main\ntitle: Test\n---\n\nBody.\n\n\
                    ~~~card-yaml\n$kind: note\nlabel: a\n~~~\n\nNote body.";
    let doc = decompose(markdown).expect("mixed shape should parse");
    assert_eq!(doc.quill_reference().name, "test_quill");
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
    assert_eq!(doc.cards()[0].body_markdown(), "Note body.");
}

#[test]
fn test_dash_opener_in_composable_card_position_errors() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\nBody.\n\n\
                    ---\n$kind: note\nlabel: a\n---\n\nNote body.";
    let err = decompose(markdown).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("composable cards") || msg.contains("Composable card"),
        "expected composable-card rejection, got: {msg}"
    );
    assert!(msg.contains("~~~"), "got: {msg}");
}

#[test]
fn test_dash_opener_with_tilde_closer_falls_through() {
    let markdown = "---\n$quill: test_quill\n$kind: main\ntitle: T\n~~~\n\nBody.";
    let err = decompose(markdown).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Missing required root"), "got: {msg}");
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

#[test]
fn test_with_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author
~~~

# Hello World

This is the body.";

    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main().body_markdown(),
        "# Hello World\n\nThis is the body."
    );
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test Document"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Author"
    );
    assert_eq!(doc.main().payload().len(), 2); // title, author
    assert_eq!(doc.cards().len(), 0);
    assert_eq!(doc.quill_reference().name, "test_quill");
}

#[test]
fn test_complex_yaml_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Complex Document
tags:
  - test
  - yaml
metadata:
  version: 1.0
  nested:
    field: value
~~~

Content here.";

    let doc = decompose(markdown).unwrap();

    assert_eq!(doc.main().body_markdown(), "Content here.");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Complex Document"
    );

    let tags = doc
        .main()
        .payload()
        .get("tags")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].as_str().unwrap(), "test");
    assert_eq!(tags[1].as_str().unwrap(), "yaml");
}

#[test]
fn test_invalid_yaml() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: [invalid yaml
author: missing close bracket
~~~

Content here.";

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("YAML error"));
}

#[test]
fn test_unclosed_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
author: Test Author

Content without closing fence";

    let msg = decompose(markdown).unwrap_err().to_string();
    assert!(
        msg.contains("Root card-yaml block opened at line 1 is never closed"),
        "got: {msg}"
    );
    assert!(msg.contains("after the last field (`author`)"), "got: {msg}");
}

#[test]
fn test_basic_card_block() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Main Document
~~~

Main body content.

~~~card-yaml
$kind: items
name: Item 1
~~~

Body of item 1.";

    let doc = decompose(markdown).unwrap();

    assert_eq!(doc.main().body_markdown(), "Main body content.");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Main Document"
    );

    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("items"));
    assert_eq!(
        card.payload().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );
    assert_eq!(card.body_markdown(), "Body of item 1.");
}

#[test]
fn cards_parse_with_correct_kind_payload_and_order() {
    enum Expect {
        Str(&'static str),
        I64(i64),
        F64(f64),
    }

    fn check_field(payload: &crate::document::Payload, key: &str, expect: &Expect, ctx: &str) {
        let v = payload
            .get(key)
            .unwrap_or_else(|| panic!("{ctx}: missing field {key:?}"));
        match expect {
            Expect::Str(s) => assert_eq!(v.as_str().unwrap(), *s, "{ctx}: field {key:?}"),
            Expect::I64(i) => assert_eq!(v.as_i64().unwrap(), *i, "{ctx}: field {key:?}"),
            Expect::F64(f) => assert_eq!(v.as_f64().unwrap(), *f, "{ctx}: field {key:?}"),
        }
    }

    struct ExpectedCard {
        kind: Option<&'static str>,
        fields: Vec<(&'static str, Expect)>,
        body: Option<&'static str>,
    }

    struct Case {
        name: &'static str,
        markdown: &'static str,
        quill: Option<&'static str>,
        main_fields: Vec<(&'static str, Expect)>,
        main_payload_len: Option<usize>,
        main_body_eq: Option<&'static str>,
        main_body_contains: Vec<&'static str>,
        cards: Vec<ExpectedCard>,
    }

    let cases = vec![
        Case {
            name: "multiple_card_blocks",
            markdown: "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item 1
tags: [a, b]
~~~

First item body.

~~~card-yaml
$kind: items
name: Item 2
tags: [c, d]
~~~

Second item body.",
            quill: None,
            main_fields: vec![],
            main_payload_len: None,
            main_body_eq: None,
            main_body_contains: vec![],
            cards: vec![
                ExpectedCard {
                    kind: Some("items"),
                    fields: vec![("name", Expect::Str("Item 1"))],
                    body: None,
                },
                ExpectedCard {
                    kind: Some("items"),
                    fields: vec![("name", Expect::Str("Item 2"))],
                    body: None,
                },
            ],
        },
        Case {
            name: "mixed_global_and_cards",
            markdown: "~~~card-yaml
$quill: test_quill
$kind: main
title: Global
author: John Doe
~~~

Global body.

~~~card-yaml
$kind: sections
title: Section 1
~~~

Section 1 content.

~~~card-yaml
$kind: sections
title: Section 2
~~~

Section 2 content.",
            quill: None,
            main_fields: vec![("title", Expect::Str("Global"))],
            main_payload_len: None,
            main_body_eq: Some("Global body."),
            main_body_contains: vec![],
            cards: vec![
                ExpectedCard {
                    kind: Some("sections"),
                    fields: vec![],
                    body: None,
                },
                ExpectedCard {
                    kind: None,
                    fields: vec![],
                    body: None,
                },
            ],
        },
        Case {
            name: "adjacent_blocks_different_kinds",
            markdown: "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item 1
~~~

Item 1 body

~~~card-yaml
$kind: sections
title: Section 1
~~~

Section 1 body",
            quill: None,
            main_fields: vec![],
            main_payload_len: None,
            main_body_eq: None,
            main_body_contains: vec![],
            cards: vec![
                ExpectedCard {
                    kind: Some("items"),
                    fields: vec![("name", Expect::Str("Item 1"))],
                    body: None,
                },
                ExpectedCard {
                    kind: Some("sections"),
                    fields: vec![("title", Expect::Str("Section 1"))],
                    body: None,
                },
            ],
        },
        Case {
            name: "order_preservation",
            markdown: "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
id: 1
~~~

First

~~~card-yaml
$kind: items
id: 2
~~~

Second

~~~card-yaml
$kind: items
id: 3
~~~

Third",
            quill: None,
            main_fields: vec![],
            main_payload_len: None,
            main_body_eq: None,
            main_body_contains: vec![],
            cards: vec![
                ExpectedCard {
                    kind: Some("items"),
                    fields: vec![("id", Expect::I64(1))],
                    body: None,
                },
                ExpectedCard {
                    kind: Some("items"),
                    fields: vec![("id", Expect::I64(2))],
                    body: None,
                },
                ExpectedCard {
                    kind: Some("items"),
                    fields: vec![("id", Expect::I64(3))],
                    body: None,
                },
            ],
        },
        Case {
            name: "product_catalog_integration",
            markdown: "~~~card-yaml
$quill: test_quill
$kind: main
title: Product Catalog
author: John Doe
date: 2024-01-01
~~~

This is the main catalog description.

~~~card-yaml
$kind: products
name: Widget A
price: 19.99
sku: WID-001
~~~

The **Widget A** is our most popular product.

~~~card-yaml
$kind: products
name: Gadget B
price: 29.99
sku: GAD-002
~~~

The **Gadget B** is perfect for professionals.

~~~card-yaml
$kind: reviews
product: Widget A
rating: 5
~~~

\"Excellent product! Highly recommended.\"

~~~card-yaml
$kind: reviews
product: Gadget B
rating: 4
~~~

\"Very good, but a bit pricey.\"",
            quill: None,
            main_fields: vec![
                ("title", Expect::Str("Product Catalog")),
                ("author", Expect::Str("John Doe")),
                ("date", Expect::Str("2024-01-01")),
            ],
            main_payload_len: Some(3),
            main_body_eq: None,
            main_body_contains: vec!["main catalog description"],
            cards: vec![
                ExpectedCard {
                    kind: Some("products"),
                    fields: vec![
                        ("name", Expect::Str("Widget A")),
                        ("price", Expect::F64(19.99)),
                    ],
                    body: None,
                },
                ExpectedCard {
                    kind: Some("products"),
                    fields: vec![("name", Expect::Str("Gadget B"))],
                    body: None,
                },
                ExpectedCard {
                    kind: Some("reviews"),
                    fields: vec![
                        ("product", Expect::Str("Widget A")),
                        ("rating", Expect::I64(5)),
                    ],
                    body: None,
                },
                ExpectedCard {
                    kind: None,
                    fields: vec![],
                    body: None,
                },
            ],
        },
        Case {
            name: "quill_with_card_blocks",
            markdown: "~~~card-yaml
$quill: document
$kind: main
title: Test Document
~~~

Main body.

~~~card-yaml
$kind: sections
name: Section 1
~~~

Section 1 body.",
            quill: Some("document"),
            main_fields: vec![("title", Expect::Str("Test Document"))],
            main_payload_len: None,
            main_body_eq: Some("Main body."),
            main_body_contains: vec![],
            cards: vec![ExpectedCard {
                kind: Some("sections"),
                fields: vec![],
                body: None,
            }],
        },
        Case {
            name: "card_consecutive_blocks",
            markdown: "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: a
id: 1
~~~

~~~card-yaml
$kind: a
id: 2
~~~",
            quill: None,
            main_fields: vec![],
            main_payload_len: None,
            main_body_eq: None,
            main_body_contains: vec![],
            cards: vec![
                ExpectedCard {
                    kind: Some("a"),
                    fields: vec![],
                    body: None,
                },
                ExpectedCard {
                    kind: Some("a"),
                    fields: vec![],
                    body: None,
                },
            ],
        },
        Case {
            name: "spec_example",
            markdown: "~~~card-yaml
$quill: blog_post
$kind: main
title: My Document
~~~

Main document body.

***

More content after horizontal rule.

~~~card-yaml
$kind: section
heading: Introduction
~~~

Introduction content.

~~~card-yaml
$kind: section
heading: Conclusion
~~~

Conclusion content.
",
            quill: Some("blog_post"),
            main_fields: vec![("title", Expect::Str("My Document"))],
            main_payload_len: None,
            main_body_eq: None,
            main_body_contains: vec![
                "Main document body.",
                "More content after horizontal rule.",
            ],
            cards: vec![
                ExpectedCard {
                    kind: Some("section"),
                    fields: vec![("heading", Expect::Str("Introduction"))],
                    body: Some("Introduction content."),
                },
                ExpectedCard {
                    kind: Some("section"),
                    fields: vec![("heading", Expect::Str("Conclusion"))],
                    body: Some("Conclusion content."),
                },
            ],
        },
    ];

    for case in &cases {
        let doc = decompose(case.markdown)
            .unwrap_or_else(|e| panic!("{}: parse failed: {e}", case.name));

        if let Some(quill) = case.quill {
            assert_eq!(
                doc.quill_reference().name,
                quill,
                "{}: quill name",
                case.name
            );
        }
        for (key, expect) in &case.main_fields {
            check_field(
                doc.main().payload(),
                key,
                expect,
                &format!("{}: main", case.name),
            );
        }
        if let Some(len) = case.main_payload_len {
            assert_eq!(
                doc.main().payload().len(),
                len,
                "{}: main payload len",
                case.name
            );
        }
        if let Some(body) = case.main_body_eq {
            assert_eq!(doc.main().body_markdown(), body, "{}: main body", case.name);
        }
        for needle in &case.main_body_contains {
            assert!(
                doc.main().body_markdown().contains(needle),
                "{}: main body missing {needle:?}",
                case.name
            );
        }

        assert_eq!(
            doc.cards().len(),
            case.cards.len(),
            "{}: cards len",
            case.name
        );
        for (i, expected) in case.cards.iter().enumerate() {
            let card = &doc.cards()[i];
            let ctx = format!("{}: card[{i}]", case.name);
            if let Some(kind) = expected.kind {
                assert_eq!(card.kind(), Some(kind), "{ctx} kind");
            }
            for (key, expect) in &expected.fields {
                check_field(card.payload(), key, expect, &ctx);
            }
            if let Some(body) = expected.body {
                assert_eq!(card.body_markdown(), body, "{ctx} body");
            }
        }
    }
}

#[test]
fn test_empty_card_metadata() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
~~~

Body without metadata.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("items"));
    assert!(card.payload().is_empty());
    assert_eq!(card.body_markdown(), "Body without metadata.");
}

#[test]
fn test_card_block_without_body() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item
~~~";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("items"));
    assert_eq!(card.body_markdown(), ""); // empty, not absent
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
fn test_delimiter_inside_fenced_code_block_backticks() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
~~~

Here is some code:

```yaml
~~~card-yaml
$kind: code_example
fake: payload
~~~
```

More content.
";

    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body_markdown().contains("fake: payload"));
    assert!(doc.main().payload().get("fake").is_none());
    assert_eq!(doc.cards().len(), 0);
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
fn test_root_with_non_main_kind_is_error() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: other
title: Test
~~~";
    let err = decompose(markdown).unwrap_err().to_string();
    assert!(
        err.contains("$kind: other") && err.contains("reserved for the document root"),
        "expected non-main-root error, got: {err}"
    );
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
    let canonical = "~~~\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\nBody.\n";
    let doc = decompose(canonical).unwrap();
    assert_eq!(doc.to_markdown(), canonical);
}

#[test]
fn test_non_root_block_declaring_quill_is_error() {
    let markdown = "~~~card-yaml
$quill: first
$kind: main
~~~

~~~card-yaml
$quill: second
$kind: note
~~~";

    let err = decompose(markdown).unwrap_err().to_string();
    assert!(err.contains("must not declare `$quill`"), "got: {err}");
}

#[test]
fn test_quill_empty_value() {
    let markdown = "~~~card-yaml
$quill:
~~~";

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid $quill reference"));
}

#[test]
fn test_card_with_unknown_meta_key_is_error() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$foo: bar
$kind: note
~~~";

    let err = decompose(markdown).unwrap_err().to_string();
    assert!(
        err.contains("Unknown `$foo`"),
        "expected unknown-key parse error, got: {err}"
    );
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

#[test]
fn fill_on_dollar_key_is_rejected() {
    let markdown = "~~~card-yaml
$quill: !must_fill test_quill
$kind: main
~~~";
    let err = decompose(markdown).unwrap_err().to_string();
    assert!(
        err.contains("`!must_fill`") && err.contains("$quill"),
        "expected !must_fill-on-$ rejection, got: {err}"
    );
}

#[test]
fn test_blank_lines_in_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author

description: This has a blank line above it
tags:
  - one
  - two
~~~

# Hello World

This is the body.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().body_markdown(),
        "# Hello World\n\nThis is the body."
    );
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test Document"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Author"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("description")
            .unwrap()
            .as_str()
            .unwrap(),
        "This has a blank line above it"
    );
    let tags = doc
        .main()
        .payload()
        .get("tags")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn test_triple_dash_between_paragraphs_is_delegated() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
~~~

First paragraph.

---

Second paragraph.";

    let doc = decompose(markdown).unwrap();
    let body = doc.main().body_markdown();
    assert!(body.contains("First paragraph."));
    assert!(body.contains("Second paragraph."));
    assert!(doc.cards().is_empty(), "--- must not split a card");
}

#[test]
fn test_extended_metadata_demo_file() {
    let markdown = include_str!("../../../../fixtures/resources/extended_metadata_demo.md");
    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Extended Metadata Demo"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Quillmark Team"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("version")
            .unwrap()
            .as_f64()
            .unwrap(),
        1.0
    );

    assert!(doc
        .main()
        .body_markdown()
        .contains("card-yaml metadata format"));

    assert_eq!(doc.cards().len(), 5);

    let features_count = doc
        .cards()
        .iter()
        .filter(|c| c.kind() == Some("features"))
        .count();
    let use_cases_count = doc
        .cards()
        .iter()
        .filter(|c| c.kind() == Some("use_cases"))
        .count();
    assert_eq!(features_count, 3);
    assert_eq!(use_cases_count, 2);

    assert_eq!(doc.cards()[0].kind(), Some("features"));
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("name")
            .unwrap()
            .as_str()
            .unwrap(),
        "Tag Directives"
    );
}

#[test]
fn test_input_size_limit() {
    let size = crate::error::MAX_INPUT_SIZE + 1;
    let large_markdown = "a".repeat(size);

    let result = decompose(&large_markdown);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Input too large"));
}

#[test]
fn test_yaml_size_limit() {
    let mut markdown = String::from("~~~card-yaml\n$quill: test_quill\n$kind: main\n");
    let size = crate::error::MAX_YAML_SIZE + 1;
    markdown.push_str("data: \"");
    markdown.push_str(&"x".repeat(size));
    markdown.push_str("\"\n~~~\n\nBody");

    let result = decompose(&markdown);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Input too large"));
}

#[test]
fn test_chevrons_preserved_in_all_contexts() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test <<with chevrons>>
items:
  - \"<<first>>\"
  - \"<<second>>\"
metadata:
  description: \"<<nested value>>\"
~~~

<<body>> text.

```
<<in code block>>
```

`<<inline code>>` and <<plain>>

~~~card-yaml
$kind: items
description: \"<<card yaml>>\"
~~~

Use <<card body>> here.";

    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test <<with chevrons>>"
    );
    let items = doc
        .main()
        .payload()
        .get("items")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(items[0].as_str().unwrap(), "<<first>>");
    assert_eq!(items[1].as_str().unwrap(), "<<second>>");
    let metadata = doc
        .main()
        .payload()
        .get("metadata")
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(
        metadata.get("description").unwrap().as_str().unwrap(),
        "<<nested value>>"
    );

    // Code contexts protect chevrons verbatim; plain-text `<<word>>` reads as an
    // inline HTML tag per CommonMark and projects away.
    let body = doc.main().body_markdown();
    assert_eq!(
        body,
        "\\<> text.\n\n```\n<<in code block>>\n```\n\n`<<inline code>>` and \\<>"
    );

    let card = &doc.cards()[0];
    assert_eq!(
        card.payload().get("description").unwrap().as_str().unwrap(),
        "<<card yaml>>"
    );
    assert_eq!(card.body_markdown(), "Use \\<> here.");
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

#[test]
fn test_payload_at_eof_no_trailing_newline() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test"
    );
    assert_eq!(doc.main().body_markdown(), "");
}

#[test]
fn test_unicode_in_yaml_keys() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitre: Bonjour\nタイトル: こんにちは\n~~~\n\nBody.";
    let err = decompose(markdown).unwrap_err();
    assert!(
        err.to_string().contains("field names must match"),
        "non-ASCII field name is a parse error: {err}"
    );

    let ok = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitre: こんにちは\n~~~\n";
    let doc = decompose(ok).unwrap();
    assert_eq!(
        doc.main().payload().get("titre").unwrap().as_str().unwrap(),
        "こんにちは"
    );
}

#[test]
fn single_field_yaml_scalar_types() {
    enum Check {
        StrEq(&'static str),
        StrContains(&'static [&'static str]),
        I64Eq(i64),
        F64Eq(f64),
        BoolEq(bool),
    }

    let cases: &[(&str, &str, &[(&str, Check)])] = &[
        (
            "literal block scalar (`|`)",
            "~~~card-yaml
$quill: test_quill
$kind: main
description: |
  This is a
  multiline string
  with preserved newlines.
~~~

Body.",
            &[(
                "description",
                Check::StrContains(&["multiline string", "\n"]),
            )],
        ),
        (
            "folded block scalar (`>`)",
            "~~~card-yaml
$quill: test_quill
$kind: main
description: >
  This is a folded
  string that becomes
  a single line.
~~~

Body.",
            &[("description", Check::StrContains(&["folded"]))],
        ),
        (
            "empty string",
            "~~~card-yaml\n$quill: test_quill\n$kind: main\nempty: \"\"\n~~~\n\nBody.",
            &[("empty", Check::StrEq(""))],
        ),
        (
            "special characters in a quoted string",
            "~~~card-yaml\n$quill: test_quill\n$kind: main\nspecial: \"colon: here, and [brackets]\"\n~~~\n\nBody.",
            &[(
                "special",
                Check::StrEq("colon: here, and [brackets]"),
            )],
        ),
        (
            "int/float/bool scalars",
            "~~~card-yaml
$quill: test_quill
$kind: main
count: 42
price: 19.99
active: true
items:
  - first
  - 100
  - true
~~~

Body.",
            &[
                ("count", Check::I64Eq(42)),
                ("price", Check::F64Eq(19.99)),
                ("active", Check::BoolEq(true)),
            ],
        ),
    ];

    for (label, markdown, fields) in cases {
        let doc = decompose(markdown).unwrap_or_else(|e| panic!("{label}: parse failed: {e}"));
        for (key, check) in *fields {
            let v = doc
                .main()
                .payload()
                .get(key)
                .unwrap_or_else(|| panic!("{label}: missing field {key:?}"));
            match check {
                Check::StrEq(s) => assert_eq!(v.as_str().unwrap(), *s, "{label}: {key}"),
                Check::StrContains(needles) => {
                    let s = v.as_str().unwrap();
                    for n in *needles {
                        assert!(s.contains(n), "{label}: {key} missing {n:?} in {s:?}");
                    }
                }
                Check::I64Eq(i) => assert_eq!(v.as_i64().unwrap(), *i, "{label}: {key}"),
                Check::F64Eq(f) => assert_eq!(v.as_f64().unwrap(), *f, "{label}: {key}"),
                Check::BoolEq(b) => assert_eq!(v.as_bool().unwrap(), *b, "{label}: {key}"),
            }
        }
    }
}

#[test]
fn test_invalid_card_kind_names_are_rejected() {
    for kind in ["ITEMS", "123items", "my-items", "Invalid-Name", ""] {
        let markdown = format!(
            "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: {kind}\n~~~\n\nBody."
        );
        let err = decompose(&markdown).unwrap_err().to_string();
        assert!(
            err.contains("Invalid `$kind`"),
            "kind {kind:?} should be rejected; got: {err}"
        );
    }
}

#[test]
fn test_body_with_leading_newlines() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\n\n\nBody with leading newlines.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body_markdown(), "Body with leading newlines.");
}

#[test]
fn test_body_with_trailing_newlines() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\nBody.\n\n\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body_markdown(), "Body.");
}

#[test]
fn test_blank_separator_strip_global_body_followed_by_card_lf() {
    let markdown =
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nbody\n\n~~~card-yaml\n$kind: x\n~~~\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body_markdown(), "body");
}

#[test]
fn test_blank_separator_strip_card_body_followed_by_card() {
    let markdown = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: a\n~~~\n\nfirst\n\n~~~card-yaml\n$kind: b\n~~~\n\nsecond\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards()[0].body_markdown(), "first");
    assert_eq!(doc.cards()[1].body_markdown(), "second");
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
fn test_allowed_card_field_collision() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
my_card: \"some global value\"
~~~

~~~card-yaml
$kind: my_card
title: \"My Card\"
~~~

Body
";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .payload()
            .get("my_card")
            .unwrap()
            .as_str()
            .unwrap(),
        "some global value"
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("my_card"));
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "My Card"
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
    use crate::{Card, Payload};

    let mut doc = Document::parse("~~~card-yaml\n$quill: my_quill\n$kind: main\n~~~\n\nBody.\n")
        .unwrap()
        .document;
    doc.cards_vec_mut().push(Card::from_parts(
        Payload::new(),
        quillmark_content::Normalized::empty(),
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
fn card_id_is_rejected_as_an_unknown_system_key() {
    let md = "~~~\n$quill: q@0.1\n~~~\n\n~~~\n$kind: note\n$id: a\n~~~\n";
    let err = Document::parse(md).unwrap_err().to_string();
    assert!(
        err.contains("Unknown `$id`"),
        "expected unknown-key parse error, got: {err}"
    );

    let root = "~~~\n$quill: q@0.1\n$id: x\n~~~\n";
    assert!(Document::parse(root)
        .unwrap_err()
        .to_string()
        .contains("Unknown `$id`"));
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
    assert!(
        !diag.message.contains("(block 0)"),
        "stale block suffix: {}",
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
