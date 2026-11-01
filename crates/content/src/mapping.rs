//! Positions across changes, after ProseMirror's maps. A [`StepMap`] is what
//! one step does to positions, read off the step alone; a [`Mapping`] is a
//! list of them with mirror links, each pairing a map with its inverse, so a
//! position a step deleted and its mirror put back maps to where it was.
//! [`MapResult`] is where a position lands and what around it went.

use crate::change::{ApplyError, Assoc, Change, IslandStep, Step};
use crate::model::{ISLAND_SLOT, Pos, Usv};
use crate::table::Table;
use crate::units::{Scalars, Units};
use std::collections::BTreeSet;

/// Where a position lands. It always lands: a position inside deleted text
/// lands on the deletion's `assoc` edge, and one inside an island a splice
/// deleted on the island's slot in the text that held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapResult {
    pub pos: Pos,
    /// The character on the `assoc` side of the position went.
    pub deleted: bool,
    /// The character before the position went.
    pub deleted_before: bool,
    /// The character after the position went.
    pub deleted_after: bool,
    /// One deletion took the characters on both sides of the position.
    pub deleted_across: bool,
    recover: Option<Recover>,
}

/// What a map's mirror needs to put a position back where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Recover {
    /// The offset into the text a replacement deleted.
    Offset(Usv),
    /// The position as it was, inside an island or slot the step removed.
    Pos(Pos),
    /// Inside a slot: the recovery at map `index` of its mapping.
    Nested { index: usize, inner: Box<Recover> },
}

/// A set of a table's slots: these ones, or every one but these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slots {
    Only(BTreeSet<String>),
    AllBut(BTreeSet<String>),
}

impl Slots {
    fn contains(&self, slot: &str) -> bool {
        match self {
            Slots::Only(s) => s.contains(slot),
            Slots::AllBut(s) => !s.contains(slot),
        }
    }
}

/// What one step does to positions.
#[derive(Debug, Clone, PartialEq)]
pub enum StepMap {
    /// A step that moves no text and removes no slot.
    Identity,
    /// `deleted` units at `at` replaced by `inserted`, removing the islands
    /// `removed` and adding `added`, by id.
    Replace { at: Usv, deleted: usize, inserted: usize, removed: Vec<String>, added: Vec<String> },
    /// The slots of `island` a props step removed and added.
    Slots { island: String, removed: Slots, added: Slots },
    /// A change inside one slot.
    Nested { island: String, slot: String, mapping: Mapping },
}

const UNNAMED: &str = "a splice that deletes an island names it in `removed`: map the normalized change";

impl StepMap {
    /// The map of `step`, its offsets counting `u`. A splice that deletes an
    /// island and names none in `removed` is refused: which island went is
    /// not in the step, and the normalized change (`Applied.applied`) says.
    pub fn of(step: &Step, u: impl Units) -> Result<StepMap, ApplyError> {
        let unnamed = ApplyError::BadStep { step: 0, reason: UNNAMED };
        Ok(match step {
            Step::Splice(s) => {
                let slots = s.delete.chars().filter(|&c| c == ISLAND_SLOT).count();
                let removed = match &s.removed {
                    _ if slots == 0 => Vec::new(),
                    Some(r) if r.islands.len() == slots => r.islands.iter().map(|i| i.id.clone()).collect(),
                    _ => return Err(unnamed),
                };
                StepMap::Replace {
                    at: s.at,
                    deleted: u.len(&s.delete),
                    inserted: u.len(&s.insert),
                    removed,
                    added: s.islands.iter().map(|i| i.id.clone()).collect(),
                }
            }
            Step::Island(IslandStep::Insert { at, island, .. }) => StepMap::Replace {
                at: *at,
                deleted: 0,
                inserted: u.char_len(ISLAND_SLOT),
                removed: Vec::new(),
                added: vec![island.id.clone()],
            },
            Step::Island(IslandStep::Props { id, props, slots }) => StepMap::Slots {
                island: id.clone(),
                removed: match Table::read(props) {
                    Some(t) => Slots::AllBut(t.slot_order().into_iter().collect()),
                    None => Slots::Only(BTreeSet::new()),
                },
                added: Slots::Only(slots.keys().cloned().collect()),
            },
            Step::Nested { island, slot, change } => StepMap::Nested {
                island: island.clone(),
                slot: slot.clone(),
                mapping: Mapping::from_change_in(change, u)?,
            },
            Step::SetBlock(_) | Step::Container(_) | Step::Mark(_) | Step::Anchor(_) => StepMap::Identity,
        })
    }

    /// The map that undoes this one.
    pub fn invert(&self) -> StepMap {
        match self {
            StepMap::Identity => StepMap::Identity,
            StepMap::Replace { at, deleted, inserted, removed, added } => StepMap::Replace {
                at: *at,
                deleted: *inserted,
                inserted: *deleted,
                removed: added.clone(),
                added: removed.clone(),
            },
            StepMap::Slots { island, removed, added } => {
                StepMap::Slots { island: island.clone(), removed: added.clone(), added: removed.clone() }
            }
            StepMap::Nested { island, slot, mapping } => {
                StepMap::Nested { island: island.clone(), slot: slot.clone(), mapping: mapping.invert() }
            }
        }
    }

    pub fn map(&self, pos: &Pos, assoc: Assoc) -> MapResult {
        match (self, pos) {
            (StepMap::Replace { at, deleted, inserted, .. }, Pos::At(p)) => replace(*p, *at, *deleted, *inserted, assoc),
            (StepMap::Replace { at, inserted, removed, .. }, Pos::In { island, .. }) if removed.contains(island) => {
                let edge = match assoc {
                    Assoc::Before => *at,
                    Assoc::After => at.saturating_add(*inserted),
                };
                gone(Pos::At(edge), pos)
            }
            (StepMap::Slots { island: id, removed, .. }, Pos::In { island, slot, .. })
                if id == island && removed.contains(slot) =>
            {
                gone(pos.clone(), pos)
            }
            (StepMap::Nested { island: i, slot: s, mapping }, Pos::In { island, slot, at }) if i == island && s == slot => {
                let (r, recover) = mapping.map_range(0, mapping.maps.len(), at, assoc);
                MapResult {
                    pos: Pos::In { island: island.clone(), slot: slot.clone(), at: Box::new(r.pos) },
                    recover: recover.map(|(index, inner)| Recover::Nested { index, inner: Box::new(inner) }),
                    ..r
                }
            }
            _ => kept(pos.clone()),
        }
    }

    /// Where `recover`, read off the map this one mirrors, puts a position
    /// back; `None` where the two do not pair.
    fn recover(&self, recover: &Recover, assoc: Assoc) -> Option<Pos> {
        match (self, recover) {
            (_, Recover::Pos(p)) => Some(p.clone()),
            (StepMap::Replace { at, .. }, Recover::Offset(off)) => Some(Pos::At(at + off)),
            (StepMap::Nested { island, slot, mapping }, Recover::Nested { index, inner }) => {
                let n = mapping.maps.len();
                let k = n.checked_sub(index + 1)?;
                let at = mapping.maps[k].recover(inner, assoc)?;
                let (r, _) = mapping.map_range(k + 1, n, &at, assoc);
                Some(Pos::In { island: island.clone(), slot: slot.clone(), at: Box::new(r.pos) })
            }
            _ => None,
        }
    }
}

fn kept(pos: Pos) -> MapResult {
    MapResult { pos, deleted: false, deleted_before: false, deleted_after: false, deleted_across: false, recover: None }
}

fn gone(pos: Pos, was: &Pos) -> MapResult {
    MapResult {
        pos,
        deleted: true,
        deleted_before: true,
        deleted_after: true,
        deleted_across: true,
        recover: Some(Recover::Pos(was.clone())),
    }
}

/// Offset `p` across `d` units at `at` replaced by `m`. A position at either
/// edge of the replaced text, or inside it, keeps to the `assoc` side.
fn replace(p: Usv, at: Usv, d: usize, m: usize, assoc: Assoc) -> MapResult {
    let end = at.saturating_add(d);
    if p < at {
        return kept(Pos::At(p));
    }
    if p > end {
        return kept(Pos::At((p - d).saturating_add(m)));
    }
    let edge = match assoc {
        Assoc::Before => at,
        Assoc::After => at.saturating_add(m),
    };
    if d == 0 {
        return kept(Pos::At(edge));
    }
    let (before, after) = (p > at, p < end);
    let (deleted, recover) = match assoc {
        Assoc::Before => (before, p != at),
        Assoc::After => (after, p != end),
    };
    MapResult {
        pos: Pos::At(edge),
        deleted,
        deleted_before: before,
        deleted_after: after,
        deleted_across: before && after,
        recover: recover.then(|| Recover::Offset(p - at)),
    }
}

/// A list of step maps, a position mapped through each in turn. A mirror
/// link pairs map `a` with a later `b` that undoes it: a position `a` deleted
/// skips to `b`, which puts it back, and maps on from there. A rebase builds
/// one as the inverse of a change's own earlier steps, then the change it
/// rebases over, then those steps rebased, each linked to its inverse. A
/// mirror of a nested map pairs their inner maps from the two ends, inner map
/// `t` with the mirror's `n - 1 - t`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mapping {
    maps: Vec<StepMap>,
    mirror: Vec<(usize, usize)>,
}

impl Mapping {
    pub fn new() -> Self {
        Mapping::default()
    }

    /// The map of each step of `change`, in order: [`StepMap::of`] each.
    pub fn from_change(change: &Change) -> Result<Mapping, ApplyError> {
        Mapping::from_change_in(change, Scalars)
    }

    /// [`from_change`](Self::from_change), offsets counting `u`.
    pub fn from_change_in(change: &Change, u: impl Units) -> Result<Mapping, ApplyError> {
        let maps = change
            .steps
            .iter()
            .enumerate()
            .map(|(n, s)| {
                StepMap::of(s, u).map_err(|e| match e {
                    ApplyError::BadStep { reason, .. } => ApplyError::BadStep { step: n, reason },
                    e => e,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Mapping { maps, mirror: Vec::new() })
    }

    pub fn maps(&self) -> &[StepMap] {
        &self.maps
    }

    pub fn len(&self) -> usize {
        self.maps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    /// Adds `map`, linked to the map at `mirror` when given.
    pub fn append_map(&mut self, map: StepMap, mirror: Option<usize>) {
        self.maps.push(map);
        if let Some(m) = mirror {
            self.set_mirror(self.maps.len() - 1, m);
        }
    }

    /// Adds every map of `other`, with its links.
    pub fn append_mapping(&mut self, other: &Mapping) {
        let start = self.maps.len();
        for (i, map) in other.maps.iter().enumerate() {
            let mirror = other.get_mirror(i).filter(|&m| m < i).map(|m| start + m);
            self.append_map(map.clone(), mirror);
        }
    }

    /// Adds the inverse of every map of `other`, last first, with its links.
    pub fn append_mapping_inverted(&mut self, other: &Mapping) {
        let total = self.maps.len() + other.maps.len();
        for i in (0..other.maps.len()).rev() {
            let mirror = other.get_mirror(i).filter(|&m| m > i).map(|m| total - m - 1);
            self.append_map(other.maps[i].invert(), mirror);
        }
    }

    /// The mapping that undoes this one.
    pub fn invert(&self) -> Mapping {
        let mut out = Mapping::new();
        out.append_mapping_inverted(self);
        out
    }

    /// Links maps `a` and `b` as each other's mirror.
    pub fn set_mirror(&mut self, a: usize, b: usize) {
        self.mirror.push((a, b));
    }

    pub fn get_mirror(&self, n: usize) -> Option<usize> {
        self.mirror.iter().find_map(|&(a, b)| match () {
            _ if a == n => Some(b),
            _ if b == n => Some(a),
            _ => None,
        })
    }

    /// Maps `from..to`, with the links between them.
    pub fn slice(&self, from: usize, to: usize) -> Mapping {
        let to = to.min(self.maps.len());
        let from = from.min(to);
        let inside = |n: usize| (from..to).contains(&n);
        Mapping {
            maps: self.maps[from..to].to_vec(),
            mirror: self.mirror.iter().filter(|(a, b)| inside(*a) && inside(*b)).map(|(a, b)| (a - from, b - from)).collect(),
        }
    }

    /// `pos` through every map, the deletion flags of each map it passed.
    pub fn map(&self, pos: &Pos, assoc: Assoc) -> MapResult {
        self.map_range(0, self.maps.len(), pos, assoc).0
    }

    /// `pos` through maps `from..to`, and the first recovery no link in the
    /// range took, with the index of its map.
    fn map_range(&self, from: usize, to: usize, pos: &Pos, assoc: Assoc) -> (MapResult, Option<(usize, Recover)>) {
        let mut out = kept(pos.clone());
        let mut open = None;
        let mut i = from;
        while i < to {
            let r = self.maps[i].map(&out.pos, assoc);
            if let Some(recover) = &r.recover {
                let back = self.get_mirror(i).filter(|&j| j > i && j < to).and_then(|j| Some((j, self.maps[j].recover(recover, assoc)?)));
                if let Some((j, p)) = back {
                    out.pos = p;
                    i = j + 1;
                    continue;
                }
                open.get_or_insert((i, recover.clone()));
            }
            out = MapResult {
                pos: r.pos,
                deleted: out.deleted || r.deleted,
                deleted_before: out.deleted_before || r.deleted_before,
                deleted_after: out.deleted_after || r.deleted_after,
                deleted_across: out.deleted_across || r.deleted_across,
                recover: None,
            };
            i += 1;
        }
        (out, open)
    }
}
