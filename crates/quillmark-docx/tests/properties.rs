//! The writer over generated text and the package reader over generated
//! bytes: every part stays well-formed XML whatever a run or an attribute
//! holds, and the reader refuses what it cannot read rather than panicking.

use proptest::prelude::*;
use quillmark_docx::model::*;
use quillmark_docx::{unzip, write_docx};

fn xml_text(s: &str) -> String {
    s.chars()
        .filter(|&c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..))
        .collect()
}

fn doc_with(text: &str, attr: &str) -> Document {
    let run = Run {
        props: RunProps {
            style: Some(attr.into()),
            color: Some(attr.into()),
            ..RunProps::default()
        },
        content: vec![RunContent::Text(text.into())],
    };
    let par = Paragraph {
        props: ParProps {
            style: Some(attr.into()),
            ..ParProps::default()
        },
        content: vec![
            Inline::Run(run.clone()),
            Inline::Hyperlink(Hyperlink {
                target: LinkTarget::Url(attr.into()),
                runs: vec![run.clone()],
            }),
            Inline::Control(Control {
                tag: attr.into(),
                title: attr.into(),
                kind: ControlKind::RichText,
                content: vec![Inline::Run(run)],
                placeholder: false,
            }),
        ],
    };
    Document {
        sections: vec![Section {
            body: Blocks::from_paragraphs(vec![par]),
            props: SectionProps::default(),
        }],
        info: Info {
            title: Some(text.into()),
            language: None,
        },
        fonts: vec![FontDecl {
            name: attr.into(),
            alt_name: Some(text.into()),
            family: FontFamily::Auto,
            pitch_fixed: false,
        }],
        ..Document::default()
    }
}

proptest! {
    #[test]
    fn any_text_leaves_every_part_well_formed_and_reads_back(text in "\\PC*|[\\x00-\\x1f<>&\"' ]*", attr in "\\PC*|[\\x00-\\x1f<>&\"']*") {
        let parts = unzip(&write_docx(&doc_with(&text, &attr))).expect("reads back");
        for (name, bytes) in &parts {
            if !name.ends_with(".xml") && !name.ends_with(".rels") {
                continue;
            }
            let s = std::str::from_utf8(bytes).expect("utf-8");
            let doc = roxmltree::Document::parse(s).map_err(|e| TestCaseError::fail(format!("{name}: {e}")))?;
            if name == "word/document.xml" {
                let t: String = doc.descendants().filter(|n| n.tag_name().name() == "t").filter_map(|n| n.text()).take(1).collect();
                prop_assert_eq!(t, xml_text(&text));
                let style = doc.descendants().find(|n| n.tag_name().name() == "pStyle").unwrap();
                prop_assert_eq!(style.attributes().next().unwrap().value(), xml_text(&attr));
            }
        }
    }

    #[test]
    fn the_reader_refuses_mutated_packages_without_panicking(flips in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8), cut in any::<usize>()) {
        let mut bytes = write_docx(&doc_with("x", "y"));
        for (at, b) in flips {
            let n = bytes.len();
            bytes[at % n] ^= b;
        }
        let _ = unzip(&bytes);
        let n = bytes.len();
        let _ = unzip(&bytes[..cut % n]);
    }

    #[test]
    fn the_reader_survives_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = unzip(&bytes);
    }
}
