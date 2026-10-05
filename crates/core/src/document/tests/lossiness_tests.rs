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

/// A tag keeps its value wherever it sits, and each on a block key warns at its
/// path, a card's rooted at the card's index among all cards.
#[test]
fn a_tag_keeps_its_value_and_warns_at_its_path() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n\
               subject: !custom Example # string\n\
               recipient: !custom # array<string>\n  - Mr. John Doe\n\
               bare: !custom\n\
               addr:\n  street: !custom Main\n  city: Springfield\n\
               x: !custom {a: 1}\n\
               to:\n  - name: !custom Jane\n    rank: Capt\n~~~\n\n\
               ~~~card-yaml\n$kind: intro\n~~~\n\n\
               ~~~card-yaml\n$kind: note\nsubject: !custom Example\n~~~\n";
    let out = Document::parse(src).unwrap();
    let get = |k: &str| out.document.main().payload().get(k).unwrap().as_json().clone();
    assert_eq!(get("subject"), "Example");
    assert_eq!(get("recipient"), serde_json::json!(["Mr. John Doe"]));
    assert_eq!(get("bare"), serde_json::Value::Null);
    assert_eq!(get("addr"), serde_json::json!({"street": "Main", "city": "Springfield"}));
    assert_eq!(get("x"), serde_json::json!({"a": 1}));
    assert_eq!(get("to"), serde_json::json!([{"name": "Jane", "rank": "Capt"}]));
    assert_eq!(
        out.document.cards()[1].payload().get("subject").unwrap().as_json(),
        "Example"
    );
    let tagged = [
        "main.subject",
        "main.recipient",
        "main.bare",
        "main.addr.street",
        "main.x",
        "main.to[0].name",
        "cards.note[1].subject",
    ];
    assert_eq!(
        anchors(&out),
        tagged.map(|path| ("parse::unsupported_yaml_tag", Some(path)))
    );

    let md = out.document.to_markdown();
    assert!(!md.contains("!custom"), "{md}");
    assert!(md.contains("subject: Example # string\n"), "{md}");
    let again = Document::parse(&md).unwrap();
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    assert_eq!(again.document, out.document, "{md}");
}

/// A `$seed` / `$ext` value is opaque, with no document address, so a tag
/// inside one warns without a `path`.
#[test]
fn a_tag_inside_meta_keeps_its_value_and_warns_without_a_path() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n$ext:\n  ns:\n    to: !custom X\n~~~\n";
    let out = Document::parse(src).unwrap();
    assert_eq!(out.document.main().ext().unwrap()["ns"]["to"], "X");
    assert_eq!(anchors(&out), [("parse::unsupported_yaml_tag", None)]);
}

/// A line continuing a flow collection, opened on its key's line or the line
/// below, is the collection's, never a key of the mapping around it, so a tag
/// there stays on its own value and warns at that value's path.
#[test]
fn a_tag_inside_a_multi_line_flow_collection_stays_on_its_value() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n\
               b: kept\n\
               x: {a: it's,\n  b: !t 2}\n\
               addr:\n  b: kept\n  y:\n    [Why? 'cause,\n   b: !t 2]\n\
               tags: [!t \"a # [\", b]\n\
               c: !t C\n~~~\n";
    let out = Document::parse(src).unwrap();
    let get = |k: &str| out.document.main().payload().get(k).unwrap().as_json().clone();
    assert_eq!(get("b"), "kept");
    assert_eq!(get("x"), serde_json::json!({"a": "it's", "b": 2}));
    assert_eq!(
        get("addr"),
        serde_json::json!({"b": "kept", "y": ["Why? 'cause", {"b": 2}]})
    );
    assert_eq!(get("tags"), serde_json::json!(["a # [", "b"]));
    assert_eq!(get("c"), "C");
    assert_eq!(
        anchors(&out),
        ["main.x.b", "main.addr.y[1].b", "main.tags[0]", "main.c"]
            .map(|path| ("parse::unsupported_yaml_tag", Some(path)))
    );
}

/// A comment trailing a line that continues a flow collection or a quoted
/// scalar is the trailer of the key or item the value belongs to, or, when a
/// comment already sits on that line or inside the value, a comment on its own
/// line after the value.
#[test]
fn a_comment_on_a_continuation_line_stays_with_its_value() {
    let cases = [
        (
            "recipient:\n  addr: {street: Main St,\n    city: Anytown}  # verified\n  name: Jo\n",
            "  addr: # verified\n",
        ),
        ("rows:\n  - {a: 1,\n     b: 2}  # c\n", "  - a: 1 # c\n"),
        ("$ext:\n  og: {title: T,\n    url: http://x}  # c\n", "  og: # c\n"),
        (
            "memo:\n  note: \"Reply by Friday,\n    see: attached\"  # from Jo\n",
            "  note: \"Reply by Friday, see: attached\" # from Jo\n",
        ),
        ("x: [1,\n  2]  # c\n", "x: # c\n"),
        (
            "m:\n  x: {a: 1, # first\n    b: 2} # second\n  y: 4\n",
            "  x: # first\n    a: 1\n    b: 2\n  # second\n",
        ),
        (
            "rows:\n  - k0: [1, # a\n      2] # b\n    k1: x\n  - k2: z\n",
            "  - k0: # a\n      - 1\n      - 2\n    # b\n    k1: x\n",
        ),
        (
            "rows:\n  - k0: # a\n      [1,\n      2] # b\n    k1: x\n  - k2: z\n",
            "  - k0: # a\n      - 1\n      - 2\n    # b\n    k1: x\n",
        ),
        (
            "rows:\n  - k0: [1, # a\n      2] # b\n  - k2: z\n",
            "      - 2\n    # b\n  - k2: z\n",
        ),
        (
            "m:\n  x: [1,\n    # mid\n    2] # c\n  y: 4\n",
            "  x:\n    - 1\n    - 2\n  # mid\n  # c\n",
        ),
        (
            "rows:\n  - [1,\n    # mid\n    2] # c\n  - 3\n",
            "  -\n    - 1\n    - 2\n  # mid\n  # c\n  - 3\n",
        ),
    ];
    for (fields, emitted) in cases {
        let src = format!("~~~card-yaml\n$quill: q\n$kind: main\n{fields}~~~\n");
        let doc = Document::parse(&src).unwrap().document;
        let md = doc.to_markdown();
        assert!(md.contains(emitted), "Source:\n{src}\nGot:\n{md}");
        assert_eq!(Document::parse(&md).unwrap().document, doc, "{md}");
    }
}

/// A plain scalar on the lines below its key holds no comment: one under it,
/// or between the key and it, sits after the key's value.
#[test]
fn a_comment_under_a_scalar_below_its_key_follows_the_value() {
    let cases = [
        "k:\n  some text\n  # c\nn: 1\n",
        "k: !t\n  some text\n  # c\nn: 1\n",
        "k: &a\n  some text\n  # c\nn: 1\n",
        "k: !t\n  # c\n  some text\nn: 1\n",
        "m:\n  k: !t\n    some text\n    # c\n  n: 1\n",
        "m:\n  k: !t\n    # c\n    some text\n  n: 1\n",
        "rows:\n  -\n    some text\n    # c\n  - b\n",
        "k:\n  some text # c\nn: 1\n",
        "k:\n  [1, # c\n  2]\nn: 1\n",
        "m:\n  k:\n    some text # c\n  n: 1\n",
        "rows:\n  -\n    some text # c\n  - b\n",
        "rows:\n  - k:\n      some text # c\n  - b\n",
    ];
    for fields in cases {
        let src = format!("~~~card-yaml\n$quill: q\n$kind: main\n{fields}~~~\n");
        let doc = Document::parse(&src).unwrap().document;
        let md = doc.to_markdown();
        assert!(md.contains("# c\n"), "Source:\n{src}\nGot:\n{md}");
        assert_eq!(Document::parse(&md).unwrap().document, doc, "{md}");
    }
}

/// Each comment keeps the container and slot the YAML gives it, whatever the
/// spelling's indentation: a sequence at its key's column, a comment indented
/// less than its block, a compact nested sequence, a continuation line.
#[test]
fn a_comment_keeps_its_slot_under_any_indentation() {
    let cases = [
        (
            "to:\n# lead\n- name: a # c1\n  # inner\n  rank: 1\n- name: b\n",
            "to:\n  # lead\n  - name: a # c1\n    # inner\n    rank: 1\n  - name: b\n",
        ),
        (
            "o:\n  k:\n  - a # c\n  - b\n  j: 1\n",
            "o:\n  k:\n    - a # c\n    - b\n  j: 1\n",
        ),
        (
            "classification:\n  value: CUI\n# note\n  controlled_by: SAF/AA # tail\n  other: 1\n",
            "classification:\n  value: CUI\n  # note\n  controlled_by: SAF/AA # tail\n  other: 1\n",
        ),
        (
            "l:\n  - - x # c1\n    - w # c2\n  - - z # c3\n",
            "l:\n  -\n    - x # c1\n    - w # c2\n  -\n    - z # c3\n",
        ),
        ("note: first\n  key:value # z\n", "note: first key:value # z\n"),
        (
            "to:\n- a\n- b\n# about next\nnext: 1\n",
            "to:\n  - a\n  - b\n# about next\nnext: 1\n",
        ),
    ];
    for (fields, emitted) in cases {
        let src = format!("~~~card-yaml\n$quill: q\n$kind: main\n{fields}~~~\n");
        let doc = Document::parse(&src).unwrap_or_else(|e| panic!("{src}\n{e}")).document;
        let md = doc.to_markdown();
        assert!(md.contains(emitted), "Source:\n{src}\nGot:\n{md}");
        assert_eq!(Document::parse(&md).unwrap().document, doc, "{md}");
    }
}

/// A tag warns at its own node's path, whatever the indentation around it.
#[test]
fn a_tag_warns_at_its_node() {
    let cases = [
        ("to:\n- name: !foo x\n", "main.to[0].name"),
        (
            "classification:\n  value: CUI\n# note\n  controlled_by: !foo SAF/AA\n",
            "main.classification.controlled_by",
        ),
        ("l:\n  - &a\n    x: 1\n    y: !foo 2\n", "main.l[0].y"),
        ("l:\n  - !foo\n    x: 1\n", "main.l[0]"),
    ];
    for (fields, path) in cases {
        let src = format!("~~~card-yaml\n$quill: q\n$kind: main\n{fields}~~~\n");
        let out = Document::parse(&src).unwrap_or_else(|e| panic!("{src}\n{e}"));
        assert_eq!(
            anchors(&out),
            [("parse::unsupported_yaml_tag", Some(path))],
            "{src}"
        );
    }
}

/// Comments under key paths past the prescan's budget refuse the block.
#[test]
fn comments_past_the_path_budget_refuse_the_block() {
    let long = "k".repeat(1000);
    let mut fields = String::new();
    for depth in 0..8 {
        fields.push_str(&format!("{}{long}{depth}:\n", " ".repeat(depth * 2)));
    }
    fields.push_str(&format!("{}x: 1\n", " ".repeat(16)));
    fields.push_str(&format!("{}#\n", " ".repeat(16)).repeat(2000));
    let src = format!("~~~card-yaml\n$quill: q\n$kind: main\n{fields}~~~\n");
    let err = Document::parse(&src).expect_err("over the budget");
    assert_eq!(err.code(), "parse::invalid_structure");
}

/// A column-zero key the YAML reads as a key is one, so a `#` inside its
/// quoted value is text and the refusal names the field.
#[test]
fn a_key_outside_the_field_grammar_is_refused_as_a_field_name() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nog:title: \"Issue #5\"\n~~~\n";
    let err = Document::parse(src).expect_err("`og:title` is no field name");
    let crate::error::ParseError::InvalidStructure(message) = err else {
        panic!("expected the field-name refusal, got {err:?}");
    };
    assert!(message.contains("og:title"), "{message}");
}

/// A tag or anchor ahead of a quoted scalar or flow collection leaves a ` #`
/// inside it text, and a comment after it a comment.
#[test]
fn a_hash_inside_a_quoted_value_behind_a_tag_or_anchor_is_text() {
    let cases: [(&str, &str, serde_json::Value); 6] = [
        ("k: !t \"a # b\" # c\n", "k", serde_json::json!("a # b")),
        ("k: &a 'a # b' # c\n", "k", serde_json::json!("a # b")),
        ("m:\n  k: !t 'a # b' # c\n", "m", serde_json::json!({"k": "a # b"})),
        ("k: !t [\"a # b\", c] # c\n", "k", serde_json::json!(["a # b", "c"])),
        ("rows:\n  - k: !t \"a # b\" # c\n", "rows", serde_json::json!([{"k": "a # b"}])),
        ("rows:\n  - !t \"a # b\" # c\n", "rows", serde_json::json!(["a # b"])),
    ];
    for (fields, key, value) in cases {
        let src = format!("~~~card-yaml\n$quill: q\n$kind: main\n{fields}~~~\n");
        let doc = Document::parse(&src).unwrap_or_else(|e| panic!("{src}\n{e}")).document;
        assert_eq!(doc.main().payload().get(key).unwrap().as_json(), &value, "{src}");
        let md = doc.to_markdown();
        assert!(md.contains("# c\n"), "Source:\n{src}\nGot:\n{md}");
        assert_eq!(Document::parse(&md).unwrap().document, doc, "{md}");
    }
}

/// A tag or anchor ahead of `|` or `>` leaves the block's lines its text: no
/// key, comment or tag among them reaches the mapping around it. The tag on
/// the block itself warns at the block's path.
#[test]
fn a_block_scalar_behind_a_tag_or_anchor_is_text() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n\
               memo:\n  body: &a |\n    # Summary\n    subject: !t TBD\n  subject: Final\n\
               notes:\n  - !!str >\n    # kept\n~~~\n";
    let out = Document::parse(src).unwrap();
    let get = |k: &str| out.document.main().payload().get(k).unwrap().as_json().clone();
    assert_eq!(
        get("memo"),
        serde_json::json!({"body": "# Summary\nsubject: !t TBD\n", "subject": "Final"})
    );
    assert_eq!(get("notes"), serde_json::json!(["# kept\n"]));
    assert_eq!(anchors(&out), [("parse::unsupported_yaml_tag", Some("main.notes[0]"))]);
    let md = out.document.to_markdown();
    assert_eq!(md.matches("# Summary").count(), 1, "{md}");
    assert_eq!(md.matches("# kept").count(), 1, "{md}");
}

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

/// `key: !t` inside a block or quoted scalar is that scalar's text, kept
/// verbatim.
#[test]
fn tag_text_inside_a_scalar_is_text() {
    let cases = [
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: |\n  see key: !t here\n~~~\n",
            "see key: !t here\n",
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: \"see key: !t here\"\n~~~\n",
            "see key: !t here",
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote: \"see\n  key: !t here\"\n~~~\n",
            "see key: !t here",
        ),
        (
            "~~~card-yaml\n$quill: q\n$kind: main\nnote:\n  \"see\n  key: !t here\"\n~~~\n",
            "see key: !t here",
        ),
    ];

    for (src, value) in cases {
        let out = Document::parse(src).unwrap();
        assert!(
            out.warnings.is_empty(),
            "tag text inside a scalar must not warn\nSource:\n{}\nGot: {:?}",
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
            "scalar must keep the tag text verbatim\nSource:\n{}",
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

/// A run of own-line comments meets no count limit, behind a trailer too:
/// every comment, and the field after the run, survives the parse.
#[test]
fn a_long_comment_run_keeps_every_comment() {
    let run: String = (0..120).map(|i| format!("  # c{i}\n")).collect();
    let src =
        format!("~~~card-yaml\n$quill: q\n$kind: main\nk: # note\n{run}  a: 1\nafter: 2\n~~~\n");
    let emitted = Document::parse(&src)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .document
        .to_markdown();
    for c in (0..120)
        .map(|i| format!("# c{i}\n"))
        .chain(["# note\n".into(), "after: 2\n".into()])
    {
        assert!(emitted.contains(&c), "{c:?} dropped:\n{emitted}");
    }
    assert_eq!(
        Document::parse(&emitted).unwrap().document.to_markdown(),
        emitted
    );
}

/// A comment indented under an empty `$ext` or `$seed` is inside it, as under
/// an empty field.
#[test]
fn a_comment_under_an_empty_meta_block_round_trips() {
    for meta in ["$ext", "$seed"] {
        let src =
            format!("~~~card-yaml\n$quill: q\n$kind: main\n{meta}: {{}}\n  # under\nk: 1\n~~~\n");
        let emitted = Document::parse(&src).unwrap().document.to_markdown();
        assert!(
            emitted.contains(&format!("{meta}: {{}}\n  # under\n")),
            "{emitted}"
        );
        assert_eq!(
            Document::parse(&emitted).unwrap().document.to_markdown(),
            emitted
        );
    }
}

/// The field grammar admits `null` in any letter case, a key YAML reads as no
/// key unless quoted.
#[test]
fn a_field_named_null_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n\"null\": x\n\"NULL\": y\n~~~\n";
    let emitted = Document::parse(src).unwrap().document.to_markdown();
    let back = Document::parse(&emitted)
        .unwrap_or_else(|e| panic!("{e:?}\n{emitted}"))
        .document;
    for (key, value) in [("null", "x"), ("NULL", "y")] {
        assert_eq!(
            back.main().payload().get(key).and_then(|v| v.as_str()),
            Some(value)
        );
    }
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
