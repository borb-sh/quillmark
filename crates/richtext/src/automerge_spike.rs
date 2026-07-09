//! Spike (not shipped): the [`automerge`] counterpart to `loro_spike.rs` —
//! same two empirical questions, same fixture, so the two are comparable.
//! Gated behind the `automerge` feature; never reaches a default build.
//!
//! 1. **Determinism.** Does `AutoCommit::save()` (raw change history) differ
//!    across edit histories for identical visible content, the way Loro's
//!    snapshot does? Does the materialized `ReadDoc::text()` +
//!    `ReadDoc::marks()` view stay content-addressable?
//! 2. **Shape fit.** `ReadDoc::marks()` already returns a range list
//!    (`Mark { start, end, name, value }`), not a run list like Loro's
//!    `to_delta()` — closer to `RichText::Mark` out of the box. Confirm the
//!    conversion still round-trips to the same canonical bytes as
//!    `import::from_markdown` for equivalent content.

#![cfg(test)]

use crate::model::{Line, LineKind, Mark as RtMark, MarkKind, RichText};
use automerge::{marks::ExpandMark, marks::Mark, transaction::Transactable, AutoCommit, ObjType};

fn kind_for_name(n: &str) -> MarkKind {
    match n {
        "bold" => MarkKind::Strong,
        "italic" => MarkKind::Emph,
        other => MarkKind::Unknown {
            tag: other.to_string(),
            attrs: serde_json::Value::Null,
        },
    }
}

/// `ReadDoc::marks()` already hands back ranges — unlike Loro's run-list
/// `to_delta()`, this is close to a direct copy into `RichText::Mark`.
fn automerge_marks_to_richtext(text: &str, marks: &[Mark]) -> RichText {
    let rt_marks = marks
        .iter()
        .filter_map(|m| {
            let automerge::ScalarValue::Boolean(true) = m.value() else {
                return None;
            };
            Some(RtMark {
                start: m.start,
                end: m.end,
                kind: kind_for_name(m.name()),
            })
        })
        .collect();

    let mut rt = RichText {
        text: text.to_string(),
        lines: vec![Line {
            kind: LineKind::Para,
            containers: Vec::new(),
            continues: false,
        }],
        marks: rt_marks,
        islands: Vec::new(),
    };
    rt.normalize();
    rt
}

#[cfg(test)]
mod spike {
    use super::*;
    use automerge::ReadDoc;

    /// Same visible text via two actors / two different edit sequences.
    /// Expect: raw `save()` bytes differ (change-history encoded); the
    /// materialized `text()` + `marks()` view is equal.
    #[test]
    fn save_bytes_are_not_content_addressable_but_materialized_view_is() {
        let mut doc_a = AutoCommit::new();
        let text_a = doc_a
            .put_object(automerge::ROOT, "body", ObjType::Text)
            .unwrap();
        doc_a.splice_text(&text_a, 0, 0, "Hello world, formatted.").unwrap();
        doc_a
            .mark(&text_a, Mark::new("bold".into(), true, 0, 5), ExpandMark::None)
            .unwrap();
        doc_a
            .mark(&text_a, Mark::new("italic".into(), true, 6, 11), ExpandMark::None)
            .unwrap();

        let mut doc_b = AutoCommit::new();
        let text_b = doc_b
            .put_object(automerge::ROOT, "body", ObjType::Text)
            .unwrap();
        doc_b.splice_text(&text_b, 0, 0, "Hello").unwrap();
        doc_b
            .mark(&text_b, Mark::new("bold".into(), true, 0, 5), ExpandMark::None)
            .unwrap();
        doc_b.splice_text(&text_b, 5, 0, "XXXXX").unwrap(); // overwritten below
        doc_b.splice_text(&text_b, 5, 5, " world").unwrap();
        doc_b
            .mark(&text_b, Mark::new("italic".into(), true, 6, 11), ExpandMark::None)
            .unwrap();
        doc_b.splice_text(&text_b, 11, 0, ", formatted.").unwrap();

        let str_a = doc_a.text(&text_a).unwrap();
        let str_b = doc_b.text(&text_b).unwrap();
        assert_eq!(str_a, str_b, "sanity: same visible text");

        let save_a = doc_a.save();
        let save_b = doc_b.save();
        assert_ne!(
            save_a, save_b,
            "raw save() bytes differ across edit histories for identical content — \
             not usable as a content hash / canonical storage form as-is"
        );

        let marks_a = doc_a.marks(&text_a).unwrap();
        let marks_b = doc_b.marks(&text_b).unwrap();
        assert_eq!(
            marks_a, marks_b,
            "materialized text()+marks() IS content-addressable — same conclusion as Loro"
        );
    }

    /// `marks()` + `text()` round-trip through the corpus model and match
    /// `import::from_markdown` for equivalent markdown.
    #[test]
    fn materialized_view_converts_to_valid_richtext_matching_markdown_import() {
        let mut doc = AutoCommit::new();
        let text = doc
            .put_object(automerge::ROOT, "body", ObjType::Text)
            .unwrap();
        doc.splice_text(&text, 0, 0, "Hello world").unwrap();
        doc.mark(&text, Mark::new("bold".into(), true, 0, 5), ExpandMark::None)
            .unwrap();

        let s = doc.text(&text).unwrap();
        let marks = doc.marks(&text).unwrap();
        let rt = automerge_marks_to_richtext(&s, &marks);
        assert_eq!(rt.validate(), Ok(()));

        let from_md = crate::import::from_markdown("**Hello** world").unwrap();
        assert_eq!(rt.text, from_md.text);
        assert_eq!(rt.marks, from_md.marks);
        assert_eq!(rt.to_canonical_json(), from_md.to_canonical_json());
    }

    /// Serialized-size comparison for the same fixture used in the Loro spike.
    #[test]
    fn size_comparison_reports_to_stderr() {
        let fixture = include_str!("../../fixtures/resources/sample.md");
        let rt = crate::import::from_markdown(fixture).unwrap();
        let corpus_json = rt.to_canonical_json();

        let mut doc = AutoCommit::new();
        let text = doc
            .put_object(automerge::ROOT, "body", ObjType::Text)
            .unwrap();
        doc.splice_text(&text, 0, 0, &rt.text).unwrap();
        for m in &rt.marks {
            let name = match &m.kind {
                MarkKind::Strong => "bold",
                MarkKind::Emph => "italic",
                _ => continue,
            };
            doc.mark(
                &text,
                Mark::new(name.into(), true, m.start, m.end),
                ExpandMark::None,
            )
            .unwrap();
        }
        let save = doc.save();

        eprintln!(
            "[automerge-spike] fixture=sample.md corpus_canonical_json_bytes={} automerge_save_bytes={} ratio={:.2}x",
            corpus_json.len(),
            save.len(),
            save.len() as f64 / corpus_json.len() as f64
        );
    }
}
