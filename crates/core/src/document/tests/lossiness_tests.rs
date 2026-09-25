use crate::document::{Document, Parsed};

/// Each warning's `(code, path)`.
fn anchors(out: &Parsed) -> Vec<(&str, Option<&str>)> {
    out.warnings
        .iter()
        .map(|w| (w.code.as_deref().unwrap_or(""), w.path.as_deref()))
        .collect()
}

/// Prescan must not record `#`-leading lines inside a literal block as YAML
/// comments: they are the scalar's own text.
#[test]
fn block_scalar_with_markdown_headings_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nbio: |-\n  ## About me\n\n  - first point\n  Plain line.\ntitle: Resume\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    assert_eq!(
        doc.main().payload().get("bio").and_then(|v| v.as_str()),
        Some("## About me\n\n- first point\nPlain line."),
        "markdown heading / bullet / plain lines inside a block scalar must survive parse",
    );
    assert_eq!(
        doc.main().payload().get("title").and_then(|v| v.as_str()),
        Some("Resume"),
    );

    let emitted = doc.to_markdown();
    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(
        doc2.main().payload().get("bio").and_then(|v| v.as_str()),
        Some("## About me\n\n- first point\nPlain line."),
        "block-scalar content must survive a full round-trip\nGot:\n{}",
        emitted
    );
    assert_eq!(emitted, doc2.to_markdown(), "round-trip must be idempotent");
}

#[test]
fn block_scalar_sequence_items_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nsections:\n  - |-\n    ## First\n    body one\n  - |-\n    ## Second\n    body two\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let arr = doc
        .main()
        .payload()
        .get("sections")
        .and_then(|v| v.as_array())
        .expect("sections array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_str(), Some("## First\nbody one"));
    assert_eq!(arr[1].as_str(), Some("## Second\nbody two"));
}

/// A block scalar on a sequence item's dash-line key holds its `#`, `- ` and
/// `key: value` lines as text; the item's next key, at the key's column, ends it.
#[test]
fn block_scalar_on_a_dash_line_key_holds_its_markdown() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\njobs:\n  - details: |\n      # Heading\n      - a\n      b: c\n    title: x\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(!emitted.contains("\n    # Heading"), "no phantom comment\nGot:\n{emitted}");
    let job = &doc.main().payload().get("jobs").unwrap().as_array().unwrap()[0];
    assert_eq!(job["details"].as_str(), Some("# Heading\n- a\nb: c\n"));
    assert_eq!(job["title"].as_str(), Some("x"));
    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(doc, doc2, "Got:\n{emitted}");
    assert_eq!(emitted, doc2.to_markdown(), "round-trip must be idempotent");
}

#[test]
fn unknown_tag_warns_and_is_not_emitted() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nmemo_from: !include value.txt\n~~~\n";
    let out = Document::parse(src).unwrap();
    assert_eq!(
        anchors(&out),
        [("parse::unsupported_yaml_tag", Some("main.memo_from"))]
    );
    assert_eq!(
        out.document.main().payload().get("memo_from").and_then(|v| v.as_str()),
        Some("value.txt")
    );
    let emitted = out.document.to_markdown();
    assert!(!emitted.contains("!include"), "{emitted}");
}

/// A retired `!must_fill` tag held a placeholder, not an answer: the value
/// under a block-style one drops wherever it sits, and each warns at its path,
/// a card's rooted at the card's index among all cards.
#[test]
fn a_retired_fill_marker_nulls_what_it_tags() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n\
               subject: !must_fill Example # string\n\
               recipient: !must_fill # array<string>\n  - Mr. John Doe\n\
               addr:\n  street: !must_fill Main\n  city: Springfield\n\
               x: !must_fill {a: 1}\n\
               to:\n  - name: !must_fill Jane\n    rank: Capt\n~~~\n\n\
               ~~~card-yaml\n$kind: intro\n~~~\n\n\
               ~~~card-yaml\n$kind: note\nsubject: !must_fill Example\n~~~\n";
    let out = Document::parse(src).unwrap();
    let get = |k: &str| out.document.main().payload().get(k).unwrap().as_json().clone();
    assert_eq!(get("subject"), serde_json::Value::Null);
    assert_eq!(get("recipient"), serde_json::Value::Null);
    assert_eq!(get("addr"), serde_json::json!({"street": null, "city": "Springfield"}));
    assert_eq!(get("x"), serde_json::Value::Null);
    assert_eq!(get("to"), serde_json::json!([{"name": null, "rank": "Capt"}]));

    assert_eq!(
        out.document.cards()[1].payload().get("subject").unwrap().as_json(),
        &serde_json::Value::Null
    );
    let dropped = [
        "main.subject",
        "main.recipient",
        "main.addr.street",
        "main.x",
        "main.to[0].name",
        "cards.note[1].subject",
    ];
    assert_eq!(
        anchors(&out),
        dropped.map(|path| ("parse::must_fill_dropped", Some(path)))
    );

    let md = out.document.to_markdown();
    assert!(!md.contains("!must_fill"), "{md}");
    assert!(md.contains("subject: # string\n"), "{md}");
    let again = Document::parse(&md).unwrap();
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    assert_eq!(again.document, out.document, "{md}");
}

/// A `$seed` / `$ext` value is opaque, so a retired marker inside one drops
/// its tag and keeps its value, as any other tag does.
#[test]
fn a_retired_fill_marker_inside_meta_keeps_its_value() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n$ext:\n  ns:\n    to: !must_fill X\n~~~\n";
    let out = Document::parse(src).unwrap();
    assert_eq!(out.document.main().ext().unwrap()["ns"]["to"], "X");
    assert_eq!(anchors(&out), [("parse::unsupported_yaml_tag", None)]);
}

/// A line continuing a flow collection, opened on its key's line or the line
/// below, is the collection's, never a key of the mapping around it, so a tag
/// there stays on its own value.
#[test]
fn a_tag_inside_a_multi_line_flow_collection_stays_on_its_value() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n\
               b: kept\n\
               x: {a: it's,\n  b: !must_fill 2}\n\
               addr:\n  b: kept\n  y:\n    [Why? 'cause,\n   b: !must_fill 2]\n\
               tags: [!t \"a # [\", b]\n\
               c: !must_fill C\n~~~\n";
    let out = Document::parse(src).unwrap();
    let get = |k: &str| out.document.main().payload().get(k).unwrap().as_json().clone();
    assert_eq!(get("b"), "kept");
    assert_eq!(get("x"), serde_json::json!({"a": "it's", "b": 2}));
    assert_eq!(
        get("addr"),
        serde_json::json!({"b": "kept", "y": ["Why? 'cause", {"b": 2}]})
    );
    assert_eq!(get("tags"), serde_json::json!(["a # [", "b"]));
    assert_eq!(get("c"), serde_json::Value::Null);
    assert_eq!(anchors(&out), [("parse::must_fill_dropped", Some("main.c"))]);
}

/// A tag or anchor ahead of `|` or `>` leaves the block's lines its text: no
/// key, comment or marker among them reaches the mapping around it.
#[test]
fn a_block_scalar_behind_a_tag_or_anchor_is_text() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n\
               memo:\n  body: &a |\n    # Summary\n    subject: !must_fill TBD\n  subject: Final\n\
               notes:\n  - !!str >\n    # kept\n~~~\n";
    let out = Document::parse(src).unwrap();
    let get = |k: &str| out.document.main().payload().get(k).unwrap().as_json().clone();
    assert_eq!(
        get("memo"),
        serde_json::json!({"body": "# Summary\nsubject: !must_fill TBD\n", "subject": "Final"})
    );
    assert_eq!(get("notes"), serde_json::json!(["# kept\n"]));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let md = out.document.to_markdown();
    assert_eq!(md.matches("# Summary").count(), 1, "{md}");
    assert_eq!(md.matches("# kept").count(), 1, "{md}");
}

/// The prescan splits on `\n`, so CRLF input reaches it with a trailing `\r` on
/// every line.
#[test]
fn crlf_input_parses_as_its_lf_twin() {
    let lf = "~~~card-yaml\n$quill: q\n$kind: main\n# note\nx: # trailing\ny: keep\n~~~\n\nBody.\n";
    let crlf = lf.replace('\n', "\r\n");

    let lf_out = Document::parse(lf).unwrap();
    let crlf_out = Document::parse(&crlf).unwrap();

    assert!(
        crlf_out.warnings.is_empty(),
        "CRLF input must parse without warnings; got: {:?}",
        crlf_out.warnings
    );
    assert_eq!(
        crlf_out.document, lf_out.document,
        "CRLF and LF input must parse to the same document"
    );
}

/// `key: !must_fill` inside a block or quoted scalar is that scalar's text,
/// kept verbatim.
#[test]
fn fill_marker_text_inside_a_scalar_is_text() {
    let cases = [
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: |\n  see key: !must_fill here\n~~~\n",
            "see key: !must_fill here\n",
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: \"see key: !must_fill here\"\n~~~\n",
            "see key: !must_fill here",
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: \"see\n  key: !must_fill here\"\n~~~\n",
            "see key: !must_fill here",
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote:\n  \"see\n  key: !must_fill here\"\n~~~\n",
            "see key: !must_fill here",
        ),
    ];

    for (src, value) in cases {
        let out = Document::parse(src).unwrap();
        assert!(
            out.warnings.is_empty(),
            "marker text inside a scalar must not warn\nSource:\n{}\nGot: {:?}",
            src,
            out.warnings
        );
        assert_eq!(
            out.document
                .main()
                .payload()
                .get("note")
                .and_then(|v| v.as_str()),
            Some(value),
            "scalar must keep the marker text verbatim\nSource:\n{}",
            src
        );
    }
}

#[test]
fn quoting_normalises_to_canonical_form_with_type_fidelity() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nsingle_q: 'hello'\nunquoted: world\ndouble_q: \"already\"\nambiguous: \"on\"\nnumeric_str: \"01234\"\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();

    assert!(
        !emitted.contains("'hello'"),
        "original single-quote style must not survive\nGot:\n{}",
        emitted
    );

    assert!(
        emitted.contains("\"on\"") || emitted.contains("'on'"),
        "ambiguous string `on` must stay quoted\nGot:\n{}",
        emitted
    );
    assert!(
        emitted.contains("\"01234\"") || emitted.contains("'01234'"),
        "numeric-looking string `01234` must stay quoted\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    for (key, expected) in [
        ("single_q", "hello"),
        ("unquoted", "world"),
        ("double_q", "already"),
        ("ambiguous", "on"),
        ("numeric_str", "01234"),
    ] {
        assert_eq!(
            doc2.main().payload().get(key).and_then(|v| v.as_str()),
            Some(expected),
            "field {key} must round-trip as string {expected:?}",
        );
    }

    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

#[test]
fn comment_position_round_trips() {
    struct Case {
        label: &'static str,
        src: &'static str,
        contains: &'static [&'static str],
        not_contains: &'static [&'static str],
        value_check: Option<(&'static str, &'static str)>,
    }

    let cases = [
        Case {
            label: "top-level own-line comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n# recipient's full name\nrecipient: Jane\nauthor: Alice\n~~~\n\nBody.\n",
            contains: &["# recipient's full name"],
            not_contains: &[],
            value_check: Some(("recipient", "Jane")),
        },
        Case {
            label: "top-level trailing inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\ntitle: My Document # this is a comment\n~~~\n\nBody.\n",
            contains: &["title: My Document # this is a comment"],
            not_contains: &["My Document\n# this is a comment"],
            value_check: Some(("title", "My Document")),
        },
        Case {
            label: "nested sequence comments (leading/between/trailing)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nitems:\n  # before-first\n  - a\n  # between\n  - b\n  # after-last\n~~~\n",
            contains: &["# before-first", "# between", "# after-last"],
            not_contains: &[],
            value_check: None,
        },
        Case {
            label: "nested mapping comments (leading/trailing)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nouter:\n  # leading\n  inner: 1\n  # trailing\n~~~\n",
            contains: &["# leading", "# trailing"],
            not_contains: &[],
            value_check: None,
        },
        Case {
            label: "nested sequence item trailing inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nitems:\n  - a # inline\n  - b\n~~~\n",
            contains: &["- a # inline"],
            not_contains: &[],
            value_check: None,
        },
        Case {
            label: "nested mapping field trailing inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nouter:\n  inner: 1 # tail\n~~~\n",
            contains: &["inner: 1 # tail"],
            not_contains: &[],
            value_check: None,
        },
        Case {
            label: "inline comment on a container key",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nouter: # describes outer\n  inner: 1\n~~~\n",
            contains: &["outer: # describes outer\n  inner: 1"],
            not_contains: &[],
            value_check: None,
        },
        Case {
            label: "own-line comment below $quill header (root payload)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n# main entry\ntitle: Hi\n~~~\n",
            contains: &["~~~\n$quill: q\n$kind: main\n# main entry\n"],
            not_contains: &[],
            value_check: None,
        },
        Case {
            label: "own-line comment below $kind header (card payload)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: foo\n# the foo card\nx: 1\n~~~\n",
            contains: &["~~~\n$kind: foo\n# the foo card\n"],
            not_contains: &[],
            value_check: None,
        },
        Case {
            label: "own-line comments flanking an inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n# header\ntitle: Hi # tail\n# footer\n~~~\n",
            contains: &["# header\n", "title: Hi # tail\n", "# footer\n"],
            not_contains: &[],
            value_check: None,
        },
    ];

    for case in cases {
        let emitted = Document::parse(case.src).unwrap().document.to_markdown();
        for needle in case.contains {
            assert!(
                emitted.contains(needle),
                "[{}] comment must survive round-trip at its position\nGot:\n{}",
                case.label,
                emitted
            );
        }
        for needle in case.not_contains {
            assert!(
                !emitted.contains(needle),
                "[{}] comment must not degrade to a different position\nGot:\n{}",
                case.label,
                emitted
            );
        }

        let doc2 = Document::parse(&emitted).unwrap().document;
        if let Some((field, expected)) = case.value_check {
            assert_eq!(
                doc2.main().payload().get(field).and_then(|v| v.as_str()),
                Some(expected),
                "[{}] value must remain intact after round-trip",
                case.label
            );
        }

        let emitted2 = doc2.to_markdown();
        assert_eq!(
            emitted, emitted2,
            "[{}] round-trip must be idempotent",
            case.label
        );
    }
}

#[test]
fn orphan_inline_after_remove_degrades_to_own_line() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nfield: value # tail\nother: 2\n~~~\n";

    let mut doc = Document::parse(src).unwrap().document;
    doc.main_mut().payload_mut().remove("field");

    let emitted = doc.to_markdown();
    assert!(
        emitted.lines().any(|line| line == "# tail"),
        "orphan comment must stand on its own line\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(
        emitted, emitted2,
        "post-orphan round-trip must be idempotent"
    );
}

#[test]
fn inline_on_empty_mapping_rides_on_the_braces() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nempty: {} # notes about empty\n~~~\n";
    let doc = Document::parse(src).unwrap().document;

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("empty: {} # notes about empty\n"),
        "empty mapping keeps its key and its inline trailer\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(doc, doc2, "empty mapping must survive the round-trip");
    assert_eq!(emitted, doc2.to_markdown(), "round-trip must be idempotent");
}

#[test]
fn nested_empty_mapping_survives_round_trip() {
    use crate::value::QuillValue;

    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n";
    let mut doc = Document::parse(src).unwrap().document;
    doc.main_mut()
        .store_field(
            "cfg",
            QuillValue::from_json(serde_json::json!({ "opts": {} })),
        )
        .unwrap();

    let emitted = doc.to_markdown();
    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(doc, doc2, "nested empty mapping must not become null\nGot:\n{emitted}");
    assert_eq!(emitted, doc2.to_markdown(), "round-trip must be idempotent");
}

/// A comment line ends a block scalar; it is not a blank line inside one. Under
/// keep chomping (`|+` / `>+`) a blank line there would be content, so the
/// distinction is the value, not just the numbering.
#[test]
fn a_comment_after_a_kept_block_scalar_adds_no_line_to_it() {
    for marker in ["|+", ">+"] {
        let src = format!(
            "~~~card-yaml\n$quill: q\n$kind: main\nbio: {marker}\n  text\n# after\nnext: x\n~~~\n"
        );
        let doc = Document::parse(&src).unwrap().document;
        let fm = doc.main().payload();
        assert_eq!(
            fm.get("bio").unwrap().as_str(),
            Some("text\n"),
            "`{marker}` keeps only the newlines the source held"
        );
        assert_eq!(fm.get("next").unwrap().as_str(), Some("x"));
        assert!(
            doc.to_markdown().contains("# after"),
            "the comment survives\nGot:\n{}",
            doc.to_markdown()
        );
    }
}

/// A comment ends a multi-line plain scalar, as it does in YAML, so what
/// follows it is a mapping line the parser refuses — rather than a fold that
/// quietly invents `"aaa bbb"` from a document no YAML parser accepts. The
/// refusal anchors at the offending source line.
#[test]
fn a_comment_inside_a_plain_scalar_is_a_located_refusal() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nkey: aaa\n  # c\n  bbb\n~~~\n";
    let err = Document::parse(src).expect_err("the continuation is not a mapping entry");
    let crate::error::ParseError::YamlErrorWithLocation { line, column, .. } = err else {
        panic!("expected a located YAML error, got {err:?}");
    };
    assert_eq!((line, column), (6, 3), "anchored at `  bbb` in the source");
}
