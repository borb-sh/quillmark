//! Two changes made from one value, merged as Automerge merges them, and the
//! conflicts a merge can still name: a record or an island's props both
//! sides set, or one the other side removed.

use crate::{Held, Read};
use quillmark_content::change::{ApplyError, Change, ContainerStep, IslandStep, Step, apply, apply_traced};
use quillmark_content::diff::{self, Keeping, Revision, ReviseError};
use quillmark_content::model::{Block, Content};
use std::collections::HashMap;

/// `ours` and `theirs`, both made from `base`, merged: `ours` written on
/// `base` and `theirs` on a fork of it at `base`, the merged state read
/// through repair.
pub fn merge3(base: &Content, ours: &Change, theirs: &Change) -> Result<Read, ApplyError> {
    let (mine, mine_trace) = apply_traced(base, ours)?;
    let (since, since_trace) = apply_traced(base, theirs)?;
    let mut held = Held::new(base);
    let heads = held.heads();
    held.write(&mine_trace, &mine.content);
    let mut fork = held.fork_at(&heads, None).expect("a document forks at its own heads");
    fork.write(&since_trace, &since.content);
    held.merge(&mut fork).expect("a fork merges back into its document");
    Ok(held.read())
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
    /// The block, container, island or slot the step names is gone since.
    Missing,
}

impl Reason {
    /// The reason's name, as a diagnostic's `reason` arg carries it.
    pub fn name(self) -> &'static str {
        match self {
            Reason::Changed => "changed",
            Reason::Missing => "missing",
        }
    }
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason {
            Reason::Changed => write!(f, "step {} sets what a change since its base set", self.step),
            Reason::Missing => write!(f, "step {} names what a change since its base removed", self.step),
        }
    }
}

/// The first step of `change`, made from `base`, that sets a block record's
/// fields, a container or an island's props `now` holds otherwise than
/// `base` did, or names one of them `base` holds and `now` does not; a
/// splice that deletes or puts back a block or island `base` holds counts as
/// setting all of it. Only what `base` held is checked: a record the change
/// puts is its own.
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

fn records(c: &Content) -> HashMap<&str, &Block> {
    c.blocks.iter().map(|b| (b.id.as_str(), b)).collect()
}
