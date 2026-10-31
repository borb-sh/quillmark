//! A bundle is bytes Quillmark may not have written. Over any edit of a
//! well-formed archive, a read refuses rather than panics; any tree whose
//! paths the format admits writes, reads back file for file, and rewrites to
//! the same bytes.

use proptest::prelude::*;
use std::collections::BTreeMap;

use super::super::{read_bundle, write_bundle};
use super::{Raw, archive, raw};
use crate::quill::FileTreeNode;

fn arb_segment() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9_.-]{1,12}".prop_filter("not a dot segment", |s| s != "." && s != ".."),
        Just("caf\u{e9}".to_string()),
        Just("Quill.yaml".to_string()),
    ]
}

/// File paths no one of which sits under another.
fn arb_files() -> impl Strategy<Value = BTreeMap<String, Vec<u8>>> {
    let path = proptest::collection::vec(arb_segment(), 1..4).prop_map(|s| s.join("/"));
    let contents = prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..300),
        (any::<u8>(), 0usize..2000).prop_map(|(b, n)| vec![b; n]),
    ];
    proptest::collection::btree_map(path, contents, 0..12).prop_map(|files| {
        let paths: Vec<String> = files.keys().cloned().collect();
        files
            .into_iter()
            .filter(|(p, _)| !paths.iter().any(|q| q.starts_with(&format!("{p}/"))))
            .collect()
    })
}

fn tree_of(files: &BTreeMap<String, Vec<u8>>) -> FileTreeNode {
    let mut root = FileTreeNode::Directory {
        files: Default::default(),
    };
    for (path, contents) in files {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.clone(),
            },
        )
        .expect("a generated path inserts");
    }
    root
}

/// One edit of an archive: a byte overwritten, the tail cut, or a span
/// repeated, each at a generated offset.
#[derive(Debug, Clone)]
enum Edit {
    Set(usize, u8),
    Cut(usize),
    Repeat(usize, usize),
}

fn arb_edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (any::<usize>(), any::<u8>()).prop_map(|(at, b)| Edit::Set(at, b)),
        any::<usize>().prop_map(Edit::Cut),
        (any::<usize>(), 1usize..64).prop_map(|(at, n)| Edit::Repeat(at, n)),
    ]
}

fn apply(bytes: &mut Vec<u8>, edit: &Edit) {
    if bytes.is_empty() {
        return;
    }
    match *edit {
        Edit::Set(at, b) => {
            let at = at % bytes.len();
            bytes[at] = b;
        }
        Edit::Cut(at) => bytes.truncate(at % bytes.len()),
        Edit::Repeat(at, n) => {
            let at = at % bytes.len();
            let span: Vec<u8> = bytes[at..(at + n).min(bytes.len())].to_vec();
            bytes.splice(at..at, span);
        }
    }
}

/// Archives this crate's writer would not emit, so an edit lands on a
/// directory entry, an extra field, a comment or a deflated entry too.
fn arb_archive() -> impl Strategy<Value = Vec<u8>> {
    let foreign = arb_files().prop_map(|files| {
        let mut entries = vec![raw("dir/", b"")];
        for (path, data) in files {
            let deflated = miniz_oxide::deflate::compress_to_vec(&data, 6);
            entries.push(Raw {
                size: Some(data.len() as u32),
                crc: Some(crc32fast::hash(&data)),
                method: super::DEFLATED,
                extra: vec![1, 0, 1, 0, 9],
                ..raw(&path, &deflated)
            });
        }
        archive(&entries, b"comment")
    });
    prop_oneof![
        arb_files().prop_map(|files| write_bundle(&tree_of(&files)).expect("writes")),
        foreign,
    ]
}

proptest! {
    #[test]
    fn a_tree_round_trips_and_rewrites_to_its_own_bytes(files in arb_files()) {
        let bytes = write_bundle(&tree_of(&files)).expect("an admitted tree writes");
        let read = read_bundle(&bytes).expect("a written bundle reads");
        let flat: BTreeMap<String, Vec<u8>> = read.flatten().into_iter().collect();
        prop_assert_eq!(&flat, &files);
        prop_assert_eq!(write_bundle(&read).expect("rewrites"), bytes);
    }

    #[test]
    fn an_edited_archive_reads_or_is_refused(
        mut bytes in arb_archive(),
        edits in proptest::collection::vec(arb_edit(), 1..4),
    ) {
        for edit in &edits {
            apply(&mut bytes, edit);
        }
        if let Err(d) = read_bundle(&bytes) {
            let codes = [
                "quill::bundle_invalid",
                "quill::bundle_unsupported",
                "quill::bundle_path",
                "quill::bundle_too_large",
            ];
            prop_assert!(codes.contains(&d.code.as_deref().unwrap_or_default()), "{d:?}");
        }
    }
}
