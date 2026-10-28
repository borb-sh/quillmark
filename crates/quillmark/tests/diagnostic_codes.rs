//! Holds `docs/reference/diagnostic-codes.md` and the minted codes to each
//! other: a rename edits the page in the same commit, where a reviewer sees it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn is_code(s: &str) -> bool {
    let Some((ns, name)) = s.split_once("::") else {
        return false;
    };
    let lead = |t: &str| t.starts_with(|c: char| c.is_ascii_lowercase());
    lead(ns)
        && lead(name)
        && ns.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Every `"ns::name"` or `'ns::name'` literal in `text`.
fn quoted_codes(text: &str, into: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    for (i, &q) in bytes.iter().enumerate() {
        if q != b'"' && q != b'\'' {
            continue;
        }
        let rest = &text[i + 1..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .unwrap_or(rest.len());
        if rest.as_bytes().get(end) == Some(&q) && is_code(&rest[..end]) {
            into.insert(rest[..end].to_string());
        }
    }
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source dir reads") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !matches!(name, "target" | "node_modules" | "pkg" | ".venv") {
                walk(&path, files);
            }
        } else if name.ends_with(".rs") && !path.ends_with(file!()) {
            files.push(path);
        }
    }
}

fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    path.components().any(|c| c.as_os_str() == "tests")
        || name == "tests.rs"
        || name.ends_with("_test.rs")
        || name.ends_with("_tests.rs")
}

/// Code literals across every source file, and across non-test files alone.
fn minted() -> (BTreeSet<String>, BTreeSet<String>) {
    let mut files = Vec::new();
    walk(&Path::new(ROOT).join("crates"), &mut files);
    let runtime = Path::new(ROOT).join("crates/bindings/wasm/runtime");
    for entry in std::fs::read_dir(runtime).expect("runtime dir reads") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|e| e == "js") {
            files.push(path);
        }
    }
    let (mut all, mut shipped) = (BTreeSet::new(), BTreeSet::new());
    for file in files {
        let text = std::fs::read_to_string(&file).expect("source reads");
        quoted_codes(&text, &mut all);
        if !is_test_file(&file) {
            quoted_codes(&text, &mut shipped);
        }
    }
    (all, shipped)
}

fn listed() -> BTreeSet<String> {
    let page = std::fs::read_to_string(Path::new(ROOT).join("docs/reference/diagnostic-codes.md"))
        .expect("diagnostic-codes.md reads");
    page.lines()
        .filter_map(|line| line.strip_prefix("- `")?.split_once('`'))
        .map(|(code, _)| code)
        .filter(|code| is_code(code))
        .map(str::to_string)
        .collect()
}

/// A code `format!` mints from another listed code: `conform::<class>` from
/// `edit::<class>`, `quill::seed_<class>` from `validation::<class>` or
/// `validation::seed_<class>`, and `quill::example_<class>` from
/// `quill::default_<class>`.
fn formatted_from_listed(code: &str, listed: &BTreeSet<String>) -> bool {
    if let Some(class) = code.strip_prefix("quill::example_") {
        return listed.contains(&format!("quill::default_{class}"));
    }
    if let Some(class) = code.strip_prefix("conform::") {
        return listed.contains(&format!("edit::{class}"));
    }
    if let Some(class) = code.strip_prefix("quill::seed_") {
        return listed.contains(&format!("validation::{class}"))
            || listed.contains(&format!("validation::seed_{class}"));
    }
    false
}

#[test]
fn every_minted_code_is_listed_and_every_listed_code_is_minted() {
    let (all, shipped) = minted();
    let listed = listed();

    let unlisted: Vec<_> = all.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "minted but missing from docs/reference/diagnostic-codes.md: {unlisted:?}"
    );

    let stale: Vec<_> = listed
        .iter()
        .filter(|c| !shipped.contains(*c) && !formatted_from_listed(c, &listed))
        .collect();
    assert!(
        stale.is_empty(),
        "listed in docs/reference/diagnostic-codes.md but minted by no non-test source: {stale:?}; \
         a removed or renamed code leaves the page in the release whose migration guide names it"
    );
}
