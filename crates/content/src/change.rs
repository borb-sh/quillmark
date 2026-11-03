//! The one write primitive. A [`Change`] is a list of steps, each reading the
//! value the step before it left; [`apply`] runs them all or none, holds each
//! step to what [`Content::check`] asks of what it touched and the whole
//! change to [`Content::validate`], and returns the change in its normalized
//! form. [`invert`] reads back the inverse each step gives as it applies;
//! [`compose`] and [`map_pos`] are pure helpers over changes.

use crate::draft::{Draft, HeldTwice};
use crate::ids::is_id;
use crate::lines::anchor::AnchorContext;
use crate::model::{
    Anchor, Block, Container, Content, ISLAND_SLOT, Invariant, Island, Mark, Pos, Scalar, break_offsets, check_block,
    check_island, check_mark_kind, is_text_char, mark_spans, normalized_marks,
};
pub use crate::mapping::{MapResult, Mapping};
pub use crate::rebase::{Conflict, RebaseError, rebase};
use crate::table::{self, Table};
use crate::units::{Scalars, Units, split_at};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// An edit of a value: steps applied in turn, all or none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Change {
    pub steps: Vec<Step>,
}

/// One step of a [`Change`], reading the value the step before it left.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Splice(Splice),
    SetBlock(SetBlock),
    Container(ContainerStep),
    Mark(MarkStep),
    Anchor(AnchorStep),
    Island(IslandStep),
    Nested { island: String, slot: String, change: Change },
}

/// Replaces `delete`, which must be the text at `at`, with `insert`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Splice {
    pub at: Scalar,
    pub delete: String,
    pub insert: String,
    /// The formatting the inserted text carries, offsets into `insert`.
    pub marks: Vec<Mark>,
    /// The record of each block an inserted `\n` opens, in order.
    pub blocks: Vec<Block>,
    /// The island of each inserted [`ISLAND_SLOT`], in order.
    pub islands: Vec<Island>,
    /// What the deleted text held, filled in by [`apply`].
    pub removed: Option<Removed>,
}

/// What a splice's deleted text held: the record of the block each deleted
/// `\n` opened, the marks over it as offsets into `delete`, and the island of
/// each deleted slot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Removed {
    pub blocks: Vec<Block>,
    pub marks: Vec<Mark>,
    pub islands: Vec<Island>,
}

/// Changes a block's kind, attributes or path. `attrs` without `kind` sets
/// the attributes of the kind the block holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SetBlock {
    pub id: String,
    pub kind: Option<String>,
    pub attrs: Option<Map<String, Value>>,
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerStep {
    /// Adds the container, or sets an existing one's attributes.
    Set(Container),
    Remove { id: String },
}

/// A `quote` is the text the range covered when the writer read it: a step
/// whose range covers other text now is [`ApplyError::Stale`].
#[derive(Debug, Clone, PartialEq)]
pub enum MarkStep {
    /// A keyed kind replaces whatever value the range held.
    Add { mark: Mark, quote: Option<String> },
    /// Clears every mark of the type over the range, whatever its value.
    Remove { start: Scalar, end: Scalar, tag: String, quote: Option<String> },
}

/// `quote` is as a mark step's.
#[derive(Debug, Clone, PartialEq)]
pub enum AnchorStep {
    /// Places an anchor under an id neither its text nor a slot in it holds:
    /// one held is [`ApplyError::AnchorHeld`], so two hosts minting one id
    /// collide loudly rather than one moving the other's anchor.
    Place { anchor: Anchor, quote: Option<String> },
    /// Moves the anchor its text holds under the id.
    Move { anchor: Anchor, quote: Option<String> },
    Remove { id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum IslandStep {
    /// Replaces the island's props. A slot the new props name and the island
    /// lacks takes its content from `slots`, or an empty paragraph; a slot
    /// they no longer name leaves.
    Props { id: String, props: Value, slots: BTreeMap<String, Content> },
    /// Inserts the island's slot character at `at`. `block` is the id of the
    /// block the writer read `at` in: one that holds `at` no longer is
    /// [`ApplyError::Stale`].
    Insert { at: Scalar, island: Island, block: Option<String> },
}

/// Which side of an insertion at a position the position keeps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Before,
    After,
}

/// Why a change was refused. Nothing changed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ApplyError {
    /// The step at `step` is past the text, deletes text the value does not
    /// hold there, quotes text its range no longer covers, or inserts an
    /// island into a block other than the one it names.
    Stale { step: usize },
    UnknownBlock { id: String },
    UnknownContainer { id: String },
    /// A container step that changes an existing container's kind.
    ContainerKind { id: String },
    ContainerInUse { id: String },
    UnknownAnchor { id: String },
    /// An anchor placement of an id the text or a slot in it already holds.
    AnchorHeld { id: String },
    UnknownIsland { id: String },
    UnknownSlot { island: String, slot: String },
    /// A splice whose `blocks` or `islands` do not match its insert, or a
    /// props step that changes an island's type of grid.
    BadStep { step: usize, reason: &'static str },
    Invalid(Invariant),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Stale { step } => write!(f, "step {step} does not match the value it reads"),
            ApplyError::UnknownBlock { id } => write!(f, "no block `{id}`"),
            ApplyError::UnknownContainer { id } => write!(f, "no container `{id}`"),
            ApplyError::ContainerKind { id } => write!(f, "container `{id}` cannot change its kind"),
            ApplyError::ContainerInUse { id } => write!(f, "container `{id}` is still on a block's path"),
            ApplyError::UnknownAnchor { id } => write!(f, "no anchor `{id}`"),
            ApplyError::AnchorHeld { id } => write!(f, "anchor `{id}` is held: move it, or place another id"),
            ApplyError::UnknownIsland { id } => write!(f, "no island `{id}`"),
            ApplyError::UnknownSlot { island, slot } => write!(f, "island `{island}` has no slot `{slot}`"),
            ApplyError::BadStep { step, reason } => write!(f, "step {step}: {reason}"),
            ApplyError::Invalid(inv) => write!(f, "the change leaves an invalid value: {inv}"),
        }
    }
}

impl std::error::Error for ApplyError {}

impl From<Invariant> for ApplyError {
    fn from(inv: Invariant) -> Self {
        ApplyError::Invalid(inv)
    }
}

/// A change applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub content: Content,
    /// The change in its normalized form: each splice's `removed` filled in,
    /// each mark and anchor step's `quote` and each island insert's `block`,
    /// each `setBlock` naming the fields its writer gave, a path-only one
    /// staying path-only, and a container step removing each container the
    /// change left unused.
    pub applied: Change,
    /// Each anchor the value held before and holds nowhere after, unless an
    /// `anchor` step removed it, as it read before, in reading order.
    pub dropped: Vec<Dropped>,
    /// Each surviving anchor, a point included, whose position, block or
    /// context the change moved, by id.
    pub moved: Vec<Moved>,
    /// Each placeholder the change gave a record it put, and the id
    /// [`mint::fill`](crate::mint::fill) gave it, minted past the ids `before`
    /// holds.
    pub minted: BTreeMap<String, String>,
}

/// An anchor a change dropped: its context as it read before, and the slot
/// that held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub context: AnchorContext,
    /// `None` for an anchor in the value's own text.
    pub slot: Option<SlotRef>,
}

/// One slot of an island.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRef {
    pub island: String,
    pub slot: String,
}

/// A surviving anchor a change moved: the text it covered before and after,
/// and what moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub id: String,
    pub before: String,
    pub after: String,
    pub reason: MoveReason,
}

/// What moved about an anchor, the first of these that holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoveReason {
    /// A step put it other than where the change's edits to the text carry
    /// it, or in another slot.
    Position,
    /// Its start sits in another block.
    Block,
    /// Its text, or the [`CONTEXT`] characters on either side of it, reads
    /// otherwise.
    Context,
}

impl MoveReason {
    /// The reason's name on the wire.
    pub fn name(self) -> &'static str {
        match self {
            MoveReason::Position => "position",
            MoveReason::Block => "block",
            MoveReason::Context => "context",
        }
    }
}

/// The characters on each side of an anchor its context reads, as many as
/// `reattach` holds a selector's prefix and suffix to.
pub const CONTEXT: Scalar = 8;

impl AnchorStep {
    /// The id of the anchor the step names.
    pub fn id(&self) -> &str {
        match self {
            AnchorStep::Place { anchor, .. } | AnchorStep::Move { anchor, .. } => &anchor.id,
            AnchorStep::Remove { id } => id,
        }
    }
}

impl Change {
    pub fn new(steps: Vec<Step>) -> Self {
        Change { steps }
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

impl Splice {
    pub fn new(at: Scalar, delete: impl Into<String>, insert: impl Into<String>) -> Self {
        Splice { at, delete: delete.into(), insert: insert.into(), ..Splice::default() }
    }
}

/// Run `change` on `before`, all or none: the value it leaves, the change in
/// its normalized form and what it dropped, or the first step that fails.
pub fn apply(before: &Content, change: &Change) -> Result<Applied, ApplyError> {
    Ok(applying(before, change, false)?.0)
}

/// The change that undoes `change` on the value it applies to: the inverse
/// each step gave as it applied, last first.
pub fn invert(change: &Change, before: &Content) -> Result<Change, ApplyError> {
    Ok(applying(before, change, true)?.1)
}

fn applying(before: &Content, change: &Change, undo: bool) -> Result<(Applied, Change), ApplyError> {
    let crate::mint::Filled { change, minted } = crate::mint::fill(change, &[before], None, None)?;
    let mut run = Run::new(undo);
    let mut draft = Draft::new(before.clone());
    let (mut applied, mut groups) = run.level(&mut draft, &change.steps)?;
    let mut content = draft.finish();
    let used: HashSet<&str> = content.blocks.iter().flat_map(|b| b.path.iter().map(String::as_str)).collect();
    let unused: Vec<Container> = content.containers.iter().filter(|c| !used.contains(c.id.as_str())).cloned().collect();
    for k in unused {
        applied.push(Step::Container(ContainerStep::Remove { id: k.id.clone() }));
        if undo {
            groups.push(vec![Step::Container(ContainerStep::Set(k))]);
        }
    }
    content.order_containers(true);
    content.validate()?;
    let dropped = match run.lost {
        true => {
            let removed: HashSet<&str> = run.removed.iter().map(String::as_str).collect();
            lost_anchors(before, &content, |id| !removed.contains(id))
        }
        false => Vec::new(),
    };
    let applied = Change { steps: applied };
    let moved = moved_anchors(before, &content, &applied, &run);
    let inverse = Change { steps: groups.into_iter().rev().flatten().collect() };
    Ok((Applied { content, applied, dropped, moved, minted }, inverse))
}

/// The text `change`'s top-level splices inserted that survives in `after`,
/// the text it produced, as scalar ranges into `after`. The change counts in
/// `u`; a range whose edge falls inside a character or past `after` is left
/// out.
pub fn inserted_spans(change: &Change, after: &str, u: impl Units) -> Vec<(Scalar, Scalar)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for step in &change.steps {
        let Step::Splice(s) = step else { continue };
        let (at, deleted, inserted) = (s.at, u.len(&s.delete), u.len(&s.insert));
        let end = at + deleted;
        let shift = |p: usize, inside: usize| match p {
            p if p <= at => p,
            p if p >= end => p - deleted + inserted,
            _ => inside,
        };
        spans = spans
            .into_iter()
            .map(|(a, b)| (shift(a, at + inserted), shift(b, at)))
            .filter(|(a, b)| a < b)
            .collect();
        if inserted > 0 {
            spans.push((at, at + inserted));
        }
    }
    spans.sort_unstable();
    spans
        .into_iter()
        .filter_map(|(a, b)| Some((crate::units::to_scalars(u, after, a)?, crate::units::to_scalars(u, after, b)?)))
        .collect()
}

/// Each anchor of `before`, in its text or a slot, that `after` holds
/// nowhere and `keep` admits, as it read in `before`, in reading order.
pub fn lost_anchors(before: &Content, after: &Content, keep: impl Fn(&str) -> bool) -> Vec<Dropped> {
    let mut held = HashMap::new();
    anchor_homes(after, None, &mut held);
    let mut was = HashMap::new();
    anchor_homes(before, None, &mut was);
    was.retain(|id, _| !held.contains_key(id) && keep(id));
    if was.is_empty() {
        return Vec::new();
    }
    crate::anchors::anchor_contexts(before)
        .into_iter()
        .filter_map(|context| {
            let slot = was.remove(&context.id)?;
            Some(Dropped { context, slot })
        })
        .collect()
}

/// Each anchor id of `c` and the slot that holds it.
fn anchor_homes(c: &Content, slot: Option<&SlotRef>, out: &mut HashMap<String, Option<SlotRef>>) {
    for a in &c.anchors {
        out.insert(a.id.clone(), slot.cloned());
    }
    for island in &c.islands {
        for (id, inner) in &island.slots {
            anchor_homes(inner, Some(&SlotRef { island: island.id.clone(), slot: id.clone() }), out);
        }
    }
}

/// `steps` run on `c`, a field or a slot, as [`apply`] runs them, without
/// the checks of the whole value at the end.
pub(crate) fn run(c: &mut Content, steps: &[Step]) -> Result<(), ApplyError> {
    let mut draft = Draft::new(std::mem::take(c));
    let done = Run::new(false).level(&mut draft, steps).map(|_| ());
    *c = draft.finish();
    done
}

/// A change running: what its steps report beyond the value they leave.
pub(crate) struct Run {
    /// Whether each step gives its inverse.
    undo: bool,
    /// The id of each anchor an `anchor` step removed.
    removed: Vec<String>,
    /// Each anchor whose position, block or context a step may have moved.
    touched: HashSet<String>,
    /// Each anchor an `anchor` step placed or moved.
    stepped: HashSet<String>,
    /// Whether a step may have lost an anchor other than by an `anchor` step.
    lost: bool,
}

/// The steps that undo one step.
type Undo = Vec<Step>;

const TWICE_BLOCK: &str = "the block id is held twice";
const TWICE_ISLAND: &str = "the island id is held twice";

impl Run {
    pub(crate) fn new(undo: bool) -> Run {
        Run { undo, removed: Vec::new(), touched: HashSet::new(), stepped: HashSet::new(), lost: false }
    }

    /// Runs `steps` on one level, each checked over what it touched, and
    /// answers them normalized and, where the run gives them, their inverses.
    fn level(&mut self, d: &mut Draft, steps: &[Step]) -> Result<(Vec<Step>, Vec<Undo>), ApplyError> {
        let mut applied = Vec::with_capacity(steps.len());
        let mut groups = Vec::with_capacity(if self.undo { steps.len() } else { 0 });
        for (n, step) in steps.iter().enumerate() {
            let (done, undo) = self.step(d, step, n)?;
            applied.push(done);
            if self.undo {
                groups.push(undo);
            }
        }
        Ok((applied, groups))
    }

    /// One step on `d`, the `n`th of its level.
    pub(crate) fn step(&mut self, d: &mut Draft, step: &Step, n: usize) -> Result<(Step, Undo), ApplyError> {
        Ok(match step {
            Step::Splice(s) => {
                let (done, undo) = self.splice(d, s, n)?;
                (Step::Splice(done), undo)
            }
            Step::SetBlock(s) => {
                let (done, undo) = set_block(d, s, n)?;
                (Step::SetBlock(done), vec![Step::SetBlock(undo)])
            }
            Step::Container(s) => (step.clone(), vec![container(&mut d.c, s)?]),
            Step::Mark(m) => {
                let (done, undo) = self.mark(d, m, n)?;
                (Step::Mark(done), undo)
            }
            Step::Anchor(a) => {
                let (done, undo) = self.anchor(d, a, n)?;
                (Step::Anchor(done), vec![undo])
            }
            Step::Island(IslandStep::Props { id, props, slots }) => {
                let (gained, undo) = self.island_props(&mut d.c, id, props, slots, n)?;
                let props = d.c.islands.iter().find(|i| &i.id == id).map(|i| i.props.clone()).unwrap_or_default();
                (Step::Island(IslandStep::Props { id: id.clone(), props, slots: gained }), vec![undo])
            }
            Step::Island(IslandStep::Insert { at, island, block }) => {
                let held = (*at <= d.len()).then(|| d.c.blocks[d.block_at(*at)].id.clone());
                if block.is_some() && *block != held {
                    return Err(ApplyError::Stale { step: n });
                }
                let s = Splice { islands: vec![island.clone()], ..Splice::new(*at, "", ISLAND_SLOT.to_string()) };
                let (done, undo) = self.splice(d, &s, n)?;
                let island = done.islands.into_iter().next().unwrap_or_else(|| island.clone());
                (Step::Island(IslandStep::Insert { at: *at, island, block: held }), undo)
            }
            Step::Nested { island, slot, change } => {
                let unknown = || ApplyError::UnknownSlot { island: island.clone(), slot: slot.clone() };
                let k = match island_index(&d.c, island, n) {
                    Err(ApplyError::UnknownIsland { .. }) => return Err(unknown()),
                    k => k?,
                };
                let target = d.c.islands[k].slots.get_mut(slot).ok_or_else(unknown)?;
                let mut inner = Draft::new(std::mem::take(target));
                let (steps, groups) = self.level(&mut inner, &change.steps)?;
                *target = inner.finish();
                if !target.is_slot_shape() {
                    return Err(Invariant::BadSlot { island: island.clone(), slot: slot.clone() }.into());
                }
                let nested = |steps| Step::Nested { island: island.clone(), slot: slot.clone(), change: Change { steps } };
                (nested(steps), vec![nested(groups.into_iter().rev().flatten().collect())])
            }
        })
    }

    fn splice(&mut self, d: &mut Draft, s: &Splice, n: usize) -> Result<(Splice, Undo), ApplyError> {
        let (at, dn, m) = (s.at, s.delete.chars().count(), s.insert.chars().count());
        let end = at.checked_add(dn).filter(|&end| end <= d.len()).ok_or(ApplyError::Stale { step: n })?;
        let (k0, k1) = (d.block_at(at), d.block_at(end));
        let (ws, text, marks) = d.window(k0, k1);
        let from = crate::units::char_to_byte(&text, at - ws);
        if text.get(from..from + s.delete.len()) != Some(s.delete.as_str()) {
            return Err(ApplyError::Stale { step: n });
        }
        if s.insert.chars().filter(|&ch| ch == '\n').count() != s.blocks.len() {
            return Err(ApplyError::BadStep { step: n, reason: "`blocks` holds one record per inserted `\\n`" });
        }
        if s.insert.chars().filter(|&ch| ch == ISLAND_SLOT).count() != s.islands.len() {
            return Err(ApplyError::BadStep { step: n, reason: "`islands` holds one island per inserted slot" });
        }
        if s.marks.iter().any(|mk| mk.start >= mk.end || mk.end > m) {
            return Err(ApplyError::BadStep { step: n, reason: "a mark of the insert is outside it" });
        }
        if let Some(ch) = s.insert.chars().find(|&ch| !is_text_char(ch)) {
            return Err(Invariant::ForbiddenChar(ch).into());
        }
        for b in &s.blocks {
            check_block(b, |id| d.c.container(id).map(|k| &k.kind))?;
        }
        for mk in &s.marks {
            check_mark_kind(&mk.kind)?;
        }
        let mut islands = s.islands.clone();
        for island in &mut islands {
            table::normalize_island(island);
            for slot in island.slots.values_mut() {
                slot.check_bounds()?;
                slot.normalize();
            }
            if !is_id(&island.id) {
                return Err(Invariant::BadId { id: island.id.clone() }.into());
            }
            check_island(island)?;
            for slot in island.slots.values() {
                self.touched.extend(slot.anchor_ids());
            }
        }

        let (first, lost) = (d.slots_before(at), d.slots_before(end) - d.slots_before(at));
        let held = marks.len();
        let removed_marks = clipped(&marks, at, end);
        let mut next = Vec::with_capacity(marks.len() + s.marks.len() + 1);
        for mk in marks {
            if mk.start < at {
                next.push(Mark::new(mk.start, mk.end.min(at), mk.kind.clone()));
            }
            if mk.end > end {
                next.push(Mark::new(mk.start.max(end) - dn + m, mk.end - dn + m, mk.kind));
            }
        }
        next.extend(s.marks.iter().map(|mk| Mark::new(mk.start + at, mk.end + at, mk.kind.clone())));
        let mut written = String::with_capacity(text.len() + s.insert.len());
        written.push_str(&text[..from]);
        written.push_str(&s.insert);
        written.push_str(&text[from + s.delete.len()..]);
        let untouched = s.marks.is_empty() && removed_marks.is_empty() && next.iter().all(|mk| mk.end < at || mk.start > at + m);
        if !untouched {
            within_marks(d, held, ws, &written, &next, d.len() - dn + m)?;
        }
        let next = if untouched { next } else { settled(ws, &written, next) };
        let inserted = clipped(&next, at, at + m);

        let breaks = s.insert.contains('\n') || s.delete.contains('\n');
        let (lo, hi) = match breaks {
            true => (ws.min(at.saturating_sub(CONTEXT)), d.block_end(k1).max(end + CONTEXT)),
            false => (at.saturating_sub(CONTEXT), end + CONTEXT),
        };
        let near = d.c.anchors.partition_point(|a| a.start <= hi);
        self.touched.extend(d.c.anchors[..near].iter().filter(|a| a.end >= lo).map(|a| a.id.clone()));
        let restores = self.shift_anchors(d, at, dn, m);
        let blocks = d.rewrite(k0, k1, &written, next, s.blocks.clone());
        let gone: Vec<Island> = d.c.islands.splice(first..first + lost, islands.iter().cloned()).collect();
        if gone.iter().any(|i| i.slots.values().any(|slot| !slot.anchors.is_empty())) {
            self.lost = true;
        }
        let removed = Removed { blocks, marks: removed_marks, islands: gone };
        let mut undo = Vec::new();
        if self.undo {
            undo.push(Step::Splice(Splice {
                at,
                delete: s.insert.clone(),
                insert: s.delete.clone(),
                marks: removed.marks.clone(),
                blocks: removed.blocks.clone(),
                islands: removed.islands.clone(),
                removed: Some(Removed { blocks: s.blocks.clone(), marks: inserted.clone(), islands: islands.clone() }),
            }));
            undo.extend(restores);
        }
        let done = Splice {
            at,
            delete: s.delete.clone(),
            insert: s.insert.clone(),
            marks: inserted,
            blocks: s.blocks.clone(),
            islands,
            removed: Some(removed),
        };
        Ok((done, undo))
    }

    /// Moves the anchors across a splice of `dn` characters at `at` by `m`,
    /// and answers a step putting back each one the splice's inverse would
    /// not, quoting the text it reads now.
    fn shift_anchors(&mut self, d: &mut Draft, at: Scalar, dn: Scalar, m: Scalar) -> Undo {
        let end = at + dn;
        let split = d.c.anchors.partition_point(|a| a.start <= end);
        for a in &mut d.c.anchors[split..] {
            a.start = a.start - dn + m;
            a.end = a.end - dn + m;
        }
        if d.c.anchors[..split].iter().all(|a| a.end <= at) {
            return Vec::new();
        }
        let mut restores = Vec::new();
        let mut changed = false;
        let mut kept = Vec::with_capacity(d.c.anchors.len());
        for a in d.c.anchors.drain(..split) {
            if a.end <= at {
                kept.push(a);
                continue;
            }
            let point = a.is_point();
            let (start, end_) = (map_start(a.start, at, dn, m, point), map_end(a.end, at, dn, m, point));
            let alive = point || start < end_;
            if !point && ((at < a.end && end > a.start) || (a.start < at && at < a.end)) {
                self.touched.insert(a.id.clone());
            }
            if self.undo {
                let back = (map_start(start, at, m, dn, point), map_end(end_, at, m, dn, point));
                if !alive || back != (a.start, a.end) {
                    restores.push((a.clone(), alive));
                }
            }
            if !alive {
                self.lost = true;
                changed = true;
                continue;
            }
            changed |= (start, end_) != (a.start, a.end);
            kept.push(Anchor { start, end: end_, ..a });
        }
        kept.append(&mut d.c.anchors);
        d.c.anchors = kept;
        if changed {
            d.c.sort_anchors();
        }
        restores
            .into_iter()
            .map(|(a, alive)| {
                let quote = Some(d.slice(a.start, a.end));
                Step::Anchor(match alive {
                    true => AnchorStep::Move { anchor: a, quote },
                    false => AnchorStep::Place { anchor: a, quote },
                })
            })
            .collect()
    }

    fn mark(&mut self, d: &mut Draft, s: &MarkStep, n: usize) -> Result<(MarkStep, Undo), ApplyError> {
        let (start, end, tag, quote) = match s {
            MarkStep::Add { mark, quote } => (mark.start, mark.end, mark.kind.tag(), quote),
            MarkStep::Remove { start, end, tag, quote } => (*start, *end, tag.as_str(), quote),
        };
        let held = quoted(d, start, end, quote, n)?;
        if start >= end || end > d.len() {
            return Err(ApplyError::BadStep { step: n, reason: "a mark step's range is empty or past the text" });
        }
        let (k0, k1) = (d.block_at(start), d.block_at(end));
        let (ws, text, mut marks) = d.window(k0, k1);
        let held_marks = marks.len();
        let was = if self.undo { marks.clone() } else { Vec::new() };
        let remove = matches!(s, MarkStep::Remove { .. }) || matches!(s, MarkStep::Add { mark, .. } if mark.kind.is_keyed());
        if remove {
            clear(&mut marks, start, end, tag);
        }
        if let MarkStep::Add { mark, .. } = s {
            check_mark_kind(&mark.kind)?;
            marks.push(mark.clone());
        }
        within_marks(d, held_marks, ws, &text, &marks, d.len())?;
        let marks = settled(ws, &text, marks);
        let undo = match self.undo {
            true => mark_fixups(&marks, &was).into_iter().map(|step| quoting(step, |a, b| d.slice(a, b))).collect(),
            false => Vec::new(),
        };
        d.set_marks(k0, k1, marks);
        let done = match s {
            MarkStep::Add { mark, .. } => MarkStep::Add { mark: mark.clone(), quote: held },
            MarkStep::Remove { .. } => MarkStep::Remove { start, end, tag: tag.to_string(), quote: held },
        };
        Ok((done, undo))
    }

    fn anchor(&mut self, d: &mut Draft, s: &AnchorStep, n: usize) -> Result<(AnchorStep, Step), ApplyError> {
        let back = |d: &Draft, a: Anchor, place: bool| {
            let quote = Some(d.slice(a.start, a.end));
            Step::Anchor(match place {
                true => AnchorStep::Place { anchor: a, quote },
                false => AnchorStep::Move { anchor: a, quote },
            })
        };
        match s {
            AnchorStep::Place { anchor: a, quote } | AnchorStep::Move { anchor: a, quote } => {
                let held = quoted(d, a.start, a.end, quote, n)?;
                if a.id.is_empty() || a.start > a.end || a.end > d.len() {
                    return Err(ApplyError::BadStep { step: n, reason: "an anchor step's range is past the text" });
                }
                let prior = d.c.anchors.iter().position(|h| h.id == a.id);
                let place = matches!(s, AnchorStep::Place { .. });
                let prior = match (place, prior) {
                    (true, None) if !d.c.islands.iter().any(|i| i.slots.values().any(|slot| slot.holds_anchor(&a.id))) => None,
                    (true, _) => return Err(ApplyError::AnchorHeld { id: a.id.clone() }),
                    (false, Some(k)) => Some(d.c.anchors.remove(k)),
                    (false, None) => return Err(ApplyError::UnknownAnchor { id: a.id.clone() }),
                };
                let key = |h: &Anchor| (h.start, h.end, h.id.clone());
                let at = d.c.anchors.partition_point(|h| key(h) < key(a));
                d.c.anchors.insert(at, a.clone());
                self.touched.insert(a.id.clone());
                self.stepped.insert(a.id.clone());
                let undo = match prior {
                    Some(p) => back(d, p, false),
                    None => Step::Anchor(AnchorStep::Remove { id: a.id.clone() }),
                };
                let done = match place {
                    true => AnchorStep::Place { anchor: a.clone(), quote: held },
                    false => AnchorStep::Move { anchor: a.clone(), quote: held },
                };
                Ok((done, undo))
            }
            AnchorStep::Remove { id } => {
                let at = d.c.anchors.iter().position(|h| &h.id == id).ok_or_else(|| ApplyError::UnknownAnchor { id: id.clone() })?;
                let held = d.c.anchors.remove(at);
                self.removed.push(id.clone());
                Ok((s.clone(), back(d, held, true)))
            }
        }
    }

    /// Sets an island's props, and answers the content of each slot it gained
    /// and the step that puts back its props and the slots it lost.
    fn island_props(
        &mut self,
        c: &mut Content,
        id: &str,
        props: &Value,
        given: &BTreeMap<String, Content>,
        n: usize,
    ) -> Result<(BTreeMap<String, Content>, Step), ApplyError> {
        let k = island_index(c, id, n)?;
        let island = &mut c.islands[k];
        if !props.is_object() {
            return Err(ApplyError::BadStep { step: n, reason: "`props` is not an object" });
        }
        let was = std::mem::replace(&mut island.props, props.clone());
        let order = island.slot_order();
        if island.island_type == crate::lines::island::IslandType::Table && Table::read(props).is_none() {
            return Err(ApplyError::BadStep { step: n, reason: "a table's props do not spell a grid" });
        }
        let mut gained = BTreeMap::new();
        let mut slots = BTreeMap::new();
        for slot in order {
            let content = match island.slots.remove(&slot) {
                Some(held) => held,
                None => {
                    let mut fresh = given.get(&slot).cloned().unwrap_or_default();
                    fresh.check_bounds()?;
                    fresh.normalize();
                    self.touched.extend(fresh.anchor_ids());
                    gained.insert(slot.clone(), fresh.clone());
                    fresh
                }
            };
            slots.insert(slot, content);
        }
        if let Some(k) = given.keys().find(|k| !gained.contains_key(*k)) {
            let reason = match slots.contains_key(k) {
                true => "`slots` names a slot the island already holds",
                false => "`slots` names a slot the props do not name",
            };
            return Err(ApplyError::BadStep { step: n, reason });
        }
        let lost = std::mem::replace(&mut island.slots, slots);
        if lost.values().any(|slot| !slot.anchors.is_empty()) {
            self.lost = true;
        }
        table::normalize_island(island);
        check_island(island)?;
        let undo = Step::Island(IslandStep::Props { id: id.to_string(), props: was, slots: lost });
        Ok((gained, undo))
    }
}

/// The index of the island `id` names, refused where none or two hold it.
fn island_index(c: &Content, id: &str, n: usize) -> Result<usize, ApplyError> {
    let mut held = c.islands.iter().enumerate().filter(|(_, i)| i.id == id).map(|(k, _)| k);
    match (held.next(), held.next()) {
        (Some(_), Some(_)) => Err(ApplyError::BadStep { step: n, reason: TWICE_ISLAND }),
        (Some(k), None) => Ok(k),
        (None, _) => Err(ApplyError::UnknownIsland { id: id.to_string() }),
    }
}

/// The marks over `[at, end)`, as offsets into it.
fn clipped(marks: &[Mark], at: Scalar, end: Scalar) -> Vec<Mark> {
    marks
        .iter()
        .filter_map(|mk| {
            let (lo, hi) = (mk.start.max(at), mk.end.min(end));
            (lo < hi).then(|| Mark::new(lo - at, hi - at, mk.kind.clone()))
        })
        .collect()
}

/// Refuses a step that would leave `d`, `len` characters long, past
/// [`max_marks`](crate::max_marks): its `held` marks over the window at `at`
/// replaced by `marks` over `text`, each counted once per block and line it
/// spans, as settling them can leave them.
fn within_marks(d: &Draft, held: usize, at: Scalar, text: &str, marks: &[Mark], len: Scalar) -> Result<(), Invariant> {
    let breaks = break_offsets(text);
    let spans = mark_spans(&breaks, marks.iter().map(|m| (m.start - at, m.end - at)));
    let total = (d.mark_count() - held).saturating_add(spans);
    let max = crate::max_marks(len);
    match total > max {
        true => Err(Invariant::TooManyMarks { marks: total, max }),
        false => Ok(()),
    }
}

/// `marks`, at offsets into the value, over `text`, which starts at `at`,
/// in the canonical form.
fn settled(at: Scalar, text: &str, marks: Vec<Mark>) -> Vec<Mark> {
    let chars: Vec<char> = text.chars().collect();
    let local = marks.into_iter().map(|m| Mark::new(m.start - at, m.end - at, m.kind)).collect();
    normalized_marks(&chars, local).into_iter().map(|m| Mark::new(m.start + at, m.end + at, m.kind)).collect()
}

/// A range anchor's start binds to the character after it; a point binds
/// left, and collapses to the deletion's start when its text goes.
fn map_start(p: Scalar, at: Scalar, dn: Scalar, m: Scalar, point: bool) -> Scalar {
    if point {
        return map_point(p, at, dn, m);
    }
    if p < at { p } else { (p.saturating_sub(dn)).max(at) + m }
}

/// A range anchor's end binds to the character before it.
fn map_end(p: Scalar, at: Scalar, dn: Scalar, m: Scalar, point: bool) -> Scalar {
    if point {
        return map_point(p, at, dn, m);
    }
    if p <= at {
        p
    } else if p <= at + dn {
        at
    } else {
        p - dn + m
    }
}

fn map_point(p: Scalar, at: Scalar, dn: Scalar, m: Scalar) -> Scalar {
    if p <= at {
        p
    } else if p < at + dn {
        at
    } else {
        p - dn + m
    }
}

/// Sets a block's record, and answers the step that puts it back.
fn set_block(d: &mut Draft, s: &SetBlock, n: usize) -> Result<(SetBlock, SetBlock), ApplyError> {
    let k = d
        .block_index(&s.id)
        .map_err(|HeldTwice| ApplyError::BadStep { step: n, reason: TWICE_BLOCK })?
        .ok_or_else(|| ApplyError::UnknownBlock { id: s.id.clone() })?;
    let (blocks, containers) = (&mut d.c.blocks, &d.c.containers);
    let b = &mut blocks[k];
    let undo = SetBlock {
        id: s.id.clone(),
        kind: Some(b.kind.tag().to_string()),
        attrs: crate::wire::block_attrs(&b.kind).and_then(|v| v.as_object().cloned()),
        path: Some(b.path.clone()),
    };
    if s.kind.is_some() || s.attrs.is_some() {
        let tag = s.kind.as_deref().unwrap_or(b.kind.tag());
        let mut attrs = match (&s.kind, crate::wire::block_attrs(&b.kind)) {
            (None, Some(Value::Object(held))) => held,
            _ => Map::new(),
        };
        attrs.extend(s.attrs.clone().unwrap_or_default());
        b.kind = crate::wire::block_kind_from(tag, &attrs)
            .map_err(|_| ApplyError::BadStep { step: n, reason: "`kind` and `attrs` do not spell a block kind" })?;
    }
    if let Some(path) = &s.path {
        b.path = path.clone();
    }
    check_block(b, |id| containers.iter().find(|k| k.id == id).map(|k| &k.kind))?;
    let attrs = crate::wire::block_attrs(&b.kind).and_then(|v| v.as_object().cloned());
    let kind = (s.kind.is_some() || s.attrs.is_some()).then(|| b.kind.tag().to_string());
    Ok((SetBlock { id: s.id.clone(), attrs: kind.as_ref().and(attrs), kind, path: s.path.clone() }, undo))
}

/// Runs a container step, and answers the step that undoes it.
fn container(c: &mut Content, s: &ContainerStep) -> Result<Step, ApplyError> {
    Ok(match s {
        ContainerStep::Set(k) => {
            let undo = match c.containers.iter_mut().find(|h| h.id == k.id) {
                Some(held) if !held.kind.same_kind(&k.kind) => return Err(ApplyError::ContainerKind { id: k.id.clone() }),
                Some(held) => ContainerStep::Set(Container { id: k.id.clone(), kind: std::mem::replace(&mut held.kind, k.kind.clone()) }),
                None => {
                    c.containers.push(k.clone());
                    ContainerStep::Remove { id: k.id.clone() }
                }
            };
            if !is_id(&k.id) {
                return Err(Invariant::BadId { id: k.id.clone() }.into());
            }
            Step::Container(undo)
        }
        ContainerStep::Remove { id } => {
            if c.blocks.iter().any(|b| b.path.contains(id)) {
                return Err(ApplyError::ContainerInUse { id: id.clone() });
            }
            let at = c
                .containers
                .iter()
                .position(|h| &h.id == id)
                .ok_or_else(|| ApplyError::UnknownContainer { id: id.clone() })?;
            Step::Container(ContainerStep::Set(c.containers.remove(at)))
        }
    })
}

/// The text over `[start, end)`, held to `quote` where the step gives one:
/// `None` where the range is past the text and no quote is given.
fn quoted(d: &Draft, start: Scalar, end: Scalar, quote: &Option<String>, n: usize) -> Result<Option<String>, ApplyError> {
    let held = (start <= end && end <= d.len()).then(|| d.slice(start, end));
    match quote {
        Some(q) if held.as_ref() != Some(q) => Err(ApplyError::Stale { step: n }),
        _ => Ok(held),
    }
}

fn clear(marks: &mut Vec<Mark>, start: Scalar, end: Scalar, tag: &str) {
    let mut out = Vec::with_capacity(marks.len() + 1);
    for mk in marks.drain(..) {
        if mk.kind.tag() != tag || mk.end <= start || mk.start >= end {
            out.push(mk);
            continue;
        }
        if mk.start < start {
            out.push(Mark::new(mk.start, start, mk.kind.clone()));
        }
        if mk.end > end {
            out.push(Mark::new(end, mk.end, mk.kind));
        }
    }
    *marks = out;
}

/// `step` with its quote filled in from `over`, the text it reads.
fn quoting(step: Step, over: impl Fn(Scalar, Scalar) -> String) -> Step {
    match step {
        Step::Mark(MarkStep::Add { mark, quote: None }) => {
            let quote = Some(over(mark.start, mark.end));
            Step::Mark(MarkStep::Add { mark, quote })
        }
        Step::Mark(MarkStep::Remove { start, end, tag, quote: None }) => {
            Step::Mark(MarkStep::Remove { start, end, tag, quote: Some(over(start, end)) })
        }
        step => step,
    }
}

/// The mark steps that take the marks `now` to `want`, both sorted, per
/// type: over each run of marks of it the two do not share, the type cleared
/// and `want`'s marks of it there added back.
pub(crate) fn mark_fixups(now: &[Mark], want: &[Mark]) -> Vec<Step> {
    let mut out = Vec::new();
    let tags: BTreeSet<&str> = now.iter().chain(want).map(|m| m.kind.tag()).collect();
    for tag in tags {
        let have: Vec<&Mark> = now.iter().filter(|m| m.kind.tag() == tag).collect();
        let need: Vec<&Mark> = want.iter().filter(|m| m.kind.tag() == tag).collect();
        let mut k = 0;
        for (lo, hi) in differing(&have, &need) {
            out.push(Step::Mark(MarkStep::Remove { start: lo, end: hi, tag: tag.to_string(), quote: None }));
            k += need[k..].iter().take_while(|m| m.end <= lo).count();
            for m in need[k..].iter().take_while(|m| m.start < hi) {
                let (s, e) = (m.start.max(lo), m.end.min(hi));
                if s < e {
                    out.push(Step::Mark(MarkStep::Add { mark: Mark::new(s, e, m.kind.clone()), quote: None }));
                }
            }
        }
    }
    out
}

/// The spans over the marks one sorted list holds and the other does not,
/// those that overlap or touch joined.
fn differing(a: &[&Mark], b: &[&Mark]) -> Vec<(Scalar, Scalar)> {
    let mut gone: Vec<(Scalar, Scalar)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let at = match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) => (x.start, x.end).min((y.start, y.end)),
            (Some(x), None) => (x.start, x.end),
            (None, Some(y)) => (y.start, y.end),
            (None, None) => break,
        };
        let i1 = i + a[i..].iter().take_while(|m| (m.start, m.end) == at).count();
        let j1 = j + b[j..].iter().take_while(|m| (m.start, m.end) == at).count();
        let differs = a[i..i1].iter().any(|m| !b[j..j1].contains(m)) || b[j..j1].iter().any(|m| !a[i..i1].contains(m));
        if differs {
            match gone.last_mut() {
                Some(last) if at.0 <= last.1 => last.1 = last.1.max(at.1),
                _ => gone.push(at),
            }
        }
        (i, j) = (i1, j1);
    }
    gone
}

/// Each anchor `run` touched that the change kept and moved, by id: one an
/// `anchor` step put other than where the change's edits carry it, then one
/// whose start sits in another block, then one whose text or context reads
/// otherwise.
fn moved_anchors(before: &Content, after: &Content, applied: &Change, run: &Run) -> Vec<Moved> {
    if run.touched.is_empty() {
        return Vec::new();
    }
    let mut was = HashMap::new();
    sites(before, &run.touched, &[], &mut was);
    let mut now = HashMap::new();
    sites(after, &run.touched, &[], &mut now);
    let maps = match run.stepped.is_empty() {
        true => None,
        false => Mapping::from_change(applied).ok(),
    };
    let mut out: Vec<Moved> = now
        .into_iter()
        .filter_map(|(id, n)| {
            let w = was.remove(&id)?;
            let placed = || match &maps {
                _ if w.path != n.path => true,
                Some(maps) => carried(maps, &w) != Some((n.start, n.end)),
                None => true,
            };
            let reason = if run.stepped.contains(&id) && placed() {
                MoveReason::Position
            } else if w.block != n.block {
                MoveReason::Block
            } else if (&w.prefix, &w.quote, &w.suffix) != (&n.prefix, &n.quote, &n.suffix) {
                MoveReason::Context
            } else {
                return None;
            };
            Some(Moved { id, before: w.quote, after: n.quote, reason })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Where an anchor read in `site` lands across `maps`, as `apply` moves it.
fn carried(maps: &Mapping, site: &Site) -> Option<(Scalar, Scalar)> {
    let at = |p: Scalar| {
        site.path.iter().rev().fold(Pos::At(p), |inner, (island, slot)| Pos::In {
            island: island.clone(),
            slot: slot.clone(),
            at: Box::new(inner),
        })
    };
    let local = |r: MapResult| {
        let mut p = r.pos;
        loop {
            match p {
                Pos::At(p) => return p,
                Pos::In { at, .. } => p = *at,
            }
        }
    };
    Some(match site.start == site.end {
        true => {
            let p = local(maps.map_point(&at(site.start)));
            (p, p)
        }
        false => (local(maps.map(&at(site.start), Assoc::After)), local(maps.map(&at(site.end), Assoc::Before))),
    })
}

/// An anchor as one value holds it: the slots it sits in, its range, the id
/// of the block its start sits in, and its text and context.
struct Site {
    path: Vec<(String, String)>,
    start: Scalar,
    end: Scalar,
    block: String,
    prefix: String,
    quote: String,
    suffix: String,
}

/// The site of each anchor `ids` names, in `c`, at `path`, and its slots.
fn sites(c: &Content, ids: &HashSet<String>, path: &[(String, String)], out: &mut HashMap<String, Site>) {
    let want: Vec<&Anchor> = c.anchors.iter().filter(|a| ids.contains(&a.id)).collect();
    if !want.is_empty() {
        let mut edges: Vec<Scalar> = want.iter().flat_map(|a| [a.start.saturating_sub(CONTEXT), a.start, a.end, a.end + CONTEXT]).collect();
        edges.sort_unstable();
        edges.dedup();
        let mut at = HashMap::with_capacity(edges.len());
        let mut next = edges.iter().peekable();
        let mut breaks = 0;
        for (k, (b, ch)) in c.text.char_indices().enumerate() {
            while next.next_if(|&&e| e == k).is_some() {
                at.insert(k, (b, breaks));
            }
            if next.peek().is_none() {
                break;
            }
            breaks += usize::from(ch == '\n');
        }
        let end = (c.text.len(), breaks);
        let read = |p: Scalar| *at.get(&p).unwrap_or(&end);
        for a in want {
            let (from, (s, k), (e, _), to) = (read(a.start.saturating_sub(CONTEXT)), read(a.start), read(a.end), read(a.end + CONTEXT));
            out.insert(
                a.id.clone(),
                Site {
                    path: path.to_vec(),
                    start: a.start,
                    end: a.end,
                    block: c.blocks.get(k).map(|b| b.id.clone()).unwrap_or_default(),
                    prefix: c.text[from.0..s].to_string(),
                    quote: c.text[s..e].to_string(),
                    suffix: c.text[e..to.0].to_string(),
                },
            );
        }
    }
    for island in &c.islands {
        for (slot, inner) in &island.slots {
            let mut deeper = path.to_vec();
            deeper.push((island.id.clone(), slot.clone()));
            sites(inner, ids, &deeper, out);
        }
    }
}

/// `a` then `b` as one change: the steps of both, adjacent splices merged
/// where the second lies within the text the first inserted or both only
/// delete and touch, a merge that undoes itself dropped, and adjacent nested
/// changes to one slot composed. A splice typed where the one before it
/// deleted stays apart, since a point anchor there stays left of the first
/// and would sit right of the merged one.
pub fn compose(a: &Change, b: &Change) -> Change {
    compose_in(a, b, Scalars)
}

/// [`compose`] over changes whose offsets count `u`.
pub fn compose_in(a: &Change, b: &Change, u: impl Units) -> Change {
    let mut steps: Vec<Step> = Vec::with_capacity(a.steps.len() + b.steps.len());
    for step in a.steps.iter().chain(&b.steps) {
        let merged = match (steps.last(), step) {
            (Some(Step::Splice(x)), Step::Splice(y)) => merge_splices(x, y, u).map(Step::Splice),
            (
                Some(Step::Nested { island: i, slot: s, change: x }),
                Step::Nested { island: j, slot: t, change: y },
            ) if i == j && s == t => {
                Some(Step::Nested { island: i.clone(), slot: s.clone(), change: compose_in(x, y, u) })
            }
            _ => None,
        };
        match merged {
            Some(Step::Splice(m)) if m.delete.is_empty() && m.insert.is_empty() => {
                steps.pop();
            }
            Some(m) => *steps.last_mut().expect("merged with the last step") = m,
            None => steps.push(step.clone()),
        }
    }
    Change { steps }
}

fn merge_splices(x: &Splice, y: &Splice, u: impl Units) -> Option<Splice> {
    let (xm, yn) = (u.len(&x.insert), u.len(&y.delete));
    let point_moves = !x.delete.is_empty() && !y.insert.is_empty() && y.at == x.at + xm;
    if y.at >= x.at && y.at + yn <= x.at + xm && !point_moves {
        let r = y.at - x.at;
        let (head, rest) = split_at(u, &x.insert, r)?;
        let (gone, tail) = split_at(u, rest, yn)?;
        if gone != y.delete {
            return None;
        }
        let insert = format!("{head}{}{tail}", y.insert);
        let ym = u.len(&y.insert);
        let count = |s: &str, ch: char| s.chars().filter(|&c| c == ch).count();
        let (nl_before, nl_gone) = (count(head, '\n'), count(gone, '\n'));
        let (sl_before, sl_gone) = (count(head, ISLAND_SLOT), count(gone, ISLAND_SLOT));
        let mut blocks = x.blocks[..nl_before].to_vec();
        blocks.extend(y.blocks.iter().cloned());
        blocks.extend(x.blocks[nl_before + nl_gone..].iter().cloned());
        let mut islands = x.islands[..sl_before].to_vec();
        islands.extend(y.islands.iter().cloned());
        islands.extend(x.islands[sl_before + sl_gone..].iter().cloned());
        let mut marks = Vec::new();
        for mk in &x.marks {
            if mk.start < r {
                marks.push(Mark::new(mk.start, mk.end.min(r), mk.kind.clone()));
            }
            if mk.end > r + yn {
                marks.push(Mark::new(mk.start.max(r + yn) - yn + ym, mk.end - yn + ym, mk.kind.clone()));
            }
        }
        marks.extend(y.marks.iter().map(|mk| Mark::new(mk.start + r, mk.end + r, mk.kind.clone())));
        return Some(Splice { at: x.at, delete: x.delete.clone(), insert, marks, blocks, islands, removed: x.removed.clone() });
    }
    if !x.insert.is_empty() || !y.insert.is_empty() || x.delete.is_empty() || y.delete.is_empty() {
        return None;
    }
    let (first, second, at) = if y.at + yn == x.at {
        (y, x, y.at)
    } else if y.at == x.at {
        (x, y, x.at)
    } else {
        return None;
    };
    let removed = match (&first.removed, &second.removed) {
        (Some(f), Some(s)) => {
            let shift = u.len(&first.delete);
            let mut r = f.clone();
            r.blocks.extend(s.blocks.iter().cloned());
            r.islands.extend(s.islands.iter().cloned());
            r.marks.extend(s.marks.iter().map(|m| Mark::new(m.start + shift, m.end + shift, m.kind.clone())));
            Some(r)
        }
        _ => None,
    };
    Some(Splice { at, delete: format!("{}{}", first.delete, second.delete), removed, ..Splice::default() })
}

/// Where `pos` lands after `change`, and what around it the change deleted:
/// [`Mapping`] over the change's steps. A splice that deletes an island names
/// it in `removed`, as the normalized change does; one that names none is
/// refused, since which island went is not in the step.
pub fn map_pos(change: &Change, pos: &Pos, assoc: Assoc) -> Result<MapResult, ApplyError> {
    map_pos_in(change, pos, assoc, Scalars)
}

/// [`map_pos`] over a change and a position whose offsets count `u`.
pub fn map_pos_in(change: &Change, pos: &Pos, assoc: Assoc, u: impl Units) -> Result<MapResult, ApplyError> {
    Ok(Mapping::from_change_in(change, u)?.map(pos, assoc))
}

mod wire {
    //! A change's JSON: `{ "steps": [...] }`, each step an object of one key
    //! naming it, positions in Unicode scalar values.

    use super::*;
    use crate::wire::{
        ParseError, anchor_from_value, anchor_value, array, attrs, block_from_value, block_value, container_kind_from,
        check_block_keys, check_container_attrs, check_island_keys, check_keys, check_mark_attrs, check_mark_keys, container_value,
        content_value, decode, island_from_value, only, island_value, mark_from_value, mark_kind_from,
        mark_value, obj, shape, str_key, usize_key,
    };

    const MARK_TAGS: [&str; 7] = ["strong", "emph", "underline", "strike", "code", "link", "font_family"];

    impl Change {
        pub fn to_value(&self) -> Value {
            crate::lines::model::sort_keys_owned(obj([("steps", self.steps.iter().map(step_value).collect())]))
        }

        pub fn to_json(&self) -> String {
            serde_json::to_string(&self.to_value()).unwrap_or_default()
        }

        /// Decodes the shape; what the steps mean is [`apply`]'s to check.
        pub fn from_value(v: &Value) -> Result<Change, ParseError> {
            Change::from_value_in(v, Scalars)
        }

        /// [`from_value`](Self::from_value) with offsets in `u`, as a binding's
        /// host writes them; [`crate::units::change_to_scalars`] reads it.
        pub fn from_value_in(v: &Value, u: impl Units) -> Result<Change, ParseError> {
            if crate::model::json_too_deep(v) {
                return Err(ParseError::Invalid(Invariant::JsonTooDeep { what: "change", max: crate::MAX_JSON_DEPTH }));
            }
            change_from(v, u)
        }

        pub fn from_json(s: &str) -> Result<Change, ParseError> {
            let v: Value = serde_json::from_str(s).map_err(|e| ParseError::Json(e.to_string()))?;
            Change::from_value(&v)
        }
    }

    fn change_from(v: &Value, u: impl Units) -> Result<Change, ParseError> {
        only(v, "a change", &["steps"])?;
        match v.get("steps") {
            Some(Value::Array(a)) => Ok(Change { steps: a.iter().map(|s| step_from(s, u)).collect::<Result<_, _>>()? }),
            _ => Err(shape("a change's `steps` is not an array")),
        }
    }

    fn one(name: &str, body: Value) -> Value {
        let mut m = Map::new();
        m.insert(name.to_string(), body);
        Value::Object(m)
    }

    pub(crate) fn step_value(s: &Step) -> Value {
        match s {
            Step::Splice(s) => {
                let mut m = Map::new();
                m.insert("at".into(), s.at.into());
                m.insert("delete".into(), s.delete.clone().into());
                m.insert("insert".into(), s.insert.clone().into());
                if !s.marks.is_empty() {
                    m.insert("marks".into(), s.marks.iter().map(mark_value).collect());
                }
                if !s.blocks.is_empty() {
                    m.insert("blocks".into(), s.blocks.iter().map(block_value).collect());
                }
                if !s.islands.is_empty() {
                    m.insert("islands".into(), s.islands.iter().map(island_value).collect());
                }
                if let Some(r) = &s.removed {
                    let mut rm = Map::new();
                    if !r.blocks.is_empty() {
                        rm.insert("blocks".into(), r.blocks.iter().map(block_value).collect());
                    }
                    if !r.marks.is_empty() {
                        rm.insert("marks".into(), r.marks.iter().map(mark_value).collect());
                    }
                    if !r.islands.is_empty() {
                        rm.insert("islands".into(), r.islands.iter().map(island_value).collect());
                    }
                    m.insert("removed".into(), Value::Object(rm));
                }
                one("splice", Value::Object(m))
            }
            Step::SetBlock(s) => {
                let mut m = Map::new();
                m.insert("id".into(), s.id.clone().into());
                if let Some(k) = &s.kind {
                    m.insert("kind".into(), k.clone().into());
                }
                if let Some(a) = &s.attrs {
                    m.insert("attrs".into(), Value::Object(a.clone()));
                }
                if let Some(p) = &s.path {
                    m.insert("path".into(), p.iter().cloned().map(Value::String).collect());
                }
                one("setBlock", Value::Object(m))
            }
            Step::Container(ContainerStep::Set(k)) => one("container", container_value(k)),
            Step::Container(ContainerStep::Remove { id }) => {
                one("container", obj([("id", id.as_str().into()), ("remove", true.into())]))
            }
            Step::Mark(MarkStep::Add { mark, quote }) => {
                let mut v = mark_value(mark);
                v["add"] = true.into();
                one("mark", quoting(v, quote))
            }
            Step::Mark(MarkStep::Remove { start, end, tag, quote }) => one(
                "mark",
                quoting(
                    obj([("start", (*start).into()), ("end", (*end).into()), ("type", tag.as_str().into()), ("add", false.into())]),
                    quote,
                ),
            ),
            Step::Anchor(AnchorStep::Place { anchor, quote }) => one("anchor", quoting(anchor_value(anchor), quote)),
            Step::Anchor(AnchorStep::Move { anchor, quote }) => {
                let mut v = quoting(anchor_value(anchor), quote);
                v["move"] = true.into();
                one("anchor", v)
            }
            Step::Anchor(AnchorStep::Remove { id }) => {
                one("anchor", obj([("id", id.as_str().into()), ("remove", true.into())]))
            }
            Step::Island(IslandStep::Props { id, props, slots }) => {
                let mut m = Map::new();
                m.insert("id".into(), id.clone().into());
                m.insert("props".into(), props.clone());
                if !slots.is_empty() {
                    m.insert("slots".into(), Value::Object(slots.iter().map(|(k, v)| (k.clone(), content_value(v))).collect()));
                }
                one("island", Value::Object(m))
            }
            Step::Island(IslandStep::Insert { at, island, block }) => {
                let mut v = obj([("at", (*at).into()), ("island", island_value(island))]);
                if let Some(b) = block {
                    v["block"] = b.as_str().into();
                }
                one("island", v)
            }
            Step::Nested { island, slot, change } => one(
                "nested",
                obj([
                    ("island", island.as_str().into()),
                    ("slot", slot.as_str().into()),
                    ("change", obj([("steps", change.steps.iter().map(step_value).collect())])),
                ]),
            ),
        }
    }

    fn quoting(mut v: Value, quote: &Option<String>) -> Value {
        if let Some(q) = quote {
            v["quote"] = q.as_str().into();
        }
        v
    }

    fn opt_str(v: &Value, key: &str) -> Result<Option<String>, ParseError> {
        match v.get(key) {
            None | Some(Value::Null) => Ok(None),
            _ => str_key(v, key).map(Some),
        }
    }

    fn removing(v: &Value) -> Result<bool, ParseError> {
        flag(v, "remove")
    }

    fn flag(v: &Value, key: &str) -> Result<bool, ParseError> {
        match v.get(key) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(false),
            Some(Value::Bool(true)) => Ok(true),
            Some(_) => Err(shape(format!("`{key}` is not a boolean"))),
        }
    }

    fn slots_from(v: &Value) -> Result<BTreeMap<String, Content>, ParseError> {
        match v.get("slots") {
            None | Some(Value::Null) => Ok(BTreeMap::new()),
            Some(Value::Object(m)) => m
                .iter()
                .map(|(k, s)| {
                    check_keys(s)?;
                    Ok((k.clone(), decode(s)?))
                })
                .collect(),
            Some(_) => Err(shape("`slots` is not an object")),
        }
    }


    /// Refuses a key a splice's `removed` records do not define.
    fn records(r: &Value) -> Result<(), ParseError> {
        for b in array(r, "blocks")? {
            check_block_keys(b)?;
        }
        for m in array(r, "marks")? {
            check_mark_keys(m)?;
        }
        for i in array(r, "islands")? {
            check_island_keys(i)?;
        }
        Ok(())
    }

    fn step_from(v: &Value, u: impl Units) -> Result<Step, ParseError> {
        let Some(m) = v.as_object().filter(|m| m.len() == 1) else {
            return Err(shape("a step is an object of one key"));
        };
        let (name, body) = m.iter().next().expect("one key");
        if !body.is_object() {
            return Err(shape(format!("`{name}` is not an object")));
        }
        Ok(match name.as_str() {
            "splice" => {
                only(body, "a splice", &["at", "delete", "insert", "marks", "blocks", "islands", "removed"])?;
                let removed = match body.get("removed") {
                    None | Some(Value::Null) => None,
                    Some(r @ Value::Object(_)) => {
                        only(r, "a splice's `removed`", &["blocks", "marks", "islands"])?;
                        records(r)?;
                        Some(Removed {
                            blocks: array(r, "blocks")?.iter().map(block_from_value).collect::<Result<_, _>>()?,
                            marks: array(r, "marks")?.iter().map(mark_from_value).collect::<Result<_, _>>()?,
                            islands: array(r, "islands")?.iter().map(island_from_value).collect::<Result<_, _>>()?,
                        })
                    }
                    Some(_) => return Err(shape("`removed` is not an object")),
                };
                for b in array(body, "blocks")? {
                    check_block_keys(b)?;
                }
                for i in array(body, "islands")? {
                    check_island_keys(i)?;
                }
                let insert = str_key(body, "insert")?;
                let len = u.len(&insert);
                let marks = array(body, "marks")?
                    .iter()
                    .map(|mv| {
                        only(mv, "a splice's mark", &["start", "end", "type", "attrs"])?;
                        check_mark_attrs(mv)?;
                        let kind = mark_kind_from(&str_key(mv, "type")?, &attrs(mv)?)?;
                        let start = if mv.get("start").is_some() { usize_key(mv, "start")? } else { 0 };
                        let end = if mv.get("end").is_some() { usize_key(mv, "end")? } else { len };
                        Ok(Mark::new(start, end, kind))
                    })
                    .collect::<Result<_, ParseError>>()?;
                Step::Splice(Splice {
                    at: usize_key(body, "at")?,
                    delete: match body.get("delete") {
                        None => String::new(),
                        _ => str_key(body, "delete")?,
                    },
                    insert,
                    marks,
                    blocks: array(body, "blocks")?.iter().map(block_from_value).collect::<Result<_, _>>()?,
                    islands: array(body, "islands")?.iter().map(island_from_value).collect::<Result<_, _>>()?,
                    removed,
                })
            }
            "setBlock" => {
                only(body, "a setBlock", &["id", "kind", "attrs", "path"])?;
                Step::SetBlock(SetBlock {
                    id: str_key(body, "id")?,
                    kind: match body.get("kind") {
                        None | Some(Value::Null) => None,
                        _ => Some(str_key(body, "kind")?),
                    },
                    attrs: match body.get("attrs") {
                        None | Some(Value::Null) => None,
                        Some(Value::Object(a)) => Some(a.clone()),
                        Some(_) => return Err(shape("`attrs` is not an object")),
                    },
                    path: match body.get("path") {
                        None | Some(Value::Null) => None,
                        Some(Value::Array(a)) => Some(
                            a.iter()
                                .map(|p| p.as_str().map(str::to_string).ok_or_else(|| shape("a path entry is not a string")))
                                .collect::<Result<_, _>>()?,
                        ),
                        Some(_) => return Err(shape("`path` is not an array")),
                    },
                })
            }
            "container" => {
                only(body, "a container step", &["id", "kind", "attrs", "remove"])?;
                let id = str_key(body, "id")?;
                if removing(body)? {
                    Step::Container(ContainerStep::Remove { id })
                } else {
                    check_container_attrs(body)?;
                    let kind = container_kind_from(&str_key(body, "kind")?, &attrs(body)?)?;
                    Step::Container(ContainerStep::Set(Container { id, kind }))
                }
            }
            "mark" => {
                only(body, "a mark step", &["start", "end", "type", "attrs", "add", "quote"])?;
                let add = body.get("add").and_then(Value::as_bool).ok_or_else(|| shape("`add` is not a boolean"))?;
                let quote = opt_str(body, "quote")?;
                if add {
                    check_mark_attrs(body)?;
                    Step::Mark(MarkStep::Add { mark: mark_from_value(body)?, quote })
                } else {
                    let tag = str_key(body, "type")?;
                    if !MARK_TAGS.contains(&tag.as_str()) && !crate::ids::is_ext_name(&tag) {
                        return Err(ParseError::UnknownName { axis: "mark type", name: tag });
                    }
                    Step::Mark(MarkStep::Remove { start: usize_key(body, "start")?, end: usize_key(body, "end")?, tag, quote })
                }
            }
            "anchor" => {
                only(body, "an anchor step", &["id", "start", "end", "quote", "remove", "move"])?;
                match (removing(body)?, flag(body, "move")?) {
                    (true, true) => return Err(shape("an anchor step names `remove` or `move`, not both")),
                    (true, false) => Step::Anchor(AnchorStep::Remove { id: str_key(body, "id")? }),
                    (false, true) => Step::Anchor(AnchorStep::Move { anchor: anchor_from_value(body)?, quote: opt_str(body, "quote")? }),
                    (false, false) => Step::Anchor(AnchorStep::Place { anchor: anchor_from_value(body)?, quote: opt_str(body, "quote")? }),
                }
            }
            "island" => match body.get("island") {
                Some(island) => {
                    only(body, "an island insert", &["at", "island", "block"])?;
                    check_island_keys(island)?;
                    Step::Island(IslandStep::Insert {
                        at: usize_key(body, "at")?,
                        island: island_from_value(island)?,
                        block: opt_str(body, "block")?,
                    })
                }
                None => {
                    only(body, "an island props step", &["id", "props", "slots"])?;
                    Step::Island(IslandStep::Props {
                        id: str_key(body, "id")?,
                        props: body.get("props").cloned().ok_or_else(|| shape("an island step names `props`"))?,
                        slots: slots_from(body)?,
                    })
                }
            },
            "nested" => {
                only(body, "a nested step", &["island", "slot", "change"])?;
                Step::Nested {
                    island: str_key(body, "island")?,
                    slot: str_key(body, "slot")?,
                    change: change_from(body.get("change").ok_or_else(|| shape("a nested step names `change`"))?, u)?,
                }
            }
            other => return Err(ParseError::UnknownName { axis: "step", name: other.to_string() }),
        })
    }
}
