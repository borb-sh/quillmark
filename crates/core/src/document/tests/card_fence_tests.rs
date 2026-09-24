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

/// No schema can claim a block that names no `$kind`, so it is no card: it
/// stays in the body above as a code block, and the prose after it with it.
#[test]
fn a_block_without_kind_is_code_in_the_body_above() {
    let src = "~~~\n$quill: q\n$kind: main\nt: !env T\n~~~\n\nIntro.\n\n~~~yaml\nname: server\n~~~\n\nConclusion.\n\n\
               ~~~\n$kind: note\nn: !env N\n~~~\n\nNote.\n\n~~~python\nprint(\"hi\")\n~~~\n\n~~~~\n- a\n- b\n~~~~\n\n\
               ~~~\n$quill: other\n~~~\n";
    let out = Document::parse(src).unwrap();
    let doc = out.document;
    assert_eq!(doc.cards().len(), 1);
    let main = doc.main().body_markdown();
    assert!(main.contains("name: server") && main.contains("Conclusion."), "{main}");
    let note = doc.cards()[0].body_markdown();
    assert!(note.contains("print(\"hi\")") && note.contains("- a"), "{note}");

    // Document order, the root's and the card's own warnings included.
    let warned: Vec<(&str, Option<u32>)> = out
        .warnings
        .iter()
        .map(|w| (w.code.as_deref().unwrap(), w.location.as_ref().map(|l| l.line)))
        .collect();
    let (tag, missing) = ("parse::unsupported_yaml_tag", "parse::missing_kind");
    assert_eq!(
        warned.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        [tag, missing, tag, missing, missing, missing]
    );
    let lines: Vec<u32> = warned.iter().filter(|(c, _)| *c == missing).filter_map(|(_, l)| *l).collect();
    assert_eq!(lines, [9, 22, 26, 31]);
    assert!(out.warnings[5].hint.as_deref().unwrap().contains("`$quill`"));
}

/// A demoted block is one code block exactly where the card scanner bounded it,
/// though CommonMark would close its tilde fence on an indented `~~~` inside.
#[test]
fn a_demoted_block_is_one_code_block_and_a_fixed_point() {
    let src = "~~~\n$quill: q\n~~~\n\nIntro.\n\n~~~\nnote: |\n  ~~~\n  inner ```\n~~~\n\nAfter prose.\n";
    let doc = Document::parse(src).unwrap().document;
    let body = doc.main().body();
    let kinds: Vec<&str> = body.lines.iter().map(|l| l.kind.tag()).collect();
    assert_eq!(kinds, ["para", "code", "code", "code", "para"]);
    assert_eq!(body.text.lines().last(), Some("After prose."));

    let again = Document::parse(&doc.to_markdown()).unwrap().document;
    assert_eq!(again, doc);
}

/// The card cap counts cards, not the blocks that read as code.
#[test]
fn demoted_blocks_do_not_count_toward_the_card_cap() {
    let code = "~~~python\nprint(i)\n~~~\n\n".repeat(crate::error::MAX_CARD_COUNT + 1);
    let doc = Document::parse(&format!("~~~\n$quill: q\n~~~\n\n{code}")).unwrap().document;
    assert!(doc.cards().is_empty());
}

/// A block's YAML is read before its `$kind`, whatever its info string, so an
/// unreadable one fails at its line.
#[test]
fn an_unreadable_block_fails_whatever_its_info_string() {
    let head = "~~~\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n";
    let fails = |block: &str| Document::parse(&format!("{head}{block}")).unwrap_err().to_diagnostic();

    let code = fails("~~~python\nvalues: [1, 2\n~~~\n");
    assert_eq!(code.code.as_deref(), Some("parse::yaml_error_with_location"));
    assert_eq!(code.location.map(|l| l.line), Some(9));
    assert!(code.hint.as_deref().unwrap().contains("backticks"), "{:?}", code.hint);

    let card = fails("~~~yaml\n$kind: note\nbad-name: 1\n~~~\n");
    assert_eq!(card.code.as_deref(), Some("parse::invalid_structure"));
}

/// The root is the first block whatever it holds, so code there fails at its
/// fence and names the backtick fence.
#[test]
fn tilde_code_as_the_root_fails_at_its_fence() {
    let diag = Document::parse("Example:\n\n~~~python\nprint(\"hi\")\n~~~\n")
        .unwrap_err()
        .to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("parse::payload_not_mapping"));
    assert_eq!(diag.location.map(|l| (l.line, l.column)), Some((3, 1)));
    assert_eq!(diag.args.get("info"), Some(&serde_json::json!("python")));
    assert_eq!(diag.args.get("actual"), Some(&serde_json::json!("string")));
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
