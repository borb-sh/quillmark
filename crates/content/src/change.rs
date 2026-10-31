//! The one write primitive. A [`Change`] is a list of steps, each reading the
//! value the step before it left; [`apply`] runs them all or none, holds every
//! step to [`Content::check`] and the whole change to [`Content::validate`],
//! and returns the change in its normalized form. [`invert`], [`compose`] and
//! [`map_pos`] are pure helpers over changes.

use crate::lines::anchor::{AnchorContext, anchor_context};
use crate::migrate::to_lines;
use crate::model::{
    Anchor, Block, Container, Content, ISLAND_SLOT, Invariant, Island, Mark, Pos, Usv,
};
use crate::table::{self, Table};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Change {
    pub steps: Vec<Step>,
}

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
    pub at: Usv,
    pub delete: String,
    pub insert: String,
    /// The formatting the inserted text carries, offsets into `insert`.
    pub marks: Vec<Mark>,
    /// The record of each segment an inserted `\n` ends, in order.
    pub blocks: Vec<Block>,
    /// The island of each inserted [`ISLAND_SLOT`], in order.
    pub islands: Vec<Island>,
    /// What the deleted text held, filled in by [`apply`].
    pub removed: Option<Removed>,
}

/// What a splice's deleted text held: the record of each deleted `\n`, the
/// marks over it as offsets into `delete`, and the island of each deleted
/// slot.
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

#[derive(Debug, Clone, PartialEq)]
pub enum MarkStep {
    /// A keyed kind replaces whatever value the range held.
    Add(Mark),
    /// Clears every mark of the type over the range, whatever its value.
    Remove { start: Usv, end: Usv, tag: &'static str },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnchorStep {
    Set(Anchor),
    Remove { id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum IslandStep {
    /// Replaces the island's props. A slot the new props name and the island
    /// lacks takes its content from `slots`, or an empty paragraph; a slot
    /// they no longer name leaves.
    Props { id: String, props: Value, slots: BTreeMap<String, Content> },
    /// Inserts the island's slot character at `at`.
    Insert { at: Usv, island: Island },
}

/// Which side of an insertion at a position the position keeps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Before,
    After,
}

/// Why a change was refused. Nothing changed.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplyError {
    /// The step at `step` is past the text, or deletes text the value does
    /// not hold there.
    Stale { step: usize },
    UnknownBlock { id: String },
    UnknownContainer { id: String },
    /// A container step that changes an existing container's kind.
    ContainerKind { id: String },
    ContainerInUse { id: String },
    UnknownAnchor { id: String },
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
    /// each `setBlock` naming its kind and attributes, and a container step
    /// removing each container the change left unused.
    pub applied: Change,
    /// Each range anchor whose text the change deleted, as it read before.
    pub dropped: Vec<AnchorContext>,
    /// Each surviving range anchor whose text the change rewrote.
    pub moved: Vec<Moved>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub id: String,
    pub before: String,
    pub after: String,
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
    pub fn new(at: Usv, delete: impl Into<String>, insert: impl Into<String>) -> Self {
        Splice { at, delete: delete.into(), insert: insert.into(), ..Splice::default() }
    }
}

pub fn apply(before: &Content, change: &Change) -> Result<Applied, ApplyError> {
    let mut content = before.clone();
    let mut applied = Vec::with_capacity(change.steps.len());
    let mut dropped = Vec::new();
    run(&mut content, &change.steps, &mut applied, &mut dropped)?;
    let used: BTreeSet<&str> = content.blocks.iter().flat_map(|b| b.path.iter().map(String::as_str)).collect();
    let unused: Vec<String> =
        content.containers.iter().filter(|c| !used.contains(c.id.as_str())).map(|c| c.id.clone()).collect();
    for id in unused {
        applied.push(Step::Container(ContainerStep::Remove { id }));
    }
    content.order_containers(true);
    content.validate()?;
    let lowered = to_lines(before);
    let dropped = dropped.iter().filter_map(|id| anchor_context(&lowered, id)).collect();
    let moved = moved_anchors(before, &content);
    Ok(Applied { content, applied: Change { steps: applied }, dropped, moved })
}

/// Runs `steps` on one level, a field or a slot, each held to `check`.
pub(crate) fn run(c: &mut Content, steps: &[Step], applied: &mut Vec<Step>, dropped: &mut Vec<String>) -> Result<(), ApplyError> {
    for (n, step) in steps.iter().enumerate() {
        let done = apply_step(c, step, n, dropped)?;
        c.check()?;
        applied.push(done);
    }
    Ok(())
}

fn apply_step(c: &mut Content, step: &Step, n: usize, dropped: &mut Vec<String>) -> Result<Step, ApplyError> {
    Ok(match step {
        Step::Splice(s) => Step::Splice(splice(c, s, n, dropped)?),
        Step::SetBlock(s) => Step::SetBlock(set_block(c, s, n)?),
        Step::Container(s) => {
            container(c, s)?;
            step.clone()
        }
        Step::Mark(m) => {
            mark(c, m, n)?;
            step.clone()
        }
        Step::Anchor(a) => {
            anchor(c, a, n)?;
            step.clone()
        }
        Step::Island(IslandStep::Props { id, props, slots }) => {
            let slots = island_props(c, id, props, slots, n)?;
            let props = c.island(id).map(|i| i.props.clone()).unwrap_or_default();
            Step::Island(IslandStep::Props { id: id.clone(), props, slots })
        }
        Step::Island(IslandStep::Insert { at, island }) => {
            let s = Splice { islands: vec![island.clone()], ..Splice::new(*at, "", ISLAND_SLOT.to_string()) };
            let done = splice(c, &s, n, dropped)?;
            let island = done.islands.into_iter().next().unwrap_or_else(|| island.clone());
            Step::Island(IslandStep::Insert { at: *at, island })
        }
        Step::Nested { island, slot, change } => {
            let unknown = || ApplyError::UnknownSlot { island: island.clone(), slot: slot.clone() };
            let target = c.slot_mut(island, slot).ok_or_else(unknown)?;
            let mut inner = Vec::new();
            run(target, &change.steps, &mut inner, dropped)?;
            if !target.is_slot_shape() {
                return Err(Invariant::BadSlot { island: island.clone(), slot: slot.clone() }.into());
            }
            Step::Nested { island: island.clone(), slot: slot.clone(), change: Change { steps: inner } }
        }
    })
}

fn splice(c: &mut Content, s: &Splice, n: usize, dropped: &mut Vec<String>) -> Result<Splice, ApplyError> {
    let chars: Vec<char> = c.text.chars().collect();
    let del: Vec<char> = s.delete.chars().collect();
    let ins: Vec<char> = s.insert.chars().collect();
    let (at, dn, m) = (s.at, del.len(), ins.len());
    if at.checked_add(dn).is_none_or(|end| end > chars.len()) || chars[at..at + dn] != del[..] {
        return Err(ApplyError::Stale { step: n });
    }
    if ins.iter().filter(|&&ch| ch == '\n').count() != s.blocks.len() {
        return Err(ApplyError::BadStep { step: n, reason: "`blocks` holds one record per inserted `\\n`" });
    }
    if ins.iter().filter(|&&ch| ch == ISLAND_SLOT).count() != s.islands.len() {
        return Err(ApplyError::BadStep { step: n, reason: "`islands` holds one island per inserted slot" });
    }
    for mk in &s.marks {
        if mk.start >= mk.end || mk.end > m {
            return Err(ApplyError::BadStep { step: n, reason: "a mark of the insert is outside it" });
        }
    }
    let first_block = chars[..at].iter().filter(|&&ch| ch == '\n').count();
    let first_island = chars[..at].iter().filter(|&&ch| ch == ISLAND_SLOT).count();
    let lost_blocks = del.iter().filter(|&&ch| ch == '\n').count();
    let lost_islands = del.iter().filter(|&&ch| ch == ISLAND_SLOT).count();

    let removed = Removed {
        blocks: c.blocks[first_block..first_block + lost_blocks].to_vec(),
        marks: c
            .marks
            .iter()
            .filter_map(|mk| {
                let (lo, hi) = (mk.start.max(at), mk.end.min(at + dn));
                (lo < hi).then(|| Mark::new(lo - at, hi - at, mk.kind.clone()))
            })
            .collect(),
        islands: c.islands[first_island..first_island + lost_islands].to_vec(),
    };

    let mut text = String::with_capacity(c.text.len() + s.insert.len());
    text.extend(&chars[..at]);
    text.push_str(&s.insert);
    text.extend(&chars[at + dn..]);
    c.text = text;
    c.blocks.splice(first_block..first_block + lost_blocks, s.blocks.iter().cloned());
    let mut islands = s.islands.clone();
    for island in &mut islands {
        table::normalize_island(island);
        for slot in island.slots.values_mut() {
            slot.normalize();
        }
    }
    c.islands.splice(first_island..first_island + lost_islands, islands.iter().cloned());

    let mut marks = Vec::with_capacity(c.marks.len() + s.marks.len() + 1);
    for mk in &c.marks {
        if mk.start < at {
            marks.push(Mark::new(mk.start, mk.end.min(at), mk.kind.clone()));
        }
        if mk.end > at + dn {
            marks.push(Mark::new(mk.start.max(at + dn) - dn + m, mk.end - dn + m, mk.kind.clone()));
        }
    }
    marks.extend(s.marks.iter().map(|mk| Mark::new(mk.start + at, mk.end + at, mk.kind.clone())));
    c.marks = marks;
    c.normalize_marks();

    c.anchors.retain_mut(|a| {
        let point = a.is_point();
        let (start, end) = (map_start(a.start, at, dn, m, point), map_end(a.end, at, dn, m, point));
        if !point && start >= end {
            dropped.push(a.id.clone());
            return false;
        }
        a.start = start;
        a.end = end;
        true
    });
    c.sort_anchors();

    Ok(Splice {
        at,
        delete: s.delete.clone(),
        insert: s.insert.clone(),
        marks: inserted_marks(c, at, m),
        blocks: s.blocks.clone(),
        islands,
        removed: Some(removed),
    })
}

/// The marks over `[at, at + m)`, as offsets into it.
fn inserted_marks(c: &Content, at: Usv, m: Usv) -> Vec<Mark> {
    c.marks
        .iter()
        .filter_map(|mk| {
            let (lo, hi) = (mk.start.max(at), mk.end.min(at + m));
            (lo < hi).then(|| Mark::new(lo - at, hi - at, mk.kind.clone()))
        })
        .collect()
}

/// A range anchor's start binds to the character after it; a point binds
/// left, and collapses to the deletion's start when its text goes.
fn map_start(p: Usv, at: Usv, dn: Usv, m: Usv, point: bool) -> Usv {
    if point {
        return map_point(p, at, dn, m);
    }
    if p < at { p } else { (p.saturating_sub(dn)).max(at) + m }
}

/// A range anchor's end binds to the character before it.
fn map_end(p: Usv, at: Usv, dn: Usv, m: Usv, point: bool) -> Usv {
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

fn map_point(p: Usv, at: Usv, dn: Usv, m: Usv) -> Usv {
    if p <= at {
        p
    } else if p < at + dn {
        at
    } else {
        p - dn + m
    }
}

fn set_block(c: &mut Content, s: &SetBlock, n: usize) -> Result<SetBlock, ApplyError> {
    let b = c
        .blocks
        .iter_mut()
        .find(|b| b.id == s.id)
        .ok_or_else(|| ApplyError::UnknownBlock { id: s.id.clone() })?;
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
    let attrs = crate::wire::block_attrs(&b.kind).and_then(|v| v.as_object().cloned());
    let kind = (s.kind.is_some() || s.attrs.is_some()).then(|| b.kind.tag().to_string());
    Ok(SetBlock { id: s.id.clone(), attrs: kind.as_ref().and(attrs), kind, path: s.path.clone() })
}

fn container(c: &mut Content, s: &ContainerStep) -> Result<(), ApplyError> {
    match s {
        ContainerStep::Set(k) => match c.containers.iter_mut().find(|h| h.id == k.id) {
            Some(held) if !held.kind.same_kind(&k.kind) => return Err(ApplyError::ContainerKind { id: k.id.clone() }),
            Some(held) => held.kind = k.kind.clone(),
            None => c.containers.push(k.clone()),
        },
        ContainerStep::Remove { id } => {
            if c.blocks.iter().any(|b| b.path.contains(id)) {
                return Err(ApplyError::ContainerInUse { id: id.clone() });
            }
            let at = c
                .containers
                .iter()
                .position(|h| &h.id == id)
                .ok_or_else(|| ApplyError::UnknownContainer { id: id.clone() })?;
            c.containers.remove(at);
        }
    }
    c.order_containers(false);
    Ok(())
}

fn mark(c: &mut Content, s: &MarkStep, n: usize) -> Result<(), ApplyError> {
    let len = c.len_usv();
    let (start, end, tag) = match s {
        MarkStep::Add(mk) => (mk.start, mk.end, mk.kind.tag()),
        MarkStep::Remove { start, end, tag } => (*start, *end, *tag),
    };
    if start >= end || end > len {
        return Err(ApplyError::BadStep { step: n, reason: "a mark step's range is empty or past the text" });
    }
    let remove = matches!(s, MarkStep::Remove { .. }) || matches!(s, MarkStep::Add(mk) if mk.kind.is_keyed());
    if remove {
        clear(&mut c.marks, start, end, tag);
    }
    if let MarkStep::Add(mk) = s {
        c.marks.push(mk.clone());
    }
    c.normalize_marks();
    Ok(())
}

fn clear(marks: &mut Vec<Mark>, start: Usv, end: Usv, tag: &str) {
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

fn anchor(c: &mut Content, s: &AnchorStep, n: usize) -> Result<(), ApplyError> {
    match s {
        AnchorStep::Set(a) => {
            if a.id.is_empty() || a.start > a.end || a.end > c.len_usv() {
                return Err(ApplyError::BadStep { step: n, reason: "an anchor step's range is past the text" });
            }
            c.anchors.retain(|h| h.id != a.id);
            c.anchors.push(a.clone());
            c.sort_anchors();
        }
        AnchorStep::Remove { id } => {
            let at = c.anchors.iter().position(|h| &h.id == id).ok_or_else(|| ApplyError::UnknownAnchor { id: id.clone() })?;
            c.anchors.remove(at);
        }
    }
    Ok(())
}

/// Sets an island's props, and returns the content of each slot it gained.
fn island_props(
    c: &mut Content,
    id: &str,
    props: &Value,
    given: &BTreeMap<String, Content>,
    n: usize,
) -> Result<BTreeMap<String, Content>, ApplyError> {
    let island = c.islands.iter_mut().find(|i| i.id == id).ok_or_else(|| ApplyError::UnknownIsland { id: id.into() })?;
    if !props.is_object() {
        return Err(ApplyError::BadStep { step: n, reason: "`props` is not an object" });
    }
    island.props = props.clone();
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
                fresh.normalize();
                gained.insert(slot.clone(), fresh.clone());
                fresh
            }
        };
        slots.insert(slot, content);
    }
    if given.keys().any(|k| !gained.contains_key(k)) {
        return Err(ApplyError::BadStep { step: n, reason: "`slots` names a slot the island already holds" });
    }
    island.slots = slots;
    table::normalize_island(island);
    Ok(gained)
}

/// Each range anchor the change kept whose text it changed.
fn moved_anchors(before: &Content, after: &Content) -> Vec<Moved> {
    let mut was = HashMap::new();
    quotes(before, &mut was);
    let mut now = HashMap::new();
    quotes(after, &mut now);
    let mut out: Vec<Moved> = now
        .into_iter()
        .filter_map(|(id, after)| {
            let before = was.remove(&id)?;
            (before != after && !before.is_empty()).then_some(Moved { id, before, after })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn quotes(c: &Content, out: &mut HashMap<String, String>) {
    let chars: Vec<char> = c.text.chars().collect();
    for a in &c.anchors {
        out.insert(a.id.clone(), chars[a.start..a.end].iter().collect());
    }
    for island in &c.islands {
        for slot in island.slots.values() {
            quotes(slot, out);
        }
    }
}

/// The change that undoes `change` on the value it applies to.
pub fn invert(change: &Change, before: &Content) -> Result<Change, ApplyError> {
    let normalized = apply(before, change)?.applied;
    let mut groups = Vec::with_capacity(normalized.steps.len());
    invert_level(before, &normalized.steps, &mut groups)?;
    Ok(Change { steps: groups.into_iter().rev().flatten().collect() })
}

fn invert_level(before: &Content, steps: &[Step], groups: &mut Vec<Vec<Step>>) -> Result<(), ApplyError> {
    let mut doc = before.clone();
    for (n, step) in steps.iter().enumerate() {
        let mut next = doc.clone();
        apply_step(&mut next, step, n, &mut Vec::new())?;
        let mut undo = undo_step(&doc, &next, step)?;
        let mut back = next.clone();
        for (k, s) in undo.iter().enumerate() {
            apply_step(&mut back, s, k, &mut Vec::new())?;
        }
        undo.extend(fixups(&back, &doc));
        groups.push(undo);
        doc = next;
    }
    Ok(())
}

/// The steps that take `after` back to `before` across one `step`, before
/// the mark and anchor fix-ups.
fn undo_step(before: &Content, after: &Content, step: &Step) -> Result<Vec<Step>, ApplyError> {
    Ok(match step {
        Step::Splice(s) => {
            let removed = s.removed.clone().unwrap_or_default();
            vec![Step::Splice(Splice {
                at: s.at,
                delete: s.insert.clone(),
                insert: s.delete.clone(),
                marks: removed.marks,
                blocks: removed.blocks,
                islands: removed.islands,
                removed: None,
            })]
        }
        Step::SetBlock(s) => {
            let b = before.blocks.iter().find(|b| b.id == s.id).ok_or_else(|| ApplyError::UnknownBlock { id: s.id.clone() })?;
            vec![Step::SetBlock(SetBlock {
                id: s.id.clone(),
                kind: Some(b.kind.tag().to_string()),
                attrs: crate::wire::block_attrs(&b.kind).and_then(|v| v.as_object().cloned()),
                path: Some(b.path.clone()),
            })]
        }
        Step::Container(ContainerStep::Set(k)) => match before.container(&k.id) {
            Some(held) => vec![Step::Container(ContainerStep::Set(held.clone()))],
            None => vec![Step::Container(ContainerStep::Remove { id: k.id.clone() })],
        },
        Step::Container(ContainerStep::Remove { id }) => {
            let held = before.container(id).ok_or_else(|| ApplyError::UnknownContainer { id: id.clone() })?;
            vec![Step::Container(ContainerStep::Set(held.clone()))]
        }
        Step::Mark(_) => Vec::new(),
        Step::Anchor(AnchorStep::Set(a)) => match before.anchor(&a.id) {
            Some(held) => vec![Step::Anchor(AnchorStep::Set(held.clone()))],
            None => vec![Step::Anchor(AnchorStep::Remove { id: a.id.clone() })],
        },
        Step::Anchor(AnchorStep::Remove { id }) => {
            let held = before.anchor(id).ok_or_else(|| ApplyError::UnknownAnchor { id: id.clone() })?;
            vec![Step::Anchor(AnchorStep::Set(held.clone()))]
        }
        Step::Island(IslandStep::Props { id, .. }) => {
            let held = before.island(id).ok_or_else(|| ApplyError::UnknownIsland { id: id.clone() })?;
            let now = after.island(id).ok_or_else(|| ApplyError::UnknownIsland { id: id.clone() })?;
            let slots = held.slots.iter().filter(|(k, _)| !now.slots.contains_key(*k)).map(|(k, v)| (k.clone(), v.clone())).collect();
            vec![Step::Island(IslandStep::Props { id: id.clone(), props: held.props.clone(), slots })]
        }
        Step::Island(IslandStep::Insert { at, .. }) => {
            vec![Step::Splice(Splice::new(*at, ISLAND_SLOT.to_string(), ""))]
        }
        Step::Nested { island, slot, change } => {
            let inner = before
                .slot(island, slot)
                .ok_or_else(|| ApplyError::UnknownSlot { island: island.clone(), slot: slot.clone() })?;
            let mut groups = Vec::new();
            invert_level(inner, &change.steps, &mut groups)?;
            let steps: Vec<Step> = groups.into_iter().rev().flatten().collect();
            vec![Step::Nested { island: island.clone(), slot: slot.clone(), change: Change { steps } }]
        }
    })
}

/// The mark and anchor steps that take `now` to `want` on one level, where
/// the two differ in nothing else.
pub(crate) fn fixups(now: &Content, want: &Content) -> Vec<Step> {
    let mut out = Vec::new();
    let tags: BTreeSet<&'static str> = now.marks.iter().chain(&want.marks).map(|m| m.kind.tag()).collect();
    for tag in tags {
        let have: Vec<&Mark> = now.marks.iter().filter(|m| m.kind.tag() == tag).collect();
        let need: Vec<&Mark> = want.marks.iter().filter(|m| m.kind.tag() == tag).collect();
        let differ: Vec<&&Mark> =
            have.iter().filter(|m| !need.contains(m)).chain(need.iter().filter(|m| !have.contains(m))).collect();
        let (Some(lo), Some(hi)) = (differ.iter().map(|m| m.start).min(), differ.iter().map(|m| m.end).max()) else {
            continue;
        };
        out.push(Step::Mark(MarkStep::Remove { start: lo, end: hi, tag }));
        for m in need {
            let (s, e) = (m.start.max(lo), m.end.min(hi));
            if s < e {
                out.push(Step::Mark(MarkStep::Add(Mark::new(s, e, m.kind.clone()))));
            }
        }
    }
    for a in &want.anchors {
        if now.anchor(&a.id) != Some(a) {
            out.push(Step::Anchor(AnchorStep::Set(a.clone())));
        }
    }
    for a in &now.anchors {
        if want.anchor(&a.id).is_none() {
            out.push(Step::Anchor(AnchorStep::Remove { id: a.id.clone() }));
        }
    }
    out
}

/// `a` then `b` as one change: the steps of both, adjacent splices merged
/// where the second lies within the text the first inserted or both only
/// delete and touch, a merge that undoes itself dropped, and adjacent nested
/// changes to one slot composed. A splice typed where the one before it
/// deleted stays apart, since a point anchor there stays left of the first
/// and would sit right of the merged one.
pub fn compose(a: &Change, b: &Change) -> Change {
    let mut steps: Vec<Step> = Vec::with_capacity(a.steps.len() + b.steps.len());
    for step in a.steps.iter().chain(&b.steps) {
        let merged = match (steps.last(), step) {
            (Some(Step::Splice(x)), Step::Splice(y)) => merge_splices(x, y).map(Step::Splice),
            (
                Some(Step::Nested { island: i, slot: s, change: x }),
                Step::Nested { island: j, slot: t, change: y },
            ) if i == j && s == t => Some(Step::Nested { island: i.clone(), slot: s.clone(), change: compose(x, y) }),
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

fn merge_splices(x: &Splice, y: &Splice) -> Option<Splice> {
    let xi: Vec<char> = x.insert.chars().collect();
    let yd: Vec<char> = y.delete.chars().collect();
    let (xm, yn) = (xi.len(), yd.len());
    let point_moves = !x.delete.is_empty() && !y.insert.is_empty() && y.at == x.at + xm;
    if y.at >= x.at && y.at + yn <= x.at + xm && !point_moves {
        let r = y.at - x.at;
        if xi[r..r + yn] != yd[..] {
            return None;
        }
        let mut insert: String = xi[..r].iter().collect();
        insert.push_str(&y.insert);
        insert.extend(&xi[r + yn..]);
        let ym = y.insert.chars().count();
        let count = |chars: &[char], ch: char| chars.iter().filter(|&&c| c == ch).count();
        let (nl_before, nl_gone) = (count(&xi[..r], '\n'), count(&yd, '\n'));
        let (sl_before, sl_gone) = (count(&xi[..r], ISLAND_SLOT), count(&yd, ISLAND_SLOT));
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
            let shift = first.delete.chars().count();
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

/// Where `pos` lands after `change`, or `None` when the change deleted it. A
/// position at the edge of an insertion keeps to the `assoc` side; one inside
/// a deleted island reads its island from the splice's `removed`.
pub fn map_pos(change: &Change, pos: &Pos, assoc: Assoc) -> Option<Pos> {
    let mut pos = pos.clone();
    for step in &change.steps {
        pos = map_step(step, pos, assoc)?;
    }
    Some(pos)
}

fn map_step(step: &Step, pos: Pos, assoc: Assoc) -> Option<Pos> {
    match (step, pos) {
        (Step::Splice(s), Pos::At(p)) => {
            map_offset(p, s.at, s.delete.chars().count(), s.insert.chars().count(), assoc).map(Pos::At)
        }
        (Step::Island(IslandStep::Insert { at, .. }), Pos::At(p)) => map_offset(p, *at, 0, 1, assoc).map(Pos::At),
        (Step::Splice(s), pos @ Pos::In { .. }) => {
            let Pos::In { island, .. } = &pos else { unreachable!() };
            let gone = s.removed.as_ref().is_some_and(|r| r.islands.iter().any(|i| &i.id == island));
            (!gone).then_some(pos)
        }
        (Step::Island(IslandStep::Props { id, props, .. }), Pos::In { island, slot, at }) if *id == island => {
            let keeps = match Table::read(props) {
                Some(t) => t.slot_order().contains(&slot),
                None => true,
            };
            keeps.then_some(Pos::In { island, slot, at })
        }
        (Step::Nested { island: i, slot: s, change }, Pos::In { island, slot, at }) if *i == island && *s == slot => {
            let at = map_pos(change, &at, assoc)?;
            Some(Pos::In { island, slot, at: Box::new(at) })
        }
        (_, pos) => Some(pos),
    }
}

fn map_offset(p: Usv, at: Usv, dn: Usv, m: Usv, assoc: Assoc) -> Option<Usv> {
    if p < at {
        Some(p)
    } else if p > at + dn {
        Some(p - dn + m)
    } else if p > at && p < at + dn {
        None
    } else {
        Some(match assoc {
            Assoc::Before => at,
            Assoc::After => at + m,
        })
    }
}

mod wire {
    //! A change's JSON: `{ "steps": [...] }`, each step an object of one key
    //! naming it, positions in Unicode scalar values.

    use super::*;
    use crate::wire::{
        ParseError, anchor_from_value, anchor_value, array, attrs, block_from_value, block_value, container_kind_from,
        container_value, content_value, decode, island_from_value, island_value, mark_from_value, mark_kind_from,
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
            if crate::model::json_too_deep(v) {
                return Err(ParseError::Invalid(Invariant::JsonTooDeep { what: "change", max: crate::MAX_JSON_DEPTH }));
            }
            change_from(v)
        }

        pub fn from_json(s: &str) -> Result<Change, ParseError> {
            let v: Value = serde_json::from_str(s).map_err(|e| ParseError::Json(e.to_string()))?;
            Change::from_value(&v)
        }
    }

    fn change_from(v: &Value) -> Result<Change, ParseError> {
        match v.get("steps") {
            Some(Value::Array(a)) => Ok(Change { steps: a.iter().map(step_from).collect::<Result<_, _>>()? }),
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
            Step::Mark(MarkStep::Add(mk)) => {
                let mut v = mark_value(mk);
                v["add"] = true.into();
                one("mark", v)
            }
            Step::Mark(MarkStep::Remove { start, end, tag }) => one(
                "mark",
                obj([("start", (*start).into()), ("end", (*end).into()), ("type", (*tag).into()), ("add", false.into())]),
            ),
            Step::Anchor(AnchorStep::Set(a)) => one("anchor", anchor_value(a)),
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
            Step::Island(IslandStep::Insert { at, island }) => {
                one("island", obj([("at", (*at).into()), ("island", island_value(island))]))
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

    fn removing(v: &Value) -> Result<bool, ParseError> {
        match v.get("remove") {
            None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(false),
            Some(Value::Bool(true)) => Ok(true),
            Some(_) => Err(shape("`remove` is not a boolean")),
        }
    }

    fn slots_from(v: &Value) -> Result<BTreeMap<String, Content>, ParseError> {
        match v.get("slots") {
            None | Some(Value::Null) => Ok(BTreeMap::new()),
            Some(Value::Object(m)) => m.iter().map(|(k, s)| Ok((k.clone(), decode(s)?))).collect(),
            Some(_) => Err(shape("`slots` is not an object")),
        }
    }

    fn step_from(v: &Value) -> Result<Step, ParseError> {
        let Some(m) = v.as_object().filter(|m| m.len() == 1) else {
            return Err(shape("a step is an object of one key"));
        };
        let (name, body) = m.iter().next().expect("one key");
        if !body.is_object() {
            return Err(shape(format!("`{name}` is not an object")));
        }
        Ok(match name.as_str() {
            "splice" => {
                let removed = match body.get("removed") {
                    None | Some(Value::Null) => None,
                    Some(r @ Value::Object(_)) => Some(Removed {
                        blocks: array(r, "blocks")?.iter().map(block_from_value).collect::<Result<_, _>>()?,
                        marks: array(r, "marks")?.iter().map(mark_from_value).collect::<Result<_, _>>()?,
                        islands: array(r, "islands")?.iter().map(island_from_value).collect::<Result<_, _>>()?,
                    }),
                    Some(_) => return Err(shape("`removed` is not an object")),
                };
                let insert = str_key(body, "insert")?;
                let len = insert.chars().count();
                let marks = array(body, "marks")?
                    .iter()
                    .map(|mv| {
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
            "setBlock" => Step::SetBlock(SetBlock {
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
            }),
            "container" => {
                let id = str_key(body, "id")?;
                if removing(body)? {
                    Step::Container(ContainerStep::Remove { id })
                } else {
                    let kind = container_kind_from(&str_key(body, "kind")?, &attrs(body)?)?;
                    Step::Container(ContainerStep::Set(Container { id, kind }))
                }
            }
            "mark" => {
                let add = body.get("add").and_then(Value::as_bool).ok_or_else(|| shape("`add` is not a boolean"))?;
                if add {
                    Step::Mark(MarkStep::Add(mark_from_value(body)?))
                } else {
                    let ty = str_key(body, "type")?;
                    let tag = MARK_TAGS
                        .into_iter()
                        .find(|t| *t == ty)
                        .ok_or(ParseError::UnknownName { axis: "mark type", name: ty })?;
                    Step::Mark(MarkStep::Remove { start: usize_key(body, "start")?, end: usize_key(body, "end")?, tag })
                }
            }
            "anchor" => {
                if removing(body)? {
                    Step::Anchor(AnchorStep::Remove { id: str_key(body, "id")? })
                } else {
                    Step::Anchor(AnchorStep::Set(anchor_from_value(body)?))
                }
            }
            "island" => match body.get("island") {
                Some(island) => {
                    Step::Island(IslandStep::Insert { at: usize_key(body, "at")?, island: island_from_value(island)? })
                }
                None => Step::Island(IslandStep::Props {
                    id: str_key(body, "id")?,
                    props: body.get("props").cloned().ok_or_else(|| shape("an island step names `props`"))?,
                    slots: slots_from(body)?,
                }),
            },
            "nested" => Step::Nested {
                island: str_key(body, "island")?,
                slot: str_key(body, "slot")?,
                change: change_from(body.get("change").ok_or_else(|| shape("a nested step names `change`"))?)?,
            },
            other => return Err(ParseError::UnknownName { axis: "step", name: other.to_string() }),
        })
    }
}
