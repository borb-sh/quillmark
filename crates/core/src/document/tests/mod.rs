mod anchor_tests;
mod assemble_tests;
mod base_tests;
mod card_fence_tests;
mod edit_tests;
mod emit_tests;
mod ext_tests;
mod fence_conformance_tests;
mod held_tests;
mod lossiness_tests;
mod multibyte_tests;
mod properties;
mod revise_tests;
mod seed_tests;
mod write_tests;

use std::path::{Path, PathBuf};

/// Parse `src`, failing the test on a parse error and dropping the warnings.
pub(super) fn parse(src: &str) -> super::Document {
    super::Document::parse(src)
        .expect("source should parse")
        .document
}

/// The id of each block and island of `after` that `before` holds
/// none of, blocks first.
pub(super) fn put_ids(before: &quillmark_automerge::content::model::Content, after: &quillmark_automerge::content::model::Content) -> Vec<String> {
    let had: std::collections::HashSet<&str> = before.blocks.iter().map(|b| b.id.as_str()).chain(before.islands.iter().map(|i| i.id.as_str())).collect();
    after.blocks.iter().map(|b| &b.id).chain(after.islands.iter().map(|i| &i.id)).filter(|id| !had.contains(id.as_str())).cloned().collect()
}

/// The id of each container of `after` that `before` holds no record of.
pub(super) fn new_containers(before: &quillmark_automerge::content::model::Content, after: &quillmark_automerge::content::model::Content) -> Vec<String> {
    after.containers.iter().filter(|c| !before.containers.iter().any(|k| k.id == c.id)).map(|c| c.id.clone()).collect()
}

/// A document holding `card` as its one card, which `card_mut(0)` writes.
pub(super) fn placed(card: super::Card) -> super::Document {
    let mut doc = parse("~~~\n$quill: q\n~~~\n");
    doc.unchecked().insert_card(None, card).expect("the card places");
    doc
}

/// `Document::parse` with the warnings dropped, for a test reading the
/// document or the refusal.
pub(super) fn decompose(markdown: &str) -> Result<super::Document, crate::error::ParseError> {
    super::Document::parse(markdown).map(|p| p.document)
}

/// Parse, emit, re-parse: the same document, or a panic naming `label` and the
/// emission that broke it.
pub(super) fn assert_round_trip(label: &str, src: &str) {
    let a = parse(src);
    let emitted = a.to_markdown();
    let b = super::Document::parse(&emitted)
        .unwrap_or_else(|e| panic!("{label}: the emission does not parse: {e}\n{emitted}"))
        .document;
    assert_eq!(a, b, "{label}: emit∘parse is not the identity\n{emitted}");
}

/// `crates/fixtures/resources`, the root of every document sweep.
pub(super) fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("crates/fixtures/resources")
}

/// Every `.md` reachable from `root`, recursively. Includes bundled quill
/// `README.md`s, which carry no root card-yaml block and are skipped at parse.
pub(super) fn collect_md_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}
