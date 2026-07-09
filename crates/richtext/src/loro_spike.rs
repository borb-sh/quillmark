//! Spike (not shipped): measure what backing [`crate::model::RichText`] with a
//! [`loro`] CRDT document would actually cost, ahead of the eventual real-time
//! collaboration goal. Gated behind the `loro` feature so it never reaches a
//! default build; see the workspace overhead numbers this module was written
//! to produce, reported alongside it rather than in this file.
//!
//! Two questions this answers empirically, not by reading docs:
//!
//! 1. **Determinism.** [`RichText::to_canonical_json`] promises byte-identical
//!    output for equal content regardless of *how* it was produced — the
//!    corpus is used as a content hash. A CRDT's raw exported state encodes
//!    op history (peer ids, Lamport counters, tombstones), so it is *not*
//!    content-addressable. `LoroText::to_delta()` (the materialized Quill-Delta
//!    view), by contrast, should be — this spike checks that empirically by
//!    building the same visible text via two different peers/edit orders and
//!    comparing both the raw snapshot bytes (expected: differ) and the
//!    materialized delta (expected: equal).
//! 2. **Shape fit.** Loro's rich text is a flat run list (Quill Delta:
//!    `{insert, attributes}`), not `RichText`'s range-list-with-union-algebra.
//!    This checks the run→range conversion is mechanical (same shape of
//!    problem the pulldown-cmark event walk in [`crate::import`] already
//!    solves) and that the result normalizes/validates identically to the
//!    existing markdown-import path for equivalent content.

use crate::model::{Line, LineKind, Mark, MarkKind, RichText};
use loro::{ExpandType, ExportMode, LoroDoc, StyleConfig, StyleConfigMap, TextDelta};

/// Loro marks default to [`ExpandType::After`] — text inserted right after a
/// mark's end boundary is silently absorbed into it. `RichText` has no such
/// concept (marks are exact ranges recomputed by each edit, never a live
/// growing boundary), so every key must be pinned to `None` to get the
/// CommonMark-style exact-bounds semantics the corpus model assumes. Finding
/// #1 of this spike, discovered by a failing assertion, not read from docs.
fn exact_bounds_styles() -> StyleConfigMap {
    let mut styles = StyleConfigMap::new();
    for key in ["bold", "italic"] {
        styles.insert(
            key.into(),
            StyleConfig {
                expand: ExpandType::None,
            },
        );
    }
    styles
}

/// Convert a Loro `to_delta()` run list into `RichText` range-marks over one
/// `Para` line. Mirrors the run→range fold `import::from_markdown` does over
/// `pulldown_cmark::Event`s — same shape of problem, different producer.
fn delta_to_richtext(delta: &[TextDelta]) -> RichText {
    let mut text = String::new();
    let mut marks: Vec<Mark> = Vec::new();
    // Open runs per attribute key, closed (and pushed to `marks`) the moment a
    // later delta run doesn't carry that key — the same "extend or close"
    // fold `normalize_marks` performs on already-flat ranges, run upstream of
    // it here because Loro hands us runs, not ranges.
    let mut open: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for run in delta {
        let TextDelta::Insert { insert, attributes } = run else {
            continue;
        };
        let start = text.chars().count();
        text.push_str(insert);
        let end = text.chars().count();

        let keys: std::collections::HashSet<String> = attributes
            .as_ref()
            .map(|a| a.keys().cloned().collect())
            .unwrap_or_default();

        // Close any open run whose key this insert no longer carries.
        let to_close: Vec<String> = open.keys().filter(|k| !keys.contains(*k)).cloned().collect();
        for k in to_close {
            let s = open.remove(&k).unwrap();
            marks.push(Mark {
                start: s,
                end: start,
                kind: kind_for_key(&k),
            });
        }
        // Open any new key this insert carries that wasn't already open.
        for k in &keys {
            open.entry(k.clone()).or_insert(start);
        }
        let _ = end;
    }
    // Close whatever is still open at end-of-text.
    let len = text.chars().count();
    for (k, s) in open {
        marks.push(Mark {
            start: s,
            end: len,
            kind: kind_for_key(&k),
        });
    }

    let mut rt = RichText {
        text,
        lines: vec![Line {
            kind: LineKind::Para,
            containers: Vec::new(),
            continues: false,
        }],
        marks,
        islands: Vec::new(),
    };
    rt.normalize();
    rt
}

fn kind_for_key(k: &str) -> MarkKind {
    match k {
        "bold" => MarkKind::Strong,
        "italic" => MarkKind::Emph,
        other => MarkKind::Unknown {
            tag: other.to_string(),
            attrs: serde_json::Value::Null,
        },
    }
}

#[cfg(test)]
mod spike {
    use super::*;

    /// Build the same visible text ("Hello world, formatted.") via two
    /// different peers and two different edit orders/insertion points, then
    /// compare (a) raw exported snapshot bytes — expected to differ, since
    /// they encode different op histories — against (b) the materialized
    /// `to_delta()` view — expected to be equal, since it reflects only
    /// current content.
    #[test]
    fn snapshot_bytes_are_not_content_addressable_but_delta_is() {
        // Peer A: type it left-to-right in one pass.
        let doc_a = LoroDoc::new();
        doc_a.set_peer_id(1).unwrap();
        doc_a.config_text_style(exact_bounds_styles());
        let text_a = doc_a.get_text("body");
        text_a.insert(0, "Hello world, formatted.").unwrap();
        text_a.mark(0..5, "bold", true).unwrap();
        text_a.mark(6..11, "italic", true).unwrap();
        doc_a.commit();

        // Peer B: same final text, built via a different peer id, different
        // insertion order (word-by-word, marks applied before the trailing
        // text exists) and an intermediate edit that gets overwritten later —
        // a different op history converging on the identical visible result.
        let doc_b = LoroDoc::new();
        doc_b.set_peer_id(2).unwrap();
        doc_b.config_text_style(exact_bounds_styles());
        let text_b = doc_b.get_text("body");
        text_b.insert(0, "Hello").unwrap();
        text_b.mark(0..5, "bold", true).unwrap();
        text_b.insert(5, "XXXXX").unwrap(); // will be overwritten
        text_b.delete(5, 5).unwrap();
        text_b.insert(5, " world").unwrap();
        text_b.mark(6..11, "italic", true).unwrap();
        text_b.insert(11, ", formatted.").unwrap();
        doc_b.commit();

        assert_eq!(text_a.to_string(), text_b.to_string(), "sanity: same visible text");

        let snap_a = doc_a.export(ExportMode::Snapshot).unwrap();
        let snap_b = doc_b.export(ExportMode::Snapshot).unwrap();
        assert_ne!(
            snap_a, snap_b,
            "raw CRDT snapshots differ across edit histories for identical content — \
             NOT usable as a content hash / canonical storage form as-is"
        );

        let delta_a = text_a.to_delta();
        let delta_b = text_b.to_delta();
        assert_eq!(
            delta_a, delta_b,
            "materialized to_delta() IS content-addressable — a canonical corpus \
             must be derived from this view, never from the raw snapshot"
        );
    }

    /// The run→range conversion from `to_delta()` produces a `RichText` that
    /// validates and matches what `import::from_markdown` produces for
    /// equivalent markdown — i.e. the shape fits the existing corpus model
    /// without inventing new mark semantics.
    #[test]
    fn delta_converts_to_valid_richtext_matching_markdown_import() {
        let doc = LoroDoc::new();
        doc.config_text_style(exact_bounds_styles());
        let text = doc.get_text("body");
        text.insert(0, "Hello world").unwrap();
        text.mark(0..5, "bold", true).unwrap();
        doc.commit();

        let rt = delta_to_richtext(&text.to_delta());
        assert_eq!(rt.validate(), Ok(()));

        let from_md = crate::import::from_markdown("**Hello** world").unwrap();
        assert_eq!(rt.text, from_md.text);
        assert_eq!(rt.marks, from_md.marks);
        assert_eq!(rt.to_canonical_json(), from_md.to_canonical_json());
    }

    /// Serialized-size comparison for a representative document: canonical
    /// corpus JSON vs. Loro's own snapshot export, for the same content built
    /// fresh (no cross-peer merge history to inflate the CRDT side further).
    #[test]
    fn size_comparison_reports_to_stderr() {
        let fixture = include_str!("../../fixtures/resources/sample.md");
        let rt = crate::import::from_markdown(fixture).unwrap();
        let corpus_json = rt.to_canonical_json();

        let doc = LoroDoc::new();
        doc.config_text_style(exact_bounds_styles());
        let text = doc.get_text("body");
        text.insert(0, &rt.text).unwrap();
        for m in &rt.marks {
            if m.kind.is_formatting() {
                let key = match &m.kind {
                    MarkKind::Strong => "bold",
                    MarkKind::Emph => "italic",
                    _ => continue,
                };
                text.mark(m.start..m.end, key, true).unwrap();
            }
        }
        doc.commit();
        let snapshot = doc.export(ExportMode::Snapshot).unwrap();

        eprintln!(
            "[loro-spike] fixture={} corpus_canonical_json_bytes={} loro_snapshot_bytes={} ratio={:.2}x",
            "sample.md",
            corpus_json.len(),
            snapshot.len(),
            snapshot.len() as f64 / corpus_json.len() as f64
        );
    }
}
