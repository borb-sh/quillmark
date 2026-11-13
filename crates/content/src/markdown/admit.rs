//! What a markdown import reads as structure, by the value's shape. A value
//! that holds no block reads the syntax that would open one as its
//! characters, so `1. 123 OG/CC` in an address is that text and no list.
//!
//! [`admitting`] rewrites the markdown ahead of the import, which reads one
//! grammar: each line that would open a block takes a backslash escape, an
//! indent drops, and a one-line value's newlines are spaces.

use std::borrow::Cow;

use super::{carrier, html};

/// What a value admits beyond marked text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// One line: a newline is a space, and block syntax is text.
    Line,
    /// Lines and paragraphs: block syntax is text.
    Lines,
    /// Every block.
    Blocks,
}

/// `markdown` as the import reads it under `admit`.
pub fn admitting(markdown: &str, admit: Admit) -> Cow<'_, str> {
    if admit == Admit::Blocks {
        return Cow::Borrowed(markdown);
    }
    let mut out = String::with_capacity(markdown.len() + 8);
    let mut joins = false;
    let mut continues = false;
    for (n, line) in markdown.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let text = line.trim_start_matches([' ', '\t']);
        if n > 0 {
            let folds = admit == Admit::Line && joins && !text.is_empty();
            out.push(if folds { ' ' } else { '\n' });
        }
        literal(text, continues, &mut out);
        continues = !text.is_empty();
        joins = !text.is_empty() && !text.ends_with('\\') && !text.ends_with("  ");
    }
    Cow::Owned(out)
}

/// `line`, its indent gone, with the character that would open a block
/// escaped. `continues` is whether the line before it holds text.
fn literal(line: &str, continues: bool, out: &mut String) {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &line[digits..];
    if (1..=9).contains(&digits) && rest.starts_with(['.', ')']) && ends_marker(&rest[1..]) {
        out.push_str(&line[..digits]);
        out.push('\\');
        out.push_str(rest);
        return;
    }
    if opens_block(line) || opens_html(line, continues) {
        out.push('\\');
    }
    out.push_str(line);
}

/// Whether what follows a list or heading marker lets it be one.
fn ends_marker(after: &str) -> bool {
    after.is_empty() || after.starts_with([' ', '\t'])
}

fn opens_block(line: &str) -> bool {
    let only = |set: &[char]| !line.is_empty() && line.chars().all(|c| set.contains(&c));
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    line.starts_with('>')
        || ((1..=6).contains(&hashes) && ends_marker(&line[hashes..]))
        || (line.starts_with(['-', '+', '*']) && ends_marker(&line[1..]))
        || line.starts_with("```")
        || line.starts_with("~~~")
        // A thematic break or a setext underline.
        || only(&['*', ' ', '\t'])
        || only(&['_', ' ', '\t'])
        || only(&['=', ' ', '\t'])
        // A setext underline, a thematic break or a table's delimiter row.
        || (line.contains('-') && only(&['-', '|', ':', ' ', '\t']))
        // A link reference or footnote definition, which reads as no text.
        || (line.starts_with('[') && line.contains("]:"))
}

/// Whether `line` opens an HTML block: a table, or markup the import drops
/// with the text after it read as blocks. A carrier tag alone on its line
/// passes, and so does any other tag alone on a line that continues a
/// paragraph, which it cannot interrupt.
fn opens_html(line: &str, continues: bool) -> bool {
    match (html::block_start(line), html::tag_at(line, 0)) {
        (None, _) => false,
        (Some(html::BlockKind::Open), Some(tag)) if !html::is_block_name(tag.name) => {
            !continues && carrier::element(tag.name).is_none()
        }
        (Some(_), _) => true,
    }
}

#[cfg(test)]
mod tests {
    mod properties;

    use super::*;
    use crate::markdown::{from_markdown, from_markdown_admitting};
    use crate::view::plaintext;

    fn lines(markdown: &str) -> crate::model::Content {
        from_markdown_admitting(markdown, Admit::Lines)
            .unwrap()
            .content
    }

    #[test]
    fn syntax_a_field_of_lines_does_not_admit_is_its_characters() {
        for typed in [
            "1. 123 OG/CC",
            "2) 123 MXG/CC",
            "- 123 MSG/CC",
            "+ one",
            "* one",
            "# 1 priority",
            "> quoted",
            "---",
            "***",
            "___",
            "```",
            "~~~",
            "    indented",
            "[OPR]: 123 CS/SCX",
            "<!-- c --> # one",
            "<div>",
        ] {
            let content = lines(typed);
            assert!(content.is_lines(), "{typed:?}");
            assert_eq!(plaintext(&content), typed.trim_start(), "{typed:?}");
        }
    }

    #[test]
    fn a_line_under_another_is_no_heading_and_no_table() {
        for typed in [
            "JOHN B. SMITH\n---",
            "Title\n===",
            "a | b\n--- | ---\nc | d",
        ] {
            let content = lines(typed);
            assert!(content.is_lines(), "{typed:?}");
            assert_eq!(content.blocks.len(), 1, "{typed:?}");
            assert_eq!(
                plaintext(&content).lines().count(),
                typed.lines().count(),
                "{typed:?}"
            );
        }
    }

    #[test]
    fn marks_still_read() {
        let content = lines("*ACC*/A6\n**123 OG**/CC");
        assert_eq!(plaintext(&content), "ACC/A6\n123 OG/CC");
        assert_eq!(content.marks.len(), 2);
    }

    #[test]
    fn a_blank_line_is_a_paragraph_break_and_a_newline_a_line_break() {
        let content = lines("one\ntwo\n\nthree");
        assert_eq!(content.blocks.len(), 2);
        assert_eq!(
            content,
            from_markdown("one\\\ntwo\n\nthree").unwrap().content
        );
    }

    #[test]
    fn a_one_line_value_reads_a_newline_as_a_space() {
        let content = from_markdown_admitting("A long\nsubject\n", Admit::Line)
            .unwrap()
            .content;
        assert!(content.is_inline());
        assert_eq!(plaintext(&content), "A long subject");
        let listed = from_markdown_admitting("1. Introduction", Admit::Line)
            .unwrap()
            .content;
        assert!(listed.is_inline());
        assert_eq!(plaintext(&listed), "1. Introduction");
    }

    #[test]
    fn every_block_reads_where_blocks_are_admitted() {
        let content = from_markdown_admitting("- one\n- two", Admit::Blocks)
            .unwrap()
            .content;
        assert!(!content.is_lines());
        assert_eq!(admitting("- one", Admit::Blocks), "- one");
    }
}
