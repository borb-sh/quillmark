//! A change rebased over another made from the same value, after
//! ProseMirror's `rebaseSteps`. Each step maps through the inverse of its
//! change's earlier steps, then the change it rebases over, then those steps
//! rebased, each linked to its inverse as its mirror; it then applies to the
//! value it now reads and holds to its own precondition there. A step that no
//! longer fits is a [`Conflict`] naming it, and nothing is rebased.

use crate::change::{AnchorStep, ApplyError, Assoc, Change, ContainerStep, IslandStep, MarkStep, Run, Splice, Step, apply};
use crate::draft::Draft;
use crate::ids::{Minter, prefix};
use crate::mapping::{MapResult, Mapping, StepMap, moved_islands};
use crate::model::{Anchor, Content, Mark, MarkKind, Pos, Usv};
use crate::units::Scalars;
use std::collections::{HashMap, HashSet};

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
    /// The text a splice deletes or a step quotes, the block an island insert
    /// names, what a step by id overwrites, an island, block record or mark
    /// a splice deletes, the formatting of a mark step's type over its range,
    /// or the slot a nested step edits where `over` removed it, now reads
    /// otherwise; or `over` placed, moved or removed the anchor the step
    /// names.
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
            ApplyError::Stale { .. } | ApplyError::AnchorHeld { .. } => Reason::Changed,
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
            Reason::Changed => write!(f, "step {step} reads what the change it rebases over changed"),
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
///   and pass unchanged, and conflict where the id is gone. One that
///   overwrites what `over` changed conflicts: the fields of a block's record
///   a `setBlock` names, a container's attributes, an island's props and the
///   slots they drop.
/// - A splice conflicts where an island, block record or mark it deletes
///   reads otherwise than its writer read it, so a stale rewrite of a word
///   `over` formatted conflicts rather than dropping the formatting.
/// - A mark step conflicts where `over` changed the formatting of its type on
///   a character of its range both values hold.
/// - An anchor step conflicts where an `anchor` step of `over` names its id:
///   a stale write never brings back an anchor `over` removed, nor wins one
///   both moved.
/// - `nested` rebases its change over what `over` did inside that slot. Where
///   `over` removed the island or the slot and the slot now reads otherwise,
///   its edits are not in the mapping, and the step conflicts.
///
/// Identity is by incarnation. A block, container or island `over` removed
/// is gone for the change even where `over` put one back under its id, so a
/// step naming it conflicts as missing rather than landing on the new one;
/// an island `over` only moves keeps its identity. An id the change
/// introduces that `over` also introduced is renamed past every id either
/// holds, through the change's steps after it.
pub fn rebase(base: &Content, change: &Change, over: &Change) -> Result<Change, RebaseError> {
    apply(base, change).map_err(RebaseError::Apply)?;
    let theirs = apply(base, over).map_err(RebaseError::Apply)?;
    let over_steps: Vec<&Step> = theirs.applied.steps.iter().collect();
    let change = Change { steps: renamed(&change.steps, &over_steps, base, &theirs.content) };
    let own = apply(base, &change).map_err(RebaseError::Apply)?;
    let maps = Mapping::from_change(&theirs.applied).map_err(RebaseError::Apply)?;
    let (mut was, mut now) = (Draft::new(base.clone()), Draft::new(theirs.content.clone()));
    let steps = level(&change.steps, &own.applied.steps, &maps, &over_steps, &mut was, &mut now)
        .map_err(|(step, reason)| RebaseError::Conflict(Conflict { step, reason }))?;
    let last = steps.len().saturating_sub(1);
    let rebased = Change { steps };
    apply(&theirs.content, &rebased)
        .map_err(|e| RebaseError::Conflict(Conflict { step: last, reason: Reason::of(e) }))?;
    Ok(rebased)
}

/// The steps of one level, a field or a slot, rebased over `over` and applied
/// in turn to `now`, as `orig` applies in turn to `was`, the value they were
/// made from. `norm` is `orig` as `apply` normalized it there, whose maps
/// take a step back across its predecessors.
fn level(
    orig: &[Step],
    norm: &[Step],
    over: &Mapping,
    over_steps: &[&Step],
    was: &mut Draft,
    now: &mut Draft,
) -> Result<Vec<Step>, (usize, Reason)> {
    let n = orig.len();
    let gone = removed_by(over_steps);
    let stepped: HashSet<&str> = over_steps
        .iter()
        .filter_map(|s| match s {
            Step::Anchor(a) => Some(a.id()),
            _ => None,
        })
        .collect();
    let mine = introduced_by(orig, &was.c);
    let mut maps = Mapping::new();
    for (i, step) in norm[..n].iter().enumerate().rev() {
        let map = StepMap::of(step, Scalars).map_err(|e| (i, Reason::Refused(e)))?;
        maps.append_map(map.invert(), None);
    }
    maps.append_mapping(over);
    let mut out = Vec::with_capacity(n);
    let (mut own, mut put) = (HashSet::new(), mine.iter().peekable());
    for (i, step) in orig.iter().enumerate() {
        let through = Tail(&maps, n - i);
        while let Some(m) = put.next_if(|m| m.step <= i) {
            own.insert((m.scope, m.id.as_str()));
        }
        if let Some((scope, id)) = names(step).into_iter().find(|named| gone.contains(named) && !own.contains(named)) {
            return Err((i, Reason::Missing(scope.unknown(id))));
        }
        let mapped = match (step, &norm[i]) {
            (Step::Nested { island, slot, change }, Step::Nested { change: made, .. }) => {
                let missing = || (i, Reason::Missing(ApplyError::UnknownSlot { island: island.clone(), slot: slot.clone() }));
                let theirs = now.c.slot(island, slot).cloned().ok_or_else(missing)?;
                let read = was.c.slot(island, slot).cloned().ok_or_else(missing)?;
                if rehomed(over, island, slot) && read != theirs {
                    return Err((i, Reason::Changed));
                }
                let inner = maps.slice(n - i, maps.len()).project(island, slot);
                let theirs_in = inside(over_steps, island, slot);
                let steps = level(&change.steps, &made.steps, &inner, &theirs_in, &mut Draft::new(read), &mut Draft::new(theirs))
                    .map_err(|(_, reason)| (i, reason))?;
                Step::Nested { island: island.clone(), slot: slot.clone(), change: Change { steps } }
            }
            (step, _) => map_step(step, &through).map_err(|reason| (i, reason))?,
        };
        let reformats = reformats(step, was, now, &through);
        let read = Run::new(false).step(was, step, i).map_err(|e| (i, Reason::Refused(e)))?;
        let wrote = Run::new(false).step(now, &mapped, i).map_err(|e| (i, Reason::of(e)))?;
        let restepped = matches!(step, Step::Anchor(a) if stepped.contains(a.id()));
        if reformats || restepped || overwrites(step, &read, &wrote) {
            return Err((i, Reason::Changed));
        }
        let map = StepMap::of(&wrote.0, Scalars).map_err(|e| (i, Reason::Refused(e)))?;
        maps.append_map(map, Some(n - 1 - i));
        out.push(mapped);
    }
    Ok(out)
}

/// The kind of part an id names: each kind holds its ids apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Block,
    Container,
    Island,
}

impl Scope {
    fn unknown(self, id: &str) -> ApplyError {
        let id = id.to_string();
        match self {
            Scope::Block => ApplyError::UnknownBlock { id },
            Scope::Container => ApplyError::UnknownContainer { id },
            Scope::Island => ApplyError::UnknownIsland { id },
        }
    }
}

/// An id a change's step at one level puts in the value.
struct Introduced {
    step: usize,
    scope: Scope,
    id: String,
    prefix: &'static str,
}

/// The steps of `change` inside one slot, in order.
fn inside<'a>(change: &[&'a Step], island: &str, slot: &str) -> Vec<&'a Step> {
    change
        .iter()
        .filter_map(|s| match s {
            Step::Nested { island: i, slot: k, change } if i == island && k == slot => Some(&change.steps),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Each id the normalized `steps` of one level take out of the value: a
/// block record or island a splice deletes and a container removed, an
/// island the steps only move aside.
fn removed_by<'a>(steps: &[&'a Step]) -> HashSet<(Scope, &'a str)> {
    let moved = moved_islands(steps.iter().copied());
    let mut out = HashSet::new();
    for (n, step) in steps.iter().enumerate() {
        match step {
            Step::Splice(s) => {
                if let Some(r) = &s.removed {
                    out.extend(r.blocks.iter().map(|b| (Scope::Block, b.id.as_str())));
                    out.extend(r.islands.iter().filter(|i| !moved.contains(&(n, i.id.as_str()))).map(|i| (Scope::Island, i.id.as_str())));
                }
            }
            Step::Container(ContainerStep::Remove { id }) => {
                out.insert((Scope::Container, id.as_str()));
            }
            _ => {}
        }
    }
    out
}

/// Each id `steps`, one level of a change made from `base`, put in the value,
/// in order: a block record or island a step inserts, and a container one
/// sets that `base` does not hold or a step before it removed.
fn introduced_by<'a>(steps: impl IntoIterator<Item = &'a Step>, base: &Content) -> Vec<Introduced> {
    let mut out = Vec::new();
    let mut removed = HashSet::new();
    let mut put = |step, scope, id: &str, prefix| out.push(Introduced { step, scope, id: id.to_string(), prefix });
    for (n, step) in steps.into_iter().enumerate() {
        match step {
            Step::Splice(s) => {
                s.blocks.iter().for_each(|b| put(n, Scope::Block, &b.id, prefix::BLOCK));
                s.islands.iter().for_each(|i| put(n, Scope::Island, &i.id, prefix::ISLAND));
            }
            Step::Island(IslandStep::Insert { island, .. }) => put(n, Scope::Island, &island.id, prefix::ISLAND),
            Step::Container(ContainerStep::Set(k)) if base.container(&k.id).is_none() || removed.contains(&k.id) => {
                put(n, Scope::Container, &k.id, prefix::container(&k.kind))
            }
            Step::Container(ContainerStep::Remove { id }) => {
                removed.insert(id.clone());
            }
            _ => {}
        }
    }
    out
}

/// Each id `step` names a part by that the value held before it, on its
/// own level: a record, container or island it overwrites, and a container
/// a path it writes nests in.
fn names(step: &Step) -> Vec<(Scope, &str)> {
    fn path(p: &[String]) -> Vec<(Scope, &str)> {
        p.iter().map(|k| (Scope::Container, k.as_str())).collect()
    }
    match step {
        Step::SetBlock(s) => {
            let mut out = vec![(Scope::Block, s.id.as_str())];
            out.extend(s.path.as_deref().map(path).unwrap_or_default());
            out
        }
        Step::Splice(s) => s.blocks.iter().flat_map(|b| path(&b.path)).collect(),
        Step::Container(ContainerStep::Set(k)) => vec![(Scope::Container, k.id.as_str())],
        Step::Container(ContainerStep::Remove { id }) => vec![(Scope::Container, id.as_str())],
        Step::Island(IslandStep::Props { id, .. }) => vec![(Scope::Island, id.as_str())],
        Step::Island(IslandStep::Insert { block: Some(b), .. }) => vec![(Scope::Block, b.as_str())],
        _ => Vec::new(),
    }
}

/// `steps`, one level of a change made from `base`, with each id they
/// introduce that `theirs`, made from `base` too, also introduced renamed
/// past every id `base`, `now` and either change holds, from the step that
/// introduces it on, and so in each slot both edit.
fn renamed(steps: &[Step], theirs: &[&Step], base: &Content, now: &Content) -> Vec<Step> {
    let mine = introduced_by(steps, base);
    let other: HashSet<(Scope, String)> = introduced_by(theirs.iter().copied(), base).into_iter().map(|t| (t.scope, t.id)).collect();
    let mut out = steps.to_vec();
    if mine.iter().any(|m| other.contains(&(m.scope, m.id.clone()))) {
        let mut ids = Minter::default();
        for c in [base, now] {
            c.blocks.iter().map(|b| &b.id).chain(c.containers.iter().map(|k| &k.id)).chain(c.islands.iter().map(|i| &i.id)).for_each(|id| ids.hold(id));
        }
        mine.iter().for_each(|m| ids.hold(&m.id));
        other.iter().for_each(|(_, id)| ids.hold(id));
        let mut to: HashMap<(Scope, String), (usize, String)> = HashMap::new();
        for m in &mine {
            let key = (m.scope, m.id.clone());
            if other.contains(&key) && !to.contains_key(&key) {
                to.insert(key, (m.step, ids.mint(m.prefix)));
            }
        }
        for (n, step) in out.iter_mut().enumerate() {
            rename(step, &|scope, id: &mut String| {
                if let Some((from, fresh)) = to.get(&(scope, id.clone()))
                    && *from <= n
                {
                    *id = fresh.clone();
                }
            });
        }
    }
    let mut slots: Vec<((String, String), Vec<usize>)> = Vec::new();
    for (n, step) in out.iter().enumerate() {
        if let Step::Nested { island, slot, .. } = step {
            match slots.iter_mut().find(|((i, s), _)| i == island && s == slot) {
                Some((_, at)) => at.push(n),
                None => slots.push(((island.clone(), slot.clone()), vec![n])),
            }
        }
    }
    for ((island, slot), at) in slots {
        let over = inside(theirs, &island, &slot);
        let (Some(was), Some(held)) = (base.slot(&island, &slot), now.slot(&island, &slot)) else { continue };
        if over.is_empty() {
            continue;
        }
        let flat: Vec<Step> = at
            .iter()
            .flat_map(|&n| match &out[n] {
                Step::Nested { change, .. } => change.steps.clone(),
                _ => Vec::new(),
            })
            .collect();
        let mut fresh = renamed(&flat, &over, was, held).into_iter();
        for n in at {
            if let Step::Nested { change, .. } = &mut out[n] {
                change.steps = fresh.by_ref().take(change.steps.len()).collect();
            }
        }
    }
    out
}

/// `step` with each block, container and island id on its own level put
/// through `to`.
fn rename(step: &mut Step, to: &dyn Fn(Scope, &mut String)) {
    let block = |b: &mut crate::model::Block| {
        to(Scope::Block, &mut b.id);
        b.path.iter_mut().for_each(|k| to(Scope::Container, k));
    };
    match step {
        Step::Splice(s) => {
            s.blocks.iter_mut().for_each(block);
            s.islands.iter_mut().for_each(|i| to(Scope::Island, &mut i.id));
            if let Some(r) = &mut s.removed {
                r.blocks.iter_mut().for_each(block);
                r.islands.iter_mut().for_each(|i| to(Scope::Island, &mut i.id));
            }
        }
        Step::SetBlock(s) => {
            to(Scope::Block, &mut s.id);
            s.path.iter_mut().flatten().for_each(|k| to(Scope::Container, k));
        }
        Step::Container(ContainerStep::Set(k)) => to(Scope::Container, &mut k.id),
        Step::Container(ContainerStep::Remove { id }) => to(Scope::Container, id),
        Step::Island(IslandStep::Props { id, .. }) => to(Scope::Island, id),
        Step::Island(IslandStep::Insert { island, block, .. }) => {
            to(Scope::Island, &mut island.id);
            block.iter_mut().for_each(|b| to(Scope::Block, b));
        }
        Step::Nested { island, .. } => to(Scope::Island, island),
        Step::Mark(_) | Step::Anchor(_) => {}
    }
}

/// Whether `over` removed the island, or that slot of it, so that what the
/// slot holds is not reached by mapping its edits.
fn rehomed(over: &Mapping, island: &str, slot: &str) -> bool {
    over.maps().iter().any(|map| match map {
        StepMap::Replace { removed, .. } => removed.iter().any(|id| id == island),
        StepMap::Slots { island: id, removed, .. } => id == island && removed.contains(slot),
        _ => false,
    })
}

/// Whether `step`, run where it was made (`read`) and where it now lands
/// (`wrote`), overwrites something that reads otherwise: what its inverse
/// puts back, for the fields it sets, or the islands, block records and
/// marks a splice deletes.
fn overwrites(step: &Step, (read, put): &(Step, Vec<Step>), (wrote, puts): &(Step, Vec<Step>)) -> bool {
    match (step, read, wrote, put.as_slice(), puts.as_slice()) {
        (Step::Splice(_), Step::Splice(a), Step::Splice(b), ..) => {
            let (a, b) = (a.removed.as_ref(), b.removed.as_ref());
            a.map(|r| (&r.islands, &r.blocks, &r.marks)) != b.map(|r| (&r.islands, &r.blocks, &r.marks))
        }
        (Step::SetBlock(s), _, _, [Step::SetBlock(x)], [Step::SetBlock(y)]) => {
            ((s.kind.is_some() || s.attrs.is_some()) && (&x.kind, &x.attrs) != (&y.kind, &y.attrs))
                || (s.path.is_some() && x.path != y.path)
        }
        (Step::Container(_) | Step::Island(IslandStep::Props { .. }), ..) => put != puts,
        _ => false,
    }
}

/// Whether `over` changed the formatting of a mark step's type on a
/// character of its range that `was`, the value the step reads, and `now`,
/// where it lands, both hold.
fn reformats(step: &Step, was: &Draft, now: &Draft, through: &Tail) -> bool {
    let (start, end, tag) = match step {
        Step::Mark(MarkStep::Add { mark, .. }) => (mark.start, mark.end, mark.kind.tag()),
        Step::Mark(MarkStep::Remove { start, end, tag, .. }) => (*start, *end, tag.as_str()),
        _ => return false,
    };
    if start >= end || end > was.len() {
        return false;
    }
    let read = formatting(was, start, end, tag);
    let lo = offset(&through.map(&Pos::At(start), Assoc::After));
    let hi = offset(&through.map(&Pos::At(end), Assoc::Before)).clamp(lo, now.len());
    let theirs = formatting(now, lo, hi, tag);
    (start..end).zip(read).any(|(p, held)| {
        let r = through.map(&Pos::At(p), Assoc::After);
        let q = offset(&r);
        !r.deleted_after && (lo..hi).contains(&q) && theirs[q - lo] != held
    })
}

/// The mark of type `tag` on each character of `[start, end)` in `d`.
fn formatting(d: &Draft, start: Usv, end: Usv, tag: &str) -> Vec<Option<MarkKind>> {
    if start >= end {
        return Vec::new();
    }
    let (_, _, marks) = d.window(d.block_at(start), d.block_at(end));
    let marks: Vec<Mark> = marks.into_iter().filter(|m| m.kind.tag() == tag && m.end > start && m.start < end).collect();
    (start..end).map(|p| marks.iter().find(|m| m.start <= p && p < m.end).map(|m| m.kind.clone())).collect()
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
        Step::Anchor(AnchorStep::Place { anchor, quote } | AnchorStep::Move { anchor, quote }) => {
            let (start, end) = match anchor.is_point() {
                true => {
                    let p = offset(&maps.map_point(&Pos::At(anchor.start)));
                    (p, p)
                }
                false => range(anchor.start, anchor.end, maps)?,
            };
            let (anchor, quote) = (Anchor { start, end, ..anchor.clone() }, quote.clone());
            Step::Anchor(match step {
                Step::Anchor(AnchorStep::Move { .. }) => AnchorStep::Move { anchor, quote },
                _ => AnchorStep::Place { anchor, quote },
            })
        }
        Step::SetBlock(_) | Step::Container(_) | Step::Anchor(AnchorStep::Remove { .. }) | Step::Island(IslandStep::Props { .. }) => {
            step.clone()
        }
        Step::Nested { .. } => unreachable!("a nested step rebases its own level"),
    })
}
