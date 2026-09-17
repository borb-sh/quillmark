mod assemble_tests;
mod card_fence_tests;
mod edit_tests;
mod emit_tests;
mod ext_tests;
mod fence_conformance_tests;
mod lossiness_tests;
mod multibyte_tests;
mod properties;
mod seed_tests;

use std::path::{Path, PathBuf};

/// Parse `src`, failing the test on a parse error and dropping the warnings.
pub(super) fn parse(src: &str) -> super::Document {
    super::Document::parse(src)
        .expect("source should parse")
        .document
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
