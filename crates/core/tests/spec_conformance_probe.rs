//! Spec requirements from `prose/references/markdown-spec.md` that have no
//! owner among the `document/tests/` unit modules.

use quillmark_core::{Document, ParseError};

#[test]
fn parse_strips_a_bidi_control_from_a_card_body() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: x\n~~~\n\n<!-- c -->trailing\u{202D}text";
    let doc = Document::parse(md).unwrap().document;
    let body = doc.cards()[0].body_markdown();
    assert!(
        body.contains("trailingtext"),
        "card body missing bidi-strip, got: {:?}",
        body
    );
}

/// U+212A KELVIN SIGN composes to `K` under NFC, and a field name is ASCII as
/// authored: parse refuses the key rather than folding it.
#[test]
fn parse_refuses_a_field_name_that_only_normalises_to_ascii() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n\u{212A}elvin: v\n~~~\n\nBody.";
    let err = Document::parse(md).expect_err("a Kelvin-sign key is not a field name");
    assert!(
        matches!(&err, ParseError::InvalidStructure(m) if m.contains("field names must match")),
        "{err:?}"
    );
}

#[test]
fn unclosed_code_block_emits_warning() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\n```\ncode line\n\n~~~card-yaml\n$kind: x\n~~~\n\ntrailing body";
    let out = Document::parse(md).unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("parse::unclosed_code_block")),
        "expected unclosed-code-block warning, got: {:?}",
        out.warnings
            .iter()
            .map(|w| (w.code.clone(), w.message.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        out.document.cards().is_empty(),
        "shielded card block must not have been parsed"
    );
}

#[test]
fn per_block_field_count_cap() {
    let mut s = String::from("~~~card-yaml\n$quill: t\n$kind: main\n");
    for i in 0..1001 {
        s.push_str(&format!("f{}: v\n", i));
    }
    s.push_str("~~~\n\nBody.");
    let diag = Document::parse(&s).unwrap_err().to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("parse::too_many_fields"));
    assert!(
        diag.message.contains("Too many fields") && !diag.message.contains("bytes"),
        "a field count is not a byte count, got: {}",
        diag.message
    );
    assert_eq!(diag.args.get("count"), Some(&serde_json::json!(1001)));
}

#[test]
fn card_count_cap_is_per_card() {
    let mut s = String::from("~~~card-yaml\n$quill: t\n$kind: main\n~~~\n");
    for _ in 0..1001 {
        s.push_str("\n~~~card-yaml\n$kind: x\n~~~\n\nB.\n");
    }
    let diag = Document::parse(&s).unwrap_err().to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("parse::too_many_cards"));
    assert!(
        diag.message.contains("Too many cards") && !diag.message.contains("bytes"),
        "a card count is not a byte count, got: {}",
        diag.message
    );
    assert_eq!(diag.args.get("count"), Some(&serde_json::json!(1001)));
}

/// `docs/integration/operations.md` tells an integrator to read the caps rather
/// than copy the numbers, so this pins the paths, not the values — from
/// out-of-crate, as a consumer reads them.
#[test]
fn spec_caps_are_reachable_at_their_documented_paths() {
    use quillmark_core::error::{
        MAX_CARD_COUNT, MAX_FIELD_COUNT, MAX_INPUT_SIZE, MAX_YAML_SIZE,
    };

    for cap in [MAX_INPUT_SIZE, MAX_YAML_SIZE, MAX_CARD_COUNT, MAX_FIELD_COUNT] {
        assert!(cap > 0);
    }
}
