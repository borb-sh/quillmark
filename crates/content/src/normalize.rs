//! Markdown-string preprocessing run before parsing, at the
//! [`from_markdown`](crate::import::from_markdown) boundary.

/// The Unicode bidi formatting controls, which sit adjacent to `**`/`_` and
/// defeat delimiter recognition.
#[inline]
pub(crate) fn is_bidi_char(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' // ARABIC LETTER MARK (ALM)
        | '\u{200E}' // LEFT-TO-RIGHT MARK (LRM)
        | '\u{200F}' // RIGHT-TO-LEFT MARK (RLM)
        | '\u{202A}' // LEFT-TO-RIGHT EMBEDDING (LRE)
        | '\u{202B}' // RIGHT-TO-LEFT EMBEDDING (RLE)
        | '\u{202C}' // POP DIRECTIONAL FORMATTING (PDF)
        | '\u{202D}' // LEFT-TO-RIGHT OVERRIDE (LRO)
        | '\u{202E}' // RIGHT-TO-LEFT OVERRIDE (RLO)
        | '\u{2066}' // LEFT-TO-RIGHT ISOLATE (LRI)
        | '\u{2067}' // RIGHT-TO-LEFT ISOLATE (RLI)
        | '\u{2068}' // FIRST STRONG ISOLATE (FSI)
        | '\u{2069}' // POP DIRECTIONAL ISOLATE (PDI)
    )
}

/// Every character Typst's lexer reads as a line break (`is_newline`) besides
/// `\n` and `\r`: one mid-paragraph reopens `at_start`, so what follows it is
/// read as a block marker the author never wrote, and two in a row are a
/// paragraph break.
#[inline]
pub fn is_line_separator(c: char) -> bool {
    matches!(
        c,
        '\u{000B}' // LINE TABULATION (VT)
        | '\u{000C}' // FORM FEED (FF)
        | '\u{0085}' // NEXT LINE (NEL)
        | '\u{2028}' // LINE SEPARATOR
        | '\u{2029}' // PARAGRAPH SEPARATOR
    )
}

/// What the content admits `c` as, `None` dropping it. A `\r` is dropped
/// because it pairs with a `\n` that stays; a line separator becomes a space,
/// both being Unicode whitespace, so dropping one would join the words it parts.
/// No downstream escape can neutralize a separator — a `\` before whitespace is
/// Typst's own linebreak — so it cannot survive in the text.
#[inline]
pub(crate) fn admit_char(c: char) -> Option<char> {
    match c {
        '\r' => None,
        c if is_bidi_char(c) => None,
        c if is_line_separator(c) => Some(' '),
        c => Some(c),
    }
}

fn admit_chars(s: &str) -> String {
    if !s.chars().any(|c| admit_char(c) != Some(c)) {
        return s.to_string();
    }

    s.chars().filter_map(admit_char).collect()
}

/// Inserts a newline after `-->` when followed by non-whitespace content.
///
/// CommonMark HTML block type 2 ends with the line containing `-->`, so text on
/// that line after `-->` would be swallowed. Bare `-->` outside a comment is
/// left untouched.
///
/// Only a comment **opening a line** starts such a block; one reached mid-line
/// is inline HTML, which swallows nothing, so a break inserted there would split
/// a paragraph the source did not.
fn fix_html_comment_fences(s: &str) -> String {
    if !s.contains("-->") {
        return s.to_string();
    }

    let mut result = String::with_capacity(s.len() + 16);
    let mut current_pos = 0;

    while let Some(open_idx) = s[current_pos..].find("<!--") {
        let abs_open = current_pos + open_idx;

        if let Some(close_idx) = s[abs_open..].find("-->") {
            let abs_close = abs_open + close_idx;
            let mut after_fence = abs_close + 3;

            // Handle `<!--- ... --->` style fences: the extra hyphen is part of
            // the fence, not leaked trailing text.
            let opener_has_extra_hyphen = s
                .get(abs_open + 4..)
                .is_some_and(|rest| rest.starts_with('-'));
            if opener_has_extra_hyphen
                && s.get(after_fence..)
                    .is_some_and(|rest| rest.starts_with('-'))
            {
                after_fence += 1;
            }

            result.push_str(&s[current_pos..after_fence]);

            let after_content = &s[after_fence..];

            // An HTML block opens on at most three spaces of indent.
            let line_start = s[..abs_open].rfind('\n').map_or(0, |i| i + 1);
            let indent = &s[line_start..abs_open];
            let opens_block = indent.len() <= 3 && indent.chars().all(|c| c == ' ');

            let needs_newline = if !opens_block
                || after_content.is_empty()
                || after_content.starts_with('\n')
                || after_content.starts_with("\r\n")
            {
                false
            } else {
                let next_newline = after_content.find('\n');
                let until_newline = match next_newline {
                    Some(pos) => &after_content[..pos],
                    None => after_content,
                };
                !until_newline.trim().is_empty()
            };

            if needs_newline {
                result.push('\n');
            }

            current_pos = after_fence;
        } else {
            // Unclosed comment: append the rest and stop.
            result.push_str(&s[current_pos..]);
            current_pos = s.len();
            break;
        }
    }

    if current_pos < s.len() {
        result.push_str(&s[current_pos..]);
    }

    result
}

/// Applies all markdown normalizations in order: CRLF → LF, bidi controls
/// dropped and line separators spaced, HTML comment fence repair.
pub fn normalize_markdown(markdown: &str) -> String {
    let cleaned = normalize_line_endings(markdown);
    let cleaned = admit_chars(&cleaned);
    fix_html_comment_fences(&cleaned)
}

// Applied only to the Markdown body (spec §7): YAML parsing normalizes its own
// scalars but passes the body verbatim, and some Windows/clipboard sources
// leave bare `\r` bytes.
fn normalize_line_endings(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bidi_controls_are_dropped_and_separators_spaced() {
        let cases: &[(&str, &str)] = &[
            ("hello world", "hello world"),
            ("", ""),
            ("**bold** text", "**bold** text"),
            ("intro\u{2028}- item", "intro - item"),
            ("intro\u{2029}= Heading", "intro = Heading"),
            ("intro\u{000B}- item", "intro - item"),
            ("intro\u{000C}- item", "intro - item"),
            ("intro\u{0085}= Heading", "intro = Heading"),
            ("- one\u{000C}\u{000C}two", "- one  two"),
            ("he\u{202D}llo", "hello"),
            ("**asdf** or \u{202D}**(1234**", "**asdf** or **(1234**"),
            ("a\u{200E}b\u{200F}c", "abc"),
            ("\u{202A}text\u{202B}more\u{202C}", "textmore"),
            ("\u{2066}a\u{2067}b\u{2068}c\u{2069}", "abc"),
            (
                "\u{061C}\u{200E}\u{200F}\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}",
                "",
            ),
            ("hello\u{061C}world", "helloworld"),
            ("\u{061C}**bold**", "**bold**"),
            ("你好世界", "你好世界"),
            ("مرحبا", "مرحبا"),
            ("🎉", "🎉"),
        ];

        for (input, expected) in cases {
            assert_eq!(admit_chars(input), *expected, "input: {:?}", input);
        }
    }

    #[test]
    fn test_normalize_markdown_basic() {
        assert_eq!(normalize_markdown("hello"), "hello");
        assert_eq!(
            normalize_markdown("**bold** \u{202D}**more**"),
            "**bold** **more**"
        );
    }

    #[test]
    fn test_fix_html_comment_fences_cases() {
        let cases: &[(&str, &str)] = &[
            ("hello world", "hello world"),
            ("**bold** text", "**bold** text"),
            ("", ""),
            (
                "<!-- comment -->Same line text",
                "<!-- comment -->\nSame line text",
            ),
            (
                "<!-- comment -->\nNext line text",
                "<!-- comment -->\nNext line text",
            ),
            (
                "<!-- comment -->   \nSome text",
                "<!-- comment -->   \nSome text",
            ),
            (
                "<!--\nmultiline\ncomment\n-->Trailing text",
                "<!--\nmultiline\ncomment\n-->\nTrailing text",
            ),
            (
                "<!--\nmultiline\n-->\n\nParagraph text",
                "<!--\nmultiline\n-->\n\nParagraph text",
            ),
            (
                "<!-- first -->Text\n\n<!-- second -->More text",
                "<!-- first -->\nText\n\n<!-- second -->\nMore text",
            ),
            (
                "Some text before <!-- comment -->",
                "Some text before <!-- comment -->",
            ),
            // Mid-line, the comment is inline HTML and swallows nothing, so the
            // text after it stays on the same line.
            (
                "Some text before <!-- comment -->and after",
                "Some text before <!-- comment -->and after",
            ),
            // Three spaces still open a block; four are an indented code line.
            ("   <!-- c -->Text", "   <!-- c -->\nText"),
            ("    <!-- c -->Text", "    <!-- c -->Text"),
            ("-->some text", "-->some text"),
            // The first <!-- opens, the first --> closes; inner <!-- is just text.
            ("<!-- <!-- -->Trailing", "<!-- <!-- -->\nTrailing"),
            (
                "<!-- valid -->FixMe\ntext --> Ignore\n<!-- valid2 -->FixMe2",
                "<!-- valid -->\nFixMe\ntext --> Ignore\n<!-- valid2 -->\nFixMe2",
            ),
            (
                "<!-- comment -->\r\nSome text",
                "<!-- comment -->\r\nSome text",
            ),
            (
                "<!--- comment --->Trailing text",
                "<!--- comment --->\nTrailing text",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(
                fix_html_comment_fences(input),
                *expected,
                "input: {:?}",
                input
            );
        }
    }
}
