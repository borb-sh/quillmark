//! A write's or a merge's patches, as automerge-js spells them, take the
//! value's text it read to the text it left: replayed on that text in
//! UTF-16 code units, each block marker and island one unit, they give the
//! text after. `CASES` raises the case count.

mod common;

use common::*;
use proptest::prelude::*;
use automerge::{ObjId, ReadDoc};
use quillmark_automerge::content::model::Content;
use quillmark_automerge::content::units::Utf16;
use quillmark_automerge::model::{Doc, ids};

fn doc_of(d: &Content) -> (Doc, ObjId) {
    let mut doc = Doc::new();
    let main = doc.main();
    let root = doc.put_value(&main, "v", d);
    doc.commit();
    (doc, root)
}

/// The value's own text as automerge-js holds it: one UTF-16 unit per code
/// unit, and one per block marker and island.
fn text_of(doc: &Doc, root: &ObjId) -> Vec<u16> {
    let (_, text) = doc.am().get(root, "text").unwrap().unwrap();
    doc.am().text(&text).unwrap().encode_utf16().collect()
}

/// `text` with each patch to the value's own text applied.
fn replay(text: &mut Vec<u16>, patches: &[serde_json::Value]) {
    for p in patches {
        let path = p["path"].as_array().unwrap();
        let [.., key, index] = path.as_slice() else { continue };
        if key != "text" || path.len() != 4 || path[1] != "v" {
            continue;
        }
        let at = index.as_u64().unwrap() as usize;
        match p["action"].as_str().unwrap() {
            "splice" => {
                let units: Vec<u16> = p["value"].as_str().unwrap().encode_utf16().collect();
                text.splice(at..at, units);
            }
            "insert" => {
                let n = p["values"].as_array().unwrap().len();
                text.splice(at..at, std::iter::repeat_n(0xfffc, n));
            }
            "del" => {
                let len = p["length"].as_u64().unwrap_or(1) as usize;
                text.drain(at..at + len);
            }
            _ => {}
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases("CASES", 96), ..ProptestConfig::default() })]

    #[test]
    fn a_writes_patches_take_the_text_before_to_the_text_after(d in document(), ops in ops()) {
        let (mut doc, root) = doc_of(&d);
        let before = doc.view(&root).content;
        let mut text = text_of(&doc, &root);
        let base = doc.heads();
        let change = ids::renamed(&change_for(&d, &ops), &d, &before);
        doc.apply(&root, &change, None).unwrap();
        let patches = doc.patches(&base, Utf16);
        replay(&mut text, &patches);
        prop_assert_eq!(text, text_of(&doc, &root), "{:?}", patches);
    }

    #[test]
    fn a_merge_s_patches_take_the_text_before_to_the_text_after(d in document(), mine in ops(), theirs in ops()) {
        let (mut doc, root) = doc_of(&d);
        let before = doc.view(&root).content;
        let base = doc.heads();
        let mut peer = Doc::load(&doc.save()).unwrap();
        doc.apply(&root, &ids::renamed(&change_for(&d, &mine), &d, &before), None).unwrap();
        peer.apply(&root, &ids::renamed(&change_from(&d, &theirs, 100), &d, &before), None).unwrap();
        let mut text = text_of(&doc, &root);
        let at = doc.heads();
        doc.merge(&peer.save_since(&base)).unwrap();
        let patches = doc.patches(&at, Utf16);
        replay(&mut text, &patches);
        prop_assert_eq!(text, text_of(&doc, &root), "{:?}", patches);
    }
}
