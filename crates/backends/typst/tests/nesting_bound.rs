//! Content nested to the import bound renders. Typst refuses past a show-rule
//! depth of 64, which each quote, list and element level spends from and the
//! marks around the deepest run spend from again.

use quillmark_content::{
    export::to_markdown,
    import::{from_markdown, ImportError},
    serial::to_canonical_value,
    MAX_NESTING_DEPTH,
};
use quillmark_core::{backend::Backend, types::RenderOptions};
use quillmark_typst::TypstBackend;

mod common;

/// Every show rule the emitter wraps a run in, nested at once.
const MARKED: &str = "**_~~<u>[`x`](https://example.com)</u>~~_**";

/// The same marks, the code beside the link: export spells no mark inside a
/// link's text.
const EXPORTABLE: &str = "**_~~<u>[x](https://example.com) `y`</u>~~_**";

/// `inner` inside `n` nested `qm-keep` elements, the one element every quill
/// renders.
fn elements(n: usize, inner: &str) -> String {
    format!("{}{inner}{}", "<qm-keep>\n\n".repeat(n), "\n\n</qm-keep>".repeat(n))
}

#[test]
fn containers_alone_count_toward_the_bound() {
    let n = MAX_NESTING_DEPTH;
    let nest = |level: &str, n: usize, inner: &str| match level {
        "element" => elements(n, inner),
        _ => format!("{}{inner}", level.repeat(n)),
    };
    for level in ["> ", "- ", "1. ", "element"] {
        let deepest = from_markdown(&nest(level, n, MARKED))
            .expect("imports at the bound")
            .content
            .lines
            .iter()
            .map(|l| l.containers.len())
            .max();
        assert_eq!(deepest, Some(n), "{level:?}");
        assert!(
            matches!(
                from_markdown(&nest(level, n + 1, "x")),
                Err(ImportError::NestingTooDeep { depth, max }) if depth == n + 1 && max == n
            ),
            "{level:?} past the bound"
        );
    }
}

#[test]
fn marked_content_nested_to_the_bound_renders_and_round_trips() {
    let quill = common::quill_with_plate(
        &common::yaml("main:\n  fields: {}\n"),
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n#qm.data.at(\"$body\", default: [])\n",
    );
    let n = MAX_NESTING_DEPTH;
    let alternating = |a: &str, b: &str| -> String {
        (0..n).map(|i| if i % 2 == 0 { a } else { b }).collect()
    };
    let shapes: [&dyn Fn(&str) -> String; 6] = [
        &|marks| format!("{}{marks}", "> ".repeat(n)),
        &|marks| format!("{}{marks}", "- ".repeat(n)),
        &|marks| format!("{}{marks}", "1. ".repeat(n)),
        &|marks| format!("{}{marks}", alternating("> ", "- ")),
        &|marks| format!("{}{marks}", alternating("1. ", "> ")),
        &|marks| elements(n, marks),
    ];
    for shape in shapes {
        for marks in [MARKED, EXPORTABLE] {
            let markdown = shape(marks);
            let imported = from_markdown(&markdown).expect("imports at the bound");
            assert!(imported.warnings.is_empty(), "{markdown:?}: {:?}", imported.warnings);
            let content = imported.content;
            if marks == EXPORTABLE {
                let exported = to_markdown(&content);
                assert_eq!(
                    from_markdown(&exported).map(|i| i.content).as_ref(),
                    Ok(&content),
                    "{exported:?} re-imports"
                );
            }
            let data = serde_json::json!({ "$body": to_canonical_value(&content) });
            let session = TypstBackend::default()
                .open(&quill, &data, common::test_date())
                .unwrap_or_else(|e| panic!("{markdown:?} opens: {e}"));
            assert!(session.warnings().is_empty(), "{markdown:?}: {:?}", session.warnings());
            let result = session
                .render(&RenderOptions::default())
                .unwrap_or_else(|e| panic!("{markdown:?} renders: {e}"));
            assert!(result.warnings.is_empty(), "{markdown:?}: {:?}", result.warnings);
        }
    }
}
