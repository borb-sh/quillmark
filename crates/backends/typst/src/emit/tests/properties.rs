//! The escapers over generated text, read back through Typst's own parser:
//! escaped text never breaks out of the string or markup context it was written
//! into, and the import-then-lower path behind them never panics.
//!
//! The oracle is [`resolve`], shared with the fixed-sample tests beside it.

use proptest::prelude::*;
use quillmark_content::export::to_plaintext;
use quillmark_content::import::from_plaintext;
use typst::syntax::{ast, ast::AstNode, SyntaxKind};

use super::{resolve, ALLOWED_LEAVES};
use crate::emit::{escape_markup, escape_string};

/// The render path: import markdown to a content, then lower it to markup.
fn mark_to_typst(markdown: &str) -> Result<String, String> {
    let rt = quillmark_content::import::from_markdown(markdown).map_err(|e| e.to_string())?;
    crate::emit::emit_content(&rt)
        .map(|ec| ec.markup)
        .map_err(|e| e.to_string())
}

// The markup escapes that fire on the character alone.
const TYPST_SPECIAL_CHARS: &[char] = &[
    '~', '*', '_', '`', '#', '[', ']', '{', '}', '$', '<', '>', '@',
];

/// A `--`, a `...`, or a separator ahead of a block marker never lands by
/// chance in a draw over all of Unicode. The second alphabet is what does land
/// one: Typst's special characters, every separator it reads as a line break,
/// and the markers and space that open a block behind one.
fn escaper_input() -> impl Strategy<Value = String> {
    prop_oneof![
        r#"[-.?/~*_#+=\[\]{}$<>@'"0-9a-c \\`\x{0B}\x{0C}\x{85}\x{2028}\x{2029}]{0,40}"#,
        "\\PC*",
    ]
}

proptest! {
    /// Typst reads the escaped literal back as one string holding the authored
    /// characters: nothing in `s` closes the literal or opens a second one.
    #[test]
    fn an_escaped_string_literal_resolves_to_the_authored_text(s in "\\PC*") {
        let src = format!("\"{}\"", escape_string(&s));
        let code = typst::syntax::parse_code(&src);
        let (errors, _) = code.errors_and_warnings();
        prop_assert!(
            errors.is_empty(),
            "escaping {:?} does not parse as code: {:?}", s, errors
        );
        let strs: Vec<_> = code
            .children()
            .filter(|n| n.kind() == SyntaxKind::Str)
            .collect();
        prop_assert_eq!(strs.len(), 1, "escaping {:?} lowered to {} literals", s, strs.len());
        let read = ast::Str::from_untyped(strs[0]).expect("a Str node").get();
        prop_assert_eq!(read.as_str(), s.as_str());
    }

    #[test]
    fn escape_markup_escapes_every_special_char(s in "\\PC*") {
        let escaped = escape_markup(&s);
        for &ch in TYPST_SPECIAL_CHARS {
            if s.contains(ch) {
                let escaped_form = format!("\\{}", ch);
                prop_assert!(escaped.contains(&escaped_form),
                    "Character '{}' in input '{}' not properly escaped in output '{}'",
                    ch, s, escaped);
            }
        }
    }

    #[test]
    fn escaped_markup_survives_the_typst_parser(s in escaper_input()) {
        // The escaper answers for the text a content holds, so the draw enters
        // through the content ingress, which spaces every separator: Typst
        // reads one as a line break that reopens `at_start`, and no escape
        // neutralizes it (a `\` before whitespace is its own linebreak).
        let s = to_plaintext(&from_plaintext(&s));
        // Past `at_start`, whose markers are the emitter's guard, not the escaper's.
        let (text, kinds) = resolve(&format!("x{}", escape_markup(&s)));

        prop_assert_eq!(&text, &format!("x{s}"),
            "escaping {:?} did not reach Typst as its own characters: {:?}", s, text);
        for k in kinds {
            prop_assert!(ALLOWED_LEAVES.contains(&k),
                "escaping {:?} lowered to a {:?} leaf", s, k);
        }
    }

    #[test]
    fn mark_to_typst_never_panics(s in "\\PC{0,1000}") {
        let _ = mark_to_typst(&s);
    }
}
