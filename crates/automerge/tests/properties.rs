//! The byte boundary of a document: [`Doc::load`] and a received sync
//! message refuse a document they cannot read rather than panic. The input
//! is a value's own document with generated edits at every depth, keys put,
//! deleted or retyped, text elements of every kind inserted and deleted,
//! marks of anchor and formatting names, and its saved bytes truncated or
//! flipped. A document that loads reads every value it holds as a valid
//! value, and so does a reload of it. `CASES` raises the case count.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::automerge::marks::{ExpandMark, Mark};
use quillmark_automerge::automerge::sync::{State, SyncDoc};
use quillmark_automerge::automerge::transaction::Transactable;
use quillmark_automerge::automerge::{AutoCommit, ObjId, ObjType, ROOT, ReadDoc, ScalarValue, Value};
use quillmark_automerge::model::{Doc, LoadError, SyncMessage, SyncState};
use quillmark_content::model::Content;

const KEYS: [&str; 10] = ["value", "text", "containers", "codec", "k", "kind", "id", "slots", "props", "c"];
const NAMES: [&str; 6] = ["@a", "<a", ">a", "strong", "a", "link"];

/// One generated edit: which object it hits, what it does there, and two
/// numbers it reads as a key, an index or a length.
type Hit = (u8, u8, u16, u16);

fn objects(doc: &AutoCommit, o: ObjId, out: &mut Vec<(ObjId, ObjType)>) {
    let Ok(ty) = doc.object_type(&o) else { return };
    out.push((o.clone(), ty));
    let kids: Vec<ObjId> = match ty {
        ObjType::Map | ObjType::Table => doc
            .keys(&o)
            .filter_map(|k| match doc.get(&o, k.as_str()).ok()?? {
                (Value::Object(_), id) => Some(id),
                _ => None,
            })
            .collect(),
        _ => doc.values(&o).filter(|(v, _)| matches!(v, Value::Object(_))).map(|(_, id)| id).collect(),
    };
    for k in kids {
        objects(doc, k, out);
    }
}

fn scalar(z: u16) -> ScalarValue {
    match z % 5 {
        0 => ScalarValue::Str("x".into()),
        1 => ScalarValue::Str("i".into()),
        2 => ScalarValue::Int(z as i64),
        3 => ScalarValue::Str("{\"kind\":5}".into()),
        _ => ScalarValue::Null,
    }
}

fn kind(z: u16) -> ObjType {
    [ObjType::Map, ObjType::List, ObjType::Text][z as usize % 3]
}

fn hit(doc: &mut AutoCommit, (target, op, x, y): Hit) {
    let mut all = Vec::new();
    objects(doc, ROOT, &mut all);
    let (o, ty) = all[target as usize % all.len()].clone();
    let key = KEYS[x as usize % KEYS.len()];
    let _ = match ty {
        ObjType::Map | ObjType::Table => match op % 3 {
            0 => doc.delete(&o, key),
            1 => doc.put(&o, key, scalar(y)),
            _ => doc.put_object(&o, key, kind(y)).map(|_| ()),
        },
        ObjType::Text | ObjType::List => {
            let len = doc.length(&o);
            let at = x as usize % (len + 1);
            match op % 5 {
                0 if len > 0 => doc.delete(&o, at.min(len - 1)),
                1 if ty == ObjType::Text => doc.splice_text(&o, at, 0, ["a", "\n", "ab"][y as usize % 3]),
                2 => doc.insert(&o, at, scalar(y)),
                3 => doc.insert_object(&o, at, kind(y)).map(|_| ()),
                _ if ty == ObjType::Text && len > 0 => {
                    let (start, end) = (at.min(len - 1), (at + 1 + y as usize % 4).min(len));
                    doc.mark(&o, Mark::new(NAMES[y as usize % NAMES.len()].to_string(), scalar(y), start, end), ExpandMark::None)
                }
                _ => Ok(()),
            }
        }
    };
}

/// A document holding `content` as its main card's body.
fn held(content: &Content) -> Doc {
    let mut doc = Doc::new();
    let main = doc.main();
    doc.put_value(&main, "body", content);
    doc.commit();
    doc
}

fn bent(content: &Content, hits: &[Hit]) -> AutoCommit {
    let mut doc = AutoCommit::load(&held(content).save()).unwrap();
    for h in hits {
        hit(&mut doc, *h);
    }
    doc
}

/// Every value the main card holds under a key, each read through repair.
fn reads(doc: &Doc) -> Vec<Content> {
    let main = doc.main();
    doc.am()
        .keys(&main)
        .filter_map(|k| doc.value(&main, &k))
        .map(|root| doc.view(&root).content)
        .collect()
}

fn holds(loaded: Result<Doc, LoadError>) -> Result<(), TestCaseError> {
    let Ok(mut doc) = loaded else { return Ok(()) };
    let values = reads(&doc);
    for v in &values {
        prop_assert_eq!(v.validate(), Ok(()));
    }
    let again = Doc::load(&doc.save()).unwrap();
    prop_assert_eq!(reads(&again), values);
    Ok(())
}

fn hits() -> impl Strategy<Value = Vec<Hit>> {
    prop::collection::vec((any::<u8>(), any::<u8>(), any::<u16>(), any::<u16>()), 1..6)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases("CASES", 64), ..ProptestConfig::default() })]

    #[test]
    fn a_document_edited_at_any_depth_loads_or_is_refused(content in document(), seed in any::<u8>(), hits in hits()) {
        let content = with_ext(content, seed);
        holds(Doc::load(&bent(&content, &hits).save()))?;
    }

    #[test]
    fn bytes_cut_or_flipped_load_or_are_refused(content in document(), at in any::<u16>(), bit in 0u8..8, cut in any::<bool>()) {
        let mut bytes = held(&content).save();
        let at = at as usize % bytes.len();
        match cut {
            true => bytes.truncate(at),
            false => bytes[at] ^= 1 << bit,
        }
        holds(Doc::load(&bytes))?;
    }

    #[test]
    fn a_message_bringing_an_edited_document_lands_or_changes_nothing(content in document(), hits in hits()) {
        let mut doc = held(&content);
        let mut peer = bent(&content, &hits);
        let (mut ours, mut theirs) = (SyncState::new(), State::new());
        for _ in 0..8 {
            if let Some(m) = doc.generate_sync_message(&mut ours) {
                peer.sync().receive_sync_message(&mut theirs, quillmark_automerge::automerge::sync::Message::decode(&m.encode()).unwrap()).unwrap();
            }
            let Some(m) = peer.sync().generate_sync_message(&mut theirs) else { break };
            let (kept, mut before) = (ours.encode(), doc.clone());
            match doc.receive_sync_message(&mut ours, SyncMessage::decode(&m.encode()).unwrap()) {
                Ok(()) => holds(Ok(doc.clone()))?,
                Err(_) => {
                    prop_assert_eq!(doc.heads(), before.heads());
                    prop_assert_eq!(ours.encode(), kept);
                    break;
                }
            }
        }
    }
}
