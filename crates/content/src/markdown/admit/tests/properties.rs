//! The rewrite over generated markdown: lines built from the characters that
//! open a block, read under each shape that admits none. The import holds no
//! container, no block but a paragraph and no table.

use proptest::prelude::*;

use crate::markdown::{Admit, admitting, from_markdown_admitting};
use crate::model::{BlockKind, IslandType};

fn line() -> impl Strategy<Value = String> {
    let opener = prop::sample::select(vec![
        "",
        "",
        "1.",
        "12)",
        "-",
        "+",
        "*",
        "#",
        "###",
        ">",
        "```",
        "~~~",
        "---",
        "***",
        "___",
        "===",
        "|",
        "| a | b |",
        "|---|---|",
        "--- | ---",
        ":-: | :-:",
        "[a]:",
        "[^a]:",
        "<table>",
        "<tr><td>",
        "</table>",
        "<div>",
        "<!--",
        "$$",
        "\\",
        "![a](b)",
        ": a",
    ]);
    (
        "[ \t]{0,5}",
        opener,
        r"[ \t]?[a-z0-9 .)#>|:=*_`~<\[\]\\\-]{0,6}[ \\]{0,2}",
    )
        .prop_map(|(indent, opener, rest)| format!("{indent}{opener}{rest}"))
}

fn markdown() -> impl Strategy<Value = String> {
    prop::collection::vec(line(), 1..6).prop_map(|lines| lines.join("\n"))
}

proptest! {
    #[test]
    fn a_rewrite_read_again_imports_the_same(md in markdown()) {
        for admit in [Admit::Line, Admit::Lines] {
            let once = admitting(&md, admit).into_owned();
            let read = |md: &str| from_markdown_admitting(md, Admit::Blocks).ok().map(|i| i.content);
            prop_assert_eq!(read(&admitting(&once, admit)), read(&once), "{:?}", md);
        }
    }

    #[test]
    fn a_value_that_admits_no_block_imports_paragraphs_in_no_container(md in markdown()) {
        for admit in [Admit::Line, Admit::Lines] {
            if let Ok(imported) = from_markdown_admitting(&md, admit) {
                let content = imported.content;
                prop_assert!(
                    content.blocks.iter().all(|b| b.kind == BlockKind::Para && b.path.is_empty()),
                    "{:?} -> {:?}", md, content.blocks
                );
                prop_assert!(
                    content.islands.iter().all(|i| i.island_type != IslandType::Table),
                    "{:?} -> {:?}", md, content.islands
                );
            }
        }
    }
}
