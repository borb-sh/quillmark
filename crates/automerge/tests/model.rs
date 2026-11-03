//! The document model: a value reads back as itself, a change lowered
//! straight into the document reads as the value `apply` gives, bytes and
//! change bytes carry the state, and peers writing at once converge. Ids
//! differ by construction, so values compare with each id renamed by its
//! place. `CASES` raises the case counts.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::model::Doc;
use quillmark_content::change::{Change, apply};
use quillmark_content::model::Content;
use serde_json::Value;
use std::collections::HashMap;

/// Each id `old` holds in its own scope, by the place it holds it, and the
/// id `new` holds there.
fn ids(old: &Content, new: &Content) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (a, b) in old.blocks.iter().zip(&new.blocks) {
        out.insert(a.id.clone(), b.id.clone());
    }
    for (a, b) in old.containers.iter().zip(&new.containers) {
        out.insert(a.id.clone(), b.id.clone());
    }
    for (a, b) in old.islands.iter().zip(&new.islands) {
        out.insert(a.id.clone(), b.id.clone());
    }
    out
}

/// The change JSON `v`, made against `old`, renamed to `new`'s ids, each
/// nested change to its slot's.
fn rename_change(v: &Value, old: &Content, new: &Content) -> Value {
    let map = ids(old, new);
    let steps = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| match step.get("nested") {
            Some(n) => {
                let (island, slot) = (n["island"].as_str().unwrap(), n["slot"].as_str().unwrap());
                let Some(i) = old.islands.iter().position(|i| i.id == island) else {
                    return step.clone();
                };
                let mut out = n.clone();
                out["island"] = Value::String(new.islands[i].id.clone());
                if let (Some(so), Some(sn)) = (old.islands[i].slots.get(slot), new.islands[i].slots.get(slot)) {
                    out["change"] = rename_change(&n["change"], so, sn);
                }
                serde_json::json!({ "nested": out })
            }
            None => rename(step, &map),
        });
    serde_json::json!({ "steps": steps.collect::<Vec<_>>() })
}

const PROSE: [&str; 6] = ["text", "insert", "delete", "quote", "url", "name"];

fn rename(v: &Value, map: &HashMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(map.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(a) => Value::Array(a.iter().map(|x| rename(x, map)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(
                    |(k, x)| match PROSE.contains(&k.as_str()) && x.is_string() {
                        true => (k.clone(), x.clone()),
                        false => (k.clone(), rename(x, map)),
                    },
                )
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `c` with each id renamed by its place, each slot on its own.
fn canon(c: &Content) -> Value {
    let mut map = HashMap::new();
    for (n, id) in c
        .blocks
        .iter()
        .map(|b| &b.id)
        .chain(c.containers.iter().map(|x| &x.id))
        .chain(c.islands.iter().map(|i| &i.id))
        .enumerate()
    {
        map.entry(id.clone()).or_insert_with(|| format!("p{n}"));
    }
    let mut v = rename(&c.to_value(), &map);
    if let Some(islands) = v.get_mut("islands").and_then(Value::as_array_mut) {
        for (i, island) in islands.iter_mut().enumerate() {
            for (k, s) in &c.islands[i].slots {
                island["slots"][k] = canon(s);
            }
        }
    }
    v
}

/// `canon(c)` without formatting and anchors, whose edges Automerge places
/// as its expand flags say rather than as `apply` does.
fn structure(c: &Content) -> Value {
    fn strip(v: &mut Value) {
        if let Some(o) = v.as_object_mut() {
            o.remove("marks");
            o.remove("anchors");
            if let Some(islands) = o.get_mut("islands").and_then(Value::as_array_mut) {
                for i in islands {
                    if let Some(slots) = i.get_mut("slots").and_then(Value::as_object_mut) {
                        slots.values_mut().for_each(strip);
                    }
                }
            }
        }
    }
    let mut v = canon(c);
    strip(&mut v);
    v
}

fn held(d: &Content) -> (Doc, automerge::ObjId) {
    let mut doc = Doc::new();
    let main = doc.main();
    let root = doc.put_value(&main, "v", d);
    doc.am_mut().commit();
    (doc, root)
}

/// `change`, made against `d`, renamed to the ids the document gives `d`.
fn renamed(doc: &Doc, root: &automerge::ObjId, d: &Content, change: &Change) -> Change {
    Change::from_value(&rename_change(
        &change.to_value(),
        d,
        &doc.view(root).content,
    ))
    .unwrap()
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
    a.am_mut().commit();
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
