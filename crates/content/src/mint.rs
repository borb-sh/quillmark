//! The ids a write mints for the records its change puts in the value.
//! [`fill`] gives each [placeholder](crate::ids::is_placeholder) an id past
//! every id the values it is given and the change hold, so a change filled
//! against the value it lands on never takes an id it holds.

use crate::change::{ApplyError, Change, ContainerStep, IslandStep, Step};
use crate::ids::{Minter, is_placeholder, prefix};
use crate::model::Content;
use std::collections::BTreeMap;

/// A change with its placeholders filled.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filled {
    pub change: Change,
    /// Each placeholder and the id it took.
    pub minted: BTreeMap<String, String>,
}

/// The kind of part an id names: each kind holds its ids apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Block,
    Container,
    Island,
}

/// Where an id sits: the value's own records, or one slot's.
type Level = Option<(String, String)>;

/// What a step does with an id: puts a record under it, minting under the
/// prefix where it is to be minted, or names one.
#[derive(Clone, Copy)]
enum Role {
    Puts(&'static str),
    Names,
}

/// `change` with each placeholder replaced by an id minted past the ids of
/// `held` and of the change itself, in the order the change puts them. One placeholder names one
/// record: putting it twice, or naming one no step puts, is
/// [`ApplyError::BadStep`].
pub fn fill(change: &Change, held: &[&Content]) -> Result<Filled, ApplyError> {
    let mut out = change.clone();
    let mut put: Vec<(usize, String, &'static str)> = Vec::new();
    let mut named: Vec<(usize, String)> = Vec::new();
    visit(&mut out.steps, &None, &mut |n, _, _, role, id| {
        if is_placeholder(id) {
            match role {
                Role::Puts(p) => put.push((n, id.clone(), p)),
                Role::Names => named.push((n, id.clone())),
            }
        }
    });
    if put.is_empty() && named.is_empty() {
        return Ok(Filled { change: out, minted: BTreeMap::new() });
    }
    let mut ids = minter(&mut out, held);
    let mut minted = BTreeMap::new();
    for (n, id, p) in put {
        if minted.insert(id, ids.mint(p)).is_some() {
            return Err(ApplyError::BadStep { step: n, reason: "a placeholder names two records" });
        }
    }
    if let Some((n, _)) = named.iter().find(|(_, id)| !minted.contains_key(id)) {
        return Err(ApplyError::BadStep { step: *n, reason: "a placeholder no step puts" });
    }
    visit(&mut out.steps, &None, &mut |_, _, _, _, id| {
        if let Some(to) = minted.get(id.as_str()) {
            *id = to.clone();
        }
    });
    Ok(Filled { change: out, minted })
}

/// A minter holding every id of `held` at every level and of `change`.
fn minter(change: &mut Change, held: &[&Content]) -> Minter {
    let mut ids = Minter::default();
    for c in held {
        hold_content(&mut ids, c);
    }
    let mut hold = |_: usize, _: &Level, _: Scope, _: Role, id: &mut String| ids.hold(id.clone());
    visit(&mut change.steps, &None, &mut hold);
    ids
}

fn hold_content(ids: &mut Minter, c: &Content) {
    c.blocks.iter().for_each(|b| ids.hold(&b.id));
    c.containers.iter().for_each(|k| ids.hold(&k.id));
    for island in &c.islands {
        ids.hold(&island.id);
        island.slots.values().for_each(|s| hold_content(ids, s));
    }
}

type Visit<'f> = dyn FnMut(usize, &Level, Scope, Role, &mut String) + 'f;

/// Calls `f` on each block, container and island id `steps` put or name,
/// with the index of the step holding it at the top, the level it sits at and
/// what the step does with it.
fn visit(steps: &mut [Step], level: &Level, f: &mut Visit<'_>) {
    for (n, step) in steps.iter_mut().enumerate() {
        visit_step(n, step, level, f);
    }
}

fn visit_step(n: usize, step: &mut Step, level: &Level, f: &mut Visit<'_>) {
    let path = |f: &mut Visit<'_>, p: &mut Vec<String>| p.iter_mut().for_each(|k| f(n, level, Scope::Container, Role::Names, k));
    match step {
        Step::Splice(s) => {
            for b in &mut s.blocks {
                f(n, level, Scope::Block, Role::Puts(prefix::BLOCK), &mut b.id);
                path(f, &mut b.path);
            }
            for island in &mut s.islands {
                visit_island(n, island, level, f);
            }
        }
        Step::SetBlock(s) => {
            f(n, level, Scope::Block, Role::Names, &mut s.id);
            if let Some(p) = &mut s.path {
                path(f, p);
            }
        }
        Step::Container(ContainerStep::Set(k)) => f(n, level, Scope::Container, Role::Puts(prefix::container(&k.kind)), &mut k.id),
        Step::Container(ContainerStep::Remove { id }) => f(n, level, Scope::Container, Role::Names, id),
        Step::Island(IslandStep::Props { id, slots, .. }) => {
            let at = id.clone();
            f(n, level, Scope::Island, Role::Names, id);
            for (slot, c) in slots {
                visit_content(n, c, &Some((at.clone(), slot.clone())), f);
            }
        }
        Step::Island(IslandStep::Insert { island, block, .. }) => {
            visit_island(n, island, level, f);
            if let Some(b) = block {
                f(n, level, Scope::Block, Role::Names, b);
            }
        }
        Step::Nested { island, slot, change } => {
            let inner = Some((island.clone(), slot.clone()));
            f(n, level, Scope::Island, Role::Names, island);
            for step in &mut change.steps {
                visit_step(n, step, &inner, f);
            }
        }
        Step::Mark(_) | Step::Anchor(_) => {}
    }
}

fn visit_island(n: usize, island: &mut crate::model::Island, level: &Level, f: &mut Visit<'_>) {
    let at = island.id.clone();
    f(n, level, Scope::Island, Role::Puts(prefix::ISLAND), &mut island.id);
    for (slot, c) in &mut island.slots {
        visit_content(n, c, &Some((at.clone(), slot.clone())), f);
    }
}

fn visit_content(n: usize, c: &mut Content, level: &Level, f: &mut Visit<'_>) {
    for k in &mut c.containers {
        f(n, level, Scope::Container, Role::Puts(prefix::container(&k.kind)), &mut k.id);
    }
    for b in &mut c.blocks {
        f(n, level, Scope::Block, Role::Puts(prefix::BLOCK), &mut b.id);
        b.path.iter_mut().for_each(|k| f(n, level, Scope::Container, Role::Names, k));
    }
    for island in &mut c.islands {
        visit_island(n, island, level, f);
    }
}
