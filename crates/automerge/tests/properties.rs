//! The byte boundary of a held value: [`Held::load`] and a received sync
//! message refuse a document they cannot read rather than panic. The input
//! is a value's own document with generated edits at every depth, keys put,
//! deleted or retyped, text elements of every kind inserted and deleted,
//! marks of anchor and formatting names, and its saved bytes truncated or
//! flipped. A document that loads reads as the value it answered, and syncing
//! it to that value leaves it reading so with nothing to repair. `CASES`
//! raises the case count.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::Held;
use quillmark_automerge::automerge::marks::{ExpandMark, Mark};
use quillmark_automerge::automerge::sync::{State, SyncDoc};
use quillmark_automerge::automerge::transaction::Transactable;
use quillmark_automerge::automerge::{AutoCommit, ObjId, ObjType, ROOT, ReadDoc, ScalarValue, Value};
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

fn bent(content: &Content, hits: &[Hit]) -> AutoCommit {
    let mut doc = AutoCommit::load(&Held::new(content, "").save()).unwrap();
    for h in hits {
        hit(&mut doc, *h);
    }
    doc
}

fn holds(loaded: Result<(Held, quillmark_automerge::Read), quillmark_automerge::LoadError>) -> Result<(), TestCaseError> {
    let Ok((mut held, read)) = loaded else { return Ok(()) };
    prop_assert_eq!(&held.read(), &read);
    held.sync_to(&read.content);
    let again = held.read();
    prop_assert_eq!(&again.content, &read.content);
    prop_assert!(again.repairs.is_empty(), "{:?}", again.repairs);
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
        holds(Held::load(&bent(&content, &hits).save(), ""))?;
    }

    #[test]
    fn bytes_cut_or_flipped_load_or_are_refused(content in document(), at in any::<u16>(), bit in 0u8..8, cut in any::<bool>()) {
        let mut bytes = Held::new(&content, "").save();
        let at = at as usize % bytes.len();
        match cut {
            true => bytes.truncate(at),
            false => bytes[at] ^= 1 << bit,
        }
        holds(Held::load(&bytes, ""))?;
    }

    #[test]
    fn a_message_bringing_an_edited_document_lands_or_changes_nothing(content in document(), hits in hits()) {
        let mut held = Held::new(&content, "");
        let mut peer = bent(&content, &hits);
        let mut before = held.read();
        let (mut ours, mut theirs) = (State::new(), State::new());
        for _ in 0..8 {
            if let Some(m) = held.generate_sync_message(&mut ours) {
                peer.sync().receive_sync_message(&mut theirs, m).unwrap();
            }
            let Some(m) = peer.sync().generate_sync_message(&mut theirs) else { break };
            let kept = ours.clone();
            match held.receive_sync_message(&mut ours, m) {
                Ok(read) => {
                    before = read.clone();
                    holds(Ok((held.clone(), read)))?
                }
                Err(_) => {
                    prop_assert_eq!(&held.read(), &before);
                    prop_assert_eq!(&ours, &kept);
                    break;
                }
            }
        }
    }
}
