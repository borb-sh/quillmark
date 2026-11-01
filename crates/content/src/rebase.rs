//! A change rebased over another made from the same value, after
//! ProseMirror's `rebaseSteps`. Each step maps through the inverse of its
//! change's earlier steps, then the change it rebases over, then those steps
//! rebased, each linked to its inverse as its mirror; it then applies to the
//! value it now reads and holds to its own precondition there. A step that no
//! longer fits is a [`Conflict`] naming it, and nothing is rebased.

use crate::change::{AnchorStep, ApplyError, Assoc, Change, IslandStep, MarkStep, Run, Splice, Step, apply};
use crate::draft::Draft;
use crate::mapping::{MapResult, Mapping, StepMap};
use crate::model::{Anchor, Content, Mark, Pos, Usv};
use crate::units::Scalars;

/// A step of the change that does not fit the value `over` left.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// The step's index in the change, a nested step's for a step inside it.
    pub step: usize,
    pub reason: Reason,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// `over` deleted text the step reads: any of the text a splice deletes,
    /// the whole range of a mark step or a range anchor, or the text on both
    /// sides of an insert.
    Deleted,
    /// The text a splice deletes or a step quotes, or the block an island
    /// insert names, now reads otherwise.
    Changed,
    /// The step names a block, container, anchor, island or slot that is no
    /// longer there.
    Missing(ApplyError),
    /// The step, mapped, does not apply for another reason, or the change
    /// leaves an invalid value.
    Refused(ApplyError),
}

impl Reason {
    /// The reason's name, as a diagnostic's `reason` arg carries it.
    pub fn name(&self) -> &'static str {
        match self {
            Reason::Deleted => "deleted",
            Reason::Changed => "changed",
            Reason::Missing(_) => "missing",
            Reason::Refused(_) => "refused",
        }
    }

    fn of(e: ApplyError) -> Reason {
        match e {
            ApplyError::Stale { .. } => Reason::Changed,
            ApplyError::UnknownBlock { .. }
            | ApplyError::UnknownContainer { .. }
            | ApplyError::UnknownAnchor { .. }
            | ApplyError::UnknownIsland { .. }
            | ApplyError::UnknownSlot { .. } => Reason::Missing(e),
            e => Reason::Refused(e),
        }
    }
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let step = self.step;
        match &self.reason {
            Reason::Deleted => write!(f, "step {step} reads text the change it rebases over deleted"),
            Reason::Changed => write!(f, "step {step} reads text that now reads otherwise"),
            Reason::Missing(e) | Reason::Refused(e) => write!(f, "step {step}: {e}"),
        }
    }
}

impl std::error::Error for Conflict {}

/// Why a rebase answered no change.
#[derive(Debug, Clone, PartialEq)]
pub enum RebaseError {
    /// The change, or the one it rebases over, does not apply to the value
    /// both were made from.
    Apply(ApplyError),
    Conflict(Conflict),
}

impl std::fmt::Display for RebaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebaseError::Apply(e) => write!(f, "{e}"),
            RebaseError::Conflict(c) => write!(f, "{c}"),
        }
    }
}

impl std::error::Error for RebaseError {}

/// `change`, made from `base`, rebased to apply after `over`, also made from
/// `base`: the writer's own steps at their mapped offsets, each holding to
/// what it read.
///
/// - A splice maps the edges of its deletion inward and still deletes its
///   `delete`; one whose text `over` deleted any of, or rewrote, conflicts.
///   An insert maps as a point anchor does, and conflicts where `over`
///   deleted the text on both sides of it.
/// - A mark step and a range anchor map their edges inward, as `apply` moves
///   a range anchor, and a point anchor as `apply` moves one. One whose range
///   `over` deleted entirely conflicts: a write is whole or refused, and a
///   step with nothing to land on is part of what its writer asked for. A
///   `quote` the range no longer covers conflicts.
/// - `setBlock`, `container`, an island's props and anchor removal are by id
///   and pass unchanged, and conflict where the id is gone. `nested` rebases
///   its change over what `over` did inside that slot.
pub fn rebase(base: &Content, change: &Change, over: &Change) -> Result<Change, RebaseError> {
    let own = apply(base, change).map_err(RebaseError::Apply)?;
    let theirs = apply(base, over).map_err(RebaseError::Apply)?;
    let maps = Mapping::from_change(&theirs.applied).map_err(RebaseError::Apply)?;
    let mut now = Draft::new(theirs.content.clone());
    let steps = level(&change.steps, &own.applied.steps, &maps, &mut now, &mut Run::new(false))
        .map_err(|(step, reason)| RebaseError::Conflict(Conflict { step, reason }))?;
    let last = steps.len().saturating_sub(1);
    let rebased = Change { steps };
    apply(&theirs.content, &rebased)
        .map_err(|e| RebaseError::Conflict(Conflict { step: last, reason: Reason::of(e) }))?;
    Ok(rebased)
}

/// The steps of one level, a field or a slot, rebased over `over` and applied
/// in turn to `now`. `norm` is `orig` as `apply` normalized it on the value
/// they were made from, whose maps take a step back across its predecessors.
fn level(orig: &[Step], norm: &[Step], over: &Mapping, now: &mut Draft, run: &mut Run) -> Result<Vec<Step>, (usize, Reason)> {
    let n = orig.len();
    let mut maps = Mapping::new();
    for (i, step) in norm[..n].iter().enumerate().rev() {
        let map = StepMap::of(step, Scalars).map_err(|e| (i, Reason::Refused(e)))?;
        maps.append_map(map.invert(), None);
    }
    maps.append_mapping(over);
    let mut out = Vec::with_capacity(n);
    for (i, step) in orig.iter().enumerate() {
        let through = Tail(&maps, n - i);
        let mapped = match (step, &norm[i]) {
            (Step::Nested { island, slot, change }, Step::Nested { change: made, .. }) => {
                let missing = || (i, Reason::Missing(ApplyError::UnknownSlot { island: island.clone(), slot: slot.clone() }));
                let mut inner = Draft::new(now.c.slot(island, slot).cloned().ok_or_else(missing)?);
                let steps = level(&change.steps, &made.steps, &maps.slice(n - i, maps.len()).project(island, slot), &mut inner, run)
                    .map_err(|(_, reason)| (i, reason))?;
                Step::Nested { island: island.clone(), slot: slot.clone(), change: Change { steps } }
            }
            (step, _) => map_step(step, &through).map_err(|reason| (i, reason))?,
        };
        let (done, _) = run.step(now, &mapped, i).map_err(|e| (i, Reason::of(e)))?;
        let map = StepMap::of(&done, Scalars).map_err(|e| (i, Reason::Refused(e)))?;
        maps.append_map(map, Some(n - 1 - i));
        out.push(mapped);
    }
    Ok(out)
}

/// A mapping's maps from one on.
struct Tail<'a>(&'a Mapping, usize);

impl Tail<'_> {
    fn map(&self, pos: &Pos, assoc: Assoc) -> MapResult {
        self.0.map_from(self.1, pos, assoc)
    }

    fn map_point(&self, pos: &Pos) -> MapResult {
        self.0.map_point_from(self.1, pos)
    }
}

fn offset(r: &MapResult) -> Usv {
    match r.pos {
        Pos::At(p) => p,
        Pos::In { .. } => unreachable!("a level's offset maps to an offset"),
    }
}

/// An insert at `at`, mapped as a point anchor.
fn insert_at(at: Usv, maps: &Tail) -> Result<Usv, Reason> {
    let r = maps.map_point(&Pos::At(at));
    if r.deleted_across {
        return Err(Reason::Deleted);
    }
    Ok(offset(&r))
}

/// A range's edges mapped inward; a range deleted entirely is gone.
fn range(start: Usv, end: Usv, maps: &Tail) -> Result<(Usv, Usv), Reason> {
    let (a, b) = (offset(&maps.map(&Pos::At(start), Assoc::After)), offset(&maps.map(&Pos::At(end), Assoc::Before)));
    if b <= a {
        return Err(Reason::Deleted);
    }
    Ok((a, b))
}

/// An offset-addressed step at its mapped offsets; a step by id as it is.
fn map_step(step: &Step, maps: &Tail) -> Result<Step, Reason> {
    Ok(match step {
        Step::Splice(s) => {
            let dn = s.delete.chars().count();
            let at = if dn == 0 {
                insert_at(s.at, maps)?
            } else {
                let a = maps.map(&Pos::At(s.at), Assoc::After);
                let b = maps.map(&Pos::At(s.at + dn), Assoc::Before);
                if a.deleted || b.deleted {
                    return Err(Reason::Deleted);
                }
                let (a, b) = (offset(&a), offset(&b));
                if b < a || b - a != dn {
                    return Err(Reason::Changed);
                }
                a
            };
            Step::Splice(Splice { at, ..s.clone() })
        }
        Step::Island(IslandStep::Insert { at, island, block }) => {
            Step::Island(IslandStep::Insert { at: insert_at(*at, maps)?, island: island.clone(), block: block.clone() })
        }
        Step::Mark(MarkStep::Add { mark, quote }) => {
            let (start, end) = range(mark.start, mark.end, maps)?;
            Step::Mark(MarkStep::Add { mark: Mark::new(start, end, mark.kind.clone()), quote: quote.clone() })
        }
        Step::Mark(MarkStep::Remove { start, end, tag, quote }) => {
            let (start, end) = range(*start, *end, maps)?;
            Step::Mark(MarkStep::Remove { start, end, tag: tag.clone(), quote: quote.clone() })
        }
        Step::Anchor(AnchorStep::Set { anchor, quote }) => {
            let (start, end) = match anchor.is_point() {
                true => {
                    let p = offset(&maps.map_point(&Pos::At(anchor.start)));
                    (p, p)
                }
                false => range(anchor.start, anchor.end, maps)?,
            };
            Step::Anchor(AnchorStep::Set { anchor: Anchor { start, end, ..anchor.clone() }, quote: quote.clone() })
        }
        Step::SetBlock(_) | Step::Container(_) | Step::Anchor(AnchorStep::Remove { .. }) | Step::Island(IslandStep::Props { .. }) => {
            step.clone()
        }
        Step::Nested { .. } => unreachable!("a nested step rebases its own level"),
    })
}
