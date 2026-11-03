//! Two changes made from one value, merged as Automerge merges them, and the
//! conflicts a merge can still name: a record or an island's props both
//! sides set, or one the other side removed.

use crate::{Held, Read, map};
use quillmark_content::change::{AnchorStep, ApplyError, Assoc, Change, ContainerStep, IslandStep, Step, apply, apply_traced, map_pos};
use quillmark_content::diff::{self, Keeping, Revision, ReviseError};
use quillmark_content::model::{Block, Content, Pos};
use std::collections::{HashMap, HashSet};

/// `ours` and `theirs`, both made from `base`, merged: `ours` written on
/// `base` and `theirs` on a fork of it at `base`, the anchors and marks
/// `ours` put held to their edges ([`Held::clip`]), and the merged state
/// read through repair.
pub fn merge3(base: &Content, ours: &Change, theirs: &Change) -> Result<Read, ApplyError> {
    merge3_checked(base, ours, theirs).map(|(read, _)| read)
}

/// [`merge3`], and the first step of `ours` that deleted text whose
/// formatting `theirs` changed ([`Held::restyled`]).
pub fn merge3_checked(base: &Content, ours: &Change, theirs: &Change) -> Result<(Read, Option<Conflict>), ApplyError> {
    let (mine, mine_trace) = apply_traced(base, ours)?;
    let (since, since_trace) = apply_traced(base, theirs)?;
    let mut held = Held::new(base, "");
    let at_base = held.clone();
    let heads = held.heads();
    held.write(&mine_trace, &mine.content);
    let mine_heads = held.heads();
    let mut fork = held.fork_at(&heads, None).expect("a document forks at its own heads");
    fork.write(&since_trace, &since.content);
    let theirs_heads = fork.heads();
    let restyled = Held::restyled(&at_base, &held, &fork, &mine_trace, &mine.applied);
    held.merge(&mut fork).expect("a fork merges back into its document");
    held.clip(&mine_heads, &theirs_heads, &mine_trace);
    Ok((held.read(), restyled))
}

/// The first step of `applied`, a change made from a value, that deletes
/// the character at offset `at` of the text `path` reaches there.
pub(crate) fn step_deleting(applied: &Change, path: &[(String, String)], at: usize) -> usize {
    let mut pos = path.iter().rev().fold(Pos::At(at), |inner, (island, slot)| Pos::In { island: island.clone(), slot: slot.clone(), at: Box::new(inner) });
    for (k, step) in applied.steps.iter().enumerate() {
        let Ok(r) = map_pos(&Change::new(vec![step.clone()]), &pos, Assoc::After) else { return k };
        if r.deleted_after || r.deleted {
            return k;
        }
        pos = r.pos;
    }
    0
}

/// [`revise_keeping`] of a write that lands on `before`.
pub fn revise(before: &Content, markdown: &str) -> Result<Revision, ReviseError> {
    revise_keeping(before, markdown, before)
}

/// A markdown rewrite of `before` that keeps what its annotated read does not
/// spell ([`diff::revise_keeping`]): where the read loses nothing, the
/// revision itself; otherwise the writer's revision of the read merged with
/// the change from the read to `before`, so what the read lost rides back in
/// wherever the writer's edit leaves room for it. The change answered takes
/// `before` to the merge.
pub fn revise_keeping(before: &Content, markdown: &str, now: &Content) -> Result<Revision, ReviseError> {
    let (read, since, mine) = match diff::revise_keeping(before, markdown, now)? {
        Keeping::Revised(r) => return Ok(r),
        Keeping::Merge { read, since, mine } => (read, since, mine),
    };
    if mine.change.is_empty() {
        return Ok(Revision { change: Change::default(), content: before.clone(), warnings: mine.warnings });
    }
    let merged = merge3(&read, &mine.change, &since).map_err(ReviseError::Refused)?.content;
    let change = diff::diff(before, &merged).map_err(|e| ReviseError::Refused(e.into()))?;
    let content = apply(before, &change).map_err(ReviseError::Refused)?.content;
    Ok(Revision { change, content, warnings: mine.warnings })
}

/// A step of a change that sets what a change since its base also set or
/// removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The step's index in the change, a nested step's for a step inside it.
    pub step: usize,
    pub reason: Reason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// The block record, container or island props the step sets, the
    /// fields it names, changed since.
    Changed,
    /// The block, container, island or slot the step names is gone since,
    /// or the record it names is gone and its id held by a record minted
    /// since.
    Missing,
    /// The anchor the step placed or moved lost its text to a change since.
    Deleted,
}

impl Reason {
    /// The reason's name, as a diagnostic's `reason` arg carries it.
    pub fn name(self) -> &'static str {
        match self {
            Reason::Changed => "changed",
            Reason::Missing => "missing",
            Reason::Deleted => "deleted",
        }
    }
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason {
            Reason::Changed => write!(f, "step {} sets what a change since its base set", self.step),
            Reason::Missing => write!(f, "step {} names what a change since its base removed", self.step),
            Reason::Deleted => write!(f, "step {} puts an anchor on text a change since its base deleted", self.step),
        }
    }
}

/// The first step of `change`, made from `base`, that sets a block record's
/// fields, a container or an island's props `now` holds otherwise than
/// `base` did, or names one of them `base` holds and `now` does not; a
/// splice that deletes or puts back a block or island `base` holds counts as
/// setting all of it. Only what `base` held is checked: a record the change
/// puts is its own.
#[doc(hidden)]
pub fn conflict(base: &Content, now: &Content, change: &Change) -> Option<Conflict> {
    let applied = apply(base, change).ok()?.applied;
    first_conflict(base, now, &applied.steps).map(|(step, reason)| Conflict { step, reason })
}

fn first_conflict(base: &Content, now: &Content, steps: &[Step]) -> Option<(usize, Reason)> {
    let (was, is) = (records(base), records(now));
    for (n, step) in steps.iter().enumerate() {
        let reason = match step {
            Step::SetBlock(s) => match (was.get(s.id.as_str()), is.get(s.id.as_str())) {
                (Some(_), None) => Some(Reason::Missing),
                (Some(w), Some(i)) => {
                    let kind = (s.kind.is_some() || s.attrs.is_some()) && w.kind != i.kind;
                    let path = s.path.is_some() && w.path != i.path;
                    (kind || path).then_some(Reason::Changed)
                }
                _ => None,
            },
            Step::Splice(s) => {
                let gone = s.removed.iter().flat_map(|r| r.blocks.iter().map(|b| b.id.as_str()));
                let put = s.blocks.iter().map(|b| b.id.as_str());
                let block = put.chain(gone).find_map(|id| match (was.get(id), is.get(id)) {
                    (Some(_), None) if s.blocks.iter().any(|b| b.id == id) => Some(Reason::Missing),
                    (Some(w), Some(i)) if w != i => Some(Reason::Changed),
                    _ => None,
                });
                let held = |v: &Content, id: &str| v.islands.iter().find(|i| i.id == id).cloned();
                let gone = s.removed.iter().flat_map(|r| r.islands.iter().map(|i| (i.id.as_str(), false)));
                let put = s.islands.iter().map(|i| (i.id.as_str(), true));
                block.or_else(|| {
                    put.chain(gone).find_map(|(id, puts)| match (held(base, id), held(now, id)) {
                        (Some(_), None) if puts => Some(Reason::Missing),
                        (Some(w), Some(i)) if w != i => Some(Reason::Changed),
                        _ => None,
                    })
                })
            }
            Step::Container(c) => {
                let id = match c {
                    ContainerStep::Set(k) => &k.id,
                    ContainerStep::Remove { id } => id,
                };
                let held = |v: &Content| v.containers.iter().find(|k| &k.id == id).cloned();
                match (held(base), held(now)) {
                    (Some(_), None) => Some(Reason::Missing),
                    (Some(w), Some(i)) if w != i => Some(Reason::Changed),
                    _ => None,
                }
            }
            Step::Island(IslandStep::Props { id, .. }) => {
                let held = |v: &Content| v.islands.iter().find(|i| &i.id == id).map(|i| i.props.clone());
                match (held(base), held(now)) {
                    (Some(_), None) => Some(Reason::Missing),
                    (Some(w), Some(i)) if w != i => Some(Reason::Changed),
                    _ => None,
                }
            }
            Step::Nested { island, slot, change } => {
                let held = |v: &Content| v.islands.iter().find(|i| &i.id == island).and_then(|i| i.slots.get(slot)).cloned();
                match (held(base), held(now)) {
                    (Some(_), None) => Some(Reason::Missing),
                    (Some(w), Some(i)) => first_conflict(&w, &i, &change.steps).map(|(_, r)| r),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(r) = reason {
            return Some((n, r));
        }
    }
    None
}

/// The first step of `applied`, a change as `apply` normalized it, that
/// places or moves an anchor `mine`, the value it gave, holds and `merged`
/// holds nowhere: the merge deleted the text it was put on.
#[doc(hidden)]
pub fn deleted(applied: &Change, mine: &Content, merged: &Content) -> Option<Conflict> {
    let (put, kept) = (anchor_ids(mine), anchor_ids(merged));
    first_putting(applied, |id| put.contains(id) && !kept.contains(id), Reason::Deleted)
}

/// The first step of `applied`, a change made from `base`, that places or
/// moves an anchor `base` holds and `now`, the value since, holds nowhere:
/// the anchor was removed since, and the step would bring it back.
#[doc(hidden)]
pub fn resolved(applied: &Change, base: &Content, now: &Content) -> Option<Conflict> {
    let (was, kept) = (anchor_ids(base), anchor_ids(now));
    first_putting(applied, |id| was.contains(id) && !kept.contains(id), Reason::Missing)
}

/// Each anchor id a value holds, at any depth.
fn anchor_ids(c: &Content) -> HashSet<String> {
    fn held(c: &Content, out: &mut HashSet<String>) {
        out.extend(c.anchors.iter().map(|a| a.id.clone()));
        for island in &c.islands {
            for inner in island.slots.values() {
                held(inner, out);
            }
        }
    }
    let mut out = HashSet::new();
    held(c, &mut out);
    out
}

/// The first step of `applied` that places or moves an anchor `hit` holds
/// for, at any depth, refused for `reason`.
fn first_putting(applied: &Change, hit: impl Fn(&str) -> bool, reason: Reason) -> Option<Conflict> {
    fn puts(step: &Step, hit: &dyn Fn(&str) -> bool) -> bool {
        match step {
            Step::Anchor(AnchorStep::Place { anchor, .. } | AnchorStep::Move { anchor, .. }) => hit(&anchor.id),
            Step::Nested { change, .. } => change.steps.iter().any(|s| puts(s, hit)),
            _ => false,
        }
    }
    applied.steps.iter().position(|s| puts(s, &hit)).map(|step| Conflict { step, reason })
}

/// The first step of `applied`, a change as `apply` normalized it from the
/// value `was` holds, that names a block or island record by an id `now`
/// holds on another record: the one the step read is gone, and its id was
/// minted again since.
#[doc(hidden)]
pub fn reminted(was: &Held, now: &Held, applied: &Change) -> Option<Conflict> {
    fn first(was: &Held, now: &Held, path: &mut Vec<(String, String)>, steps: &[Step]) -> bool {
        fn by_id(s: &Step) -> Option<&str> {
            match s {
                Step::SetBlock(b) => Some(b.id.as_str()),
                Step::Island(IslandStep::Props { id, .. }) => Some(id.as_str()),
                Step::Nested { island, .. } => Some(island.as_str()),
                _ => None,
            }
        }
        if !steps.iter().any(|s| by_id(s).is_some()) {
            return false;
        }
        let at = |h: &Held| map::reach(&h.doc, &h.root, path).map(|t| map::markers(&h.doc, &t));
        let (Some(w), Some(n)) = (at(was), at(now)) else { return false };
        steps.iter().any(|s| {
            let swapped = by_id(s).is_some_and(|id| matches!((w.get(id), n.get(id)), (Some(a), Some(b)) if a != b));
            swapped
                || match s {
                    Step::Nested { island, slot, change } => {
                        path.push((island.clone(), slot.clone()));
                        let found = first(was, now, path, &change.steps);
                        path.pop();
                        found
                    }
                    _ => false,
                }
        })
    }
    applied
        .steps
        .iter()
        .position(|s| first(was, now, &mut Vec::new(), std::slice::from_ref(s)))
        .map(|step| Conflict { step, reason: Reason::Missing })
}

fn records(c: &Content) -> HashMap<&str, &Block> {
    c.blocks.iter().map(|b| (b.id.as_str(), b)).collect()
}
