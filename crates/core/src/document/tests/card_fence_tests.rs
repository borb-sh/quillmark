use crate::document::Document;

/// Any column-zero tilde run of three or more, with any info string, opens the
/// same block the bare `~~~` does, so every spelling parses to one document and
/// emits its canonical form.
#[test]
fn every_tilde_opener_parses_as_the_bare_form() {
    let doc = |open: &str, close: &str| {
        Document::parse(&format!(
            "{open}\n$quill: q\n$kind: main\ntitle: Hi\n{close}\n\nBody.\n\n{open}\n$kind: note\nname: Widget\n{close}\n\nCard body.\n"
        ))
        .unwrap_or_else(|e| panic!("{open:?}: {e}"))
        .document
    };
    let bare = doc("~~~", "~~~");
    assert_eq!(bare.cards()[0].kind(), Some("note"));
    for (open, close) in [
        ("~~~card-yaml", "~~~"),
        ("~~~yaml", "~~~"),
        ("~~~rust", "~~~"),
        ("~~~~", "~~~~"),
        ("~~~", "~~~~~"),
    ] {
        assert_eq!(doc(open, close), bare, "{open:?} / {close:?}");
    }
}

#[test]
fn shorter_tilde_run_does_not_close_a_longer_fence() {
    // CommonMark fence matching: the closer must be at least as long as the opener.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~~\n$kind: note\nbody: \"a ~~~ b\"\n~~~~\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(
        doc.cards()[0].payload().get("body").unwrap().as_str(),
        Some("a ~~~ b")
    );
}

#[test]
fn backtick_fence_is_the_code_block_escape_hatch() {
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n```\n~~~\nnot a card\n~~~\n```\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body_markdown().contains("not a card"));
}

#[test]
fn tilde_code_in_a_body_fails_at_its_fence() {
    let head = "~~~\n$quill: q\n$kind: main\n~~~\n\nExample:\n\n";
    let fails = |block: &str| {
        Document::parse(&format!("{head}{block}"))
            .unwrap_err()
            .to_diagnostic()
    };

    let tagged = fails("~~~python\nprint(\"hi\")\n~~~\n");
    assert_eq!(tagged.code.as_deref(), Some("parse::payload_not_mapping"));
    assert_eq!(tagged.location.map(|l| (l.line, l.column)), Some((8, 1)));
    assert_eq!(tagged.args.get("info"), Some(&serde_json::json!("python")));
    assert_eq!(tagged.args.get("actual"), Some(&serde_json::json!("string")));

    let bare = fails("~~~~  \n- a\n- b\n~~~~\n");
    assert_eq!(bare.code.as_deref(), Some("parse::payload_not_mapping"));
    assert_eq!(bare.args.get("info"), None);
    assert_eq!(bare.args.get("actual"), Some(&serde_json::json!("sequence")));

    // A block after the root is a card, so a mapping naming no `$kind` fails:
    // a card missing its kind line, or code whose text reads as a mapping.
    for (block, info) in [
        ("~~~yaml\nname: server\n~~~\n\nConclusion.\n", Some("yaml")),
        ("~~~python\ndef f(x):\n    return x\n~~~\n", Some("python")),
        ("~~~\n~~~\n", None),
        ("~~~card-yaml\ntitle: T\n~~~\n", Some("card-yaml")),
    ] {
        let kindless = fails(block);
        assert_eq!(kindless.code.as_deref(), Some("parse::missing_kind"), "{block:?}");
        assert_eq!(kindless.location.map(|l| (l.line, l.column)), Some((8, 1)));
        assert_eq!(kindless.args.get("info"), info.map(|i| serde_json::json!(i)).as_ref());
        let hint = kindless.hint.unwrap();
        let fence = format!("```{}", info.filter(|i| *i != "card-yaml").unwrap_or(""));
        assert!(hint.contains("`$kind: <kind>`") && hint.ends_with(&fence), "{hint}");
    }

    let card = fails("~~~yaml\n$kind: note\nbad-name: 1\n~~~\n");
    assert_eq!(card.code.as_deref(), Some("parse::invalid_structure"));
    assert!(!card.message.contains("```"), "{}", card.message);
}

#[test]
fn indented_tilde_opener_is_not_a_card() {
    // A card opener must be at column zero (spec §3.2).
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n   ~~~\n$kind: note\nx: 1\n   ~~~\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body_markdown().contains("$kind: note"));
}

#[test]
fn unclosed_bare_tilde_in_body_falls_through_to_commonmark() {
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\nIntro.\n\n~~~\nstray\n";
    let out = Document::parse(src).unwrap();
    assert_eq!(out.document.cards().len(), 0);
    assert!(out.document.main().body_markdown().contains("stray"));
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code.as_deref() == Some("parse::unclosed_code_block")));
}

#[test]
fn card_fence_without_blank_line_above_is_not_a_card() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nSome prose.\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n";
    let out = Document::parse(src).unwrap();
    assert_eq!(out.document.cards().len(), 0);
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code.as_deref() == Some("parse::card_fence_missing_blank")));
}

#[test]
fn indented_tilde_inside_block_scalar_is_payload_not_closer() {
    // Only a column-zero `~~~` closes a card block; indented ones are payload.
    let src = "\
~~~
$quill: q@1.0
$kind: main
snippet: |
  Here is code:
  ~~~
  let x = 1;
  ~~~
  done
~~~

The body.
";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(
        doc.main().payload().get("snippet").unwrap().as_str(),
        Some("Here is code:\n~~~\nlet x = 1;\n~~~\ndone\n"),
        "block scalar must keep the embedded tilde fence intact"
    );
    assert_eq!(doc.main().body_markdown(), "The body.");
}
