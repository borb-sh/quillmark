mod properties;

use super::*;
use std::collections::HashMap;

const MANIFEST: &[u8] =
    b"quill:\n  name: memo\n  version: \"1.0.0\"\n  backend: typst\n  description: memo\n";

/// One entry as an arbitrary zip writer might lay it out. `size` and `crc`
/// default to what `data` holds.
#[derive(Clone)]
pub(super) struct Raw {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub method: u16,
    pub flags: u16,
    pub made_by: u16,
    pub external: u32,
    pub size: Option<u32>,
    pub crc: Option<u32>,
    pub extra: Vec<u8>,
}

pub(super) fn raw(name: &str, data: &[u8]) -> Raw {
    Raw {
        name: name.as_bytes().to_vec(),
        data: data.to_vec(),
        method: STORED,
        flags: 0,
        made_by: VERSION,
        external: 0,
        size: None,
        crc: None,
        extra: Vec::new(),
    }
}

/// A zip archive of `entries` with `comment` after its end record, written
/// field by field so a test can spell what this crate's writer never would.
pub(super) fn archive(entries: &[Raw], comment: &[u8]) -> Vec<u8> {
    let (mut out, mut directory) = (Vec::new(), Vec::new());
    for e in entries {
        let size = e.size.unwrap_or(e.data.len() as u32);
        let crc = e.crc.unwrap_or_else(|| crc32fast::hash(&e.data));
        let offset = out.len() as u32;
        let shared = |into: &mut Vec<u8>| {
            for half in [VERSION, e.flags, e.method, 0, DOS_DATE_1980_01_01] {
                into.extend_from_slice(&half.to_le_bytes());
            }
            for word in [crc, e.data.len() as u32, size] {
                into.extend_from_slice(&word.to_le_bytes());
            }
            into.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            into.extend_from_slice(&(e.extra.len() as u16).to_le_bytes());
        };
        out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        shared(&mut out);
        out.extend_from_slice(&e.name);
        out.extend_from_slice(&e.extra);
        out.extend_from_slice(&e.data);

        directory.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
        directory.extend_from_slice(&e.made_by.to_le_bytes());
        shared(&mut directory);
        directory.extend_from_slice(&[0; 6]);
        directory.extend_from_slice(&e.external.to_le_bytes());
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(&e.name);
        directory.extend_from_slice(&e.extra);
    }
    let directory_at = out.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&end_record(entries.len() as u16, directory.len() as u32, directory_at, comment));
    out
}

fn end_record(count: u16, directory_len: u32, directory_at: u32, comment: &[u8]) -> Vec<u8> {
    let mut out = END_SIG.to_le_bytes().to_vec();
    for half in [0, 0, count, count] {
        out.extend_from_slice(&u16::to_le_bytes(half));
    }
    out.extend_from_slice(&directory_len.to_le_bytes());
    out.extend_from_slice(&directory_at.to_le_bytes());
    out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    out.extend_from_slice(comment);
    out
}

fn tree(entries: &[(&str, &[u8])]) -> FileTreeNode {
    let mut root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    for &(path, contents) in entries {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.to_vec(),
            },
        )
        .expect("insert");
    }
    root
}

fn code_of<T: std::fmt::Debug>(result: Result<T, Vec<Diagnostic>>) -> String {
    let diags = result.expect_err("the bundle is refused");
    assert_eq!(diags.len(), 1, "one refusal: {diags:?}");
    diags[0].code.clone().expect("a coded refusal")
}

#[test]
fn a_quill_round_trips_and_its_bundle_is_a_fixed_point() {
    let noise: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    let quill = Quill::from_tree(tree(&[
        ("Quill.yaml", MANIFEST),
        ("plate.typ", "= Title\n".repeat(200).as_bytes()),
        ("assets/fonts/Body.ttf", &noise),
        ("packages/preview/pkg/0.1.0/lib.typ", b""),
        ("assets/caf\u{e9}.txt", b"named outside ASCII"),
    ]))
    .expect("the tree loads");

    let bundle = quill.to_bundle().expect("the quill writes");
    let read = Quill::from_bundle(&bundle).expect("the bundle reads");
    assert_eq!(read.to_tree(), quill.to_tree());
    assert_eq!(read.to_bundle().expect("rewrites"), bundle);
}

/// What zip tools write and this crate does not: directory entries, a data
/// descriptor flag, extra fields, Unix modes, an archive comment.
#[test]
fn reads_what_other_zip_writers_emit() {
    let plate = miniz_oxide::deflate::compress_to_vec(b"= Title\n= Title\n= Title\n", 9);
    let unix = |mut e: Raw, mode: u32| {
        e.made_by = (MADE_BY_UNIX << 8) | 30;
        e.external = mode << 16;
        e
    };
    let entries = [
        unix(raw("assets/", b""), 0o040755),
        unix(raw("Quill.yaml", MANIFEST), 0o100644),
        Raw {
            method: DEFLATED,
            flags: 1 << 3,
            size: Some(24),
            crc: Some(crc32fast::hash(b"= Title\n= Title\n= Title\n")),
            extra: vec![0x55, 0x54, 1, 0, 0],
            ..raw("plate.typ", &plate)
        },
        raw("assets/logo.svg", b"<svg/>"),
    ];
    let quill = Quill::from_bundle(&archive(&entries, b"made elsewhere")).expect("reads");
    let paths: Vec<_> = quill.to_tree().into_iter().map(|(p, _)| p).collect();
    assert_eq!(paths, ["Quill.yaml", "assets/logo.svg", "plate.typ"]);
    assert_eq!(quill.files().get_file("plate.typ"), Some(&b"= Title\n= Title\n= Title\n"[..]));
}

#[test]
fn refuses_an_archive_outside_the_format_by_code() {
    let with = |e: Raw| archive(&[raw("Quill.yaml", MANIFEST), e], b"");
    let oversized = Raw {
        size: Some(MAX_BUNDLE_FILE_BYTES as u32 + 1),
        method: DEFLATED,
        ..raw("big.bin", b"")
    };
    let mut too_many = archive(&[], b"");
    too_many[8..12].copy_from_slice(&[0x01, 0x08, 0x01, 0x08]);

    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("not a zip", b"%PDF-1.7 not an archive at all".to_vec(), "quill::bundle_invalid"),
        ("bad CRC", with(Raw { crc: Some(0), ..raw("a.typ", b"x") }), "quill::bundle_invalid"),
        ("truncated", with(raw("a.typ", b"x"))[..40].to_vec(), "quill::bundle_invalid"),
        ("traversal", with(raw("../a.typ", b"x")), "quill::bundle_path"),
        ("absolute", with(raw("/a.typ", b"x")), "quill::bundle_path"),
        ("backslash", with(raw("assets\\a.typ", b"x")), "quill::bundle_path"),
        ("drive", with(raw("C:/a.typ", b"x")), "quill::bundle_path"),
        ("empty segment", with(raw("assets//a.typ", b"x")), "quill::bundle_path"),
        ("not UTF-8", with(Raw { name: vec![0xff, b'a'], ..raw("", b"x") }), "quill::bundle_path"),
        ("twice", with(raw("Quill.yaml", MANIFEST)), "quill::bundle_path"),
        (
            "file and directory",
            archive(&[raw("Quill.yaml", MANIFEST), raw("a", b"x"), raw("a/b", b"y")], b""),
            "quill::bundle_path",
        ),
        (
            "file and directory entry",
            archive(&[raw("a/b/", b""), raw("Quill.yaml", MANIFEST), raw("a", b"x")], b""),
            "quill::bundle_path",
        ),
        ("bzip2", with(Raw { method: 12, ..raw("a.typ", b"x") }), "quill::bundle_unsupported"),
        ("encrypted", with(Raw { flags: 1, ..raw("a.typ", b"x") }), "quill::bundle_unsupported"),
        (
            "symlink",
            with(Raw { made_by: MADE_BY_UNIX << 8, external: 0o120777 << 16, ..raw("a.typ", b"/etc/passwd") }),
            "quill::bundle_unsupported",
        ),
        ("one file over budget", with(oversized), "quill::bundle_too_large"),
        ("too many entries", too_many, "quill::bundle_too_large"),
    ];
    for (what, bytes, code) in cases {
        assert_eq!(code_of(Quill::from_bundle(&bytes)), code, "{what}");
    }
}

#[test]
fn a_tree_zipped_inside_its_folder_names_the_folder() {
    let bytes = archive(&[raw("memo/Quill.yaml", MANIFEST), raw("memo/plate.typ", b"")], b"");
    let diags = Quill::from_bundle(&bytes).expect_err("no root Quill.yaml");
    assert_eq!(diags[0].code.as_deref(), Some("quill::missing_file"));
    assert!(diags[0].hint.as_deref().is_some_and(|h| h.contains("\"memo\"")), "{diags:?}");
}

#[test]
fn the_writer_refuses_what_a_reader_would() {
    let with = |path: &str, contents: Vec<u8>| {
        let mut quill = Quill::from_tree(tree(&[("Quill.yaml", MANIFEST)])).expect("loads");
        quill.files.insert(path, FileTreeNode::File { contents }).expect("insert");
        quill.to_bundle()
    };
    assert_eq!(code_of(with("a:b.typ", Vec::new())), "quill::bundle_path");
    let over = vec![0; MAX_BUNDLE_FILE_BYTES as usize + 1];
    assert_eq!(code_of(with("big.bin", over)), "quill::bundle_too_large");
}
