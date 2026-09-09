use crate::document::Document;

#[test]
fn card_fence_parses_kind_fields_and_body() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\nname: Widget\nprice: 19\n~~~\n\nWidget description.\n";
    let doc = Document::parse(src).unwrap().document;

    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("product"));
    assert_eq!(card.payload().get("name").unwrap().as_str(), Some("Widget"));
    assert_eq!(card.body_markdown(), "Widget description.");
}

#[test]
fn emit_uses_canonical_card_fence() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert_eq!(
        emitted,
        "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~\n$kind: product\nname: Widget\n~~~\n"
    );
}

#[test]
fn card_fence_body_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nMain body.\n\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n\nCard body.\n";
    let a = Document::parse(src).unwrap().document;
    let b = Document::parse(&a.to_markdown()).unwrap().document;
    assert_eq!(a, b);
    assert_eq!(a.main().body_markdown(), "Main body.");
    assert_eq!(a.cards()[0].body_markdown(), "Card body.");
}

#[test]
fn card_fence_preserves_yaml_comments() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\n# a banner\nname: Widget\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("~~~\n$kind: product\n# a banner\nname: Widget\n~~~\n"),
        "emit:\n{emitted}"
    );
    let reparsed = Document::parse(&emitted).unwrap().document;
    assert_eq!(doc, reparsed);
}

#[test]
fn card_fence_without_kind_is_allowed() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\nname: Widget\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), None);
}

#[test]
fn bare_tilde_fence_opens_a_card_yaml_block() {
    let src =
        "~~~\n$quill: q\n$kind: main\ntitle: Hi\n~~~\n\nBody.\n\n~~~\n$kind: note\nname: N\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.quill_reference().name, "q");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str(),
        Some("Hi")
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

#[test]
fn legacy_card_yaml_info_string_normalizes_to_bare_tilde() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n";
    let emitted = Document::parse(src).unwrap().document.to_markdown();
    assert_eq!(emitted, "~~~\n$quill: q\n$kind: main\n~~~\n");
}

#[test]
fn yaml_info_string_opens_a_card_yaml_block() {
    let src = "~~~yaml\n$quill: q\n$kind: main\ntitle: Hi\n~~~\n\nBody.\n\n~~~yaml\n$kind: note\nname: N\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.quill_reference().name, "q");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str(),
        Some("Hi")
    );
    assert_eq!(doc.main().body_markdown(), "Body.");
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

#[test]
fn yaml_info_string_normalizes_to_bare_tilde() {
    let src = "~~~yaml\n$quill: q\n$kind: main\n~~~\n";
    let emitted = Document::parse(src).unwrap().document.to_markdown();
    assert_eq!(emitted, "~~~\n$quill: q\n$kind: main\n~~~\n");
}

#[test]
fn backtick_yaml_fence_stays_an_ordinary_code_block() {
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n```yaml\n$quill: not_a_card\n```\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body_markdown().contains("$quill: not_a_card"));
}

#[test]
fn dash_yaml_line_is_not_a_fence() {
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n---yaml\nkey: value\n---\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 0);
    assert!(
        doc.main().body_markdown().contains("---yaml"),
        "body: {:?}",
        doc.main().body_markdown()
    );
}

#[test]
fn longer_tilde_run_still_opens_a_card() {
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~~\n$kind: note\nname: Widget\n~~~~\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("~~~\n$kind: note\nname: Widget\n~~~\n"),
        "{emitted}"
    );
    assert!(
        !emitted.contains("~~~~"),
        "longer runs normalise to `~~~`: {emitted}"
    );
}

#[test]
fn shorter_tilde_run_does_not_close_a_longer_fence() {
    // CommonMark fence matching: the closer must be at least as long as the opener.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~~\nbody: \"a ~~~ b\"\n~~~~\n";
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
fn tilde_fence_with_language_info_opens_a_card() {
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~rust\n$kind: note\nx: 1\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

#[test]
fn language_tagged_tilde_code_in_a_body_is_parsed_as_yaml() {
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~rust\nlet x = 1;\n~~~\n";
    let err = Document::parse(src).unwrap_err().to_string();
    assert!(err.contains("mapping"), "got: {err}");
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

#[test]
fn indented_tilde_line_never_closes_a_card_fence() {
    // An indented `~~~` is a valid CommonMark closer but not a card closer.
    let src = "~~~\n$quill: q@1.0\n$kind: main\nx: 1\n  ~~~\n";
    let err = Document::parse(src).unwrap_err();
    assert!(
        matches!(err, crate::error::ParseError::MissingQuill(_)),
        "expected MissingQuill, got {err:?}"
    );
}
