//! A font collection a quill ships loads every face it holds.

use quillmark_core::backend::Backend;
use quillmark_core::quill::{FileTreeNode, Quill};
use quillmark_typst::TypstBackend;

mod common;

const YAML: &str = "quill:\n  name: t\n  version: \"1.0.0\"\n  backend: typst\n  \
                    description: d\n\ntypst:\n  plate_file: plate.typ\n";

/// A collection of `fonts`: a `ttcf` header, then each font, 4-byte aligned,
/// with its table offsets moved to where it lands.
fn collection_of(fonts: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"ttcf".to_vec();
    out.extend(0x0001_0000u32.to_be_bytes());
    out.extend(u32::try_from(fonts.len()).unwrap().to_be_bytes());
    let mut start = 12 + 4 * fonts.len();
    let mut body = Vec::new();
    for font in fonts {
        out.extend(u32::try_from(start).unwrap().to_be_bytes());
        let mut font = font.clone();
        let tables = usize::from(u16::from_be_bytes([font[4], font[5]]));
        for i in 0..tables {
            let at = 12 + 16 * i + 8;
            let offset = u32::from_be_bytes(font[at..at + 4].try_into().unwrap());
            let moved = offset + u32::try_from(start).unwrap();
            font[at..at + 4].copy_from_slice(&moved.to_be_bytes());
        }
        font.resize(font.len().next_multiple_of(4), 0);
        start += font.len();
        body.extend(font);
    }
    out.extend(body);
    out
}

#[test]
fn a_ttc_under_assets_fonts_sets_every_family_it_holds() {
    let figtree = std::fs::read(
        quillmark_fixtures::quills_path("taro").join("assets/fonts/Figtree-Regular.ttf"),
    )
    .expect("read Figtree");
    let roboto = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/src/fonts/Roboto.ttf"))
        .expect("read Roboto");
    let ttc = collection_of(&[figtree, roboto]);

    let mut root = FileTreeNode::Directory {
        files: Default::default(),
    };
    for (path, contents) in [
        ("Quill.yaml", YAML.as_bytes()),
        (
            "plate.typ",
            b"#text(font: \"Figtree\")[one] #text(font: \"Roboto\")[two]\n",
        ),
        ("assets/fonts/Both.ttc", &ttc),
    ] {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.to_vec(),
            },
        )
        .expect("insert");
    }
    let q = Quill::from_tree(root).expect("load quill");
    let session = TypstBackend::default()
        .open(&q, &serde_json::json!({}), common::test_date())
        .expect("the plate compiles");
    let unknown: Vec<_> = session
        .warnings()
        .iter()
        .filter(|d| d.message.starts_with("unknown font family"))
        .collect();
    assert!(unknown.is_empty(), "both families resolve: {unknown:?}");
}

#[test]
fn a_collection_header_declaring_u32_max_faces_opens_promptly() {
    let mut ttc = b"ttcf".to_vec();
    ttc.extend(0x0001_0000u32.to_be_bytes());
    ttc.extend(u32::MAX.to_be_bytes());
    ttc.extend(12u32.to_be_bytes());

    let mut root = FileTreeNode::Directory {
        files: Default::default(),
    };
    for (path, contents) in [
        ("Quill.yaml", YAML.as_bytes()),
        ("plate.typ", b"one\n".as_slice()),
        ("assets/fonts/Huge.ttc", &ttc),
    ] {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.to_vec(),
            },
        )
        .expect("insert");
    }
    let q = Quill::from_tree(root).expect("load quill");
    TypstBackend::default()
        .open(&q, &serde_json::json!({}), common::test_date())
        .expect("the plate compiles");
}

#[test]
fn a_collection_declaring_past_the_face_cap_loads_none_and_warns() {
    let declared = 1_000u32;
    let mut ttc = b"ttcf".to_vec();
    ttc.extend(0x0001_0000u32.to_be_bytes());
    ttc.extend(declared.to_be_bytes());
    ttc.extend(std::iter::repeat_n(0u8, declared as usize * 4));

    let mut root = FileTreeNode::Directory {
        files: Default::default(),
    };
    for (path, contents) in [
        ("Quill.yaml", YAML.as_bytes()),
        ("plate.typ", b"one\n".as_slice()),
        ("assets/fonts/Many.ttc", &ttc),
    ] {
        root.insert(
            path,
            FileTreeNode::File {
                contents: contents.to_vec(),
            },
        )
        .expect("insert");
    }
    let q = Quill::from_tree(root).expect("load quill");
    let session = TypstBackend::default()
        .open(&q, &serde_json::json!({}), common::test_date())
        .expect("the plate compiles");
    let skipped: Vec<_> = session
        .warnings()
        .iter()
        .filter(|d| d.code.as_deref() == Some("typst::font_collection_skipped"))
        .collect();
    assert_eq!(skipped.len(), 1, "{:?}", session.warnings());
}
