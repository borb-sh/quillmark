//! The document model: a value reads back as itself, a change lowered
//! straight into the document reads as the value `apply` gives, bytes and
//! change bytes carry the state, and peers writing at once converge. Ids
//! differ by construction, so values compare with each id renamed by its
//! place (`common::canon`). `CASES` raises the case counts.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::model::{Doc, ids};
use quillmark_automerge::content::change::{Change, apply};
use quillmark_automerge::content::model::Content;

fn held(d: &Content) -> (Doc, automerge::ObjId) {
    let mut doc = Doc::new();
    let main = doc.main();
    let root = doc.put_value(&main, "v", d);
    doc.commit();
    (doc, root)
}

/// `change`, made against `d`, renamed to the ids the document gives `d`.
fn renamed(doc: &Doc, root: &automerge::ObjId, d: &Content, change: &Change) -> Change {
    ids::renamed(change, d, &doc.view(root).content)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases("CASES", 128)))]

    #[test]
    fn a_value_reads_back_as_itself(d in document()) {
        let (doc, root) = held(&d);
        let v = doc.view(&root);
        prop_assert!(v.repairs.is_empty(), "repairs {:?}", v.repairs);
        prop_assert_eq!(canon(&v.content), canon(&d));
    }

    #[test]
    fn a_lowered_change_reads_as_apply_gives(d in document(), ops in ops()) {
        let change = change_for(&d, &ops);
        let want = apply(&d, &change).unwrap().content;
        let (mut doc, root) = held(&d);
        let change = renamed(&doc, &root, &d, &change);
        if let Err(e) = doc.apply(&root, &change, None) {
            prop_assert!(false, "{e}\nchange {}\nview {}", change.to_json(), doc.view(&root).content.to_json());
        }
        let got = doc.view(&root).content;
        prop_assert_eq!(structure(&got), structure(&want));
        let ids = |c: &Content| { let mut v = c.anchor_ids().into_iter().collect::<Vec<_>>(); v.sort(); v };
        prop_assert_eq!(ids(&got), ids(&want));
    }

    #[test]
    fn bytes_hold_the_state(d in document(), ops in ops()) {
        let change = change_for(&d, &ops);
        let (mut doc, root) = held(&d);
        let change = renamed(&doc, &root, &d, &change);
        doc.apply(&root, &change, None).unwrap();
        let mut back = Doc::load(&doc.save()).unwrap();
        prop_assert_eq!(back.heads(), doc.heads());
        prop_assert_eq!(back.view(&root), doc.view(&root));
    }

    #[test]
    fn two_peers_writing_at_once_converge(d in document(), mine in ops(), theirs in ops()) {
        let (mut a, root) = held(&d);
        let mut b = Doc::load(&a.save()).unwrap();
        let base = a.heads();
        let m = renamed(&a, &root, &d, &change_for(&d, &mine));
        let t = renamed(&b, &root, &d, &change_from(&d, &theirs, 100));
        a.apply(&root, &m, None).unwrap();
        b.apply(&root, &t, None).unwrap();
        let (from_a, from_b) = (a.save_since(&base), b.save_since(&base));
        a.merge(&from_b).unwrap();
        b.merge(&from_a).unwrap();
        prop_assert_eq!(a.heads(), b.heads());
        let (va, vb) = (a.view(&root), b.view(&root));
        prop_assert_eq!(&va, &vb);
        prop_assert!(va.content.validate().is_ok());
    }

    #[test]
    fn a_write_at_earlier_heads_merges_with_one_since(d in document(), mine in ops(), theirs in ops()) {
        let (mut doc, root) = held(&d);
        let base = doc.heads();
        let t = renamed(&doc, &root, &d, &change_from(&d, &theirs, 100));
        let m = renamed(&doc, &root, &d, &change_for(&d, &mine));
        doc.apply(&root, &t, None).unwrap();
        doc.apply(&root, &m, Some(&base)).unwrap();
        let (mut peer, _) = (Doc::load(&doc.save()).unwrap(), ());
        prop_assert_eq!(peer.view(&root), doc.view(&root));
        prop_assert_eq!(peer.heads(), doc.heads());
    }
}

#[test]
fn two_documents_built_alike_share_their_origin() {
    let (mut a, mut b) = (Doc::new(), Doc::new());
    let ha = a.heads();
    assert_eq!(ha, b.heads());
    let main = a.main();
    a.put_value(&main, "v", &Content::empty());
    a.commit();
    let bytes = a.save_since(&ha);
    b.merge(&bytes).unwrap();
    assert_eq!(a.heads(), b.heads());
}

#[test]
fn bytes_that_are_not_a_document_refuse() {
    assert_eq!(Doc::load(b"nope").unwrap_err().name(), "decode");
    let mut other = automerge::AutoCommit::new();
    assert_eq!(Doc::load(&other.save()).unwrap_err().name(), "shape");
}

#[test]
fn a_step_naming_an_id_no_object_spells_refuses() {
    let (mut doc, root) = held(&quillmark_automerge::content::markdown::from_markdown("one").unwrap().content);
    for id in ["b0", "1@zz", "1@abc", "x@ab"] {
        let step = quillmark_automerge::content::change::Step::SetBlock(quillmark_automerge::content::change::SetBlock { id: id.into(), kind: Some("para".into()), ..Default::default() });
        let err = doc.apply(&root, &Change::new(vec![step]), None).unwrap_err();
        assert!(matches!(err, quillmark_automerge::model::StepError::Unknown { .. }), "{id}: {err}");
    }
}
