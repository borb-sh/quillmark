//! A long change lowered at earlier heads, where the document forks there. Automerge isolated at earlier
//! heads reads every op against them, which costs the whole text per op, so
//! the change is lowered on a fork at those heads and the change it makes
//! there lands in the document as the one isolation would have made: under
//! the actor isolation picks, its op counters past every op the document
//! holds.

use automerge::legacy::{ElementId, Key, ObjectId, OpId, OpType};
use automerge::{ActorId, AutoCommit, ChangeHash, ExpandedChange, ObjId, ScalarValue};
use quillmark_content::change::Change;
use quillmark_content::model::Content;

use super::step::{self, Lowered, StepError};

pub(super) fn apply(
    doc: &mut AutoCommit,
    root: &ObjId,
    change: &Change,
    heads: &[ChangeHash],
    read: Option<&Content>,
) -> Option<Result<Lowered, StepError>> {
    let mut fork = super::fork(doc, heads)?;
    let actor = isolated_actor(doc, heads);
    fork.set_actor(actor.clone());
    let mut lowered = match step::lower(&mut fork, root, change, read) {
        Ok(l) => l,
        Err(e) => return Some(Err(e)),
    };
    fork.commit();
    let Some(made) = fork.get_changes(heads).into_iter().find(|c| *c.actor_id() == actor) else {
        return Some(Ok(lowered));
    };
    let past = max_op(doc);
    let start = made.start_op().get();
    let shift = past.saturating_sub(start - 1);
    let hex = actor.to_hex_string();
    let mut e: ExpandedChange = made.decode();
    let moved = |id: &OpId| match id.1 == actor && id.0 >= start {
        true => OpId(id.0 + shift, id.1.clone()),
        false => id.clone(),
    };
    for op in &mut e.operations {
        if let ObjectId::Id(id) = &op.obj {
            op.obj = ObjectId::Id(moved(id));
        }
        if let Key::Seq(ElementId::Id(id)) = &op.key {
            op.key = Key::Seq(ElementId::Id(moved(id)));
        }
        op.pred = op.pred.iter().map(moved).collect();
        if let OpType::Put(ScalarValue::Str(v)) = &op.action
            && let Some(named) = renamed(v, &hex, start, shift)
        {
            op.action = OpType::Put(ScalarValue::Str(named.into()));
        }
    }
    e.start_op = std::num::NonZeroU64::new(start + shift).expect("op counters start at one");
    e.hash = None;
    doc.apply_changes([automerge::Change::from(e)]).expect("a change made at the document's heads applies");
    lowered.minted = lowered
        .minted
        .into_iter()
        .map(|(k, v)| {
            let v = renamed(&v, &hex, start, shift).unwrap_or(v);
            (k, v)
        })
        .collect();
    Some(Ok(lowered))
}

/// The actor a write isolated at `heads` writes under: the document's own
/// where no change of it lies past `heads`, otherwise the first of its
/// concurrency levels none does, as Automerge picks one.
fn isolated_actor(doc: &mut AutoCommit, heads: &[ChangeHash]) -> ActorId {
    let since: Vec<ActorId> = doc.get_changes(heads).iter().map(|c| c.actor_id().clone()).collect();
    let base = doc.get_actor().clone();
    (0..128)
        .map(|i: u8| match i {
            0 => base.clone(),
            _ => ActorId::from([&CONCURRENT[..], &[i], base.to_bytes()].concat()),
        })
        .find(|a| !since.contains(a))
        .expect("a free concurrency level under 128")
}

/// The string naming an op of the fork's change by its id, as an id or as a
/// cursor, `shift` counters on; none for any other string. A block's path,
/// a point's cursor and a minted id are written so.
fn renamed(v: &str, actor: &str, start: u64, shift: u64) -> Option<String> {
    let (lead, id) = v.strip_prefix('-').map_or(("", v), |rest| ("-", rest));
    let (ctr, a) = id.split_once('@')?;
    let c: u64 = ctr.parse().ok()?;
    (a == actor && c >= start).then(|| format!("{lead}{}@{a}", c + shift))
}

/// What Automerge opens a concurrency level's actor with, before the level
/// and the base actor's bytes.
const CONCURRENT: [u8; 4] = [0x13, 0xb2, 0x23, 0x09];

/// The greatest op counter the document holds: its heads' changes hold it.
fn max_op(doc: &mut AutoCommit) -> u64 {
    doc.get_heads()
        .iter()
        .filter_map(|h| doc.get_change_by_hash(h).map(|c| c.max_op()))
        .max()
        .unwrap_or(0)
}
