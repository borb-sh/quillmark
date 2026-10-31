//! Spec requirements from `prose/references/markdown-spec.md` that have no
//! owner among the `document/tests/` unit modules.

use quillmark_core::document::Document;
use quillmark_core::error::{MAX_CARD_COUNT, MAX_FIELD_COUNT};

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
    let block = |count: usize| {
        let mut s = String::from("~~~card-yaml\n$quill: t\n$kind: main\n");
        for i in 0..count {
            s.push_str(&format!("f{}: v\n", i));
        }
        s.push_str("~~~\n\nBody.");
        s
    };

    // The width the cap admits reaches the payload whole: every field, no
    // silent drop on the way.
    let at_cap = Document::parse(&block(MAX_FIELD_COUNT))
        .expect("a block at the cap parses")
        .document;
    assert_eq!(at_cap.main().payload().len(), MAX_FIELD_COUNT);

    let s = block(MAX_FIELD_COUNT + 1);
    let diag = Document::parse(&s).unwrap_err().to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("parse::too_many_fields"));
    assert_eq!(
        diag.args.get("count"),
        Some(&serde_json::json!(MAX_FIELD_COUNT + 1))
    );
}

#[test]
fn card_count_cap_is_per_card() {
    let mut s = String::from("~~~card-yaml\n$quill: t\n$kind: main\n~~~\n");
    for _ in 0..MAX_CARD_COUNT + 1 {
        s.push_str("\n~~~card-yaml\n$kind: x\n~~~\n\nB.\n");
    }
    let diag = Document::parse(&s).unwrap_err().to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("parse::too_many_cards"));
    assert_eq!(
        diag.args.get("count"),
        Some(&serde_json::json!(MAX_CARD_COUNT + 1))
    );
}

/// `docs/integration/operations.md` tells an integrator to read the caps rather
/// than copy the numbers, so this pins the paths, not the values — from
/// out-of-crate, as a consumer reads them.
#[test]
fn spec_caps_are_reachable_at_their_documented_paths() {
    use quillmark_core::error::{MAX_INPUT_SIZE, MAX_YAML_SIZE};

    for cap in [
        MAX_INPUT_SIZE,
        MAX_YAML_SIZE,
        MAX_CARD_COUNT,
        MAX_FIELD_COUNT,
    ] {
        assert!(cap > 0);
    }
}
