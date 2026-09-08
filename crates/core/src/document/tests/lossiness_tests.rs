use crate::document::Document;

/// The prescan comment-stripper must not treat `#`-leading lines inside a
/// literal block as YAML comments.
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

#[test]
fn custom_tags_lose_tag_but_keep_value() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nmemo_from: !must_fill 2d lt example\n~~~\n";
    let doc = Document::parse(src).unwrap().document;

    let fm = doc.main().payload();
    assert_eq!(
        fm.get("memo_from").and_then(|v| v.as_str()),
        Some("2d lt example"),
        "string value must survive tag parsing"
    );
    assert!(fm.is_fill("memo_from"), "fill marker must be recorded");

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("memo_from: !must_fill"),
        "`!must_fill` tag must round-trip\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    assert!(
        doc2.main().payload().is_fill("memo_from"),
        "fill marker must survive a full round-trip"
    );

    let src2 = "~~~card-yaml\n$quill: q\n$kind: main\nmemo_from: !include value.txt\n~~~\n";
    let out = Document::parse(src2).unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("parse::unsupported_yaml_tag")),
        "expected unsupported_yaml_tag warning; got: {:?}",
        out.warnings
    );
    let emitted2 = out.document.to_markdown();
    assert!(
        !emitted2.contains("!include"),
        "unknown tag must not re-appear on emit\nGot:\n{}",
        emitted2
    );
}

/// The YAML parser drops a marker in these positions silently, so prescan warns
/// instead.
#[test]
fn unsupported_fill_position_warns_not_silently_dropped() {
    let code = "parse::fill_marker_unsupported_position";
    let warns = |src: &str| {
        Document::parse(src)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.code.as_deref() == Some(code))
    };

    assert!(
        warns("~~~card-yaml\n$quill: q\n$kind: main\naddr: {street: !must_fill, city: x}\n~~~\n"),
        "flow-map marker must warn"
    );
    assert!(
        warns("~~~card-yaml\n$quill: q\n$kind: main\ntags: [!must_fill, a]\n~~~\n"),
        "flow-sequence marker must warn"
    );
    assert!(
        warns("~~~card-yaml\n$quill: q\n$kind: main\ntags:\n  - !must_fill\n  - a\n~~~\n"),
        "bare sequence-element marker must warn"
    );
    assert!(
        warns("~~~card-yaml\n$quill: q\n$kind: main\naddr:\n  street: {n: !must_fill}\n~~~\n"),
        "marker in a nested flow value must warn"
    );
    assert!(
        warns("~~~card-yaml\n$quill: q\n$kind: main\nto:\n  - name: [!must_fill]\n~~~\n"),
        "marker in a flow value on a sequence-item line must warn"
    );
    assert!(
        !warns(
            "~~~card-yaml\n$quill: q\n$kind: main\naddr:\n  street: !must_fill\n  city: x\n~~~\n"
        ),
        "block-style nested marker must not warn"
    );
    assert!(
        !warns("~~~card-yaml\n$quill: q\n$kind: main\nnote: \"see !must_fill docs\"\n~~~\n"),
        "quoted literal must not warn"
    );
}

/// A `$seed` / `$ext` value carries no fill markers, so a marker nested inside
/// one is dropped like any other unpreservable position.
#[test]
fn fill_marker_inside_meta_value_warns_with_its_path() {
    let cases = [
        (
            "~~~card-yaml\n$quill: q\n$kind: main\n$seed:\n  note:\n    from: !must_fill\n    to: !must_fill X\n~~~\n",
            ["$seed.note.from", "$seed.note.to"],
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\n$ext:\n  ns:\n    from: !must_fill\n    to: !must_fill X\n~~~\n",
            ["$ext.ns.from", "$ext.ns.to"],
        ),
    ];

    for (src, paths) in cases {
        let out = Document::parse(src).unwrap();
        let named: Vec<&str> = out
            .warnings
            .iter()
            .filter(|w| w.code.as_deref() == Some("parse::fill_marker_unsupported_position"))
            .map(|w| w.message.as_str())
            .collect();
        for path in paths {
            assert!(
                named.iter().any(|m| m.contains(path)),
                "a marker at `{}` must warn\nGot: {:?}",
                path,
                named
            );
        }
    }
}

/// The prescan splits on `\n`, so CRLF input reaches it with a trailing `\r` on
/// every line.
#[test]
fn crlf_input_parses_as_its_lf_twin() {
    let lf = "~~~card-yaml\n$quill: q\n$kind: main\n# note\nx: !must_fill\ny: keep\n~~~\n\nBody.\n";
    let crlf = lf.replace('\n', "\r\n");

    let lf_out = Document::parse(lf).unwrap();
    let crlf_out = Document::parse(&crlf).unwrap();

    assert!(
        crlf_out.warnings.is_empty(),
        "CRLF input must parse without warnings; got: {:?}",
        crlf_out.warnings
    );
    assert!(
        crlf_out.document.main().payload().is_fill("x"),
        "a bare `!must_fill` must be recognised under CRLF"
    );
    assert_eq!(
        crlf_out.document, lf_out.document,
        "CRLF and LF input must parse to the same document"
    );
}

/// `key: !must_fill` inside a block or quoted scalar is that scalar's text,
/// kept verbatim.
#[test]
fn fill_marker_text_inside_a_scalar_does_not_warn() {
    let cases = [
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: |\n  see key: !must_fill here\n~~~\n",
            "see key: !must_fill here\n",
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: \"see key: !must_fill here\"\n~~~\n",
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
        assert!(
            !out.document.main().payload().is_fill("note"),
            "marker text is not a marker\nSource:\n{}",
            src
        );
    }
}

#[test]
fn nested_must_fill_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\naddr:\n  street: !must_fill\n  city: Springfield\n~~~\n";
    let doc = Document::parse(src).unwrap().document;

    let fm = doc.main().payload();
    let addr = fm.get("addr").unwrap();
    assert!(
        addr.get("street").unwrap().fill(),
        "nested `street` must carry the fill marker"
    );
    assert!(!addr.get("city").unwrap().fill(), "`city` must not");
    assert_eq!(addr.get("city").unwrap().as_str(), Some("Springfield"));
    assert!(!addr.fill());

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("street: !must_fill") && emitted.contains("city: Springfield"),
        "nested fill must round-trip at depth\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    assert!(doc2
        .main()
        .payload()
        .get("addr")
        .unwrap()
        .get("street")
        .unwrap()
        .fill());
}

#[test]
fn fill_tag_mapping_rejected() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nx: !must_fill {a: 1}\n~~~\n";
    let err = Document::parse(src).unwrap_err();
    assert!(
        err.to_string().contains("!must_fill") && err.to_string().contains("mapping"),
        "expected mapping-rejection error; got: {}",
        err
    );
}

#[test]
fn fill_tag_all_scalar_types_round_trip() {
    let src = concat!(
        "~~~card-yaml\n$quill: q\n$kind: main\n",
        "s: !must_fill hello\n",
        "i: !must_fill 42\n",
        "f: !must_fill 3.14\n",
        "b: !must_fill true\n",
        "n: !must_fill\n",
        "~~~\n",
    );

    let doc = Document::parse(src).unwrap().document;
    let fm = doc.main().payload();

    assert_eq!(fm.get("s").and_then(|v| v.as_str()), Some("hello"));
    assert_eq!(fm.get("i").and_then(|v| v.as_i64()), Some(42));
    #[allow(clippy::approx_constant)]
    let expected_f = 3.14;
    assert_eq!(fm.get("f").and_then(|v| v.as_f64()), Some(expected_f));
    assert_eq!(fm.get("b").and_then(|v| v.as_bool()), Some(true));
    assert!(fm.get("n").map(|v| v.is_null()).unwrap_or(false));

    for key in ["s", "i", "f", "b", "n"] {
        assert!(fm.is_fill(key), "{} must be fill-tagged", key);
    }

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("n: !must_fill\n"),
        "bare `!must_fill` must round-trip as `key: !must_fill`\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    for key in ["s", "i", "f", "b", "n"] {
        assert!(
            doc2.main().payload().is_fill(key),
            "{} must remain fill-tagged after round-trip",
            key
        );
    }

    struct SeqCase {
        label: &'static str,
        key: &'static str,
        src: &'static str,
        expected_items: &'static [&'static str],
        emitted_contains: &'static str,
    }

    let seq_cases = [
        SeqCase {
            label: "block sequence",
            key: "recipient",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nrecipient: !must_fill\n  - Dr. Who\n  - 1 TARDIS Lane\n~~~\n",
            expected_items: &["Dr. Who", "1 TARDIS Lane"],
            emitted_contains: "recipient: !must_fill\n",
        },
        SeqCase {
            label: "flow sequence normalises to block form",
            key: "tags",
            src: "~~~card-yaml\n$quill: q\n$kind: main\ntags: !must_fill [a, b, c]\n~~~\n",
            expected_items: &["a", "b", "c"],
            emitted_contains: "tags: !must_fill",
        },
        SeqCase {
            label: "empty sequence",
            key: "items",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nitems: !must_fill []\n~~~\n",
            expected_items: &[],
            emitted_contains: "items: !must_fill []\n",
        },
    ];

    for case in seq_cases {
        let doc = Document::parse(case.src).unwrap().document;
        let fm = doc.main().payload();
        assert!(
            fm.is_fill(case.key),
            "[{}] key must be fill-tagged",
            case.label
        );

        let arr = fm.get(case.key).and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            arr.len(),
            case.expected_items.len(),
            "[{}] array length",
            case.label
        );
        for (item, expected) in arr.iter().zip(case.expected_items) {
            assert_eq!(
                item.as_str(),
                Some(*expected),
                "[{}] array element",
                case.label
            );
        }

        let emitted = doc.to_markdown();
        assert!(
            emitted.contains(case.emitted_contains),
            "[{}] must emit `{}`\nGot:\n{}",
            case.label,
            case.emitted_contains,
            emitted
        );

        let doc2 = Document::parse(&emitted).unwrap().document;
        assert!(
            doc2.main().payload().is_fill(case.key),
            "[{}] fill marker must survive round-trip",
            case.label
        );
        assert_eq!(doc2, doc, "[{}] full round-trip must be equal", case.label);
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
        no_drop_warning: bool,
    }

    let cases = [
        Case {
            label: "top-level own-line comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n# recipient's full name\nrecipient: Jane\nauthor: Alice\n~~~\n\nBody.\n",
            contains: &["# recipient's full name"],
            not_contains: &[],
            value_check: Some(("recipient", "Jane")),
            no_drop_warning: false,
        },
        Case {
            label: "top-level trailing inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\ntitle: My Document # this is a comment\n~~~\n\nBody.\n",
            contains: &["title: My Document # this is a comment"],
            not_contains: &["My Document\n# this is a comment"],
            value_check: Some(("title", "My Document")),
            no_drop_warning: false,
        },
        Case {
            label: "nested sequence comments (leading/between/trailing)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nitems:\n  # before-first\n  - a\n  # between\n  - b\n  # after-last\n~~~\n",
            contains: &["# before-first", "# between", "# after-last"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: true,
        },
        Case {
            label: "nested mapping comments (leading/trailing)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nouter:\n  # leading\n  inner: 1\n  # trailing\n~~~\n",
            contains: &["# leading", "# trailing"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: false,
        },
        Case {
            label: "nested sequence item trailing inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nitems:\n  - a # inline\n  - b\n~~~\n",
            contains: &["- a # inline"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: false,
        },
        Case {
            label: "nested mapping field trailing inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nouter:\n  inner: 1 # tail\n~~~\n",
            contains: &["inner: 1 # tail"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: false,
        },
        Case {
            label: "inline comment on a container key",
            src: "~~~card-yaml\n$quill: q\n$kind: main\nouter: # describes outer\n  inner: 1\n~~~\n",
            contains: &["outer: # describes outer\n  inner: 1"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: false,
        },
        Case {
            label: "own-line comment below $quill header (root payload)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n# main entry\ntitle: Hi\n~~~\n",
            contains: &["~~~\n$quill: q\n$kind: main\n# main entry\n"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: false,
        },
        Case {
            label: "own-line comment below $kind header (card payload)",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: foo\n# the foo card\nx: 1\n~~~\n",
            contains: &["~~~\n$kind: foo\n# the foo card\n"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: false,
        },
        Case {
            label: "own-line comments flanking an inline comment",
            src: "~~~card-yaml\n$quill: q\n$kind: main\n# header\ntitle: Hi # tail\n# footer\n~~~\n",
            contains: &["# header\n", "title: Hi # tail\n", "# footer\n"],
            not_contains: &[],
            value_check: None,
            no_drop_warning: false,
        },
    ];

    for case in cases {
        let out = Document::parse(case.src).unwrap();
        if case.no_drop_warning {
            assert!(
                !out.warnings
                    .iter()
                    .any(|w| w.code.as_deref() == Some("parse::comments_in_nested_yaml_dropped")),
                "[{}] no dropped-comment warning expected; nested comments are now preserved",
                case.label
            );
        }

        let emitted = out.document.to_markdown();
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
fn fill_with_inline_comment_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\ndept: !must_fill Sales # placeholder\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    assert!(
        doc.main().payload().is_fill("dept"),
        "fill marker must be set"
    );

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("dept: !must_fill Sales # placeholder"),
        "`!must_fill` and inline comment must round-trip together\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

#[test]
fn orphan_inline_after_remove_degrades_to_own_line() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nfield: value # tail\nother: 2\n~~~\n";

    let mut doc = Document::parse(src).unwrap().document;
    doc.main_mut().payload_mut().remove("field");

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("# tail"),
        "orphan comment text must be preserved\nGot:\n{}",
        emitted
    );
    assert!(
        !emitted.contains("\" # tail"),
        "orphan comment must not appear inline on another line\nGot:\n{}",
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
    use crate::QuillValue;

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

/// `- key: !must_fill` puts the marker on the dash line, where prescan inspects
/// it inline.
#[test]
fn seq_item_inline_first_key_fill_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nrecipients:\n  - name: !must_fill\n    role: lead\n~~~\n\nBody.\n";
    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("- name: !must_fill"),
        "first-key fill marker must survive round-trip\nGot:\n{}",
        emitted
    );
    assert!(emitted.contains("role: lead"), "Got:\n{}", emitted);
    let emitted2 = Document::parse(&emitted).unwrap().document.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

#[test]
fn array_element_nested_fill_survives_markdown_and_storage() {
    let src = "~~~card-yaml\n$quill: q@0.1\n$kind: main\nrecipients:\n  - name: Alice\n    org: !must_fill\n~~~\n\nBody.\n";
    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(emitted.contains("org: !must_fill"), "Got:\n{}", emitted);
    assert!(emitted.contains("name: Alice"), "Got:\n{}", emitted);

    let restored: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(
        doc, restored,
        "nested array-element fill must survive storage"
    );
    assert_eq!(
        emitted,
        restored.to_markdown(),
        "markdown must be identical after a storage round-trip"
    );
}
