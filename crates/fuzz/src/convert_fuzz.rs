use proptest::prelude::*;
use quillmark_content::export::to_plaintext;
use quillmark_content::import::from_plaintext;
use quillmark_typst::emit::{escape_markup, escape_string};
use typst::syntax::SyntaxKind;

/// The render path: import markdown to a `Content`, then lower it to markup.
fn mark_to_typst(markdown: &str) -> Result<String, String> {
    let rt = quillmark_content::import::from_markdown(markdown).map_err(|e| e.to_string())?;
    quillmark_typst::emit::emit_content(&rt)
        .map(|ec| ec.markup)
        .map_err(|e| e.to_string())
}

// The markup escapes that fire on the character alone.
const TYPST_SPECIAL_CHARS: &[char] = &[
    '~', '*', '_', '`', '#', '[', ']', '{', '}', '$', '<', '>', '@',
];

/// What escaped document text may lower to. `SmartQuote` is the declared
/// exception: `'` and `"` stay authored so a quill sets its own typography with
/// `#set smartquote(…)`.
const ALLOWED_LEAVES: &[SyntaxKind] = &[
    SyntaxKind::Text,
    SyntaxKind::Space,
    SyntaxKind::Parbreak,
    SyntaxKind::Escape,
    SyntaxKind::SmartQuote,
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

/// What Typst resolves a markup fragment to: its characters, and the leaf kinds
/// it read them as. A comment resolves to nothing and a shorthand to the
/// codepoint it substitutes, so either shows as a mismatch rather than as its
/// own source text.
fn resolve(markup: &str) -> (String, Vec<SyntaxKind>) {
    use typst::syntax::{ast, ast::AstNode, SyntaxNode};

    fn walk(n: &SyntaxNode, text: &mut String, kinds: &mut Vec<SyntaxKind>) {
        if n.children().len() > 0 {
            for c in n.children() {
                walk(c, text, kinds);
            }
            return;
        }
        kinds.push(n.kind());
        match n.kind() {
            SyntaxKind::Escape => text.push(ast::Escape::from_untyped(n).unwrap().get()),
            SyntaxKind::Shorthand => text.push(ast::Shorthand::from_untyped(n).unwrap().get()),
            SyntaxKind::LineComment | SyntaxKind::BlockComment => {}
            _ => text.push_str(n.leaf_text()),
        }
    }

    let (mut text, mut kinds) = (String::new(), Vec::new());
    walk(&typst::syntax::parse(markup), &mut text, &mut kinds);
    (text, kinds)
}

proptest! {
    #[test]
    fn fuzz_escape_string_no_raw_quotes(s in "\\PC*") {
        let escaped = escape_string(&s);
        let chars: Vec<char> = escaped.chars().collect();
        for i in 0..chars.len() {
            if chars[i] == '"' {
                assert!(i > 0 && chars[i-1] == '\\',
                    "Found unescaped quote at position {} in escaped string: {:?}", i, escaped);
            }
        }
    }

    #[test]
    fn fuzz_escape_markup_typst_chars_escaped(s in "\\PC*") {
        let escaped = escape_markup(&s);
        for &ch in TYPST_SPECIAL_CHARS {
            if s.contains(ch) {
                let escaped_form = format!("\\{}", ch);
                assert!(escaped.contains(&escaped_form),
                    "Character '{}' in input '{}' not properly escaped in output '{}'",
                    ch, s, escaped);
            }
        }
    }

    #[test]
    fn fuzz_escape_markup_survives_the_typst_parser(s in escaper_input()) {
        // The escaper answers for the text a content holds, so the draw enters
        // through the content ingress, which spaces every separator: Typst
        // reads one as a line break that reopens `at_start`, and no escape
        // neutralizes it (a `\` before whitespace is its own linebreak).
        let s = to_plaintext(&from_plaintext(&s));
        // Past `at_start`, whose markers are the emitter's guard, not the escaper's.
        let (text, kinds) = resolve(&format!("x{}", escape_markup(&s)));

        assert_eq!(text, format!("x{s}"),
            "escaping {:?} did not reach Typst as its own characters: {:?}", s, text);
        for k in kinds {
            assert!(ALLOWED_LEAVES.contains(&k),
                "escaping {:?} lowered to a {:?} leaf", s, k);
        }
    }

    #[test]
    fn fuzz_mark_to_typst_no_panic(s in "\\PC{0,1000}") {
        let _ = mark_to_typst(&s);
    }

}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn fuzz_escape_string_injection_safety(s in "[\\\\\"].*[\\\\\"].*") {
        let escaped = escape_string(&s);

        // Patterns that would break out of the string context.
        let dangerous_patterns = [
            "\"); ",
            "\")); ",
            "\\\"); ",
        ];

        for pattern in &dangerous_patterns {
            assert!(!escaped.contains(pattern),
                "Dangerous pattern '{}' found in escaped output: {}", pattern, escaped);
        }
    }

    #[test]
    fn fuzz_markdown_parser_malicious_nesting(depth in 1usize..20) {
        let nested_quotes = "> ".repeat(depth) + "text";
        let result = mark_to_typst(&nested_quotes).expect("Conversion should succeed");
        assert!(!result.is_empty() || depth == 0);
    }

    #[test]
    fn fuzz_markdown_parser_malicious_lists(depth in 1usize..20) {
        let nested_list = (0..depth)
            .map(|i| format!("{}- item", "  ".repeat(i)))
            .collect::<Vec<_>>()
            .join("\n");
        let result = mark_to_typst(&nested_list).expect("Conversion should succeed");
        assert!(!result.is_empty());
    }

    #[test]
    fn fuzz_markdown_large_input(size in 1usize..10000) {
        let input = "a".repeat(size);
        let result = mark_to_typst(&input).expect("Conversion should succeed");
        assert!(result.contains("a"));
    }
}
